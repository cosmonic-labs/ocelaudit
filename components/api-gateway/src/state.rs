use std::cell::RefCell;

use ocelaudit_search::SearchEngine;
use ocelaudit_storage_jsonfs::{
    parse_storage_backend, JsonFsStorage, StorageBackend,
};

use crate::auth::SessionSigner;

const DEFAULT_STORAGE: &str = "jsonfs:/data";

pub struct AppState {
    pub storage: JsonFsStorage,
    pub signer: SessionSigner,
    /// Cached search index. Built lazily on first query, invalidated by
    /// `/api/v1/csl/refresh`. RefCell is fine here — we run on a
    /// single-threaded wasm runtime.
    pub engine: RefCell<Option<SearchEngine>>,
}

impl AppState {
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
            eprintln!(
                "ocelaudit: seeded users.json. admin password: {}  compliance password: {}",
                creds.admin_password, creds.compliance_password
            );
        }
        let (signer, was_generated) = SessionSigner::from_env_or_keyfile(storage.root());
        if was_generated {
            eprintln!(
                "ocelaudit: SESSION_SIGNING_KEY not set; generated a fresh key and persisted it \
                 to {}/session.key. Set the env var to a stable secret in any non-demo deployment.",
                storage.root().display()
            );
        }
        Ok(Self {
            storage,
            signer,
            engine: RefCell::new(None),
        })
    }

    /// Borrow the cached engine, building it from storage on first
    /// access. Returns the cached value on subsequent calls. Cleared
    /// by `/api/v1/csl/refresh`.
    pub fn ensure_engine(&self) -> Result<std::cell::Ref<'_, SearchEngine>, String> {
        {
            let borrowed = self.engine.borrow();
            if borrowed.is_some() {
                return Ok(std::cell::Ref::map(borrowed, |o| o.as_ref().unwrap()));
            }
        }
        let entries = self
            .storage
            .csl_list_all()
            .map_err(|e| e.to_string())?;
        let engine = SearchEngine::build(entries);
        *self.engine.borrow_mut() = Some(engine);
        Ok(std::cell::Ref::map(self.engine.borrow(), |o| {
            o.as_ref().unwrap()
        }))
    }

    pub fn invalidate_engine(&self) {
        *self.engine.borrow_mut() = None;
    }
}
