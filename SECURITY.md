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
- Browser session proof private keys
- Native device proof private keys
- Unexpired short-link fragment seeds
- Local named-service control tokens
- Managed-service configuration and supervisor-definition integrity
- Files below the explicitly shared site root
- Integrity of the endpoint identity, route, authorization journal, bootstrap
  origin, and grant limits

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
- Browser code removes the invitation fragment before remote HTML runs and
  zeroes mutable Rust copies after authorization. JavaScript strings cannot be
  reliably erased.
- The service worker and the first injected Urspace script retain a resume
  handoff only in memory. For protocol v4, the handoff contains a host-signed
  session grant and its separate browser proof key; it does not contain the
  invitation capability. Legacy v2/v3 hosts retain their previous invitation
  and endpoint-key handoff during the compatibility window.
  It is carried only on navigation responses in a one-time data attribute on a
  tiny inline bootstrap. The worker authorizes that bootstrap with a fresh CSP
  nonce, the bootstrap captures the handoff before application scripts run,
  removes its element, proactively returns the handoff during page lifecycle
  transitions, and answers only trusted service-worker messages through a
  transferred port. Ordinary application fetches never receive the handoff;
  ambiguous or combined CSP policies omit it and fail closed.
- No invitation, capability, endpoint secret, session proof key, grant, or
  resume handoff is written to
  `localStorage`, `sessionStorage`, IndexedDB, the Cache API, or cookies. A
  restarted service worker accepts a handoff only for its exact site origin, and
  the host still verifies a fresh proof bound to its nonce, the grant, and the
  new Iroh endpoint identity before consulting revocation and kick state.
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

## Session grant invariants

Protocol v4 treats an invitation as enrollment, not a permanent reconnect
credential. A successful admission creates a stable random session id and an
Ed25519 proof key generated by the browser. The host returns a signed grant that
contains the session public key but never the invitation capability.

Every reconnect uses a fresh host nonce and signs a canonical transcript bound
to the exact host, site, ALPN, grant hash, stable session id, current Iroh
endpoint identity, challenge id, and challenge expiry. The host verifies the
grant and current registry record both before and after the proof. A copied
grant cannot reconnect without the proof key; a copied proof cannot be replayed
on a different endpoint or challenge. Kicks and invite revocations remain
stateful and override every signed grant immediately.

The host rotates the grant after a successful reconnect. Cloudflare serves only
the generic bootstrap and does not store the grant, proof key, or application
traffic. See [docs/session-grants.md](docs/session-grants.md) for the complete
wire contract.

## Named service state and control

Named service mode persists an append-only authorization journal containing
capability hashes, invitation limits, public session keys, endpoint identities,
revocation state, and optional operator-supplied access labels. Labels are local
administrative metadata, not authenticated identities, and must never grant
authority by themselves. The journal never writes capability plaintext, a
signed grant, or a browser private key. Mutations are appended and synced before
the live registry changes. Recovery truncates an incomplete final append and
rejects inconsistent complete events.

The service pins the relay address included in its signed session grants so the
exact endpoint ticket remains stable across host process restarts. Startup closes
all earlier invitations to new admissions before minting a new one; admitted
sessions retain their state. It does not weaken signer, origin, entry-path, or
endpoint-ticket matching during resume.

Management uses a loopback-only TCP listener authenticated by a new random
256-bit token for every service run. The listener address and token are kept in a
private file below the user's Urspace data directory and removed on graceful
shutdown. Requests and responses are size- and time-limited. Anyone able to read
the service owner's files or process memory is already inside this local trust
boundary.

`service install` stores sharing settings in a private, versioned config and
generates a per-user launchd or systemd definition. Supervisor definitions use an
argument array or quoted unit arguments and contain no invitation capability,
session grant, or control token. Supervised runs do not print invitations into
service logs; invitation creation remains an authenticated local control action.
Generated systemd units enable `NoNewPrivileges`, a private temporary directory,
read-only system and home views, and persistent write access only to the selected
Urspace data directory.

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

## Native managed-device boundary

`urspace connect` verifies the signed invitation and Iroh endpoint locally. It
creates a separate Ed25519 device proof key, persists that key and its signed
non-bearer session grant in a user-only file, and proves possession against a
fresh host challenge on every process restart. Invitation capabilities are
zeroed after the admission request and are never written to the device file.
The Cloudflare bootstrap, Worker, short-link resolver, and service worker do not
participate in this path.

The browser-facing gateway binds only to a numeric loopback address and requires
an unpredictable 128-bit label in the `*.localhost` Host header. Requests with a
different Host are rejected before proxying, and responses force a no-referrer
policy so normal navigation does not disclose the local gateway origin. The
random label is local access control, not remote authentication; processes with
the same operating-system account are inside this local trust boundary. The
enrollment also stores its selected loopback port so reconnects preserve the
browser origin; an explicit `--listen` override changes that origin for the run.

The current client protects device keys with atomic creation and mode `0600` on
Unix; on Windows it relies on the user data directory's inherited ACL. It does
not yet use macOS Keychain, Windows Credential Manager, or Linux Secret Service.
A later hardening pass should add those backends while retaining an explicit
file-based mode for headless machines.

Owner-facing session listings expose only a separate random operator handle.
It is generated independently rather than derived from the private session UUID.
Raw session UUIDs and browser endpoint identities are not written to terminal or
supervisor logs. The host resolves a handle back to live private state before a
kick; the handle itself grants no access.

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
- Foreground `serve` and `static` authorization state remains memory-only and
  rejects old grants after restart. Named services persist loopback-app
  authorization and expose authenticated local management. Automatic service
  installation supports per-user launchd on macOS and systemd on Linux; Windows
  installation and privileged system-wide services are not implemented yet.
- Named services pin one relay for route continuity. Relay migration and
  redundant signed routes are not implemented yet.
- The authorization journal is append-only and does not compact old events yet.
- The browser and loopback proxy buffer each HTTP response up to 64 MiB. Streaming
  responses, SSE, uploads larger than 16 MiB, and stricter content-specific
  limits are not yet supported.
- Browser Iroh endpoints are relay-only under current Web platform constraints.
- A short link adds a centralized first-open availability dependency. If the
  resolver is unavailable, recipients need the direct invitation printed by the
  `raw` console command. Application traffic never traverses the resolver.
- Service-worker eviction can be recovered while at least one controlled
  Urspace page remains alive, and proactive page-lifecycle handoff covers normal
  refresh. Full page discard, closing every site page long enough for the worker
  to terminate, or a browser process restart destroys the in-memory factors and
  requires reopening the invitation. Lifecycle behavior still needs testing
  across target browsers.
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
