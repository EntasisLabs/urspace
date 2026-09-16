# urspace marketing site

React + Vite + Tailwind landing page for Urspace.

```bash
cd apps/www
npm install
npm run dev
```

Production build:

```bash
npm run build
npm run preview
```

## Hosting (GitHub Pages)

The apex domain `urspace.online` is served from GitHub Pages. The
`www.urspace.online` and `u.urspace.online` hostnames are **not** part of this
deployment; they stay on the Cloudflare Worker in `deploy/cloudflare-worker`
(its route is `*.urspace.online/*`, which does not match the apex).

### How deploys work

`.github/workflows/pages-www.yml` runs on every push to `main` that touches
`apps/www/**` (or the workflow file itself), and can also be started manually
from the Actions tab via `workflow_dispatch`. It:

1. installs dependencies with `npm ci` and runs `npm run build`;
2. uploads `apps/www/dist` with `actions/upload-pages-artifact`;
3. publishes the artifact with `actions/deploy-pages` into the `github-pages`
   environment.

`vite.config.ts` deliberately keeps the default `base: '/'`: the site is served
from the root of a custom domain, not from a `/urspace/` project-pages path.

`public/CNAME` contains `urspace.online`. Vite copies it verbatim into `dist/`,
so the custom domain is baked into every published artifact and Pages will not
lose the domain setting between deploys. Keep that file and the Pages custom
domain setting in sync.

### One-time setup after the first merge

1. In the repository go to **Settings → Pages** and set **Source** to
   **GitHub Actions**. Until this is done the `deploy` job fails; re-run the
   workflow (or use *Run workflow*) once Pages is enabled.
2. After the first successful deploy, confirm **Settings → Pages → Custom
   domain** shows `urspace.online` (it is read from the `CNAME` file) and tick
   **Enforce HTTPS** once the certificate has been issued.

### Cloudflare DNS for the apex

In the `urspace.online` zone create the apex records below. Set them to
**DNS only** (grey cloud, not proxied) so GitHub can validate the domain and
issue its own TLS certificate.

| Type | Name | Content           |
|------|------|-------------------|
| A    | `@`  | `185.199.108.153` |
| A    | `@`  | `185.199.109.153` |
| A    | `@`  | `185.199.110.153` |
| A    | `@`  | `185.199.111.153` |

Optionally add `AAAA` records for `@` pointing at GitHub's published IPv6
addresses (see the "Managing a custom domain for your GitHub Pages site" page
in the GitHub docs for the current set).

Do **not** point `www.urspace.online` at GitHub Pages and do not add a `www`
entry to the Pages custom-domain settings. `www` (and every other subdomain) is
owned by the Cloudflare Worker.
