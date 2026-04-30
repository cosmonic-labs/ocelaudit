//! Storage tests with a fresh temp dir per test, so they're hermetic.

use super::*;
use ocelaudit_search::{CslEntry, EntityType};

/// Tiny stand-in for `tempfile::TempDir` to avoid pulling in a dev dep
/// just for one struct. Each test gets a unique nested directory under
/// the workspace target dir; the directory is removed on Drop.
struct TempDir(PathBuf);
impl TempDir {
    fn new(name: &str) -> Self {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "ocelaudit-storage-jsonfs-{}-{}-{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

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

// ---------- csl ----------

#[test]
fn csl_metadata_returns_none_when_empty() {
    let d = TempDir::new("csl-meta-empty");
    let s = JsonFsStorage::open(d.path()).unwrap();
    assert!(s.csl_metadata().unwrap().is_none());
    assert!(s.csl_list_all().unwrap().is_empty());
}

#[test]
fn csl_bulk_replace_then_metadata() {
    let d = TempDir::new("csl-bulk");
    let s = JsonFsStorage::open(d.path()).unwrap();
    let entries = vec![
        entry("a", "Acme Holdings", "SDN"),
        entry("b", "Volga Shipping", "EL"),
        entry("c", "Tehran Metals", "ITAR-DPL"),
        entry("d", "Black Sea Co", "SDN"),
    ];
    s.csl_bulk_replace(entries.clone(), 1_700_000_000, "v0").unwrap();

    let m = s.csl_metadata().unwrap().expect("metadata present");
    assert_eq!(m.count, 4);
    assert_eq!(m.fetched_at, 1_700_000_000);
    assert_eq!(m.version, "v0");
    let mut sources: Vec<_> = m.sources.iter().map(|s| s.name.as_str()).collect();
    sources.sort();
    assert_eq!(sources, vec!["EL", "ITAR-DPL", "SDN"]);
}

#[test]
fn csl_bulk_replace_overwrites_previous() {
    let d = TempDir::new("csl-overwrite");
    let s = JsonFsStorage::open(d.path()).unwrap();
    s.csl_bulk_replace(vec![entry("a", "Acme", "SDN")], 1, "v0")
        .unwrap();
    s.csl_bulk_replace(vec![entry("b", "Volga", "EL")], 2, "v1")
        .unwrap();
    let all = s.csl_list_all().unwrap();
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].id, "b");
    let m = s.csl_metadata().unwrap().unwrap();
    assert_eq!(m.fetched_at, 2);
    assert_eq!(m.version, "v1");
}

#[test]
fn csl_get_and_list_by_source() {
    let d = TempDir::new("csl-list-by");
    let s = JsonFsStorage::open(d.path()).unwrap();
    s.csl_bulk_replace(
        vec![
            entry("a", "Acme", "SDN"),
            entry("b", "Volga", "EL"),
            entry("c", "Tehran", "SDN"),
        ],
        0,
        "v0",
    )
    .unwrap();
    assert_eq!(s.csl_get("b").unwrap().unwrap().name, "Volga");
    assert!(s.csl_get("nope").unwrap().is_none());
    let sdn = s.csl_list_by_source("SDN").unwrap();
    assert_eq!(sdn.len(), 2);
    let el = s.csl_list_by_source("EL").unwrap();
    assert_eq!(el.len(), 1);
}

#[test]
fn csl_atomic_rename_means_no_partial_state() {
    // After a successful bulk-replace, only `csl.json` exists, no `.tmp`.
    let d = TempDir::new("csl-atomic");
    let s = JsonFsStorage::open(d.path()).unwrap();
    s.csl_bulk_replace(vec![entry("a", "A", "SDN")], 0, "v0")
        .unwrap();
    assert!(d.path().join("csl.json").exists());
    assert!(!d.path().join("csl.json.tmp").exists());
}

// ---------- audit ----------

fn synth_event(id: &str, who: &str) -> SearchEvent {
    SearchEvent {
        audit_id: id.into(),
        who: who.into(),
        when: 1_700_000_000,
        query: format!("query for {}", id),
        tlp: "green".into(),
        top_hit_ids: vec![],
        decision: "auto-green".into(),
        source: "api".into(),
        top_hits: vec![],
    }
}

#[test]
fn audit_log_then_get_and_list_recent() {
    let d = TempDir::new("audit-roundtrip");
    let s = JsonFsStorage::open(d.path()).unwrap();
    for i in 0..5 {
        s.audit_log(&synth_event(&format!("a{}", i), "compliance"))
            .unwrap();
    }
    assert_eq!(s.audit_get("a3").unwrap().unwrap().who, "compliance");
    assert!(s.audit_get("nope").unwrap().is_none());

    let recent = s.audit_list_recent(3, 0).unwrap();
    assert_eq!(recent.len(), 3);
    assert_eq!(recent[0].audit_id, "a4"); // newest-first
    assert_eq!(recent[2].audit_id, "a2");

    let page2 = s.audit_list_recent(3, 3).unwrap();
    assert_eq!(page2.len(), 2);
    assert_eq!(page2[0].audit_id, "a1");
    assert_eq!(page2[1].audit_id, "a0");
}

#[test]
fn audit_append_only_does_not_clobber_on_repeated_open() {
    // Simulates two "processes" opening + appending. JSONL is append-only,
    // so both writes survive.
    let d = TempDir::new("audit-append-only");
    let s1 = JsonFsStorage::open(d.path()).unwrap();
    s1.audit_log(&synth_event("first", "a")).unwrap();
    drop(s1);
    let s2 = JsonFsStorage::open(d.path()).unwrap();
    s2.audit_log(&synth_event("second", "b")).unwrap();
    let all = s2.audit_list_recent(10, 0).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].audit_id, "second");
    assert_eq!(all[1].audit_id, "first");
}

#[test]
fn audit_list_on_empty_returns_empty() {
    let d = TempDir::new("audit-empty");
    let s = JsonFsStorage::open(d.path()).unwrap();
    assert!(s.audit_list_recent(10, 0).unwrap().is_empty());
}

// ---------- users ----------

#[test]
fn users_seed_writes_file_and_returns_credentials_once() {
    let d = TempDir::new("users-seed");
    let s = JsonFsStorage::open(d.path()).unwrap();

    let creds = s.users_seed_if_empty().unwrap().expect("first seed");
    // Demo-only fixed credentials. Real deployments rotate these
    // through the (forthcoming) admin flow.
    assert_eq!(creds.admin_password, super::DEMO_ADMIN_PASSWORD);
    assert_eq!(creds.compliance_password, super::DEMO_COMPLIANCE_PASSWORD);

    // Second call is a no-op.
    assert!(s.users_seed_if_empty().unwrap().is_none());
}

#[test]
fn users_verify_password_against_hash() {
    let d = TempDir::new("users-verify");
    let s = JsonFsStorage::open(d.path()).unwrap();
    let creds = s.users_seed_if_empty().unwrap().unwrap();

    let admin = s
        .users_verify("admin", &creds.admin_password)
        .unwrap()
        .expect("verify admin");
    assert_eq!(admin.role, Role::Admin);

    let compl = s
        .users_verify("compliance", &creds.compliance_password)
        .unwrap()
        .expect("verify compliance");
    assert_eq!(compl.role, Role::Compliance);

    // Wrong password fails closed.
    assert!(s.users_verify("admin", "wrong").unwrap().is_none());
    // Unknown user is not an error — same shape as wrong password.
    assert!(s.users_verify("ghost", "anything").unwrap().is_none());
}

#[test]
fn users_get_returns_user_with_hash_but_public_form_does_not() {
    let d = TempDir::new("users-public");
    let s = JsonFsStorage::open(d.path()).unwrap();
    s.users_seed_if_empty().unwrap();
    let raw = s.users_get("admin").unwrap().unwrap();
    assert!(!raw.password_hash.is_empty());
    let pubu: PublicUser = (&raw).into();
    let json = serde_json::to_string(&pubu).unwrap();
    assert!(!json.contains("password"));
    assert!(!json.contains(&raw.password_hash));
}

// ---------- workflow ----------

#[test]
fn workflow_log_then_history() {
    let d = TempDir::new("workflow-history");
    let s = JsonFsStorage::open(d.path()).unwrap();
    s.workflow_log(&WorkflowEntry {
        audit_id: "a1".into(),
        decision: "pending-review".into(),
        decided_by: "system".into(),
        decided_at: 1,
        note: None,
    })
    .unwrap();
    s.workflow_log(&WorkflowEntry {
        audit_id: "a1".into(),
        decision: "cleared".into(),
        decided_by: "compliance".into(),
        decided_at: 2,
        note: Some("known false positive".into()),
    })
    .unwrap();
    s.workflow_log(&WorkflowEntry {
        audit_id: "a2".into(),
        decision: "blocked".into(),
        decided_by: "compliance".into(),
        decided_at: 3,
        note: None,
    })
    .unwrap();

    let h1 = s.workflow_history("a1").unwrap();
    assert_eq!(h1.len(), 2);
    assert_eq!(h1[1].decision, "cleared");

    let h2 = s.workflow_history("a2").unwrap();
    assert_eq!(h2.len(), 1);
    assert_eq!(h2[0].decided_by, "compliance");

    let recent = s.workflow_recent(10).unwrap();
    assert_eq!(recent.len(), 3);
    // Newest first.
    assert_eq!(recent[0].audit_id, "a2");
}
