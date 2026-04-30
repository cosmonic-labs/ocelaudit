wit_bindgen::generate!({
    world: "api-gateway",
    generate_all,
});

use std::cell::OnceCell;

use exports::wasi::http::incoming_handler::Guest;
use wasi::http::types::{
    Fields, IncomingRequest, OutgoingBody, OutgoingResponse, ResponseOutparam,
};

mod auth;
mod routes;
mod state;
mod static_assets;

use state::AppState;

struct Component;

thread_local! {
    static APP: OnceCell<AppState> = const { OnceCell::new() };
}

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
        let query_string = raw_path.split_once('?').map(|(_, q)| q.to_string());
        let cookie_header = read_cookie_header(&request);
        let body = read_body(request);

        let app_result = app();
        let resp = routes::dispatch(routes::DispatchInput {
            method: &method,
            path: &path,
            query_string: query_string.as_deref(),
            cookie_header: cookie_header.as_deref(),
            body: body.as_deref(),
            app: &app_result,
        });

        write_response(response_out, resp);
    }
}

fn read_cookie_header(req: &IncomingRequest) -> Option<String> {
    let headers = req.headers();
    // wasi:http header names are case-sensitive in the API even though
    // HTTP itself is case-insensitive. Probe both common casings.
    for name in &["cookie", "Cookie"] {
        let entries = headers.get(&name.to_string());
        if let Some(raw) = entries.into_iter().next() {
            return String::from_utf8(raw).ok();
        }
    }
    None
}

fn read_body(req: IncomingRequest) -> Option<Vec<u8>> {
    let body = req.consume().ok()?;
    let stream = body.stream().ok()?;
    let mut out = Vec::new();
    loop {
        match stream.blocking_read(8192) {
            Ok(chunk) => {
                if chunk.is_empty() {
                    // No data available right now; treat as EOF for
                    // request bodies (we already finished reading).
                    break;
                }
                out.extend_from_slice(&chunk);
            }
            Err(_) => break,
        }
    }
    drop(stream);
    Some(out)
}

#[derive(Debug, Default)]
pub(crate) struct RouteResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
    pub set_cookie: Option<String>,
    pub extra_headers: Vec<(&'static str, String)>,
}

impl RouteResponse {
    pub fn json(status: u16, value: serde_json::Value) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: value.to_string().into_bytes(),
            set_cookie: None,
            extra_headers: Vec::new(),
        }
    }
    pub fn plain(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type: "text/plain",
            body: body.into().into_bytes(),
            set_cookie: None,
            extra_headers: Vec::new(),
        }
    }
    pub fn bytes(status: u16, content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type,
            body,
            set_cookie: None,
            extra_headers: Vec::new(),
        }
    }
    pub fn err(status: u16, message: impl Into<String>) -> Self {
        Self::json(status, serde_json::json!({"error": message.into()}))
    }
    pub fn with_cookie(mut self, set_cookie: String) -> Self {
        self.set_cookie = Some(set_cookie);
        self
    }
    pub fn with_header(mut self, name: &'static str, value: String) -> Self {
        self.extra_headers.push((name, value));
        self
    }
}

fn write_response(out: ResponseOutparam, r: RouteResponse) {
    let headers = Fields::new();
    let _ = headers.set(
        &"content-type".to_string(),
        &[r.content_type.as_bytes().to_vec()],
    );
    if let Some(cookie) = &r.set_cookie {
        let _ = headers.append(&"set-cookie".to_string(), &cookie.as_bytes().to_vec());
    }
    for (name, value) in &r.extra_headers {
        let _ = headers.append(&name.to_string(), &value.as_bytes().to_vec());
    }
    let resp = OutgoingResponse::new(headers);
    let _ = resp.set_status_code(r.status);
    let outgoing_body = resp.body().unwrap();
    ResponseOutparam::set(out, Ok(resp));
    let stream = outgoing_body.write().unwrap();
    let _ = stream.blocking_write_and_flush(&r.body);
    drop(stream);
    let _ = OutgoingBody::finish(outgoing_body, None);
}

export!(Component);
