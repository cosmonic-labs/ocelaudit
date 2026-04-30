import { useEffect, useState } from "preact/hooks";
import { api, ApiError, type Me } from "./api";
import { Login } from "./pages/Login";
import { Dashboard } from "./pages/Dashboard";
import { SearchPage } from "./pages/Search";
import { navigate, useRoute } from "./router";

export function App() {
  const [me, setMe] = useState<Me | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    api
      .me()
      .then((m) => setMe(m))
      .catch((e) => {
        if (!(e instanceof ApiError) || e.status !== 401) {
          console.error("me() failed:", e);
        }
        setMe(null);
      })
      .finally(() => setLoading(false));
  }, []);

  const path = useRoute();

  if (loading) {
    return (
      <main class="grid h-full place-items-center text-sm text-neutral-500">
        loading…
      </main>
    );
  }

  if (!me) {
    return <Login onLogin={(u) => setMe({ username: u.username, role: u.role, iat: Date.now() / 1000 })} />;
  }

  return (
    <Shell
      me={me}
      onLogout={async () => {
        try {
          await api.logout();
        } finally {
          setMe(null);
        }
      }}
    >
      {path.startsWith("/search") ? (
        <SearchPage />
      ) : (
        <Dashboard me={me} />
      )}
    </Shell>
  );
}

interface ShellProps {
  me: Me;
  onLogout: () => void;
  children: preact.ComponentChildren;
}

function Shell({ me, onLogout, children }: ShellProps) {
  return (
    <div class="min-h-full">
      <header class="border-b border-neutral-200 bg-white dark:border-neutral-800 dark:bg-neutral-900">
        <div class="mx-auto flex max-w-5xl items-center justify-between px-4 py-3">
          <a
            href="/"
            class="flex items-center gap-2"
            onClick={(e) => {
              e.preventDefault();
              navigate("/");
            }}
          >
            <img src="/brand/ocelot.svg" alt="" class="h-8 w-8 text-ocelot-mark dark:text-ocelot-paper" />
            <span class="font-display text-lg">OcelAudit</span>
          </a>
          <nav class="flex items-center gap-4 text-sm">
            <a
              href="/search"
              class="text-neutral-600 hover:text-ocelot-accent dark:text-neutral-300"
              onClick={(e) => {
                e.preventDefault();
                navigate("/search");
              }}
            >
              Search
            </a>
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
          </nav>
        </div>
      </header>
      <main class="mx-auto max-w-5xl px-4 py-8">{children}</main>
    </div>
  );
}
