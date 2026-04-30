import { useEffect, useState } from "preact/hooks";
import { api, ApiError, type Branding, type Me } from "./api";
import { Login } from "./pages/Login";
import { Dashboard } from "./pages/Dashboard";
import { SearchPage } from "./pages/Search";
import { AuditPage } from "./pages/Audit";
import { ReviewPage } from "./pages/Review";
import { AdminPage } from "./pages/Admin";
import { CslStatusPage } from "./pages/CslStatus";
import { navigate, useRoute } from "./router";

export function App() {
  const [me, setMe] = useState<Me | null>(null);
  const [brand, setBrand] = useState<Branding | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    api.branding().then(setBrand);
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

  useEffect(() => {
    if (brand) {
      const root = document.documentElement;
      root.style.setProperty("--brand-primary", brand.primary_color);
      root.style.setProperty("--brand-accent", brand.accent_color);
      document.title = brand.wordmark;
    }
  }, [brand]);

  const path = useRoute();

  if (loading) {
    return (
      <main class="grid h-full place-items-center text-sm text-neutral-500">
        loading…
      </main>
    );
  }

  if (!me) {
    return (
      <Login
        brand={brand}
        onLogin={(u) => setMe({ username: u.username, role: u.role, iat: Date.now() / 1000 })}
      />
    );
  }

  return (
    <Shell
      me={me}
      brand={brand}
      onLogout={async () => {
        try {
          await api.logout();
        } finally {
          setMe(null);
        }
      }}
    >
      {renderPage(path, me)}
    </Shell>
  );
}

function NavLink({ href, label }: { href: string; label: string }) {
  return (
    <a
      href={href}
      class="text-neutral-600 hover:text-ocelot-accent dark:text-neutral-300"
      onClick={(e) => {
        e.preventDefault();
        navigate(href);
      }}
    >
      {label}
    </a>
  );
}

function renderPage(path: string, me: Me) {
  if (path.startsWith("/search")) return <SearchPage />;
  if (path.startsWith("/audit")) return <AuditPage />;
  if (path.startsWith("/review")) return <ReviewPage />;
  if (path.startsWith("/admin")) return <AdminPage me={me} />;
  if (path.startsWith("/csl/status") || path.startsWith("/csl")) return <CslStatusPage />;
  return <Dashboard me={me} />;
}

interface ShellProps {
  me: Me;
  brand: Branding | null;
  onLogout: () => void;
  children: preact.ComponentChildren;
}

function Shell({ me, brand, onLogout, children }: ShellProps) {
  const logo = brand?.logo_url ?? "/brand/ocelot.svg";
  const wordmark = brand?.wordmark ?? "OcelAudit";
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
            <img src={logo} alt="" class="h-8 w-8 text-ocelot-mark dark:text-ocelot-paper" />
            <span class="font-display text-lg">{wordmark}</span>
          </a>
          <nav class="flex items-center gap-4 text-sm">
            <NavLink href="/search" label="Search" />
            <NavLink href="/audit" label="Audit" />
            <NavLink href="/review" label="Review" />
            {me.role === "admin" && <NavLink href="/admin" label="Admin" />}
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
