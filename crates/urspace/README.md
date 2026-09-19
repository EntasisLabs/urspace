# urspace

Embed an Urspace host or client in a Rust application. The CLI remains the
hands-off path for sharing a local app. This crate is for developers who want
the same private session from their own process.

```toml
urspace = { git = "https://github.com/EntasisLabs/urspace" }
```

## Host

Mint a secret invite URL, the same kind `urspace serve` prints:

```ignore
use urspace::Site;

let host = Site::serve("localhost:8787").await?;
let invite = host.bearer_invite()?;
```

Or mint a subject-bound token for one session key. Stealing the token is
useless without the private key:

```ignore
use urspace::Site;

let host = Site::serve("localhost:8787").await?;
let key = urspace::generate_session_key();
let token = host.mint(*key.public().as_bytes())?;
```

## Client

```ignore
use urspace::{Client, RequestMethod, unix_now};

let session = Client::connect(invite, unix_now()).await?;
let page = session
    .fetch(RequestMethod::Get, "/".into(), Vec::new(), Vec::new())
    .await?;

let minted = Client::connect_minted(token, key, unix_now()).await?;
```

Do not log invitation URLs, mint tokens, or session keys. Browser apps should
use [`@urspace/client`](../../packages/client) instead of this crate.
