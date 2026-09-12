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

## Browser transport spike

The browser client validates the entire invitation before dialing, wipes its
Rust-side copy of the URL and capability after authorization, and fetches the
entry HTML over Iroh. The bootstrap removes the fragment from browser history
before executing the fetched document in a sandboxed, opaque-origin iframe.

For local development, loopback HTTP is allowed only under `localhost`; remote
bootstrap origins must use HTTPS.

```bash
cd apps/bootstrap
npm run build
npm run serve
```

Then mint an invite with `--bootstrap-origin http://localhost:8080`. This phase
supports a single-file HTML entry point. Transparent subresource loading is the
next service-worker milestone.

On macOS, building `ring` for the browser target requires a Clang with WebAssembly
support. The build script automatically uses Homebrew LLVM when it is installed;
otherwise set `CC_wasm32_unknown_unknown` to a suitable compiler.
