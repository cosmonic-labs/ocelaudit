use std::collections::{BTreeMap, HashMap, HashSet};

use crate::model::{CslEntry, EntityType, Hit, Tlp};
use crate::tlp::{band, TlpThresholds};
use crate::tokenize::{normalize, tokenize, trigrams};

const BM25_K1: f32 = 1.2;
const BM25_B: f32 = 0.75;
const RERANK_TOP_K: usize = 200;
/// Weight on Jaro-Winkler similarity vs. BM25 in the final blended score.
/// 0.0 = pure BM25, 1.0 = pure name-similarity. We bias towards name match
/// because the screening use case is dominated by "is this name in the list".
const JW_WEIGHT: f32 = 0.7;

#[derive(Debug, Clone)]
pub struct SearchParams {
    pub q: String,
    pub sources: Option<Vec<String>>,
    pub entity_types: Option<Vec<EntityType>>,
    pub fuzzy: bool,
    pub limit: usize,
}

impl SearchParams {
    pub fn new(q: impl Into<String>) -> Self {
        Self {
            q: q.into(),
            sources: None,
            entity_types: None,
            fuzzy: true,
            limit: 20,
        }
    }
}

/// In-memory search engine over a set of `CslEntry` records.
///
/// The index is built once via [`SearchEngine::build`]; queries are
/// `&self`-immutable so the same engine can serve concurrent reads.
pub struct SearchEngine {
    entries: Vec<CslEntry>,
    /// term -> sorted list of (doc_id, term-frequency-in-doc).
    inverted: HashMap<String, Vec<(u32, u32)>>,
    /// trigram -> set of doc ids that contain it.
    trigram: HashMap<String, Vec<u32>>,
    /// Per-doc total token count across name + aliases (used by BM25).
    doc_lens: Vec<u32>,
    avg_doc_len: f32,
    /// Per-doc set of normalized name+alias surface forms (used for
    /// exact-alias detection and for the JW reranker).
    surfaces: Vec<Vec<String>>,
    /// Lowercased prefix index keyed by the FIRST surface character →
    /// list of (surface, doc_id). Suitable for prefix autocomplete.
    prefix: BTreeMap<char, Vec<(String, u32)>>,
    pub thresholds: TlpThresholds,
}

impl SearchEngine {
    pub fn build(entries: Vec<CslEntry>) -> Self {
        let n = entries.len();
        let mut inverted: HashMap<String, Vec<(u32, u32)>> = HashMap::new();
        let mut trigram: HashMap<String, HashSet<u32>> = HashMap::new();
        let mut doc_lens = Vec::with_capacity(n);
        let mut surfaces: Vec<Vec<String>> = Vec::with_capacity(n);
        let mut prefix: BTreeMap<char, Vec<(String, u32)>> = BTreeMap::new();

        for (i, e) in entries.iter().enumerate() {
            let doc_id = i as u32;
            // Surface forms = name + each alias, normalized.
            let mut surface_set: Vec<String> = Vec::new();
            surface_set.push(normalize(&e.name));
            for a in &e.aliases {
                surface_set.push(normalize(a));
            }
            surfaces.push(surface_set.clone());

            // Per-doc token counts (BM25 needs raw frequencies + total len).
            let mut tf: HashMap<String, u32> = HashMap::new();
            let mut total_tokens: u32 = 0;
            let feed = |s: &str, tf: &mut HashMap<String, u32>, total: &mut u32| {
                for t in tokenize(s) {
                    *tf.entry(t).or_insert(0) += 1;
                    *total += 1;
                }
            };
            feed(&e.name, &mut tf, &mut total_tokens);
            for a in &e.aliases {
                feed(a, &mut tf, &mut total_tokens);
            }
            for ad in &e.addresses {
                feed(ad, &mut tf, &mut total_tokens);
            }
            for n in &e.nationalities {
                feed(n, &mut tf, &mut total_tokens);
            }
            for p in &e.programs {
                feed(p, &mut tf, &mut total_tokens);
            }
            doc_lens.push(total_tokens);

            for (term, freq) in tf {
                inverted.entry(term).or_default().push((doc_id, freq));
            }

            // Trigram index over name + aliases only (substrings of names
            // are the typical typo-recall target; addresses are noisier).
            for s in &surfaces[i] {
                for tg in trigrams(s) {
                    trigram.entry(tg).or_default().insert(doc_id);
                }
            }

            // Prefix index over surface forms.
            for s in &surfaces[i] {
                if let Some(c) = s.chars().next() {
                    prefix.entry(c).or_default().push((s.clone(), doc_id));
                }
            }
        }

        let avg_doc_len = if n == 0 {
            0.0
        } else {
            doc_lens.iter().map(|x| *x as f32).sum::<f32>() / n as f32
        };

        // Sort posting lists by doc_id for binary search; sort prefix entries
        // by surface for prefix-walks.
        for postings in inverted.values_mut() {
            postings.sort_by_key(|(d, _)| *d);
        }
        for v in prefix.values_mut() {
            v.sort_by(|a, b| a.0.cmp(&b.0));
        }

        let trigram_flat: HashMap<String, Vec<u32>> = trigram
            .into_iter()
            .map(|(k, set)| {
                let mut v: Vec<u32> = set.into_iter().collect();
                v.sort_unstable();
                (k, v)
            })
            .collect();

        Self {
            entries,
            inverted,
            trigram: trigram_flat,
            doc_lens,
            avg_doc_len,
            surfaces,
            prefix,
            thresholds: TlpThresholds::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn entry(&self, id: &str) -> Option<&CslEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Run a query and return up to `params.limit` ranked hits.
    ///
    /// Pipeline:
    /// 1. Tokenize + BM25-score every doc that contains at least one query
    ///    term. If `fuzzy`, also include trigram-overlap candidates.
    /// 2. Take top-K=200 by BM25 score.
    /// 3. Rerank by blending BM25 (normalized) with the maximum
    ///    Jaro-Winkler similarity between the normalized query and any
    ///    surface form of the candidate.
    /// 4. Apply source / entity-type filters, return top `limit`.
    pub fn search(&self, params: &SearchParams) -> SearchResult {
        let q_norm = normalize(&params.q);
        let q_tokens = tokenize(&params.q);

        // Stage 1: candidates from inverted index.
        let mut bm25: HashMap<u32, f32> = HashMap::new();
        for term in &q_tokens {
            let postings = match self.inverted.get(term) {
                Some(p) => p,
                None => continue,
            };
            let n_docs_with_term = postings.len() as f32;
            let n = self.entries.len() as f32;
            // Robertson-Spärck-Jones IDF (always >= 0).
            let idf = ((n - n_docs_with_term + 0.5) / (n_docs_with_term + 0.5) + 1.0).ln();
            for &(doc, tf) in postings {
                let dl = self.doc_lens[doc as usize] as f32;
                let denom = tf as f32
                    + BM25_K1 * (1.0 - BM25_B + BM25_B * dl / self.avg_doc_len.max(1.0));
                let contrib = idf * (tf as f32 * (BM25_K1 + 1.0)) / denom.max(f32::EPSILON);
                *bm25.entry(doc).or_insert(0.0) += contrib;
            }
        }

        // Stage 1b: trigram overlap candidates (fuzzy recall).
        if params.fuzzy {
            let q_grams = trigrams(&params.q);
            if !q_grams.is_empty() {
                let mut overlap: HashMap<u32, u32> = HashMap::new();
                for g in &q_grams {
                    if let Some(docs) = self.trigram.get(g) {
                        for &d in docs {
                            *overlap.entry(d).or_insert(0) += 1;
                        }
                    }
                }
                // Promote any doc whose trigram overlap is at least half the
                // query's trigram count, even if it had zero BM25.
                let needed = (q_grams.len() / 2).max(1) as u32;
                for (d, k) in overlap {
                    if k >= needed {
                        bm25.entry(d).or_insert(0.0);
                    }
                }
            }
        }

        // Stage 2: take top-K by BM25 (or all candidates if fewer than K).
        let mut by_bm25: Vec<(u32, f32)> = bm25.into_iter().collect();
        by_bm25.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        if by_bm25.len() > RERANK_TOP_K {
            by_bm25.truncate(RERANK_TOP_K);
        }

        // Normalization for BM25 component of the blended score.
        let bm25_max = by_bm25.iter().map(|(_, s)| *s).fold(0.0_f32, f32::max);

        // Stage 3: compute the per-doc match-strength (pure name/alias
        // similarity) and the blended ranking score.
        //
        // Two scores per candidate, on purpose:
        //
        // - `match_strength` (0..1): the maximum Jaro-Winkler similarity
        //   between the normalized query and any surface form (name or
        //   alias). This drives TLP banding per PLAN.md §3.3 (exact ⇒ RED,
        //   ≥ 0.95 ⇒ RED, ≥ 0.75 ⇒ YELLOW, else GREEN). It's robust to
        //   noise in BM25.
        // - `score` (0..1): the ranking score, blending name similarity
        //   with BM25 over the full record (so addresses, programs,
        //   nationalities still help ranking). BM25 is normalized by an
        //   absolute reference rather than the max in the result set, so
        //   a query that has only weak matches doesn't get a falsely
        //   inflated top-hit score.
        const BM25_REFERENCE: f32 = 5.0;
        let mut exact_alias_hit_id: Option<u32> = None;
        let mut scored: Vec<(u32, f32, f32, f32)> = Vec::with_capacity(by_bm25.len());
        for (doc, bm) in by_bm25 {
            let surfaces = &self.surfaces[doc as usize];
            let mut max_jw: f32 = 0.0;
            for s in surfaces {
                if s == &q_norm && !q_norm.is_empty() {
                    max_jw = 1.0;
                    if exact_alias_hit_id.is_none() {
                        exact_alias_hit_id = Some(doc);
                    }
                    break;
                }
                let jw = strsim::jaro_winkler(s, &q_norm) as f32;
                if jw > max_jw {
                    max_jw = jw;
                }
            }
            let bm_norm = (bm / BM25_REFERENCE).min(1.0);
            let blended_score = JW_WEIGHT * max_jw + (1.0 - JW_WEIGHT) * bm_norm;
            scored.push((doc, blended_score, max_jw, bm));
        }
        let _ = bm25_max; // intentionally unused — kept for diagnostics if needed.

        // Stage 4: apply filters, sort, truncate to limit.
        let entries = &self.entries;
        let sources = params.sources.as_ref();
        let etypes = params.entity_types.as_ref();
        scored.retain(|(d, _, _, _)| {
            let e = &entries[*d as usize];
            let ok_source = sources.is_none_or(|s| s.iter().any(|x| x == &e.source_list));
            let ok_etype = etypes.is_none_or(|s| s.contains(&e.entity_type));
            ok_source && ok_etype
        });
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let limit = params.limit.max(1);
        scored.truncate(limit);

        let exact_alias_match = exact_alias_hit_id
            .map(|id| scored.iter().any(|(d, _, _, _)| *d == id))
            .unwrap_or(false);

        // Result-set TLP uses match-strength of the top hit, not the
        // blended score (per PLAN.md §3.3).
        let top_match_strength = scored.first().map(|(_, _, jw, _)| *jw);
        let result_tlp = band(top_match_strength, exact_alias_match, &self.thresholds);

        let hits = scored
            .iter()
            .map(|&(d, score, jw, _bm)| {
                let e = &self.entries[d as usize];
                let mut matched = Vec::new();
                if jw >= 0.95 {
                    matched.push("name".into());
                }
                if !q_tokens.is_empty()
                    && q_tokens.iter().any(|t| {
                        e.addresses
                            .iter()
                            .any(|a| tokenize(a).iter().any(|x| x == t))
                    })
                {
                    matched.push("addresses".into());
                }
                let snippet = e.name.clone();
                let hit_tlp = if exact_alias_hit_id == Some(d) {
                    Tlp::Red
                } else if jw >= self.thresholds.red {
                    Tlp::Red
                } else if jw >= self.thresholds.yellow {
                    Tlp::Yellow
                } else {
                    Tlp::Green
                };
                Hit {
                    entry_id: e.id.clone(),
                    score,
                    matched_fields: matched,
                    snippet,
                    tlp: hit_tlp,
                }
            })
            .collect();

        SearchResult {
            audit_id: format!("synthetic-{}", q_norm.len()), // real UUIDv7 in M4
            tlp: result_tlp,
            hits,
            exact_alias_match,
        }
    }

    /// Prefix autocomplete over surface forms (name + aliases). Returns up
    /// to `limit` distinct suggestions, lexicographically sorted within
    /// each starting character bucket.
    pub fn autocomplete(&self, prefix: &str, limit: usize) -> Vec<String> {
        let p = normalize(prefix);
        if p.is_empty() {
            return Vec::new();
        }
        let first = p.chars().next().unwrap();
        let bucket = match self.prefix.get(&first) {
            Some(b) => b,
            None => return Vec::new(),
        };
        let mut out: Vec<String> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::new();
        for (surface, _doc) in bucket {
            if surface.starts_with(&p) && seen.insert(surface.as_str()) {
                out.push(surface.clone());
                if out.len() >= limit {
                    break;
                }
            }
        }
        out
    }
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub audit_id: String,
    pub tlp: Tlp,
    pub hits: Vec<Hit>,
    /// True when the normalized query equals one of the top-hit's
    /// surface forms (name or alias) exactly. Drives the
    /// `auto-block` vs `pending-block` decision split — exact matches
    /// auto-decide, near-matches go to the review queue.
    pub exact_alias_match: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_index() -> SearchEngine {
        let entries = vec![
            CslEntry {
                id: "1".into(),
                source_list: "SDN".into(),
                name: "Acme Holdings Pyongyang".into(),
                aliases: vec!["Acme Holdings".into(), "Acme P.Y.".into()],
                entity_type: EntityType::Entity,
                addresses: vec!["DPRK".into()],
                nationalities: vec![],
                programs: vec!["NK".into()],
            },
            CslEntry {
                id: "2".into(),
                source_list: "EL".into(),
                name: "Volga Shipping LLC".into(),
                aliases: vec![],
                entity_type: EntityType::Entity,
                addresses: vec!["Russia".into()],
                nationalities: vec![],
                programs: vec![],
            },
            CslEntry {
                id: "3".into(),
                source_list: "SDN".into(),
                name: "Müller, Hans".into(),
                aliases: vec!["Mueller, Hans".into()],
                entity_type: EntityType::Individual,
                addresses: vec![],
                nationalities: vec!["DE".into()],
                programs: vec![],
            },
        ];
        SearchEngine::build(entries)
    }

    #[test]
    fn exact_name_match_is_red() {
        let e = tiny_index();
        let r = e.search(&SearchParams::new("Acme Holdings Pyongyang"));
        assert_eq!(r.hits[0].entry_id, "1");
        assert_eq!(r.tlp, Tlp::Red);
    }

    #[test]
    fn exact_alias_match_is_red() {
        let e = tiny_index();
        let r = e.search(&SearchParams::new("Acme Holdings"));
        assert_eq!(r.hits[0].entry_id, "1");
        assert_eq!(r.tlp, Tlp::Red);
    }

    #[test]
    fn diacritic_fold_finds_canonical() {
        let e = tiny_index();
        let r = e.search(&SearchParams::new("Mueller Hans"));
        assert_eq!(r.hits[0].entry_id, "3");
        // Normalized "muller hans" matches alias "mueller, hans" exactly.
        assert_eq!(r.tlp, Tlp::Red);
    }

    #[test]
    fn typo_recall_via_trigrams() {
        let e = tiny_index();
        // "Volgaa" — extra letter.
        let r = e.search(&SearchParams {
            q: "Volgaa Shipping".into(),
            sources: None,
            entity_types: None,
            fuzzy: true,
            limit: 5,
        });
        assert_eq!(r.hits[0].entry_id, "2");
        assert!(matches!(r.tlp, Tlp::Yellow | Tlp::Red));
    }

    #[test]
    fn green_on_unrelated() {
        let e = tiny_index();
        let r = e.search(&SearchParams::new("Quetzal Insurance"));
        assert_eq!(r.tlp, Tlp::Green);
    }

    #[test]
    fn source_filter_excludes_others() {
        let e = tiny_index();
        let r = e.search(&SearchParams {
            q: "Acme".into(),
            sources: Some(vec!["EL".into()]),
            entity_types: None,
            fuzzy: true,
            limit: 5,
        });
        assert!(r.hits.iter().all(|h| h.entry_id != "1"));
    }

    #[test]
    fn entity_type_filter() {
        let e = tiny_index();
        let r = e.search(&SearchParams {
            q: "Hans".into(),
            sources: None,
            entity_types: Some(vec![EntityType::Entity]),
            fuzzy: true,
            limit: 5,
        });
        assert!(r.hits.iter().all(|h| h.entry_id != "3"));
    }

    #[test]
    fn autocomplete_returns_prefixed_surfaces() {
        let e = tiny_index();
        let suggestions = e.autocomplete("Acme", 8);
        assert!(suggestions.iter().any(|s| s.starts_with("acme")));
    }

    #[test]
    fn empty_query_returns_no_hits() {
        let e = tiny_index();
        let r = e.search(&SearchParams::new(""));
        assert!(r.hits.is_empty());
        assert_eq!(r.tlp, Tlp::Green);
    }

    #[test]
    fn build_handles_empty_corpus() {
        let e = SearchEngine::build(Vec::new());
        let r = e.search(&SearchParams::new("anything"));
        assert!(r.hits.is_empty());
        assert_eq!(r.tlp, Tlp::Green);
    }
}
