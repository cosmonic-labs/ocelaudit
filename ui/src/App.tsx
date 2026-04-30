import { useEffect, useState } from "preact/hooks";
import { api, ApiError, type Me } from "./api";
import { Login } from "./pages/Login";
import { Dashboard } from "./pages/Dashboard";

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
    <Dashboard
      me={me}
      onLogout={async () => {
        try {
          await api.logout();
        } finally {
          setMe(null);
        }
      }}
    />
  );
}
