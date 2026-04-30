//! OcelAudit storage backend — JSON-on-disk over `wasi:filesystem`.
//!
//! M2 default per PLAN.md §1.3. Four concerns, four files in a
//! configurable directory:
//!
//! - `csl.json`   — full CSL corpus, whole-file rewrite via atomic
//!   rename (`csl.json.tmp` → `csl.json`).
//! - `audit.jsonl` — append-only newline-delimited JSON, one search event
//!   per line.
//! - `users.json` — whole-file rewrite, Argon2id-hashed passwords.
//! - `workflow.jsonl` — append-only newline-delimited JSON, one decision
//!   event per line.
//!
//! No transactions; no SQL. Good enough for the demo. The whole point of
//! M2 is to ship the rest of the app on a known-good substrate before
//! M11 introduces SQLite + Turso behind the same surface.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::SaltString;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use ocelaudit_search::CslEntry;
use serde::{Deserialize, Serialize};

pub mod config;

pub use config::{parse_storage_backend, StorageBackend};

/// The storage interface OcelAudit's gateway talks to. Implementations:
/// `JsonFsStorage` (this crate), `MemoryStorage` (sibling crate
/// `ocelaudit-storage-memory`). SQLite + Turso land later when wasi-sdk
/// is wired up — see `docs/storage-backends.md` for the gap.
///
/// All methods take `&self` so a single instance can serve concurrent
/// reads. The wasmCloud runtime is single-threaded inside a component;
/// the `Send + Sync` bounds are belt-and-braces for future flexibility.
pub trait Storage: Send + Sync {
    // ----- csl-records -----
    fn csl_metadata(&self) -> Result<Option<CslMetadata>>;
    fn csl_list_all(&self) -> Result<Vec<CslEntry>>;
    fn csl_list_by_source(&self, source: &str) -> Result<Vec<CslEntry>>;
    fn csl_get(&self, id: &str) -> Result<Option<CslEntry>>;
    fn csl_bulk_replace(
        &self,
        entries: Vec<CslEntry>,
        fetched_at: u64,
        version: String,
    ) -> Result<()>;

    // ----- audit -----
    fn audit_log(&self, event: &SearchEvent) -> Result<String>;
    fn audit_list_recent(&self, limit: usize, offset: usize) -> Result<Vec<SearchEvent>>;
    fn audit_get(&self, audit_id: &str) -> Result<Option<SearchEvent>>;

    // ----- users -----
    fn users_seed_if_empty(&self) -> Result<Option<SeededCredentials>>;
    fn users_list(&self) -> Result<Vec<User>>;
    fn users_get(&self, username: &str) -> Result<Option<User>>;
    fn users_verify(&self, username: &str, password: &str) -> Result<Option<PublicUser>>;

    // ----- workflow -----
    fn workflow_log(&self, entry: &WorkflowEntry) -> Result<()>;
    fn workflow_history(&self, audit_id: &str) -> Result<Vec<WorkflowEntry>>;
    fn workflow_recent(&self, limit: usize) -> Result<Vec<WorkflowEntry>>;

    /// Where this backend persists data. Used by api-gateway to colocate
    /// the session signing key. Memory-only backends should return
    /// a stable in-process path under `/tmp` or similar.
    fn root_path(&self) -> &Path;
}

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    Serde(serde_json::Error),
    Argon(String),
    UnknownUser(String),
    Conflict(String),
    InvalidConfig(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StorageError::Io(e) => write!(f, "io: {}", e),
            StorageError::Serde(e) => write!(f, "serde: {}", e),
            StorageError::Argon(e) => write!(f, "argon: {}", e),
            StorageError::UnknownUser(u) => write!(f, "unknown user: {}", u),
            StorageError::Conflict(s) => write!(f, "conflict: {}", s),
            StorageError::InvalidConfig(s) => write!(f, "invalid config: {}", s),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(e: std::io::Error) -> Self {
        StorageError::Io(e)
    }
}
impl From<serde_json::Error> for StorageError {
    fn from(e: serde_json::Error) -> Self {
        StorageError::Serde(e)
    }
}

pub type Result<T> = std::result::Result<T, StorageError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CslMetadata {
    pub fetched_at: u64,
    pub count: u32,
    pub sources: Vec<SourceCount>,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceCount {
    pub name: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CslSnapshot {
    pub fetched_at: u64,
    pub entries: Vec<CslEntry>,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchEvent {
    pub audit_id: String,
    pub who: String,
    pub when: u64,
    pub query: String,
    pub tlp: String,
    pub top_hit_ids: Vec<String>,
    pub decision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    Compliance,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub username: String,
    pub role: Role,
    /// PHC-string Argon2id hash. Never logged, never returned by /me.
    pub password_hash: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PublicUser {
    pub username: String,
    pub role: Role,
}

impl From<&User> for PublicUser {
    fn from(u: &User) -> Self {
        Self {
            username: u.username.clone(),
            role: u.role.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEntry {
    pub audit_id: String,
    pub decision: String,
    pub decided_by: String,
    pub decided_at: u64,
    pub note: Option<String>,
}

/// Concrete JSON-on-disk storage. All methods take `&self` so the same
/// instance can serve concurrent reads; writes serialize via filesystem
/// semantics (each rename / append is an atomic syscall).
pub struct JsonFsStorage {
    root: PathBuf,
}

impl JsonFsStorage {
    /// Create or open a storage rooted at `root`. Creates the directory
    /// if it doesn't exist.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn path(&self, file: &str) -> PathBuf {
        self.root.join(file)
    }

    // ----- csl-records -----

    pub fn csl_metadata(&self) -> Result<Option<CslMetadata>> {
        let snap = match self.read_csl_snapshot()? {
            Some(s) => s,
            None => return Ok(None),
        };
        let mut by_source: std::collections::BTreeMap<String, u32> =
            std::collections::BTreeMap::new();
        for e in &snap.entries {
            *by_source.entry(e.source_list.clone()).or_insert(0) += 1;
        }
        let sources = by_source
            .into_iter()
            .map(|(name, count)| SourceCount { name, count })
            .collect();
        Ok(Some(CslMetadata {
            fetched_at: snap.fetched_at,
            count: snap.entries.len() as u32,
            sources,
            version: snap.version,
        }))
    }

    pub fn csl_list_all(&self) -> Result<Vec<CslEntry>> {
        Ok(self
            .read_csl_snapshot()?
            .map(|s| s.entries)
            .unwrap_or_default())
    }

    pub fn csl_list_by_source(&self, source: &str) -> Result<Vec<CslEntry>> {
        Ok(self
            .csl_list_all()?
            .into_iter()
            .filter(|e| e.source_list == source)
            .collect())
    }

    pub fn csl_get(&self, id: &str) -> Result<Option<CslEntry>> {
        Ok(self
            .csl_list_all()?
            .into_iter()
            .find(|e| e.id == id))
    }

    pub fn csl_bulk_replace(
        &self,
        entries: Vec<CslEntry>,
        fetched_at: u64,
        version: impl Into<String>,
    ) -> Result<()> {
        let snap = CslSnapshot {
            fetched_at,
            entries,
            version: version.into(),
        };
        self.write_atomic(self.path("csl.json"), &snap)
    }

    fn read_csl_snapshot(&self) -> Result<Option<CslSnapshot>> {
        let p = self.path("csl.json");
        if !p.exists() {
            return Ok(None);
        }
        let mut buf = String::new();
        File::open(&p)?.read_to_string(&mut buf)?;
        let snap: CslSnapshot = serde_json::from_str(&buf)?;
        Ok(Some(snap))
    }

    // ----- audit -----

    /// Append a search event. Returns the same `audit_id` it was given,
    /// for caller convenience.
    pub fn audit_log(&self, event: &SearchEvent) -> Result<String> {
        self.append_jsonl(self.path("audit.jsonl"), event)?;
        Ok(event.audit_id.clone())
    }

    /// Most-recent `limit` audit events, skipping `offset` from the end
    /// (i.e. paginated newest-first). Reads the whole file each call;
    /// fine for demo-scale audit volume.
    pub fn audit_list_recent(&self, limit: usize, offset: usize) -> Result<Vec<SearchEvent>> {
        let mut all: Vec<SearchEvent> = self.read_jsonl(self.path("audit.jsonl"))?;
        all.reverse();
        Ok(all.into_iter().skip(offset).take(limit).collect())
    }

    pub fn audit_get(&self, audit_id: &str) -> Result<Option<SearchEvent>> {
        let all: Vec<SearchEvent> = self.read_jsonl(self.path("audit.jsonl"))?;
        Ok(all.into_iter().find(|e| e.audit_id == audit_id))
    }

    // ----- users -----

    /// Seed the users table on first boot if empty. Generates a fresh
    /// random password for both `admin` and `compliance`, hashes them
    /// with Argon2id, writes `users.json` atomically, and returns the
    /// plaintext credentials so the caller can log them to stderr
    /// exactly once.
    pub fn users_seed_if_empty(&self) -> Result<Option<SeededCredentials>> {
        if self.path("users.json").exists() {
            return Ok(None);
        }
        let admin_pw = generate_password();
        let compl_pw = generate_password();
        let users = vec![
            User {
                username: "admin".into(),
                role: Role::Admin,
                password_hash: hash_password(&admin_pw)?,
            },
            User {
                username: "compliance".into(),
                role: Role::Compliance,
                password_hash: hash_password(&compl_pw)?,
            },
        ];
        self.write_atomic(self.path("users.json"), &users)?;
        Ok(Some(SeededCredentials {
            admin_password: admin_pw,
            compliance_password: compl_pw,
        }))
    }

    pub fn users_list(&self) -> Result<Vec<User>> {
        let p = self.path("users.json");
        if !p.exists() {
            return Ok(Vec::new());
        }
        let mut buf = String::new();
        File::open(&p)?.read_to_string(&mut buf)?;
        Ok(serde_json::from_str(&buf)?)
    }

    pub fn users_get(&self, username: &str) -> Result<Option<User>> {
        Ok(self
            .users_list()?
            .into_iter()
            .find(|u| u.username == username))
    }

    pub fn users_verify(&self, username: &str, password: &str) -> Result<Option<PublicUser>> {
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

    // ----- workflow -----

    pub fn workflow_log(&self, entry: &WorkflowEntry) -> Result<()> {
        self.append_jsonl(self.path("workflow.jsonl"), entry)?;
        Ok(())
    }

    pub fn workflow_history(&self, audit_id: &str) -> Result<Vec<WorkflowEntry>> {
        let all: Vec<WorkflowEntry> = self.read_jsonl(self.path("workflow.jsonl"))?;
        Ok(all
            .into_iter()
            .filter(|e| e.audit_id == audit_id)
            .collect())
    }

    pub fn workflow_recent(&self, limit: usize) -> Result<Vec<WorkflowEntry>> {
        let mut all: Vec<WorkflowEntry> = self.read_jsonl(self.path("workflow.jsonl"))?;
        all.reverse();
        Ok(all.into_iter().take(limit).collect())
    }

    // ----- private helpers -----

    fn write_atomic<T: Serialize>(&self, target: PathBuf, value: &T) -> Result<()> {
        let tmp = target.with_extension("json.tmp");
        let mut f = File::create(&tmp)?;
        let bytes = serde_json::to_vec_pretty(value)?;
        f.write_all(&bytes)?;
        f.sync_data()?;
        drop(f);
        fs::rename(&tmp, &target)?;
        Ok(())
    }

    fn append_jsonl<T: Serialize>(&self, target: PathBuf, value: &T) -> Result<()> {
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&target)?;
        let line = serde_json::to_vec(value)?;
        f.write_all(&line)?;
        f.write_all(b"\n")?;
        f.sync_data()?;
        Ok(())
    }

    fn read_jsonl<T: for<'de> Deserialize<'de>>(&self, target: PathBuf) -> Result<Vec<T>> {
        if !target.exists() {
            return Ok(Vec::new());
        }
        let f = File::open(&target)?;
        let r = BufReader::new(f);
        let mut out = Vec::new();
        for line in r.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let v: T = serde_json::from_str(&line)?;
            out.push(v);
        }
        Ok(out)
    }
}

#[derive(Debug)]
pub struct SeededCredentials {
    pub admin_password: String,
    pub compliance_password: String,
}

fn hash_password(plain: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon = Argon2::default();
    let hash = argon
        .hash_password(plain.as_bytes(), &salt)
        .map_err(|e| StorageError::Argon(e.to_string()))?;
    Ok(hash.to_string())
}

/// Generate a 24-char URL-safe random password. Not for humans to type;
/// for the demo runner to copy out of the seed-log line.
fn generate_password() -> String {
    use argon2::password_hash::rand_core::RngCore;
    let mut bytes = [0u8; 18];
    OsRng.fill_bytes(&mut bytes);
    let alphabet = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789";
    bytes
        .iter()
        .map(|b| alphabet[(*b as usize) % alphabet.len()] as char)
        .collect::<String>()
}

impl Storage for JsonFsStorage {
    fn csl_metadata(&self) -> Result<Option<CslMetadata>> {
        Self::csl_metadata(self)
    }
    fn csl_list_all(&self) -> Result<Vec<CslEntry>> {
        Self::csl_list_all(self)
    }
    fn csl_list_by_source(&self, source: &str) -> Result<Vec<CslEntry>> {
        Self::csl_list_by_source(self, source)
    }
    fn csl_get(&self, id: &str) -> Result<Option<CslEntry>> {
        Self::csl_get(self, id)
    }
    fn csl_bulk_replace(
        &self,
        entries: Vec<CslEntry>,
        fetched_at: u64,
        version: String,
    ) -> Result<()> {
        Self::csl_bulk_replace(self, entries, fetched_at, version)
    }

    fn audit_log(&self, event: &SearchEvent) -> Result<String> {
        Self::audit_log(self, event)
    }
    fn audit_list_recent(&self, limit: usize, offset: usize) -> Result<Vec<SearchEvent>> {
        Self::audit_list_recent(self, limit, offset)
    }
    fn audit_get(&self, audit_id: &str) -> Result<Option<SearchEvent>> {
        Self::audit_get(self, audit_id)
    }

    fn users_seed_if_empty(&self) -> Result<Option<SeededCredentials>> {
        Self::users_seed_if_empty(self)
    }
    fn users_list(&self) -> Result<Vec<User>> {
        Self::users_list(self)
    }
    fn users_get(&self, username: &str) -> Result<Option<User>> {
        Self::users_get(self, username)
    }
    fn users_verify(&self, username: &str, password: &str) -> Result<Option<PublicUser>> {
        Self::users_verify(self, username, password)
    }

    fn workflow_log(&self, entry: &WorkflowEntry) -> Result<()> {
        Self::workflow_log(self, entry)
    }
    fn workflow_history(&self, audit_id: &str) -> Result<Vec<WorkflowEntry>> {
        Self::workflow_history(self, audit_id)
    }
    fn workflow_recent(&self, limit: usize) -> Result<Vec<WorkflowEntry>> {
        Self::workflow_recent(self, limit)
    }

    fn root_path(&self) -> &Path {
        self.root()
    }
}

#[cfg(test)]
mod tests;
