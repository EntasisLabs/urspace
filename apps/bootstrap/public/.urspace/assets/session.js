const RESUME_VERSION = 2;
const MAX_RESUME_PAYLOAD_LENGTH = 32768;
const MAX_RESUME_CREDENTIAL_LENGTH = 16384;
const MAX_INVITATION_URL_LENGTH = 4096;
const RESUME_NONCE_BYTES = 16;

export const RESUME_BOOTSTRAP_SOURCE = `(()=>{let p="";const s=document.currentScript;try{const v=s?.dataset.urspaceResume||"";if(/^[A-Za-z0-9_-]{1,32768}$/.test(v))p=v}finally{s?.remove()}const m=MessagePort.prototype.postMessage,c=MessagePort.prototype.close,w=ServiceWorker.prototype.postMessage,h=()=>{const x=navigator.serviceWorker.controller;if(x&&p)Reflect.apply(w,x,[{type:"urspace-resume-handoff",payload:p}])};addEventListener("beforeunload",h,{capture:true});addEventListener("pagehide",h,{capture:true});document.addEventListener("visibilitychange",()=>{if(document.visibilityState==="hidden")h()},{capture:true});navigator.serviceWorker.addEventListener("message",e=>{if(!e.isTrusted||e.data?.type!=="urspace-resume-request"||!p||!e.ports[0])return;Reflect.apply(m,e.ports[0],[{type:"urspace-resume",payload:p}]);Reflect.apply(c,e.ports[0],[])})})();`;

function encodeBase64Url(bytes) {
  return btoa(String.fromCharCode(...bytes))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

function decodeBase64Url(value, expectedLength = null) {
  if (typeof value !== "string" || !/^[A-Za-z0-9_-]+$/.test(value)) {
    throw new Error("The browser resume session is malformed.");
  }
  const padded = value
    .replace(/-/g, "+")
    .replace(/_/g, "/")
    .padEnd(Math.ceil(value.length / 4) * 4, "=");
  let bytes;
  try {
    bytes = Uint8Array.from(atob(padded), (character) => character.charCodeAt(0));
  } catch {
    throw new Error("The browser resume session is malformed.");
  }
  if (expectedLength !== null && bytes.length !== expectedLength) {
    throw new Error("The browser resume session is malformed.");
  }
  if (encodeBase64Url(bytes) !== value) {
    throw new Error("The browser resume session is malformed.");
  }
  return bytes;
}

function validateInvitationUrl(raw, expectedOrigin = null) {
  if (typeof raw !== "string" || raw.length === 0 || raw.length > MAX_INVITATION_URL_LENGTH) {
    throw new Error("The browser resume session is malformed.");
  }
  let invitation;
  try {
    invitation = new URL(raw);
  } catch {
    throw new Error("The browser resume session is malformed.");
  }
  const validPath =
    (invitation.pathname === "/.urspace/open/" && invitation.hash.startsWith("#u3=")) ||
    (invitation.pathname === "/.urspace/open/" && invitation.hash.startsWith("#u4=")) ||
    (invitation.pathname === "/.medousa/open/" && invitation.hash.startsWith("#m2="));
  const validProtocol =
    invitation.protocol === "https:" ||
    (invitation.protocol === "http:" && invitation.hostname.endsWith(".localhost"));
  if (
    !validProtocol ||
    invitation.username ||
    invitation.password ||
    invitation.search ||
    (expectedOrigin !== null && invitation.origin !== expectedOrigin) ||
    !validPath
  ) {
    throw new Error("The browser resume session is malformed.");
  }
  return invitation.href;
}

function validateResumeCredential(raw, expectedOrigin = null) {
  if (
    typeof raw === "string" &&
    raw.length >= 16 &&
    raw.length <= MAX_RESUME_CREDENTIAL_LENGTH &&
    /^usg1\.[A-Za-z0-9_-]+$/.test(raw)
  ) {
    return raw;
  }
  return validateInvitationUrl(raw, expectedOrigin);
}

export function createResumePayload(resumeCredential, sessionSecret) {
  const secret = sessionSecret instanceof Uint8Array
    ? sessionSecret
    : new Uint8Array(sessionSecret);
  if (secret.byteLength !== 32) {
    throw new Error("The browser resume identity is invalid.");
  }
  const value = JSON.stringify({
    version: RESUME_VERSION,
    resumeCredential: validateResumeCredential(resumeCredential),
    sessionSecret: encodeBase64Url(secret),
  });
  return encodeBase64Url(new TextEncoder().encode(value));
}

export function parseResumePayload(payload, expectedOrigin = null) {
  if (typeof payload !== "string" || payload.length === 0 || payload.length > MAX_RESUME_PAYLOAD_LENGTH) {
    throw new Error("The browser resume session is malformed.");
  }
  let value;
  try {
    value = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(decodeBase64Url(payload)));
  } catch (error) {
    if (error instanceof Error && error.message === "The browser resume session is malformed.") {
      throw error;
    }
    throw new Error("The browser resume session is malformed.");
  }
  if (
    !value ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    ![1, RESUME_VERSION].includes(value.version)
  ) {
    throw new Error("The browser resume session is malformed.");
  }
  if (value.version === 1) {
    return {
      resumeCredential: validateInvitationUrl(value.invitationUrl, expectedOrigin),
      sessionSecret: decodeBase64Url(value.endpointSecret, 32),
    };
  }
  return {
    resumeCredential: validateResumeCredential(value.resumeCredential, expectedOrigin),
    sessionSecret: decodeBase64Url(value.sessionSecret, 32),
  };
}

export function createResumeNonce(cryptoApi = crypto) {
  return encodeBase64Url(cryptoApi.getRandomValues(new Uint8Array(RESUME_NONCE_BYTES)));
}

export function authorizeResumeBootstrap(contentSecurityPolicy, nonce) {
  if (!/^[A-Za-z0-9_-]{22}$/.test(nonce)) {
    throw new Error("The browser resume nonce is invalid.");
  }
  if (!contentSecurityPolicy?.trim()) {
    return { allowed: true, policy: contentSecurityPolicy };
  }
  // Combined policies and report URLs can both contain commas. Fail closed rather than
  // guessing how a browser split multiple CSP header fields.
  if (contentSecurityPolicy.includes(",")) {
    return { allowed: false, policy: contentSecurityPolicy };
  }

  const directives = contentSecurityPolicy
    .split(";")
    .map((directive) => directive.trim())
    .filter(Boolean);
  const positions = new Map();
  for (let index = 0; index < directives.length; index += 1) {
    const name = directives[index].trim().split(/\s+/, 1)[0].toLowerCase();
    if (!["script-src-elem", "script-src", "default-src"].includes(name)) continue;
    if (positions.has(name)) return { allowed: false, policy: contentSecurityPolicy };
    positions.set(name, index);
  }
  if (positions.size === 0) return { allowed: true, policy: contentSecurityPolicy };
  for (const index of positions.values()) {
    directives[index] = `${directives[index]} 'nonce-${nonce}'`;
  }
  return { allowed: true, policy: directives.join("; ") };
}

function prependToDocument(html, scripts) {
  return /^\s*<!doctype[^>]*>/i.test(html)
    ? html.replace(/^(\s*<!doctype[^>]*>)/i, (_, doctype) => `${doctype}${scripts}`)
    : `${scripts}${html}`;
}

export function injectSocketShim(html) {
  return prependToDocument(
    html,
    '<script src="/.urspace/assets/socket-shim.js"></script>',
  );
}

export function injectResumeShim(html, resumePayload, nonce) {
  parseResumePayload(resumePayload);
  if (!/^[A-Za-z0-9_-]{22}$/.test(nonce)) {
    throw new Error("The browser resume nonce is invalid.");
  }
  const bootstrap = `<script nonce="${nonce}" data-urspace-resume="${resumePayload}">${RESUME_BOOTSTRAP_SOURCE}</script>`;
  return prependToDocument(html, `${bootstrap}<script src="/.urspace/assets/socket-shim.js"></script>`);
}
