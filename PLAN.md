# OcelAudit — Build Plan for Claude Code

A demonstration application for **CNCF wasmCloud v2** that screens entities
against the U.S. **Consolidated Screening List (CSL)**. Backend is a set of
modular Rust WebAssembly components; frontend is a static SPA served by the
same wasmCloud host. Theme: an ocelot, monochrome-friendly, easily replaced.

---

## 0. Ground truth (read this first, do not skip)

These constraints come from empirical verification against the released
`wash` 2.0.4 binary on 2026-04-30, the wasmCloud P3 blog post
<https://wasmcloud.com/blog/wasi-p3-on-wasmcloud/>, and the published
fixtures in the wasmCloud repo. Treat them as facts, not suggestions.

- **Hard requirement:** built on **wasmCloud v2** (`wash` v2.x).
- **We target WASI P2, not P3.** This was an early correction.
  Sequence on 2026-04-30:
  1. Original plan called for P3 (per the wasmCloud blog post).
  2. We tried `wash` 2.0.4 with `dev.wasip3: true` — the config
     field is exposed and `wash dev` accepts it (the user pushed
     back rightly that we shouldn't rebuild wash from source if
     2.0.4 already supports the schema).
  3. Building the P3 fixture works (wit-bindgen 0.54 emits a P3
     component), but loading it into wash 2.0.4's runtime fails
     with `\`stream\` requires the component model async feature
     (at offset 0xc)`. The wasmtime engine inside the released
     2.0.4 binary does not have the component-model async feature
     compiled in. The blog post's "build wash from source with
     `--features wasip3`" instruction was load-bearing for the
     *runtime*, not just the config schema.
  4. **Decision (Liam, 2026-04-30):** "just proceed with wash 2.0.4
     and build this out with WASI P2." Stay on the released binary;
     drop the P3 fixture pattern; use `wasi:http@0.2.2` and the
     standard P2 `incoming-handler` Guest. We can revisit P3 when
     a wash release ships with the runtime feature on.
- **Build target:** `wasm32-wasip2`. Same rustc target as the
  hypothetical P3 path.
- **WIT version pin:** `wasi:http@0.2.2`, `wasi:io@0.2.2`,
  `wasi:clocks@0.2.2`, `wasi:random@0.2.2`, `wasi:cli@0.2.2`.
  Vendored under repo-root `wit/deps/` (unversioned dir names)
  from the wasmCloud upstream
  `crates/wash-runtime/tests/fixtures/p2-wit-deps/` set.
- **Build flow:** `wash` 2.0.4's built-in `wkg` resolver chokes on
  text-WIT path overrides (decode error on the leading byte of
  `.wit` files — it expects binary packages). Workaround: use
  standalone `wkg` 0.15.0 (`cargo install wkg`) which understands
  text-WIT directories. The `Makefile` chains `wkg wit fetch -t wit`
  then `wash build --skip-fetch`. `components/*/wit/deps/` and
  `components/*/wkg.lock` are gitignored (regenerated artefacts);
  the workspace-root `wit/deps/` is committed and is the
  source-of-truth that `wkg.toml` overrides point at.
- **No threads.** Even on P2, threads aren't available in the
  wasmCloud runtime. Single-threaded async only.
- **Tantivy.** Treat as **risky**. Compile path is unproven on
  wasm32-wasip2. M1 spike-and-decide; frozen after M1.
- **`tools/build-wash.sh` is dormant.** It exists for the case where
  we need a wash capability that's not in any released 2.0.x —
  e.g. if we revisit P3 when its runtime is mainlined. Default
  path uses the released binary. Surface the trigger explicitly
  when you invoke the script.

If any of the above breaks, stop and ask before working around it. These
are the assumptions every later milestone depends on.

---

## 1. Architecture

### 1.1 Component graph

```
                    ┌──────────────────────────────┐
                    │   wasi:http  (P3 incoming)   │
                    └──────────────┬───────────────┘
                                   │
                    ┌──────────────▼──────────────┐
                    │  api-gateway  (Rust, P3)    │
                    │  routes /api/*, /assets/*   │
                    │  auth, audit-id, rate limit │
                    └──┬───────┬─────────┬────────┘
                       │       │         │
            ┌──────────▼┐  ┌───▼─────┐  ┌▼──────────────┐
            │ search     │  │ csl-    │  │ static-       │
            │ (Rust, P3) │  │ ingest  │  │ assets        │
            │ tantivy or │  │ (Rust)  │  │ (Rust, embeds │
            │ fallback   │  │ cron +  │  │  SPA bundle)  │
            └──┬─────────┘  │ on-demand│ └───────────────┘
               │            └────┬─────┘
               │                 │
            ┌──▼─────────────────▼─────────┐
            │  storage  (Rust, P3)         │
            │  trait-based: SQLite default │
            │  audit log, users, hits,     │
            │  workflow state, cached CSL  │
            └──────────────────────────────┘
```

Every backend component is its own crate, its own `wit/world.wit`, its own
unit tests. Components talk to each other via WIT interfaces — never via
shared Rust types. The composition is declared in `wadm.yaml`.

### 1.2 Component responsibilities

| Component       | Exports                                          | Imports                                                 |
|-----------------|--------------------------------------------------|---------------------------------------------------------|
| `api-gateway`   | `wasi:http/handler@0.3.0-rc-2026-03-15`          | `ocelaudit:search/query`, `ocelaudit:storage/*`, `ocelaudit:csl/refresh`, `ocelaudit:assets/serve` |
| `search`        | `ocelaudit:search/query`, `ocelaudit:search/index` | `ocelaudit:storage/csl-records`, `wasi:filesystem`, `wasi:clocks` |
| `csl-ingest`    | `ocelaudit:csl/refresh`, `wasi:cli/run` (for cron-style invocation) | `wasi:http/handler@0.3.0-rc-2026-03-15` (outgoing fetch), `ocelaudit:storage/csl-records`, `ocelaudit:search/index` |
| `storage-jsonfs` (M2 default) / `storage-sqlite`, `storage-turso` (M11) | `ocelaudit:storage/csl-records`, `ocelaudit:storage/audit`, `ocelaudit:storage/users`, `ocelaudit:storage/workflow` | `wasi:filesystem`, `wasi:clocks`, `wasi:random` |
| `static-assets` | `ocelaudit:assets/serve`                         | `wasi:filesystem` (or `wasi-virt`-embedded)             |

### 1.3 Storage abstraction

Storage is the one place we explicitly design for swap-out. Clean WIT
interface with one record per concern; one default implementation is
chosen and shipped first; the binding name in `wadm.yaml` selects it.
To swap to a different backend, ship a different component that exports
the same interfaces and change one binding line.

**SQLite-on-Wasm options researched (April 2026):**

| Option                         | Pros                                                                                          | Cons                                                                                                                                              |
|--------------------------------|-----------------------------------------------------------------------------------------------|---------------------------------------------------------------------------------------------------------------------------------------------------|
| `rusqlite` w/ `bundled`        | Battle-tested, full SQL feature set incl. FTS5 if we want it                                  | Compiles SQLite C source; needs `wasi-sdk` clang + custom linker flags; historically required `wasm32-wasi-vfs` shim. Recent releases improved wasip2 support but build is still fiddly. |
| `libsql` (Turso fork of SQLite)| Mature, also a C-based build                                                                  | Same toolchain headaches as rusqlite                                                                                                              |
| `turso` / `limbo` (pure Rust)  | No C toolchain. Async-native (fits P3 idiom). Builds clean to wasm32-wasip2. Includes optional tantivy-powered FTS. | Beta. SQLite compatibility incomplete. May hit features it doesn't support.                                                                       |
| Custom file format on `wasi:filesystem` | No DB at all; minimal deps; fully under our control                                  | We're writing a database. Don't.                                                                                                                  |

**Decision:** **start with the simplest mechanism that works**, ship the
whole app on it, *then* add alternatives in a later milestone. This is
explicit per the user's instruction.

- **M2 default (simplest):** an in-process **JSON-on-disk** store using
  `wasi:filesystem`. One file per concern (`csl.json`, `audit.jsonl`,
  `users.json`, `workflow.jsonl`). Append-only for the two `.jsonl`
  files; whole-file rewrite for the others (small data, atomic
  rename). No transactions, no SQL — but we get to ship M3–M9 quickly
  on a known-good substrate, and the WIT interface hides the choice.
- **M11 (alternative backends):** add `storage-sqlite` (rusqlite +
  bundled, with the wasi-sdk linker dance) **and** a pure-Rust
  alternative `storage-turso` so we have one each of C-bundled and
  pure-Rust paths. Each is its own component crate exporting the same
  WIT interfaces. `wadm.yaml` chooses one binding. M11 also writes
  `docs/storage-backends.md` comparing footprint, build complexity,
  and benchmark numbers across the three.
- **Configuration:** every storage component reads `STORAGE_BACKEND`
  env. Values: `jsonfs:/data/ocelaudit/`, `sqlite:/data/ocelaudit.db`,
  `turso:/data/ocelaudit.db`. Unknown values fail fast at startup.

The contract is the same across all three; nothing in `api-gateway`,
`search`, `csl-ingest`, or the SPA changes when we swap.

```wit
package ocelaudit:storage@0.1.0;

interface csl-records {
  record csl-entry {
    id: string,                     // canonical id from source
    source-list: string,            // SDN, EL, UVL, NS-MBS, ITAR-DPL, etc.
    name: string,
    aliases: list<string>,
    entity-type: entity-type,
    addresses: list<string>,
    nationalities: list<string>,
    programs: list<string>,
    federal-register-notice: option<string>,
    source-list-url: string,
    raw-json: string,               // full record, for UI detail view
  }
  enum entity-type { individual, entity, vessel, aircraft, unknown }
  list-all: func() -> result<list<csl-entry>, storage-error>;
  list-by-source: func(source: string) -> result<list<csl-entry>, storage-error>;
  get: func(id: string) -> result<option<csl-entry>, storage-error>;
  bulk-replace: func(entries: list<csl-entry>, fetched-at: u64) -> result<_, storage-error>;
  metadata: func() -> result<csl-metadata, storage-error>;
  // ...
}

interface audit {
  record search-event { /* id, who, query, params, top-hits, tlp, ts */ }
  log-search: func(event: search-event) -> result<string, storage-error>;
  list-recent: func(limit: u32, offset: u32) -> result<list<search-event>, storage-error>;
  get: func(audit-id: string) -> result<option<search-event>, storage-error>;
  // ...
}

interface workflow { /* yellow/red hits, decisions, reviewer, decided-at */ }
interface users    { /* hardcoded admin + compliance for the demo */ }
```

Configuration: each storage component reads `STORAGE_BACKEND` (env), e.g.
`sqlite:/data/ocelaudit.db`. The default `wadm.yaml` provides the SQLite
component; an alternate `wadm.kv.yaml` is provided as an example of the
swap pattern (does not need to fully work for M1–M9 — it just has to
demonstrate the binding swap is one-line).

### 1.4 SPA security posture

The user's concern is right: shipping the entire app to the browser
shouldn't ship secrets or admin surface area. Discipline:

- The SPA is **public, unauthenticated content + a login form**. No admin
  routes, no list of users, no DB schema, no internal endpoint inventory
  in the bundle.
- **Login** is a `POST /api/auth/login` returning a session cookie that is
  `HttpOnly`, `Secure`, `SameSite=Strict`, `Path=/`. JS in the SPA cannot
  read it. Server signs sessions with a key from config (env var
  `SESSION_SIGNING_KEY`, randomly generated on first start if absent).
- **All `/api/*` except `/api/auth/login` require the cookie** and the
  gateway enforces this in one place. Audit log records the user.
- **CSP**: strict default-src 'self'; no inline scripts; the background
  video served from `/assets/video/`. No third-party CDNs at runtime.
- **No client-side secrets**, no API keys, no user list, no role list
  embedded in the bundle. Roles arrive from `/api/me` after login.
- The two demo accounts (`admin` / `compliance`) live in the `users`
  storage component, seeded on first boot, password hashed with
  Argon2id. The seed is logged to stderr **once** so the demo runner can
  copy it; subsequent boots do not re-seed and do not log credentials.
- Rate-limit `/api/auth/login` (token bucket per remote IP) to make the
  demo feel real.
- CSRF: because the cookie is `SameSite=Strict` and we accept JSON
  bodies only on `/api/*`, we are protected for the demo. Note this in
  the README, do not pretend it's bank-grade.

### 1.5 Repo layout

```
ocelaudit/
├── README.md
├── PLAN.md                     (this file)
├── wadm.yaml                   (default deployment, jsonfs binding)
├── wadm.sqlite.yaml            (alt binding example, lands in M11)
├── wadm.turso.yaml             (alt binding example, lands in M11)
├── .github/
│   └── workflows/
│       ├── ci.yml              (every push/PR: build, test, audit, SBOM)
│       └── release.yml         (on v* tag: build, publish, attest, SBOM upload)
├── .wash/
│   └── config.yaml             (wasip3: true, cargo-auditable wrapping)
├── tools/
│   ├── wash-version.txt        (pinned wasmCloud commit SHA for local P3 build)
│   └── build-wash.sh           (clones + builds wash --features wasip3)
├── wit/
│   └── deps/                   (canonical p3-wit-deps copied here)
├── interfaces/
│   └── ocelaudit/              (our own WIT packages)
│       ├── search.wit
│       ├── storage.wit
│       ├── csl.wit
│       └── assets.wit
├── components/
│   ├── api-gateway/
│   ├── search/
│   ├── csl-ingest/
│   ├── storage-jsonfs/         (M2 default)
│   ├── storage-sqlite/         (M11)
│   ├── storage-turso/          (M11)
│   └── static-assets/
├── ui/                         (SPA, pnpm + Vite + a small framework)
│   ├── public/
│   │   └── video/ocelot.mp4    (replaceable; see config)
│   ├── src/
│   └── ocelaudit.config.json   (logo path, video path, theme tokens)
├── tests/
│   ├── api/                    (curl + jq scripts; run via `make test-api`)
│   ├── ui/                     (Playwright smoke tests)
│   └── components/             (per-component Rust tests)
└── Makefile                    (single entry point: build, test, run, sbom)
```

### 1.6 Brand assets (the ocelot)

- Logo: a single SVG `ui/public/brand/ocelot.svg`, pure black on
  transparent, designed to read at 24×24 (favicon) and 256×256 (login
  splash). Geometry only — head silhouette with three rosette spots, no
  gradients, no fills other than `currentColor`.
- Wordmark: "OcelAudit" set in a single typeface, paired with the mark.
- Theme tokens in `ui/src/theme.css` (CSS variables) — primary, accent,
  TLP green/yellow/red/white, neutrals. One file to retheme.
- Background video path in `ui/ocelaudit.config.json`. Replacing the
  video is "edit one JSON value, drop a new mp4 in
  `ui/public/video/`" — documented in README.
- Logo and config swap is its own milestone (M9), so for M1–M8 a
  placeholder ASCII-style mark in SVG is fine.

### 1.7 CI/CD, supply chain, and release

CI is set up in **M0** (not as an afterthought) using the official
wasmCloud GitHub Actions per
<https://wasmcloud.com/docs/kubernetes-operator/operator-manual/cicd/>.
We push at the end of every milestone — the user's repo is already
wired upstream — and CI gates the green checkmark.

**Two workflows**, both under `.github/workflows/`:

#### `ci.yml` — runs on every push and PR

Single source of truth for what "passing" means. The same
Make targets run locally and in CI; nothing is CI-only.

- `setup-wash-action` (pinned version) → installs wash + the
  `wasm32-wasip2` Rust target.
- `setup-wash-cargo-auditable` → installs `cargo-auditable` and
  `cargo-audit`, configures `.wash/config.yaml` so `wash build`
  uses `cargo auditable build` under the hood. Result: every
  compiled `.wasm` ships **embedded dependency metadata** that
  `cargo audit bin <component>.wasm` can read for SBOM-style
  reporting. This is the wasmCloud-recommended supply-chain
  story today.
- `make build` → `wash-build` for each component crate
  (matrix per component).
- `make test` → exact same target a developer runs locally.
  Internally chains `make test-rust` → `make test-api` →
  `make test-ui`. Per §7.
- `make audit` → `cargo audit` against each `.wasm` artifact;
  failing advisories fail the build.
- `make sbom` → CycloneDX SBOM generated for every crate, merged
  into one document; uploaded as a workflow artifact named
  `sbom-<commit-sha>.cdx.json`.
- Failure semantics: any step fails → workflow fails → branch
  protection blocks merge.

#### `release.yml` — runs on `v*` tag push

This is the **gated release** workflow. The tag is created at the
end of each milestone (`v0.1.0`, `v0.2.0`, …) and triggers the full
build → test → publish → attest → release pipeline.

The structure is `test → build → publish → release`, with each
stage gating the next. **No artifacts get published if any test
fails**, even if the tag was already pushed. A failed release run
on a tag means: delete the tag, fix forward on `main` with a patch
bump (`v0.X.Y+1`), retag, push.

```
on:
  push:
    tags: ['v*']

permissions:
  contents: write       # create the GitHub Release
  packages: write       # push to GHCR
  attestations: write   # SLSA attestations
  id-token: write       # OIDC for attestation signing

jobs:
  test:                 # runs the full ci.yml test matrix
    uses: ./.github/workflows/ci.yml
  build:
    needs: test
    # wash-build all components, generate SBOM
  publish:
    needs: build
    # wash-oci-publish each component to GHCR with attestation: "true"
  release:
    needs: publish
    # gh release create v0.X.0 with auto-generated notes,
    # SBOM attached, links to attestations
```

What each release produces:

- **OCI artifacts** at `ghcr.io/<owner>/ocelaudit-<component>:v0.X.0`
  and `ghcr.io/<owner>/ocelaudit-<component>:latest`, one per
  component. Multi-arch is not needed (wasm is portable).
- **Per-artifact SLSA build provenance** generated by
  `wash-oci-publish`'s `attestation: "true"` flag and uploaded
  to the registry alongside the OCI image. Verifiable with
  `gh attestation verify oci://<image-ref>`.
- **CycloneDX SBOM** (`sbom-v0.X.0.cdx.json`) attached to the
  GitHub Release.
- **`auditable` metadata** embedded inside each `.wasm`
  (built into the artifact itself, not a sidecar).
- **GitHub Release** with auto-generated notes:
  - Title: "OcelAudit v0.X.0 — M<N>: <milestone description>"
  - Body sections: "What changed" (commits since previous tag,
    grouped by Conventional Commit prefix), "Artifacts" (table
    of all published OCI images with digests), "Verification"
    (copy-paste recipe for `gh attestation verify`),
    "Known issues" (link to README's caveats section).

**Per-milestone protocol** for Claude Code:

```
# end of every milestone, after `make test` passes locally
git add -A
git commit -m "M<N>: <short description>"
git tag v0.<N+1>.0
git push origin main --follow-tags
# then watch CI; do not start M<N+1> until both ci.yml AND
# release.yml are green
```

`ci.yml` running on `push` to `main` provides the test gate; the
`v0.<N+1>.0` tag triggers `release.yml` which **re-runs the full
test suite via the reusable workflow** before publishing anything.
If either workflow fails, fix forward — do not move to the next
milestone.

**Pinning.** Both local development and CI use the released `wash`
2.0.4 binary (now P3-capable per §0). `setup-wash-action` in CI
pins to `v2.0.4`. `tools/wash-version.txt` records `2.0.4` and a
SHA fallback for the source-build escape hatch only — that path
is dormant unless we hit a P3 capability that hasn't been released.

---

## 2. CSL data details

- Source: International Trade Administration's CSL, published as JSON
  at <https://api.trade.gov/static/consolidated_screening_list/consolidated.json>
  (verify URL on first run; the agency has changed paths before).
- 11 sub-lists per the user's spec — capture the actual list as the
  CSV/JSON's `source` field values and surface them in the UI as
  faceted filters. Do not hardcode the names; derive from the data.
- Size: ~5–15 MB. Cache on disk under `/data/csl/` with the file naming
  `consolidated.<utc-iso8601>.json`. Keep last 7 daily snapshots. The
  newest one is always symlinked to `consolidated.latest.json`.
- On startup `csl-ingest` does:
  1. If `/data/csl/consolidated.latest.json` exists and is < 30h old,
     load it and signal `search` to (re)build the index. Done.
  2. Else fetch from the trade.gov URL with a 30s timeout. On success,
     write a new dated snapshot, update the symlink, signal `search`.
  3. On fetch failure with no usable cached copy, the component logs
     a warning and serves an empty index — the API returns `503` for
     search, the UI shows a "data unavailable" banner. The app does
     not crash.
- Cron: at 05:01 local daily (use `wasi:clocks/wall-clock` plus a
  sleep-loop subtask spawned at boot — there is no host cron). On each
  tick, run the same fetch flow as above. The "Update now" button in
  the UI calls `POST /api/csl/refresh` which invokes the same code path
  with an `audit-id` recording who triggered it.

---

## 3. Search and matching

### 3.1 Index contents

Per CSL entry: name, aliases, addresses, nationalities, programs,
entity-type. The search component owns the index lifecycle and
exposes:

```wit
interface query {
  record search-params {
    q: string,
    sources: option<list<string>>,        // filter to sub-lists
    entity-types: option<list<entity-type>>,
    fuzzy: bool,
    max-edits: option<u8>,                // 0..2
    limit: u32,
  }
  record hit {
    entry-id: string,
    score: float32,                       // normalized 0..1
    matched-fields: list<string>,
    snippet: string,
    tlp: tlp-level,                        // see §3.3
  }
  enum tlp-level { green, yellow, red }
  search: func(params: search-params) -> result<list<hit>, search-error>;
  autocomplete: func(prefix: string, limit: u32) -> result<list<string>, search-error>;
}
```

### 3.2 Indexing: tantivy attempt then fallback

**M1 spike, gated.** Attempt 1: tantivy with default features in
`wasm32-wasip2`. Almost certainly will fail to compile (mmap, threading,
file locks). Attempt 2: tantivy with `default-features = false` and only
single-threaded features enabled, with a custom in-memory `Directory`
implementation (similar to `tantivy-wasm`'s approach). Attempt 3 (if
attempt 2 fails to either compile or run inside wasmCloud): drop tantivy
and use the **fallback**:

- In-memory inverted index over normalized tokens (lowercased, NFKC,
  punctuation-stripped, accent-folded).
- Trigram index for substring/typo recall.
- Scorer: BM25 on whole-token matches + Jaro-Winkler on candidate names
  (top-K=200) for the final fuzzy ranking.
- N-gram autocomplete from name + aliases.

The fallback is not impressive on the wire, but it's deterministic,
single-threaded, fast on 5–15 MB, and entirely owned by us. It lives in
the same component crate as the tantivy code path, behind a Cargo
feature `tantivy-engine`. M1 ends with a written one-page decision
document in `docs/m1-search-engine-decision.md` recording which path
won, the reproducible build steps, and what we tested. **All later
milestones build against whichever engine M1 selected.** Do not revisit
this decision after M1 without a new spike.

### 3.3 TLP scoring

Per query, compute a TLP level for the *top hit* and for the *result
set as a whole*:

- **RED**: exact case-insensitive match on `name` or any `alias`, OR
  any single hit with combined score ≥ 0.95.
- **YELLOW**: best score in `[0.75, 0.95)`, OR ≥ 3 hits with score
  ≥ 0.6 and the same source list.
- **GREEN**: everything else (including empty result set).

Defaults are configurable per deployment via env (`TLP_RED_THRESHOLD`,
`TLP_YELLOW_THRESHOLD`). The UI exposes them in an admin-only "Scoring"
panel.

### 3.4 Workflow on hit

Every search produces an `audit-id` (UUIDv7, returned in API response
and rendered in the UI). The audit row stores: `audit-id`, `who`,
`when`, `params`, `top-hits[5]`, `tlp`, `decision`, `decided-by`,
`decided-at`.

- **GREEN**: auto-cleared. `decision = "auto-green"`. Visible in
  /audit but does not appear in /review.
- **YELLOW**: enters review queue. API caller receives
  `decision = "pending-review"`. Compliance officer can mark
  `cleared` or `blocked` from /review.
- **RED**: enters review queue with `decision = "pending-block"`.
  Same two outcomes possible. The API contract: callers must treat
  `pending-block` as a hard fail until decision is recorded.

This is a workflow, not a CRUD endpoint, so the API exposes it
explicitly: `GET /api/audit/{id}` returns the *current* decision
state, callers poll. (Streaming via P3 streams is a stretch in M9.)

---

## 4. API surface (v1)

All under `/api/v1`. JSON in, JSON out. Cookie auth except `/auth/login`.
Every response carries `X-OcelAudit-Audit-Id` where applicable.

```
POST   /auth/login                {username, password} -> sets cookie, {role}
POST   /auth/logout               clears cookie
GET    /me                        {username, role}

GET    /csl/metadata              fetched_at, count, sources[], version
POST   /csl/refresh               admin-only, triggers ingest
GET    /csl/sources               []list of sub-list names + counts
GET    /csl/entries/{id}          full entry detail

POST   /search                    {q, sources?, entity_types?, fuzzy?, limit?}
                                  -> {audit_id, tlp, hits[]}
GET    /search/autocomplete?q=    -> [string]
GET    /search/history            paginated, filterable by user/date/tlp

POST   /screen/ofac               convenience: name+dob+nationality, returns
                                  decision + audit_id, hits filtered to OFAC sources
POST   /screen/pep                convenience: name+country+role, hits filtered
                                  to PEP-relevant sources
GET    /audit/{audit_id}          decision state + history
GET    /audit                     paginated audit log (admin/compliance)

GET    /review                    queue of yellow/red items (compliance)
POST   /review/{audit_id}/decide  {decision: "cleared"|"blocked", note}

GET    /metrics                   counts: total entries by source, queries
                                  today, tlp histogram, last refresh
```

OpenAPI 3.1 doc lives at `/api/openapi.json` and the Swagger UI at
`/api/docs` (admin-only in production, demo-open here).

Help text and external links — for every CSL source there's a static
map in `interfaces/ocelaudit/source-meta.json` (BIS Entity List → bis.doc.gov,
OFAC SDN → treasury.gov/ofac, State DPL → state.gov, etc.). The UI
renders these in tooltips on hover. The map ships as data, not code,
so it's editable without rebuild.

---

## 5. UI

### 5.1 Stack

- Vite + Preact (smaller than React, plenty for this) + TypeScript.
- Tailwind for utility CSS, with a tiny custom design layer in
  `theme.css` for TLP colors and brand palette.
- TanStack Query for fetch caching, TanStack Router for routing.
- No state lib beyond Query — keeps the bundle tight.
- Bundle target: < 250 KB gzip without the video.

### 5.2 Pages

1. `/login` — full-bleed background video, centered card, brand mark,
   two demo-account hints rendered server-side from a non-secret env
   var so the demo is self-explanatory.
2. `/dashboard` — TLP-colored KPI cards (today's queries by TLP, list
   size, last refresh, queue depth), search bar with autocomplete.
3. `/search` — full search with filters (sources, entity type, fuzzy
   toggle, max-edits slider). Results are TLP-banded cards with score
   bars, expandable to full record + outbound gov links.
4. `/audit` — searchable, filterable table; click-through to detail.
5. `/review` — only items needing a decision. Two-button decide UI
   with required note. Compliance + admin only.
6. `/admin/scoring` — thresholds editor. Admin only.
7. `/admin/data` — CSL metadata, "Update now" button. Admin only.

### 5.3 Feel

- Single navigation bar, fixed top, dense.
- Background video on `/login` only — it's gated behind login, so on
  every other page a faint gradient stands in.
- All TLP colors carry **icon and text labels too**, never color alone
  (a11y).
- Tooltips on every CSL source name, every score, every TLP badge,
  with link out to the relevant agency reference page.
- Autocomplete: 150 ms debounce, max 8 suggestions, prefix-only.
- Keyboard: `/` focuses search from anywhere.

### 5.4 Configurability

- `ui/ocelaudit.config.json` controls: logo SVG path, wordmark text,
  background video path, primary color, accent color. Read at build
  time and at runtime (a small `/api/v1/branding` endpoint lets
  ops change theme without a rebuild — payload: same shape, JSON).
- Background video: drop file in `ui/public/video/`, edit one path in
  the JSON. Documented in README §"Replacing the brand".

---

## 6. Milestones (most-risky first, UI last)

Each milestone ends with **all tests passing** locally via `make test`,
then:

```
git add -A
git commit -m "M<N>: <short description>"
git tag v0.<N+1>.0       # M0 → v0.1.0, M1 → v0.2.0, ..., M11 → v0.12.0
git push origin main --follow-tags
```

**Pre-1.0 semver convention.** While we're below 1.0, each milestone
is a minor version bump. M0 ships as `v0.1.0` (the first releasable
slice — bootstrap + CI + a working hello P3 component). Patch
versions (`v0.1.1`, `v0.1.2`, …) are reserved for fixes between
milestones — bug fixes, doc fixes, dependency bumps. We hit `v1.0.0`
only when the project graduates from "demo" to "supported", which is
explicitly out of scope here. Mapping:

| Milestone | Version  |
|-----------|----------|
| M0        | v0.1.0   |
| M1        | v0.2.0   |
| M2        | v0.3.0   |
| M3        | v0.4.0   |
| M4        | v0.5.0   |
| M5        | v0.6.0   |
| M6        | v0.7.0   |
| M7        | v0.8.0   |
| M8        | v0.9.0   |
| M9        | v0.10.0  |
| M10       | v0.11.0  |
| M11       | v0.12.0  |

The push triggers `ci.yml`; the tag triggers `release.yml` which
**runs the full test suite again as a gate** before doing anything
artifact-producing. Only on green does it build, publish to GHCR
with SLSA attestations, attach the CycloneDX SBOM, and cut a GitHub
Release with auto-generated notes (commits since the previous tag,
which artifacts were published, links to attestation + SBOM).

**Both workflows must be green before starting M<N+1>.** If CI is
red, fix forward — don't paper over it in the next milestone. Bug
fixes between milestones bump patch (`v0.<N+1>.1`, etc.) and go
through the same release flow.

(M0 sets up CI itself, so for M0 only, CI must succeed at least
once before M1 starts.)

### M0 — Bootstrap + CI (1 commit)

- Repo scaffold per §1.5. Component crates build to wasm via
  `wkg wit fetch -t wit` + `wash build --skip-fetch` (per §0).
- Each component has its own `.wash/config.yaml` with `build.command`
  + `build.component_path`. No `wasip3:true` (we're on P2 — see §0).
- `tools/build-wash.sh` exists as a dormant escape hatch only.
  Default path uses the released `wash` 2.0.4 binary on `$PATH`.
- `Makefile` targets: `build`, `test`, `dev`, `lint`, `fmt`, `clean`,
  `sbom` (runs `cargo cyclonedx` per crate, merges into one file).
- A single hello-world `api-gateway` exporting P3 `wasi:http/handler`
  that returns `200 "ocelaudit booting"`. Adapted from the blog post's
  fixture verbatim.
- **CI from day one** per §1.7:
  - `.github/workflows/ci.yml` — `setup-wash-action`,
    `setup-wash-cargo-auditable`, `wash-build`, `cargo test`,
    `cargo audit`, CycloneDX SBOM upload.
  - `.github/workflows/release.yml` — gated on `v*` tags;
    `wash-oci-publish` to `ghcr.io/<owner>/ocelaudit-api-gateway`
    with `attestation: "true"`. Permissions block has the four
    required scopes (contents, packages, attestations, id-token).
  - `.wash/config.yaml` configured to use `cargo auditable build`.
- **README skeleton** per §11: every section from §11.1 lands as a
  heading, with real content for the parts that exist now (§11.2
  ASCII diagram, supply-chain verification recipe, WASI P3 caveats,
  quick start) and `TODO (M<N>)` markers for sections that grow
  later. The skeleton is **never empty headings only** — anything
  that's true at M0 is written down at M0.
- **Run end-to-end on `wash dev` with `wasip3: true`.** This is the
  ground truth that the toolchain works before anything else lands.
- Tests: `tests/api/m0-hello.sh` curls `/` and asserts
  `200 ocelaudit booting`.
- **Exit criteria**: `make test` is green; `wash dev` serves the page;
  `git push origin main --follow-tags` (tag `v0.1.0`) results in
  green `ci.yml` AND green `release.yml`. Because `release.yml`
  re-runs the full test suite as a gate, the green release implies
  the published OCI artifact carries a verifiable SLSA attestation
  and a CycloneDX SBOM is attached to the GitHub Release.

### M1 — Search engine decision (RISKIEST — gate the rest of the project)

- Spike `search` component, three attempts as in §3.2.
- Build a synthetic 10k-record index, run a fixed set of 50 queries
  (in `tests/components/search-fixtures/`), record latency p50/p95
  and recall@10 against a hand-labeled gold set.
- Write `docs/m1-search-engine-decision.md`: which engine won, the
  build incantation, what failed in the others, perf numbers, known
  limitations.
- Land the chosen engine behind the `query` interface from §3.1.
- Tests: `cargo test -p search` — unit tests for tokenizer, scorer,
  TLP banding, and the 50-query fixture run as an integration test
  with hard-coded expected top-1 IDs.
- **Exit criteria**: a written decision doc, a passing test suite,
  p95 query latency < 100 ms on the fixture. If we cannot meet this,
  STOP and replan with the user before M2.

### M2 — Storage component (simplest path: JSON on `wasi:filesystem`)

- `storage-jsonfs` crate. Implements `csl-records`, `audit`, `users`,
  `workflow` per §1.3 over four files in a config-driven directory.
- `csl.json`: whole-file replace on bulk-replace, atomic rename
  (`csl.json.tmp` → `csl.json`).
- `audit.jsonl` and `workflow.jsonl`: append-only newline-delimited
  JSON. List operations stream the file with a bounded line count.
- `users.json`: whole-file replace, hashed passwords (Argon2id).
- All writes go through a small in-memory write-coalescing layer with
  `wasi:filesystem` `sync_data` on each batch — good enough for a demo.
- `STORAGE_BACKEND` env var read at start; for M2 only `jsonfs:<dir>`
  is accepted; unknown values fail fast. Code structured so that
  adding `sqlite://` or `turso://` later in M11 is a new module, not
  a refactor.
- Seed admin/compliance users on first boot; log seed credentials to
  stderr exactly once.
- Tests: `cargo test -p storage-jsonfs` with a temp dir per test.
  Concurrency tests for the append-only logs.
- **Exit criteria**: `make test` green; reading and writing all four
  interfaces via the gateway's `/api/v1/me` and a debug
  `/api/v1/audit/_test` endpoint (deleted in M5).

### M3 — CSL ingest + scheduled refresh

- `csl-ingest` component. Fetch logic per §2. Snapshot rotation. Cron
  loop in a `wit_bindgen::spawn`-launched subtask: compute next 05:01
  local from `wasi:clocks/wall-clock`, sleep, refresh, repeat.
- Bulk-replace into storage. Trigger reindex via `search.index()`.
- Manual trigger via internal interface so M4 can call it from the API.
- Tests: integration test that points the fetcher at a local
  fixture file (a stripped-down `consolidated.json` checked into
  `tests/fixtures/csl/`), verifies storage and index were updated.
  Cron logic tested by injecting a mock clock.
- **Exit criteria**: cold start with no data on disk → fetch (or
  load fixture in test mode) → index built → `/api/v1/csl/metadata`
  shows correct count.

### M4 — API gateway: real routes (no UI yet)

- Implement all routes in §4 except `/admin/branding` (M9).
- Cookie-based auth, Argon2id password verify, signed-session
  cookie, rate limit on `/auth/login`. `SESSION_SIGNING_KEY` env var.
- Audit ID generation, TLP banding, workflow state machine.
- OpenAPI 3.1 served at `/api/openapi.json`. Generate from a single
  source of truth (e.g. `utoipa` macros).
- Tests: `tests/api/*.sh` — bash + curl + jq, one script per route
  group. Each script is `set -euo pipefail` and produces a row of
  ✓/✗ output. `make test-api` runs them all and exits non-zero if
  any fail. Coverage targets: auth happy/sad path, search with each
  TLP outcome, /screen/ofac, /screen/pep, /audit pagination, /review
  decide flow, /csl/refresh as admin and as compliance (403).
- **Exit criteria**: every API in §4 returns expected shapes; full
  test suite green.

### M5 — Hit workflow polish + screening conveniences

- `/screen/ofac` and `/screen/pep` with the right defaults and
  source filters. Source-meta JSON wired through to API responses
  so the UI can show citations later.
- Block-on-pending-block contract verified. Audit detail returns
  full history of decision changes.
- Source-meta map authored: at minimum entries for SDN, EL, UVL,
  NS-MBS, ITAR-DPL, Sectoral Sanctions, FSE, PLC, NS-Plc, with
  authoritative URLs. (Verify each at write-time.)
- Tests: scripted scenarios — "name on SDN screened by compliance,
  blocked, second screen of same name returns blocked immediately"
  etc.
- **Exit criteria**: scenario tests green.

### M6 — Static-assets component + SPA shell

- `static-assets` component: serves `/`, `/assets/*`, `/video/*`
  from an embedded virtual filesystem (`include_bytes!` or
  `wasi-virt`). Caches strongly, sets correct MIME types, gzip.
- SPA shell only: routing, login page (talking to real
  `/api/v1/auth/login`), dashboard with stub widgets, theme tokens,
  brand placeholder mark.
- Background video plays on `/login` and is config-driven.
- Tests: Playwright smoke — load `/`, see login, login as
  compliance, see dashboard, logout. CSP headers asserted.
- **Exit criteria**: full login round-trip works against the real
  backend, SPA bundle < 250 KB gzipped (without video).

### M7 — Search & dashboard pages

- `/search` page with filters, results, score bars, TLP banding,
  source citations from M5's source-meta.
- `/dashboard` KPI cards, autocomplete in the global search bar.
- All inline help / tooltips / outbound gov links.
- Tests: Playwright — search a known SDN entry, confirm RED banner,
  confirm audit id displayed, click through to detail and out to
  treasury.gov (mock the navigation, just assert href).
- **Exit criteria**: a non-developer can run the demo from login →
  search → see a real RED hit and understand it.

### M8 — Audit, review, admin pages

- `/audit`, `/review`, `/admin/scoring`, `/admin/data` per §5.2.
- Decision UI with required note.
- Threshold editor that persists to storage (not env, not bundle).
- "Update now" wired to `/api/v1/csl/refresh`.
- Tests: Playwright — a yellow hit appears in /review for a
  compliance officer, decision flips to cleared, audit detail
  shows decision history.
- **Exit criteria**: full workflow demonstrable end-to-end.

### M9 — Brand swap milestone

- Real ocelot SVG mark per §1.6 (single-color, currentColor-driven,
  geometry only). Wordmark.
- `/api/v1/branding` runtime endpoint reading from storage; admin
  page to update logo URL, wordmark, video URL, primary/accent
  colors. Reflected live without rebuild.
- README §"Replacing the brand" with a 5-step recipe.
- Tests: change brand via admin page, reload, see new logo and
  video; revert; assert default restored.
- **Exit criteria**: replacing the ocelot is a one-page docs read.

### M10 — Demo polish

- Loading and empty states everywhere. 404, 500, 503 pages.
- A `make demo` target that: builds wash, builds all components,
  applies wadm, seeds users, fetches CSL once (or loads fixture),
  prints login URL and credentials, opens browser.
- A 90-second walkthrough script in `docs/demo-script.md`.
- A short Loom-ready CONTRIBUTING note pointing at the riskiest
  files.
- **Exit criteria**: cold-clone → `make demo` → working login in
  < 5 minutes on a clean Linux box.

### M11 — Alternative storage backends (proves the abstraction works)

This milestone does *not* change the running app. It demonstrates that
the storage interface is genuinely swappable by shipping two more
implementations alongside `storage-jsonfs`.

- `storage-sqlite`: rusqlite + bundled. Build via the wasi-sdk linker
  recipe (cargo will need `WASI_SDK_PATH`, `CARGO_TARGET_*_LINKER`,
  and the `--no-entry` rustc-link-arg from build.rs). Export the
  same WIT interfaces, same migrations as embedded SQL. Verify it
  actually runs in wasmCloud, not just compiles.
- `storage-turso`: pure-Rust Turso (formerly Limbo). Same WIT
  contract. No wasi-sdk needed.
- `wadm.sqlite.yaml` and `wadm.turso.yaml`: each one is identical to
  the default `wadm.yaml` except for the storage component image
  reference. Document the swap as a single line change.
- `STORAGE_BACKEND` env now accepts all three prefixes; unknown
  fails fast; mismatched component vs. env (e.g. binding
  `storage-jsonfs` but env says `sqlite://`) fails fast.
- `docs/storage-backends.md`: footprint, build complexity, full
  benchmark numbers (insert 100k audit rows, paginated read,
  full-table scan) for all three. Honest pros/cons.
- Tests: every existing test runs against all three backends in CI
  via a matrix job. Same green checkmarks, three columns.
- **Exit criteria**: CI matrix is green across `jsonfs`, `sqlite`,
  `turso`. Swapping backends is documented as a single-line edit.

---

## 7. Testing strategy

**Local-first, CI-identical.** Every test that runs in CI runs locally
through the same Make target with the same arguments. There are no
CI-only steps and no local-only shortcuts. If a developer can run
`make test` and see green, they can push with confidence that CI will
agree. This is non-negotiable — it's the primary mechanism that keeps
"works on my machine" out of this codebase.

### 7.1 The three layers

1. **Per-component Rust unit tests** (`make test-rust`) —
   `cargo test --workspace`. Pure-Rust logic, no host calls,
   no `wash dev` required. Fastest feedback; runs in seconds.
   Includes the M1 fixture-query suite for the search component.
2. **API integration tests** (`make test-api`) — bash scripts under
   `tests/api/`, each one `set -euo pipefail` and printing
   `✓` / `✗` per assertion. Each script is independently runnable
   (`./tests/api/m4-search.sh`) for tight iteration on one feature.
   `make test-api` boots `wash dev` in the background, waits for
   `/healthz`, runs every script, captures output, tears down on
   exit (trap-based cleanup so Ctrl-C doesn't leak a host).
3. **UI smoke tests** (`make test-ui`) — Playwright under
   `tests/ui/`. Reuses the same `wash dev` boot from
   `make test-api`. Headless by default; `make test-ui-headed` for
   debugging.

### 7.2 Make targets (the contract)

```
make test            # everything, fail-fast, sequential
make test-rust       # cargo test --workspace, no host needed
make test-api        # boots wash dev, runs bash suite, tears down
make test-ui         # Playwright, headless
make test-ui-headed  # Playwright with browser visible
make test-watch      # cargo-watch on test-rust for inner-loop dev
make test-one TEST=  # run a single test by name
                     #   make test-one TEST=tests/api/m4-search.sh
                     #   make test-one TEST=search::tokenizer::lowercase
```

### 7.3 Local developer ergonomics

- `make test-rust` < 10 seconds on a clean machine. Keep it that way;
  if a test gets slow, mark it `#[ignore]` and move it to a slower
  tier behind `make test-slow`.
- `make test-api` boots a single `wash dev` once, runs all scripts
  against it, then tears it down. Do not boot per script.
- A `tests/api/_lib.sh` file holds shared helpers: `wait_for`,
  `expect_status`, `expect_json_field`, `assert_eq`. Every script
  sources it. Output is uniform.
- Test fixtures (CSL JSON, golden query results, brand assets) live
  under `tests/fixtures/`. Never reach for the live trade.gov URL
  in tests — fixture-only.
- Playwright traces are saved to `tests/ui/.traces/` on failure
  with the same filename pattern CI uses, so a failure in CI can
  be reproduced locally with `make test-ui` and the same trace
  artifact path.
- `make dev` boots `wash dev` for manual exploration. Separate from
  the test-suite host so a developer can have both running.

### 7.4 CI parity

`ci.yml` runs *exactly* these targets:

```yaml
- run: make build
- run: make test           # all three layers, same as local
- run: make audit
- run: make sbom
```

No bespoke CI scripts, no copy-paste of test commands into YAML.
If a developer wants to add a new test, they drop it into
`tests/{rust,api,ui}/` and the existing target picks it up. CI
notices automatically.

### 7.5 The release-time test gate

`release.yml` calls `ci.yml` as a reusable workflow before doing
anything that produces an artifact. Concretely: the `test` job
runs the same `make test` invocation, and the `build` / `publish`
/ `release` jobs all `needs: test`. A failed test on the tag run
means **no OCI push, no GitHub Release, no SBOM upload**. The
tag remains in git but no artifacts exist for it — the broken
state is recoverable by deleting the tag and bumping patch.

This is intentional: the tag is a developer's *intent* to release;
the artifacts are produced only when CI agrees the intent is
sound.

### 7.6 Coverage targets per milestone

Each milestone's "Tests:" line in §6 enumerates what gets added.
By the end:

- M0: smoke (`/` returns 200).
- M1: 50-query fuzzy fixture suite + tokenizer/scorer/TLP units.
- M2: full storage interface coverage on temp dirs.
- M3: ingest with mocked fetcher + mock-clock cron.
- M4: every API route, happy + sad paths.
- M5: workflow scenarios (yellow → cleared, red → blocked).
- M6: SPA login round-trip, CSP headers asserted.
- M7–M8: Playwright per page, full search → review flow.
- M9: brand swap end-to-end.
- M10: cold-start budget (`make demo` < 5 min, asserted).
- M11: matrix-test the full suite across all three storage backends.

---

## 8. Things explicitly out of scope

- Multi-tenant. One tenant, demo-grade.
- HTTPS termination — wasmCloud's HTTP server handles plain HTTP;
  put a reverse proxy in front for TLS in any real deployment, and
  say so in the README.
- Real OAuth/SSO. Two seeded users only.
- Persistent sessions across host restart. Restart logs everyone
  out — acceptable for a demo.
- PEP data. The CSL doesn't include PEP per se; `/screen/pep`
  filters to the closest available signals (e.g., specially
  designated officials in OFAC) and the UI is honest that this
  isn't a true PEP database.
- Streaming search results via P3 `stream<u8>`. Worth doing for
  the wow factor in a follow-up; not on the critical path here.

---

## 9. Open questions to resolve as they come up

- Does the canonical CSL JSON URL still resolve at the path above
  on the day M3 starts? Check before coding the fetcher; trade.gov
  has changed paths. Capture in `docs/m3-csl-source.md`.
- Are wasmCloud P3 release candidate WIT versions still
  `0.3.0-rc-2026-03-15` when M0 starts? If newer, update the
  pinned WIT deps and the WIT version pin in this doc.
- Does the user's GitHub repo have GHCR write access set up by the
  time M0 pushes its first tag? If not, the `release.yml` workflow
  will fail with a 403 on `wash-oci-publish` — surface it
  immediately rather than working around it. (Once-per-repo: enable
  GHCR, set the package visibility, ensure
  `Settings → Actions → General → Workflow permissions` is
  "Read and write".)
- For M11: is `turso` (formerly Limbo) past the breaking-change
  thrash that was visible in early 2025? Check the changelog before
  starting; if it's still moving fast, pin to a specific commit
  rather than a published crate version.

---

## 10. Hand-off checklist for Claude Code

When picking this up, in order:

1. Read this file end to end, then re-read §0.
2. Read the wasmCloud P3 blog post linked in §0 and the
   `http-handler-p3` fixture in the wasmCloud repo. Mirror its
   `Cargo.toml`, `wit/world.wit`, and bindgen incantation in M0
   exactly. Do not improvise.
3. Read the wasmCloud CI/CD doc
   <https://wasmcloud.com/docs/kubernetes-operator/operator-manual/cicd/>
   and use its workflow as the starting template for `release.yml`,
   adapting tag triggers and matrix builds for our component set.
4. Use the released `wash` 2.0.4 binary on `$PATH`. Set
   `dev.wasip3: true` in `.wash/config.yaml` (per §0). Only invoke
   `tools/build-wash.sh` if a P3 capability we need is missing
   from 2.0.4 — surface the trigger explicitly in that case.
5. Work milestone by milestone. **End each milestone with the
   git commit + tag + push protocol from §6.** Do not start the
   next milestone until both `ci.yml` and `release.yml` are green
   on the upstream repo.
6. **Update `README.md` at the end of every milestone** per §11.
   The README is a living document; it is not written once at the
   end. Each milestone's exit criteria explicitly include the
   README sections it touches.
7. Don't start a milestone whose previous milestone's tests are
   not all green locally AND in CI.
8. If M1's tantivy spike fails all three attempts, stop and
   surface to the user before falling back. The fallback is
   real but the user wanted to know.
9. Storage in M2 is **JSON-on-disk only**. Resist the temptation
   to skip ahead to SQLite — it lands in M11 and the whole point
   of the ordering is to get the app running on the simplest
   substrate first.

---

## 11. README contract

The repo's `README.md` is the front door. It must be **comprehensive,
honest, and accurate** — what's real vs. what's a demo simulation
matters more than polish. Treat this section as the spec for what
`README.md` must contain by the end of the project, with notes on
which milestone first lands each section.

### 11.1 Required structure (in this order)

1. **Title + one-line tagline + status badges**
   - Project name, ocelot mark inline, one sentence on what it does.
   - Badges: CI status, release version, license, "demo only" badge.

2. **What this is, and what it isn't** (lands M0, refined throughout)
   - Plain English: a wasmCloud v2 demonstration that screens entities
     against the U.S. Consolidated Screening List.
   - **Explicit "not for production" disclaimer**, with the specific
     reasons (no HTTPS, demo auth, single-tenant, no SLA on CSL data).
   - Who this is for (developers evaluating wasmCloud, compliance
     teams curious about WASI-based architectures, conference demos).

3. **30-second demo** (lands M0, polished M10)
   - The exact `make demo` invocation and what to expect.
   - Default URL, default credentials, where they come from.
   - A screenshot or asciinema cast at top of README (lands M9).

4. **Architecture overview** (lands M0 skeleton, expanded each milestone)
   - **ASCII component diagram** (see §11.2 below for the canonical
     version). Updated whenever the component graph changes.
   - One-paragraph-per-component: responsibility, what it imports,
     what it exports, where its WIT lives, and the Cargo crate path.
   - Request lifecycle for one representative call (e.g.,
     `POST /api/v1/search`): browser → gateway → search → storage →
     gateway → browser, with a sentence per hop.
   - **Trust boundaries diagram**: what runs in the sandbox, what
     runs on the host, what crosses NATS, what's served to the
     browser. Lands M6 when the SPA appears.

5. **Technology choices and rationale** (lands per milestone as
   the choices are made)
   - **Table** with columns: layer / choice / why / alternatives
     considered / where it lives.
   - Required rows by end of project: wasmCloud v2 (host), WASI P3
     (interface), Rust (component language), wit-bindgen with
     async-spawn (codegen), tantivy *or* fallback (search — name
     whichever M1 chose), JSON-on-disk (M2) / SQLite (M11) /
     Turso (M11) for storage, Vite + Preact + TypeScript (UI),
     Argon2id (passwords), CycloneDX + cargo-auditable + SLSA
     attestations (supply chain), GitHub Container Registry
     (artifact distribution).
   - For every row, the "why" must be one sentence and the
     "alternatives considered" must name at least one real
     alternative we rejected. **No vague answers.**

6. **Wasm artifact details** (lands M0 placeholder, M10 final)
   - **Per-component table** with columns: component name / role /
     wasm size (release, gzipped) / WIT exports / WIT imports /
     OCI image reference / SBOM link / SLSA attestation link.
   - Updated by a `make stats` target that fills the table from
     the actual built artifacts so it can't drift from reality.
   - Total app footprint summed at the bottom (target:
     < 25 MB across all components, gzipped).
   - Cold-start time for `wash dev` from clean state (target:
     < 10 s on a 2024-class laptop).

7. **Supply chain and attestations** (lands M0)
   - Step-by-step verification recipe a security reviewer can
     run unmodified: pull the OCI image, verify the GitHub
     attestation with `gh attestation verify`, extract the
     embedded auditable metadata with `cargo audit bin`,
     download the CycloneDX SBOM from the GitHub Release.
   - Show the actual command output (or a representative example).
   - Document what the attestation does and does not prove
     (proves: this artifact came from this commit via this
     workflow on these runners; does not prove: the code is
     correct, the dependencies are safe, the design is sound).

8. **WASI P3 caveats — what's real, what's not** (lands M0,
   refined as we hit issues)
   - This is the most important honesty section. wasmCloud's P3
     support is preview-quality and the line between "works" and
     "doesn't work yet" matters.
   - **Working today** (per the wasmCloud P3 blog post + our own
     observations): Rust HTTP P3 components, async handler
     signatures, `wit_bindgen::spawn` for subtasks, streaming
     response bodies in straightforward cases.
   - **Not working / experimental / fragile**: Threads (don't
     exist on wasmCloud P3 — period), TypeScript components
     (componentize-js works but rough), long-lived streams under
     load (issue #5028 in upstream), other WASI P3 interfaces
     beyond HTTP (blobstore, sockets) for which our code paths
     are based on internal fixtures rather than documented APIs.
   - **What we faked or skipped** (cumulative across milestones,
     never deleted): demo authentication uses static seeded
     accounts, no real OAuth/SSO, sessions don't survive host
     restart, PEP screening is approximated from CSL signals
     rather than being a true PEP feed, scheduled refresh is
     in-process not real cron, no HTTPS termination.
   - The phrase "this is a demo, not a product" appears
     literally somewhere in this section.

9. **Quick start (development)** (lands M0, refined each milestone)
   - Prereqs (Rust toolchain, `wasm32-wasip2` target, Node + pnpm
     for UI from M6, `wash` build from source for P3).
   - One command to bootstrap (`tools/build-wash.sh`).
   - One command to run (`make dev`).
   - One command to test (`make test`).
   - Common gotchas and their fixes.

10. **Configuration** (lands M2, expanded as flags appear)
    - Every env var the app reads, with default, type, example
      value, which component reads it, and which milestone
      introduced it.
    - `STORAGE_BACKEND`, `SESSION_SIGNING_KEY`, TLP thresholds,
      CSL refresh URL, brand config path, etc.
    - The brand swap recipe lives here, lifted from M9.

11. **Repository layout** (lands M0, kept current)
    - Tree from §1.5, with one-line annotation per directory.
    - Updated whenever a new top-level directory appears.

12. **Testing** (lands M0, expanded each milestone)
    - The three layers (Rust unit, API bash, Playwright UI).
    - How to run each individually.
    - How CI runs them (link to workflow file).
    - How to debug a test failure (where logs go, how to run
      `wash dev` against the same fixtures).

13. **Deployment** (lands M10)
    - `wash dev` → local development.
    - `wadm.yaml` → single-host deployment (default jsonfs
      backend).
    - `wadm.sqlite.yaml` / `wadm.turso.yaml` → backend swap
      examples (M11).
    - Pointer to the wasmCloud Kubernetes operator docs for
      production K8s deployment, with the explicit caveat that
      this demo has not been hardened for production.

14. **Roadmap and known issues**
    - Items pulled forward from §8 (out of scope) and §9 (open
      questions) of `PLAN.md`.
    - Known WASI P3 fragility points with links to upstream
      issues (e.g., wasmCloud #5028).

15. **Contributing**
    - Tiny section pointing at where the riskiest code lives
      (M1 search engine decision doc, P3 plumbing in
      `api-gateway`).
    - Issue + PR conventions.

16. **License + acknowledgments**
    - License (Apache-2.0 unless user specifies otherwise).
    - Acknowledgments: CNCF wasmCloud, ITA / trade.gov for the
      CSL data, tantivy (whether or not we end up using it),
      Turso/Limbo (M11).

### 11.2 The canonical ASCII diagram

The README's architecture section uses **this exact diagram** as its
spine, kept in sync as components evolve. It must use Unicode box
characters so it renders cleanly in GitHub:

```
┌───────────────────────────────────────────────────────────────────────────┐
│                              browser (SPA)                                │
│            login form · search bar · TLP dashboard · review queue         │
└─────────────────────────────────┬─────────────────────────────────────────┘
                                  │ HTTPS (terminated upstream)
                                  │ HttpOnly · Secure · SameSite=Strict
                                  ▼
                       ┌──────────────────────┐
                       │   wasmCloud host     │
                       │   (wash, P3 enabled) │
                       └──────────┬───────────┘
                                  │ wasi:http/handler@0.3.0-rc-2026-03-15
                                  ▼
                ┌───────────────────────────────────┐
                │            api-gateway            │
                │  routes · auth · rate-limit ·     │
                │  audit-id · TLP banding · CSP     │
                └─┬───────────┬──────────┬──────────┘
                  │           │          │
       ocelaudit: │  ocelaudit:│  ocelaudit:
       search/    │  storage/  │  csl/
       query      │  *         │  refresh
                  ▼           ▼          ▼
        ┌────────────────┐  ┌──────────────────────┐  ┌──────────────────┐
        │     search     │  │       storage        │  │    csl-ingest    │
        │ tantivy or     │  │  jsonfs (M2 default) │  │  fetch · cron ·  │
        │ fallback (M1)  │  │  sqlite / turso (M11)│  │  bulk-replace    │
        │ in-memory idx  │  │                      │  │  reindex trigger │
        └────────┬───────┘  └─────────┬────────────┘  └────────┬─────────┘
                 │                    │                        │
                 │  wasi:filesystem   │  wasi:filesystem       │  wasi:http
                 │  wasi:clocks       │  wasi:clocks           │  (outgoing)
                 ▼                    ▼                        ▼
        ┌─────────────────────────────────────────────────────────────────┐
        │  /data/  ── csl.json · audit.jsonl · users.json · workflow.jsonl │
        │            (or .db file when on sqlite/turso backends)           │
        └─────────────────────────────────────────────────────────────────┘

                       ┌──────────────────────┐       wasi:filesystem
                       │   static-assets      │ ◄─────  embedded SPA bundle,
                       │   /, /assets/*,      │         brand SVG,
                       │   /video/*           │         background video
                       └──────────────────────┘
```

A second smaller diagram showing the **trust boundary** (introduced
when M6 lands the SPA):

```
   ┌──────── browser (untrusted) ────────┐    ┌── host (trusted) ──┐
   │  SPA bundle (public, no secrets)    │    │  session signing   │
   │  cookie (HttpOnly, signed)          │ ─► │  key, audit log,   │
   │  /api/v1/* (strict CSP)             │    │  user table, CSL   │
   └─────────────────────────────────────┘    └────────────────────┘
                  ▲                                      ▲
                  │  no admin endpoints in bundle        │  no plaintext
                  │  no role list in bundle              │  passwords stored
                  │  no API keys in bundle               │  Argon2id only
```

### 11.3 Per-milestone README updates (do not skip)

Each milestone's exit criteria implicitly include "README updated".
Specifically:

| Milestone | README sections touched                                                      |
|-----------|------------------------------------------------------------------------------|
| M0        | All skeleton sections; supply chain + WASI P3 caveats with current state; ASCII diagram landed |
| M1        | Tech-choice table row for search; caveats updated with whichever engine won; link to `docs/m1-search-engine-decision.md` |
| M2        | Storage row in tech-choice table; configuration section gets `STORAGE_BACKEND` |
| M3        | Configuration gains CSL refresh env vars; caveats updated re: scheduled refresh being in-process not cron |
| M4        | API surface listed in README's "what's there"; `/api/openapi.json` linked |
| M5        | Workflow caveats added; PEP approximation honest disclosure                  |
| M6        | Trust boundary diagram added; SPA security posture section                   |
| M7        | Screenshots / asciinema added                                                |
| M8        | Audit/review/admin pages mentioned                                           |
| M9        | Brand swap recipe lifted into the configuration section                      |
| M10       | `make demo` instructions; cold-start time measured and recorded; total wasm footprint table filled in via `make stats` |
| M11       | Tech-choice table gains SQLite + Turso rows; storage backend swap recipe; backend comparison table from `docs/storage-backends.md` summarized |

### 11.4 Honesty rules

- **No marketing voice.** "Production-grade enterprise compliance
  platform" — no. "wasmCloud v2 demo for screening against the
  CSL" — yes.
- **Every claim is testable or refutable.** "Fast" without numbers
  doesn't appear. "p95 search latency 32 ms on the M1 fixture set"
  does.
- **Caveats stay forever.** Once M3 acknowledges the cron caveat,
  it doesn't get quietly removed in M10 because the demo polish
  feels embarrassed by it. The README is the truth log.
- **No fabricated benchmarks.** If `make stats` hasn't run, the
  numbers say "TODO (run `make stats`)" — never invent.
