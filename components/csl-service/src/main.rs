//! OcelAudit CSL service.
//!
//! Long-running wasmCloud service workload (exports `wasi:cli/run`).
//! Listens on `127.0.0.1:7878`. Holds the parsed CSL corpus and the
//! prebuilt search engine in RAM for the lifetime of the host
//! process — eliminates the per-request engine load that the
//! reinstantiated `wasi:http` component pays.
//!
//! Wire protocol: one JSON object per line (request), one JSON
//! object per line (response). Persistent connection — clients can
//! reuse the same socket for many round-trips.
//!
//! Operations:
//!
//! ```jsonc
//! // search → returns top-K hits with snapshot data
//! {"op":"search","q":"Sberbank","limit":20,"sources":null,"entity_types":null,"fuzzy":true}
//!
//! // entry → fetch one record by id
//! {"op":"entry","id":"OFAC-12345"}
//!
//! // stats → corpus aggregates (by_source, by_entity_type, etc.)
//! {"op":"stats"}
//!
//! // refresh → re-read csl.json from disk + rebuild the engine.
//! //           api-gateway calls this after /api/v1/csl/refresh
//! //           writes a new snapshot.
//! {"op":"refresh"}
//!
//! // ping → liveness check
//! {"op":"ping"}
//! ```
//!
//! Every response carries `{"ok": true|false, "data"|"error": ...}`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::time::SystemTime;

use anyhow::Result;
use ocelaudit_search::{EntityType, SearchEngine, SearchParams};
use ocelaudit_storage_jsonfs::JsonFsStorage;
use serde::{Deserialize, Serialize};
use wstd::io::{AsyncRead, AsyncWrite};
use wstd::iter::AsyncIterator;
use wstd::net::TcpListener;

const LISTEN_ADDR: &str = "0.0.0.0:7878"; // wash rewrites 0.0.0.0 → 127.0.0.1
const STORAGE_ROOT: &str = "/data";
const INDEX_CACHE_PATH: &str = "/data/search-index.bin";

#[wstd::main]
async fn main() -> Result<()> {
    let storage = JsonFsStorage::open(STORAGE_ROOT)?;

    // Initial engine: try the postcard cache first (fast warm-boot),
    // fall back to building from csl.json if the cache is missing or
    // stale. Empty engine if both are absent.
    let engine = build_engine(&storage);
    eprintln!(
        "csl-service: ready · listening on {} · n={}",
        LISTEN_ADDR,
        engine.len()
    );
    let engine = RefCell::new(engine);

    let listener = TcpListener::bind(LISTEN_ADDR).await?;
    let mut incoming = listener.incoming();

    while let Some(stream) = incoming.next().await {
        let mut stream = match stream {
            Ok(s) => s,
            Err(e) => {
                eprintln!("csl-service: accept error: {}", e);
                continue;
            }
        };

        let mut buf = vec![0u8; 64 * 1024];
        let mut line_buf: Vec<u8> = Vec::new();
        loop {
            let n = match stream.read(&mut buf).await {
                Ok(n) => n,
                Err(_) => break,
            };
            if n == 0 {
                break;
            }
            for &byte in &buf[..n] {
                if byte == b'\n' {
                    let response = handle_line(&engine, &storage, &line_buf);
                    line_buf.clear();
                    if stream.write_all(&response).await.is_err() {
                        break;
                    }
                    if stream.write_all(b"\n").await.is_err() {
                        break;
                    }
                    if stream.flush().await.is_err() {
                        break;
                    }
                } else {
                    line_buf.push(byte);
                }
            }
        }
    }

    Ok(())
}

fn build_engine(storage: &JsonFsStorage) -> SearchEngine {
    if let Ok(bytes) = std::fs::read(INDEX_CACHE_PATH) {
        match SearchEngine::load_from_bytes(&bytes) {
            Ok(Some(engine)) => {
                eprintln!(
                    "csl-service: warm-loaded {} records from {}",
                    engine.len(),
                    INDEX_CACHE_PATH
                );
                return engine;
            }
            Ok(None) => eprintln!("csl-service: cache version mismatch — rebuilding"),
            Err(e) => eprintln!("csl-service: cache decode failed: {} — rebuilding", e),
        }
    }
    match storage.csl_list_all() {
        Ok(entries) if !entries.is_empty() => {
            let n = entries.len();
            let t = SystemTime::now();
            let engine = SearchEngine::build(entries);
            let elapsed = t.elapsed().map(|d| d.as_millis()).unwrap_or(0);
            eprintln!("csl-service: built engine from csl.json: n={}, {} ms", n, elapsed);
            // Write the cache for next boot.
            if let Ok(bytes) = engine.serialize_to_bytes() {
                let _ = std::fs::write(INDEX_CACHE_PATH, &bytes);
            }
            engine
        }
        _ => {
            eprintln!("csl-service: starting with empty corpus");
            SearchEngine::build(Vec::new())
        }
    }
}

// ---------- request/response shapes ----------

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum Request {
    Ping,
    Refresh,
    Stats,
    Entry {
        id: String,
    },
    Search {
        q: String,
        #[serde(default)]
        sources: Option<Vec<String>>,
        #[serde(default)]
        entity_types: Option<Vec<String>>,
        #[serde(default)]
        fuzzy: Option<bool>,
        #[serde(default)]
        limit: Option<u32>,
    },
    Autocomplete {
        prefix: String,
        #[serde(default)]
        limit: Option<u32>,
    },
}

#[derive(Serialize)]
struct Envelope<T: Serialize> {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn ok<T: Serialize>(data: T) -> Vec<u8> {
    serde_json::to_vec(&Envelope {
        ok: true,
        data: Some(data),
        error: None,
    })
    .unwrap_or_else(|e| serde_json::to_vec(&fail(format!("encode failed: {}", e))).unwrap())
}

fn fail(message: String) -> Envelope<()> {
    Envelope {
        ok: false,
        data: None,
        error: Some(message),
    }
}

fn fail_bytes(message: impl Into<String>) -> Vec<u8> {
    serde_json::to_vec(&fail(message.into())).unwrap()
}

// ---------- dispatch ----------

fn handle_line(engine: &RefCell<SearchEngine>, storage: &JsonFsStorage, line: &[u8]) -> Vec<u8> {
    let req: Request = match serde_json::from_slice(line) {
        Ok(r) => r,
        Err(e) => return fail_bytes(format!("parse: {}", e)),
    };
    match req {
        Request::Ping => ok(serde_json::json!({"pong": true})),
        Request::Refresh => match build_engine_safely(storage) {
            Ok(new_engine) => {
                let n = new_engine.len();
                *engine.borrow_mut() = new_engine;
                ok(serde_json::json!({"reloaded": true, "n": n}))
            }
            Err(e) => fail_bytes(format!("refresh: {}", e)),
        },
        Request::Stats => {
            let e = engine.borrow();
            ok(stats_payload(&e))
        }
        Request::Entry { id } => {
            let e = engine.borrow();
            match e.entry(&id) {
                Some(entry) => ok(serde_json::to_value(entry).unwrap_or(serde_json::Value::Null)),
                None => fail_bytes(format!("entry not found: {}", id)),
            }
        }
        Request::Autocomplete { prefix, limit } => {
            let e = engine.borrow();
            let suggestions = e.autocomplete(&prefix, limit.unwrap_or(8) as usize);
            ok(serde_json::json!({"suggestions": suggestions}))
        }
        Request::Search {
            q,
            sources,
            entity_types,
            fuzzy,
            limit,
        } => {
            let e = engine.borrow();
            let etypes = entity_types.map(|v| {
                v.into_iter()
                    .filter_map(|s| match s.to_ascii_lowercase().as_str() {
                        "individual" => Some(EntityType::Individual),
                        "entity" => Some(EntityType::Entity),
                        "vessel" => Some(EntityType::Vessel),
                        "aircraft" => Some(EntityType::Aircraft),
                        "unknown" => Some(EntityType::Unknown),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            });
            let params = SearchParams {
                q,
                sources,
                entity_types: etypes,
                fuzzy: fuzzy.unwrap_or(true),
                limit: limit.unwrap_or(20) as usize,
            };
            let result = e.search(&params);
            ok(search_response_payload(&e, &result))
        }
    }
}

fn build_engine_safely(storage: &JsonFsStorage) -> Result<SearchEngine, String> {
    let entries = storage.csl_list_all().map_err(|e| e.to_string())?;
    let engine = SearchEngine::build(entries);
    if let Ok(bytes) = engine.serialize_to_bytes() {
        let _ = std::fs::write(INDEX_CACHE_PATH, &bytes);
    }
    Ok(engine)
}

// ---------- payload shapes returned to api-gateway ----------

fn stats_payload(engine: &SearchEngine) -> serde_json::Value {
    let mut by_source: BTreeMap<String, u32> = BTreeMap::new();
    let mut by_entity: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut by_program: BTreeMap<String, u32> = BTreeMap::new();
    let mut by_nationality: BTreeMap<String, u32> = BTreeMap::new();
    let mut with_addresses = 0u32;
    let mut with_aliases = 0u32;
    for e in engine.entries() {
        *by_source.entry(e.source_list.clone()).or_insert(0) += 1;
        *by_entity.entry(entity_token(e.entity_type)).or_insert(0) += 1;
        for p in &e.programs {
            if !p.is_empty() {
                *by_program.entry(p.clone()).or_insert(0) += 1;
            }
        }
        for n in &e.nationalities {
            if !n.is_empty() {
                *by_nationality.entry(n.clone()).or_insert(0) += 1;
            }
        }
        if !e.addresses.is_empty() {
            with_addresses += 1;
        }
        if !e.aliases.is_empty() {
            with_aliases += 1;
        }
    }
    let mut programs: Vec<_> = by_program.into_iter().collect();
    programs.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    programs.truncate(50);
    let mut nationalities: Vec<_> = by_nationality.into_iter().collect();
    nationalities.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    nationalities.truncate(50);
    serde_json::json!({
        "count": engine.len(),
        "by_source": by_source.into_iter().map(|(code, count)| serde_json::json!({"code": code, "count": count})).collect::<Vec<_>>(),
        "by_entity_type": by_entity.into_iter().map(|(k, v)| serde_json::json!({"entity_type": k, "count": v})).collect::<Vec<_>>(),
        "top_programs": programs.iter().map(|(name, count)| serde_json::json!({"name": name, "count": count})).collect::<Vec<_>>(),
        "top_nationalities": nationalities.iter().map(|(name, count)| serde_json::json!({"name": name, "count": count})).collect::<Vec<_>>(),
        "with_addresses": with_addresses,
        "with_aliases": with_aliases,
    })
}

fn search_response_payload(
    engine: &SearchEngine,
    r: &ocelaudit_search::engine::SearchResult,
) -> serde_json::Value {
    let hits: Vec<_> = r
        .hits
        .iter()
        .map(|h| {
            let entry = engine.entry(&h.entry_id);
            serde_json::json!({
                "entry_id": h.entry_id,
                "score": h.score,
                "tlp": tlp_token(h.tlp),
                "matched_fields": h.matched_fields,
                "snippet": h.snippet,
                "entry": entry.map(|e| serde_json::to_value(e).ok()).unwrap_or(None),
            })
        })
        .collect();
    serde_json::json!({
        "tlp": tlp_token(r.tlp),
        "exact_alias_match": r.exact_alias_match,
        "hits": hits,
    })
}

fn tlp_token(t: ocelaudit_search::Tlp) -> &'static str {
    match t {
        ocelaudit_search::Tlp::Green => "green",
        ocelaudit_search::Tlp::Yellow => "yellow",
        ocelaudit_search::Tlp::Red => "red",
    }
}

fn entity_token(t: EntityType) -> &'static str {
    match t {
        EntityType::Individual => "individual",
        EntityType::Entity => "entity",
        EntityType::Vessel => "vessel",
        EntityType::Aircraft => "aircraft",
        EntityType::Unknown => "unknown",
    }
}
