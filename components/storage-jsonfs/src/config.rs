use crate::StorageError;

/// Parsed `STORAGE_BACKEND` env value.
///
/// `jsonfs:<dir>` is the production-ready default since M2.
/// `memory:` is M11's reference second backend — ephemeral in-process
/// storage for tests, demos, and the CI matrix.
/// `sqlite:` and `turso:` are accepted in the parser but reject at
/// runtime with a documented next-step pointer; both require a
/// wasi-sdk linker setup that's out of scope for this demo. See
/// `docs/storage-backends.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageBackend {
    JsonFs { path: String },
    Memory,
}

pub fn parse_storage_backend(raw: &str) -> Result<StorageBackend, StorageError> {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix("jsonfs:") {
        if rest.is_empty() {
            return Err(StorageError::InvalidConfig(
                "jsonfs:<path> requires a non-empty path".into(),
            ));
        }
        return Ok(StorageBackend::JsonFs { path: rest.into() });
    }
    if raw == "memory:" || raw == "memory" {
        return Ok(StorageBackend::Memory);
    }
    if raw.starts_with("sqlite:") {
        return Err(StorageError::InvalidConfig(format!(
            "STORAGE_BACKEND={} — sqlite backend needs a wasi-sdk-built rusqlite. \
             See docs/storage-backends.md for the path forward.",
            raw
        )));
    }
    if raw.starts_with("turso:") {
        return Err(StorageError::InvalidConfig(format!(
            "STORAGE_BACKEND={} — turso (formerly Limbo) needs a wasi-sdk-built mimalloc. \
             See docs/storage-backends.md for the path forward.",
            raw
        )));
    }
    Err(StorageError::InvalidConfig(format!(
        "STORAGE_BACKEND={} is not understood. Supported: jsonfs:<dir>, memory:",
        raw
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_jsonfs() {
        assert_eq!(
            parse_storage_backend("jsonfs:/data/ocelaudit").unwrap(),
            StorageBackend::JsonFs {
                path: "/data/ocelaudit".into()
            }
        );
    }

    #[test]
    fn jsonfs_requires_path() {
        assert!(parse_storage_backend("jsonfs:").is_err());
    }

    #[test]
    fn rejects_sqlite_with_pointer_to_storage_doc() {
        let err = parse_storage_backend("sqlite:/data/ocelaudit.db")
            .unwrap_err()
            .to_string();
        assert!(err.contains("docs/storage-backends.md"));
    }

    #[test]
    fn rejects_turso_with_pointer_to_storage_doc() {
        let err = parse_storage_backend("turso:/data/ocelaudit.db")
            .unwrap_err()
            .to_string();
        assert!(err.contains("docs/storage-backends.md"));
    }

    #[test]
    fn parses_memory() {
        assert_eq!(parse_storage_backend("memory:").unwrap(), StorageBackend::Memory);
        assert_eq!(parse_storage_backend("memory").unwrap(), StorageBackend::Memory);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_storage_backend("redis://localhost").is_err());
        assert!(parse_storage_backend("").is_err());
    }
}
