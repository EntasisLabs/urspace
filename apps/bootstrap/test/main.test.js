import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import {
  decryptShortInvite,
  explainConnectionError,
  isSafari,
} from "../public/.urspace/assets/main.js";

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
