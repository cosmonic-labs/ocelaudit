//! Outgoing HTTPS fetch for the live CSL data feed.
//!
//! Hits data.trade.gov/downloadable_consolidated_screening_list/v1/consolidated.json
//! over `wasi:http/outgoing-handler@0.2.2`. Returns the raw response
//! body as a `Vec<u8>` so the caller can hand it to
//! `ocelaudit_csl_ingest::parse_external_json`. Errors are stringified
//! and surfaced upward — the gateway falls back to the bundled seed
//! file on any failure.

use crate::wasi::http::outgoing_handler;
use crate::wasi::http::types::{Fields, IncomingResponse, Method, OutgoingRequest, Scheme};
use crate::wasi::io::streams::StreamError;

/// Live CSL endpoint. The api.trade.gov path 301s to this, so we hit
/// the redirect target directly to avoid implementing redirect-follow
/// in raw bindings.
pub const TRADE_GOV_HOST: &str = "data.trade.gov";
pub const TRADE_GOV_PATH: &str = "/downloadable_consolidated_screening_list/v1/consolidated.json";

#[derive(Debug)]
pub struct FetchOk {
    pub bytes: Vec<u8>,
    pub status: u16,
}

pub fn fetch_consolidated_json() -> Result<FetchOk, String> {
    let headers = Fields::new();
    let _ = headers.set(
        &"user-agent".to_string(),
        &[b"OcelAudit-demo/0.13".to_vec()],
    );
    let _ = headers.set(&"accept".to_string(), &[b"application/json".to_vec()]);

    let req = OutgoingRequest::new(headers);
    req.set_method(&Method::Get).map_err(|_| "set_method".to_string())?;
    req.set_scheme(Some(&Scheme::Https))
        .map_err(|_| "set_scheme".to_string())?;
    req.set_authority(Some(TRADE_GOV_HOST))
        .map_err(|_| "set_authority".to_string())?;
    req.set_path_with_query(Some(TRADE_GOV_PATH))
        .map_err(|_| "set_path".to_string())?;

    let future = outgoing_handler::handle(req, None)
        .map_err(|e| format!("handle: {:?}", e))?;

    // Block until the response head is available.
    let pollable = future.subscribe();
    pollable.block();
    drop(pollable);

    let resp_outer = future
        .get()
        .ok_or_else(|| "future returned no response".to_string())?
        .map_err(|()| "future was already consumed".to_string())?;
    let response: IncomingResponse =
        resp_outer.map_err(|e| format!("response error: {:?}", e))?;
    let status = response.status();

    let body = response
        .consume()
        .map_err(|()| "consume body".to_string())?;
    let stream = body.stream().map_err(|()| "open body stream".to_string())?;

    let mut bytes: Vec<u8> = Vec::with_capacity(8 * 1024 * 1024);
    loop {
        match stream.blocking_read(64 * 1024) {
            Ok(chunk) => {
                if chunk.is_empty() {
                    break;
                }
                bytes.extend_from_slice(&chunk);
            }
            Err(StreamError::Closed) => break,
            Err(StreamError::LastOperationFailed(e)) => {
                return Err(format!("read body: {}", e.to_debug_string()));
            }
        }
    }
    drop(stream);

    Ok(FetchOk { bytes, status })
}
