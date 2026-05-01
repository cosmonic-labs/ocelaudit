import { useEffect, useState } from "preact/hooks";
import { api, type AuditEvent, type Hit } from "../api";
import { Tag } from "../components/Tag";

interface Toast {
  audit_id: string;
  decision: "approved" | "blocked";
  decided_by: string;
}

export function ReviewPage() {
  const [items, setItems] = useState<AuditEvent[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [toast, setToast] = useState<Toast | null>(null);
  const [includeAuto, setIncludeAuto] = useState(false);

  async function reload() {
    try {
      const r = await api.reviewQueue({ includeAuto });
      setItems(r.items);
    } catch (e) {
      setError(String((e as Error).message ?? e));
    }
  }

  useEffect(() => {
    void reload();
  }, [includeAuto]);

  async function decide(audit_id: string, decision: "approved" | "blocked") {
    if (!note.trim()) {
      setError("a note is required when deciding.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const r = await api.reviewDecide(audit_id, decision, note.trim());
      setToast({ audit_id, decision, decided_by: r.decided_by });
      setNote("");
      setActiveId(null);
      await reload();
      // Surface the toast for ~6s then drop it.
      window.setTimeout(() => setToast((t) => (t?.audit_id === audit_id ? null : t)), 6000);
    } catch (e) {
      setError(String((e as Error).message ?? e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <header class="mb-2 flex items-baseline justify-between">
        <h1 class="font-display text-2xl">Review queue</h1>
        <p class="text-sm text-neutral-500">{items?.length ?? "…"} shown</p>
      </header>
      <p class="mb-4 max-w-3xl text-xs text-neutral-500">
        Default: <strong>pending-review</strong> (yellow) and <strong>pending-block</strong> (red,
        high-similarity but not exact). Items that hit <strong>auto-block</strong> — exact
        name/alias matches — are auto-decided and don't enter the queue. Toggle
        below to spot-check them anyway.
      </p>
      <label class="mb-4 inline-flex cursor-pointer items-center gap-2 text-xs text-neutral-700 dark:text-neutral-300">
        <input
          type="checkbox"
          checked={includeAuto}
          onChange={(e) => setIncludeAuto((e.currentTarget as HTMLInputElement).checked)}
        />
        also show <code class="rounded bg-neutral-100 px-1 dark:bg-neutral-800">auto-block</code> items
      </label>

      {error && (
        <p class="mb-4 rounded border border-tlp-red/40 bg-tlp-red/10 px-3 py-2 text-sm text-tlp-red">
          {error}
        </p>
      )}

      {toast && (
        <p
          class={`mb-4 rounded border px-3 py-2 text-sm ${
            toast.decision === "approved"
              ? "border-tlp-green/40 bg-tlp-green/10 text-tlp-green"
              : "border-tlp-red/40 bg-tlp-red/10 text-tlp-red"
          }`}
        >
          ✓ <code class="rounded bg-white/40 px-1 dark:bg-black/30">{toast.audit_id.slice(0, 8)}…</code> marked{" "}
          <strong>reviewed · {toast.decision === "approved" ? "approved" : "blocked"}</strong> by{" "}
          {toast.decided_by} — recorded in the audit log.
        </p>
      )}

      {!items ? (
        <p class="text-sm text-neutral-500">loading…</p>
      ) : items.length === 0 ? (
        <p class="text-sm text-neutral-500">queue is empty. Run a search that produces a YELLOW or RED hit to populate.</p>
      ) : (
        <ul class="space-y-3">
          {items.map((it) => (
            <li
              key={it.audit_id}
              class="rounded-lg border border-neutral-200 bg-white p-4 dark:border-neutral-800 dark:bg-neutral-900"
            >
              <div class="flex items-start justify-between gap-4">
                <div class="min-w-0">
                  <div class="flex items-center gap-2 text-xs">
                    <TlpBadge tlp={it.tlp} />
                    <code class="rounded bg-neutral-100 px-1 dark:bg-neutral-800">{it.decision}</code>
                    <span class="text-neutral-500">by {it.who}</span>
                    <span class="text-neutral-500">
                      · {new Date(it.when * 1000).toISOString().slice(0, 19).replace("T", " ")}
                    </span>
                  </div>
                  <p class="mt-2 font-display text-base">{it.query}</p>
                  <p class="mt-1 truncate text-xs text-neutral-500">audit_id {it.audit_id}</p>
                </div>
                <button
                  type="button"
                  onClick={() => setActiveId(activeId === it.audit_id ? null : it.audit_id)}
                  class="shrink-0 rounded border border-neutral-300 px-3 py-1 text-xs hover:bg-neutral-100 dark:border-neutral-700 dark:hover:bg-neutral-800"
                >
                  {activeId === it.audit_id ? "cancel" : "decide"}
                </button>
              </div>
              {activeId === it.audit_id && (
                <div class="mt-4 space-y-4 border-t border-neutral-200 pt-4 dark:border-neutral-800">
                  {/* Original hits the engine returned at search time. */}
                  <CandidateMatches hits={it.top_hits ?? []} />

                  <div>
                    <label class="mb-1 block text-xs uppercase tracking-wide text-neutral-500">
                      Reviewer note (required)
                    </label>
                    <textarea
                      placeholder="Why are you clearing or blocking this?"
                      value={note}
                      onInput={(e) => setNote((e.currentTarget as HTMLTextAreaElement).value)}
                      rows={3}
                      class="w-full rounded border border-neutral-300 bg-white px-3 py-2 text-sm dark:border-neutral-700 dark:bg-neutral-800"
                    />
                  </div>
                  <div class="flex items-center gap-2">
                    <button
                      type="button"
                      disabled={busy || !note.trim()}
                      onClick={() => decide(it.audit_id, "approved")}
                      class="rounded bg-tlp-green px-3 py-1.5 text-sm font-semibold text-white disabled:opacity-50"
                    >
                      Approve
                    </button>
                    <button
                      type="button"
                      disabled={busy || !note.trim()}
                      onClick={() => decide(it.audit_id, "blocked")}
                      class="rounded bg-tlp-red px-3 py-1.5 text-sm font-semibold text-white disabled:opacity-50"
                    >
                      Block
                    </button>
                  </div>
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function CandidateMatches({ hits }: { hits: Hit[] }) {
  if (hits.length === 0) {
    return (
      <p class="text-xs italic text-neutral-500">
        No candidate hits stored for this event (older audit row, or empty result set).
      </p>
    );
  }
  return (
    <section>
      <h3 class="mb-2 text-xs font-semibold uppercase tracking-wide text-neutral-500">
        Candidate matches ({hits.length})
      </h3>
      <ol class="space-y-2">
        {hits.map((h) => (
          <li
            key={h.entry_id}
            class="rounded border border-neutral-200 bg-neutral-50 p-3 text-sm dark:border-neutral-800 dark:bg-neutral-800/40"
          >
            <div class="flex items-start justify-between gap-3">
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <TlpBadge tlp={h.tlp} />
                  <span class="text-xs text-neutral-500">score {h.score.toFixed(3)}</span>
                  <span class="truncate text-xs text-neutral-500">id {h.entry_id}</span>
                </div>
                <p class="mt-1 font-display text-base leading-tight">{h.snippet}</p>
                {h.matched_fields.length > 0 && (
                  <p class="mt-1 text-xs text-neutral-500">matched: {h.matched_fields.join(", ")}</p>
                )}
                {h.tags && (
                  <div class="mt-2 flex flex-wrap gap-1.5">
                    {h.tags.source_list && (
                      <Tag
                        kind="source"
                        source_code={h.tags.source_list}
                        href={h.citation?.agency_url}
                        title={h.citation?.long_name ?? h.tags.source_list}
                      >
                        {h.tags.source_list}
                      </Tag>
                    )}
                    {h.tags.entity_type && h.tags.entity_type !== "unknown" && (
                      <Tag kind="entity">{h.tags.entity_type}</Tag>
                    )}
                    {h.tags.programs.slice(0, 4).map((p) => (
                      <Tag kind="program">{p}</Tag>
                    ))}
                    {h.tags.programs.length > 4 && (
                      <Tag kind="neutral">+{h.tags.programs.length - 4}</Tag>
                    )}
                    {h.tags.nationalities.slice(0, 4).map((n) => (
                      <Tag kind="nationality">{n}</Tag>
                    ))}
                  </div>
                )}
              </div>
            </div>
          </li>
        ))}
      </ol>
    </section>
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
