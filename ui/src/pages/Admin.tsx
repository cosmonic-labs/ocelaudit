import { useEffect, useState } from "preact/hooks";
import { api, type Me, type MetricsBody } from "../api";

interface Props {
  me: Me;
}

export function AdminPage({ me }: Props) {
  if (me.role !== "admin") {
    return (
      <div class="rounded-lg border border-tlp-red/40 bg-tlp-red/10 p-6 text-tlp-red">
        Admin access required. Sign in as the <code>admin</code> user to view this page.
      </div>
    );
  }
  return <AdminBody />;
}

interface RefreshResult {
  ingested: number;
  version: string;
  source: string;
  warning?: string;
  index_built_ms?: number | null;
  index_error?: string;
}

function AdminBody() {
  const [metrics, setMetrics] = useState<MetricsBody | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [refreshResult, setRefreshResult] = useState<RefreshResult | null>(null);

  async function reload() {
    try {
      const m = await api.metrics();
      setMetrics(m);
    } catch (e) {
      setError(String((e as Error).message ?? e));
    }
  }

  useEffect(() => {
    void reload();
  }, []);

  async function refresh() {
    setBusy(true);
    setError(null);
    setRefreshResult(null);
    try {
      const r = await api.cslRefresh();
      setRefreshResult({
        ingested: r.ingested,
        version: r.version,
        source: r.source,
        warning: r.warning,
        index_built_ms: r.index_built_ms,
        index_error: r.index_error,
      });
      await reload();
    } catch (e) {
      setError(String((e as Error).message ?? e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div class="space-y-8">
      <header>
        <h1 class="font-display text-2xl">Admin</h1>
        <p class="mt-1 text-sm text-neutral-500">CSL data + scoring thresholds.</p>
      </header>

      {error && (
        <p class="rounded border border-tlp-red/40 bg-tlp-red/10 px-3 py-2 text-sm text-tlp-red">{error}</p>
      )}

      <section class="rounded-lg border border-neutral-200 bg-white p-6 dark:border-neutral-800 dark:bg-neutral-900">
        <h2 class="font-display text-lg">CSL data</h2>
        {!metrics ? (
          <p class="mt-2 text-sm text-neutral-500">loading…</p>
        ) : (
          <dl class="mt-3 grid gap-3 sm:grid-cols-2">
            <Field label="records" value={String(metrics.csl_count)} />
            <Field
              label="last refresh"
              value={
                metrics.last_csl_refresh > 0
                  ? new Date(metrics.last_csl_refresh * 1000).toISOString().slice(0, 19).replace("T", " ")
                  : "never"
              }
            />
            <Field label="distinct sources" value={String(metrics.csl_sources.length)} />
            <Field label="queue depth" value={String(metrics.queue_depth)} />
          </dl>
        )}
        <div class="mt-4 flex items-center gap-3">
          <button
            type="button"
            disabled={busy}
            onClick={refresh}
            class="inline-flex items-center gap-2 rounded bg-ocelot-mark px-4 py-2 text-sm font-semibold text-white disabled:opacity-50 dark:bg-ocelot-paper dark:text-ocelot-ink"
          >
            {busy && (
              <span
                class="inline-block h-3 w-3 animate-spin rounded-full border-2 border-white/40 border-t-white"
                aria-hidden
              />
            )}
            {busy ? "fetching + rebuilding index…" : "Update CSL now"}
          </button>
          <p class="text-xs text-neutral-500">
            Tries a live HTTPS pull from <code>data.trade.gov</code>; falls back to
            <code>/data/csl/seed.json</code> on any failure.
          </p>
        </div>
        {refreshResult && (
          <div class="mt-3 space-y-2">
            <p
              class={`rounded px-3 py-2 text-sm ${
                refreshResult.source === "trade.gov"
                  ? "bg-tlp-green/10 text-tlp-green"
                  : "bg-tlp-yellow/10 text-tlp-yellow"
              }`}
            >
              {refreshResult.source === "trade.gov" ? "✓" : "⚠"} ingested{" "}
              <strong>{refreshResult.ingested.toLocaleString()}</strong> records from{" "}
              <code>{refreshResult.source}</code> (version {refreshResult.version}).
              {typeof refreshResult.index_built_ms === "number" && (
                <>
                  {" "}
                  Search index rebuilt in{" "}
                  <strong>{refreshResult.index_built_ms.toLocaleString()} ms</strong> — first
                  /search will land warm.
                </>
              )}
            </p>
            {refreshResult.warning && (
              <p class="rounded bg-neutral-200/60 px-3 py-2 text-xs text-neutral-700 dark:bg-neutral-800 dark:text-neutral-300">
                <strong>fallback note:</strong> {refreshResult.warning}
              </p>
            )}
            {refreshResult.index_error && (
              <p class="rounded bg-tlp-red/10 px-3 py-2 text-xs text-tlp-red">
                <strong>index rebuild error:</strong> {refreshResult.index_error}
              </p>
            )}
          </div>
        )}
      </section>

      <section class="rounded-lg border border-neutral-200 bg-white p-6 dark:border-neutral-800 dark:bg-neutral-900">
        <h2 class="font-display text-lg">TLP thresholds</h2>
        <p class="mt-1 text-sm text-neutral-500">
          Read-only for now. Persistent threshold editing lands when search is split into its own component.
        </p>
        <dl class="mt-3 grid gap-3 sm:grid-cols-2">
          <Field label="red" value={<code>≥ 0.95</code>} />
          <Field label="yellow" value={<code>0.75 – 0.95</code>} />
        </dl>
      </section>
    </div>
  );
}

function Field({ label, value }: { label: string; value: preact.ComponentChildren }) {
  return (
    <div>
      <dt class="text-xs uppercase tracking-wide text-neutral-500">{label}</dt>
      <dd class="mt-1 text-sm">{value}</dd>
    </div>
  );
}
