# Medousa Sites

Private experimental workspace for capability-addressed sites served over Iroh.

The first milestone is intentionally narrow: a native host and client prove the
identity, invitation, authorization, revocation, and static-file boundaries. A
browser/Wasm gateway will use the same protocol after these foundations are
stable.

## Security model

- The Iroh endpoint key is the stable site identity and signs every invite.
- An invite contains a random 256-bit capability. The host stores only its
  BLAKE3 digest.
- The signature binds the endpoint ticket, site identity, invite id,
  capability, expiry, entry path, and session limit.
- The client checks that the signer, URL hostname, and endpoint ticket all name
  the same site before dialing.
- Capabilities expire, can be revoked, and have a bounded session count.
- Static paths are percent-decoded, canonicalized, and confined beneath the
  configured root, including through symlinks.

Protocol knowledge is not an authorization factor. Security depends on the
endpoint private key and unguessable capability material.

## Current spike

```bash
cargo run -p medousa-site-host -- serve ./public \
  --bootstrap-origin https://sites.example

cargo run -p medousa-site-host -- get '<invite-url>' /index.html
```

The bootstrap origin is only encoded into the invitation URL during this native
phase. No public service is contacted by the CLI.

