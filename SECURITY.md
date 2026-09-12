# Security model

Protocol knowledge is assumed public. An attacker may know every field, state
transition, endpoint, and implementation detail described here.

## Protected assets

- Site endpoint private key
- Unexpired invitation capabilities
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
- Revocation is consulted for every request, including requests on an already
  authorized connection.
- Browser code removes the fragment before remote HTML runs and zeroes mutable
  Rust copies after authorization. JavaScript strings cannot be reliably erased,
  so the bootstrap also drops its reference as soon as the WASM call returns.

## Current limitations requiring hardening

- The bootstrap origin is trusted executable code. A compromised deployment can
  read invitation fragments before they are cleared. Production requires pinned,
  reproducible loader artifacts and tightly controlled deployment credentials.
- Static path confinement currently canonicalizes and then opens the file. This
  rejects traversal and ordinary symlink escapes but is not yet safe against a
  malicious local writer racing path resolution. Replace it with capability-based
  directory handles before serving attacker-writable trees.
- Capability state is memory-only. Restarting the host invalidates every invite
  (fail closed), and there is not yet a management socket for live revocation.
- The browser bootstrap buffers a response up to 64 MiB. Streaming and stricter
  content-specific limits are planned.
- Browser Iroh endpoints are relay-only under current Web platform constraints.

## Deliberate exclusions

- No access to the Medousa daemon or its API surface
- No arbitrary filesystem roots inferred from invitation data
- No POST, PUT, DELETE, WebSocket, or server execution support
- No ambient cookies or shared origin across site identities

