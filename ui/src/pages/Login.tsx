import { useState } from "preact/hooks";
import { api, ApiError, type Branding, type Role } from "../api";

interface Props {
  brand: Branding | null;
  onLogin: (u: { username: string; role: Role }) => void;
}

export function Login({ brand, onLogin }: Props) {
  const logo = brand?.logo_url ?? "/brand/ocelot.svg";
  const wordmark = brand?.wordmark ?? "OcelAudit";
  const video = brand?.video_url ?? null;
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function submit(e: Event) {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const u = await api.login(username, password);
      onLogin(u);
    } catch (e: unknown) {
      if (e instanceof ApiError) {
        setError(e.status === 401 ? "Invalid credentials." : e.message);
      } else {
        setError("Network error.");
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <main class="relative grid h-full place-items-center bg-ocelot-paper dark:bg-ocelot-ink">
      {video && (
        <video
          autoplay
          muted
          loop
          playsInline
          class="pointer-events-none absolute inset-0 h-full w-full object-cover opacity-30 dark:opacity-20"
        >
          <source src={video} />
        </video>
      )}
      <div class="relative w-full max-w-sm rounded-xl border border-neutral-200 bg-white/95 p-8 shadow-sm backdrop-blur dark:border-neutral-800 dark:bg-neutral-900/95">
        <header class="mb-6 flex items-center gap-3">
          <img src={logo} alt="" class="h-10 w-10 text-ocelot-mark dark:text-ocelot-paper" />
          <div>
            <h1 class="font-display text-xl">{wordmark}</h1>
            <p class="text-xs text-neutral-500">CSL screening · demo</p>
          </div>
        </header>
        <form onSubmit={submit} class="space-y-4">
          <label class="block text-sm">
            <span class="block font-medium">Username</span>
            <input
              type="text"
              autocomplete="username"
              required
              value={username}
              onInput={(e) => setUsername((e.currentTarget as HTMLInputElement).value)}
              class="mt-1 block w-full rounded border border-neutral-300 bg-white px-3 py-2 text-sm outline-none focus:border-ocelot-accent dark:border-neutral-700 dark:bg-neutral-800"
            />
          </label>
          <label class="block text-sm">
            <span class="block font-medium">Password</span>
            <input
              type="password"
              autocomplete="current-password"
              required
              value={password}
              onInput={(e) => setPassword((e.currentTarget as HTMLInputElement).value)}
              class="mt-1 block w-full rounded border border-neutral-300 bg-white px-3 py-2 text-sm outline-none focus:border-ocelot-accent dark:border-neutral-700 dark:bg-neutral-800"
            />
          </label>
          {error && (
            <p class="rounded border border-tlp-red/40 bg-tlp-red/10 px-3 py-2 text-sm text-tlp-red">
              {error}
            </p>
          )}
          <button
            type="submit"
            disabled={busy || !username || !password}
            class="w-full rounded bg-ocelot-mark px-3 py-2 text-sm font-semibold text-white transition disabled:opacity-50 dark:bg-ocelot-paper dark:text-ocelot-ink"
          >
            {busy ? "signing in…" : "Sign in"}
          </button>
        </form>
        <p class="mt-6 text-xs text-neutral-500">
          Demo credentials:{" "}
          <code class="rounded bg-neutral-100 px-1 dark:bg-neutral-800">admin / admin</code>{" "}
          ·{" "}
          <code class="rounded bg-neutral-100 px-1 dark:bg-neutral-800">compliance / compliance</code>
          . Both are fixed defaults — rotate before any real deployment.
        </p>
      </div>
    </main>
  );
}
