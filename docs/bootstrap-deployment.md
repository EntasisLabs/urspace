# Deploy the wildcard bootstrap

The bootstrap is public, generic, and contains no site address or capability.
Its job is to deliver a small audited browser client at a unique origin for each
Iroh site identity.

This component is security-sensitive: whoever can change its JavaScript or WASM
can read future invitation fragments. Deploy a reviewed source revision, record
the resulting Worker version or container digest, tightly limit deployment
credentials, and keep the edge configuration as small as possible.

## Domain and TLS contract

The production Urspace base domain is `urspace.online`. Each invitation uses
exactly one identity label beneath it:

```text
https://<iroh-public-key>.urspace.online/.urspace/open/#u4=<signed-invite>
```

For the default Urspace deployment, Cloudflare provisions wildcard TLS and runs
the bootstrap Worker directly at the edge. Create a proxied wildcard DNS record
and follow [`deploy/cloudflare-worker/README.md`](../deploy/cloudflare-worker/README.md).

For a fully self-hosted edge, provision:

1. Wildcard DNS for `*.urspace.online` pointing at the HTTPS edge.
2. A certificate valid for `*.urspace.online`.
3. A reverse proxy that preserves the original `Host` header and forwards every
   wildcard host to the bootstrap container over a private network.

A wildcard certificate covers the one-label site origins. An exact DNS record
for the bare `urspace.online` name takes precedence and can host a normal product
site independently.

The URL fragment is never included in an HTTP request, reverse-proxy log, or TLS
request path. The site identity in the hostname is public; the fragment is the
secret.

The optional `u.urspace.online` resolver uses the same rule. Its fragment holds
a short-link seed; the Worker receives only a derived lookup id and an encrypted
invite envelope. The resolver requires a SQLite-backed Durable Object binding
and is intentionally not implemented by the static self-hosted container.

## Build and run

Cloudflare users do not need this container or a VM. The Worker deployment above
serves the same built assets without an origin server or tunnel.

For a self-hosted edge, from the repository root:

```bash
docker build -f deploy/bootstrap/Dockerfile -t urspace-bootstrap .
docker run --read-only --cap-drop=ALL \
  --publish 127.0.0.1:8080:8080 \
  urspace-bootstrap \
  --base-domain urspace.online
```

Terminate TLS at the reverse proxy and forward to `127.0.0.1:8080`, or place the
container on a private container network without publishing it publicly. Do not
expose the plaintext listener directly to the Internet.

The server:

- accepts content requests only for a canonical Iroh public-key subdomain;
- serves only the seven allow-listed bootstrap assets;
- rejects traversal, unknown paths, and non-GET/HEAD methods;
- marks all responses `no-store` and sends CSP, HSTS, nosniff, referrer,
  opener/resource isolation, and permissions-policy headers;
- exposes `GET /healthz` for infrastructure health checks;
- does not emit per-request logs.

## Verify the edge

Generate a real Iroh public key using `urspace-host`, then verify through
the public edge:

```bash
curl --fail --show-error --head \
  https://<site-id>.urspace.online/.urspace/open/
curl --fail --show-error \
  https://<site-id>.urspace.online/healthz
```

Confirm the first response is HTML, is not cacheable, includes the security
headers, and that an invalid hostname receives HTTP 421. Also verify the WASM
response uses `application/wasm` and `/sw.js` includes
`Service-Worker-Allowed: /`.

## Mint remote BoxClub invites

Run BoxClub and its Iroh proxy on the private host, but use the exact public HTTPS
origin when minting:

```bash
cd /path/to/urspace
cargo run -p urspace-host --bin urspace -- serve localhost:8787 \
  --name boxclub \
  --ttl 1h \
  --max-sessions 4
```

The recipient loads only the generic bootstrap from the HTTPS edge. Cloudflare
does not proxy BoxClub or the Iroh stream. The bootstrap validates the signature,
URL origin, Iroh identity, endpoint ticket, expiry, and capability locally before
it dials the BoxClub host over Iroh.

## Operational rules

- Roll out bootstrap changes deliberately and retain the exact source commit and
  image digest used for every deployment.
- Change the `BOOTSTRAP_REVISION` value in `apps/bootstrap/public/sw.js` whenever
  a worker security header or behavior changes. Browsers compare worker script
  bytes during updates; changing only an HTTP header does not replace an already
  installed worker. The bootstrap explicitly checks for an update before arming.
- Never add analytics, third-party scripts, remote fonts, or tag managers.
- Never accept a plaintext invitation or capability through a query parameter,
  request body, cookie, or server-side redirect. The short-link API accepts only
  bounded authenticated ciphertext; only a browser-local fragment can decrypt it.
- Do not terminate the Iroh connection at the bootstrap server. It is a static
  code origin, not a relay or application proxy.
- Rotate deployment credentials after suspected compromise. Previously minted
  capability signatures remain cryptographically valid, but recipients must not
  trust a bootstrap origin that may serve altered code.
