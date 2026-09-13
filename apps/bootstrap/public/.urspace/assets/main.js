const status = globalThis.document?.querySelector("#status");
const title = globalThis.document?.querySelector("h1");
const SHORT_LINK_PREFIX = "s1=";
const SHORT_LINK_SALT = new TextEncoder().encode("urspace-short-link-v1");

const PROGRESS_MESSAGES = {
  "loading-client": "Starting the encrypted browser client…",
  "connecting-relay": "Connecting to the Iroh relay…",
};

export function isSafari(userAgent) {
  return /Safari\//.test(userAgent) && !/(?:Chrome|Chromium|CriOS|Edg|OPR)\//.test(userAgent);
}

export function explainConnectionError(message, userAgent) {
  if (message.includes("Iroh relay connection timed out")) {
    if (isSafari(userAgent)) {
      return {
        title: "Safari couldn’t open the mesh",
        message: "This Safari build could not negotiate the Iroh relay. Reopen the original invitation in Chrome while Safari compatibility is being fixed.",
      };
    }
    return {
      title: "Couldn’t reach the private site",
      message: "The Iroh relay connection timed out. Confirm the host is still sharing, then reopen the invitation.",
    };
  }
  return { title: "Couldn’t open the private site", message };
}

function decodeBase64Url(value, expectedLength) {
  if (!/^[A-Za-z0-9_-]+$/.test(value)) throw new Error("The short invitation is malformed.");
  const padded = value
    .replace(/-/g, "+")
    .replace(/_/g, "/")
    .padEnd(Math.ceil(value.length / 4) * 4, "=");
  let bytes;
  try {
    bytes = Uint8Array.from(atob(padded), (char) => char.charCodeAt(0));
  } catch {
    throw new Error("The short invitation is malformed.");
  }
  if (bytes.length !== expectedLength) throw new Error("The short invitation is malformed.");
  if (encodeBase64Url(bytes) !== value) throw new Error("The short invitation is malformed.");
  return bytes;
}

function encodeBase64Url(bytes) {
  return btoa(String.fromCharCode(...bytes))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

function isCanonicalSiteLabel(label) {
  const alphabet = "ybndrfg8ejkmcpqxot1uwisza345h769";
  if (label.length !== 52) return false;
  let accumulator = 0;
  let bits = 0;
  let decodedBytes = 0;
  for (const character of label) {
    const digit = alphabet.indexOf(character);
    if (digit === -1) return false;
    accumulator = (accumulator << 5) | digit;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      decodedBytes += 1;
      accumulator &= (1 << bits) - 1;
    }
  }
  return decodedBytes === 32 && bits === 4 && accumulator === 0;
}

async function deriveShortLinkMaterial(seed) {
  const material = await crypto.subtle.importKey("raw", seed, "HKDF", false, ["deriveBits", "deriveKey"]);
  const parameters = (info) => ({
    name: "HKDF",
    hash: "SHA-256",
    salt: SHORT_LINK_SALT,
    info: new TextEncoder().encode(info),
  });
  const lookup = new Uint8Array(
    await crypto.subtle.deriveBits(parameters("lookup"), material, 128),
  );
  const key = await crypto.subtle.deriveKey(
    parameters("encryption"),
    material,
    { name: "AES-GCM", length: 256 },
    false,
    ["decrypt"],
  );
  return { lookup, key };
}

function validateExpandedInvite(raw, shortHostname) {
  let invite;
  try {
    invite = new URL(raw);
  } catch {
    throw new Error("The short invitation contained an invalid URL.");
  }
  const baseDomain = shortHostname.toLowerCase().startsWith("u.")
    ? shortHostname.toLowerCase().slice(2)
    : "";
  const suffix = "." + baseDomain;
  const siteLabel = invite.hostname.toLowerCase().endsWith(suffix)
    ? invite.hostname.slice(0, -suffix.length)
    : "";
  const validSiteLabel = !siteLabel.includes(".") && isCanonicalSiteLabel(siteLabel);
  const validPath =
    (invite.pathname === "/.urspace/open/" && invite.hash.startsWith("#u3=")) ||
    (invite.pathname === "/.medousa/open/" && invite.hash.startsWith("#m2="));
  if (
    invite.protocol !== "https:" ||
    invite.username ||
    invite.password ||
    invite.port ||
    invite.search ||
    !baseDomain ||
    !validSiteLabel ||
    !validPath
  ) {
    throw new Error("The short invitation did not resolve to this Urspace service.");
  }
  return invite.href;
}

export async function decryptShortInvite(seedEncoded, envelope, shortHostname, nowUnix) {
  const seed = decodeBase64Url(seedEncoded, 32);
  try {
    if (
      !envelope ||
      envelope.version !== 1 ||
      !Number.isSafeInteger(envelope.expiresAtUnix) ||
      envelope.expiresAtUnix <= nowUnix
    ) {
      throw new Error("The short invitation has expired or is invalid.");
    }
    const nonce = decodeBase64Url(envelope.nonce, 12);
    const ciphertext = decodeBase64Url(
      envelope.ciphertext,
      Math.floor((envelope.ciphertext.length * 6) / 8),
    );
    if (ciphertext.length < 17 || ciphertext.length > 4096) {
      throw new Error("The short invitation is malformed.");
    }
    const { lookup, key } = await deriveShortLinkMaterial(seed);
    const aad = new TextEncoder().encode(
      "urspace-short-link-v1:" + envelope.expiresAtUnix,
    );
    let plaintext;
    try {
      plaintext = await crypto.subtle.decrypt(
        { name: "AES-GCM", iv: nonce, additionalData: aad },
        key,
        ciphertext,
      );
    } catch {
      throw new Error("The short invitation could not be authenticated.");
    }
    const raw = new TextDecoder("utf-8", { fatal: true }).decode(plaintext);
    return {
      invitationUrl: validateExpandedInvite(raw, shortHostname),
      lookup: encodeBase64Url(lookup),
    };
  } finally {
    seed.fill(0);
  }
}

async function expandShortInvite(seedEncoded) {
  const seed = decodeBase64Url(seedEncoded, 32);
  let lookup;
  try {
    ({ lookup } = await deriveShortLinkMaterial(seed));
  } finally {
    seed.fill(0);
  }
  const lookupEncoded = encodeBase64Url(lookup);
  const response = await fetch("/api/short-links/" + lookupEncoded, {
    cache: "no-store",
    headers: { Accept: "application/json" },
  });
  if (response.status === 404) throw new Error("This short invitation does not exist.");
  if (response.status === 410) throw new Error("This short invitation has expired.");
  if (!response.ok) throw new Error("The short invitation service is unavailable.");
  const envelope = await response.json();
  return (
    await decryptShortInvite(
      seedEncoded,
      envelope,
      location.hostname,
      Math.floor(Date.now() / 1000),
    )
  ).invitationUrl;
}

function setStatus(message, failed = false, heading = "Opening a private site…") {
  if (!status || !title) return;
  title.textContent = heading;
  status.textContent = message;
  document.body.classList.toggle("failed", failed);
}

function waitForWorker(worker) {
  if (worker.state === "activated") return Promise.resolve();
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error("The site worker did not start.")), 15000);
    worker.addEventListener("statechange", () => {
      if (worker.state === "activated") {
        clearTimeout(timeout);
        resolve();
      }
      if (worker.state === "redundant") {
        clearTimeout(timeout);
        reject(new Error("The site worker could not be installed."));
      }
    });
  });
}

function armWorker(worker, invitationUrl, onProgress) {
  return new Promise((resolve, reject) => {
    const channel = new MessageChannel();
    const timeout = setTimeout(() => reject(new Error("The encrypted connection timed out.")), 45000);
    channel.port1.onmessage = ({ data }) => {
      if (data?.type === "progress") {
        const message = PROGRESS_MESSAGES[data.stage];
        if (message) onProgress(message);
        return;
      }
      clearTimeout(timeout);
      data?.ok ? resolve(data) : reject(new Error(data?.error || "The invitation was rejected."));
    };
    worker.postMessage(
      { type: "urspace-arm", invitationUrl, nowUnix: Math.floor(Date.now() / 1000) },
      [channel.port2],
    );
  });
}

async function openSite() {
  let invitationUrl = window.location.href;
  const shortSeed = location.hash.startsWith(`#${SHORT_LINK_PREFIX}`)
    ? location.hash.slice(SHORT_LINK_PREFIX.length + 1)
    : "";
  history.replaceState(null, "", `${location.pathname}${location.search}`);
  try {
    if (shortSeed) {
      setStatus("Decrypting the short invitation…");
      invitationUrl = await expandShortInvite(shortSeed);
      location.replace(invitationUrl);
      invitationUrl = "";
      return;
    }
    if (!("serviceWorker" in navigator)) throw new Error("This browser cannot run private sites.");
    setStatus("Preparing the private site boundary…");
    const registration = await navigator.serviceWorker.register("/sw.js", {
      scope: "/",
      type: "module",
      updateViaCache: "none",
    });
    await registration.update();
    const worker = registration.installing || registration.waiting || registration.active;
    if (!worker) throw new Error("The site worker is unavailable.");
    await waitForWorker(worker);
    setStatus("Connecting over the encrypted mesh…");
    const armed = await armWorker(
      registration.active || worker,
      invitationUrl,
      (message) => setStatus(message),
    );
    invitationUrl = "";
    location.replace(armed.entryPath || "/");
  } catch (error) {
    invitationUrl = "";
    const detail = explainConnectionError(
      error instanceof Error ? error.message : String(error),
      navigator.userAgent,
    );
    setStatus(detail.message, true, detail.title);
  }
}

if (globalThis.document && globalThis.navigator) void openSite();
