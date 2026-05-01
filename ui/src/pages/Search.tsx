import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { api, type Hit, type SearchResponse, type Tlp } from "../api";
import { Tag } from "../components/Tag";
import { navigate, readQuery } from "../router";

export function SearchPage() {
  const initialQ = readQuery("q") ?? "";
  const [q, setQ] = useState(initialQ);
  const [sources, setSources] = useState<Set<string>>(new Set());
  const [knownSources, setKnownSources] = useState<{ code: string; long_name: string }[]>([]);
  const [fuzzy, setFuzzy] = useState(true);
  const [limit, setLimit] = useState(20);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [response, setResponse] = useState<SearchResponse | null>(null);
  const [autocomplete, setAutocomplete] = useState<string[]>([]);
  const debounceRef = useRef<number | null>(null);

  useEffect(() => {
    api
      .cslSources()
      .then((r) => setKnownSources(r.known))
      .catch(() => setKnownSources([]));
  }, []);

  useEffect(() => {
    if (initialQ) {
      void runSearch(initialQ);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Debounced autocomplete on q change.
  useEffect(() => {
    if (debounceRef.current !== null) {
      window.clearTimeout(debounceRef.current);
      debounceRef.current = null;
    }
    if (!q || q.length < 2) {
      setAutocomplete([]);
      return;
    }
    debounceRef.current = window.setTimeout(async () => {
      const r = await api.autocomplete(q);
      setAutocomplete(r);
    }, 150);
    return () => {
      if (debounceRef.current !== null) {
        window.clearTimeout(debounceRef.current);
      }
    };
  }, [q]);

  async function runSearch(value: string) {
    setBusy(true);
    setError(null);
    // Submitting the search dismisses the autocomplete drop-down. The
    // earlier behaviour left it floating over the first result row.
    setAutocomplete([]);
    if (debounceRef.current !== null) {
      window.clearTimeout(debounceRef.current);
      debounceRef.current = null;
    }
    try {
      const r = await api.search(value, {
        sources: sources.size > 0 ? [...sources] : undefined,
        fuzzy,
        limit,
      });
      setResponse(r);
      // Push the query into the URL so it's bookmarkable.
      const url = new URL(window.location.href);
      url.searchParams.set("q", value);
      window.history.replaceState({}, "", url.toString());
    } catch (e: unknown) {
      setError(String((e as Error)?.message ?? e));
    } finally {
      setBusy(false);
    }
  }

  function toggleSource(code: string) {
    const next = new Set(sources);
    if (next.has(code)) next.delete(code);
    else next.add(code);
    setSources(next);
  }

  return (
    <div>
      <header class="mb-6 flex items-baseline gap-4">
        <h1 class="font-display text-2xl">Search</h1>
        <a href="/" onClick={(e) => { e.preventDefault(); navigate("/"); }} class="text-sm text-ocelot-accent hover:underline">
          ← Dashboard
        </a>
      </header>

      <form
        onSubmit={(e) => {
          e.preventDefault();
          void runSearch(q);
        }}
        class="mb-6 space-y-4 rounded-lg border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900"
      >
        <div class="relative">
          <input
            type="text"
            placeholder="Name, alias, or address…"
            value={q}
            onInput={(e) => setQ((e.currentTarget as HTMLInputElement).value)}
            autocomplete="off"
            class="w-full rounded border border-neutral-300 bg-white px-3 py-2 text-sm outline-none focus:border-ocelot-accent dark:border-neutral-700 dark:bg-neutral-800"
          />
          {autocomplete.length > 0 && (
            <ul class="absolute z-10 mt-1 max-h-64 w-full overflow-auto rounded border border-neutral-200 bg-white text-sm shadow-md dark:border-neutral-700 dark:bg-neutral-800">
              {autocomplete.map((s) => (
                <li>
                  <button
                    type="button"
                    onClick={() => {
                      setQ(s);
                      setAutocomplete([]);
                      void runSearch(s);
                    }}
                    class="block w-full px-3 py-2 text-left hover:bg-neutral-100 dark:hover:bg-neutral-700"
                  >
                    {s}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>

        <fieldset class="flex flex-wrap items-center gap-4">
          <legend class="sr-only">Filters</legend>
          <label class="inline-flex items-center gap-2 text-sm">
            <input
              type="checkbox"
              checked={fuzzy}
              onChange={(e) => setFuzzy((e.currentTarget as HTMLInputElement).checked)}
            />
            Fuzzy match
          </label>
          <label class="inline-flex items-center gap-2 text-sm">
            <span>Limit:</span>
            <select
              value={String(limit)}
              onChange={(e) => setLimit(Number((e.currentTarget as HTMLSelectElement).value))}
              class="rounded border border-neutral-300 bg-white px-2 py-1 dark:border-neutral-700 dark:bg-neutral-800"
            >
              {[10, 20, 50, 100].map((n) => (
                <option value={n}>{n}</option>
              ))}
            </select>
          </label>
          <button
            type="submit"
            disabled={busy || !q}
            class="ml-auto rounded bg-ocelot-mark px-4 py-2 text-sm font-semibold text-white disabled:opacity-50 dark:bg-ocelot-paper dark:text-ocelot-ink"
          >
            {busy ? "searching…" : "Search"}
          </button>
        </fieldset>

        {knownSources.length > 0 && (
          <details>
            <summary class="cursor-pointer text-xs text-neutral-500">Filter by source list</summary>
            <div class="mt-2 flex flex-wrap gap-2">
              {knownSources.map((s) => (
                <button
                  type="button"
                  onClick={() => toggleSource(s.code)}
                  title={s.long_name}
                  class={`rounded border px-2 py-1 text-xs transition ${
                    sources.has(s.code)
                      ? "border-ocelot-accent bg-ocelot-accent/10 text-ocelot-accent"
                      : "border-neutral-300 hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
                  }`}
                >
                  {s.code}
                </button>
              ))}
            </div>
          </details>
        )}
      </form>

      {error && (
        <p class="mb-4 rounded border border-tlp-red/40 bg-tlp-red/10 px-3 py-2 text-sm text-tlp-red">
          {error}
        </p>
      )}

      {response && <ResultBlock response={response} />}
    </div>
  );
}

function ResultBlock({ response }: { response: SearchResponse }) {
  const tlpColor = useMemo(() => tlpClasses(response.tlp), [response.tlp]);
  return (
    <section>
      <header class={`mb-4 rounded-lg border p-3 ${tlpColor.bg}`}>
        <div class="flex items-center gap-3">
          <span class={`inline-block h-3 w-3 rounded-full ${tlpColor.dot}`} aria-hidden />
          <strong class={`font-semibold uppercase tracking-wider ${tlpColor.text}`}>{response.tlp}</strong>
          <span class="text-sm text-neutral-600 dark:text-neutral-300">
            {response.hits.length} {response.hits.length === 1 ? "hit" : "hits"}
            {" · decision: "}
            <code class="rounded bg-white/60 px-1 dark:bg-black/30">{response.decision}</code>
            {" · audit_id: "}
            <code class="rounded bg-white/60 px-1 dark:bg-black/30">{response.audit_id.slice(0, 8)}…</code>
          </span>
        </div>
        {response.note && (
          <p class="mt-2 text-sm text-neutral-700 dark:text-neutral-300">{response.note}</p>
        )}
      </header>
      <ul class="space-y-2">
        {response.hits.map((h) => <HitCard key={h.entry_id} hit={h} />)}
        {response.hits.length === 0 && (
          <li class="text-sm text-neutral-500">no hits.</li>
        )}
      </ul>
    </section>
  );
}

export function HitCard({ hit }: { hit: Hit }) {
  const t = tlpClasses(hit.tlp);
  const cite = hit.citation;
  const tags = hit.tags;
  return (
    <li class={`rounded-lg border ${t.border} bg-white p-4 dark:bg-neutral-900`}>
      <div class="flex items-start justify-between gap-4">
        <div class="min-w-0 flex-1">
          <div class="flex items-center gap-2">
            <span class={`inline-block h-2 w-2 rounded-full ${t.dot}`} aria-hidden />
            <span class={`text-xs font-semibold uppercase tracking-wider ${t.text}`}>{hit.tlp}</span>
            <span class="text-xs text-neutral-500">score {hit.score.toFixed(3)}</span>
          </div>
          <h3 class="mt-1 font-display text-lg">{hit.snippet}</h3>
          <p class="mt-1 truncate text-xs text-neutral-500">id: {hit.entry_id}</p>
          {hit.matched_fields.length > 0 && (
            <p class="mt-1 text-xs text-neutral-500">matched: {hit.matched_fields.join(", ")}</p>
          )}
          {tags && (tags.source_list || tags.entity_type || tags.programs.length > 0 || tags.nationalities.length > 0) && (
            <div class="mt-2 flex flex-wrap gap-1.5">
              {tags.source_list && (
                <Tag
                  kind="source"
                  source_code={tags.source_list}
                  href={cite?.agency_url}
                  title={cite?.long_name ?? tags.source_list}
                >
                  {tags.source_list}
                </Tag>
              )}
              {tags.entity_type && tags.entity_type !== "unknown" && (
                <Tag kind="entity">{tags.entity_type}</Tag>
              )}
              {tags.programs.slice(0, 4).map((p) => (
                <Tag kind="program">{p}</Tag>
              ))}
              {tags.programs.length > 4 && (
                <Tag kind="neutral">+{tags.programs.length - 4}</Tag>
              )}
              {tags.nationalities.slice(0, 4).map((n) => (
                <Tag kind="nationality">{n}</Tag>
              ))}
            </div>
          )}
        </div>
        {cite?.agency_url && (
          <a
            href={cite.agency_url}
            target="_blank"
            rel="noreferrer noopener"
            class="shrink-0 text-xs text-ocelot-accent hover:underline"
          >
            agency ↗
          </a>
        )}
      </div>
    </li>
  );
}

function tlpClasses(t: Tlp) {
  switch (t) {
    case "red":
      return { dot: "bg-tlp-red", text: "text-tlp-red", border: "border-tlp-red/40", bg: "bg-tlp-red/5 border-tlp-red/40" };
    case "yellow":
      return { dot: "bg-tlp-yellow", text: "text-tlp-yellow", border: "border-tlp-yellow/40", bg: "bg-tlp-yellow/5 border-tlp-yellow/40" };
    case "green":
      return { dot: "bg-tlp-green", text: "text-tlp-green", border: "border-tlp-green/40", bg: "bg-tlp-green/5 border-tlp-green/40" };
  }
}
