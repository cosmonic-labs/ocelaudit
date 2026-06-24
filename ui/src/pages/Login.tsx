import { useState } from "preact/hooks";
import { api, ApiError, type Branding, type Role } from "../api";

interface Props {
  brand: Branding | null;
  onLogin: (u: { username: string; role: Role }) => void;
}

// Hero assets for the login background. The poster paints instantly so
// users (especially on slow/mobile links) never see a black box while
// the MP4 buffers; the video then takes over and loops muted in place.
const HERO_POSTER = "/brand/ocelaudit-sun-skyscraper-poster.png";
const HERO_VIDEO = "/video/OcelAudit-background-vid-720-4mbps.mp4";

export function Login({ brand, onLogin }: Props) {
  const logo = brand?.logo_url ?? "/brand/ocelot.svg";
  const wordmark = brand?.wordmark ?? "OcelAudit";
  const video = brand?.video_url ?? HERO_VIDEO;
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // The hero poster + video aren't always present (the embedded desktop build
  // ships the SPA without the heavy media). Track load failures so we fall back
  // to a simple gradient background instead of a broken/black box.
  const [posterOk, setPosterOk] = useState(true);
  const [videoOk, setVideoOk] = useState(true);

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
    <main class="relative grid h-full place-items-center overflow-hidden bg-ocelot-ink">
      {/* Simple gradient background — always present. The poster + video are
          progressive enhancement layered on top; when they're absent (e.g. the
          embedded desktop build ships no media) this is what shows. */}
      <div
        aria-hidden="true"
        class="pointer-events-none absolute inset-0"
        style={{ background: "linear-gradient(135deg,#0b1220 0%,#172a4d 55%,#c2570d 138%)" }}
      />
      {/* Poster paints first when available; hidden if it fails to load. */}
      {posterOk && (
        <img
          aria-hidden="true"
          src={HERO_POSTER}
          onError={() => setPosterOk(false)}
          class="pointer-events-none absolute inset-0 h-full w-full object-cover"
        />
      )}
      {video && videoOk && (
        <video
          aria-hidden="true"
          autoplay
          muted
          loop
          playsInline
          preload="auto"
          poster={posterOk ? HERO_POSTER : undefined}
          onError={() => setVideoOk(false)}
          class="pointer-events-none absolute inset-0 h-full w-full object-cover"
        >
          <source src={video} type="video/mp4" />
        </video>
      )}
      {/* CSS dim overlay so the form stays readable without re-encoding the video. */}
      <div aria-hidden="true" class="pointer-events-none absolute inset-0 bg-black/55" />
      <div class="relative z-10 w-full max-w-sm rounded-xl border border-neutral-200 bg-white/95 p-8 shadow-sm backdrop-blur dark:border-neutral-800 dark:bg-neutral-900/95">
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
          <code class="rounded bg-neutral-100 px-1 dark:bg-neutral-800">admin / OcelAudit</code>{" "}
          ·{" "}
          <code class="rounded bg-neutral-100 px-1 dark:bg-neutral-800">compliance / OcelAudit</code>
          . Both are fixed defaults — rotate before any real deployment.
        </p>
      </div>
    </main>
  );
}
