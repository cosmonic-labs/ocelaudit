import { useEffect, useState } from "preact/hooks";
import { api, type Me, type MetricsBody } from "../api";

interface Props {
  me: Me;
  onLogout: () => void;
}

export function Dashboard({ me, onLogout }: Props) {
  const [metrics, setMetrics] = useState<MetricsBody | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .metrics()
      .then(setMetrics)
      .catch((e) => setError(String(e?.message ?? e)));
  }, []);

  return (
    <div class="min-h-full">
      <header class="border-b border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-900">
        <div class="mx-auto flex max-w-5xl items-center justify-between px-4 py-3">
          <div class="flex items-center gap-2">
            <img src="/brand/ocelot.svg" alt="" class="h-8 w-8 text-ocelot-mark dark:text-ocelot-paper" />
            <span class="font-display text-lg">OcelAudit</span>
          </div>
          <div class="flex items-center gap-3 text-sm">
            <span class="text-neutral-500">
              <code class="rounded bg-neutral-100 px-1 dark:bg-neutral-800">{me.username}</code>{" "}
              · {me.role}
            </span>
            <button
              onClick={onLogout}
              class="rounded border border-neutral-300 px-2 py-1 text-xs hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
            >
              Sign out
            </button>
          </div>
        </div>
      </header>

      <main class="mx-auto max-w-5xl px-4 py-8">
        <h1 class="mb-4 font-display text-2xl">Dashboard</h1>
        <p class="mb-8 text-sm text-neutral-500">
          Search and review pages land in M7 + M8. Today the dashboard
          shows the live API state.
        </p>

        {error && (
          <p class="mb-6 rounded border border-tlp-red/40 bg-tlp-red/10 px-3 py-2 text-sm text-tlp-red">
            {error}
          </p>
        )}

        {!metrics ? (
          <p class="text-sm text-neutral-500">loading metrics…</p>
        ) : (
          <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
            <Card title="CSL records" value={String(metrics.csl_count)} />
            <Card
              title="Last refresh"
              value={
                metrics.last_csl_refresh > 0
                  ? new Date(metrics.last_csl_refresh * 1000).toISOString().slice(0, 19).replace("T", " ")
                  : "never"
              }
            />
            <Card title="Recent queries" value={String(metrics.queries_recent)} />
            <Card title="Pending review" value={String(metrics.queue_depth)} />
            <Tlp label="RED" count={metrics.tlp_histogram.red} color="text-tlp-red" />
            <Tlp label="YELLOW" count={metrics.tlp_histogram.yellow} color="text-tlp-yellow" />
            <Tlp label="GREEN" count={metrics.tlp_histogram.green} color="text-tlp-green" />
            <Card title="CSL sources" value={String(metrics.csl_sources.length)} />
          </div>
        )}
      </main>
    </div>
  );
}

function Card({ title, value }: { title: string; value: string }) {
  return (
    <div class="rounded-lg border border-neutral-200 bg-white p-4 shadow-sm dark:border-neutral-800 dark:bg-neutral-900">
      <div class="text-xs uppercase tracking-wide text-neutral-500">{title}</div>
      <div class="mt-2 font-display text-2xl">{value}</div>
    </div>
  );
}

function Tlp({ label, count, color }: { label: string; count: number; color: string }) {
  return (
    <div class="rounded-lg border border-neutral-200 bg-white p-4 shadow-sm dark:border-neutral-800 dark:bg-neutral-900">
      <div class="flex items-center gap-2">
        <span class={`inline-block h-2 w-2 rounded-full ${color.replace("text-", "bg-")}`} aria-hidden />
        <span class="text-xs uppercase tracking-wide text-neutral-500">{label}</span>
      </div>
      <div class={`mt-2 font-display text-2xl ${color}`}>{count}</div>
    </div>
  );
}
