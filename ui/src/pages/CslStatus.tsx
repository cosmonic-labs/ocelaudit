import { useEffect, useState } from "preact/hooks";
import { api, type CslStats } from "../api";
import { Tag } from "../components/Tag";
import { navigate } from "../router";

export function CslStatusPage() {
  const [stats, setStats] = useState<CslStats | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .cslStats()
      .then(setStats)
      .catch((e) => setError(String((e as Error).message ?? e)));
  }, []);

  return (
    <div>
      <header class="mb-6 flex items-baseline gap-4">
        <h1 class="font-display text-2xl">CSL database</h1>
        <a
          href="/"
          onClick={(e) => {
            e.preventDefault();
            navigate("/");
          }}
          class="text-sm text-ocelot-accent hover:underline"
        >
          ← Dashboard
        </a>
      </header>

      {error && (
        <p class="mb-4 rounded border border-tlp-red/40 bg-tlp-red/10 px-3 py-2 text-sm text-tlp-red">
          {error}
        </p>
      )}

      {!stats ? (
        <p class="text-sm text-neutral-500">loading…</p>
      ) : stats.count === 0 ? (
        <div class="rounded-lg border border-neutral-200 bg-white p-6 text-sm dark:border-neutral-800 dark:bg-neutral-900">
          <p>
            The CSL database is empty. As an admin, go to{" "}
            <a
              href="/admin"
              class="text-ocelot-accent hover:underline"
              onClick={(e) => {
                e.preventDefault();
                navigate("/admin");
              }}
            >
              Admin
            </a>{" "}
            and click <strong>Update CSL now</strong>.
          </p>
        </div>
      ) : (
        <div class="space-y-8">
          <Summary stats={stats} />
          <BySourceTable stats={stats} />
          <ByEntityTypeTable stats={stats} />
          <TopList title="Top programs" rows={stats.top_programs.slice(0, 12)} kind="program" />
          <TopList
            title="Top nationalities"
            rows={stats.top_nationalities.slice(0, 12)}
            kind="nationality"
          />
        </div>
      )}
    </div>
  );
}

function Summary({ stats }: { stats: CslStats }) {
  const fetchedAt =
    stats.fetched_at && stats.fetched_at > 0
      ? new Date(stats.fetched_at * 1000).toISOString().replace("T", " ").slice(0, 19) + " UTC"
      : "—";
  const sourceFromVersion = stats.version?.split("-")[0] ?? "—";
  return (
    <section class="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
      <Stat label="Total records" value={stats.count.toLocaleString()} />
      <Stat label="Source lists" value={String(stats.by_source.length)} />
      <Stat label="With aliases" value={`${pct(stats.with_aliases, stats.count)}% (${stats.with_aliases.toLocaleString()})`} />
      <Stat
        label="With addresses"
        value={`${pct(stats.with_addresses, stats.count)}% (${stats.with_addresses.toLocaleString()})`}
      />
      <Stat label="Fetched" value={fetchedAt} />
      <Stat label="Source" value={sourceFromVersion} />
      <Stat label="Version" value={stats.version ?? "—"} mono />
    </section>
  );
}

function Stat({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div class="rounded-lg border border-neutral-200 bg-white p-3 shadow-sm dark:border-neutral-800 dark:bg-neutral-900">
      <div class="text-xs uppercase tracking-wide text-neutral-500">{label}</div>
      <div class={`mt-1 ${mono ? "font-mono text-sm break-all" : "font-display text-lg"}`}>{value}</div>
    </div>
  );
}

function BySourceTable({ stats }: { stats: CslStats }) {
  return (
    <section>
      <h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-neutral-500">By source list</h2>
      <div class="overflow-hidden rounded-lg border border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-900">
        <table class="w-full text-sm">
          <thead class="bg-neutral-50 text-xs uppercase tracking-wide text-neutral-500 dark:bg-neutral-800">
            <tr>
              <th class="px-3 py-2 text-left">Code</th>
              <th class="px-3 py-2 text-left">Long name</th>
              <th class="px-3 py-2 text-right">Records</th>
              <th class="px-3 py-2 text-right">% of corpus</th>
              <th class="px-3 py-2 text-left">Agency</th>
            </tr>
          </thead>
          <tbody>
            {stats.by_source.map((s) => (
              <tr key={s.code} class="border-t border-neutral-100 dark:border-neutral-800">
                <td class="px-3 py-2">
                  <Tag kind="source" source_code={s.code}>
                    {s.code}
                  </Tag>
                </td>
                <td class="px-3 py-2 text-xs">{s.long_name ?? <em class="text-neutral-500">unknown</em>}</td>
                <td class="px-3 py-2 text-right font-mono">{s.count.toLocaleString()}</td>
                <td class="px-3 py-2 text-right text-xs text-neutral-500">{pct(s.count, stats.count)}%</td>
                <td class="px-3 py-2 text-xs">
                  {s.agency_url ? (
                    <a
                      href={s.agency_url}
                      target="_blank"
                      rel="noreferrer noopener"
                      class="text-ocelot-accent hover:underline"
                    >
                      ↗ link
                    </a>
                  ) : (
                    <span class="text-neutral-500">—</span>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function ByEntityTypeTable({ stats }: { stats: CslStats }) {
  return (
    <section>
      <h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-neutral-500">By entity type</h2>
      <div class="overflow-hidden rounded-lg border border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-900">
        <table class="w-full text-sm">
          <thead class="bg-neutral-50 text-xs uppercase tracking-wide text-neutral-500 dark:bg-neutral-800">
            <tr>
              <th class="px-3 py-2 text-left">Type</th>
              <th class="px-3 py-2 text-right">Records</th>
              <th class="px-3 py-2 text-right">% of corpus</th>
            </tr>
          </thead>
          <tbody>
            {stats.by_entity_type.map((row) => (
              <tr key={row.entity_type} class="border-t border-neutral-100 dark:border-neutral-800">
                <td class="px-3 py-2">
                  <Tag kind="entity">{row.entity_type}</Tag>
                </td>
                <td class="px-3 py-2 text-right font-mono">{row.count.toLocaleString()}</td>
                <td class="px-3 py-2 text-right text-xs text-neutral-500">{pct(row.count, stats.count)}%</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

function TopList({
  title,
  rows,
  kind,
}: {
  title: string;
  rows: { name: string; count: number }[];
  kind: "program" | "nationality";
}) {
  if (rows.length === 0) return null;
  return (
    <section>
      <h2 class="mb-3 text-sm font-semibold uppercase tracking-wide text-neutral-500">{title}</h2>
      <div class="flex flex-wrap gap-2 rounded-lg border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900">
        {rows.map((r) => (
          <span key={r.name} class="inline-flex items-center gap-1.5">
            <Tag kind={kind}>{r.name}</Tag>
            <span class="text-xs text-neutral-500">{r.count.toLocaleString()}</span>
          </span>
        ))}
      </div>
    </section>
  );
}

function pct(part: number, total: number): string {
  if (!total) return "0.0";
  return ((100 * part) / total).toFixed(1);
}
