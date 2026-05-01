//! Request routing for the gateway.

use ocelaudit_csl_ingest::{parse_external_json, source_meta};
use ocelaudit_search::{CslEntry, EntityType, SearchParams, Tlp};
use ocelaudit_storage_jsonfs::{
    HitCitation, HitSnapshot, HitTags, Role, SearchEvent, WorkflowEntry,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::{clear_cookie, extract_session_cookie, set_cookie, Session};
use crate::state::AppState;
use crate::static_assets;
use crate::wasi::clocks;
use crate::wasi::clocks::monotonic_clock;
use crate::wasi::http::types::Method;
use crate::RouteResponse;

/// Returns a label-with-elapsed-ms that can be eprintln!'d for
/// quick stderr timing. Single-threaded wasm; no need for syncing.
fn ms_since(start_ns: u64) -> u64 {
    (monotonic_clock::now() - start_ns) / 1_000_000
}

const CSL_SEED_PATH: &str = "/data/csl/seed.json";

/// One bag of "everything we already know about the request" so route
/// handlers can be plain functions instead of dragging IncomingRequest
/// around.
pub(crate) struct DispatchInput<'a> {
    pub method: &'a Method,
    pub path: &'a str,
    pub query_string: Option<&'a str>,
    pub cookie_header: Option<&'a str>,
    pub source: &'a str,
    pub body: Option<&'a [u8]>,
    pub app: &'a Result<&'static AppState, String>,
}

pub(crate) fn dispatch(in_: DispatchInput<'_>) -> RouteResponse {
    let app = match in_.app {
        Ok(a) => *a,
        Err(e) => {
            return match (in_.method, in_.path) {
                (Method::Get, "/") => RouteResponse::plain(200, "ocelaudit booting"),
                _ => RouteResponse::err(503, e.clone()),
            };
        }
    };

    let session = current_session(app, in_.cookie_header);

    match (in_.method, in_.path) {
        // -- public --
        (Method::Get, "/healthz") => RouteResponse::json(200, json!({"ok": true})),
        (Method::Post, "/api/v1/auth/login") => login(app, in_.body),
        (Method::Post, "/api/v1/auth/logout") => logout(),

        // -- always-public CSL surface (helps the unauth login page hint) --
        (Method::Get, "/api/v1/csl/sources") => csl_sources(app),

        // -- always-public branding (login page needs the logo) --
        (Method::Get, "/api/v1/branding") => branding(),

        // -- static SPA assets, served from /data/static/ --
        (Method::Get, "/") => static_assets::serve("/")
            .unwrap_or_else(|| RouteResponse::plain(200, "ocelaudit booting")),
        (Method::Get, p) if !p.starts_with("/api/") && !p.starts_with("/healthz") => {
            // Try the literal path; fall back to SPA index.html so the
            // client-side router can take over for routes like
            // /dashboard, /search, etc.
            static_assets::serve(p)
                .or_else(static_assets::spa_fallback)
                .unwrap_or_else(|| {
                    RouteResponse::json(404, json!({"error": "not found", "path": p}))
                })
        }

        // -- everything else needs auth --
        _ => match session {
            Some(s) => dispatch_authed(app, &s, in_),
            None => RouteResponse::err(401, "authentication required"),
        },
    }
}

fn dispatch_authed(
    app: &AppState,
    session: &Session,
    in_: DispatchInput<'_>,
) -> RouteResponse {
    match (in_.method, in_.path) {
        (Method::Get, "/api/v1/me") => me(session),

        (Method::Get, "/api/v1/csl/metadata") => csl_metadata(app),
        (Method::Get, "/api/v1/csl/stats") => csl_stats(app),
        (Method::Post, "/api/v1/csl/refresh") => {
            require_admin(session).unwrap_or_else(|| csl_refresh(app, in_.query_string))
        }
        (Method::Get, p) if p.starts_with("/api/v1/csl/entries/") => {
            let id = p.trim_start_matches("/api/v1/csl/entries/");
            csl_entry(app, id)
        }

        (Method::Post, "/api/v1/search") => search(app, session, in_.body, in_.source, None, None),
        (Method::Get, "/api/v1/search/autocomplete") => {
            autocomplete(app, in_.query_string)
        }

        (Method::Post, "/api/v1/screen/ofac") => screen_ofac(app, session, in_.body, in_.source),
        (Method::Post, "/api/v1/screen/pep") => screen_pep(app, session, in_.body, in_.source),

        (Method::Get, "/api/v1/audit") => audit_list(app, in_.query_string),
        (Method::Get, p) if p.starts_with("/api/v1/audit/") => {
            audit_get(app, p.trim_start_matches("/api/v1/audit/"))
        }

        (Method::Post, p) if p.starts_with("/api/v1/review/") && p.ends_with("/decide") => {
            let id = p
                .trim_start_matches("/api/v1/review/")
                .trim_end_matches("/decide");
            review_decide(app, session, id, in_.body)
        }
        (Method::Get, "/api/v1/review") => review_queue(app, in_.query_string),

        (Method::Get, "/api/v1/metrics") => metrics(app),

        _ => RouteResponse::json(404, json!({"error": "not found", "path": in_.path})),
    }
}

// ---------- session middleware ----------

fn current_session(app: &AppState, cookie_header: Option<&str>) -> Option<Session> {
    let header = cookie_header?;
    let raw = extract_session_cookie(header)?;
    app.signer.verify(raw)
}

fn require_admin(session: &Session) -> Option<RouteResponse> {
    if session.role == Role::Admin {
        None
    } else {
        Some(RouteResponse::err(403, "admin role required"))
    }
}

// ---------- /api/v1/auth ----------

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

fn login(app: &AppState, body: Option<&[u8]>) -> RouteResponse {
    let body = match body {
        Some(b) if !b.is_empty() => b,
        _ => return RouteResponse::err(400, "missing JSON body"),
    };
    let creds: LoginBody = match serde_json::from_slice(body) {
        Ok(c) => c,
        Err(e) => return RouteResponse::err(400, format!("bad JSON: {}", e)),
    };
    let ok = match app.storage.users_verify(&creds.username, &creds.password) {
        Ok(o) => o,
        Err(e) => return RouteResponse::err(500, e.to_string()),
    };
    let user = match ok {
        Some(u) => u,
        None => return RouteResponse::err(401, "invalid credentials"),
    };
    let issued_at = clocks::wall_clock::now().seconds;
    let session = Session {
        username: user.username.clone(),
        role: user.role.clone(),
        issued_at,
    };
    let cookie_value = match app.signer.issue(&session) {
        Ok(s) => s,
        Err(e) => return RouteResponse::err(500, format!("issue: {}", e)),
    };
    RouteResponse::json(
        200,
        json!({"username": user.username, "role": user.role}),
    )
    .with_cookie(set_cookie(&cookie_value))
}

fn logout() -> RouteResponse {
    RouteResponse::json(200, json!({"ok": true})).with_cookie(clear_cookie())
}

// ---------- /api/v1/me ----------

fn me(session: &Session) -> RouteResponse {
    RouteResponse::json(
        200,
        json!({"username": session.username, "role": session.role, "iat": session.issued_at}),
    )
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

fn csl_stats(app: &AppState) -> RouteResponse {
    let metadata = match app.storage.csl_metadata() {
        Ok(m) => m,
        Err(e) => return RouteResponse::err(500, e.to_string()),
    };
    let entries = match app.storage.csl_list_all() {
        Ok(e) => e,
        Err(e) => return RouteResponse::err(500, e.to_string()),
    };

    // Aggregate per-source (with agency URLs), per-entity-type, top
    // programs, top nationalities. Single linear scan over the corpus.
    use std::collections::BTreeMap;
    let mut by_source: BTreeMap<String, u32> = BTreeMap::new();
    let mut by_entity: BTreeMap<&'static str, u32> = BTreeMap::new();
    let mut by_program: BTreeMap<String, u32> = BTreeMap::new();
    let mut by_nationality: BTreeMap<String, u32> = BTreeMap::new();
    let mut with_addresses = 0u32;
    let mut with_aliases = 0u32;
    for e in &entries {
        *by_source.entry(e.source_list.clone()).or_insert(0) += 1;
        *by_entity.entry(entity_type_token(e.entity_type)).or_insert(0) += 1;
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

    let by_source_json: Vec<_> = by_source
        .iter()
        .map(|(code, count)| {
            let meta = source_meta::meta_for_code(code);
            json!({
                "code": code,
                "count": count,
                "long_name": meta.map(|m| m.long_name),
                "agency_url": meta.map(|m| m.agency_url),
            })
        })
        .collect();
    let by_entity_json: Vec<_> = by_entity
        .iter()
        .map(|(k, v)| json!({"entity_type": k, "count": v}))
        .collect();
    // Top 25 programs / nationalities by frequency.
    let mut programs_sorted: Vec<_> = by_program.into_iter().collect();
    programs_sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    programs_sorted.truncate(25);
    let mut nationalities_sorted: Vec<_> = by_nationality.into_iter().collect();
    nationalities_sorted.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    nationalities_sorted.truncate(25);

    let (count, fetched_at, version) = match metadata {
        Some(m) => (m.count, Some(m.fetched_at), Some(m.version)),
        None => (0, None, None),
    };

    RouteResponse::json(
        200,
        json!({
            "count": count,
            "fetched_at": fetched_at,
            "version": version,
            "by_source": by_source_json,
            "by_entity_type": by_entity_json,
            "top_programs": programs_sorted.iter().map(|(name, count)| json!({"name": name, "count": count})).collect::<Vec<_>>(),
            "top_nationalities": nationalities_sorted.iter().map(|(name, count)| json!({"name": name, "count": count})).collect::<Vec<_>>(),
            "with_addresses": with_addresses,
            "with_aliases": with_aliases,
        }),
    )
}

fn csl_sources(app: &AppState) -> RouteResponse {
    let metas: Vec<_> = source_meta::ALL
        .iter()
        .map(|m| {
            json!({"code": m.code, "long_name": m.long_name, "agency_url": m.agency_url})
        })
        .collect();
    let counts = match app.storage.csl_metadata() {
        Ok(Some(m)) => m.sources,
        _ => vec![],
    };
    RouteResponse::json(200, json!({"known": metas, "counts": counts}))
}

fn csl_refresh(app: &AppState, query: Option<&str>) -> RouteResponse {
    let when = clocks::wall_clock::now().seconds;

    // `?source=seed` forces the seed-file path (skipping the live HTTP
    // fetch). Used by the test suite (which expects deterministic
    // synthetic data) and by anyone who's pre-staged a specific
    // snapshot at /data/csl/seed.json. Default: try live first.
    let force_seed = query
        .map(|q| q.split('&').any(|kv| kv == "source=seed"))
        .unwrap_or(false);

    let (bytes, source, fetch_warning) = if force_seed {
        (Vec::new(), "seed.json", None)
    } else {
        match crate::csl_fetch::fetch_consolidated_json() {
            Ok(ok) if ok.status == 200 && !ok.bytes.is_empty() => {
                (ok.bytes, "trade.gov", None)
            }
            Ok(ok) => (
                Vec::new(),
                "trade.gov",
                Some(format!(
                    "live fetch returned http {} / {} bytes",
                    ok.status,
                    ok.bytes.len()
                )),
            ),
            Err(e) => (Vec::new(), "trade.gov", Some(format!("live fetch failed: {}", e))),
        }
    };

    let (bytes, source, warning) = if !bytes.is_empty() {
        (bytes, source, fetch_warning)
    } else {
        match std::fs::read(CSL_SEED_PATH) {
            Ok(b) => (b, "seed.json", fetch_warning),
            Err(e) => {
                return RouteResponse::err(
                    503,
                    format!(
                        "{}; seed fallback at {} also unreadable: {}",
                        fetch_warning.unwrap_or_else(|| "live fetch failed".into()),
                        CSL_SEED_PATH,
                        e
                    ),
                );
            }
        }
    };

    let entries = match parse_external_json(&bytes) {
        Ok(e) => e,
        Err(e) => return RouteResponse::err(400, format!("parse failed: {}", e)),
    };
    let count = entries.len();
    let version = format!("{}-{}", source, when);
    if let Err(e) = app
        .storage
        .csl_bulk_replace(entries, when, version.clone())
    {
        return RouteResponse::err(500, e.to_string());
    }
    app.invalidate_engine();

    // Eagerly rebuild the search index right here, on the operator's
    // refresh click, so the first user-facing /search lands on a warm
    // cache instead of paying the ~1 s cold-build cost. The Admin UI
    // already shows a spinner; this just lets that spinner cover the
    // build, not the next person's search bar.
    let t_index_start = monotonic_clock::now();
    let mut index_built_ms: Option<u64> = None;
    let mut index_error: Option<String> = None;
    match app.ensure_engine() {
        Ok(engine) => {
            // Drop the borrow before computing elapsed.
            let n = engine.len();
            drop(engine);
            let elapsed = (monotonic_clock::now() - t_index_start) / 1_000_000;
            index_built_ms = Some(elapsed);
            eprintln!(
                "ocelaudit: post-refresh index rebuild: {} ms · n={}",
                elapsed, n
            );
        }
        Err(e) => {
            index_error = Some(e.clone());
            eprintln!("ocelaudit: post-refresh engine rebuild failed: {}", e);
        }
    }

    let mut body = json!({
        "ingested": count,
        "fetched_at": when,
        "version": version,
        "source": source,
        "index_built_ms": index_built_ms,
    });
    if let Some(w) = warning {
        body.as_object_mut().unwrap().insert("warning".into(), json!(w));
    }
    if let Some(e) = index_error {
        body.as_object_mut()
            .unwrap()
            .insert("index_error".into(), json!(e));
    }
    RouteResponse::json(200, body)
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

// ---------- /api/v1/search ----------

#[derive(Deserialize, Default)]
struct SearchBody {
    q: String,
    #[serde(default)]
    sources: Option<Vec<String>>,
    #[serde(default)]
    entity_types: Option<Vec<String>>,
    #[serde(default)]
    fuzzy: Option<bool>,
    #[serde(default)]
    limit: Option<u32>,
}

/// `/api/v1/branding` config path. The SPA reads this on boot to drive
/// logo/wordmark/colors. Drop a JSON file at this path under the wash
/// dev volume mount to override; missing or malformed = defaults.
const BRANDING_CONFIG_PATH: &str = "/data/static/ocelaudit.config.json";

fn branding() -> RouteResponse {
    let default = json!({
        "logo_url": "/brand/ocelot.svg",
        "wordmark": "OcelAudit",
        "video_url": null,
        "primary_color": "#1f2937",
        "accent_color": "#b45309",
    });
    let body = match std::fs::read(BRANDING_CONFIG_PATH) {
        Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(mut v) => {
                // Merge: any missing key in the file falls back to default.
                let obj = v.as_object_mut();
                let def = default.as_object().unwrap();
                if let Some(o) = obj {
                    for (k, dv) in def {
                        o.entry(k.clone()).or_insert(dv.clone());
                    }
                }
                v
            }
            Err(_) => default,
        },
        Err(_) => default,
    };
    RouteResponse::json(200, body)
}

/// OFAC-issued lists per Treasury Department.
const OFAC_SOURCES: &[&str] = &["SDN", "NS-MBS", "NS-ISA", "FSE", "SSI", "CAPTA", "PLC"];

/// Best-effort PEP-style filter. The CSL is *not* a true PEP feed; the
/// closest public-list signal is the Palestinian Legislative Council
/// list, which is name-of-officials shaped. Honest disclosure shipped
/// alongside the response.
const PEP_SOURCES: &[&str] = &["PLC"];

fn search(
    app: &AppState,
    session: &Session,
    body: Option<&[u8]>,
    source: &str,
    forced_sources: Option<Vec<String>>,
    note: Option<&str>,
) -> RouteResponse {
    let body = match body {
        Some(b) if !b.is_empty() => b,
        _ => return RouteResponse::err(400, "missing JSON body"),
    };
    let req: SearchBody = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(e) => return RouteResponse::err(400, format!("bad JSON: {}", e)),
    };
    let etypes = req.entity_types.as_ref().map(|v| {
        v.iter()
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
    let sources = forced_sources.or(req.sources.clone());
    let params = SearchParams {
        q: req.q.clone(),
        sources,
        entity_types: etypes,
        fuzzy: req.fuzzy.unwrap_or(true),
        limit: req.limit.unwrap_or(20) as usize,
    };

    let t_start = monotonic_clock::now();
    let engine = match app.ensure_engine() {
        Ok(e) => e,
        Err(e) => return RouteResponse::err(500, e),
    };
    let t_engine_ready = monotonic_clock::now();
    let result = engine.search(&params);
    let t_search_done = monotonic_clock::now();

    let audit_id = Uuid::now_v7().to_string();
    let when = clocks::wall_clock::now().seconds;
    let top_ids: Vec<String> = result.hits.iter().take(5).map(|h| h.entry_id.clone()).collect();
    let tlp_str = tlp_token(result.tlp);
    // RED with an exact name/alias match goes auto-block (no review
    // needed — the entity is on the list verbatim). RED with high
    // similarity but not exact stays pending-block (compliance reviews).
    // YELLOW always pending-review. GREEN auto-clears.
    let decision = match result.tlp {
        Tlp::Green => "auto-green",
        Tlp::Yellow => "pending-review",
        Tlp::Red if result.exact_alias_match => "auto-block",
        Tlp::Red => "pending-block",
    };
    // Snapshot the top-K hits with their entry-level metadata. This is
    // both what we serialize to the response AND what we persist to
    // audit so the review UI later can show reviewers exactly what the
    // engine returned at search time.
    let snapshots: Vec<HitSnapshot> = result
        .hits
        .iter()
        .map(|h| build_hit_snapshot(&engine, h))
        .collect();
    let t_snapshots = monotonic_clock::now();

    let event = SearchEvent {
        audit_id: audit_id.clone(),
        who: session.username.clone(),
        when,
        query: params.q.clone(),
        tlp: tlp_str.into(),
        top_hit_ids: top_ids.clone(),
        decision: decision.into(),
        source: source.to_string(),
        top_hits: snapshots.iter().take(5).cloned().collect(),
    };
    if let Err(e) = app.storage.audit_log(&event) {
        return RouteResponse::err(500, format!("audit_log: {}", e));
    }
    let t_audit_logged = monotonic_clock::now();

    let hits_json: Vec<_> = snapshots.iter().map(snapshot_to_json).collect();
    eprintln!(
        "ocelaudit: search timing: engine={} ms, search={} ms, snapshot={} ms, audit_log={} ms, total_so_far={} ms · q='{}' · hits={}",
        ms_since(t_start) - ms_since(t_engine_ready),
        ms_since(t_engine_ready) - ms_since(t_search_done),
        ms_since(t_search_done) - ms_since(t_snapshots),
        ms_since(t_snapshots) - ms_since(t_audit_logged),
        ms_since(t_start),
        params.q,
        result.hits.len(),
    );

    let mut body_obj = json!({
        "audit_id": audit_id,
        "tlp": tlp_str,
        "decision": decision,
        "hits": hits_json,
    });
    if let Some(n) = note {
        body_obj
            .as_object_mut()
            .unwrap()
            .insert("note".into(), json!(n));
    }
    RouteResponse::json(200, body_obj)
}

fn screen_ofac(
    app: &AppState,
    session: &Session,
    body: Option<&[u8]>,
    source: &str,
) -> RouteResponse {
    let forced = OFAC_SOURCES.iter().map(|s| s.to_string()).collect();
    search(
        app,
        session,
        body,
        source,
        Some(forced),
        Some("Filter restricted to OFAC-issued source lists (SDN, NS-MBS, NS-ISA, FSE, SSI, CAPTA, PLC)."),
    )
}

fn screen_pep(
    app: &AppState,
    session: &Session,
    body: Option<&[u8]>,
    source: &str,
) -> RouteResponse {
    let forced = PEP_SOURCES.iter().map(|s| s.to_string()).collect();
    search(
        app,
        session,
        body,
        source,
        Some(forced),
        Some(
            "DISCLAIMER: this is not a true PEP feed. The CSL doesn't include PEP per se; \
             we approximate by filtering to PLC (Palestinian Legislative Council) and \
             other publicly-listed officials. Use a dedicated PEP database for compliance.",
        ),
    )
}

fn tlp_token(t: Tlp) -> &'static str {
    match t {
        Tlp::Green => "green",
        Tlp::Yellow => "yellow",
        Tlp::Red => "red",
    }
}

fn entity_type_token(t: EntityType) -> &'static str {
    match t {
        EntityType::Individual => "individual",
        EntityType::Entity => "entity",
        EntityType::Vessel => "vessel",
        EntityType::Aircraft => "aircraft",
        EntityType::Unknown => "unknown",
    }
}

/// Build the per-hit snapshot we surface to clients and persist into
/// the audit row. Pulls the entry's source_list / entity_type /
/// programs / nationalities from the in-memory engine — going through
/// `storage.csl_get` here means re-reading + re-parsing 31 MB of
/// csl.json per hit, which made each /search take ~5 s. Engine
/// lookup is O(1) via the by-id index.
fn build_hit_snapshot(engine: &ocelaudit_search::SearchEngine, h: &ocelaudit_search::Hit) -> HitSnapshot {
    let entry: Option<&CslEntry> = engine.entry(&h.entry_id);
    let citation = entry.and_then(|e| {
        source_meta::meta_for_code(&e.source_list).map(|m| HitCitation {
            source_code: m.code.into(),
            long_name: m.long_name.into(),
            agency_url: m.agency_url.into(),
        })
    });
    let tags = match entry {
        Some(e) => HitTags {
            source_list: e.source_list.clone(),
            entity_type: entity_type_token(e.entity_type).into(),
            programs: e.programs.clone(),
            nationalities: e.nationalities.clone(),
        },
        None => HitTags::default(),
    };
    HitSnapshot {
        entry_id: h.entry_id.clone(),
        score: h.score,
        tlp: tlp_token(h.tlp).into(),
        matched_fields: h.matched_fields.clone(),
        snippet: h.snippet.clone(),
        citation,
        tags,
    }
}

fn snapshot_to_json(s: &HitSnapshot) -> serde_json::Value {
    let citation = match &s.citation {
        Some(c) => json!({
            "source_code": c.source_code,
            "long_name": c.long_name,
            "agency_url": c.agency_url,
        }),
        None => json!(null),
    };
    json!({
        "entry_id": s.entry_id,
        "score": s.score,
        "tlp": s.tlp,
        "matched_fields": s.matched_fields,
        "snippet": s.snippet,
        "citation": citation,
        "tags": {
            "source_list": s.tags.source_list,
            "entity_type": s.tags.entity_type,
            "programs": s.tags.programs,
            "nationalities": s.tags.nationalities,
        },
    })
}

fn autocomplete(app: &AppState, query: Option<&str>) -> RouteResponse {
    let prefix = query
        .and_then(|q| {
            q.split('&')
                .find_map(|kv| kv.strip_prefix("q="))
                .map(|s| urldecode(s))
        })
        .unwrap_or_default();
    if prefix.is_empty() {
        return RouteResponse::json(200, json!([]));
    }
    let engine = match app.ensure_engine() {
        Ok(e) => e,
        Err(e) => return RouteResponse::err(500, e),
    };
    let suggestions = engine.autocomplete(&prefix, 8);
    RouteResponse::json(200, json!(suggestions))
}

/// Tiny percent-decoder for query strings. Handles `%XX` and `+`. Does
/// not implement full RFC 3986; the gateway only cares about ASCII
/// search prefixes.
fn urldecode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                match (hi, lo) {
                    (Some(h), Some(l)) => {
                        out.push((h * 16 + l) as u8);
                        i += 3;
                    }
                    _ => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ---------- /api/v1/audit ----------

fn audit_list(app: &AppState, query: Option<&str>) -> RouteResponse {
    let (limit, offset) = parse_paging(query);
    match app.storage.audit_list_recent(limit, offset) {
        Ok(events) => RouteResponse::json(
            200,
            json!({"limit": limit, "offset": offset, "events": events}),
        ),
        Err(e) => RouteResponse::err(500, e.to_string()),
    }
}

fn audit_get(app: &AppState, id: &str) -> RouteResponse {
    if id.is_empty() {
        return RouteResponse::err(400, "missing id");
    }
    let event = match app.storage.audit_get(id) {
        Ok(Some(e)) => e,
        Ok(None) => return RouteResponse::err(404, format!("audit_id {} not found", id)),
        Err(e) => return RouteResponse::err(500, e.to_string()),
    };
    // Workflow history. The current decision is either the latest
    // workflow entry's decision, or the search event's auto-decision
    // if no review has happened yet.
    let history = app.storage.workflow_history(id).unwrap_or_default();
    let current = history
        .last()
        .map(|h| h.decision.clone())
        .unwrap_or_else(|| event.decision.clone());
    RouteResponse::json(
        200,
        json!({
            "audit_id": event.audit_id,
            "who": event.who,
            "when": event.when,
            "query": event.query,
            "tlp": event.tlp,
            "top_hit_ids": event.top_hit_ids,
            "top_hits": event.top_hits.iter().map(snapshot_to_json).collect::<Vec<_>>(),
            "decision": current,
            "initial_decision": event.decision,
            "source": event.source,
            "history": history,
        }),
    )
}

fn parse_paging(query: Option<&str>) -> (usize, usize) {
    let mut limit = 50usize;
    let mut offset = 0usize;
    if let Some(q) = query {
        for kv in q.split('&') {
            if let Some(v) = kv.strip_prefix("limit=") {
                if let Ok(n) = v.parse::<usize>() {
                    limit = n.clamp(1, 500);
                }
            }
            if let Some(v) = kv.strip_prefix("offset=") {
                if let Ok(n) = v.parse::<usize>() {
                    offset = n.clamp(0, 100_000);
                }
            }
        }
    }
    (limit, offset)
}

// ---------- /api/v1/metrics ----------

fn metrics(app: &AppState) -> RouteResponse {
    let csl_count = app
        .storage
        .csl_metadata()
        .ok()
        .flatten()
        .map(|m| m.count)
        .unwrap_or(0);
    let sources = app
        .storage
        .csl_metadata()
        .ok()
        .flatten()
        .map(|m| m.sources)
        .unwrap_or_default();
    let recent = app.storage.audit_list_recent(1000, 0).unwrap_or_default();
    let mut tlp_red = 0u32;
    let mut tlp_yellow = 0u32;
    let mut tlp_green = 0u32;
    for e in &recent {
        match e.tlp.as_str() {
            "red" => tlp_red += 1,
            "yellow" => tlp_yellow += 1,
            "green" => tlp_green += 1,
            _ => {}
        }
    }
    let last_refresh = app
        .storage
        .csl_metadata()
        .ok()
        .flatten()
        .map(|m| m.fetched_at)
        .unwrap_or(0);
    RouteResponse::json(
        200,
        json!({
            "csl_count": csl_count,
            "csl_sources": sources,
            "queries_recent": recent.len(),
            "tlp_histogram": {"red": tlp_red, "yellow": tlp_yellow, "green": tlp_green},
            "last_csl_refresh": last_refresh,
            "queue_depth": 0,
        }),
    )
}

// ---------- /api/v1/review ----------

#[derive(Deserialize)]
struct DecideBody {
    decision: String,
    #[serde(default)]
    note: Option<String>,
}

fn review_decide(
    app: &AppState,
    session: &Session,
    audit_id: &str,
    body: Option<&[u8]>,
) -> RouteResponse {
    if audit_id.is_empty() {
        return RouteResponse::err(400, "missing audit id in path");
    }
    let body = match body {
        Some(b) if !b.is_empty() => b,
        _ => return RouteResponse::err(400, "missing JSON body"),
    };
    let req: DecideBody = match serde_json::from_slice(body) {
        Ok(r) => r,
        Err(e) => return RouteResponse::err(400, format!("bad JSON: {}", e)),
    };
    let decision = match req.decision.to_ascii_lowercase().as_str() {
        "cleared" => "cleared",
        "blocked" => "blocked",
        other => {
            return RouteResponse::err(
                400,
                format!("decision must be 'cleared' or 'blocked', got '{}'", other),
            )
        }
    };
    if app.storage.audit_get(audit_id).ok().flatten().is_none() {
        return RouteResponse::err(404, format!("audit_id {} not found", audit_id));
    }
    let entry = WorkflowEntry {
        audit_id: audit_id.into(),
        decision: decision.into(),
        decided_by: session.username.clone(),
        decided_at: clocks::wall_clock::now().seconds,
        note: req.note.clone(),
    };
    if let Err(e) = app.storage.workflow_log(&entry) {
        return RouteResponse::err(500, format!("workflow_log: {}", e));
    }
    RouteResponse::json(
        200,
        json!({
            "audit_id": audit_id,
            "decision": decision,
            "decided_by": entry.decided_by,
            "decided_at": entry.decided_at,
        }),
    )
}

/// Queue of audit events still pending decision. By default only
/// items whose final decision is `pending-review` or `pending-block`
/// (i.e. they need human attention). Pass `?include=auto` to also
/// include `auto-block` items (exact-match RED hits that were
/// already decided automatically — useful for spot-checking that
/// the right entities are being auto-decided).
fn review_queue(app: &AppState, query: Option<&str>) -> RouteResponse {
    let include_auto = query
        .map(|q| q.split('&').any(|kv| kv == "include=auto" || kv == "include=auto-block"))
        .unwrap_or(false);

    let recent = match app.storage.audit_list_recent(500, 0) {
        Ok(r) => r,
        Err(e) => return RouteResponse::err(500, e.to_string()),
    };
    let mut pending = Vec::new();
    for ev in recent {
        // If a review decision has been made, that wins.
        let final_decision = app
            .storage
            .workflow_history(&ev.audit_id)
            .ok()
            .and_then(|h| h.last().map(|w| w.decision.clone()))
            .unwrap_or(ev.decision.clone());
        let in_queue = final_decision.starts_with("pending-")
            || (include_auto && final_decision == "auto-block");
        if in_queue {
            pending.push(json!({
                "audit_id": ev.audit_id,
                "who": ev.who,
                "when": ev.when,
                "query": ev.query,
                "tlp": ev.tlp,
                "decision": final_decision,
                "top_hit_ids": ev.top_hit_ids,
                "top_hits": ev.top_hits.iter().map(snapshot_to_json).collect::<Vec<_>>(),
                "source": ev.source,
            }));
        }
    }
    RouteResponse::json(200, json!({"count": pending.len(), "items": pending}))
}
