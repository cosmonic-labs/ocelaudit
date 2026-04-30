//! OcelAudit CSL ingest.
//!
//! Two surfaces:
//!
//! - `parse_external_json(bytes)` — pure transform from the ITA CSL JSON
//!   shape into our `CslEntry` records. Unit-testable on the host
//!   target; no WASI imports.
//! - `fetch::http_get(url)` (behind the `wasi-http` cargo feature) — the
//!   wasi:http/outgoing-handler client used by the gateway in production.
//!   Disabled by default so `cargo test` on the host target stays
//!   compileable without the wit-bindgen-emitted host-incompatible code.
//!
//! No in-process cron. WASI P2 components are request/response — they
//! don't run loops between calls. Scheduled refresh is delegated to an
//! external scheduler that POSTs to `/api/v1/csl/refresh`. Captured in
//! the README's "what we faked or skipped" section.

use ocelaudit_search::{CslEntry, EntityType};
use serde::{Deserialize, Deserializer, Serialize};

/// `#[serde(deserialize_with = "null_to_default")]` adapter so a JSON
/// `null` for a Vec field deserializes as `Vec::new()` instead of
/// failing. The live trade.gov CSL emits `null` for absent
/// `alt_names`, `programs`, `addresses`, etc., on ~12% of records.
fn null_to_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    T: Default + Deserialize<'de>,
    D: Deserializer<'de>,
{
    let opt = Option::<T>::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

pub mod source_meta;

#[derive(Debug)]
pub enum IngestError {
    Parse(serde_json::Error),
    Fetch(String),
    InvalidUrl(String),
}

impl std::fmt::Display for IngestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IngestError::Parse(e) => write!(f, "csl json parse: {}", e),
            IngestError::Fetch(s) => write!(f, "csl fetch: {}", s),
            IngestError::InvalidUrl(s) => write!(f, "csl invalid url: {}", s),
        }
    }
}

impl std::error::Error for IngestError {}

impl From<serde_json::Error> for IngestError {
    fn from(e: serde_json::Error) -> Self {
        IngestError::Parse(e)
    }
}

/// Outcome of one ingest call. The caller is responsible for handing
/// `entries` to `JsonFsStorage::csl_bulk_replace` (kept decoupled so the
/// ingest crate doesn't pull a hard dep on storage).
#[derive(Debug, Clone)]
pub struct IngestResult {
    pub entries: Vec<CslEntry>,
    pub fetched_at: u64,
    pub version: String,
    pub source_count: usize,
}

/// External JSON shape — the ITA's CSL format. Optionally wrapped in
/// `{"results": [...]}`; we accept either.
#[derive(Deserialize)]
#[serde(untagged)]
enum CslJson {
    Wrapped { results: Vec<ExternalRecord> },
    Bare(Vec<ExternalRecord>),
}

/// Keep only the fields we actually transform. Other fields the upstream
/// emits (federal_register_notice, dates_of_birth, vessel_type, ...) are
/// silently ignored — we don't `deny_unknown_fields` so the parser is
/// forward-compatible with shape evolution.
#[derive(Deserialize)]
struct ExternalRecord {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    alt_names: Vec<String>,
    #[serde(default, rename = "type")]
    type_: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    programs: Vec<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    addresses: Vec<ExternalAddress>,
    #[serde(default, deserialize_with = "null_to_default")]
    nationalities: Vec<String>,
    #[serde(default, deserialize_with = "null_to_default")]
    citizenships: Vec<String>,
}

#[derive(Deserialize, Serialize, Default, Clone)]
struct ExternalAddress {
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    city: Option<String>,
    #[serde(default)]
    state_or_province: Option<String>,
    #[serde(default)]
    postal_code: Option<String>,
    #[serde(default)]
    country: Option<String>,
}

impl ExternalAddress {
    fn flat(&self) -> String {
        [
            self.address.as_deref(),
            self.city.as_deref(),
            self.state_or_province.as_deref(),
            self.postal_code.as_deref(),
            self.country.as_deref(),
        ]
        .iter()
        .filter_map(|x| *x)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(", ")
    }
}

/// Parse the external CSL JSON bytes into our `CslEntry` records.
///
/// Rejects records with no `name` (we can't index those). Tolerant of
/// missing optional fields, both wrapper and bare-array shapes, unknown
/// source strings (passed through verbatim).
pub fn parse_external_json(bytes: &[u8]) -> Result<Vec<CslEntry>, IngestError> {
    let parsed: CslJson = serde_json::from_slice(bytes)?;
    let records = match parsed {
        CslJson::Wrapped { results } => results,
        CslJson::Bare(v) => v,
    };

    // The live trade.gov feed contains ~91 groups of records sharing
    // the same `id` (different addresses or programs in each row).
    // Suffixing with a per-id sequence keeps every row distinct so the
    // engine can return them as separate hits.
    let mut id_counts: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();

    let mut out = Vec::with_capacity(records.len());
    for (i, r) in records.into_iter().enumerate() {
        let name = match r.name.as_deref() {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => continue,
        };
        let source_raw = r.source.as_deref().unwrap_or("Unknown");
        let source_list = source_meta::short_code(source_raw)
            .map(str::to_string)
            .unwrap_or_else(|| source_raw.to_string());
        let entity_type = parse_entity_type(r.type_.as_deref().unwrap_or(""));
        let raw_id = r
            .id
            .clone()
            .unwrap_or_else(|| format!("{}-{}", source_list, i));
        let count = id_counts.entry(raw_id.clone()).or_insert(0);
        let id = if *count == 0 {
            raw_id.clone()
        } else {
            format!("{}#{}", raw_id, *count + 1)
        };
        *count += 1;
        let addresses: Vec<String> = r
            .addresses
            .iter()
            .map(|a| a.flat())
            .filter(|s| !s.is_empty())
            .collect();
        let mut nationalities = r.nationalities.clone();
        nationalities.extend(r.citizenships.clone());
        nationalities.dedup();
        out.push(CslEntry {
            id,
            source_list,
            name,
            aliases: r.alt_names,
            entity_type,
            addresses,
            nationalities,
            programs: r.programs,
        });
    }
    Ok(out)
}

fn parse_entity_type(s: &str) -> EntityType {
    let s = s.trim().to_ascii_lowercase();
    match s.as_str() {
        "individual" | "person" => EntityType::Individual,
        "entity" | "company" | "organization" => EntityType::Entity,
        "vessel" | "ship" => EntityType::Vessel,
        "aircraft" | "plane" => EntityType::Aircraft,
        _ => EntityType::Unknown,
    }
}

/// Build an `IngestResult` from already-parsed entries. Caller supplies
/// `fetched_at` (UTC seconds) and `version` (e.g. an ETag or content
/// hash from the upstream fetch). Used by both the test path (loading a
/// local fixture) and the production path (after `fetch::http_get`).
pub fn build_result(
    entries: Vec<CslEntry>,
    fetched_at: u64,
    version: impl Into<String>,
) -> IngestResult {
    let mut sources = std::collections::BTreeSet::new();
    for e in &entries {
        sources.insert(e.source_list.clone());
    }
    IngestResult {
        source_count: sources.len(),
        entries,
        fetched_at,
        version: version.into(),
    }
}

#[cfg(feature = "wasi-http")]
pub mod fetch;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wrapped_shape() {
        let json = br#"{"results": [
            {"id": "a", "name": "Acme", "type": "Entity",
             "source": "Specially Designated Nationals (SDN) - Treasury Department"},
            {"id": "b", "name": "Smith, John", "type": "Individual",
             "source": "Entity List (EL) - Bureau of Industry and Security"}
        ]}"#;
        let entries = parse_external_json(json).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].source_list, "SDN");
        assert_eq!(entries[1].source_list, "EL");
        assert!(matches!(entries[0].entity_type, EntityType::Entity));
        assert!(matches!(entries[1].entity_type, EntityType::Individual));
    }

    #[test]
    fn parses_bare_array_shape() {
        let json = br#"[
            {"id": "x", "name": "Volga", "type": "Vessel",
             "source": "Specially Designated Nationals (SDN) - Treasury Department"}
        ]"#;
        let entries = parse_external_json(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0].entity_type, EntityType::Vessel));
    }

    #[test]
    fn skips_records_without_name() {
        let json = br#"{"results":[
            {"id": "a", "name": "Acme", "type": "Entity"},
            {"id": "b", "type": "Entity"}
        ]}"#;
        let entries = parse_external_json(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Acme");
    }

    #[test]
    fn unknown_source_passes_through_verbatim() {
        let json = br#"{"results":[
            {"name":"X","type":"Entity","source":"Made-Up List - Nowhere Department"}
        ]}"#;
        let entries = parse_external_json(json).unwrap();
        assert_eq!(entries[0].source_list, "Made-Up List - Nowhere Department");
    }

    #[test]
    fn duplicate_ids_get_distinct_suffixes() {
        let json = br#"{"results":[
            {"id":"X","name":"X-row-1","type":"Entity","source":"Specially Designated Nationals (SDN) - Treasury Department"},
            {"id":"X","name":"X-row-2","type":"Entity","source":"Specially Designated Nationals (SDN) - Treasury Department"},
            {"id":"X","name":"X-row-3","type":"Entity","source":"Specially Designated Nationals (SDN) - Treasury Department"}
        ]}"#;
        let entries = parse_external_json(json).unwrap();
        assert_eq!(entries.len(), 3);
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, vec!["X", "X#2", "X#3"]);
    }

    #[test]
    fn synthesizes_id_when_missing() {
        let json = br#"[{"name":"Anon","type":"Entity","source":"Specially Designated Nationals (SDN) - Treasury Department"}]"#;
        let entries = parse_external_json(json).unwrap();
        assert_eq!(entries[0].id, "SDN-0");
    }

    #[test]
    fn flattens_address_object_into_one_string() {
        let json = br#"[{
            "name":"Acme","type":"Entity","source":"Specially Designated Nationals (SDN) - Treasury Department",
            "addresses":[{"address":"1 Foo St","city":"Pyongyang","country":"DPRK"}]
        }]"#;
        let entries = parse_external_json(json).unwrap();
        assert_eq!(entries[0].addresses, vec!["1 Foo St, Pyongyang, DPRK"]);
    }

    #[test]
    fn merges_citizenships_into_nationalities() {
        let json = br#"[{
            "name":"X","type":"Individual","source":"Specially Designated Nationals (SDN) - Treasury Department",
            "nationalities":["IR"], "citizenships":["IR","DE"]
        }]"#;
        let entries = parse_external_json(json).unwrap();
        let mut nats = entries[0].nationalities.clone();
        nats.sort();
        assert_eq!(nats, vec!["DE", "IR"]);
    }

    #[test]
    fn tolerates_null_array_fields() {
        // Real trade.gov data emits null for absent arrays. Exercised
        // here with all five Vec fields nulled out.
        let json = br#"{"results":[
            {"name":"Anon","type":"Entity",
             "source":"Specially Designated Nationals (SDN) - Treasury Department",
             "alt_names":null,"programs":null,"addresses":null,
             "nationalities":null,"citizenships":null}
        ]}"#;
        let entries = parse_external_json(json).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(entries[0].aliases.is_empty());
        assert!(entries[0].programs.is_empty());
        assert!(entries[0].addresses.is_empty());
        assert!(entries[0].nationalities.is_empty());
    }

    #[test]
    fn entity_type_synonyms() {
        assert!(matches!(parse_entity_type("Person"), EntityType::Individual));
        assert!(matches!(parse_entity_type("COMPANY"), EntityType::Entity));
        assert!(matches!(parse_entity_type("ship"), EntityType::Vessel));
        assert!(matches!(parse_entity_type("Plane"), EntityType::Aircraft));
        assert!(matches!(parse_entity_type("???"), EntityType::Unknown));
    }

    #[test]
    fn build_result_counts_distinct_sources() {
        let entries = vec![
            CslEntry {
                id: "1".into(),
                source_list: "SDN".into(),
                name: "A".into(),
                aliases: vec![],
                entity_type: EntityType::Entity,
                addresses: vec![],
                nationalities: vec![],
                programs: vec![],
            },
            CslEntry {
                id: "2".into(),
                source_list: "EL".into(),
                name: "B".into(),
                aliases: vec![],
                entity_type: EntityType::Entity,
                addresses: vec![],
                nationalities: vec![],
                programs: vec![],
            },
            CslEntry {
                id: "3".into(),
                source_list: "SDN".into(),
                name: "C".into(),
                aliases: vec![],
                entity_type: EntityType::Entity,
                addresses: vec![],
                nationalities: vec![],
                programs: vec![],
            },
        ];
        let r = build_result(entries, 100, "v");
        assert_eq!(r.source_count, 2);
        assert_eq!(r.entries.len(), 3);
    }
}
