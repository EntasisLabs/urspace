# Urspace

Capability-addressed web apps served from a private machine over Iroh. The
browser receives a signed, expiring invite URL; a generic bootstrap verifies it,
connects to the named Iroh endpoint, and exposes the app at an isolated origin.

Protocol knowledge is not an authorization factor. Access requires the random
256-bit capability in the URL fragment, and the endpoint key signs every field
that decides where and how the browser connects.

## Install and share

Prebuilt releases are published for Apple Silicon and Intel macOS, Arm64 and
x86-64 Linux, and x86-64 Windows. macOS and Linux users can install the latest
release without a Rust toolchain:

```bash
curl --proto '=https' --tlsv1.2 -fsSL \
  https://github.com/EntasisLabs/urspace/releases/latest/download/install-urspace.sh \
  | bash
```

Set `URSPACE_INSTALL_DIR` to choose a destination other than `~/.local/bin`, or
download the script and pass a tag such as `v0.2.1` to install an exact release.
The installer uses public GitHub release URLs and verifies the selected archive
against the release's `SHA256SUMS` before installing it. Windows users can
download `urspace-*-x86_64-pc-windows-msvc.zip` from
[GitHub Releases](https://github.com/EntasisLabs/urspace/releases).
Every release archive contains the license texts and has GitHub build-provenance
attestations. A downloaded archive can be independently checked with:

```bash
gh attestation verify <archive> --repo EntasisLabs/urspace
```

To build from source instead:

```bash
cargo install --locked --path crates/urspace-host
```

Share an app already listening on loopback:

```bash
urspace serve localhost:8787
```

Urspace prints a signed, one-hour invitation under `urspace.online`. The
recipient needs only a modern browser. The app stays on this machine, no inbound
port is opened, and pressing Ctrl+C stops new traffic immediately. The hour is
an admission window: browsers already connected remain authorized after it
closes, while new connections are rejected.

For a compact share URL, explicitly opt into encrypted shortening:

```bash
urspace serve localhost:8787 --short
```

This prints a link shaped like `https://u.urspace.online/#s1=<secret>`. The CLI
encrypts the complete signed invitation locally and uploads only the ciphertext.
The secret remains in the URL fragment, is never included in the HTTP request,
and decrypts the invitation in the recipient's browser. The short-link service
can observe creation and retrieval metadata or deny service, but cannot read or
forge the invitation. Shortening is limited to seven-day invitations and falls
back to the direct URL if the optional service is unavailable.

While sharing, the same terminal accepts live operator commands:

- `invite` or `rotate` closes new admissions on the current link and prints a
  fresh one; already-admitted browsers keep their sessions.
- `raw` prints the current direct capability URL when shortening is enabled.
- `sessions` lists admitted browser identities and connection state.
- `kick <session>` or `kick all` immediately disconnects selected identities,
  denies their automatic reconnects, closes the URL they know to newcomers, and
  prints a fresh URL for future sharing.

Use a stable local identity name and tighter invitation limits when desired:

```bash
urspace serve localhost:8787 --name boxclub --ttl 10m --max-sessions 1
```

`--name` selects a persistent local signing identity. Every command invocation
still creates a new random capability, invite ID, expiry, and session budget.
Identity keys are stored in the platform-local application data directory under
`urspace/sites` with private file permissions.

See [docs/cli.md](docs/cli.md) for the complete command contract.

## What works

- Static directories with confined GET/HEAD access
- Loopback HTTP apps with GET, HEAD, POST, PUT, PATCH, DELETE, and OPTIONS
- Same-origin browser `fetch` calls, including request bodies and response headers
- Same-origin WebSockets through an injected standards-shaped browser shim
- Vite/React bundles, client-side routes, and page-defined WebMCP tools
- Automatic Iroh reconnection for already-admitted browser sessions
- Expiring, revocable, session-limited invites
- Optional end-to-end encrypted short links with direct-link fallback

The loopback proxy is intentionally limited to `http://127.0.0.1`,
`http://[::1]`, or `http://localhost`. It will not proxy to LAN or Internet
origins and does not follow upstream redirects.

## BoxClub / StreetClanker

BoxClub is the first full-app acceptance target. Run its existing all-in-one
production server; no BoxClub source changes are required.

For the local end-to-end setup, one command builds and runs BoxClub, the browser
bootstrap, and the Iroh proxy until Ctrl+C:

```bash
./scripts/dev-boxclub.sh /path/to/boxclub
```

The equivalent three-terminal setup is useful when debugging:

```bash
cd /path/to/boxclub
npm run build
NODE_ENV=production PORT=8787 npm start
```

Build and serve the generic bootstrap locally in another terminal:

```bash
cd /path/to/urspace/apps/bootstrap
npm run build
npm run serve
```

Then mint the invite and keep the proxy running:

```bash
cd /path/to/urspace
cargo run -p urspace-host --bin urspace -- serve localhost:8787 \
  --bootstrap-origin http://localhost:8080 \
  --name boxclub \
  --ttl 1h \
  --max-sessions 4
```

Remove the local bootstrap override and add `--short` when sharing StreetClanker
through the production `urspace.online` edge.

Open the emitted URL in a browser. The capability fragment is removed before
BoxClub code runs. Its frontend chunks, `/api/*` requests, `/ws` connection, and
WebMCP HTTP fallback all traverse the authenticated Iroh connection.

`localhost` is a same-machine development bootstrap. To share an invite with
another device, deploy `apps/bootstrap/public` as immutable static files behind
the `https://urspace.online` wildcard origin. It must route
`https://<site-id>.urspace.online/.urspace/open/` and serve a wildcard TLS
certificate. The bootstrap is generic and never receives the fragment over
HTTP; fragments stay client-side.

## Static sites

```bash
cargo run -p urspace-host --bin urspace -- static ./public \
  --entry-path /index.html
```

The native diagnostic client uses the same invite verification and transport:

```bash
cargo run -p urspace-host --bin urspace -- get '<invite-url>' /index.html
```

## Browser boundary

The signed URL uses the reserved `/.urspace/open/` bootstrap path. A root-scoped
service worker holds the authenticated Iroh client in memory and maps ordinary
same-origin requests onto independent QUIC streams. It injects only the small
WebSocket compatibility shim into HTML responses; application scripts otherwise
run unchanged at the capability-derived site origin.

If the browser terminates the worker, the connection fails closed. The invite
is not persisted to IndexedDB, Cache Storage, cookies, or local storage; reopen
the original invite to reconnect.

See [SECURITY.md](SECURITY.md) before exposing a non-development bootstrap.
The production container, wildcard DNS/TLS contract, verification steps, and
operational rules are in [docs/bootstrap-deployment.md](docs/bootstrap-deployment.md).

## Contributing

Bug reports and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md)
for the security boundaries and local checks. Report suspected vulnerabilities
privately as described in [SECURITY.md](SECURITY.md).

## License

Urspace is available under either the
[Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your
option. Contributions are accepted under the same terms.
