import assert from "node:assert/strict";
import test from "node:test";

import {
  decryptShortInvite,
  explainConnectionError,
  isSafari,
} from "../public/.urspace/assets/main.js";

const SAFARI = "Mozilla/5.0 AppleWebKit/605.1.15 Version/26.0 Safari/605.1.15";
const CHROME = "Mozilla/5.0 AppleWebKit/537.36 Chrome/140.0.0.0 Safari/537.36";

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
  const invitationUrl =
    "https://3mied18mppzo5rm16uzw5s6rxakceay3snhimxph3yjqi5tkdy8o.urspace.online/.urspace/open/#u3=fixture";
  const result = await decryptShortInvite(
    "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc",
    {
      version: 1,
      expiresAtUnix: 2_000_000_000,
      nonce: "CQkJCQkJCQkJCQkJ",
      ciphertext:
        "mf0-gJGTa1ppw25HRCQxKBIwIdVgvWUKj-JJosiiYT-RbWfShCxhUweiI4fp4rXwdV64Mm86dyvI7O6jUeuBsKOFeZve7Wv9UauF5yccIpcHJnnjav7maC_oM-7HaK8dWywStrY7WHsuWc8UIvTK6QPIrje-",
    },
    "u.urspace.online",
    1_000_000_000,
  );

  assert.equal(result.lookup, "cBr_kIlOrtVJaefP5s3yIg");
  assert.equal(result.invitationUrl, invitationUrl);
});

test("rejects tampered or cross-service short-link envelopes", async () => {
  const envelope = {
    version: 1,
    expiresAtUnix: 2_000_000_000,
    nonce: "CQkJCQkJCQkJCQkJ",
    ciphertext:
      "mf0-gJGTa1ppw25HRCQxKBIwIdVgvWUKj-JJosiiYT-RbWfShCxhUweiI4fp4rXwdV64Mm86dyvI7O6jUeuBsKOFeZve7Wv9UauF5yccIpcHJnnjav7maC_oM-7HaK8dWywStrY7WHsuWc8UIvTK6QPIrjeA",
  };
  await assert.rejects(
    decryptShortInvite(
      "BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc",
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
        ciphertext:
          "mf0-gJGTa1ppw25HRCQxKBIwIdVgvWUKj-JJosiiYT-RbWfShCxhUweiI4fp4rXwdV64Mm86dyvI7O6jUeuBsKOFeZve7Wv9UauF5yccIpcHJnnjav7maC_oM-7HaK8dWywStrY7WHsuWc8UIvTK6QPIrje-",
      },
      "u.example.com",
      1_000_000_000,
    ),
    /did not resolve to this Urspace service/,
  );
});
