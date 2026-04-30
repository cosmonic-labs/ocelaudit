import { useEffect, useMemo, useState } from "preact/hooks";
import { api, type AuditEvent } from "../api";
import { decisionStyle } from "../decision";
import { navigate, readQuery } from "../router";

export function AuditPage() {
  const id = readQuery("id");
  if (id) return <AuditDetail id={id} />;
  return <AuditList />;
}

interface ColumnFilter {
  when: string;
  who: string;
  source: string;
  query: string;
  tlp: string;
  decision: string;
}

const EMPTY_FILTER: ColumnFilter = {
  when: "",
  who: "",
  source: "",
  query: "",
  tlp: "",
  decision: "",
};

function AuditList() {
  const [events, setEvents] = useState<AuditEvent[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [offset, setOffset] = useState(0);
  const [filter, setFilter] = useState<ColumnFilter>(EMPTY_FILTER);
  const limit = 50;

  useEffect(() => {
    api
      .auditList(limit, offset)
      .then((r) => setEvents(r.events))
      .catch((e) => setError(String((e as Error).message ?? e)));
  }, [offset]);

  const filtered = useMemo(() => {
    if (!events) return [];
    const norm = (s: string) => s.trim().toLowerCase();
    const f = {
      when: norm(filter.when),
      who: norm(filter.who),
      source: norm(filter.source),
      query: norm(filter.query),
      tlp: norm(filter.tlp),
      decision: norm(filter.decision),
    };
    return events.filter((e) => {
      const whenStr = new Date(e.when * 1000)
        .toISOString()
        .slice(0, 19)
        .replace("T", " ");
      return (
        (!f.when || whenStr.toLowerCase().includes(f.when)) &&
        (!f.who || e.who.toLowerCase().includes(f.who)) &&
        (!f.source || (e.source ?? "api").toLowerCase().includes(f.source)) &&
        (!f.query || e.query.toLowerCase().includes(f.query)) &&
        (!f.tlp || e.tlp.toLowerCase().includes(f.tlp)) &&
        (!f.decision || e.decision.toLowerCase().includes(f.decision))
      );
    });
  }, [events, filter]);

  const matchSummary =
    events && events.length > 0
      ? `${filtered.length} of ${events.length} shown`
      : "—";

  return (
    <div>
      <header class="mb-6 flex items-baseline justify-between">
        <h1 class="font-display text-2xl">Audit log</h1>
        <p class="text-sm text-neutral-500">{matchSummary} · newest first</p>
      </header>

      {error && (
        <p class="mb-4 rounded border border-tlp-red/40 bg-tlp-red/10 px-3 py-2 text-sm text-tlp-red">
          {error}
        </p>
      )}

      {!events ? (
        <p class="text-sm text-neutral-500">loading…</p>
      ) : events.length === 0 ? (
        <p class="text-sm text-neutral-500">no audit events yet — run a search.</p>
      ) : (
        <div class="overflow-hidden rounded-lg border border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-900">
          <table class="w-full text-sm">
            <thead class="bg-neutral-50 text-xs uppercase tracking-wide text-neutral-500 dark:bg-neutral-800">
              <tr>
                <th class="px-3 py-2 text-left">When</th>
                <th class="px-3 py-2 text-left">Who</th>
                <th class="px-3 py-2 text-left">Source</th>
                <th class="px-3 py-2 text-left">Query</th>
                <th class="px-3 py-2 text-left">TLP</th>
                <th class="px-3 py-2 text-left">Decision</th>
              </tr>
              <tr class="bg-white dark:bg-neutral-900">
                <FilterCell value={filter.when} onChange={(v) => setFilter({ ...filter, when: v })} placeholder="2026-…" />
                <FilterCell value={filter.who} onChange={(v) => setFilter({ ...filter, who: v })} placeholder="user…" />
                <FilterCell value={filter.source} onChange={(v) => setFilter({ ...filter, source: v })} placeholder="ui / api" />
                <FilterCell value={filter.query} onChange={(v) => setFilter({ ...filter, query: v })} placeholder="text…" />
                <FilterCell value={filter.tlp} onChange={(v) => setFilter({ ...filter, tlp: v })} placeholder="green/yellow/red" />
                <FilterCell value={filter.decision} onChange={(v) => setFilter({ ...filter, decision: v })} placeholder="auto/pending/blocked…" />
              </tr>
            </thead>
            <tbody>
              {filtered.map((e) => {
                const ds = decisionStyle(e.decision);
                return (
                  <tr
                    key={e.audit_id}
                    class="cursor-pointer border-t border-neutral-100 hover:bg-neutral-50 dark:border-neutral-800 dark:hover:bg-neutral-800/60"
                    onClick={() => navigate(`/audit?id=${encodeURIComponent(e.audit_id)}`)}
                  >
                    <td class="whitespace-nowrap px-3 py-2 text-xs text-neutral-500">
                      {new Date(e.when * 1000)
                        .toISOString()
                        .slice(0, 19)
                        .replace("T", " ")}
                    </td>
                    <td class="px-3 py-2">{e.who}</td>
                    <td class="px-3 py-2">
                      <SourceBadge source={e.source ?? "api"} />
                    </td>
                    <td class="px-3 py-2 max-w-xs truncate">{e.query}</td>
                    <td class="px-3 py-2">
                      <TlpBadge tlp={e.tlp} />
                    </td>
                    <td class="px-3 py-2">
                      <span class={`inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-xs ${ds.bg} ${ds.text}`}>
                        <span class={`inline-block h-1.5 w-1.5 rounded-full ${ds.dot}`} aria-hidden />
                        <code>{e.decision}</code>
                      </span>
                    </td>
                  </tr>
                );
              })}
              {filtered.length === 0 && events.length > 0 && (
                <tr>
                  <td class="px-3 py-6 text-center text-xs text-neutral-500" colSpan={6}>
                    no rows match the column filters above
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      )}

      <footer class="mt-4 flex items-center justify-between">
        <button
          disabled={offset === 0}
          onClick={() => setOffset(Math.max(0, offset - limit))}
          class="rounded border border-neutral-300 px-3 py-1 text-sm disabled:opacity-50 dark:border-neutral-700"
        >
          ← Newer
        </button>
        <span class="text-xs text-neutral-500">offset {offset}</span>
        <button
          disabled={(events?.length ?? 0) < limit}
          onClick={() => setOffset(offset + limit)}
          class="rounded border border-neutral-300 px-3 py-1 text-sm disabled:opacity-50 dark:border-neutral-700"
        >
          Older →
        </button>
      </footer>
    </div>
  );
}

function FilterCell({ value, onChange, placeholder }: { value: string; onChange: (v: string) => void; placeholder: string }) {
  return (
    <th class="px-2 py-1">
      <input
        type="text"
        value={value}
        placeholder={placeholder}
        onInput={(e) => onChange((e.currentTarget as HTMLInputElement).value)}
        class="w-full rounded border border-neutral-200 bg-white px-2 py-1 text-xs font-normal normal-case tracking-normal text-neutral-700 outline-none focus:border-ocelot-accent dark:border-neutral-700 dark:bg-neutral-800 dark:text-neutral-200"
      />
    </th>
  );
}

function SourceBadge({ source }: { source: string }) {
  const cls =
    source === "ui"
      ? "bg-blue-500/10 text-blue-600 dark:text-blue-400"
      : "bg-neutral-200/60 text-neutral-700 dark:bg-neutral-700/60 dark:text-neutral-200";
  return (
    <span class={`inline-block rounded px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wider ${cls}`}>
      {source}
    </span>
  );
}

function AuditDetail({ id }: { id: string }) {
  const [event, setEvent] = useState<AuditEvent | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .auditGet(id)
      .then(setEvent)
      .catch((e) => setError(String((e as Error).message ?? e)));
  }, [id]);

  return (
    <div>
      <header class="mb-6 flex items-baseline gap-4">
        <h1 class="font-display text-2xl">Audit detail</h1>
        <a
          href="/audit"
          onClick={(e) => {
            e.preventDefault();
            navigate("/audit");
          }}
          class="text-sm text-ocelot-accent hover:underline"
        >
          ← back to list
        </a>
      </header>

      {error && (
        <p class="mb-4 rounded border border-tlp-red/40 bg-tlp-red/10 px-3 py-2 text-sm text-tlp-red">
          {error}
        </p>
      )}

      {!event ? (
        <p class="text-sm text-neutral-500">loading…</p>
      ) : (
        <article class="space-y-6 rounded-lg border border-neutral-200 bg-white p-6 dark:border-neutral-800 dark:bg-neutral-900">
          <dl class="grid gap-4 sm:grid-cols-2">
            <Field label="audit_id" value={<code class="break-all text-xs">{event.audit_id}</code>} />
            <Field label="when" value={new Date(event.when * 1000).toISOString().replace("T", " ").slice(0, 19)} />
            <Field label="who" value={<code>{event.who}</code>} />
            <Field label="source" value={<SourceBadge source={event.source ?? "api"} />} />
            <Field label="tlp" value={<TlpBadge tlp={event.tlp} />} />
            <Field
              label="initial decision"
              value={<DecisionBadge decision={event.initial_decision ?? event.decision} />}
            />
            <Field label="current decision" value={<DecisionBadge decision={event.decision} />} />
            <Field label="query" value={<span class="break-words">{event.query}</span>} />
          </dl>

          {(event.history?.length ?? 0) > 0 && (
            <section>
              <h2 class="mb-2 text-sm font-semibold uppercase tracking-wide text-neutral-500">Decision history</h2>
              <ol class="space-y-2">
                {event.history!.map((h, i) => (
                  <li
                    key={i}
                    class="rounded border border-neutral-200 bg-neutral-50 p-3 text-sm dark:border-neutral-800 dark:bg-neutral-800/40"
                  >
                    <div class="flex items-center justify-between">
                      <DecisionBadge decision={h.decision} />
                      <span class="text-xs text-neutral-500">
                        by {h.decided_by} ·{" "}
                        {new Date(h.decided_at * 1000).toISOString().slice(0, 19).replace("T", " ")}
                      </span>
                    </div>
                    {h.note && <p class="mt-1 text-xs text-neutral-600 dark:text-neutral-300">{h.note}</p>}
                  </li>
                ))}
              </ol>
            </section>
          )}
        </article>
      )}
    </div>
  );
}

function DecisionBadge({ decision }: { decision: string }) {
  const ds = decisionStyle(decision);
  return (
    <span class={`inline-flex items-center gap-1.5 rounded-full px-2 py-0.5 text-xs ${ds.bg} ${ds.text}`}>
      <span class={`inline-block h-1.5 w-1.5 rounded-full ${ds.dot}`} aria-hidden />
      <code>{decision}</code>
    </span>
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

function TlpBadge({ tlp }: { tlp: string }) {
  const dot = tlp === "red" ? "bg-tlp-red" : tlp === "yellow" ? "bg-tlp-yellow" : "bg-tlp-green";
  const text = tlp === "red" ? "text-tlp-red" : tlp === "yellow" ? "text-tlp-yellow" : "text-tlp-green";
  return (
    <span class="inline-flex items-center gap-1.5">
      <span class={`inline-block h-2 w-2 rounded-full ${dot}`} aria-hidden />
      <span class={`text-xs font-semibold uppercase tracking-wider ${text}`}>{tlp}</span>
    </span>
  );
}
