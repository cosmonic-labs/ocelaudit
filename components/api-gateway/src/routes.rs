//! Request routing for the gateway. Pure-data dispatch:
//! `(method, path)` → `RouteResponse`.

use ocelaudit_csl_ingest::{parse_external_json, source_meta};
use ocelaudit_storage_jsonfs::{JsonFsStorage, SearchEvent};
use serde_json::json;

use crate::state::AppState;
use crate::wasi::clocks;
use crate::wasi::http::types::{IncomingRequest, Method};
use crate::RouteResponse;

/// Source path for `/api/v1/csl/refresh`. Wash dev mounts a host
/// directory at `/data`; drop your CSL JSON here and POST refresh. Real
/// HTTP fetch lands later — see README "WASI P3 caveats" for the gap.
const CSL_SEED_PATH: &str = "/data/csl/seed.json";

pub(crate) fn dispatch(
    method: &Method,
    path: &str,
    app: &Result<&'static AppState, String>,
    _req: &IncomingRequest,
) -> RouteResponse {
    let app = match app {
        Ok(a) => *a,
        Err(e) => {
            return match (method, path) {
                (Method::Get, "/") => RouteResponse::plain(200, "ocelaudit booting"),
                _ => RouteResponse::err(503, e.clone()),
            };
        }
    };

    match (method, path) {
        (Method::Get, "/") => RouteResponse::plain(200, "ocelaudit booting"),
        (Method::Get, "/healthz") => RouteResponse::json(200, json!({"ok": true})),

        (Method::Get, "/api/v1/me") => me(app),
        (Method::Get, "/api/v1/audit/_test") => audit_test_get(app),
        (Method::Post, "/api/v1/audit/_test") => audit_test_post(app),

        (Method::Get, "/api/v1/csl/metadata") => csl_metadata(app),
        (Method::Get, "/api/v1/csl/sources") => csl_sources(app),
        (Method::Post, "/api/v1/csl/refresh") => csl_refresh(app),
        (Method::Get, p) if p.starts_with("/api/v1/csl/entries/") => {
            let id = p.trim_start_matches("/api/v1/csl/entries/");
            csl_entry(app, id)
        }

        _ => RouteResponse::json(404, json!({"error": "not found", "path": path})),
    }
}

// ---------- /api/v1/me ----------

fn me(app: &AppState) -> RouteResponse {
    match app.storage.users_get("admin") {
        Ok(Some(u)) => RouteResponse::json(200, json!({"username": u.username, "role": u.role})),
        Ok(None) => RouteResponse::err(500, "users not seeded"),
        Err(e) => RouteResponse::err(500, e.to_string()),
    }
}

// ---------- /api/v1/audit/_test ----------

fn audit_test_get(app: &AppState) -> RouteResponse {
    match app.storage.audit_list_recent(10, 0) {
        Ok(events) => RouteResponse::json(200, json!({"count": events.len(), "events": events})),
        Err(e) => RouteResponse::err(500, e.to_string()),
    }
}

fn audit_test_post(app: &AppState) -> RouteResponse {
    let when = clocks::wall_clock::now().seconds;
    let event_id = format!("debug-{}-{}", when, app.storage.root().display().to_string().len());
    let event = SearchEvent {
        audit_id: event_id.clone(),
        who: "system".into(),
        when,
        query: "synthetic m2 debug write".into(),
        tlp: "green".into(),
        top_hit_ids: vec![],
        decision: "auto-green".into(),
    };
    match app.storage.audit_log(&event) {
        Ok(_) => RouteResponse::json(201, json!({"audit_id": event_id})),
        Err(e) => RouteResponse::err(500, e.to_string()),
    }
}

// ---------- /api/v1/csl/* ----------

fn csl_metadata(app: &AppState) -> RouteResponse {
    match app.storage.csl_metadata() {
        Ok(Some(m)) => RouteResponse::json(
            200,
            json!({
                "fetched_at": m.fetched_at,
                "count": m.count,
                "sources": m.sources,
                "version": m.version,
            }),
        ),
        Ok(None) => RouteResponse::json(
            200,
            json!({"fetched_at": null, "count": 0, "sources": [], "version": null}),
        ),
        Err(e) => RouteResponse::err(500, e.to_string()),
    }
}

fn csl_sources(app: &AppState) -> RouteResponse {
    let metas: Vec<_> = source_meta::ALL
        .iter()
        .map(|m| {
            json!({
                "code": m.code,
                "long_name": m.long_name,
                "agency_url": m.agency_url,
            })
        })
        .collect();
    let counts = match app.storage.csl_metadata() {
        Ok(Some(m)) => m.sources,
        _ => vec![],
    };
    RouteResponse::json(200, json!({"known": metas, "counts": counts}))
}

fn csl_refresh(app: &AppState) -> RouteResponse {
    let bytes = match std::fs::read(CSL_SEED_PATH) {
        Ok(b) => b,
        Err(e) => {
            return RouteResponse::err(
                404,
                format!(
                    "no CSL seed at {}: {}. Drop a JSON file there and try again. \
                     Real HTTP fetch lands in a later milestone.",
                    CSL_SEED_PATH, e
                ),
            )
        }
    };
    let entries = match parse_external_json(&bytes) {
        Ok(e) => e,
        Err(e) => return RouteResponse::err(400, format!("parse failed: {}", e)),
    };
    let count = entries.len();
    let when = clocks::wall_clock::now().seconds;
    let version = format!("seed-{}", when);
    if let Err(e) = app
        .storage
        .csl_bulk_replace(entries, when, version.clone())
    {
        return RouteResponse::err(500, e.to_string());
    }
    RouteResponse::json(
        200,
        json!({
            "ingested": count,
            "fetched_at": when,
            "version": version,
            "source_path": CSL_SEED_PATH,
        }),
    )
}

fn csl_entry(app: &AppState, id: &str) -> RouteResponse {
    if id.is_empty() {
        return RouteResponse::err(400, "missing id");
    }
    match app.storage.csl_get(id) {
        Ok(Some(e)) => RouteResponse::json(
            200,
            json!({
                "id": e.id,
                "name": e.name,
                "aliases": e.aliases,
                "source_list": e.source_list,
                "entity_type": e.entity_type,
                "addresses": e.addresses,
                "nationalities": e.nationalities,
                "programs": e.programs,
            }),
        ),
        Ok(None) => RouteResponse::err(404, format!("entry {} not found", id)),
        Err(e) => RouteResponse::err(500, e.to_string()),
    }
}

// suppress unused-import warning for storage type when we use direct calls
#[allow(dead_code)]
fn _types(_s: &JsonFsStorage) {}
