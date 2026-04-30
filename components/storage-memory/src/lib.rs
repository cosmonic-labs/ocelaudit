//! Ephemeral in-memory storage backend.
//!
//! Every operation is a HashMap/Vec mutation behind a `RefCell`. Useful
//! for tests, the CI matrix, and demonstrating that the `Storage` trait
//! is real. **Not durable** — when the host restarts, all state vanishes.
//! For real persistence use `storage-jsonfs` (M2 default) or, once
//! wasi-sdk lands, `storage-sqlite` / `storage-turso`.
//!
//! Same hash algorithm as `storage-jsonfs` (Argon2id) so password
//! verification semantics are identical across backends.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use ocelaudit_search::CslEntry;
use ocelaudit_storage_jsonfs::{
    CslMetadata, PublicUser, Result, Role, SearchEvent, SeededCredentials, SourceCount, Storage,
    StorageError, User, WorkflowEntry,
};

#[derive(Default)]
struct Inner {
    csl: Option<CslSnapshot>,
    audit: Vec<SearchEvent>,
    users: HashMap<String, User>,
    workflow: Vec<WorkflowEntry>,
    seeded: bool,
}

#[derive(Clone)]
struct CslSnapshot {
    fetched_at: u64,
    version: String,
    entries: Vec<CslEntry>,
}

pub struct MemoryStorage {
    inner: RefCell<Inner>,
    root: PathBuf,
}

// SAFETY: WasmCloud components are single-threaded. The trait requires
// `Send + Sync` for future flexibility; RefCell isn't Sync, so we
// implement these unsafely as a single-threaded contract. Crash early
// if anyone tries to use this from multiple threads.
unsafe impl Send for MemoryStorage {}
unsafe impl Sync for MemoryStorage {}

impl MemoryStorage {
    pub fn new() -> Self {
        // Use a fresh per-process tmp dir so the gateway can persist
        // its session-signing key alongside the in-memory data even
        // though the audit/users tables themselves are ephemeral.
        let mut root = std::env::temp_dir();
        root.push(format!(
            "ocelaudit-memory-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::create_dir_all(&root);
        Self {
            inner: RefCell::new(Inner::default()),
            root,
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl Storage for MemoryStorage {
    fn csl_metadata(&self) -> Result<Option<CslMetadata>> {
        let i = self.inner.borrow();
        let snap = match &i.csl {
            Some(s) => s,
            None => return Ok(None),
        };
        let mut by_source: std::collections::BTreeMap<String, u32> =
            std::collections::BTreeMap::new();
        for e in &snap.entries {
            *by_source.entry(e.source_list.clone()).or_insert(0) += 1;
        }
        Ok(Some(CslMetadata {
            fetched_at: snap.fetched_at,
            count: snap.entries.len() as u32,
            sources: by_source
                .into_iter()
                .map(|(name, count)| SourceCount { name, count })
                .collect(),
            version: snap.version.clone(),
        }))
    }

    fn csl_list_all(&self) -> Result<Vec<CslEntry>> {
        Ok(self
            .inner
            .borrow()
            .csl
            .as_ref()
            .map(|s| s.entries.clone())
            .unwrap_or_default())
    }

    fn csl_list_by_source(&self, source: &str) -> Result<Vec<CslEntry>> {
        Ok(self
            .csl_list_all()?
            .into_iter()
            .filter(|e| e.source_list == source)
            .collect())
    }

    fn csl_get(&self, id: &str) -> Result<Option<CslEntry>> {
        Ok(self.csl_list_all()?.into_iter().find(|e| e.id == id))
    }

    fn csl_bulk_replace(
        &self,
        entries: Vec<CslEntry>,
        fetched_at: u64,
        version: String,
    ) -> Result<()> {
        self.inner.borrow_mut().csl = Some(CslSnapshot {
            fetched_at,
            version,
            entries,
        });
        Ok(())
    }

    fn audit_log(&self, event: &SearchEvent) -> Result<String> {
        self.inner.borrow_mut().audit.push(event.clone());
        Ok(event.audit_id.clone())
    }

    fn audit_list_recent(&self, limit: usize, offset: usize) -> Result<Vec<SearchEvent>> {
        let i = self.inner.borrow();
        Ok(i.audit
            .iter()
            .rev()
            .skip(offset)
            .take(limit)
            .cloned()
            .collect())
    }

    fn audit_get(&self, audit_id: &str) -> Result<Option<SearchEvent>> {
        Ok(self
            .inner
            .borrow()
            .audit
            .iter()
            .find(|e| e.audit_id == audit_id)
            .cloned())
    }

    fn users_seed_if_empty(&self) -> Result<Option<SeededCredentials>> {
        let mut i = self.inner.borrow_mut();
        if i.seeded {
            return Ok(None);
        }
        let admin_pw = ocelaudit_storage_jsonfs::DEMO_ADMIN_PASSWORD;
        let compl_pw = ocelaudit_storage_jsonfs::DEMO_COMPLIANCE_PASSWORD;
        i.users.insert(
            "admin".into(),
            User {
                username: "admin".into(),
                role: Role::Admin,
                password_hash: hash_password(admin_pw)?,
            },
        );
        i.users.insert(
            "compliance".into(),
            User {
                username: "compliance".into(),
                role: Role::Compliance,
                password_hash: hash_password(compl_pw)?,
            },
        );
        i.seeded = true;
        Ok(Some(SeededCredentials {
            admin_password: admin_pw.to_string(),
            compliance_password: compl_pw.to_string(),
        }))
    }

    fn users_list(&self) -> Result<Vec<User>> {
        Ok(self.inner.borrow().users.values().cloned().collect())
    }

    fn users_get(&self, username: &str) -> Result<Option<User>> {
        Ok(self.inner.borrow().users.get(username).cloned())
    }

    fn users_verify(&self, username: &str, password: &str) -> Result<Option<PublicUser>> {
        let user = match self.users_get(username)? {
            Some(u) => u,
            None => return Ok(None),
        };
        let parsed = PasswordHash::new(&user.password_hash)
            .map_err(|e| StorageError::Argon(e.to_string()))?;
        match Argon2::default().verify_password(password.as_bytes(), &parsed) {
            Ok(()) => Ok(Some(PublicUser::from(&user))),
            Err(_) => Ok(None),
        }
    }

    fn workflow_log(&self, entry: &WorkflowEntry) -> Result<()> {
        self.inner.borrow_mut().workflow.push(entry.clone());
        Ok(())
    }

    fn workflow_history(&self, audit_id: &str) -> Result<Vec<WorkflowEntry>> {
        Ok(self
            .inner
            .borrow()
            .workflow
            .iter()
            .filter(|e| e.audit_id == audit_id)
            .cloned()
            .collect())
    }

    fn workflow_recent(&self, limit: usize) -> Result<Vec<WorkflowEntry>> {
        Ok(self
            .inner
            .borrow()
            .workflow
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect())
    }

    fn root_path(&self) -> &Path {
        &self.root
    }
}

fn hash_password(plain: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon = Argon2::default();
    let hash = argon
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| StorageError::Argon(e.to_string()))?;
    Ok(hash.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ocelaudit_search::EntityType;

    fn entry(id: &str, name: &str, source: &str) -> CslEntry {
        CslEntry {
            id: id.into(),
            source_list: source.into(),
            name: name.into(),
            aliases: vec![],
            entity_type: EntityType::Entity,
            addresses: vec![],
            nationalities: vec![],
            programs: vec![],
        }
    }

    #[test]
    fn roundtrip_csl() {
        let s = MemoryStorage::new();
        s.csl_bulk_replace(vec![entry("a", "Acme", "SDN")], 100, "v0".into())
            .unwrap();
        assert_eq!(s.csl_metadata().unwrap().unwrap().count, 1);
        assert_eq!(s.csl_get("a").unwrap().unwrap().name, "Acme");
        assert!(s.csl_get("nope").unwrap().is_none());
    }

    #[test]
    fn audit_newest_first_pagination() {
        let s = MemoryStorage::new();
        for i in 0..5 {
            s.audit_log(&SearchEvent {
                audit_id: format!("a{}", i),
                who: "x".into(),
                when: i as u64,
                query: "q".into(),
                tlp: "green".into(),
                top_hit_ids: vec![],
                decision: "auto-green".into(),
            })
            .unwrap();
        }
        let r = s.audit_list_recent(3, 0).unwrap();
        assert_eq!(r[0].audit_id, "a4");
        assert_eq!(r[2].audit_id, "a2");
        assert_eq!(s.audit_list_recent(3, 3).unwrap().len(), 2);
    }

    #[test]
    fn users_seed_once_then_verify() {
        let s = MemoryStorage::new();
        let creds = s.users_seed_if_empty().unwrap().unwrap();
        assert!(s.users_seed_if_empty().unwrap().is_none());
        assert!(s.users_verify("admin", &creds.admin_password).unwrap().is_some());
        assert!(s.users_verify("admin", "wrong").unwrap().is_none());
    }

    #[test]
    fn workflow_history_per_audit() {
        let s = MemoryStorage::new();
        for (id, decision) in [("a1", "pending-review"), ("a1", "cleared"), ("a2", "blocked")] {
            s.workflow_log(&WorkflowEntry {
                audit_id: id.into(),
                decision: decision.into(),
                decided_by: "compliance".into(),
                decided_at: 0,
                note: None,
            })
            .unwrap();
        }
        assert_eq!(s.workflow_history("a1").unwrap().len(), 2);
        assert_eq!(s.workflow_history("a2").unwrap().len(), 1);
    }

    #[test]
    fn implements_storage_trait_object() {
        let s: Box<dyn Storage> = Box::new(MemoryStorage::new());
        assert!(s.csl_metadata().unwrap().is_none());
    }
}
