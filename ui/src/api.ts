// Small fetch wrapper for the OcelAudit API.
// Cookies travel automatically with `credentials: "include"` so the
// HttpOnly session cookie set by /api/v1/auth/login flows on subsequent
// requests without any client-side handling.

export type Role = "admin" | "compliance";
export type Tlp = "green" | "yellow" | "red";

export interface Me {
  username: string;
  role: Role;
  iat: number;
}

export interface MetricsBody {
  csl_count: number;
  csl_sources: { name: string; count: number }[];
  queries_recent: number;
  tlp_histogram: { red: number; yellow: number; green: number };
  last_csl_refresh: number;
  queue_depth: number;
}

export interface Hit {
  entry_id: string;
  score: number;
  tlp: Tlp;
  matched_fields: string[];
  snippet: string;
  citation: { source_code?: string; long_name?: string; agency_url?: string } | null;
}

export interface SearchResponse {
  audit_id: string;
  tlp: Tlp;
  decision: string;
  hits: Hit[];
  note?: string;
}

class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.status = status;
  }
}

async function call<T>(method: string, path: string, body?: unknown): Promise<T> {
  const init: RequestInit = {
    method,
    credentials: "include",
    headers: body ? { "content-type": "application/json" } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  };
  const r = await fetch(path, init);
  if (!r.ok) {
    let msg = r.statusText;
    try {
      const j = (await r.json()) as { error?: string };
      if (j.error) msg = j.error;
    } catch {
      // body wasn't JSON — keep statusText
    }
    throw new ApiError(r.status, msg);
  }
  return (await r.json()) as T;
}

export interface AuditEvent {
  audit_id: string;
  who: string;
  when: number;
  query: string;
  tlp: string;
  decision: string;
  initial_decision?: string;
  top_hit_ids: string[];
  history?: WorkflowHistoryEntry[];
}

export interface WorkflowHistoryEntry {
  audit_id: string;
  decision: string;
  decided_by: string;
  decided_at: number;
  note: string | null;
}

export interface AuditList {
  limit: number;
  offset: number;
  events: AuditEvent[];
}

export interface ReviewQueue {
  count: number;
  items: AuditEvent[];
}

export interface Branding {
  logo_url: string;
  wordmark: string;
  video_url: string | null;
  primary_color: string;
  accent_color: string;
}

export const api = {
  login: (username: string, password: string) =>
    call<{ username: string; role: Role }>("POST", "/api/v1/auth/login", { username, password }),
  logout: () => call<{ ok: boolean }>("POST", "/api/v1/auth/logout"),
  me: () => call<Me>("GET", "/api/v1/me"),
  metrics: () => call<MetricsBody>("GET", "/api/v1/metrics"),
  search: (q: string, opts?: Partial<{ sources: string[]; entity_types: string[]; fuzzy: boolean; limit: number }>) =>
    call<SearchResponse>("POST", "/api/v1/search", { q, ...opts }),
  autocomplete: async (q: string): Promise<string[]> => {
    if (!q) return [];
    const r = await fetch(`/api/v1/search/autocomplete?q=${encodeURIComponent(q)}`, {
      credentials: "include",
    });
    if (!r.ok) return [];
    return (await r.json()) as string[];
  },
  cslSources: () =>
    call<{ known: { code: string; long_name: string; agency_url: string }[]; counts: { name: string; count: number }[] }>(
      "GET",
      "/api/v1/csl/sources",
    ),
  auditList: (limit = 50, offset = 0) =>
    call<AuditList>("GET", `/api/v1/audit?limit=${limit}&offset=${offset}`),
  auditGet: (id: string) => call<AuditEvent>("GET", `/api/v1/audit/${encodeURIComponent(id)}`),
  reviewQueue: () => call<ReviewQueue>("GET", "/api/v1/review"),
  reviewDecide: (auditId: string, decision: "cleared" | "blocked", note?: string) =>
    call<{ audit_id: string; decision: string; decided_by: string; decided_at: number }>(
      "POST",
      `/api/v1/review/${encodeURIComponent(auditId)}/decide`,
      { decision, note: note ?? null },
    ),
  cslRefresh: () =>
    call<{ ingested: number; fetched_at: number; version: string; source_path: string }>(
      "POST",
      "/api/v1/csl/refresh",
    ),
  branding: async (): Promise<Branding> => {
    const r = await fetch("/api/v1/branding", { credentials: "include" });
    if (!r.ok) {
      return {
        logo_url: "/brand/ocelot.svg",
        wordmark: "OcelAudit",
        video_url: null,
        primary_color: "#1f2937",
        accent_color: "#b45309",
      };
    }
    return (await r.json()) as Branding;
  },
};

export { ApiError };
