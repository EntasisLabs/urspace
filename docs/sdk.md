# Embed Urspace in an application

The CLI is the hands-off path: run `urspace serve`, send the secret URL. This
page is for developers who want the same protocol inside their own app.

Bearer invitation URLs stay the default. Subject-bound mint is a second
enrollment path on the same host. After admit, both use the same session
grant, proof, kick, and (later) quota.

## Cargo

```toml
urspace = { git = "https://github.com/EntasisLabs/urspace" }
```

Host a loopback app and mint either kind of ticket:

```rust,ignore
use urspace::{Client, Site, unix_now};

let host = Site::serve("localhost:8787").await?;
let invite = host.bearer_invite()?; // secret URL
let session = Client::connect(invite.as_str(), unix_now()).await?;

let key = urspace::generate_session_key();
let token = host.mint(*key.public().as_bytes())?; // usi1.… token
let minted = Client::connect_minted(&token, key, unix_now()).await?;
```

`Site::serve` uses the production Iroh relay preset so a browser can reach
you. `Site::serve_local` is for tests and isolated networks.

The public website must not hold the host identity key. Your Rust process is
the host. A public `POST /bootstrap` should call `host.mint(public_key)` and
return the token. Rate-limit that endpoint. Do not log the URL, token, or
session key.

See [`crates/urspace`](../crates/urspace) for the crate README.

## npm

```bash
npm --prefix packages/client run build
npm install ./packages/client
```

Or, once published:

```bash
npm install @urspace/client
```

```js
import { connect, connectMinted, generateSessionKey } from "@urspace/client";

const session = await connect(inviteUrl);

const keys = generateSessionKey();
const token = await mintFromYourServer(keys.publicKey);
const minted = await connectMinted(token, keys.secretKey);
```

`@urspace/client` is a browser package. It wraps the WASM `SiteClient`.
Native apps should use the Cargo crate, not this package.

## What not to mix

- A `usi1.` token is not a shareable URL. Do not put it after `#u4=`.
- A bearer URL is not a mint token. `connectMinted` will reject it.
- Rotating a bearer link still closes that link to newcomers. Subject-bound
  tokens are single-use by default and expire quickly (`MintOptions::ttl`).
