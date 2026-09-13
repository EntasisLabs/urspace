# Cloudflare Worker edge

This deploys only Urspace's public bootstrap assets to Cloudflare's edge. It does
not proxy a hosted app through Cloudflare: after the bootstrap loads, the browser
opens the encrypted Iroh connection to the machine running `urspace serve`.

The Worker validates key-shaped site hostnames, exposes only the bootstrap paths
required by the protocol, and applies the same hardened response headers as the
self-hosted Rust bootstrap server. Direct invites remain in URL fragments.

It also serves the optional `u.urspace.online` short-link resolver. The CLI
encrypts each signed invite locally; a SQLite-backed Durable Object stores only
the bounded ciphertext, nonce, and expiry under a key derived from the fragment
seed. Create and read requests are independently rate limited, records expire
after at most seven days, and alarms erase expired storage.

## One-time Cloudflare setup

1. In the `urspace.online` DNS zone, create a proxied record:
   - Type: `A`
   - Name: `*`
   - IPv4 address: `192.0.2.1`
   - Proxy status: **Proxied**
2. Authenticate Wrangler from this directory with `npx wrangler login`.

The documentation-only address is an originless placeholder. Requests matching
the Worker route are served by the Worker and its static asset binding; no VM,
open inbound port, tunnel daemon, or origin web server is involved.

## Verify and deploy

```bash
cd deploy/cloudflare-worker
npm install
npm run check
npm run deploy
```

After deployment:

```bash
curl -i https://<site-id>.urspace.online/healthz
cargo run -p urspace-host --bin urspace -- serve http://127.0.0.1:8787
```

Open the URL printed by the CLI. A valid site URL should load the bootstrap from
Cloudflare and then connect directly to the CLI's Iroh endpoint.

To exercise the encrypted resolver after deployment:

```bash
urspace serve localhost:8787 --short
```

## Security boundary

Cloudflare serves public bootstrap code and sees the site public key in a direct
invite's hostname. For short links it sees the caller IP, timing, derived lookup
id, ciphertext size, nonce, and expiry. It never receives the fragment seed,
plaintext invite, or capability. The browser authenticates and decrypts the
envelope, constrains its destination to the matching Urspace base domain, and
then performs the existing signed-invite verification before opening Iroh.
Cloudflare remains a replaceable rendezvous, not the app-data transport or
authority.

For operators who want no Cloudflare dependency, the hardened Rust bootstrap
server and container remain available in `crates/urspace-bootstrap`.
