wit_bindgen::generate!({
    world: "api-gateway",
    generate_all,
});

use std::cell::OnceCell;

use exports::wasi::http::incoming_handler::Guest;
use wasi::http::types::{
    Fields, IncomingRequest, OutgoingBody, OutgoingResponse, ResponseOutparam,
};

mod routes;
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
        if cell.get().is_none() {
            let s = AppState::startup()?;
            let _ = cell.set(s);
        }
        let r: *const AppState = cell.get().unwrap();
        // SAFETY: thread_local on a single-threaded wasm runtime; the
        // OnceCell never resets, so the reference lives for the
        // process's lifetime.
        Ok(unsafe { &*r })
    })
}

impl Guest for Component {
    fn handle(request: IncomingRequest, response_out: ResponseOutparam) {
        let method = request.method();
        let raw_path = request.path_with_query().unwrap_or_else(|| "/".into());
        let path = raw_path.split('?').next().unwrap_or("/").to_string();
        let app_result = app();

        let resp = routes::dispatch(&method, &path, &app_result, &request);
        write_response(response_out, resp.status, resp.content_type, resp.body.as_bytes());
    }
}

pub(crate) struct RouteResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

impl RouteResponse {
    pub fn json(status: u16, value: serde_json::Value) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: value.to_string(),
        }
    }
    pub fn plain(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type: "text/plain",
            body: body.into(),
        }
    }
    pub fn err(status: u16, message: impl Into<String>) -> Self {
        Self::json(status, serde_json::json!({"error": message.into()}))
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
