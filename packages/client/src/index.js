import init, { SiteClient } from "../wasm/urspace_browser.js";

let initialized = null;

function clock() {
  return Math.floor(Date.now() / 1000);
}

/**
 * Load the WASM client. Safe to call more than once.
 * @param {RequestInfo | URL | undefined} wasmUrl
 */
export async function ready(wasmUrl) {
  if (!initialized) {
    initialized = init(wasmUrl).then(() => undefined);
  }
  await initialized;
}

/**
 * Generate an ephemeral session keypair. Keep `secretKey` in memory.
 */
export function generateSessionKey() {
  const secretKey = SiteClient.generateSessionKey();
  const publicKey = SiteClient.sessionPublicKey(secretKey);
  return { secretKey, publicKey };
}

/**
 * Connect with a bearer invitation URL.
 * @param {string} invitationUrl
 */
export async function connect(invitationUrl) {
  await ready();
  return SiteClient.connect(invitationUrl, clock());
}

/**
 * Connect with a subject-bound mint token and the matching secret key.
 * @param {string} token
 * @param {Uint8Array} secretKey
 */
export async function connectMinted(token, secretKey) {
  await ready();
  return SiteClient.connectMinted(token, secretKey, clock());
}

/**
 * Resume an admitted session from a host-signed grant and its proof key.
 * @param {string} sessionGrant
 * @param {Uint8Array} secretKey
 * @param {string} expectedOrigin
 */
export async function resume(sessionGrant, secretKey, expectedOrigin) {
  await ready();
  return SiteClient.resume(sessionGrant, secretKey, clock(), expectedOrigin);
}

export { SiteClient };
