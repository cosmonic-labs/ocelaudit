use ocelaudit_storage_jsonfs::{
    parse_storage_backend, JsonFsStorage, Storage, StorageBackend,
};
use ocelaudit_storage_memory::MemoryStorage;

use crate::auth::SessionSigner;

const DEFAULT_STORAGE: &str = "jsonfs:/data";

pub struct AppState {
    pub storage: Box<dyn Storage>,
    pub signer: SessionSigner,
}

impl AppState {
    pub fn startup() -> Result<Self, String> {
        let raw = std::env::var("STORAGE_BACKEND")
            .unwrap_or_else(|_| DEFAULT_STORAGE.into());
        let backend = parse_storage_backend(&raw).map_err(|e| e.to_string())?;
        let storage: Box<dyn Storage> = match backend {
            StorageBackend::JsonFs { path } => Box::new(
                JsonFsStorage::open(path).map_err(|e| e.to_string())?,
            ),
            StorageBackend::Memory => Box::new(MemoryStorage::new()),
        };
        if let Some(creds) = storage.users_seed_if_empty().map_err(|e| e.to_string())? {
            eprintln!(
                "ocelaudit: seeded users.json. admin password: {}  compliance password: {}",
                creds.admin_password, creds.compliance_password
            );
        }
        let (signer, was_generated) = SessionSigner::from_env_or_keyfile(storage.root_path());
        if was_generated {
            eprintln!(
                "ocelaudit: SESSION_SIGNING_KEY not set; generated a fresh key and persisted it \
                 to {}/session.key. Set the env var to a stable secret in any non-demo deployment.",
                storage.root_path().display()
            );
        }
        Ok(Self { storage, signer })
    }
}
