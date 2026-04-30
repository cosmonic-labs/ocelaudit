# CLAUDE.md — orientation for future sessions

This file is loaded automatically into Claude Code's context when working in
this repository. Read it first.

## What this is

OcelAudit is a CNCF wasmCloud v2 demo that screens entities against the U.S.
Consolidated Screening List (CSL). Backend is a set of Rust WebAssembly
components glued together via WASI P3 interfaces; frontend is a static SPA
served by the same wasmCloud host. **Demo, not product.**

## Authoritative spec

`PLAN.md` at the repo root is the canonical build plan. Treat it as the
source of truth for architecture, milestones, and exit criteria. **Do not
deviate from `PLAN.md` without surfacing first** — the user spent
significant effort on it and the ordering matters (most-risky first).

## Hard constraints (do not work around silently)

- **wasmCloud v2 only.** `wash` v2.x. Anything older does not apply.
- **We target WASI P2, not P3.** Decision made on 2026-04-30 after
  empirical testing — see PLAN.md §0 for the full sequence. Short
  version: wash 2.0.4 has the `dev.wasip3: true` config schema, but
  its bundled wasmtime can't actually load a P3 component (errors
  on `stream` async feature). Sticking with the released binary
  means staying on P2 (`wasi:http@0.2.2`). Revisit P3 only when a
  wash release ships with the runtime feature on.
- **WIT version pin:** `wasi:http@0.2.2` (and the rest of WASI P2 at
  `0.2.2`). Vendored under repo-root `wit/deps/` (unversioned dir
  names) from upstream
  `wasmCloud/crates/wash-runtime/tests/fixtures/p2-wit-deps`.
- **Build target:** `wasm32-wasip2`.
- **Build flow:** `wash` 2.0.4's built-in `wkg` resolver mis-decodes
  text-WIT path overrides. Use the standalone `wkg` 0.15.0 (`cargo
  install wkg`):
  `wkg wit fetch -t wit` *then* `wash build --skip-fetch`.
  `make build` in the Makefile chains both. `components/*/wit/deps/`
  and `components/*/wkg.lock` are gitignored (regenerated).
- **No threads.** Even on P2, the wasmCloud runtime doesn't expose
  threads. Single-threaded async only. No Rayon, no
  `std::thread::spawn`, no scoped threads.
- **Tantivy is risky.** M1 spike-and-decide. Frozen after M1.
- **Storage starts as JSON-on-disk** (`storage-jsonfs` in M2). SQLite
  and Turso land in M11 alongside, not before.
- **`tools/build-wash.sh` is dormant.** Escape hatch only — invoke
  only if we need a wash capability not present in any 2.0.x release.

## Working agreements

- **Make is the single entry point.** `make build`, `make test`,
  `make dev`, `make demo`. Same targets run locally and in CI; nothing
  is CI-only.
- **Three test layers:** `make test-rust` (cargo workspace), `make
  test-api` (bash + curl + jq, boots `wash dev`), `make test-ui`
  (Playwright). `make test` runs all three.
- **End-of-milestone protocol** (per §6 of `PLAN.md`):
  ```sh
  # only after `make test` is green locally
  git add -A
  git commit -m "M<N>: <short description>"
  git tag v0.<N+1>.0
  git push origin main --follow-tags
  ```
  Then **wait for both `ci.yml` and `release.yml` to come back green**
  before starting the next milestone. If CI is red, fix forward; do not
  paper over it in the next milestone. Bug fixes between milestones bump
  patch (`v0.<N+1>.1`).
- **Pre-1.0 semver:** M0 → v0.1.0, M1 → v0.2.0, … M11 → v0.12.0. We
  reach v1.0.0 only when this graduates from demo to supported (out of
  scope here).
- **README is a living document.** Update it at the end of each
  milestone per §11.3 of `PLAN.md`. Caveats stay forever — never quietly
  remove a known limitation because polish feels embarrassed by it.
- **Honesty rules (§11.4):** no marketing voice, every claim testable,
  no fabricated benchmarks, the words "this is a demo, not a product"
  appear literally in the README.
- **Supply chain:** every `.wasm` is built via `cargo auditable build`
  (configured in `.wash/config.yaml`); release artifacts carry SLSA
  attestations from `wash-oci-publish`; CycloneDX SBOMs are attached to
  GitHub Releases. Never disable any of these to "make CI go green."

## Repo layout (see PLAN.md §1.5 for full annotated tree)

```
ocelaudit/
├── CLAUDE.md                   (this file)
├── PLAN.md                     (canonical build plan)
├── README.md                   (front door, see PLAN.md §11)
├── Makefile                    (single entry point)
├── wadm.yaml                   (default deployment)
├── .github/workflows/          (ci.yml + release.yml)
├── .wash/config.yaml           (cargo-auditable wrapping, wasip3: true)
├── tools/                      (build-wash.sh, wash-version.txt)
├── wit/deps/                   (canonical p3-wit-deps)
├── interfaces/ocelaudit/       (our WIT packages)
├── components/                 (one Rust crate per component)
├── ui/                         (Vite + Preact + TS SPA, M6+)
└── tests/{rust,api,ui,fixtures,components}/
```

## Memory

Persistent memory lives at
`/Users/liam/.claude/projects/-Users-liam-source-wasmcloud-demos-ocelaudit/memory/`.
The index is `MEMORY.md` in that directory. Use it to record user
preferences, project context, feedback, and external references — but
*not* facts derivable from the code itself (those go in code/comments).

## When you're stuck

1. Re-read `PLAN.md` §0 ("Ground truth").
2. Check `docs/m1-search-engine-decision.md` if anything around search
   feels off — it freezes the engine choice.
3. Check `tools/wash-version.txt` matches what's installed via
   `tools/build-wash.sh`.
4. If a milestone's tests aren't green, do not start the next milestone.
   Surface the failure to the user.
