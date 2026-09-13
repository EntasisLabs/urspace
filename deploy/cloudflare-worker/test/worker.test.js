import assert from "node:assert/strict";
import test from "node:test";

import {
  ASSETS,
  LEGACY_V2_ASSETS,
  handleRequest,
  isCanonicalSiteHost,
  isCanonicalZ32KeyLabel,
} from "../src/index.js";

const SITE_ID = "3mied18mppzo5rm16uzw5s6rxakceay3snhimxph3yjqi5tkdy8o";
const SITE_HOST = `${SITE_ID}.urspace.online`;

function fixture({ assetStatus = 200 } = {}) {
  const requests = [];
  return {
    requests,
    env: {
      ASSETS: {
        async fetch(request) {
          requests.push(request);
          const body = request.method === "HEAD" ? null : `asset:${new URL(request.url).pathname}`;
          return new Response(body, {
            status: assetStatus,
            headers: { "X-Fixture": "asset-binding" },
          });
        },
      },
    },
  };
}

test("accepts only canonical 32-byte z-base-32 site hosts", () => {
  assert.equal(isCanonicalZ32KeyLabel(SITE_ID), true);
  assert.equal(isCanonicalSiteHost(SITE_HOST), true);
  assert.equal(isCanonicalSiteHost(`${SITE_HOST.toUpperCase()}.`), true);
  assert.equal(isCanonicalSiteHost(`extra.${SITE_HOST}`), false);
  assert.equal(isCanonicalSiteHost(`${SITE_ID}.example.com`), false);
  assert.equal(isCanonicalZ32KeyLabel(`${SITE_ID.slice(0, -1)}b`), false);
  assert.equal(isCanonicalZ32KeyLabel(`${SITE_ID.slice(0, -1)}0`), false);
});

test("the public asset surface is an exact allowlist", () => {
  assert.deepEqual([...ASSETS.keys()], [
    "/.urspace/open/",
    "/.urspace/assets/main.js",
    "/.urspace/assets/socket-shim.js",
    "/.urspace/assets/style.css",
    "/sw.js",
    "/.urspace/wasm/urspace_browser.js",
    "/.urspace/wasm/urspace_browser_bg.wasm",
  ]);
});

test("serves an allowlisted asset with hardened headers", async () => {
  const { env, requests } = fixture();
  const result = await handleRequest(new Request(`https://${SITE_HOST}/sw.js`), env);

  assert.equal(result.status, 200);
  assert.equal(await result.text(), "asset:/sw.js");
  assert.equal(requests.length, 1);
  assert.equal(new URL(requests[0].url).pathname, "/sw.js");
  assert.equal(result.headers.get("content-type"), "text/javascript; charset=utf-8");
  assert.equal(result.headers.get("service-worker-allowed"), "/");
  assert.equal(result.headers.get("cache-control"), "no-store");
  assert.equal(result.headers.get("x-content-type-options"), "nosniff");
  assert.match(result.headers.get("content-security-policy"), /default-src 'none'/);
});

test("maps the public open path to its private index asset", async () => {
  const { env, requests } = fixture();
  const result = await handleRequest(
    new Request(`https://${SITE_HOST}/.urspace/open/?invite=secret#ignored`),
    env,
  );

  assert.equal(result.status, 200);
  assert.equal(new URL(requests[0].url).pathname, "/.urspace/open/index.html");
  assert.equal(new URL(requests[0].url).search, "");
});

test("maps legacy v2 paths to current Urspace assets", async () => {
  assert.equal(LEGACY_V2_ASSETS.has("/.medousa/open/"), true);
  const { env, requests } = fixture();
  const result = await handleRequest(
    new Request(`https://${SITE_HOST}/.medousa/open/#m2=redacted`),
    env,
  );

  assert.equal(result.status, 200);
  assert.equal(new URL(requests[0].url).pathname, "/.urspace/open/index.html");
});

test("rejects invalid hosts, methods, and paths before touching assets", async () => {
  for (const [request, expectedStatus] of [
    [new Request("https://bad.urspace.online/sw.js"), 421],
    [new Request(`https://${SITE_HOST}/index.html`), 404],
    [new Request(`https://${SITE_HOST}/sw.js`, { method: "POST" }), 405],
  ]) {
    const { env, requests } = fixture();
    const result = await handleRequest(request, env);
    assert.equal(result.status, expectedStatus);
    assert.equal(requests.length, 0);
  }
});

test("health check does not require a site hostname", async () => {
  const { env, requests } = fixture();
  const result = await handleRequest(new Request("https://urspace.online/healthz"), env);

  assert.equal(result.status, 200);
  assert.equal(await result.text(), "ok\n");
  assert.equal(requests.length, 0);
});

test("redirects plaintext requests to HTTPS", async () => {
  const { env, requests } = fixture();
  const result = await handleRequest(new Request(`http://${SITE_HOST}/sw.js`), env);

  assert.equal(result.status, 308);
  assert.equal(result.headers.get("location"), `https://${SITE_HOST}/sw.js`);
  assert.equal(requests.length, 0);
});

test("fails closed when an embedded asset is missing", async () => {
  const { env } = fixture({ assetStatus: 404 });
  const result = await handleRequest(new Request(`https://${SITE_HOST}/sw.js`), env);

  assert.equal(result.status, 500);
  assert.equal(await result.text(), "bootstrap asset unavailable\n");
});
