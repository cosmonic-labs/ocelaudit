// Tiny URL-driven router. Reads window.location.pathname; emits a
// `routechange` event when navigate() is called so listeners (a top-level
// useState) can re-render.
//
// We don't use tanstack-router or wouter — adding them would 4× the
// JS bundle. The whole demo has 4 pages, hand-rolled is fine.

import { useEffect, useState } from "preact/hooks";

const EVENT = "ocelaudit:routechange";

export function navigate(path: string) {
  if (window.location.pathname !== path) {
    window.history.pushState({}, "", path);
    window.dispatchEvent(new Event(EVENT));
  }
}

export function useRoute() {
  const [path, setPath] = useState(window.location.pathname);
  useEffect(() => {
    const update = () => setPath(window.location.pathname);
    window.addEventListener(EVENT, update);
    window.addEventListener("popstate", update);
    return () => {
      window.removeEventListener(EVENT, update);
      window.removeEventListener("popstate", update);
    };
  }, []);
  return path;
}

/** Read a query-string param without dragging in URLSearchParams polyfills. */
export function readQuery(name: string): string | null {
  return new URL(window.location.href).searchParams.get(name);
}
