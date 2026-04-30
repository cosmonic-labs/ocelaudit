use ocelaudit_storage_jsonfs::{
    parse_storage_backend, JsonFsStorage, StorageBackend,
};

/// Default storage path when `STORAGE_BACKEND` is unset.
///
/// Wash dev mounts the host directory under `dev.volumes` at the guest
/// path `/data` (see `.wash/config.yaml`); without an env mechanism for
/// the primary component on wash 2.0.4, we default to that. M4 wires
/// the proper env variable when wadm composition lands.
const DEFAULT_STORAGE: &str = "jsonfs:/data";

/// Process-wide application state. One instance per gateway process,
/// initialized lazily on first request via `OnceCell`.
pub struct AppState {
    pub storage: JsonFsStorage,
}

impl AppState {
    /// Read config from the environment (or fall back to the default),
    /// open storage, seed users on first boot. Errors here surface as
    /// 503 from the gateway — they don't crash the host.
    pub fn startup() -> Result<Self, String> {
        let raw = std::env::var("STORAGE_BACKEND")
            .unwrap_or_else(|_| DEFAULT_STORAGE.into());
        let backend = parse_storage_backend(&raw).map_err(|e| e.to_string())?;
        let storage = match backend {
            StorageBackend::JsonFs { path } => {
                JsonFsStorage::open(path).map_err(|e| e.to_string())?
            }
        };
        if let Some(creds) = storage.users_seed_if_empty().map_err(|e| e.to_string())? {
            // Per PLAN.md §1.4 — log seed credentials to stderr exactly
            // once so the demo runner can copy them. Subsequent boots
            // skip this branch.
            eprintln!(
                "ocelaudit: seeded users.json. admin password: {}  compliance password: {}",
                creds.admin_password, creds.compliance_password
            );
        }
        Ok(Self { storage })
    }
}
