use crate::StorageError;

/// Parsed `STORAGE_BACKEND` env value.
///
/// M2 only accepts the `jsonfs:` prefix. M11 will add `sqlite:` and
/// `turso:` here as new variants — *not* by editing the parser to be
/// backend-aware, but by adding new `StorageBackend::*` cases. Keeps the
/// "new module, not a refactor" promise from PLAN.md §1.3 honest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageBackend {
    JsonFs { path: String },
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
    if raw.starts_with("sqlite:") {
        return Err(StorageError::InvalidConfig(format!(
            "STORAGE_BACKEND={} — sqlite backend lands in M11. Use jsonfs:<dir> for now.",
            raw
        )));
    }
    if raw.starts_with("turso:") {
        return Err(StorageError::InvalidConfig(format!(
            "STORAGE_BACKEND={} — turso backend lands in M11. Use jsonfs:<dir> for now.",
            raw
        )));
    }
    Err(StorageError::InvalidConfig(format!(
        "STORAGE_BACKEND={} is not understood. Supported in M2: jsonfs:<dir>",
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
    fn rejects_sqlite_in_m2_with_pointer_to_m11() {
        let err = parse_storage_backend("sqlite:/data/ocelaudit.db")
            .unwrap_err()
            .to_string();
        assert!(err.contains("M11"));
    }

    #[test]
    fn rejects_turso_in_m2_with_pointer_to_m11() {
        let err = parse_storage_backend("turso:/data/ocelaudit.db")
            .unwrap_err()
            .to_string();
        assert!(err.contains("M11"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_storage_backend("redis://localhost").is_err());
        assert!(parse_storage_backend("").is_err());
    }
}
