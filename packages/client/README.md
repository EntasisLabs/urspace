# @urspace/client

Browser client for Urspace. Use this when a web app should open a private
session without going through the CLI.

```bash
npm install @urspace/client
```

Build the WASM bindings from the repository root before installing from git:

```bash
npm --prefix packages/client run build
```

## Bearer invite URL

```js
import { connect } from "@urspace/client";

const session = await connect(inviteUrl);
const page = await session.fetch("GET", "/", [], new Uint8Array());
```

That `inviteUrl` is the same secret link `urspace serve` prints.

## Subject-bound mint

```js
import { connectMinted, generateSessionKey } from "@urspace/client";

const keys = generateSessionKey();
const token = await fetch("/bootstrap", {
  method: "POST",
  headers: { "content-type": "application/json" },
  body: JSON.stringify({ public_key: Array.from(keys.publicKey) }),
}).then((response) => response.text());

const session = await connectMinted(token, keys.secretKey);
const reply = await session.fetch(
  "POST",
  "/v1/chat",
  [{ name: "content-type", value: "application/json" }],
  new TextEncoder().encode(JSON.stringify({ messages })),
);
```

Your server should call the Rust host `Site::mint(public_key)` and return the
`usi1.` token. Do not put the host identity key or the model API key in this
package. Do not log the invite URL, mint token, or `secretKey`.

`POST /bootstrap` is not authentication. Rate-limit it. The token is useless
without `secretKey`, but anyone who can hit mint can ask for a ticket.
