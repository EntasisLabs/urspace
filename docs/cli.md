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
- `--ttl` accepts whole seconds or a suffix of `s`, `m`, `h`, or `d`.
- `--max-sessions` limits successful browser authorizations for that invitation.
- `--entry-path` chooses the first path the browser requests.

Defaults are a one-hour lifetime, four successful browser connections, `/` as
the entry path, and `https://urspace.online` as the bootstrap. Every invocation
mints a fresh random capability even when the site identity is reused.

Press Ctrl+C to close the Iroh endpoint. The invitation also fails closed after
its expiration or once its session budget has been consumed.

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
