wit_bindgen::generate!({
    world: "api-gateway",
    generate_all,
});

use std::cell::OnceCell;

use exports::wasi::http::incoming_handler::Guest;
use ocelaudit_storage_jsonfs::SearchEvent;
use serde_json::json;
use wasi::http::types::{
    Fields, IncomingRequest, Method, OutgoingBody, OutgoingResponse, ResponseOutparam,
};

mod state;
use state::AppState;

struct Component;

thread_local! {
    static APP: OnceCell<AppState> = const { OnceCell::new() };
}

/// Lazy-init the app state on first request. Returns `Err(reason)` if
/// startup config is invalid (e.g. STORAGE_BACKEND missing/bad) — the
/// gateway responds 503 with the reason rather than crashing the host.
fn app() -> Result<&'static AppState, String> {
    APP.with(|cell| -> Result<&'static AppState, String> {
        // OnceCell isn't `'static` directly; SAFETY: thread_local in a
        // single-threaded wasm runtime — the cell lives for the entire
        // process. We hand out a 'static reference deliberately.
        if cell.get().is_none() {
            let s = AppState::startup()?;
            let _ = cell.set(s);
        }
        let r: *const AppState = cell.get().unwrap();
        // SAFETY: see above — single-threaded wasm, OnceCell never resets.
        Ok(unsafe { &*r })
    })
}

impl Guest for Component {
    fn handle(request: IncomingRequest, response_out: ResponseOutparam) {
        let method = request.method();
        let path = request.path_with_query().unwrap_or_else(|| "/".into());
        let path = path.split('?').next().unwrap_or("/").to_string();

        let app_result = app();

        let (status, body, content_type) = match (&method, path.as_str()) {
            (Method::Get, "/") => (200, "ocelaudit booting".into(), "text/plain"),
            (Method::Get, "/healthz") => match &app_result {
                Ok(_) => (200, json!({"ok": true}).to_string(), "application/json"),
                Err(e) => (
                    503,
                    json!({"ok": false, "error": e}).to_string(),
                    "application/json",
                ),
            },
            (Method::Get, "/api/v1/me") => handle_me(app_result),
            (Method::Get, "/api/v1/audit/_test") => handle_audit_test_get(app_result),
            (Method::Post, "/api/v1/audit/_test") => {
                handle_audit_test_post(app_result, &request)
            }
            _ => (
                404,
                json!({"error": "not found", "path": path}).to_string(),
                "application/json",
            ),
        };

        write_response(response_out, status, content_type, body.as_bytes());
    }
}

fn handle_me(app: Result<&AppState, String>) -> (u16, String, &'static str) {
    let app = match app {
        Ok(a) => a,
        Err(e) => {
            return (
                503,
                json!({"error": e}).to_string(),
                "application/json",
            )
        }
    };
    // M2: there is no auth yet (lands in M4). Return the seeded admin as
    // "the current user" so the demo has something to display.
    match app.storage.users_get("admin") {
        Ok(Some(u)) => {
            let body = json!({
                "username": u.username,
                "role": u.role,
            });
            (200, body.to_string(), "application/json")
        }
        Ok(None) => (
            500,
            json!({"error": "users not seeded"}).to_string(),
            "application/json",
        ),
        Err(e) => (
            500,
            json!({"error": e.to_string()}).to_string(),
            "application/json",
        ),
    }
}

fn handle_audit_test_get(app: Result<&AppState, String>) -> (u16, String, &'static str) {
    let app = match app {
        Ok(a) => a,
        Err(e) => return (503, json!({"error": e}).to_string(), "application/json"),
    };
    match app.storage.audit_list_recent(10, 0) {
        Ok(events) => {
            let body = json!({
                "count": events.len(),
                "events": events,
            });
            (200, body.to_string(), "application/json")
        }
        Err(e) => (
            500,
            json!({"error": e.to_string()}).to_string(),
            "application/json",
        ),
    }
}

fn handle_audit_test_post(
    app: Result<&AppState, String>,
    _req: &IncomingRequest,
) -> (u16, String, &'static str) {
    let app = match app {
        Ok(a) => a,
        Err(e) => return (503, json!({"error": e}).to_string(), "application/json"),
    };
    // M2 doesn't read the request body; the endpoint exists to prove the
    // storage write path through wasi:filesystem. Real /api/v1/search
    // (M4) will read the JSON body, run the search, and produce a real
    // SearchEvent.
    let now = wasi::clocks::wall_clock::now();
    let when = now.seconds;
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
        Ok(_) => {
            let body = json!({"audit_id": event_id});
            (201, body.to_string(), "application/json")
        }
        Err(e) => (
            500,
            json!({"error": e.to_string()}).to_string(),
            "application/json",
        ),
    }
}

fn write_response(out: ResponseOutparam, status: u16, content_type: &str, body: &[u8]) {
    let headers = Fields::new();
    let _ = headers.set(&"content-type".to_string(), &[content_type.as_bytes().to_vec()]);
    let resp = OutgoingResponse::new(headers);
    let _ = resp.set_status_code(status);
    let outgoing_body = resp.body().unwrap();
    ResponseOutparam::set(out, Ok(resp));
    let stream = outgoing_body.write().unwrap();
    let _ = stream.blocking_write_and_flush(body);
    drop(stream);
    let _ = OutgoingBody::finish(outgoing_body, None);
}

export!(Component);
