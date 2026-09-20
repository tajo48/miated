"use strict";

/* Offline support for miated: the core assets are precached so the app works
   with no network, navigations are network-first so a newly deployed version
   shows up on the first reload (cache only as a fallback), and Google-Fonts
   requests are cached as they happen. All paths are relative, so the worker
   works at any URL depth. */

/* @@VERSION@@ is stamped at build time — GitHub Actions replaces it with the
   commit sha, and the local dev server with a hash of the served assets — so
   every deploy/rebuild produces a fresh cache and activate cleans up the old. */
const VERSION = "@@VERSION@@";
const CORE = "miated-core-" + VERSION;
const FONTS = "miated-fonts-" + VERSION;

const CORE_ASSETS = [
  "./",
  "./index.html",
  "./manifest.json",
  "./favicon.svg",
  "./favicon-32.png",
  "./apple-touch-icon.png",
  "./icon-192.png",
  "./icon-512.png",
  "./icon-192-maskable.png",
  "./icon-512-maskable.png",
];

self.addEventListener("install", (e) => {
  e.waitUntil(
    caches.open(CORE).then((cache) => cache.addAll(CORE_ASSETS)).then(() => self.skipWaiting())
  );
});

self.addEventListener("activate", (e) => {
  e.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(keys.filter((k) => k !== CORE && k !== FONTS).map((k) => caches.delete(k)))
      )
      .then(() => self.clients.claim())
  );
});

async function staleWhileRevalidate(request, cacheName) {
  const cache = await caches.open(cacheName);
  const cached = await cache.match(request, { ignoreSearch: true });
  const refreshed = fetch(request)
    .then((res) => {
      // "opaque" (no-cors) responses have status 0 but are fine to cache.
      if (res.ok || res.type === "opaque") cache.put(request, res.clone());
      return res;
    })
    .catch(() => null);
  return cached || (await refreshed) || Response.error();
}

self.addEventListener("fetch", (e) => {
  if (e.request.method !== "GET") return;
  const url = new URL(e.request.url);

  // App shell: network-first, cache only when offline.
  if (e.request.mode === "navigate") {
    e.respondWith(
      (async () => {
        const cache = await caches.open(CORE);
        try {
          const res = await fetch(e.request);
          if (res.ok) cache.put("./index.html", res.clone());
          return res;
        } catch {
          return (
            (await cache.match("./index.html", { ignoreSearch: true })) ||
            (await cache.match("./", { ignoreSearch: true })) ||
            new Response("offline", { status: 503, headers: { "Content-Type": "text/plain" } })
          );
        }
      })()
    );
    return;
  }

  // Same-origin assets: cache-first, then network + cache fill-in.
  if (url.origin === self.location.origin) {
    e.respondWith(
      (async () => {
        const cache = await caches.open(CORE);
        const cached = await cache.match(e.request, { ignoreSearch: true });
        if (cached) return cached;
        try {
          const res = await fetch(e.request);
          if (res.ok) cache.put(e.request, res.clone());
          return res;
        } catch {
          return new Response("offline", { status: 503, headers: { "Content-Type": "text/plain" } });
        }
      })()
    );
    return;
  }

  // Fonts: stale-while-revalidate so the UI keeps its Bebas Neue offline.
  if (url.hostname === "fonts.googleapis.com" || url.hostname === "fonts.gstatic.com") {
    e.respondWith(staleWhileRevalidate(e.request, FONTS));
  }
});
