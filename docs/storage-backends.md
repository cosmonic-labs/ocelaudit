# Storage backends

OcelAudit's `Storage` trait (defined in `components/storage-jsonfs/src/lib.rs`) is the swap point. Anything that implements it can plug into the gateway via the `STORAGE_BACKEND` environment variable.

This document is the honest comparison the plan asks for, including the parts that don't work yet.

## Summary

| Backend                   | Status                | Persistence    | C deps?      | wasi-sdk needed | Crate                            |
|---------------------------|-----------------------|----------------|--------------|-----------------|----------------------------------|
| `jsonfs:<dir>` (default)  | **production-grade**  | yes (filesystem) | no         | no              | `ocelaudit-storage-jsonfs`       |
| `memory:`                 | **shipped, ephemeral** | no            | no           | no              | `ocelaudit-storage-memory`       |
| `sqlite:<file>`           | future work           | yes            | yes (zstd, bundled SQLite C) | yes | (`ocelaudit-storage-sqlite`, not built) |
| `turso:<file>`            | future work           | yes            | yes (`mimalloc`)             | yes | (`ocelaudit-storage-turso`, not built)  |

## Why M11 didn't ship SQLite or Turso

Both the SQLite (`rusqlite` with `bundled`) and Turso (formerly Limbo) crates pull in a C dependency that the system `clang` cannot target without a wasi-sdk install:

```
clang -cc1as: error: unknown target triple 'wasm32-unknown-wasip2'
```

This is the exact toolchain trap PLAN.md §1.3 calls out. To unblock either backend you need:

1. wasi-sdk installed locally (https://github.com/WebAssembly/wasi-sdk/releases). On macOS, untar to `/opt/wasi-sdk` and set `WASI_SDK_PATH=/opt/wasi-sdk`.
2. `CC_wasm32_wasip2`, `AR_wasm32_wasip2`, `CXX_wasm32_wasip2` env vars pointing at `${WASI_SDK_PATH}/bin/clang`, `llvm-ar`, `clang++` respectively.
3. `CARGO_TARGET_WASM32_WASIP2_LINKER=${WASI_SDK_PATH}/bin/clang` and `RUSTFLAGS="-C link-arg=--target=wasm32-unknown-wasip2 -C link-arg=--sysroot=${WASI_SDK_PATH}/share/wasi-sysroot"`.
4. For `rusqlite`: `[dependencies] rusqlite = { version = "0.32", features = ["bundled"] }`. The bundled SQLite C source is `static.c`/`zstd_v0X.c` which will then build cleanly under the configured clang.
5. For `turso`: same setup unblocks `mimalloc-sys`. Turso also has SQLite-compat gaps; see https://github.com/tursodatabase/turso for the latest.

We chose to keep the demo dep-free at M11 rather than require every contributor to install wasi-sdk. The trait + the `MemoryStorage` reference impl prove the swap pattern works; adding `storage-sqlite` and `storage-turso` is a few hundred lines of straightforward delegation through `rusqlite` / `turso` once the toolchain is in place.

## Switching backends today

```sh
# default — jsonfs at /data
STORAGE_BACKEND=jsonfs:/data make demo

# ephemeral memory backend (CI matrix, throwaway demos)
STORAGE_BACKEND=memory: make demo
```

To make wash dev pick this up, edit `components/api-gateway/.wash/config.yaml`'s `dev` section. (Per-component env overrides on the primary component aren't ergonomic on wash 2.0.4 — see PLAN.md §0.)

## What you lose with `memory:`

- Audit log doesn't survive a host restart.
- Users are reseeded every boot (passwords printed each time).
- CSL data is reseeded every boot.
- The session signing key is still persisted (the trait's `root_path()` lives under `/tmp/ocelaudit-memory-<pid>-<ts>/`), so within a single boot, sessions remain valid.

The point of `memory:` is: tests, the CI matrix, and quick exploratory demos where you don't want any disk state. Don't run real workloads against it.

## Trait shape

The trait is in `components/storage-jsonfs/src/lib.rs`. Object-safe (no generic methods), all methods take `&self` (single-threaded async semantics inside a wasm component). Implementations are roughly 200 lines each — one file, no inheritance, no surprises.

```rust
pub trait Storage: Send + Sync {
    fn csl_metadata(&self) -> Result<Option<CslMetadata>>;
    fn csl_list_all(&self) -> Result<Vec<CslEntry>>;
    fn csl_list_by_source(&self, source: &str) -> Result<Vec<CslEntry>>;
    fn csl_get(&self, id: &str) -> Result<Option<CslEntry>>;
    fn csl_bulk_replace(&self, entries: Vec<CslEntry>, fetched_at: u64, version: String) -> Result<()>;

    fn audit_log(&self, event: &SearchEvent) -> Result<String>;
    fn audit_list_recent(&self, limit: usize, offset: usize) -> Result<Vec<SearchEvent>>;
    fn audit_get(&self, audit_id: &str) -> Result<Option<SearchEvent>>;

    fn users_seed_if_empty(&self) -> Result<Option<SeededCredentials>>;
    fn users_list(&self) -> Result<Vec<User>>;
    fn users_get(&self, username: &str) -> Result<Option<User>>;
    fn users_verify(&self, username: &str, password: &str) -> Result<Option<PublicUser>>;

    fn workflow_log(&self, entry: &WorkflowEntry) -> Result<()>;
    fn workflow_history(&self, audit_id: &str) -> Result<Vec<WorkflowEntry>>;
    fn workflow_recent(&self, limit: usize) -> Result<Vec<WorkflowEntry>>;

    fn root_path(&self) -> &Path;
}
```

## Adding a new backend

1. Create `components/storage-<name>/` next to the others.
2. `pub struct MyStorage { ... }`. `impl Storage for MyStorage { ... }`.
3. Add to `Cargo.toml` workspace members.
4. In `components/storage-jsonfs/src/config.rs`, add a `StorageBackend::MyName { ... }` variant and parse the `myname:` prefix.
5. In `components/api-gateway/src/state.rs`, add a `Cargo` dep, import `MyStorage`, extend the `match backend { ... }` with the new arm.
6. Run the existing API test suite. The whole point of the trait is that no other code should need to change.
7. Document in this file's summary table.

Same surface area, same tests, different storage. That's the swap.

## Future: real CI matrix

Today CI runs the suite against `jsonfs:` only. Adding a matrix run for `memory:` is a workflow tweak (an extra `STORAGE_BACKEND=memory:` job). Adding `sqlite:` and `turso:` is gated on the wasi-sdk install in the job — see the PLAN.md §1.7 notes for `.github/workflows/ci.yml`.
