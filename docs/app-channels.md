# App channels

Status: design sketch, not a protocol change.

This is the Urspace-shaped answer to: “I have a site that should run an
agent loop, but I do not want to host a public model API, invent auth, and
hope only my page can reach it.”

It is the invitation URL, reused as a client library, and optionally wired
by one host process so the site and the private backend share a single
admitted session.

The current secret invite URL stays. Subject-bound mint is a second way
to enroll, not a replacement. `urspace serve`, short links, `invite` /
`rotate`, `max-sessions`, and “send someone the link” keep working.

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
| Who may call the model? | An admitted session for that site, until kicked, revoked, or over its host-side time/usage cap |
| Does the visitor hold the model key? | No. The loopback adapter holds it. The WASM agent only has a session |
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

## Owner-held model key, WASM agent, session quota

This is the flow that makes the SDK worth building.

You serve a site. The agent loop runs in WASM in the visitor’s browser.
That loop needs a model. The model key is yours, so the visitor must never
type it, and the page must never ship it. Urspace is the hop from that
WASM client to a loopback process that already has the key. The admitted
session is the identity you meter.

```text
visitor browser
  ├─ your site (UI)
  └─ WASM agent loop
         │  session grant + fresh proof
         │  no model key
         ▼
urspace host
  ├─ knows session_id, budget, expiry
  └─ loopback model adapter (holds the key)
         │
         ▼
localhost model / provider
```

Admission still works as it does today. The visitor opens an invitation
(or resumes a grant). After that, `session_id` is a stable host-allocated
identity for that browser. It is not “Alice.” It is “this admitted
client.” One invitation with `--max-sessions 4` can create four separate
meters. A one-person demo should use `--max-sessions 1`.

The model key stays on the host side of the loopback adapter. The WASM
agent only calls something like `session.fetch("/v1/chat")`. The adapter
attaches the key, talks to the model, and returns the completion. If the
site itself is also served through Urspace, that fetch is same-origin and
the existing service worker already carries it. If the site is a public
shell, the page only needs the host’s public routing data; the SDK then
runs the handshake below. The key still never enters the bundle.

## Two enrollment modes

After the host admits a client, everything else is the same session:
grant, proof, kick, quota, named backends. Only the first ticket differs.

| Mode | Who mints | What the client holds | Who can finish admit |
| --- | --- | --- | --- |
| **Bearer URL** (today, default) | Host, before anyone connects | A secret invitation URL (capability after `#`) | Anyone who has the unexpired, unused invite |
| **Subject-bound mint** (app channel) | Host, after seeing a session public key | A short-lived invite whose `subject` is that key | Only the browser that still holds the matching private key |

Bearer URL is the product you already have:

```bash
urspace serve localhost:8787 --short
```

The printed link is a temporary password. The bootstrap page never sees
the fragment as a server-side secret. The visitor’s browser generates a
`session_key` at Admit, proves it, and gets a grant. Rotating the link
closes it to newcomers; admitted sessions stay. None of that is removed
by adding mint.

Use bearer when you want to share access with a person. Use subject-bound
mint when a public page must enroll itself without putting a transferable
secret in JavaScript. A host may offer both on the same site identity. An
invite minted in one mode must not be accepted as the other: a bearer
capability has no `subject`, and a subject-bound invite is not a URL you
forward.

## Subject-bound mint, then the existing challenge

The public-site handshake people draw is this, and it is almost
protocol v4 already:

```text
Browser loads
    ↓
generate ephemeral session keypair
    ↓
private key stays in browser
public key = browser identity
    ↓
Mint { session_public_key }
    ↓
host signs a subject-bound invite
{
  subject: session_public_key,
  role: "web-agent",
  service: "inference",
  expires: now+30s,
  uses: 1
}
    ↓
browser -> host  Admit { invite_id, invite, session_public_key }
host    -> browser  Challenge { nonce, challenge_id, expires_at_unix }
browser -> host  Proof { challenge_id, signature }
    ↓
host verifies
  invite signature
  invite TTL
  invite unused
  session_public_key == invite.subject
  proof over the host nonce
    ↓
Granted { session_grant }
    ↓
CONNECTED — model calls metered on session_id
```

Map that onto names that already exist. The keypair is today’s
`session_key`. The challenge and proof are the v4 admit transcript.
`uses: 1` is `max_sessions: 1`. The 30-second clock is invite TTL: finish
the handshake now. It is not the model budget. After `Granted`, reconnects
use the session grant and a fresh proof, exactly as
[session grants](session-grants.md) already specify.

Two things are new.

**1. The invite is bound to the key before admit.** Today’s invitation is
a bearer capability. Whoever has the URL may present any
`session_public_key`. In this mode the host sees the public key first and
signs `subject` into the invite. Stealing the mint response is useless
without the browser private key. That is what makes it safe to hand an
invite to a page you do not consider secret.

**2. Minting is the public gate.** Today the operator mints the invite
with the CLI and sends it to a person. Here anyone who can load the site
can ask for a ticket. `POST /bootstrap` (or an in-band `Mint` on the Iroh
connection) is therefore not a lock. It is a policy hook: rate limit,
global token pool, captcha, or “no more invites this hour.” The host still
signs. The public website must not hold the host identity key. The page
may know `host_id` and the endpoint ticket; those are routing, not
authority.

A first cut can keep mint and admit as two host messages so the
single-use invite stays inspectable. They can later collapse into one
round trip. Either way the hosted application still never sees the
session private key, and remaining quota still lives in the registry, not
in the invite.

`role` and `service` are scoped-invite fields. They need a protocol
version if they become authoritative. Until then the host can treat every
subject-bound mint as the `inference` backend and assign the same
session budget it already inherited from the invitation template.

Quota is host policy, like kick. It is not a number in the signed grant
and not a count the WASM reports.

```text
session_id -> {
  ...existing grant registry...
  model_expires_at?,
  token_budget?,
  tokens_used,
}
```

On each proxied model request the host, or the loopback adapter it
trusts, does three checks before the key is used:

1. The session is still `active`.
2. `now < model_expires_at` when a time cap was set.
3. `tokens_used + estimated_cost <= token_budget` when a usage cap was
   set.

If any check fails, the site can stay up and the model call is denied
(HTTP 429 or an equivalent framed error). That is better than kicking the
session: the page can say the budget is gone. A kick remains available
when you want the client gone entirely.

Count from the authoritative upstream response (or from the request and
response bodies the host actually proxied). Do not add a client-supplied
`usage` field and believe it. The WASM agent is yours, but it runs on
someone else’s computer.

Budgets are inherited from the invitation at admission, the same way
`max-sessions` and `allow_tcp` already are:

```bash
urspace serve localhost:5173 \
  --backend model=localhost:11434 \
  --session-ttl 30m \
  --token-budget 50000 \
  --max-sessions 1
```

`--session-ttl` is not today’s invite `--ttl`. Invite TTL is how long new
people may enter. Session TTL is how long an admitted client may keep
using the model. `--token-budget` is a usage cap on that same identity.
Both are host-side. Putting remaining tokens into the grant would let an
old copy disagree with the registry; the registry wins, so the grant
should not carry a spendable balance.

The operator view is the existing session list plus remaining budget.
The page may ask for *its own* remaining quota over the admitted
session. It must not be able to read another session’s meter, and the
response must not include the invitation, the model key, or the proof
key.

`DenialCode` changes are not required for the first cut. Connection
admission and model spend are different questions. A session can stay
authorized for the site after its model budget is exhausted. A new denial
code belongs only if we later refuse reconnects because a quota was
exhausted, and that needs a protocol version and the usual negative
tests.

## What not to do

- Do not put a bearer invitation fragment, capability, or session key in a
  public JavaScript bundle, source repo, or CDN asset. That publishes the
  lock. A subject-bound mint response is not a bearer secret, but the
  private key still never leaves the browser.
- Do not treat CORS, referrers, or allowed origins as the authorization
  boundary. They are not.
- Do not have the hosted agent mint or log invitations. The host remains
  the only authority that can admit, kick, or revoke.
- Do not add a second “app token” that bypasses session proofs.
- Do not let the SDK name an arbitrary dial target. The host’s configured
  loopback list is the entire reachability set.
- Do not put the model API key in the WASM agent, the page, or an
  invitation. The loopback adapter is the only process that should hold
  it.
- Do not trust the client to report tokens used. Meter on the host from
  the proxied model call.
- Do not treat `session_id` as a login. It is one admitted browser. Share
  an invite widely and you mint one budget per successful admission.
- Do not treat `POST /bootstrap` as authentication. In subject-bound mode
  anyone who can hit mint can ask for a ticket. Rate-limit it. Never give
  the public site the host identity key.
- Do not replace the bearer invite URL with mint. The secret link is still
  how you share a private site with a person. Mint is only for a page that
  must enroll without a transferable secret.

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
5. **Session quotas.** Host-side time and token caps inherited from the
   invite, keyed by `session_id`, enforced on the model backend only.
   Deny the model call; do not put remaining balance in the grant. Count
   from the upstream response.
6. **Subject-bound mint.** Host mints a short-lived, single-use invite
   whose `subject` is the browser’s session public key. Verify that the
   Admit key matches `subject`. Do not put the host identity key in the
   public site. Rate-limit the mint; the invite is no longer the secret.
7. **Scoped invites (only if needed).** Versioned fields that limit which
   backends or paths an invite may use, with the same negative tests as
   today’s grants.

Phase 4 is the “wired by one application” moment for a browser agent: the
visitor opens one link, the page runs an agent loop, and the model stays
on the host. Phase 3 is the same thing for a non-browser client that
should not have to mount a local port first. Phase 5 is how the host
turns that admitted identity into a timed or usage-capped model budget
without giving the visitor the key.

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
