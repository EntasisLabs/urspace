import assert from "node:assert/strict";
import test from "node:test";

import { explainConnectionError, isSafari } from "../public/.medousa/assets/main.js";

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
