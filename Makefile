# OcelAudit — single entry point for build, test, and dev.
# Same targets run locally and in CI; nothing is CI-only.

.PHONY: help build test test-rust test-api test-ui test-ui-headed test-watch \
        test-one dev clean fmt lint audit sbom demo stats stop-dev

SHELL := /usr/bin/env bash
.SHELLFLAGS := -eu -o pipefail -c

# Components in build order (hello-world api-gateway only at M0; expand
# as later milestones land their crates).
COMPONENTS := api-gateway

# Where wash dev listens by default. Override if you change the wadm
# manifest's HTTP server bindings.
DEV_HOST_ADDR ?= 127.0.0.1:8000
DEV_PID_FILE := .cache/wash-dev.pid
DEV_LOG_FILE := .cache/wash-dev.log

help:
	@awk 'BEGIN {FS = ":.*?## "} /^[a-zA-Z0-9_-]+:.*?## / {printf "  %-18s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

# ----- build -----------------------------------------------------------

build: ## Build every component to wasm (wkg fetch + wash build)
	@for c in $(COMPONENTS); do \
	  echo ">> wkg wit fetch -t wit components/$$c"; \
	  (cd components/$$c && rm -f wkg.lock && wkg wit fetch -t wit); \
	  echo ">> wash build --skip-fetch components/$$c"; \
	  (cd components/$$c && wash build --skip-fetch); \
	done

# ----- test ------------------------------------------------------------

test: test-rust test-api test-ui ## Run all three test layers (sequential, fail-fast)

test-rust: ## cargo check + test the workspace
	cargo check --workspace --target wasm32-wasip2
	@# Host-target unit tests for component crates require restructuring
	@# the bindgen module behind cfg(target_arch="wasm32"); that lands
	@# alongside the first crate that has unit-testable logic (M1).
	@# Until then this target is a wasm compile gate.
	@if cargo metadata --no-deps --format-version 1 \
	  | grep -q '"crate_types":\["lib"\]\|"crate_types":\["rlib"\]'; then \
	  cargo test --workspace --all-targets; \
	fi

test-api: build ## Boot wash dev, run bash+curl scripts under tests/api/
	@bash tests/api/_runner.sh

test-ui: ## Playwright smoke tests (lands in M6)
	@if [ -d tests/ui ] && [ -f tests/ui/package.json ]; then \
	  (cd tests/ui && pnpm install --frozen-lockfile && pnpm exec playwright test); \
	else \
	  echo "  (no Playwright suite yet — landing in M6)"; \
	fi

test-ui-headed: ## Playwright with browser visible
	@if [ -d tests/ui ] && [ -f tests/ui/package.json ]; then \
	  (cd tests/ui && pnpm exec playwright test --headed); \
	else \
	  echo "  (no Playwright suite yet — landing in M6)"; \
	fi

test-watch: ## cargo-watch for inner-loop dev
	cargo watch -x 'test --workspace --all-targets'

test-one: ## Run a single test by name. Usage: make test-one TEST=path/to/script.sh OR TEST=mod::name
	@if [ -z "$(TEST)" ]; then echo "Usage: make test-one TEST=..."; exit 1; fi
	@case "$(TEST)" in \
	  tests/api/*.sh) bash "$(TEST)" ;; \
	  tests/ui/*) (cd tests/ui && pnpm exec playwright test "$(TEST)") ;; \
	  *) cargo test --workspace --all-targets -- "$(TEST)" ;; \
	esac

# ----- dev -------------------------------------------------------------

dev: build ## Start wash dev for manual exploration
	wash dev

stop-dev: ## Stop a wash dev instance launched by tests/api/_runner.sh
	@if [ -f $(DEV_PID_FILE) ]; then \
	  pid=$$(cat $(DEV_PID_FILE)); \
	  if kill -0 $$pid 2>/dev/null; then \
	    echo "stopping wash dev (pid $$pid)"; \
	    kill $$pid 2>/dev/null || true; \
	    sleep 1; \
	    kill -9 $$pid 2>/dev/null || true; \
	  fi; \
	  rm -f $(DEV_PID_FILE); \
	fi

# ----- supply chain ----------------------------------------------------

audit: build ## cargo audit on every built .wasm
	@for c in $(COMPONENTS); do \
	  artefact=$$(ls components/$$c/build/*.wasm 2>/dev/null | head -1); \
	  if [ -n "$$artefact" ]; then \
	    echo ">> cargo audit bin $$artefact"; \
	    cargo audit bin "$$artefact"; \
	  fi; \
	done

sbom: ## Generate CycloneDX SBOMs and merge into one document
	@mkdir -p .cache/sbom
	@for c in $(COMPONENTS); do \
	  echo ">> cyclonedx-rs components/$$c"; \
	  (cd components/$$c && cargo cyclonedx --format json --output-pattern bom 2>/dev/null || true); \
	done
	@echo "SBOMs under components/*/bom.cdx.json (merge step lands in CI release flow)"

# ----- ergonomics ------------------------------------------------------

fmt: ## cargo fmt + prettier (UI lands M6)
	cargo fmt --all

lint: ## clippy
	cargo clippy --workspace --all-targets -- -D warnings

clean: stop-dev ## Remove build artifacts and caches
	cargo clean
	rm -rf .cache
	@for c in $(COMPONENTS); do rm -rf components/$$c/build; done

stats: ## Per-component wasm size table (TODO M10)
	@echo "TODO (M10): per-component wasm size table; populated by make stats"

demo: ## Cold-start demo bootstrap (TODO M10)
	@echo "TODO (M10): cold-clone -> working login in <5min"
