import { useEffect, useState } from "preact/hooks";
import { api, type AuditEvent } from "../api";

interface Toast {
  audit_id: string;
  decision: "cleared" | "blocked";
  decided_by: string;
}

export function ReviewPage() {
  const [items, setItems] = useState<AuditEvent[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [toast, setToast] = useState<Toast | null>(null);

  async function reload() {
    try {
      const r = await api.reviewQueue();
      setItems(r.items);
    } catch (e) {
      setError(String((e as Error).message ?? e));
    }
  }

  useEffect(() => {
    void reload();
  }, []);

  async function decide(audit_id: string, decision: "cleared" | "blocked") {
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
      <header class="mb-6 flex items-baseline justify-between">
        <h1 class="font-display text-2xl">Review queue</h1>
        <p class="text-sm text-neutral-500">{items?.length ?? "…"} pending</p>
      </header>

      {error && (
        <p class="mb-4 rounded border border-tlp-red/40 bg-tlp-red/10 px-3 py-2 text-sm text-tlp-red">
          {error}
        </p>
      )}

      {toast && (
        <p
          class={`mb-4 rounded border px-3 py-2 text-sm ${
            toast.decision === "cleared"
              ? "border-tlp-green/40 bg-tlp-green/10 text-tlp-green"
              : "border-tlp-red/40 bg-tlp-red/10 text-tlp-red"
          }`}
        >
          ✓ <code class="rounded bg-white/40 px-1 dark:bg-black/30">{toast.audit_id.slice(0, 8)}…</code> marked{" "}
          <strong>{toast.decision}</strong> by {toast.decided_by} — recorded in the audit log.
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
                <div class="mt-4 space-y-3 border-t border-neutral-200 pt-4 dark:border-neutral-800">
                  <textarea
                    placeholder="Reason / note (required)"
                    value={note}
                    onInput={(e) => setNote((e.currentTarget as HTMLTextAreaElement).value)}
                    rows={3}
                    class="w-full rounded border border-neutral-300 bg-white px-3 py-2 text-sm dark:border-neutral-700 dark:bg-neutral-800"
                  />
                  <div class="flex items-center gap-2">
                    <button
                      type="button"
                      disabled={busy || !note.trim()}
                      onClick={() => decide(it.audit_id, "cleared")}
                      class="rounded bg-tlp-green px-3 py-1.5 text-sm font-semibold text-white disabled:opacity-50"
                    >
                      Clear
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
