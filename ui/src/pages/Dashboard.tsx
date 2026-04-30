import { useEffect, useState } from "preact/hooks";
import { api, type Me, type MetricsBody } from "../api";
import { navigate } from "../router";

interface Props {
  me: Me;
}

export function Dashboard({ me }: Props) {
  const [metrics, setMetrics] = useState<MetricsBody | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [q, setQ] = useState("");

  useEffect(() => {
    api
      .metrics()
      .then(setMetrics)
      .catch((e) => setError(String(e?.message ?? e)));
  }, []);

  function go(e: Event) {
    e.preventDefault();
    if (q.trim()) navigate(`/search?q=${encodeURIComponent(q.trim())}`);
  }

  return (
    <>
      <h1 class="mb-2 font-display text-2xl">
        Welcome, <span class="text-ocelot-accent">{me.username}</span>.
      </h1>
      <p class="mb-6 text-sm text-neutral-500">
        Search the U.S. Consolidated Screening List. Hits are TLP-banded and audit-logged.
      </p>

      <form
        onSubmit={go}
        class="mb-8 flex items-center gap-2 rounded-lg border border-neutral-200 bg-white p-3 dark:border-neutral-800 dark:bg-neutral-900"
      >
        <input
          type="text"
          placeholder="Search a name, alias, or address…"
          value={q}
          onInput={(e) => setQ((e.currentTarget as HTMLInputElement).value)}
          autocomplete="off"
          class="flex-1 rounded border border-neutral-300 bg-white px-3 py-2 text-sm outline-none focus:border-ocelot-accent dark:border-neutral-700 dark:bg-neutral-800"
        />
        <button
          type="submit"
          disabled={!q.trim()}
          class="rounded bg-ocelot-mark px-4 py-2 text-sm font-semibold text-white disabled:opacity-50 dark:bg-ocelot-paper dark:text-ocelot-ink"
        >
          Go
        </button>
      </form>

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
          <Tlp label="RED" count={metrics.tlp_histogram.red} color="red" />
          <Tlp label="YELLOW" count={metrics.tlp_histogram.yellow} color="yellow" />
          <Tlp label="GREEN" count={metrics.tlp_histogram.green} color="green" />
          <Card title="CSL sources" value={String(metrics.csl_sources.length)} />
        </div>
      )}
    </>
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

function Tlp({ label, count, color }: { label: string; count: number; color: "red" | "yellow" | "green" }) {
  const dot =
    color === "red" ? "bg-tlp-red" : color === "yellow" ? "bg-tlp-yellow" : "bg-tlp-green";
  const text =
    color === "red" ? "text-tlp-red" : color === "yellow" ? "text-tlp-yellow" : "text-tlp-green";
  return (
    <div class="rounded-lg border border-neutral-200 bg-white p-4 shadow-sm dark:border-neutral-800 dark:bg-neutral-900">
      <div class="flex items-center gap-2">
        <span class={`inline-block h-2 w-2 rounded-full ${dot}`} aria-hidden />
        <span class="text-xs uppercase tracking-wide text-neutral-500">{label}</span>
      </div>
      <div class={`mt-2 font-display text-2xl ${text}`}>{count}</div>
    </div>
  );
}
