# App channels

Status: design sketch, not a protocol change.

This is the Urspace-shaped answer to: “I have a site that should run an
agent loop, but I do not want to host a public model API, invent auth, and
hope only my page can reach it.”

It is the invitation URL, reused as a client library, and optionally wired
by one host process so the site and the private backend share a single
admitted session.

## The problem

A local agent is easy until the model (or any other private backend) has to
be reachable from a browser or another application:

1. Host an HTTP server for the model.
2. Put auth in front of it.
3. Restrict callers so random clients cannot spend your tokens or read the
   loop.
4. Keep that policy correct as the site, the agent, and the model move.

That work is not about the agent. It is about turning a private process into
a public API. Urspace already refuses that trade for websites: you share an
invitation, not a hostname. App channels apply the same rule to the
client-to-backend hop.

## What you already have

If the site and the model are one local app, today’s host is enough:

```bash
urspace serve localhost:8787
```

Anyone who opens the invitation reaches only that loopback app. The page
can `fetch("/agent/...")` as a same-origin request. The service worker
carries those bytes over the admitted Iroh session. The model never needs a
public URL, an API key in JavaScript, or a CORS policy. Kicking the session
or rotating the invite is the access control.

That is the important picture. The “SDK” is not a second security system.
It is a way to keep that picture when the UI and the backend are no longer
the same process, or when the client is not a tab that opened the bootstrap
URL.

## Two shapes people conflate

### 1. Private site, private backend

The visitor is admitted first. Then the page talks to the agent. This is
Urspace as it exists, plus optional extra loopback targets on the same
session.

Use this when the site itself should stay invitation-gated: a demo, an
internal agent, a shared local app.

### 2. Public site, private backend

The page is on the ordinary internet. The model stays on a machine you
control. A public bundle cannot hold a secret, so it cannot be “the only
caller” by itself. An `Origin` header is not a capability; any client can
send one.

Urspace can still help, but the capability has to live with a visitor or a
device, not in the shipped JavaScript:

- the public page is only a shell
- the visitor presents an invitation (or a later session proof)
- the SDK opens the private channel after admission
- the model remains loopback on the host

If the site must stay public and the model must stay private, the invite
(or an already admitted session) is still the lock. The SDK does not remove
that lock. It removes the extra public API you would otherwise build around
it.

## What “wired by one application” means

Today the pairing step is a URL you copy. The host mints a capability; a
person carries it to a browser; the bootstrap page finishes the handshake.

An app channel is the same pairing, owned by one process:

```text
urspace host
  ├─ site origin     127.0.0.1:5173
  └─ agent origin    127.0.0.1:11434
         │
         │  one invitation, one session grant
         ▼
client SDK (page, worker, or native app)
  ├─ load the site
  └─ call the agent as an admitted client
```

The host still signs invitations and session grants. The hosted site still
never sees the invitation fragment or the session private key. The SDK is
just another client of protocol v4, like the browser WASM client and
`urspace connect`.

The application wires the two ends so you do not stand up a second server,
mint an API key, and then try to make “only this site” true after the fact.

## Client surface

The useful API is the one the browser client already has, without forcing
the Cloudflare bootstrap page to be the only way in:

```js
import { connect } from "@urspace/client";

const session = await connect(invite);

const page = await session.fetch("/");
const reply = await session.fetch("/v1/chat", {
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify({ messages }),
});

const stream = await session.socket("/v1/chat/stream");
```

`invite` is today’s invitation URL, a short link, or a later in-memory
handoff from a page that is already admitted. After `connect`, the
capability is gone. Reconnects use the host-signed grant and a proof from
the client key, exactly as [session grants](session-grants.md) already
require.

Native applications can use the same contract the CLI already implements
with `urspace connect`. The SDK is that client as a library: admit once,
store only the grant plus the client key, resume with a fresh proof.

## Host surface

No new public hostname. The host still exposes only loopback upstreams it
was given explicitly.

The first host feature is multiple named upstreams on one site identity:

```bash
urspace serve localhost:5173 \
  --backend agent=localhost:11434
```

Admitted sessions keep one grant. The site remains the default origin the
browser loads. Named backends are extra loopback apps the SDK can address
without a second invite. The host, not the client, chooses the target
address. The client never sends a destination.

A later convenience command can own both processes so a developer does not
think in “web server plus model server plus auth”:

```bash
urspace app --site ./ui --agent localhost:11434
```

That command would still be `serve` underneath: one identity, one
invitation, one session registry, two loopback origins.

## How authorization stays the same

App channels reuse protocol v4. They do not add a side-door token.

| Question | Answer |
| --- | --- |
| Who may enter? | Possession of a current invitation, then a host-signed session grant |
| Who may call the model? | An admitted session for that site, until kicked or revoked |
| Does the site hold the secret? | No. The bootstrap and SDK drop the capability after admit |
| Can a copied grant be replayed? | No. Resume needs the client private key and a fresh host nonce |
| Can the client pick a different backend host? | No. The host dials only configured loopback origins |
| Does “only my origin” replace the invite? | No. Origin checks are not a capability |

If we later need finer limits — “this invite may load the site but not the
agent”, or “this session may call `/v1/chat` but not `/admin`” — those are
new invitation and grant fields. They need a protocol version bump and the
tamper, expiry, and denial tests in [session grants](session-grants.md).
Until then, an admitted session can use every backend configured on that
host, the same way it can already request every path on a single loopback
app.

Raw TCP stays an explicit `--tcp` invitation scope. A web SDK session does
not inherit a tunnel mount.

## What not to do

- Do not put an invitation fragment, capability, or session key in a public
  JavaScript bundle, source repo, or CDN asset. That publishes the lock.
- Do not treat CORS, referrers, or allowed origins as the authorization
  boundary. They are not.
- Do not have the hosted agent mint or log invitations. The host remains
  the only authority that can admit, kick, or revoke.
- Do not add a second “app token” that bypasses session proofs.
- Do not let the SDK name an arbitrary dial target. The host’s configured
  loopback list is the entire reachability set.

## Phased work

None of this is implemented yet. The order is deliberate: reuse the
current clients before changing the wire format.

1. **Document the existing pattern.** Ship the agent UI and model behind
   one loopback app and one `urspace serve`. This already solves the
   “I do not want a public model API” case.
2. **Named loopback backends.** One site identity, extra
   `--backend name=localhost:port` targets, still protocol v4. Fail closed
   on unknown names and non-loopback origins.
3. **Publish the client.** Extract the WASM `SiteClient` and the native
   connect client behind one small API (`connect`, `resume`, `fetch`,
   `socket`, `close`) for pages and applications that already have an
   invite.
4. **Same-tab injection.** When Urspace is already serving the page,
   expose that admitted session to application script without handing it
   the capability or proof key. The page asks the worker to `fetch` a
   named backend; it does not receive raw credentials.
5. **Scoped invites (only if needed).** Versioned fields that limit which
   backends or paths an invite may use, with the same negative tests as
   today’s grants.

Phase 4 is the “wired by one application” moment for a browser agent: the
visitor opens one link, the page runs an agent loop, and the model stays
on the host. Phase 3 is the same thing for a non-browser client that
should not have to mount a local port first.

## Why this stays an Urspace feature

Tailscale, Cloudflare Tunnel, and an API gateway can all put a hostname in
front of a model. They start from a network or a platform account. App
channels start from the same invitation you already send for a site:

- no extra public origin for the model
- no application-invented bearer token
- the host can still kick one session without taking the site down
- the bootstrap still never sees application traffic

The SDK is how an application holds that invitation. It is not a different
product, and it is not a way to hide a secret in a public page.
