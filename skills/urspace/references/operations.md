# Urspace operations

Use the smallest mode that satisfies the request. Commands below use placeholders;
replace them only with values established from the user's environment.

## Install or update

First check whether `urspace` is already available:

```bash
command -v urspace
urspace --help
```

On macOS or Linux, use the official installer when the user asks to install or
update. It selects the local architecture, downloads the latest GitHub release,
checks the archive against the release's `SHA256SUMS`, and installs without root
to `~/.local/bin` by default:

```bash
curl --proto '=https' --tlsv1.2 -fsSL \
  https://github.com/EntasisLabs/urspace/releases/latest/download/install-urspace.sh \
  | bash
```

Set `URSPACE_INSTALL_DIR` only when the user selected another destination. Do
not use `sudo`. After installation, resolve the installed path and run
`urspace --help` from that path if the user's shell has not reloaded `PATH`.

On Windows, select the `x86_64-pc-windows-msvc.zip` asset from the latest
official GitHub release, verify it against `SHA256SUMS`, and place
`urspace.exe` in a user-owned directory on `PATH`. Do not claim automatic
Windows service installation is supported.

When updating a host with named services, restart only the service names the user
put in scope:

```bash
urspace service restart <name>
urspace service status <name>
```

## Temporary HTTP app share

Confirm the app is already accepting connections on loopback, then start a
foreground host:

```bash
urspace serve localhost:<app-port> --ttl <duration> --max-sessions <count>
```

Use `--name <name>` when the user wants to reuse a stable local site identity.
Use `--entry-path /path` only when the app should open somewhere other than `/`.
For one recipient with no stated policy, propose or clearly state a conservative
`--ttl 10m --max-sessions 1` assumption before running.

Direct invitations are the default. Add `--short` only when the user wants a
compact link and accepts the optional encrypted short-link lookup:

```bash
urspace serve localhost:<app-port> --ttl 10m --max-sessions 1 --short
```

Keep the process running. Its interactive console supports:

```text
sessions
invite
rotate
raw
kick <session-handle>
kick all
help
```

`invite` and `rotate` close the old invitation to newcomers but retain admitted
sessions. A kick also rotates outstanding invitation access. Ctrl+C stops the
foreground host and disconnects active sessions.

## Temporary static directory share

Resolve the intended directory and ensure it does not include secrets or files
outside the requested sharing scope:

```bash
urspace static <directory> --entry-path /index.html
```

Static mode supports GET and HEAD. Do not substitute a broader parent directory
for convenience.

## Persistent named service

On macOS or Linux, install a named HTTP app as a per-user operating-system
service:

```bash
urspace service install localhost:<app-port> --name <name>
```

Add explicit admission settings or `--short` when requested. The command starts
the service and returns its initial invitation. On Linux, enabling user lingering
for startup before login is a separate system-policy choice; do not run
`loginctl enable-linger` unless the user asks and has the necessary authority.

Manage only the named service in scope:

```bash
urspace service status <name>
urspace service start <name>
urspace service restart <name>
urspace service stop <name>
urspace service uninstall <name>
```

`stop` preserves installation and state. `uninstall` removes automatic startup
but deliberately preserves site identity and authorization state. Explain this
before treating uninstall as data deletion.

## Browser invitations and native browser gateway

Create a browser invitation for a named service:

```bash
urspace service invite <name> --for "<person or device>"
```

For a native client that should bypass the browser bootstrap while retaining an
origin-isolated local browser URL, create a direct invite on the host:

```bash
urspace service invite <name> --for "<person or device>" --direct
```

Then launch the client without placing the invitation in argv:

```bash
urspace connect <local-enrollment-name> --invite-stdin
```

Start the command first and send the invitation through stdin. Later reconnects
need no invitation:

```bash
urspace connect <local-enrollment-name>
```

## Raw TCP mount for a managed device

Raw TCP requires a separately scoped invitation. Create it on the host:

```bash
urspace service invite <name> --for "<person or device>" --tcp
```

On the trusted client, bind the desired fixed loopback port and provide the
invitation through stdin:

```bash
urspace connect <local-enrollment-name> localhost:<client-port> --invite-stdin
```

The client cannot select the destination on the host. The named service always
dials the exact numeric loopback endpoint saved during `service install`.
`--tcp` implies a direct invitation and does not involve Cloudflare. It carries
TCP protocols such as HTTP, WebSockets, SSH, and databases; it does not carry UDP
or Unix-domain sockets.

Later runs restore the saved mount and require no invite:

```bash
urspace connect <local-enrollment-name>
```

## Review or revoke named-service access

List the operator-safe handles before choosing a target:

```bash
urspace service sessions <name>
```

Kick only the handle the user identified:

```bash
urspace service kick <name> <session-handle>
```

Kick every admitted device only when the user explicitly requests that scope:

```bash
urspace service kick-all <name>
```

Never derive or expose internal session UUIDs, endpoint identities, device keys,
or journal contents. The displayed session handle is the intended management
interface.
