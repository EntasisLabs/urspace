# Urspace CLI

The primary product path is one command and requires no account, DNS setup, or
inbound firewall rule:

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
- `--max-sessions` limits unique browser endpoints admitted by that invitation.
  Reconnecting the same in-memory browser endpoint does not consume another slot.
- `--entry-path` chooses the first path the browser requests.

Defaults are a one-hour lifetime, four successful browser connections, `/` as
the entry path, and `https://urspace.online` as the bootstrap. Every invocation
mints a fresh random capability even when the site identity is reused.

Expiration and the session budget are admission controls: they reject new
browser connections but do not terminate a connection that was already
authorized. An admitted browser remains authorized until the host kicks or
revokes it, the host stops sharing, or the browser loses every in-memory copy of
the session. Transport drops and idle service-worker eviction reconnect with the
same ephemeral Iroh identity—even after the admission window closes or the link
rotates—while at least one controlled Urspace page remains alive. Resumption is
bound to both that authenticated endpoint identity and the capability; neither
is written to browser storage. A full page discard or browser restart therefore
requires reopening the invitation.

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
