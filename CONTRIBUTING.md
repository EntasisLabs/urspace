# Contributing to Urspace

Thanks for helping make private, self-hosted web apps easier to share.

## Before opening a pull request

- Keep the host restricted to an explicit static directory or loopback HTTP
  origin. Do not add arbitrary network proxying or command execution.
- Treat invitation URLs, capability fragments, endpoint private keys, and
  deployment credentials as secrets. Never include them in issues, test fixtures,
  screenshots, logs, or bug requests.
- Add tamper, expiry, authorization, and path-confinement tests when changing a
  security boundary or protocol field.
- Keep compatibility behavior explicit and versioned.

Run the same checks as CI from the repository root:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny check advisories licenses sources
cd apps/bootstrap && npm test
cd ../../deploy/cloudflare-worker && npm ci && npm run check
```

Open a pull request with a concise explanation of the behavior, security impact,
and tests. For suspected vulnerabilities, do not open a public issue; follow the
private reporting instructions in [SECURITY.md](SECURITY.md).

Unless you explicitly state otherwise, contributions intentionally submitted to
Urspace are licensed under both the MIT and Apache-2.0 licenses, at your option,
without additional terms or conditions.
