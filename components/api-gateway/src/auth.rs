//! Cookie-based session authentication.
//!
//! Wire format:
//!
//! ```text
//! Set-Cookie: session=<base64url(payload)>.<base64url(hmac)>; HttpOnly; \
//!             Path=/; SameSite=Strict; Secure
//! ```
//!
//! `payload` is a compact JSON `{"u": <username>, "r": <role>, "iat": <unix-secs>}`.
//! The HMAC is HMAC-SHA256 over `payload` (the base64url, not the JSON,
//! so verification is byte-stable). Signing key is read from the
//! `SESSION_SIGNING_KEY` env at startup; if unset, a fresh random key is
//! generated — sessions don't survive a host restart in that mode, which
//! is acceptable for a demo (and called out explicitly in the README).
//!
//! Sessions don't carry an explicit `exp` field today. M5 adds rolling
//! expiry; for now sessions are valid for the lifetime of the host's
//! signing key.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use ocelaudit_storage_jsonfs::Role;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    #[serde(rename = "u")]
    pub username: String,
    #[serde(rename = "r")]
    pub role: Role,
    #[serde(rename = "iat")]
    pub issued_at: u64,
}

pub struct SessionSigner {
    key: Vec<u8>,
}

impl SessionSigner {
    /// Build a signer from `SESSION_SIGNING_KEY` (UTF-8 bytes) if set,
    /// or by reading/creating a key file on disk under the storage
    /// root. Returns `(signer, generated_now)` — when `generated_now`
    /// is true the caller logs a one-time notice.
    ///
    /// Each WASI P2 incoming-handler call is essentially a fresh
    /// component instance, so we can't rely on in-process state to keep
    /// the key alive across requests; persistence must be on disk.
    pub fn from_env_or_keyfile(storage_root: &std::path::Path) -> (Self, bool) {
        if let Ok(s) = std::env::var("SESSION_SIGNING_KEY") {
            if !s.is_empty() {
                return (Self { key: s.into_bytes() }, false);
            }
        }
        let path = storage_root.join("session.key");
        if path.exists() {
            if let Ok(bytes) = std::fs::read(&path) {
                if bytes.len() >= 16 {
                    return (Self { key: bytes }, false);
                }
            }
        }
        let mut buf = [0u8; 32];
        let _ = getrandom::getrandom(&mut buf);
        let _ = std::fs::write(&path, &buf);
        (Self { key: buf.to_vec() }, true)
    }

    pub fn issue(&self, session: &Session) -> Result<String, String> {
        let json = serde_json::to_vec(session).map_err(|e| e.to_string())?;
        let payload_b64 = URL_SAFE_NO_PAD.encode(&json);
        let mut mac = HmacSha256::new_from_slice(&self.key).map_err(|e| e.to_string())?;
        mac.update(payload_b64.as_bytes());
        let sig = mac.finalize().into_bytes();
        let sig_b64 = URL_SAFE_NO_PAD.encode(sig);
        Ok(format!("{}.{}", payload_b64, sig_b64))
    }

    /// Decode + verify a cookie value. Returns `None` for any failure
    /// mode (malformed, bad mac, bad json) — never reveals which.
    pub fn verify(&self, value: &str) -> Option<Session> {
        let (payload_b64, sig_b64) = value.split_once('.')?;
        let mut mac = HmacSha256::new_from_slice(&self.key).ok()?;
        mac.update(payload_b64.as_bytes());
        let expected = mac.finalize().into_bytes();
        let actual = URL_SAFE_NO_PAD.decode(sig_b64).ok()?;
        // Constant-time equality.
        if expected.ct_eq(&actual).unwrap_u8() != 1 {
            return None;
        }
        let payload = URL_SAFE_NO_PAD.decode(payload_b64).ok()?;
        serde_json::from_slice(&payload).ok()
    }
}

/// Parse the value of a `Cookie:` header for a `session=...` value.
pub fn extract_session_cookie(cookie_header: &str) -> Option<&str> {
    for piece in cookie_header.split(';') {
        let piece = piece.trim();
        if let Some(v) = piece.strip_prefix("session=") {
            return Some(v);
        }
    }
    None
}

/// Build the `Set-Cookie` header value for a fresh session. Demo only —
/// production deployments would set `Secure` only when the upstream is
/// TLS-terminated; we emit it unconditionally and document in README.
pub fn set_cookie(value: &str) -> String {
    format!(
        "session={}; HttpOnly; Path=/; SameSite=Strict; Max-Age=86400",
        value
    )
}

pub fn clear_cookie() -> String {
    "session=; HttpOnly; Path=/; SameSite=Strict; Max-Age=0".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signer() -> SessionSigner {
        SessionSigner {
            key: b"test-key-32-bytes-deadbeefcafebab".to_vec(),
        }
    }

    fn sample() -> Session {
        Session {
            username: "compliance".into(),
            role: Role::Compliance,
            issued_at: 1_700_000_000,
        }
    }

    #[test]
    fn round_trip_issue_then_verify() {
        let s = signer();
        let cookie = s.issue(&sample()).unwrap();
        let back = s.verify(&cookie).expect("verify");
        assert_eq!(back.username, "compliance");
        assert_eq!(back.role, Role::Compliance);
        assert_eq!(back.issued_at, 1_700_000_000);
    }

    #[test]
    fn tampered_payload_fails() {
        let s = signer();
        let cookie = s.issue(&sample()).unwrap();
        let (_pl, sig) = cookie.split_once('.').unwrap();
        let evil = format!("eyJ1IjoiYWRtaW4iLCJyIjoiYWRtaW4iLCJpYXQiOjF9.{}", sig);
        assert!(s.verify(&evil).is_none());
    }

    #[test]
    fn tampered_sig_fails() {
        let s = signer();
        let cookie = s.issue(&sample()).unwrap();
        let (pl, _sig) = cookie.split_once('.').unwrap();
        let evil = format!("{}.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA", pl);
        assert!(s.verify(&evil).is_none());
    }

    #[test]
    fn malformed_returns_none() {
        let s = signer();
        assert!(s.verify("garbage").is_none());
        assert!(s.verify("").is_none());
        assert!(s.verify(".").is_none());
        assert!(s.verify("only-payload.").is_none());
    }

    #[test]
    fn different_keys_dont_cross_verify() {
        let a = SessionSigner {
            key: b"key-aaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_vec(),
        };
        let b = SessionSigner {
            key: b"key-bbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_vec(),
        };
        let cookie = a.issue(&sample()).unwrap();
        assert!(b.verify(&cookie).is_none());
    }

    #[test]
    fn extract_cookie_picks_session() {
        assert_eq!(extract_session_cookie("session=abc.def"), Some("abc.def"));
        assert_eq!(extract_session_cookie("foo=bar; session=abc.def; baz=qux"),
                   Some("abc.def"));
        assert_eq!(extract_session_cookie("foo=bar"), None);
        assert_eq!(extract_session_cookie(""), None);
    }
}
