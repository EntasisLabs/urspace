# Medousa Sites

Capability-addressed web apps served from a private machine over Iroh. The
browser receives a signed, expiring invite URL; a generic bootstrap verifies it,
connects to the named Iroh endpoint, and exposes the app at an isolated origin.

Protocol knowledge is not an authorization factor. Access requires the random
256-bit capability in the URL fragment, and the endpoint key signs every field
that decides where and how the browser connects.

## What works

- Static directories with confined GET/HEAD access
- Loopback HTTP apps with GET, HEAD, POST, PUT, PATCH, DELETE, and OPTIONS
- Same-origin browser `fetch` calls, including request bodies and response headers
- Same-origin WebSockets through an injected standards-shaped browser shim
- Vite/React bundles, client-side routes, and page-defined WebMCP tools
- Expiring, revocable, session-limited invites

The loopback proxy is intentionally limited to `http://127.0.0.1`,
`http://[::1]`, or `http://localhost`. It will not proxy to LAN or Internet
origins and does not follow upstream redirects.

## BoxClub / StreetClanker

BoxClub is the first full-app acceptance target. Run its existing all-in-one
production server; no BoxClub source changes are required.

For the local end-to-end setup, one command builds and runs BoxClub, the browser
bootstrap, and the Iroh proxy until Ctrl+C:

```bash
./scripts/dev-boxclub.sh /Users/theelevators/boxclub/BoxClub
```

The equivalent three-terminal setup is useful when debugging:

```bash
cd /Users/theelevators/boxclub/BoxClub
npm run build
NODE_ENV=production PORT=8787 npm start
```

Build and serve the generic bootstrap locally in another terminal:

```bash
cd /Users/theelevators/medousa/medousa-sites/apps/bootstrap
npm run build
npm run serve
```

Then mint the invite and keep the proxy running:

```bash
cd /Users/theelevators/medousa/medousa-sites
cargo run -p medousa-site-host -- proxy \
  --upstream http://127.0.0.1:8787 \
  --bootstrap-origin http://localhost:8080 \
  --ttl-seconds 3600 \
  --max-sessions 4
```

Open the emitted URL in a browser. The capability fragment is removed before
BoxClub code runs. Its frontend chunks, `/api/*` requests, `/ws` connection, and
WebMCP HTTP fallback all traverse the authenticated Iroh connection.

`localhost` is a same-machine development bootstrap. To share an invite with
another device, deploy `apps/bootstrap/public` as immutable static files behind
an HTTPS wildcard origin, then mint with that exact origin. For example, a
bootstrap at `https://sites.example` must route
`https://<site-id>.sites.example/.medousa/open/` and serve a wildcard TLS
certificate. The bootstrap is generic and never receives the fragment over
HTTP; fragments stay client-side.

## Static sites

```bash
cargo run -p medousa-site-host -- serve ./public \
  --bootstrap-origin https://sites.example \
  --entry-path /index.html
```

The native diagnostic client uses the same invite verification and transport:

```bash
cargo run -p medousa-site-host -- get '<invite-url>' /index.html
```

## Browser boundary

The signed URL uses the reserved `/.medousa/open/` bootstrap path. A root-scoped
service worker holds the authenticated Iroh client in memory and maps ordinary
same-origin requests onto independent QUIC streams. It injects only the small
WebSocket compatibility shim into HTML responses; application scripts otherwise
run unchanged at the capability-derived site origin.

If the browser terminates the worker, the connection fails closed. The invite
is not persisted to IndexedDB, Cache Storage, cookies, or local storage; reopen
the original invite to reconnect.

See [SECURITY.md](SECURITY.md) before exposing a non-development bootstrap.
