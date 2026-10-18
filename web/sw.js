// Offline support. Pages are fetched network-first (so new deploys show up
// straight away) with the cached copy as a fallback. Trunk's build outputs have
// content hashes in their names, so they are cached forever, and the hashed
// files of older builds are pruned whenever a fresh page arrives.

const CACHE = "ringcode-v1";
const STATIC = ["./", "manifest.webmanifest", "icon.svg", "icons/icon-192.png", "icons/icon-512.png"];
const HASHED = /-[0-9a-f]{16}(_bg)?\.[a-z]+$/;

self.addEventListener("install", (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(CACHE);
      await cache.addAll(STATIC);
      // Also fetch the current build's assets so the first visit works offline.
      const page = await cache.match("./");
      if (page) await cache.addAll(assetsIn(await page.text()));
      await self.skipWaiting();
    })(),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      for (const key of await caches.keys()) {
        if (key !== CACHE) await caches.delete(key);
      }
      await self.clients.claim();
    })(),
  );
});

self.addEventListener("fetch", (event) => {
  const request = event.request;
  const url = new URL(request.url);
  if (request.method !== "GET" || url.origin !== self.location.origin) return;

  if (request.mode === "navigate") {
    event.respondWith(networkFirst(request, "./"));
  } else if (HASHED.test(url.pathname)) {
    event.respondWith(cacheFirst(request));
  } else {
    event.respondWith(networkFirst(request, request));
  }
});

/** Same-origin hashed asset URLs referenced by a page. */
function assetsIn(html) {
  const urls = [...html.matchAll(/(?:href|src)="([^"]+)"/g)]
    .map((m) => new URL(m[1], self.registration.scope))
    .filter((u) => u.origin === self.location.origin && HASHED.test(u.pathname));
  return [...new Set(urls.map((u) => u.href))];
}

async function cacheFirst(request) {
  const cache = await caches.open(CACHE);
  const hit = await cache.match(request);
  if (hit) return hit;
  const response = await fetch(request);
  if (response.ok) cache.put(request, response.clone());
  return response;
}

/** Fetches from the network, storing the response under `key`; falls back to the cache. */
async function networkFirst(request, key) {
  const cache = await caches.open(CACHE);
  try {
    const response = await fetch(request);
    if (response.ok) {
      await cache.put(key, response.clone());
      if (key === "./") prune(cache, await response.clone().text());
    }
    return response;
  } catch (err) {
    const hit = await cache.match(key);
    if (hit) return hit;
    throw err;
  }
}

/** Drops cached hashed assets that the current page no longer uses. */
async function prune(cache, html) {
  const keep = new Set(assetsIn(html));
  for (const request of await cache.keys()) {
    if (HASHED.test(new URL(request.url).pathname) && !keep.has(request.url)) {
      await cache.delete(request);
    }
  }
}
