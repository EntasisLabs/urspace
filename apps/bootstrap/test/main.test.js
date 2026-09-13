import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { runInNewContext } from "node:vm";

import {
  decryptShortInvite,
  explainConnectionError,
  isSafari,
} from "../public/.urspace/assets/main.js";
import {
  RESUME_BOOTSTRAP_SOURCE,
  authorizeResumeBootstrap,
  createResumeNonce,
  createResumePayload,
  injectResumeShim,
  injectSocketShim,
  parseResumePayload,
} from "../public/.urspace/assets/session.js";

const SAFARI = "Mozilla/5.0 AppleWebKit/605.1.15 Version/26.0 Safari/605.1.15";
const CHROME = "Mozilla/5.0 AppleWebKit/537.36 Chrome/140.0.0.0 Safari/537.36";
const SHORT_LINK_VECTOR = JSON.parse(
  readFileSync(new URL("../../../testdata/short-link-v1.json", import.meta.url), "utf8"),
);

test("identifies Safari without mistaking Chromium for Safari", () => {
  assert.equal(isSafari(SAFARI), true);
  assert.equal(isSafari(CHROME), false);
});

test("turns Safari relay timeouts into an actionable message", () => {
  const detail = explainConnectionError(
    "Iroh relay connection timed out before the site could be reached",
    SAFARI,
  );

  assert.equal(detail.title, "Safari couldn’t open the mesh");
  assert.match(detail.message, /Reopen the original invitation in Chrome/);
});

test("keeps non-Safari relay and protocol failures distinct", () => {
  const relay = explainConnectionError(
    "Iroh relay connection timed out before the site could be reached",
    CHROME,
  );
  assert.equal(relay.title, "Couldn’t reach the private site");

  const rejected = explainConnectionError("invitation rejected: expired", SAFARI);
  assert.equal(rejected.title, "Couldn’t open the private site");
  assert.equal(rejected.message, "invitation rejected: expired");
});

test("decrypts Rust short-link envelopes without exposing the seed to HTTP", async () => {
  const result = await decryptShortInvite(
    SHORT_LINK_VECTOR.seed,
    {
      version: 1,
      expiresAtUnix: SHORT_LINK_VECTOR.expiresAtUnix,
      nonce: SHORT_LINK_VECTOR.nonce,
      ciphertext: SHORT_LINK_VECTOR.ciphertext,
    },
    "u.urspace.online",
    1_000_000_000,
  );

  assert.equal(result.lookup, SHORT_LINK_VECTOR.lookup);
  assert.equal(result.invitationUrl, SHORT_LINK_VECTOR.invitationUrl);
});

test("rejects tampered or cross-service short-link envelopes", async () => {
  const envelope = {
    version: 1,
    expiresAtUnix: SHORT_LINK_VECTOR.expiresAtUnix,
    nonce: SHORT_LINK_VECTOR.nonce,
    ciphertext: `${SHORT_LINK_VECTOR.ciphertext.slice(0, -1)}A`,
  };
  await assert.rejects(
    decryptShortInvite(
      SHORT_LINK_VECTOR.seed,
      envelope,
      "u.urspace.online",
      1_000_000_000,
    ),
    /could not be authenticated/,
  );

  await assert.rejects(
    decryptShortInvite(
      "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc",
      {
        ...envelope,
        ciphertext: SHORT_LINK_VECTOR.ciphertext,
      },
      "u.example.com",
      1_000_000_000,
    ),
    /did not resolve to this Urspace service/,
  );
});

test("round-trips a tab-scoped resume identity without exposing it in HTML text", () => {
  const endpointSecret = Uint8Array.from({ length: 32 }, (_, index) => index);
  const payload = createResumePayload(SHORT_LINK_VECTOR.invitationUrl, endpointSecret);
  const resumed = parseResumePayload(payload);

  assert.equal(resumed.invitationUrl, SHORT_LINK_VECTOR.invitationUrl);
  assert.deepEqual(resumed.endpointSecret, endpointSecret);
  assert.throws(
    () => parseResumePayload(payload, "https://other.urspace.online"),
    /resume session is malformed/,
  );

  const nonce = createResumeNonce({
    getRandomValues(bytes) {
      bytes.fill(7);
      return bytes;
    },
  });
  const html = injectResumeShim(
    "<!doctype html><html><head><title>BoxClub</title></head></html>",
    payload,
    nonce,
  );
  assert.match(html, new RegExp(`data-urspace-resume="${payload}"`));
  assert.match(html, new RegExp(`nonce="${nonce}"`));
  assert.match(html, /src="\/\.urspace\/assets\/socket-shim\.js"/);
  assert.equal(html.includes(SHORT_LINK_VECTOR.invitationUrl), false);
  assert.ok(html.indexOf("socket-shim.js") < html.indexOf("<html>"));
  assert.ok(html.indexOf("socket-shim.js") < html.indexOf("<title>"));
});

test("rejects malformed resume payloads and remote plaintext invitations", () => {
  const endpointSecret = new Uint8Array(32);
  assert.throws(() => parseResumePayload("not_json"), /resume session is malformed/);
  assert.throws(
    () => createResumePayload(
      "http://example.com/.urspace/open/#u3=opaque",
      endpointSecret,
    ),
    /resume session is malformed/,
  );
  assert.throws(
    () => createResumePayload(SHORT_LINK_VECTOR.invitationUrl, new Uint8Array(31)),
    /resume identity is invalid/,
  );
});

test("CSP authorization permits only the per-navigation resume nonce", () => {
  const nonce = "BwcHBwcHBwcHBwcHBwcHBw";
  assert.deepEqual(authorizeResumeBootstrap(null, nonce), { allowed: true, policy: null });
  assert.deepEqual(
    authorizeResumeBootstrap("default-src 'self'; script-src 'none'; object-src 'none'", nonce),
    {
      allowed: true,
      policy: `default-src 'self' 'nonce-${nonce}'; script-src 'none' 'nonce-${nonce}'; object-src 'none'`,
    },
  );
  assert.equal(
    authorizeResumeBootstrap("script-src 'self', default-src 'none'", nonce).allowed,
    false,
  );
  assert.equal(
    authorizeResumeBootstrap("script-src 'self'; script-src 'none'", nonce).allowed,
    false,
  );
  assert.equal(
    authorizeResumeBootstrap(
      "default-src 'none'; script-src 'self'; script-src-elem https://cdn.example",
      nonce,
    ).policy,
    `default-src 'none' 'nonce-${nonce}'; script-src 'self' 'nonce-${nonce}'; script-src-elem https://cdn.example 'nonce-${nonce}'`,
  );
  assert.match(injectSocketShim("<html></html>"), /^<script src=/);
});

test("the inline bootstrap hides its resume payload and answers only trusted worker messages", () => {
  const endpointSecret = new Uint8Array(32);
  const payload = createResumePayload(SHORT_LINK_VECTOR.invitationUrl, endpointSecret);
  let removed = false;
  let workerMessageHandler = null;

  class FakeMessagePort {
    postMessage(value) {
      this.sent = value;
    }

    close() {
      this.closed = true;
    }
  }

  const context = {
    document: {
      currentScript: {
        dataset: { urspaceResume: payload },
        remove() {
          removed = true;
        },
      },
    },
    navigator: {
      serviceWorker: {
        addEventListener(type, handler) {
          if (type === "message") workerMessageHandler = handler;
        },
      },
    },
    MessagePort: FakeMessagePort,
    EventTarget,
    DOMException,
    URL,
    Reflect,
  };
  runInNewContext(RESUME_BOOTSTRAP_SOURCE, context);

  assert.equal(removed, true);
  assert.equal(typeof workerMessageHandler, "function");

  const untrustedPort = new FakeMessagePort();
  workerMessageHandler({
    isTrusted: false,
    data: { type: "urspace-resume-request" },
    ports: [untrustedPort],
  });
  assert.equal(untrustedPort.sent, undefined);

  const trustedPort = new FakeMessagePort();
  workerMessageHandler({
    isTrusted: true,
    data: { type: "urspace-resume-request" },
    ports: [trustedPort],
  });
  assert.equal(trustedPort.sent.type, "urspace-resume");
  assert.equal(trustedPort.sent.payload, payload);
  assert.equal(trustedPort.closed, true);
});
