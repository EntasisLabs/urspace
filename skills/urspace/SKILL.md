---
name: urspace
description: Install and operate the Urspace CLI to privately share local websites, static files, and TCP services over encrypted Iroh connections. Use when the user asks to install or update Urspace, serve or share a localhost app, run a persistent named Urspace service, connect a managed device, mount a remote service on localhost, or manage Urspace invitations and admitted devices.
license: MIT OR Apache-2.0
---

# Urspace

Urspace gives a local app a private encrypted path to an invited browser or
device without opening an inbound firewall port. Iroh may connect peers directly
or through an encrypted relay; do not promise that every route is physically
peer-to-peer.

Read [references/operations.md](references/operations.md) before running an
install, host, connect, or access-management command.

## Choose the operating mode

- Use `urspace serve` for a temporary foreground share of an HTTP app already
  listening on loopback.
- Use `urspace static` for a temporary read-only share of a directory.
- Use `urspace service install` for a named HTTP app that should survive terminal
  closure, failures, and login/reboot according to the host platform.
- Use `urspace connect` for an enrolled device. With no positional endpoint it
  creates the origin-isolated browser gateway; with `localhost:<port>` it mounts
  the host service as raw TCP.
- Use `urspace service sessions`, `kick`, and `kick-all` to administer access.

## Preserve the security boundary

- Accept only an explicit directory or numeric/localhost loopback origin. Never
  turn Urspace into a proxy to a LAN address, public host, or user-supplied
  destination on the connecting side.
- Treat every invitation as a temporary password. Return it only to the user who
  requested it; never commit it, write it to a durable note, post it publicly, or
  repeat it unnecessarily.
- Enroll native clients with `--invite-stdin`. Pass the invitation through the
  process's stdin facility; never interpolate it into a shell command or use
  `--invite URL` unless the user explicitly accepts command-history and process-
  list exposure.
- Keep direct invitations as the storage-free default. Use `--short` only when
  the user wants easier sharing and explain that it uploads an encrypted envelope
  to the configured short-link service.
- Scope invitations to the intended audience. For one person or device, prefer a
  short admission window and one session. Use `--for` on named services so the
  host can recognize the admitted device later.
- Use `--tcp` only when the user asks to mount a service for a trusted managed
  device. Any local process on that client may attempt to use the bound port, so
  the upstream application should retain its own authentication when local
  software is outside the trust boundary.
- Do not inspect, print, copy, or edit Urspace identity files, authorization
  journals, control tokens, device keys, or saved grants. Use the CLI's management
  commands.
- Do not add DNS records, firewall rules, Cloudflare tunnels, port forwarding, or
  unrelated daemons. Urspace does not require them.

## Operate with clear authority

An explicit request to install, host, connect, stop, or remove an Urspace service
authorizes that named operation. Do not infer permission to distribute an invite,
kick a different device, uninstall another service, or expose a broader target.

Before hosting, confirm that the selected local app is listening or that the
static directory is the intended root. Before binding a client mount, confirm the
local port is appropriate and does not need to be reachable beyond loopback.

Keep foreground hosting or connecting processes alive for as long as the user
requested. Report how to stop them. For persistent service changes, verify with
`urspace service status <name>` and show access with `urspace service sessions
<name>` when relevant.

## Report the result

State the mode, exact local target, admission lifetime/session limit, whether a
short-link service participates, and how the user can stop or revoke access.
Share a newly printed invitation once in the private response. Do not claim the
site is public: possession of a valid invitation or an already admitted device
is still required.
