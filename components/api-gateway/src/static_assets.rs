//! Static asset serving from `/data/static/`.
//!
//! For M6 the SPA bundle lives on disk under the wash-dev volume mount
//! (host: `.cache/ocelaudit-data/static/`, guest: `/data/static/`). The
//! `_runner.sh` test harness rsync's `ui/dist/*` into that directory
//! before booting wash dev; for live development you can run `pnpm
//! build` then `cp -r ui/dist/* .cache/ocelaudit-data/static/`.
//!
//! The architecturally-correct version of this — a separate
//! static-assets component the gateway calls via WIT — is the
//! published M6 design. We're shipping the simpler in-process variant
//! today; the WIT split is mechanical to add when the demo wants two
//! components instead of one.

use crate::RouteResponse;

const STATIC_ROOT: &str = "/data/static";

pub fn serve(path: &str) -> Option<RouteResponse> {
    // Map browser paths to filesystem paths.
    let rel = match path {
        "/" => "index.html",
        p => p.trim_start_matches('/'),
    };
    // Reject `..` so we can't escape the root, even though wash dev's
    // sandboxing should already prevent it.
    if rel.contains("..") {
        return None;
    }
    let full = format!("{}/{}", STATIC_ROOT, rel);
    let bytes = std::fs::read(&full).ok()?;
    let content_type = content_type_for(rel);
    Some(
        RouteResponse::bytes(200, content_type, bytes)
            .with_header("content-security-policy", CSP_VALUE.to_string()),
    )
}

/// SPA fallback: when the browser hits a route the gateway doesn't have
/// (e.g. `/dashboard`, `/search/...`), serve `index.html` so the
/// client-side router can take over. This is standard SPA behavior.
pub fn spa_fallback() -> Option<RouteResponse> {
    serve("/")
}

fn content_type_for(rel: &str) -> &'static str {
    match rel.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Strict CSP for the SPA. `default-src 'self'` lets the bundle load
/// its own JS/CSS but blocks third-party iframes/scripts, inline JS,
/// and remote fonts. `media-src 'self' data:` is added so the M9
/// background video on /login works without weakening the policy.
pub const CSP_VALUE: &str =
    "default-src 'self'; img-src 'self' data:; media-src 'self' data:; \
     style-src 'self' 'unsafe-inline'; script-src 'self'; \
     base-uri 'self'; form-action 'self'; frame-ancestors 'none'";
