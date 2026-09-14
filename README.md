# Urspace

Share a web app running on your computer with one private link.

```bash
urspace serve localhost:8787 --short
```

Send the link to someone. They open it in Chrome and use your app. They do not
need an Urspace account, a VPN, a command-line tool, or access to your network.
Your app stays on your computer, and you do not have to open a router port.

Stop Urspace and the site goes away.

> Urspace is an early preview. Chrome and Chromium-based browsers are the main
> supported path today. Safari support is still experimental.

## What is Urspace for?

Urspace is useful when you want to share something that already runs locally:

- a work-in-progress website
- a private dashboard or home service
- a game, demo, or tool for a few friends
- a full local app such as BoxClub or StreetClanker

The important difference is that you share an **invitation**, not a public
server. The invitation can expire, limit how many people may enter, and be
replaced whenever you want. Once admitted, a browser can normally refresh and
reconnect without asking for the original invitation again. The host can still
kick that browser out at any time.

Urspace is not trying to be a general-purpose VPN or permanent public hosting.
It is for giving a browser temporary, private access to one local web app.

## How is it different from Tailscale or Cloudflare Tunnel?

They solve related problems, but they start from different ideas:

| Tool | Best fit | Who can open the site? | What the host sets up |
| --- | --- | --- | --- |
| **Urspace** | Privately sharing one local web app with a person | Anyone you send a valid invitation to; they only need a supported browser | Run one `urspace serve` command |
| [**Tailscale Serve**](https://tailscale.com/docs/reference/tailscale-cli/serve) | Sharing a service inside an existing private Tailscale network | People or devices already allowed into that Tailscale network | Tailscale on the participating devices and network access rules |
| [**Tailscale Funnel**](https://tailscale.com/docs/features/tailscale-funnel) | Publishing a local service to the public internet | Anyone on the internet while the Funnel is running | A Tailscale account, device, and Funnel configuration |
| [**Cloudflare Tunnel**](https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/) | Putting a stable hostname in front of a private service | The public, or users allowed by separately configured Cloudflare Access rules | `cloudflared`, a Cloudflare setup, DNS, and any access rules |

The short version:

- Choose **Tailscale Serve** when everyone should join the same private network.
- Choose **Tailscale Funnel** or **Cloudflare Tunnel** when you want a normal,
  stable internet address and their surrounding platform features.
- Choose **Urspace** when the guest should install nothing and possession of a
  temporary invitation should be enough to enter one app.

Urspace does use a small Cloudflare-hosted page to start the browser connection,
and optionally to store an encrypted short-link record. Cloudflare does **not**
proxy the app itself. After the page opens, app requests travel over an
end-to-end encrypted Iroh connection between the browser and the host. The
current browser path uses an Iroh relay to carry those encrypted bytes, but the
relay cannot read the app traffic.

## Install

Prebuilt releases are available for Apple Silicon and Intel macOS, Arm64 and
x86-64 Linux, and x86-64 Windows.

On macOS or Linux:

```bash
curl --proto '=https' --tlsv1.2 -fsSL \
  https://github.com/EntasisLabs/urspace/releases/latest/download/install-urspace.sh \
  | bash
```

The installer downloads the latest release to `~/.local/bin` and checks that it
has not been changed. Set `URSPACE_INSTALL_DIR` if you want another location.

On Windows, download the Windows zip from
[GitHub Releases](https://github.com/EntasisLabs/urspace/releases).

To build from source instead:

```bash
cargo install --locked --path crates/urspace-host
```

## Share an app

First, start your app normally. If it is listening on `localhost:8787`, run:

```bash
urspace serve localhost:8787 --short
```

Urspace prints a compact `u.urspace.online` invitation. Keep the command
running, send the link, and press Ctrl+C when you are finished.

Without `--short`, Urspace prints a longer link that works without the optional
short-link service:

```bash
urspace serve localhost:8787
```

You can also use a reusable local identity name, a shorter invitation window,
and a one-person limit:

```bash
urspace serve localhost:8787 --name boxclub --ttl 10m --max-sessions 1
```

The time limit controls how long **new** people may enter. Someone already
admitted keeps their private session, including through refreshes and temporary
connection drops, until one of these things happens:

- they close every Urspace tab
- they restart the browser or it completely discards the page from memory
- you kick their session
- you stop the Urspace host

While Urspace is running, type a command into the same terminal:

- `sessions` shows admitted browsers and whether they are connected.
- `kick <session>` removes one browser and makes a fresh invitation.
- `kick all` removes everyone and makes a fresh invitation.
- `invite` or `rotate` stops new people from using the current invitation and
  prints a new one. Already-admitted browsers stay connected.
- `raw` prints the full invitation when you started with `--short`.

Treat an invitation like a temporary password: anyone who receives it can try to
enter until it expires, reaches its session limit, or you rotate it.

See [the CLI guide](docs/cli.md) for every option.

## Keep a named app running

Install a named site once and let the operating system keep it running:

```bash
urspace service install localhost:8787 --name boxclub --short
```

Urspace installs a private launchd agent on macOS or systemd user service on
Linux, starts it immediately, and prints the first invitation. The site returns
after failures and starts again when you log in. On a headless Linux server,
`loginctl enable-linger` also lets it start at boot before you log in.

Unlike a one-off `urspace serve` process, an installed service remembers who you
allowed. If the process or computer restarts, old invite links stop accepting new
people, but browsers that were already let in can reconnect. Kicked browsers stay
kicked. Urspace does not save the secret from an invite link or a browser's
private key.

Manage the running service from another terminal:

```bash
urspace service status boxclub
urspace service start boxclub
urspace service restart boxclub
urspace service invite boxclub
urspace service invite boxclub --for "Alice / work laptop"
urspace service sessions boxclub
urspace service kick boxclub <session-id>
urspace service kick-all boxclub
urspace service stop boxclub
urspace service uninstall boxclub
```

`--for` makes a one-browser enrollment by default and puts that local label next
to the admitted session, so an owner can tell devices apart before kicking one.
The label is deliberately not treated as proof that the person is Alice: whoever
receives the secret invitation can claim that slot. A future organization login
can verify the person while reusing the same host-side enrollment boundary.

Management commands require a private control token stored with your Urspace
data. Uninstalling removes automatic startup but deliberately preserves the site
identity and browser access list. Windows can run named services in the
foreground but does not have automatic service installation yet.

## Share a folder of static files

```bash
urspace static ./public --entry-path /index.html
```

Urspace confines file access to that folder and serves the selected entry page.

## What works today

- Local HTTP apps, including common request methods and response headers
- Same-site browser requests made with `fetch`
- WebSockets used by apps and development tools
- Vite and React apps, client-side routes, and WebMCP tools
- Static folders
- Automatic reconnects and ordinary page refreshes for admitted browsers
- Expiring invitations, session limits, link rotation, and host-side kicking
- Optional encrypted short links with automatic fallback to the full link

For safety, the app proxy only connects to `127.0.0.1`, `::1`, or `localhost`.
It will not forward requests to another machine on your LAN or to an internet
address.

## How it works, in plain English

1. Urspace creates a hard-to-guess invitation for one app and signs it with the
   host's identity.
2. The browser checks that signature before it connects.
3. The invitation secret is kept after the `#` in the URL, so it is not sent to
   the web server that loads the opening page.
4. The host verifies the invitation and gives that browser its own signed
   session. The browser uses the session for refreshes and reconnects instead of
   reusing the original invitation.
5. Session credentials stay in browser memory. Closing every tab forgets them
   and requires an invitation again.

With a short link, the full invitation is encrypted on the host before its
scrambled form is uploaded. The decryption secret stays after the `#` in the
short URL. The short-link service can see that a record was created or fetched,
but it cannot read or change the invitation.

For the protocol details, see [session grants](docs/session-grants.md). For the
security model and current limitations, read [SECURITY.md](SECURITY.md).

## Developing Urspace

BoxClub is the first full-app acceptance target. One script builds and runs
BoxClub, the local browser-opening page, and the Urspace host until Ctrl+C:

```bash
./scripts/dev-boxclub.sh /path/to/boxclub
```

To test the parts separately or deploy the browser-opening page, see
[the bootstrap deployment guide](docs/bootstrap-deployment.md). The native
diagnostic client can open an invitation without a browser:

```bash
cargo run -p urspace-host --bin urspace -- get '<invite-url>' /index.html
```

## Contributing

Bug reports and pull requests are welcome. Read
[CONTRIBUTING.md](CONTRIBUTING.md) for the local checks and security boundaries.
Report suspected vulnerabilities privately as described in
[SECURITY.md](SECURITY.md).

## License

Urspace is available under either the
[Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT), at your
option. Contributions are accepted under the same terms.
