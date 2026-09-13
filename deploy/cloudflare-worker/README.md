# Cloudflare Worker edge

This deploys only Urspace's public bootstrap assets to Cloudflare's edge. It does
not proxy a hosted app through Cloudflare: after the bootstrap loads, the browser
opens the encrypted Iroh connection to the machine running `urspace serve`.

The Worker validates the key-shaped hostname, exposes only the seven bootstrap
paths required by the protocol, and applies the same hardened response headers as
the self-hosted Rust bootstrap server. The invite remains in the URL fragment, so
it is not sent to Cloudflare in an HTTP request.

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
curl -i https://urspace.online/healthz
cargo run -p urspace-host --bin urspace -- serve http://127.0.0.1:8787
```

Open the URL printed by the CLI. A valid site URL should load the bootstrap from
Cloudflare and then connect directly to the CLI's Iroh endpoint.

## Security boundary

Cloudflare serves public, immutable bootstrap code and sees the site public key
in the hostname. It does not receive the invite secret, because URL fragments are
never included in HTTP requests. The browser still verifies the signed invite,
expiry, audience, and site key before opening the Iroh protocol. Cloudflare is
therefore a replaceable rendezvous for code delivery, not the app-data transport
or authority.

For operators who want no Cloudflare dependency, the hardened Rust bootstrap
server and container remain available in `crates/urspace-bootstrap`.
