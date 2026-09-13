# Repository guidance

- Treat protocol details as discoverable. Never rely on obscurity for security.
- Keep the site-serving process separate from every unrelated daemon and expose
  only an explicit directory or loopback upstream.
- Never log invitation fragments, raw capabilities, private keys, or session
  secrets.
- New protocol fields require versioning and tamper/expiry tests.
- Resolve filesystem paths canonically and reject any target outside the shared
  root.
- Keep this repository private unless its owner explicitly changes that policy.
