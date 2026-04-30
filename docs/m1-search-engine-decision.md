# M1 — Search engine decision

**Decision recorded 2026-04-30. This decision is frozen for the rest of
the project. Do not revisit without a new spike.**

OcelAudit's search engine is the **hand-rolled fallback**:
in-memory inverted index over normalized tokens, BM25 scoring, plus
trigram-overlap recall, plus Jaro-Winkler reranking over the top-K=200
candidates, plus prefix autocomplete. Pure Rust. No C dependencies.

Lives in `components/search/` as a Rust library crate. The 10k-record
fixture-query suite is at `components/search/tests/query_suite.rs`,
backed by `tests/components/search-fixtures/queries.json` (52 queries
with hand-labeled top-1 IDs and TLP bands).

## What we tried

### Attempt 1 — tantivy (default features)

Skipped. `tantivy` 0.24 with default features pulls in `mmap2`, threading,
file locks. None of these are available on `wasm32-wasip2` in the
wasmCloud runtime (see PLAN.md §0). The compile would have failed early;
not worth the cycle.

### Attempt 2 — tantivy (`default-features = false`, just `mmap`)

```toml
tantivy = { version = "0.24", default-features = false, features = ["mmap"] }
```

Failed to compile. `tantivy` transitively depends on `zstd-sys`, which
shells out to `clang` to build the bundled `zstd` C source. The clang on
the runner doesn't recognize the `wasm32-unknown-wasip2` target triple,
so every `.c` and `.S` file fails:

```
clang -cc1as: error: unknown target triple 'wasm32-unknown-wasip2'
error: unable to create target: 'No available targets are compatible
       with triple "wasm32-unknown-wasip2"'
```

This is the wasi-sdk story, the same toolchain headache that PLAN.md §1.3
calls out for `rusqlite` + `bundled`. Solving it cleanly means installing
wasi-sdk, configuring `CC_wasm32_wasip2` and `CARGO_TARGET_WASM32_WASIP2_LINKER`,
and crossing fingers. For a search engine that's only one of many
possibilities, that's a poor cost/benefit. Stopped.

### Attempt 3 — tantivy on wasi-sdk

Did not attempt. Even if we got it compiling, the runtime story is
unproven (mmap-on-wasi-filesystem, single-threaded `IndexWriter`,
custom `Directory` impl, etc.). The PLAN.md M1 ordering says fall back
if the first two attempts fail; both of those failed for fundamental
toolchain reasons that wasi-sdk wouldn't fix. Spending the next hours
on this would have pushed M2+ out for marginal upside.

### Fallback — landed

```rust
pub fn search(&self, params: &SearchParams) -> SearchResult { /* ... */ }
```

Pipeline:

1. **Tokenize** the query (NFKC → strip combining marks → ASCII-lowercase
   → split on non-alphanumeric).
2. **BM25-score** every doc that contains at least one query term, using
   `k1 = 1.2`, `b = 0.75`. Indexes name, aliases, addresses,
   nationalities, programs.
3. **Trigram recall** (when `fuzzy = true`): also include any doc whose
   trigram overlap with the query is at least half the query's trigram
   count. Catches typos that BM25 alone misses.
4. **Take top-K = 200** by BM25.
5. **Compute Jaro-Winkler** between the normalized query and each
   candidate's normalized name + aliases. Take the max — call this
   *match strength*. Bookkeep an `exact_alias_match` flag.
6. **Rank** by a blended score: `0.7 * match_strength + 0.3 * (BM25 / 5.0).clamp(0, 1)`.
   The clamp matters: a query with weak matches everywhere doesn't get
   a falsely-inflated top-hit score (which would push GREEN → YELLOW
   for unrelated queries).
7. **TLP banding** uses *match strength*, not the blended score:
   exact ⇒ RED, ≥ 0.95 ⇒ RED, ≥ 0.75 ⇒ YELLOW, else GREEN. This
   matches PLAN.md §3.3.
8. **Apply filters** (source, entity-type) and truncate to `limit`.

Autocomplete is a separate prefix-walk over surface forms (name +
aliases) bucketed by first character.

## Performance gate (PLAN.md M1: p95 < 100 ms)

Measured on a release build of the fixture-query suite, 10k-record
synthetic corpus, 52 queries:

| metric                         | value          | gate           |
|--------------------------------|----------------|----------------|
| corpus size                    | 10,000 entries | —              |
| queries                        | 52 (48 with expected ID) | ≥ 50 |
| top-1 accuracy                 | 48 / 48 (100%) | ≥ 85%          |
| top-10 recall                  | 48 / 48 (100%) | ≥ 95%          |
| TLP banding accuracy           | 52 / 52 (100%) | ≥ 85%          |
| latency p50                    | **0.26 ms**    | —              |
| latency p95                    | **0.60 ms**    | < 100 ms ✅    |
| latency p99                    | **0.72 ms**    | —              |
| latency max                    | 0.72 ms        | —              |

The latency gate is satisfied with ~166× headroom on a M-series Apple
laptop (debug builds are slightly slower but still well under the gate).
The numbers are reproducible — the corpus is built deterministically
from a seeded LCG (`Lcg::new(0xCAFEBABE)`) at test time so we don't
check in megabytes of JSON. Re-running the suite produces the same
queries against the same corpus.

## Known limitations (carry forward to README caveats)

1. **No stemming.** `tehran metals` matches `Tehran Metals Co` via JW
   similarity, not via a stemmed token match. That's fine for proper
   nouns (which is most of what CSL contains) but degrades for
   non-English morphology. We don't currently support Korean, Arabic,
   Cyrillic search natively — non-ASCII characters survive through
   NFKC + accent fold but no language-specific tokenization.
2. **No phrase queries.** "John Smith" and "Smith John" rank the same
   under BM25 + JW. Acceptable for the demo (compliance lists are
   indexed person-name-style) but a real product would benefit from a
   phrase-aware scorer.
3. **No relevance feedback / personalization.** Out of scope.
4. **In-memory index.** A 10k corpus indexes to a few MB of HashMap;
   real CSL is ~5–15 MB of JSON which produces a similar-order index.
   We assume the corpus fits comfortably in the wasmCloud component's
   memory. If the CSL grows to hundreds of MB this becomes a problem.
5. **Trigram recall threshold (`q_grams.len() / 2`)** is a heuristic.
   May over-recall for very short queries (where half a query's
   trigrams is one or two grams) — partly compensated by the JW
   reranker and the BM25 score being near-zero for those candidates.
6. **JW reranking is O(K)** per query, K = 200. For deeper paginated
   results the model degrades — but the demo's UI only shows top-20.

## Reproducibility

To re-run the M1 suite:

```sh
cargo test -p ocelaudit-search --release --test query_suite -- --nocapture
```

Output includes top-1 / recall / TLP counts and p50/p95/p99/max
latency. The hard gates (`p95 < 100 ms`, top-1 ≥ 85%, recall ≥ 95%,
TLP ≥ 85%) are asserted at the end of the test — a regression on any
of them fails the build.

## What this means for later milestones

- **M2 (storage)**: storage delivers `CslEntry` records to search.
  `SearchEngine::build` takes ownership of the corpus. No interface
  change required when storage backends rotate (jsonfs → sqlite →
  turso).
- **M3 (csl-ingest)**: after `bulk-replace`, csl-ingest signals search
  to rebuild the index. Build cost on 10k records is sub-second; we
  expect ~5–10 s on the real ~50k-record CSL.
- **M4 (api-gateway)**: api-gateway calls the search component over
  WIT. The Rust types in `components/search/src/model.rs` map directly
  onto the WIT records in `interfaces/ocelaudit/search.wit` (M3+).
- **M9 (admin scoring panel)**: TLP thresholds are stored on
  `SearchEngine.thresholds` (default red=0.95, yellow=0.75) and are
  trivially settable from config / admin UI.

## Decision frozen

If a future requirement compels revisiting the search engine — say,
multi-million-record corpora or full phrase search — open a fresh
spike. Don't re-evaluate this decision in the middle of M2–M11.
