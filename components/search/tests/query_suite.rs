//! M1 fixture-query suite. Builds a 10k-record synthetic corpus, runs the
//! 50-query gold set from `tests/components/search-fixtures/queries.json`,
//! asserts top-1 expected IDs and TLP bands, and measures p50/p95 latency.
//!
//! Exit gate (per PLAN.md M1): p95 query latency < 100 ms on the corpus.
//! If this regresses, M1's exit criteria fail and we replan.

use std::path::PathBuf;
use std::time::Instant;

use ocelaudit_search::{CslEntry, EntityType, SearchEngine, SearchParams, Tlp};
use serde::Deserialize;

/// 10k records: 23 hand-crafted "known" entries that the gold-set queries
/// target, plus 9977 procedural entries built from a deterministic LCG so
/// the corpus is identical across runs without checking in megabytes of
/// JSON.
const TOTAL_ENTRIES: usize = 10_000;
const P95_GATE_MS: f64 = 100.0;

#[derive(Debug, Deserialize)]
struct QueryCase {
    q: String,
    expected_id: String,
    expected_tlp: String,
    #[serde(default)]
    kind: String,
}

fn known_entries() -> Vec<CslEntry> {
    use EntityType::*;
    vec![
        e("known-0", "SDN", "Acme Holdings Pyongyang",
          &["Acme Holdings", "Acme P.Y."], Entity, &["DPRK"], &["KP"], &["NK"]),
        e("known-1", "EL", "Volga Shipping LLC",
          &[], Entity, &["Russia"], &["RU"], &["RU-SHIP"]),
        e("known-2", "SDN", "Müller, Hans",
          &["Mueller, Hans", "Hans Mueller"], Individual, &["Hamburg"], &["DE"], &["EU-FRZ"]),
        e("known-3", "ITAR-DPL", "Tehran Metals Co",
          &["TMC Iran"], Entity, &["Iran"], &["IR"], &["IR-MIL"]),
        e("known-4", "EL", "Pyongyang Steel Industries",
          &["PSI"], Entity, &["DPRK"], &["KP"], &["NK"]),
        e("known-5", "SDN", "Petrov Industries Moscow",
          &["Petrov Industries"], Entity, &["Russia"], &["RU"], &["RU-DEF"]),
        e("known-6", "EL", "Caspian Maritime Group",
          &["CMG", "Caspian Maritime"], Entity, &["Iran"], &["IR"], &["IR-SHIP"]),
        e("known-7", "SDN", "MV Sea Star",
          &["Sea Star"], Vessel, &["Pyongyang"], &["KP"], &["NK-VESSEL"]),
        e("known-8", "SDN", "Aircraft N12345",
          &["N12345"], Aircraft, &["Tehran"], &["IR"], &["IR-AIR"]),
        e("known-9", "ITAR-DPL", "Khan, Abdul",
          &["Abdul Q. Khan"], Individual, &["Karachi"], &["PK"], &["NUCLEAR"]),
        e("known-10", "ITAR-DPL", "Tehran Aerospace Defense",
          &["TAD"], Entity, &["Iran"], &["IR"], &["IR-MIL"]),
        e("known-11", "EL", "Singapore Trading Partners",
          &["STP"], Entity, &["Singapore"], &["SG"], &["FRONT-CO"]),
        e("known-12", "SDN", "Black Sea Shipping Co",
          &["BSS Co"], Entity, &["Sevastopol"], &["RU"], &["RU-SHIP"]),
        e("known-13", "SDN", "Aleksei Volkov",
          &["Alexei Volkov", "A. Volkov"], Individual, &["Moscow"], &["RU"], &["RU-DEF"]),
        e("known-14", "ITAR-DPL", "Damascus Defense Industries",
          &["DDI Damascus", "DDI"], Entity, &["Damascus"], &["SY"], &["SY-MIL"]),
        e("known-15", "SDN", "Kasparov, Mikhail",
          &["Mikhail Kasparov"], Individual, &["St Petersburg"], &["RU"], &["RU-CYBER"]),
        e("known-16", "EL", "Crimean Mining Consortium",
          &["Crimea Mining"], Entity, &["Sevastopol"], &["UA"], &["RU-CRIMEA"]),
        e("known-17", "EL", "Beijing Cyber Solutions",
          &["BCS"], Entity, &["Beijing"], &["CN"], &["CN-CYBER"]),
        e("known-18", "SDN", "Lazarev, Yuri",
          &["Yuri Lazarev"], Individual, &["Moscow"], &["RU"], &["RU-CYBER"]),
        e("known-19", "EL", "Havana Imports Group",
          &["HIG"], Entity, &["Havana"], &["CU"], &["CU-EMB"]),
        e("known-20", "SDN", "Northeast Industrial Holdings",
          &["NIH"], Entity, &["Pyongyang"], &["KP"], &["NK"]),
        e("known-21", "ITAR-DPL", "Reza Sharif",
          &["Sharif, Reza"], Individual, &["Tehran"], &["IR"], &["IR-MIL"]),
        e("known-22", "SDN", "Pyongyang Bank Corp",
          &["Pyong Yang Bank"], Entity, &["Pyongyang"], &["KP"], &["NK"]),
    ]
}

fn e(
    id: &str,
    src: &str,
    name: &str,
    aliases: &[&str],
    et: EntityType,
    addrs: &[&str],
    nats: &[&str],
    progs: &[&str],
) -> CslEntry {
    CslEntry {
        id: id.into(),
        source_list: src.into(),
        name: name.into(),
        aliases: aliases.iter().map(|s| s.to_string()).collect(),
        entity_type: et,
        addresses: addrs.iter().map(|s| s.to_string()).collect(),
        nationalities: nats.iter().map(|s| s.to_string()).collect(),
        programs: progs.iter().map(|s| s.to_string()).collect(),
    }
}

/// Deterministic 32-bit linear congruential generator, seeded fixed so the
/// 10k synthetic corpus is identical across runs without rand-as-a-dep.
struct Lcg(u32);
impl Lcg {
    fn new(seed: u32) -> Self {
        Self(seed.wrapping_mul(2654435761).wrapping_add(1))
    }
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1664525).wrapping_add(1013904223);
        self.0
    }
    fn pick<'a, T>(&mut self, slice: &'a [T]) -> &'a T {
        &slice[self.next() as usize % slice.len()]
    }
}

fn synthetic_entries(n: usize) -> Vec<CslEntry> {
    let first_names = &[
        "Andrei", "Sergei", "Dmitri", "Vladimir", "Igor", "Anatoli", "Pavel", "Alexei",
        "Mohammad", "Ali", "Hassan", "Hossein", "Mehdi", "Reza", "Saeed", "Karim",
        "Kim", "Park", "Choi", "Yi", "Lee", "Chen", "Wang", "Liu", "Zhang", "Zhao",
        "Carlos", "Miguel", "Jose", "Diego", "Raul", "Hector", "Roberto", "Eduardo",
        "Fatima", "Aisha", "Layla", "Noor", "Yasmin", "Zahra",
    ];
    let last_names = &[
        "Petrov", "Volkov", "Smirnov", "Kuznetsov", "Sokolov", "Mikhailov", "Fedorov",
        "Khan", "Sharif", "Hosseini", "Karimi", "Naderi", "Ahmadi",
        "Park", "Choi", "Yi", "Han", "Cho", "Jung",
        "Wang", "Chen", "Liu", "Zhang", "Zhao", "Yang",
        "Garcia", "Martinez", "Rodriguez", "Lopez", "Hernandez",
    ];
    let org_prefixes = &[
        "Crescent", "Northern", "Southern", "Eastern", "Western", "Pacific", "Atlantic",
        "Industrial", "Maritime", "Aerospace", "Energy", "Mineral", "Metals", "Defence",
        "Trading", "Commerce", "Holdings", "Group", "Consortium", "Capital",
    ];
    let org_suffixes = &[
        "LLC", "Co", "Inc", "Corp", "Group", "Industries", "Trading", "Partners",
        "International", "Holdings", "Limited", "Ltd", "GmbH", "S.A.", "A.G.",
    ];
    let cities = &[
        "Pyongyang", "Tehran", "Moscow", "Damascus", "Havana", "Caracas", "Beijing",
        "Singapore", "Karachi", "Beirut", "Tripoli", "Khartoum", "Yangon", "Minsk",
    ];
    let countries = &[
        "KP", "IR", "RU", "SY", "CU", "VE", "CN", "SG", "PK", "LB", "LY", "SD", "MM", "BY",
    ];
    let sources = &[
        "SDN", "EL", "UVL", "NS-MBS", "ITAR-DPL", "FSE", "PLC", "NS-Plc", "Sectoral",
    ];
    let programs = &[
        "NK", "IR-MIL", "IR-SHIP", "RU-DEF", "RU-SHIP", "RU-CYBER", "SY-MIL", "CU-EMB",
        "CN-CYBER", "FRONT-CO", "NUCLEAR", "WMD",
    ];

    let mut rng = Lcg::new(0xCAFEBABE);
    (0..n)
        .map(|i| {
            let kind = rng.next() % 100;
            let (name, et) = if kind < 30 {
                let f = rng.pick(first_names);
                let l = rng.pick(last_names);
                (format!("{} {}", f, l), EntityType::Individual)
            } else if kind < 92 {
                let p = rng.pick(org_prefixes);
                let q = rng.pick(org_prefixes);
                let s = rng.pick(org_suffixes);
                (format!("{} {} {}", p, q, s), EntityType::Entity)
            } else if kind < 96 {
                let p = rng.pick(org_prefixes);
                (format!("MV {} {}", p, rng.pick(cities)), EntityType::Vessel)
            } else {
                let p = rng.pick(org_prefixes);
                (format!("Aircraft {} {}", p, rng.next() % 100000), EntityType::Aircraft)
            };
            CslEntry {
                id: format!("syn-{}", i),
                source_list: rng.pick(sources).to_string(),
                name,
                aliases: vec![],
                entity_type: et,
                addresses: vec![rng.pick(cities).to_string()],
                nationalities: vec![rng.pick(countries).to_string()],
                programs: vec![rng.pick(programs).to_string()],
            }
        })
        .collect()
}

fn build_corpus() -> Vec<CslEntry> {
    let mut out = known_entries();
    let synthetic_n = TOTAL_ENTRIES - out.len();
    out.extend(synthetic_entries(synthetic_n));
    out
}

fn fixture_path() -> PathBuf {
    // CARGO_MANIFEST_DIR points at components/search/. Walk up two levels
    // to reach the workspace root, then into tests/components/search-fixtures.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/components/search-fixtures/queries.json")
}

fn load_queries() -> Vec<QueryCase> {
    let path = fixture_path();
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    serde_json::from_slice(&bytes).expect("parse queries.json")
}

fn parse_tlp(s: &str) -> Tlp {
    match s {
        "green" => Tlp::Green,
        "yellow" => Tlp::Yellow,
        "red" => Tlp::Red,
        other => panic!("unknown tlp '{}'", other),
    }
}

#[test]
fn fixture_query_suite() {
    let queries = load_queries();
    assert!(
        queries.len() >= 50,
        "expected ≥ 50 queries in gold set, got {}",
        queries.len()
    );

    let corpus = build_corpus();
    assert_eq!(corpus.len(), TOTAL_ENTRIES);
    let engine = SearchEngine::build(corpus);

    let mut latencies_us: Vec<u128> = Vec::with_capacity(queries.len());
    let mut top1_correct = 0usize;
    let mut tlp_correct = 0usize;
    let mut top10_recall = 0usize;
    let mut failures: Vec<String> = Vec::new();

    for case in &queries {
        let params = SearchParams {
            q: case.q.clone(),
            sources: None,
            entity_types: None,
            fuzzy: true,
            limit: 10,
        };
        let start = Instant::now();
        let r = engine.search(&params);
        let elapsed = start.elapsed();
        latencies_us.push(elapsed.as_micros());

        let actual_tlp = r.tlp;
        let want_tlp = parse_tlp(&case.expected_tlp);
        if actual_tlp == want_tlp {
            tlp_correct += 1;
        } else {
            failures.push(format!(
                "TLP mismatch [{}]: q='{}' expected={:?} got={:?}",
                case.kind, case.q, want_tlp, actual_tlp
            ));
        }

        if !case.expected_id.is_empty() {
            let top1 = r.hits.first().map(|h| h.entry_id.as_str()).unwrap_or("");
            if top1 == case.expected_id {
                top1_correct += 1;
            } else {
                failures.push(format!(
                    "top1 mismatch [{}]: q='{}' expected={} got={}",
                    case.kind, case.q, case.expected_id, top1
                ));
            }
            if r.hits.iter().any(|h| h.entry_id == case.expected_id) {
                top10_recall += 1;
            }
        }
    }

    latencies_us.sort_unstable();
    let p50_ms = latencies_us[latencies_us.len() / 2] as f64 / 1000.0;
    let p95_ms = latencies_us[(latencies_us.len() * 95) / 100] as f64 / 1000.0;
    let p99_idx = ((latencies_us.len() * 99) / 100).min(latencies_us.len() - 1);
    let p99_ms = latencies_us[p99_idx] as f64 / 1000.0;
    let max_ms = *latencies_us.last().unwrap() as f64 / 1000.0;

    let id_queries = queries.iter().filter(|q| !q.expected_id.is_empty()).count();

    eprintln!("\nM1 fixture-query suite:");
    eprintln!("  corpus      : {} entries", TOTAL_ENTRIES);
    eprintln!("  queries     : {} ({} with expected ID)", queries.len(), id_queries);
    eprintln!("  top-1 hits  : {}/{}", top1_correct, id_queries);
    eprintln!("  top-10 recall: {}/{}", top10_recall, id_queries);
    eprintln!("  TLP correct : {}/{}", tlp_correct, queries.len());
    eprintln!(
        "  latency ms  : p50={:.2}  p95={:.2}  p99={:.2}  max={:.2}",
        p50_ms, p95_ms, p99_ms, max_ms
    );

    for f in &failures {
        eprintln!("  ! {}", f);
    }

    // Hard gates per PLAN.md M1:
    assert!(
        p95_ms < P95_GATE_MS,
        "p95 {:.2} ms ≥ gate {:.2} ms",
        p95_ms,
        P95_GATE_MS
    );
    assert!(
        top1_correct as f64 / id_queries as f64 >= 0.85,
        "top-1 accuracy {:.2}% < 85% gate",
        100.0 * top1_correct as f64 / id_queries as f64
    );
    assert!(
        top10_recall as f64 / id_queries as f64 >= 0.95,
        "top-10 recall {:.2}% < 95% gate",
        100.0 * top10_recall as f64 / id_queries as f64
    );
    assert!(
        tlp_correct as f64 / queries.len() as f64 >= 0.85,
        "TLP accuracy {:.2}% < 85% gate",
        100.0 * tlp_correct as f64 / queries.len() as f64
    );
}
