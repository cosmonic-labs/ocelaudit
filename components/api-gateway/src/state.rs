use std::cell::RefCell;

use ocelaudit_search::SearchEngine;
use ocelaudit_storage_jsonfs::{
    parse_storage_backend, JsonFsStorage, Storage, StorageBackend,
};
use ocelaudit_storage_memory::MemoryStorage;

use crate::auth::SessionSigner;

const DEFAULT_STORAGE: &str = "jsonfs:/data";

pub struct AppState {
    pub storage: Box<dyn Storage>,
    pub signer: SessionSigner,
    pub engine: RefCell<Option<SearchEngine>>,
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
        Ok(Self {
            storage,
            signer,
            engine: RefCell::new(None),
        })
    }

    /// Disk cache for the prebuilt search index. Sits next to the
    /// corpus under the storage root so per-data-dir caches stay
    /// isolated. Wash 2.0.5 reinstantiates the wasm component per
    /// request, so without this cache every /search rebuilt the
    /// 25,600-record index from scratch (~1 s per call).
    pub fn index_cache_path(&self) -> std::path::PathBuf {
        self.storage.root_path().join("search-index.bin")
    }

    pub fn ensure_engine(&self) -> Result<std::cell::Ref<'_, SearchEngine>, String> {
        {
            let borrowed = self.engine.borrow();
            if borrowed.is_some() {
                return Ok(std::cell::Ref::map(borrowed, |o| o.as_ref().unwrap()));
            }
        }

        let t_start = crate::wasi::clocks::monotonic_clock::now();

        // 1. Try the disk cache first.
        let cache_path = self.index_cache_path();
        if let Ok(bytes) = std::fs::read(&cache_path) {
            match SearchEngine::load_from_bytes(&bytes) {
                Ok(Some(engine)) => {
                    let t_loaded = crate::wasi::clocks::monotonic_clock::now();
                    eprintln!(
                        "ocelaudit: ensure_engine hot-load: from_disk={} ms, n={}",
                        (t_loaded - t_start) / 1_000_000,
                        engine.len(),
                    );
                    *self.engine.borrow_mut() = Some(engine);
                    return Ok(std::cell::Ref::map(self.engine.borrow(), |o| {
                        o.as_ref().unwrap()
                    }));
                }
                Ok(None) => {
                    eprintln!(
                        "ocelaudit: ensure_engine: disk cache at {} is stale (format mismatch); rebuilding",
                        cache_path.display()
                    );
                }
                Err(e) => {
                    eprintln!(
                        "ocelaudit: ensure_engine: disk cache at {} failed to decode ({}); rebuilding",
                        cache_path.display(),
                        e
                    );
                }
            }
        }

        // 2. Cold build path: read corpus, build index.
        let entries = self
            .storage
            .csl_list_all()
            .map_err(|e| e.to_string())?;
        let t_loaded = crate::wasi::clocks::monotonic_clock::now();
        let n = entries.len();
        let engine = SearchEngine::build(entries);
        let t_built = crate::wasi::clocks::monotonic_clock::now();

        // 3. Persist for next request. Failures are logged but
        //    non-fatal — search still works without the cache.
        match engine.serialize_to_bytes() {
            Ok(bytes) => {
                let written = bytes.len();
                if let Err(e) = std::fs::write(&cache_path, &bytes) {
                    eprintln!("ocelaudit: failed to write index cache: {}", e);
                } else {
                    eprintln!(
                        "ocelaudit: ensure_engine cold-build: csl_list_all={} ms, engine_build={} ms, persist={} bytes, n={}",
                        (t_loaded - t_start) / 1_000_000,
                        (t_built - t_loaded) / 1_000_000,
                        written,
                        n,
                    );
                }
            }
            Err(e) => {
                eprintln!("ocelaudit: failed to serialize index: {}", e);
            }
        }

        *self.engine.borrow_mut() = Some(engine);
        Ok(std::cell::Ref::map(self.engine.borrow(), |o| {
            o.as_ref().unwrap()
        }))
    }

    pub fn invalidate_engine(&self) {
        *self.engine.borrow_mut() = None;
        // Also nuke the disk cache so the next /search rebuilds against
        // the fresh corpus instead of serving stale results.
        let _ = std::fs::remove_file(self.index_cache_path());
    }
}
