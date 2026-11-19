// Exchange Service Worker
// Cache strategy:
//   - Static assets (JS, CSS, fonts, images): cache-first
//   - API calls (/api/, /admin/, /ws): network-first with cache fallback
//   - Navigation requests: network-first, offline fallback HTML on failure

const CACHE_VERSION = 'exchange-v1';
const STATIC_CACHE  = `${CACHE_VERSION}-static`;
const API_CACHE     = `${CACHE_VERSION}-api`;

// Minimal shell to pre-cache on install
const PRECACHE_URLS = ['/', '/index.html'];

// ---- Install: pre-cache the app shell ----
self.addEventListener('install', (event) => {
  event.waitUntil(
    caches.open(STATIC_CACHE).then((cache) =>
      cache.addAll(PRECACHE_URLS)
    ).then(() => self.skipWaiting())
  );
});

// ---- Activate: remove stale caches ----
self.addEventListener('activate', (event) => {
  const validCaches = [STATIC_CACHE, API_CACHE];
  event.waitUntil(
    caches.keys().then((keys) =>
      Promise.all(
        keys
          .filter((key) => !validCaches.includes(key))
          .map((key) => caches.delete(key))
      )
    ).then(() => self.clients.claim())
  );
});

// ---- Fetch: route to the right strategy ----
self.addEventListener('fetch', (event) => {
  const { request } = event;
  const url = new URL(request.url);

  // Only handle same-origin and known CDN requests
  if (url.origin !== self.location.origin &&
      !url.hostname.endsWith('fonts.googleapis.com') &&
      !url.hostname.endsWith('fonts.gstatic.com')) {
    return;
  }

  // API, admin, WS: network-first
  if (url.pathname.startsWith('/api/') ||
      url.pathname.startsWith('/admin/') ||
      url.pathname.startsWith('/ws')) {
    event.respondWith(networkFirstWithCache(request, API_CACHE));
    return;
  }

  // Static assets (hashed filenames, fonts, images): cache-first
  if (isStaticAsset(url.pathname)) {
    event.respondWith(cacheFirstWithNetwork(request, STATIC_CACHE));
    return;
  }

  // Navigation requests (HTML): network-first, offline fallback
  if (request.mode === 'navigate') {
    event.respondWith(
      networkFirstWithCache(request, STATIC_CACHE).catch(() =>
        caches.match('/index.html').then((cached) =>
          cached ?? offlineFallbackResponse()
        )
      )
    );
    return;
  }

  // Default: network-first
  event.respondWith(networkFirstWithCache(request, STATIC_CACHE));
});

// ---- Push notifications ----
self.addEventListener('push', (event) => {
  const data = event.data ? event.data.json() : {};
  const title   = data.title   ?? 'Exchange';
  const options = {
    body:    data.body    ?? '',
    icon:    data.icon    ?? '/icons/icon-192.png',
    badge:   data.badge   ?? '/icons/icon-192.png',
    tag:     data.tag     ?? 'exchange-push',
    data:    data.data    ?? {},
    vibrate: [200, 100, 200],
  };

  event.waitUntil(self.registration.showNotification(title, options));
});

self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  const targetUrl = event.notification.data?.url ?? '/';
  event.waitUntil(
    clients.matchAll({ type: 'window' }).then((windowClients) => {
      for (const client of windowClients) {
        if (client.url === targetUrl && 'focus' in client) {
          return client.focus();
        }
      }
      return clients.openWindow(targetUrl);
    })
  );
});

// ---- Helpers ----

function isStaticAsset(pathname) {
  return (
    /\.(js|css|woff2?|ttf|otf|eot|png|jpg|jpeg|gif|svg|ico|webp|avif)(\?|$)/.test(pathname)
  );
}

async function cacheFirstWithNetwork(request, cacheName) {
  const cached = await caches.match(request);
  if (cached) return cached;

  const response = await fetch(request);
  if (response.ok) {
    const cache = await caches.open(cacheName);
    cache.put(request, response.clone());
  }
  return response;
}

async function networkFirstWithCache(request, cacheName) {
  try {
    const response = await fetch(request);
    if (response.ok && request.method === 'GET') {
      const cache = await caches.open(cacheName);
      cache.put(request, response.clone());
    }
    return response;
  } catch {
    const cached = await caches.match(request);
    if (cached) return cached;
    throw new Error('Network unavailable and no cached response');
  }
}

function offlineFallbackResponse() {
  const html = `<!doctype html>
<html lang="en">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <meta name="theme-color" content="#0b0e11" />
  <title>Exchange — Offline</title>
  <style>
    body {
      margin: 0;
      background: #0b0e11;
      color: #eaecef;
      font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif;
      display: flex;
      align-items: center;
      justify-content: center;
      min-height: 100vh;
      text-align: center;
      padding: 24px;
    }
    .card {
      max-width: 400px;
    }
    h1 {
      font-size: 1.5rem;
      margin-bottom: 12px;
      color: #f0b90b;
    }
    p {
      color: #848e9c;
      line-height: 1.6;
    }
    button {
      margin-top: 24px;
      padding: 10px 24px;
      background: #2ee5a6;
      color: #0b0e11;
      border: none;
      border-radius: 8px;
      font-size: 0.875rem;
      font-weight: 600;
      cursor: pointer;
    }
  </style>
</head>
<body>
  <div class="card">
    <h1>You are offline</h1>
    <p>Exchange requires a network connection for live market data and order execution. Please check your connection and try again.</p>
    <button onclick="window.location.reload()">Retry</button>
  </div>
</body>
</html>`;

  return new Response(html, {
    status: 200,
    headers: { 'Content-Type': 'text/html; charset=utf-8' },
  });
}
