//! TCP client for the long-running `ocelaudit-csl-service` (M14).
//!
//! Wire protocol: line-delimited JSON. Open a fresh loopback TCP
//! connection per call, write one request line, read one response
//! line, close. The whole exchange is sub-millisecond on loopback —
//! moving the corpus + index from "rebuilt-per-component-instance"
//! to "always-resident-in-the-service" is what makes /search fast.
//!
//! All wasi:sockets bindings are raw wit-bindgen — no `wstd` here,
//! so we don't disturb the gateway's existing HTTP plumbing.

use crate::wasi::clocks::monotonic_clock;
use crate::wasi::io::streams::StreamError;
use crate::wasi::sockets::instance_network;
use crate::wasi::sockets::network::{
    ErrorCode, IpAddressFamily, IpSocketAddress, Ipv4SocketAddress,
};
use crate::wasi::sockets::tcp_create_socket;

const SERVICE_HOST: (u8, u8, u8, u8) = (127, 0, 0, 1);
const SERVICE_PORT: u16 = 7878;
const READ_TIMEOUT_NS: u64 = 30 * 1_000_000_000; // 30 s — generous

/// Send a single JSON request line, return the JSON response bytes
/// (without the trailing newline). Blocks the calling request until
/// the round-trip completes.
pub fn request(line_json: &[u8]) -> Result<Vec<u8>, String> {
    let net = instance_network::instance_network();
    let sock = tcp_create_socket::create_tcp_socket(IpAddressFamily::Ipv4)
        .map_err(|e| format!("create tcp socket: {:?}", e))?;
    let addr = IpSocketAddress::Ipv4(Ipv4SocketAddress {
        port: SERVICE_PORT,
        address: SERVICE_HOST,
    });

    sock.start_connect(&net, addr)
        .map_err(|e| format!("start_connect: {:?}", e))?;
    let pollable = sock.subscribe();
    pollable.block();
    drop(pollable);

    let (input, output) = sock
        .finish_connect()
        .map_err(|e| describe_connect_error(e))?;

    // Write the request line, chunked to honour wasi:io's 4096-byte
    // limit (same constraint we hit in lib.rs::write_response).
    const MAX_CHUNK: usize = 4096;
    for chunk in line_json.chunks(MAX_CHUNK) {
        output
            .blocking_write_and_flush(chunk)
            .map_err(|e| format!("write request: {:?}", e))?;
    }
    output
        .blocking_write_and_flush(b"\n")
        .map_err(|e| format!("write newline: {:?}", e))?;

    // Read until newline OR timeout. The service always terminates
    // its replies with `\n`.
    let deadline = monotonic_clock::now() + READ_TIMEOUT_NS;
    let mut buf = Vec::with_capacity(64 * 1024);
    while buf.last().copied() != Some(b'\n') {
        if monotonic_clock::now() > deadline {
            return Err(format!(
                "service read timeout after {} ms",
                READ_TIMEOUT_NS / 1_000_000
            ));
        }
        match input.blocking_read(64 * 1024) {
            Ok(chunk) if chunk.is_empty() => break,
            Ok(chunk) => buf.extend_from_slice(&chunk),
            Err(StreamError::Closed) => break,
            Err(StreamError::LastOperationFailed(e)) => {
                return Err(format!("read body: {}", e.to_debug_string()));
            }
        }
    }
    if let Some(&b'\n') = buf.last() {
        buf.pop();
    }

    drop(input);
    drop(output);
    drop(sock);

    Ok(buf)
}

fn describe_connect_error(e: ErrorCode) -> String {
    format!(
        "connect: {:?} — is the csl-service running on 127.0.0.1:{}? \
         Check `wash dev`'s `dev.service_file` config and the wash log \
         for the `csl-service: ready` line.",
        e, SERVICE_PORT
    )
}

// ---------- typed wrappers ----------

use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(tag = "op", rename_all = "lowercase")]
#[allow(dead_code)] // Ping + Entry are reserved for future use.
pub enum Op<'a> {
    Ping,
    Refresh,
    Stats,
    Entry {
        id: &'a str,
    },
    Search(SearchOp<'a>),
    Autocomplete {
        prefix: &'a str,
        limit: Option<u32>,
    },
}

#[derive(Serialize)]
pub struct SearchOp<'a> {
    pub q: &'a str,
    pub sources: Option<&'a [String]>,
    pub entity_types: Option<&'a [String]>,
    pub fuzzy: Option<bool>,
    pub limit: Option<u32>,
}

#[derive(Deserialize)]
struct Envelope {
    ok: bool,
    #[serde(default)]
    data: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<String>,
}

/// Send an op, get back the unpacked `data` field. Errors include the
/// service's `error` string verbatim so callers can surface them
/// upstream.
pub fn call(op: Op<'_>) -> Result<serde_json::Value, String> {
    let line = serde_json::to_vec(&op).map_err(|e| format!("encode op: {}", e))?;
    let response_bytes = request(&line)?;
    let env: Envelope = serde_json::from_slice(&response_bytes)
        .map_err(|e| format!("decode response: {} — body: {:?}", e, String::from_utf8_lossy(&response_bytes)))?;
    if !env.ok {
        return Err(env.error.unwrap_or_else(|| "service error".into()));
    }
    Ok(env.data.unwrap_or(serde_json::Value::Null))
}
