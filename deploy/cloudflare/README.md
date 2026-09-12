# Cloudflare Tunnel deployment

This is the no-inbound-port deployment for the Urspace bootstrap. Docker Compose
runs two containers on a private project network:

- `bootstrap` serves the reviewed browser loader at `bootstrap:8080`.
- `cloudflared` connects outward to Cloudflare and is the only route to it.

Neither service publishes a VM host port. BoxClub and other user applications do
not run in this stack and never pass through Cloudflare.

## 1. Create the tunnel route

In the Cloudflare dashboard, open **Networking → Tunnels**, create a remotely
managed tunnel named `urspace-bootstrap`, and add a published application route:

| Field | Value |
|---|---|
| Hostname | `*.urspace.online` |
| Service type | `HTTP` |
| Service URL | `bootstrap:8080` |

Leave the HTTP Host Header override unset. The original key-bearing hostname
must reach the bootstrap so it can validate the canonical Iroh public key.

Do not put Cloudflare Access authentication in front of this route. Invitations
already provide cryptographic authorization, and recipients must be able to load
the generic bootstrap without an Urspace or Cloudflare account.

Copy the Docker connector command to a private scratchpad and extract only its
`eyJ...` tunnel token. Anyone with that token can run a connector for this
tunnel, so never paste it into chat, a shell command, Git, or an environment file.

## 2. Prepare the VM

The VM needs Git, Docker Engine, the Docker Compose plugin, and outbound access
to Cloudflare. Clone the private repository using the VM's existing GitHub
credentials:

```bash
git clone https://github.com/EntasisLabs/urspace.git
cd urspace
```

Create the token file without recording the token in shell history:

```bash
install -d -m 700 deploy/cloudflare/secrets
read -r -s -p 'Cloudflare tunnel token: ' tunnel_token; echo
umask 077
printf '%s' "${tunnel_token}" > deploy/cloudflare/secrets/cloudflare-tunnel-token
unset tunnel_token
```

The secrets directory is excluded from both Git and the Docker build context.
Compose mounts the token as a read-only runtime secret and uses cloudflared's
`--token-file` support.

## 3. Build and start

```bash
docker compose -f deploy/cloudflare/compose.yaml build --pull bootstrap
docker compose -f deploy/cloudflare/compose.yaml pull cloudflared
docker compose -f deploy/cloudflare/compose.yaml up -d
docker compose -f deploy/cloudflare/compose.yaml ps
```

Wait for the tunnel to show **Healthy** in Cloudflare. Inspect startup logs if it
does not:

```bash
docker compose -f deploy/cloudflare/compose.yaml logs --tail=100 bootstrap cloudflared
```

The Compose stack pins cloudflared release `2026.9.0` by its multi-architecture
image digest; update both deliberately after reviewing release notes. The
bootstrap image is built from the checked out Urspace commit.

## 4. Verify

From any machine with `curl`:

```bash
./deploy/cloudflare/check-edge.sh
```

This confirms public HTTPS health and verifies that an invalid Iroh hostname is
rejected with HTTP 421. After `urspace serve` prints a real site identity, test
the complete bootstrap surface too:

```bash
./deploy/cloudflare/check-edge.sh '<iroh-site-id>'
```

## Updating and token rotation

Deploy a reviewed commit with:

```bash
git pull --ff-only
docker compose -f deploy/cloudflare/compose.yaml build --pull bootstrap
docker compose -f deploy/cloudflare/compose.yaml up -d
```

If the tunnel token is exposed, rotate it in Cloudflare, replace the secret file
using the no-history procedure above, and recreate the connector:

```bash
docker compose -f deploy/cloudflare/compose.yaml up -d --force-recreate cloudflared
```
