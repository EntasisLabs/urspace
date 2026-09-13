import assert from "node:assert/strict";
import test from "node:test";

import {
  ASSETS,
  LEGACY_V2_ASSETS,
  SHORT_ASSETS,
  SHORT_HOST,
  ShortLinkStore,
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

function shortLinkFixture({ createRateLimitSuccess = true, readRateLimitSuccess = true } = {}) {
  let record = null;
  const stub = {
    async fetch(input, init) {
      const request = input instanceof Request ? input : new Request(input, init);
      if (request.method === "PUT") {
        const next = await request.text();
        if (record && record !== next) return new Response(null, { status: 409 });
        const status = record ? 200 : 201;
        record = next;
        return new Response(null, { status });
      }
      return record
        ? new Response(record, { headers: { "Content-Type": "application/json" } })
        : new Response(null, { status: 404 });
    },
  };
  const base = fixture();
  return {
    ...base,
    env: {
      ...base.env,
      SHORT_LINKS: {
        idFromName: (name) => name,
        get: () => stub,
      },
      SHORT_LINK_CREATES: {
        limit: async () => ({ success: createRateLimitSuccess }),
      },
      SHORT_LINK_READS: {
        limit: async () => ({ success: readRateLimitSuccess }),
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
    "/.urspace/assets/session.js",
    "/.urspace/assets/socket-shim.js",
    "/.urspace/assets/style.css",
    "/sw.js",
    "/.urspace/wasm/urspace_browser.js",
    "/.urspace/wasm/urspace_browser_bg.wasm",
  ]);
});

test("the short-link host exposes only its resolver assets", async () => {
  assert.deepEqual([...SHORT_ASSETS.keys()], [
    "/",
    "/.urspace/assets/main.js",
    "/.urspace/assets/style.css",
  ]);
  const { env, requests } = fixture();
  const page = await handleRequest(new Request(`https://${SHORT_HOST}/`), env);
  assert.equal(page.status, 200);
  assert.equal(new URL(requests[0].url).pathname, "/.urspace/open/index.html");
  const worker = await handleRequest(new Request(`https://${SHORT_HOST}/sw.js`), env);
  assert.equal(worker.status, 404);
});

test("stores and returns only bounded encrypted short-link envelopes", async () => {
  const { env } = shortLinkFixture();
  const lookup = "cBr_kIlOrtVJaefP5s3yIg";
  const endpoint = `https://${SHORT_HOST}/api/short-links/${lookup}`;
  const envelope = {
    version: 1,
    expiresAtUnix: Math.floor(Date.now() / 1000) + 3600,
    nonce: "CQkJCQkJCQkJCQkJ",
    ciphertext: "AAAAAAAAAAAAAAAAAAAAAAA",
  };
  const created = await handleRequest(
    new Request(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json", "CF-Connecting-IP": "192.0.2.10" },
      body: JSON.stringify(envelope),
    }),
    env,
  );
  assert.equal(created.status, 201);
  assert.equal(created.headers.get("cache-control"), "no-store");

  const fetched = await handleRequest(new Request(endpoint), env);
  assert.equal(fetched.status, 200);
  assert.deepEqual(await fetched.json(), envelope);
});

test("rejects lookup collisions without replacing the original envelope", async () => {
  const { env } = shortLinkFixture();
  const endpoint = `https://${SHORT_HOST}/api/short-links/cBr_kIlOrtVJaefP5s3yIg`;
  const expiresAtUnix = Math.floor(Date.now() / 1000) + 3600;
  const envelope = {
    version: 1,
    expiresAtUnix,
    nonce: "CQkJCQkJCQkJCQkJ",
    ciphertext: "AAAAAAAAAAAAAAAAAAAAAAA",
  };
  const replacement = { ...envelope, ciphertext: "AQEBAQEBAQEBAQEBAQEBAQE" };

  for (const [body, expectedStatus] of [
    [envelope, 201],
    [replacement, 409],
  ]) {
    const result = await handleRequest(
      new Request(endpoint, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
      }),
      env,
    );
    assert.equal(result.status, expectedStatus);
  }

  const fetched = await handleRequest(new Request(endpoint), env);
  assert.deepEqual(await fetched.json(), envelope);
});

test("rate limits short-link reads before returning an envelope", async () => {
  const { env } = shortLinkFixture({ readRateLimitSuccess: false });
  const endpoint = `https://${SHORT_HOST}/api/short-links/cBr_kIlOrtVJaefP5s3yIg`;
  const result = await handleRequest(new Request(endpoint), env);
  assert.equal(result.status, 429);
  assert.equal(result.headers.get("retry-after"), "60");
});

test("rejects invalid, oversized, and rate-limited short-link writes", async () => {
  const lookup = "cBr_kIlOrtVJaefP5s3yIg";
  const endpoint = `https://${SHORT_HOST}/api/short-links/${lookup}`;
  const invalid = await handleRequest(
    new Request(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ version: 1, expiresAtUnix: 1, nonce: "bad", ciphertext: "bad" }),
    }),
    shortLinkFixture().env,
  );
  assert.equal(invalid.status, 400);

  const oversized = await handleRequest(
    new Request(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "x".repeat(9000),
    }),
    shortLinkFixture().env,
  );
  assert.equal(oversized.status, 400);

  const limited = await handleRequest(
    new Request(endpoint, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: "{}",
    }),
    shortLinkFixture({ createRateLimitSuccess: false }).env,
  );
  assert.equal(limited.status, 429);
});

test("durable short-link records expire and alarms erase storage", async () => {
  const values = new Map();
  let alarm = null;
  const storage = {
    get: async (key) => values.get(key),
    put: async (key, value) => values.set(key, value),
    setAlarm: async (value) => {
      alarm = value;
    },
    deleteAll: async () => {
      values.clear();
      alarm = null;
    },
  };
  const durable = new ShortLinkStore({ storage });
  const expiresAtUnix = Math.floor(Date.now() / 1000) + 60;
  const created = await durable.fetch(
    new Request("https://short-link.internal/record", {
      method: "PUT",
      body: JSON.stringify({ expiresAtUnix, ciphertext: "opaque" }),
    }),
  );
  assert.equal(created.status, 201);
  assert.equal(alarm, expiresAtUnix * 1000);
  assert.equal((await durable.fetch(new Request("https://short-link.internal/record"))).status, 200);
  await durable.alarm();
  assert.equal((await durable.fetch(new Request("https://short-link.internal/record"))).status, 404);
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
