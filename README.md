# OcelAudit

> A wasmCloud v2 demonstration that screens entities against the U.S. Consolidated Screening List (CSL).

[![CI](https://github.com/cosmonic/ocelaudit/actions/workflows/ci.yml/badge.svg)](https://github.com/cosmonic/ocelaudit/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/cosmonic/ocelaudit?include_prereleases)](https://github.com/cosmonic/ocelaudit/releases)
![License](https://img.shields.io/badge/license-Apache--2.0-blue)
![Status](https://img.shields.io/badge/status-demo--only-orange)

---

## What this is, and what it isn't

OcelAudit is a CNCF wasmCloud v2 demonstration that screens entities (people, organizations, vessels, aircraft) against the U.S. Consolidated Screening List published by the International Trade Administration. Backend is a set of Rust WebAssembly components glued together via WASI P2 interfaces; frontend is a static SPA served by the same wasmCloud host.

**This is a demo, not a product.** Specifically:
- No HTTPS termination — wasmCloud serves plain HTTP. Put a reverse proxy in front for any real deployment.
- Demo authentication only — two seeded users (`admin`, `compliance`) with Argon2id-hashed passwords. No OAuth, no SSO.
- Single-tenant. No org isolation, no multi-tenant data partitioning.
- No SLA on the CSL data feed. The trade.gov endpoint changes paths historically.
- Sessions don't survive a host restart.
- "PEP screening" is approximated from CSL signals. Not a true PEP feed.

Who this is for:
- Developers evaluating wasmCloud v2 for production-shaped workloads.
- Compliance teams curious about WASI-based component architectures.
- Conference demos that need a non-trivial application story.

---

## 30-second demo

```sh
make demo
```

Banner shows the URL (`http://127.0.0.1:8000/`) and two random
passwords (admin + compliance). The browser opens automatically.
See [`docs/demo-script.md`](docs/demo-script.md) for the 90-second
walkthrough that hits every TLP outcome and the full review flow.

Cold-start budget: < 5 minutes from clean clone (per PLAN.md M10).

---

## Architecture overview

> **TODO (M1+):** component-by-component descriptions land as each component lands. Today only `api-gateway` exists, and it's a hello-world.

The end-state architecture (canonical ASCII diagram, kept in sync as components land):

```
┌───────────────────────────────────────────────────────────────────────────┐
│                              browser (SPA)                                │
│            login form · search bar · TLP dashboard · review queue         │
└─────────────────────────────────┬─────────────────────────────────────────┘
                                  │ HTTP (TLS terminated upstream)
                                  │ HttpOnly · Secure · SameSite=Strict
                                  ▼
                       ┌──────────────────────┐
                       │   wasmCloud host     │
                       │   (wash 2.0.4)       │
                       └──────────┬───────────┘
                                  │ wasi:http/incoming-handler@0.2.2
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

Trust-boundary diagram lands in M6 when the SPA appears.

---

## Technology choices and rationale

> **TODO (M1+):** rows fill in as the relevant components land.

| Layer        | Choice            | Why                                                      | Alternatives considered            |
|--------------|-------------------|----------------------------------------------------------|------------------------------------|
| Host         | wasmCloud v2      | Mandate of the demo; latest released distribution.       | wasmtime-cli, wasmEdge.            |
| Interface    | WASI P2 (`0.2.2`) | Released wash 2.0.4 supports the runtime today (see WASI P3 caveats). | WASI P3 — see caveats. |
| Component    | Rust              | Mature wit-bindgen support, smallest .wasm artefacts.    | TypeScript (`componentize-js`).    |
| Codegen      | wit-bindgen 0.54  | Matches upstream wasmCloud fixtures Cargo.lock pin.      | older 0.42-0.50 series.            |
| WIT fetch    | wkg 0.15.0        | wash 2.0.4's bundled wkg mis-decodes text-WIT overrides. | wash's bundled wkg (broken).       |
| Search       | Hand-rolled (BM25 + Jaro-Winkler + trigrams) | tantivy fails to compile to wasm32-wasip2 (zstd-sys C dep can't target the triple). Fallback gets 100% top-10 recall on the 10k-record fixture at p95 0.60 ms. See `docs/m1-search-engine-decision.md`. | tantivy default, tantivy single-thread (both blocked on C toolchain). |
| Storage      | JSON-on-disk (default) + in-memory (M11 alt) | `pub trait Storage`. Two impls, swap via `STORAGE_BACKEND` env. SQLite + Turso documented as wasi-sdk future work in `docs/storage-backends.md`. | rusqlite-bundled, turso (both block on the same wasi-sdk dance), KV via host bindings. |
| UI           | TBD M6            | Vite + Preact + TS planned.                              | React, SolidJS.                    |
| Passwords    | Argon2id          | Standard.                                                | bcrypt, scrypt.                    |
| Supply chain | cargo-auditable + CycloneDX + SLSA attestations | wasmCloud-recommended chain. | unsigned, sigstore-only.           |
| Distribution | GHCR              | Free, attached to repo, tooling is `gh`.                 | Docker Hub.                        |

---

## Wasm artifact details

> **TODO (M10):** populated by `make stats` from real built artefacts. No fabricated numbers.

| Component   | Role                          | Wasm size           | Image ref                                          |
|-------------|-------------------------------|---------------------|----------------------------------------------------|
| api-gateway | HTTP entry, routes, auth, TLP | 572 KB (release, M4; storage + search + csl-ingest + auth all compiled in) | `ghcr.io/<owner>/ocelaudit-api-gateway:<tag>` |
| search      | BM25 + JW search engine       | host-target only at M1; .wasm landed when wired in M3+ | `ghcr.io/<owner>/ocelaudit-search:<tag>` |
| storage-jsonfs | JSON-on-disk persistence   | compiled into api-gateway in M2 (no separate .wasm yet); separate component lands when WIT plumbing splits | `ghcr.io/<owner>/ocelaudit-storage-jsonfs:<tag>` |
| csl-ingest  | Parse ITA CSL JSON → CslEntry | compiled into api-gateway in M3; HTTP fetch deferred (see caveats) | `ghcr.io/<owner>/ocelaudit-csl-ingest:<tag>` |

(Other rows land with their components.)

---

## Supply chain and attestations

Every component is built via `cargo auditable build` (configured at the wash layer), so each `.wasm` carries embedded dependency metadata. Release artifacts get SLSA build provenance via `wash-oci-publish`'s `attestation: "true"` flag, and a CycloneDX SBOM is attached to each GitHub Release.

A security reviewer can verify a release artifact end-to-end without trusting the README:

```sh
# 1. Pull the artifact
wash oci pull ghcr.io/<owner>/ocelaudit-api-gateway:v0.1.0 \
  --component-path ./api-gateway.wasm

# 2. Verify the SLSA build provenance
gh attestation verify oci://ghcr.io/<owner>/ocelaudit-api-gateway:v0.1.0 \
  --owner <owner>

# 3. Read the embedded auditable metadata for the dependency tree
cargo audit bin ./api-gateway.wasm

# 4. Download the CycloneDX SBOM attached to the release
gh release download v0.1.0 -p 'sbom-*.cdx.json'
```

What the attestation **does** prove: this `.wasm` came from the commit named in the attestation, built on a GitHub-hosted runner via the workflow named in the attestation. What it **does not** prove: the code is correct, the dependencies are safe, the design is sound.

---

## WASI P3 caveats — what's real, what's not

> The most important honesty section. The line between "works" and "doesn't work yet" matters.

**Working today (verified 2026-04-30 with `wash` 2.0.4):**
- WASI P2 (`wasi:http@0.2.2`) Rust HTTP components.
- Synchronous handler signature (`fn handle(req, out: ResponseOutparam)`).
- Blocking writes via `OutputStream::blocking_write_and_flush`.

**Not working / experimental / fragile:**
- **WASI P3 components.** wash 2.0.4 exposes `dev.wasip3: true` in its config schema, and `wash dev` accepts it without complaint. But its bundled wasmtime engine doesn't have the component-model async feature compiled in, so loading a P3 component fails:
  > `failed to parse WebAssembly module — \`stream\` requires the component model async feature (at offset 0xc)`

  We tried to use the P3 path early in M0; it doesn't work today. The wasmCloud P3 blog post says to build wash from source with `--features wasip3` — that path remains available via `tools/build-wash.sh` if a future capability requires it, but it's dormant by default. **OcelAudit is built on WASI P2 against the released wash 2.0.4 binary.** When a wash release ships with the P3 runtime feature on, this section will be revisited.
- **Threads.** Not available in the wasmCloud runtime, regardless of P2/P3. Single-threaded async only. No Rayon, no `std::thread::spawn`.
- **TypeScript components via componentize-js.** The blog notes this works "but is rougher." We don't use it — Rust everywhere.
- **wash 2.0.4's bundled `wkg`.** Mis-decodes text-WIT path overrides (treats `.wit` text files as binary component packages, fails on the leading byte). Workaround: install standalone `wkg` 0.15.0 (`cargo install wkg`) and use `wkg wit fetch -t wit` before `wash build --skip-fetch`. The Makefile chains both.
- **tantivy on wasm32-wasip2.** Doesn't compile — `zstd-sys` (a transitive C dependency) can't target the wasi-p2 triple under the system clang. Solving it requires a wasi-sdk dance the demo isn't worth. We fell back to a hand-rolled BM25+JW engine in M1 — see `docs/m1-search-engine-decision.md`.

**What we faked or skipped (cumulative across milestones, never deleted):**
- Demo authentication uses two static seeded accounts. No real OAuth/SSO.
- Sessions don't survive a host restart.
- **"PEP screening" is approximated from CSL signals — not a true PEP feed.** `/api/v1/screen/pep` filters to `PLC` (Palestinian Legislative Council) plus other CSL records of publicly-listed officials. The response body always carries a DISCLAIMER note. Use a dedicated PEP database for real compliance.
- **CSL refresh (M3) reads from a file path under the wash dev volume mount, not a live trade.gov HTTP fetch.** Drop your `consolidated.json` at `/data/csl/seed.json` (host: `.cache/ocelaudit-data/csl/seed.json`) and POST `/api/v1/csl/refresh`. Real `wasi:http/outgoing-handler` fetch lands later. Reason: implementing it pulls in the `wstd` async-runtime dep, which we deferred to keep M3 focused on the parse/store path.
- No in-process scheduled refresh. WASI P2 components are request/response — they don't run loops between calls. Use an external scheduler (cron, systemd timer, k8s CronJob) that hits `/api/v1/csl/refresh`.
- **Each WASI P2 incoming-handler call is a fresh component instance.** That means in-process state (signing key, cached search index, anything in `OnceCell`) doesn't survive between requests; it has to be persisted to disk or rebuilt each call. We persist the session signing key to `/data/session.key`; the search index is rebuilt per query (acceptable on the 10k-record fixture; M5 will look at amortizing it).
- No HTTPS termination. Plain HTTP only.

**This is a demo, not a product.**

---

## Quick start (development)

Prereqs (macOS / Linux):

```sh
rustup target add wasm32-wasip2
brew install gh jq                            # or your distro equivalent
cargo install wkg                             # standalone wkg 0.15+; wash's bundled one mis-decodes text-WIT
cargo install cargo-auditable cargo-audit     # supply chain: embed + analyse dep metadata
# wash 2.0.5 from your package manager or https://wasmcloud.com/docs/installation
# (v2.0.4 also works locally where it's installed, but lacks a published Linux
# binary — CI must pin to v2.0.5; see tools/wash-version.txt)
```

Then:

```sh
git clone https://github.com/cosmonic/ocelaudit.git
cd ocelaudit
make build      # wkg fetch + wash build, per component
make test       # all three test layers (rust + api + ui — ui skipped pre-M6)
make dev        # boots `wash dev` for components/api-gateway
```

Common gotchas:
- "build.command is required in wash config" — you cd'd into the wrong directory. `wash build` reads `.wash/config.yaml` from CWD; run it from a component crate directory.
- "failed to decode content of dependency" during wash build — that's wash 2.0.4's bundled wkg. The Makefile uses standalone `wkg` to fetch deps first; if you're calling wash directly, run `wkg wit fetch -t wit` first then `wash build --skip-fetch`.
- "stream requires the component model async feature" — you're trying to run a P3 component on wash 2.0.4. We're on P2 — see "WASI P3 caveats" above.

---

## Replacing the brand

OcelAudit's mark, wordmark, login video, and theme colors are runtime-configurable. No rebuild required.

1. Drop your assets into the volume's static dir (host: `.cache/ocelaudit-data/static/`, guest: `/data/static/`):
   ```sh
   cp my-logo.svg .cache/ocelaudit-data/static/brand/my-logo.svg
   cp my-video.mp4 .cache/ocelaudit-data/static/video/my-video.mp4
   ```
2. Write `.cache/ocelaudit-data/static/ocelaudit.config.json`:
   ```json
   {
     "logo_url": "/brand/my-logo.svg",
     "wordmark": "AcmeScreen",
     "video_url": "/video/my-video.mp4",
     "primary_color": "#0f172a",
     "accent_color": "#dc2626"
   }
   ```
3. Reload the SPA — it reads `/api/v1/branding` on boot.
4. To revert, delete the config file. Defaults take over.
5. (For permanent / committed branding: stage `ui/public/brand/` and `ui/public/video/` in your fork, then rebuild the SPA.)

Missing keys in the config fall back to defaults (so a partial override is fine).

---

## Configuration

> **TODO (M2+):** every env var lands here as it's introduced.

| Var                    | Default                    | Type         | Component       | Introduced | Purpose                                      |
|------------------------|----------------------------|--------------|-----------------|------------|----------------------------------------------|
| `DEV_HOST_ADDR`        | `127.0.0.1:8000`           | `host:port`  | (Makefile only) | M0         | Where `wash dev` listens for tests.          |
| `STORAGE_BACKEND`      | `jsonfs:/data` (M2)        | `jsonfs:<path>` / `sqlite:<file>` (M11) / `turso:<file>` (M11) | api-gateway | M2 | Selects storage backend. M2: jsonfs only; sqlite/turso fail-fast with a pointer to M11. |
| `SESSION_SIGNING_KEY`  | reads or writes `/data/session.key` if unset | UTF-8 secret | api-gateway | M4 | Signs session cookies. WASI P2 components are re-instantiated per request, so the signing key has to live on disk to survive between requests; we generate it once and write it under the storage root. Set this env to a stable value to override. |
| `TLP_RED_THRESHOLD`    | `0.95`                     | float        | search          | M1         | Hits ≥ this score are RED.                   |
| `TLP_YELLOW_THRESHOLD` | `0.75`                     | float        | search          | M1         | Hits ≥ this and < red are YELLOW.            |
| `CSL_SEED_PATH` (de-facto) | `/data/csl/seed.json` (hardcoded in M3) | path | api-gateway | M3 | Where `/api/v1/csl/refresh` reads from. Configurable via env in a later milestone alongside `CSL_REFRESH_URL` for the live HTTP fetch path. |

---

## Repository layout

```
ocelaudit/
├── CLAUDE.md                   orientation for Claude Code sessions
├── PLAN.md                     canonical build plan (do not deviate)
├── README.md                   this file
├── Makefile                    single entry point: build, test, dev, sbom
├── Cargo.toml                  Rust workspace
├── rust-toolchain.toml         pins stable + wasm32-wasip2
├── wadm.yaml                   default deployment (lands M2)
├── .github/workflows/          ci.yml + release.yml
├── tools/                      build-wash.sh (escape hatch), wash-version.txt
├── wit/deps/                   vendored WASI P2 deps (unversioned dir names)
├── interfaces/ocelaudit/       our own WIT packages (search, storage, csl, assets)
├── components/                 one Rust crate per component
│   └── api-gateway/            M0 hello-world; routes land in M4
├── ui/                         Vite + Preact + TS SPA (lands M6)
└── tests/{api,components,fixtures,ui}/
```

---

## Testing

Three layers, all driven by `make test`. Same targets locally and in CI; nothing is CI-only.

| Layer            | Target          | What runs                                                                                       |
|------------------|-----------------|-------------------------------------------------------------------------------------------------|
| Rust unit        | `make test-rust`| `cargo check --workspace --target wasm32-wasip2`; real `cargo test` lands as logic appears (M1+). |
| API integration  | `make test-api` | Boots `wash dev` once via `tests/api/_runner.sh`, runs every `tests/api/m*.sh` script, tears down on exit. |
| UI smoke         | `make test-ui`  | Playwright (lands M6). Skipped cleanly until then.                                              |

Single-test invocation:

```sh
make test-one TEST=tests/api/m0-hello.sh
make test-one TEST=search::tokenizer::lowercase   # M1+
```

CI runs `make build`, `make test-rust`, `make test-api`, `make test-ui`, `make audit`, `make sbom` in `.github/workflows/ci.yml`. The release workflow re-runs the full CI suite as a gate before publishing artifacts.

---

## Deployment

> **TODO (M10):** real deployment recipe lands with `make demo`.

Local development uses `wash dev` against a single component. Single-host wadm-driven deployment (`wadm.yaml`) lands in M2 alongside the storage-jsonfs binding. Backend swap examples (`wadm.sqlite.yaml`, `wadm.turso.yaml`) land in M11.

Production K8s deployment is out of scope for the demo. See [the wasmCloud Kubernetes operator docs](https://wasmcloud.com/docs/kubernetes-operator/) for that path; this codebase has not been hardened for it.

---

## Roadmap and known issues

- M0 ✅ — Bootstrap + CI; api-gateway hello-world; release.yml wired.
- M1 ✅ — Hand-rolled search engine (tantivy ruled out on wasi-toolchain grounds); 10k-record fixture suite with 100% top-1 / top-10 / TLP and p95 0.60 ms; decision frozen at `docs/m1-search-engine-decision.md`.
- M2 ✅ — `storage-jsonfs` over `wasi:filesystem`: csl-records, audit, users (Argon2id-seeded), workflow. 17 unit tests + 8 API assertions; api-gateway exposes `/healthz`, `/api/v1/me`, `/api/v1/audit/_test`.
- M3 ✅ — `csl-ingest` parser + 9-source-list fixture; `/api/v1/csl/{metadata,refresh,sources,entries/{id}}`. Refresh reads `/data/csl/seed.json` from the volume mount (real HTTP fetch is in caveats below).
- M4 ✅ — Cookie-session auth (HMAC-SHA256, key persisted to `/data/session.key`), `/api/v1/{auth/{login,logout},me,search,search/autocomplete,audit,audit/{id},metrics}`. UUIDv7 audit IDs. 6 unit tests + 53 API assertions across the M0+M2+M3+M4 suite.
- M5 ✅ — `/screen/{ofac,pep}` with source-list scoping + scope-note in body; per-hit `citation` (source_meta agency_url) on `/search` + `/screen` responses; `/review/{audit_id}/decide` writes `WorkflowEntry` so `/audit/{id}` reflects the latest decision and full history. Total: 72 API assertions.
- M6 ✅ — Vite + Preact + TS SPA under `ui/` (10 KB CSS + 20 KB JS). Login + Dashboard pages talk to the real backend via the HttpOnly session cookie. Gateway serves `/`, `/assets/*`, `/brand/*` from `/data/static/` with strict CSP and SPA fallback for client-side routes. Total: 83 API assertions.
- M7 ✅ — Search page (form + filters + TLP-banded result cards + agency citations + 150ms debounced autocomplete), dashboard search bar; tiny URL-driven router (no tanstack/wouter dep). Bundle stays under 32 KB JS gzipped to ~10 KB.
- M8 ✅ — Audit (paginated list + click-through to detail with full decision history), Review (queue with cleared/blocked decision UI + required note), Admin (admin-only: "Update CSL now" button + threshold display). 5 pages now. Bundle: 40 KB JS, 14 KB CSS, gzipped 16 KB total.
- M9 ✅ — `/api/v1/branding` endpoint reads `/data/static/ocelaudit.config.json` (logo, wordmark, video, colors); missing keys fall back to defaults. SPA loads it on boot, applies CSS custom properties, plays the optional login video. 10 new API assertions; brand swap recipe in README below.
- M10 ✅ — `make demo` (cold-start bootstrap, prints URL + creds, opens browser); `make stats` (per-component wasm size table from real artefacts); `docs/demo-script.md` (90-second walkthrough hitting every TLP outcome).
- M11 ✅ — Extracted `pub trait Storage` (16 methods, object-safe). `JsonFsStorage` (M2) + `MemoryStorage` (new, ephemeral) both implement it. Gateway holds `Box<dyn Storage>` and dispatches on `STORAGE_BACKEND` env (`jsonfs:<dir>` / `memory:`). SQLite + Turso documented as future work in `docs/storage-backends.md` — both blocked on wasi-sdk, with a complete walkthrough of how to unblock them.

Known issues (will not be quietly removed once acknowledged):
- WASI P3 not usable on wash 2.0.4 — see "WASI P3 caveats". Tracked upstream in [wasmCloud#5028](https://github.com/wasmCloud/wasmCloud/issues/5028).
- wash 2.0.4's bundled `wkg` mis-decodes text-WIT overrides (see "Quick start" gotchas).

---

## Contributing

The riskiest code lives at:
- `components/search/` (M1 search engine decision; see `docs/m1-search-engine-decision.md` once landed).
- `components/api-gateway/src/lib.rs` (P2 plumbing; bindgen surface area).

Issues and PRs welcome. Conventional Commit prefixes (`feat:`, `fix:`, `chore:`, `docs:`) keep the auto-generated release notes readable.

---

## License + acknowledgments

Apache-2.0. See `LICENSE` (lands in M10 polish).

Acknowledgments:
- CNCF wasmCloud — the host this is built on.
- ITA / trade.gov — publishers of the CSL data feed.
- tantivy / Turso (Limbo) — possible engines whose decision lives in M1 / M11.
