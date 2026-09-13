# Security model

Protocol knowledge is assumed public. An attacker may know every field, state
transition, endpoint, and implementation detail described here.

## Reporting a vulnerability

Do not open a public issue for a suspected vulnerability or include invitation
URLs, capabilities, private keys, or deployment credentials in a report. Use
[GitHub private vulnerability reporting](https://github.com/EntasisLabs/urspace/security/advisories/new)
so the maintainers can investigate before public disclosure.

Security fixes are provided for the latest release. Older preview releases may
be used to confirm regressions but should not be assumed to receive patches.

## Protected assets

- Site endpoint private key
- Unexpired invitation capabilities
- Unexpired short-link fragment seeds
- Files below the explicitly shared site root
- Integrity of the endpoint identity, route, bootstrap origin, and grant limits

## Invitation invariants

The Iroh endpoint private key signs a deterministic invitation payload. Clients
reject the invitation unless all of the following hold:

1. The signature verifies under the embedded public key.
2. The z-base-32 site id is derived from that public key.
3. The endpoint ticket names that same public key.
4. The actual URL uses the site-specific subdomain of the signed bootstrap
   origin, including its scheme and port.
5. The invitation is unexpired and its entry path is structurally valid.

The host independently checks expiry and capability possession. Client-side
validation is defense in depth, not the authorization boundary.

## Capability handling

- Capabilities contain 256 random bits.
- The host retains only a BLAKE3 digest and compares candidate digests in
  constant time.
- Each capability has an expiry and maximum connection-session count.
- Rotating an invitation closes it to new browser identities without removing
  previously admitted identities; kicking an identity removes its admission and
  closes its current Iroh connection. A kick also rotates the current bearer URL
  so reopening the known URL under a new ephemeral endpoint cannot regain access.
- Revocation is consulted for every request, including requests on an already
  authorized connection.
- Browser code removes the fragment before remote HTML runs and zeroes mutable
  Rust copies after authorization. JavaScript strings cannot be reliably erased,
  so the bootstrap also drops its reference as soon as the WASM call returns.
- The service worker retains the live client only in memory. It never writes the
  invitation or capability into browser persistence.
- Synthetic site responses are marked `Cache-Control: no-store`; neither the
  capability nor proxied site content is intentionally persisted by the bootstrap.

## Encrypted short links

Shortening is explicit operator opt-in. The CLI generates a separate random
256-bit seed, derives a 128-bit opaque lookup id and AES-256-GCM key using
HKDF-SHA-256, and encrypts the complete signed invitation locally. Only the
lookup id, authenticated ciphertext, random nonce, and expiry are sent to the
short-link service. The seed stays in the URL fragment and is not included in
HTTP requests.

The browser removes the seed fragment before retrieval, derives the same lookup
id and key, authenticates and decrypts the envelope, and accepts only a
capability-derived Urspace origin beneath the same base domain. The existing
signed-invitation verification then runs unchanged. Modified ciphertext, nonce,
expiry, or destination fails closed.

Encrypted records are size bounded, creation-rate limited, stored in isolated
SQLite-backed Durable Objects, and deleted by an expiry alarm. The service can
observe IP addresses, timing, lookup ids, and ciphertext sizes. A compromised
service can delete or withhold records, but cannot recover a capability or forge
a valid replacement. Direct invitation URLs remain available through the
operator console and do not depend on this service.

## Loopback proxy boundary

- `urspace serve` canonicalizes shorthand such as `localhost:8787` before Iroh
  starts. Its parser rejects remote hosts, credentials, paths, queries, and
  fragments rather than passing ambiguous input into the proxy.
- Named and source-derived site identities persist locally, but every run mints
  a fresh 256-bit capability and invite ID. Treat the printed URL as a bearer
  secret until it expires.
- Dynamic apps must be explicitly exposed as an HTTP loopback origin. HTTPS,
  LAN, Internet, credential-bearing, and path-bearing upstream URLs are rejected.
- `localhost` is normalized to the numeric `127.0.0.1` address before requests,
  avoiding ambient DNS resolution in the proxy boundary.
- The upstream client does not follow redirects, preventing the local app from
  redirecting the proxy into a different network authority.
- Browser-controlled `Host`, connection, upgrade, transfer-encoding, and content
  length headers are not forwarded.
- Request bodies are limited to 16 MiB, response bodies to 64 MiB, and protocol
  frames to 1 MiB. These are availability limits, not content trust decisions.
- WebSocket upgrades are made only against the configured loopback authority.
- Expiry and revocation are rechecked once per second on live WebSocket tunnels;
  a withdrawn grant closes the browser and upstream sides with policy code 1008.

## Current limitations requiring hardening

- The bootstrap origin is trusted executable code. A compromised deployment can
  read invitation fragments before they are cleared. Production requires pinned,
  reproducible loader artifacts and tightly controlled deployment credentials.
- The production bootstrap validates that every content request uses exactly one
  canonical Iroh public-key label under its configured base domain and serves
  only an explicit asset allow-list. This reduces accidental hosting exposure;
  it does not make altered bootstrap code trustworthy.
- Static path confinement currently canonicalizes and then opens the file. This
  rejects traversal and ordinary symlink escapes but is not yet safe against a
  malicious local writer racing path resolution. Replace it with capability-based
  directory handles before serving attacker-writable trees.
- Capability state is memory-only. Restarting the host invalidates every invite
  (fail closed). Live rotation and revocation are available only through the
  foreground host console; there is not yet an authenticated management socket.
- The browser and loopback proxy buffer each HTTP response up to 64 MiB. Streaming
  responses, SSE, uploads larger than 16 MiB, and stricter content-specific
  limits are not yet supported.
- Browser Iroh endpoints are relay-only under current Web platform constraints.
- A short link adds a centralized first-open availability dependency. If the
  resolver is unavailable, recipients need the direct invitation printed by the
  `raw` console command. Application traffic never traverses the resolver.
- Browsers may terminate an idle service worker. This intentionally loses the
  in-memory capability and requires reopening the invite. An active BoxClub
  WebSocket keeps the worker operation alive in tested browsers, but lifecycle
  behavior must be tested across target browsers.
- The WebSocket shim currently supports text/binary frames and close codes, but
  not WebSocket subprotocol negotiation, extensions, or Blob sends.
- An upstream CSP that disallows `/.urspace/assets/socket-shim.js` will preserve
  its policy and therefore disable the compatibility shim. The bootstrap does
  not silently weaken application CSP.

## Deliberate exclusions

- No access to unrelated application daemons or their API surfaces
- No arbitrary filesystem roots inferred from invitation data
- No arbitrary command execution or non-loopback reverse proxying
- No ambient cookies or shared origin across site identities
