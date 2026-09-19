# Urspace CLI

The primary product path is one command and requires no account, DNS setup, or
inbound firewall rule. To embed the same host or client in a Rust or browser
app, see [the SDK guide](sdk.md).

```bash
urspace serve localhost:8787
```

The target must be an HTTP origin on `localhost`, `127.0.0.1`, or `[::1]` and
must not contain credentials, a path, query, or fragment. Urspace deliberately
cannot become an open proxy to LAN or Internet services. It verifies that the
local port is accepting connections before creating an identity or Iroh endpoint.

Direct capability URLs are the default and require no storage service. Add
`--short` to publish an encrypted short-link envelope:

```bash
urspace serve localhost:8787 --short
```

The resulting `https://u.urspace.online/#s1=…` link carries a random 256-bit
seed in its fragment. The CLI derives an opaque lookup id and AES-256-GCM key
with HKDF-SHA-256, uploads only the encrypted signed invite, and falls back to
the direct link if publishing fails. The official short-link service accepts a
maximum seven-day TTL. `--short-origin` is available as an advanced option for
compatible deployments under another base domain.

## Invitation controls

```bash
urspace serve localhost:8787 \
  --name boxclub \
  --ttl 10m \
  --max-sessions 1 \
  --entry-path /
```

- `--name` selects a stable local site identity. Names contain at most 64 ASCII
  letters, numbers, dashes, or underscores.
- `--ttl` controls how long the invitation accepts new browser sessions. It
  accepts whole seconds or a suffix of `s`, `m`, `h`, or `d`.
- `--max-sessions` limits stable browser sessions admitted by that invitation.
  Reconnecting an admitted session from a new Iroh endpoint does not consume
  another slot.
- `--entry-path` chooses the first path the browser requests.

Defaults are a one-hour lifetime, four successful browser connections, `/` as
the entry path, and `https://urspace.online` as the bootstrap. Every invocation
mints a fresh random capability even when the site identity is reused.

Expiration and the session budget are admission controls: they reject new
browser connections but do not terminate a connection that was already
authorized. An admitted browser remains authorized until the host kicks or
revokes it, the host stops sharing, or the browser loses every in-memory copy of
the session. Transport drops and idle service-worker eviction reconnect with the
same host-issued session—even after the admission window closes or the link
rotates—while at least one controlled Urspace page remains alive. The browser
proves possession of a session key against a fresh host nonce and its current
ephemeral Iroh identity; the original capability is discarded after admission.
The grant and proof key remain memory-only. A full page discard or browser
restart therefore requires reopening the invitation.

The foreground sharing console provides live controls without restarting the
site:

```text
invite          mint a fresh link and close the previous link to newcomers
rotate          alias for invite
raw             print the current direct capability URL
sessions        list admitted identities and whether each is connected
kick <session>  disconnect one identity and rotate the outstanding invite
kick all        disconnect every identity and rotate the outstanding invite
help            show the commands
```

Rotation preserves every identity admitted before the old link closed. Kicking
also rotates the link, so the removed identity cannot reopen the bearer URL as a
fresh browser session; a new URL is printed for future recipients. Press Ctrl+C
to close the Iroh endpoint and every active session.

## Named service mode

Install a named local app as an operating-system user service:

```bash
urspace service install localhost:8787 --name boxclub --short
```

`service install` accepts the same bootstrap, invitation lifetime, session limit,
entry path, and short-link settings as `serve`. It requires a name because that
name selects its persistent site identity, service configuration, authorization
journal, pinned Iroh relay, and local control endpoint.

On macOS, Urspace installs a per-user launchd agent. On Linux, it installs a
systemd user service. Both start immediately, return after failures, and start
again when the user logs in. Linux servers that must start the user service at
boot before login should enable lingering for that account:

```bash
loginctl enable-linger
```

Installing the service does not require root access; whether enabling Linux
lingering needs administrator approval depends on the host's policy. The
generated supervisor definition contains only the Urspace executable path, a
validated service name, and the Urspace data directory. The local app and sharing
options live in a private, versioned config file. A supervised process does not
print invitation URLs into service logs; the installing or managing CLI returns
them over the authenticated local control connection.

Each authorization change is appended and synced to disk before it takes effect.
The journal contains capability hashes, invitation limits, public browser keys,
endpoint identities, and revocation state. It never stores an invitation
capability or browser private key. An incomplete final journal record left by a
power loss is discarded during recovery; an inconsistent complete record fails
startup instead of reconstructing authority from browser grants.

On restart, service mode closes every previous invitation to new admissions and
prints a fresh invitation. Previously admitted sessions remain authorized unless
they were kicked or revoked. The service pins its selected relay so the exact
signed reconnect route remains stable across restarts. Restart with the same
`--bootstrap-origin` and `--entry-path`; changing either intentionally makes old
session grants fail their exact-match checks.

Use a second terminal to administer it over an authenticated loopback-only
control connection:

```text
urspace service status boxclub
urspace service start boxclub
urspace service restart boxclub
urspace service invite boxclub
urspace service invite boxclub --for "Alice / work laptop" --direct
urspace service invite boxclub --for "Alice / work laptop" --tcp
urspace service invite boxclub --for "QA team" --max-sessions 4
urspace service sessions boxclub
urspace service kick boxclub <session-handle>
urspace service kick-all boxclub
urspace service stop boxclub
urspace service uninstall boxclub
```

A named invitation is the first team-access building block. `--for` records a
local administrative label with browsers admitted through that invitation and
defaults that invitation to one session. Use `--max-sessions` when the label is
for several devices. The label is stored in the host's private authorization
journal and shown by `service sessions`; it is not sent to the browser or used as
identity proof. Anyone holding the invitation can consume an available slot, so
send each named invitation only to its intended recipient.

The independent random `session-…` handle shown by `service sessions` is required
for a kick. Urspace deliberately does not print the underlying session UUID or
browser endpoint identity into terminal or supervisor logs.
The local control token is regenerated for each run and stored in a private file
below the user's Urspace data directory. Set `URSPACE_DATA_DIR` to an absolute
directory to relocate all Urspace identity and service state. Use the same
environment value when running later management commands; the installed service
records it for its own restarts.

`service stop` leaves the service installed, while `service uninstall` removes
automatic startup. Both preserve the stable site identity, authorization
journal, and admitted browser list. Reinstalling the same name therefore requires
the same sharing settings; use a new name when intentionally creating a different
site authority. `service run` remains available as a foreground entry point for
containers and custom supervisors. Automatic Windows service installation is not
implemented yet.

## Native device connection

For a managed device, mount the host's saved loopback app on a client-side
loopback port without executing the Cloudflare-hosted browser bootstrap:

```bash
urspace service invite boxclub --for "Alice / work laptop" --tcp
# On Alice's device:
urspace connect boxclub localhost:9090 --invite-stdin
```

The host's `--tcp` flag is explicit authorization for raw TCP access and implies
`--direct`. The client cannot choose or change the host-side target: every local
connection goes only to the loopback endpoint saved when the named service was
installed. The client reads the invitation from standard input, verifies it
locally, creates a distinct device proof key, and stores the host-signed grant
and private key below the local Urspace data directory. The file is created with
mode `0600` on Unix, inherits the user data directory's ACL on Windows, and is
never printed.

While the command runs, software on Alice's machine can use `localhost:9090` as
though the service were local. Each accepted TCP connection gets its own Iroh
stream. The tunnel supports half-close behavior and carries bytes without
parsing or rewriting them, so it works for HTTP, WebSockets, SSH, databases, and
other TCP protocols. UDP is not supported.

Reconnect later with:

```bash
urspace connect boxclub
```

The saved grant is not a bearer credential: the host sends a fresh challenge and
requires a signature from the saved device key. A host-side kick takes effect on
native devices exactly as it does for browsers. The first TCP enrollment saves
the selected local port, so later runs restore the same mount. Supplying a new
`localhost:<port>` overrides the mount for that run. Raw mounts bind only to
`127.0.0.1`, but any process on the client device may connect to that local port;
use them only on a trusted device.

The origin-isolated browser gateway remains available by omitting the positional
endpoint and using an invitation made with `--direct`:

```bash
urspace connect boxclub --invite-stdin
```

That mode prints a random `http://<random>.localhost:<port>/` URL. The random
hostname is checked on every request so unrelated browser traffic cannot select
the gateway by port alone. Its first enrollment selects and saves a free local
port, preserving the same browser origin, cookies, and storage on later runs.
`--listen` is an advanced one-run port override for browser-gateway mode.

For non-interactive automation, `--invite URL` is available. Prefer
`--invite-stdin` for people because command arguments may be retained in shell
history or visible to same-user process inspection. Device keys currently rely
on private filesystem permissions rather than an operating-system keychain.

## Browser compatibility

Chrome and Chromium are the primary tested browser path for the current preview.
Urspace also normalizes Iroh relay hostnames for Safari, which rejects TLS relay
URLs containing the DNS root's terminal dot. Failed relay handshakes are canceled
without authorizing the invitation and produce a browser-specific recovery message.

## Static files

Static directories use the same capability and identity model:

```bash
urspace static ./public --entry-path /index.html
```

Only GET and HEAD are supported for static sites. Paths are confined beneath the
canonicalized directory root.

## Development bootstrap

The production origin is the default. Local bootstrap development may override
it explicitly:

```bash
urspace serve localhost:8787 --bootstrap-origin http://localhost:8080
```

Plain HTTP is accepted only for `localhost`; non-local bootstrap origins require
HTTPS and are cryptographically bound into the invitation.
