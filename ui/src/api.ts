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

export const api = {
  login: (username: string, password: string) =>
    call<{ username: string; role: Role }>("POST", "/api/v1/auth/login", { username, password }),
  logout: () => call<{ ok: boolean }>("POST", "/api/v1/auth/logout"),
  me: () => call<Me>("GET", "/api/v1/me"),
  metrics: () => call<MetricsBody>("GET", "/api/v1/metrics"),
  search: (q: string, opts?: Partial<{ sources: string[]; entity_types: string[]; fuzzy: boolean; limit: number }>) =>
    call<SearchResponse>("POST", "/api/v1/search", { q, ...opts }),
};

export { ApiError };
