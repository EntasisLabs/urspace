const BASE_DOMAIN = "urspace.online";
const SHORT_HOST = "u." + BASE_DOMAIN;
const Z32_ALPHABET = "ybndrfg8ejkmcpqxot1uwisza345h769";
const SHORT_LINK_PATH = /^\/api\/short-links\/([A-Za-z0-9_-]{22})$/;
const MAX_SHORT_LINK_BODY_BYTES = 8192;
const MAX_SHORT_LINK_TTL_SECONDS = 7 * 24 * 60 * 60;

const ASSETS = new Map([
  ["/.urspace/open/", ["/.urspace/open/index.html", "text/html; charset=utf-8"]],
  ["/.urspace/assets/main.js", ["/.urspace/assets/main.js", "text/javascript; charset=utf-8"]],
  ["/.urspace/assets/session.js", ["/.urspace/assets/session.js", "text/javascript; charset=utf-8"]],
  ["/.urspace/assets/socket-shim.js", ["/.urspace/assets/socket-shim.js", "text/javascript; charset=utf-8"]],
  ["/.urspace/assets/style.css", ["/.urspace/assets/style.css", "text/css; charset=utf-8"]],
  ["/sw.js", ["/sw.js", "text/javascript; charset=utf-8"]],
  ["/.urspace/wasm/urspace_browser.js", ["/.urspace/wasm/urspace_browser.js", "text/javascript; charset=utf-8"]],
  ["/.urspace/wasm/urspace_browser_bg.wasm", ["/.urspace/wasm/urspace_browser_bg.wasm", "application/wasm"]],
]);

const SHORT_ASSETS = new Map([
  ["/", ["/.urspace/open/index.html", "text/html; charset=utf-8"]],
  ["/.urspace/assets/main.js", ["/.urspace/assets/main.js", "text/javascript; charset=utf-8"]],
  ["/.urspace/assets/style.css", ["/.urspace/assets/style.css", "text/css; charset=utf-8"]],
]);

// v0.1 links request these paths. All aliases resolve to current Urspace assets.
const LEGACY_V2_ASSETS = new Map([
  ["/.medousa/open/", ["/.urspace/open/index.html", "text/html; charset=utf-8"]],
  ["/.medousa/assets/main.js", ["/.urspace/assets/main.js", "text/javascript; charset=utf-8"]],
  ["/.medousa/assets/socket-shim.js", ["/.urspace/assets/socket-shim.js", "text/javascript; charset=utf-8"]],
  ["/.medousa/assets/style.css", ["/.urspace/assets/style.css", "text/css; charset=utf-8"]],
  ["/.medousa/wasm/medousa_site_browser.js", ["/.urspace/wasm/urspace_browser.js", "text/javascript; charset=utf-8"]],
  ["/.medousa/wasm/medousa_site_browser_bg.wasm", ["/.urspace/wasm/urspace_browser_bg.wasm", "application/wasm"]],
  ["/.medousa/wasm/urspace_browser.js", ["/.urspace/wasm/urspace_browser.js", "text/javascript; charset=utf-8"]],
  ["/.medousa/wasm/urspace_browser_bg.wasm", ["/.urspace/wasm/urspace_browser_bg.wasm", "application/wasm"]],
]);

const SECURITY_HEADERS = {
  "Cache-Control": "no-store",
  "Content-Security-Policy": "default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; connect-src 'self' https: wss:; style-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
  "Cross-Origin-Opener-Policy": "same-origin",
  "Cross-Origin-Resource-Policy": "same-origin",
  "Permissions-Policy": "camera=(), microphone=(), geolocation=()",
  "Referrer-Policy": "no-referrer",
  "Strict-Transport-Security": "max-age=31536000; includeSubDomains",
  "X-Content-Type-Options": "nosniff",
  "X-Frame-Options": "DENY",
  "X-Robots-Tag": "noindex, nofollow, noarchive",
};

export class ShortLinkStore {
  constructor(ctx) {
    this.ctx = ctx;
  }

  async fetch(request) {
    if (new URL(request.url).pathname !== "/record") return new Response(null, { status: 404 });
    if (request.method === "PUT") {
      const record = await request.text();
      const parsed = JSON.parse(record);
      const existing = await this.ctx.storage.get("record");
      if (existing && existing !== record) return new Response(null, { status: 409 });
      await this.ctx.storage.put("record", record);
      await this.ctx.storage.setAlarm(parsed.expiresAtUnix * 1000);
      return new Response(null, { status: existing ? 200 : 201 });
    }
    if (request.method === "GET") {
      const record = await this.ctx.storage.get("record");
      if (!record) return new Response(null, { status: 404 });
      const parsed = JSON.parse(record);
      if (parsed.expiresAtUnix <= Math.floor(Date.now() / 1000)) {
        await this.ctx.storage.deleteAll();
        return new Response(null, { status: 410 });
      }
      return new Response(record, {
        headers: { "Content-Type": "application/json; charset=utf-8", "Cache-Control": "no-store" },
      });
    }
    return new Response(null, { status: 405 });
  }

  async alarm() {
    await this.ctx.storage.deleteAll();
  }
}

function isCanonicalZ32KeyLabel(label) {
  if (label.length !== 52) return false;
  let accumulator = 0;
  let bits = 0;
  let decodedBytes = 0;
  for (const char of label) {
    const digit = Z32_ALPHABET.indexOf(char);
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

function isCanonicalSiteHost(hostname) {
  const hostnameWithoutDot = hostname.toLowerCase().replace(/\.$/, "");
  const suffix = "." + BASE_DOMAIN;
  if (!hostnameWithoutDot.endsWith(suffix)) return false;
  const label = hostnameWithoutDot.slice(0, -suffix.length);
  return !label.includes(".") && isCanonicalZ32KeyLabel(label);
}

function response(body, status, extraHeaders = {}) {
  return new Response(body, {
    status,
    headers: { ...SECURITY_HEADERS, ...extraHeaders },
  });
}

function decodeCanonicalBase64Url(value, expectedBytes) {
  if (!/^[A-Za-z0-9_-]+$/.test(value)) return null;
  try {
    const padded = value.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(value.length / 4) * 4, "=");
    const bytes = Uint8Array.from(atob(padded), (char) => char.charCodeAt(0));
    if (bytes.length !== expectedBytes) return null;
    const canonical = btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
    return canonical === value ? bytes : null;
  } catch {
    return null;
  }
}

async function readBodyLimited(request, limit) {
  if (!request.body) return new Uint8Array();
  const reader = request.body.getReader();
  const chunks = [];
  let length = 0;
  while (true) {
    const { value, done } = await reader.read();
    if (done) break;
    length += value.byteLength;
    if (length > limit) {
      await reader.cancel();
      throw new Error("body too large");
    }
    chunks.push(value);
  }
  const body = new Uint8Array(length);
  let offset = 0;
  for (const chunk of chunks) {
    body.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return body;
}

function validateShortLinkEnvelope(value, nowUnix) {
  if (!value || typeof value !== "object" || Array.isArray(value)) return null;
  const { version, expiresAtUnix, nonce, ciphertext } = value;
  if (version !== 1 || !Number.isSafeInteger(expiresAtUnix)) return null;
  if (expiresAtUnix <= nowUnix || expiresAtUnix > nowUnix + MAX_SHORT_LINK_TTL_SECONDS) return null;
  if (typeof nonce !== "string" || !decodeCanonicalBase64Url(nonce, 12)) return null;
  if (typeof ciphertext !== "string" || ciphertext.length < 23 || ciphertext.length > 5500) return null;
  const estimatedBytes = Math.floor((ciphertext.length * 6) / 8);
  const ciphertextBytes = decodeCanonicalBase64Url(ciphertext, estimatedBytes);
  if (!ciphertextBytes || ciphertextBytes.length < 17 || ciphertextBytes.length > 4096) return null;
  return { version, expiresAtUnix, nonce, ciphertext };
}

function shortLinkStub(env, lookup) {
  if (!env.SHORT_LINKS) return null;
  return env.SHORT_LINKS.get(env.SHORT_LINKS.idFromName(lookup));
}

async function handleShortLinkApi(request, env, lookup) {
  if (!decodeCanonicalBase64Url(lookup, 16)) {
    return response("not found\n", 404, { "Content-Type": "text/plain; charset=utf-8" });
  }
  const stub = shortLinkStub(env, lookup);
  if (!stub) {
    return response("short-link storage unavailable\n", 503, {
      "Content-Type": "text/plain; charset=utf-8",
    });
  }

  if (request.method === "GET") {
    if (env.SHORT_LINK_READS) {
      const actor = request.headers.get("CF-Connecting-IP") || "unknown";
      const { success } = await env.SHORT_LINK_READS.limit({ key: actor });
      if (!success) {
        return response("short-link retrieval rate exceeded\n", 429, {
          "Content-Type": "text/plain; charset=utf-8",
          "Retry-After": "60",
        });
      }
    }
    const stored = await stub.fetch("https://short-link.internal/record");
    if (!stored.ok) {
      const status = stored.status === 404 || stored.status === 410 ? stored.status : 502;
      const message =
        status === 410
          ? "short link expired\n"
          : status === 404
            ? "short link not found\n"
            : "short-link storage failed\n";
      return response(message, status, {
        "Content-Type": "text/plain; charset=utf-8",
      });
    }
    return response(await stored.text(), 200, {
      "Content-Type": "application/json; charset=utf-8",
    });
  }

  if (request.method !== "POST") {
    return response("method not allowed\n", 405, {
      Allow: "GET, POST",
      "Content-Type": "text/plain; charset=utf-8",
    });
  }
  if (!request.headers.get("content-type")?.toLowerCase().startsWith("application/json")) {
    return response("content type must be application/json\n", 415, {
      "Content-Type": "text/plain; charset=utf-8",
    });
  }
  if (env.SHORT_LINK_CREATES) {
    const actor = request.headers.get("CF-Connecting-IP") || "unknown";
    const { success } = await env.SHORT_LINK_CREATES.limit({ key: actor });
    if (!success) {
      return response("short-link creation rate exceeded\n", 429, {
        "Content-Type": "text/plain; charset=utf-8",
        "Retry-After": "60",
      });
    }
  }

  let parsed;
  try {
    const bytes = await readBodyLimited(request, MAX_SHORT_LINK_BODY_BYTES);
    const text = new TextDecoder("utf-8", { fatal: true }).decode(bytes);
    parsed = JSON.parse(text);
  } catch {
    return response("invalid short-link envelope\n", 400, {
      "Content-Type": "text/plain; charset=utf-8",
    });
  }
  const envelope = validateShortLinkEnvelope(parsed, Math.floor(Date.now() / 1000));
  if (!envelope) {
    return response("invalid short-link envelope\n", 400, {
      "Content-Type": "text/plain; charset=utf-8",
    });
  }

  const stored = await stub.fetch("https://short-link.internal/record", {
    method: "PUT",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(envelope),
  });
  if (stored.status === 409) {
    return response("short-link collision\n", 409, {
      "Content-Type": "text/plain; charset=utf-8",
    });
  }
  if (!stored.ok) {
    return response("short-link storage failed\n", 502, {
      "Content-Type": "text/plain; charset=utf-8",
    });
  }
  return response(null, stored.status === 200 ? 200 : 201, {
    Location: "https://" + SHORT_HOST + "/",
  });
}

async function serveAsset(request, env, asset) {
  const [assetPath, contentType] = asset;
  const assetUrl = new URL(assetPath, "https://assets.invalid");
  const assetResponse = await env.ASSETS.fetch(new Request(assetUrl, { method: request.method }));
  if (!assetResponse.ok) {
    return response("bootstrap asset unavailable\n", 500, {
      "Content-Type": "text/plain; charset=utf-8",
    });
  }

  const headers = new Headers(assetResponse.headers);
  for (const [name, value] of Object.entries(SECURITY_HEADERS)) headers.set(name, value);
  headers.set("Content-Type", contentType);
  if (new URL(request.url).pathname === "/sw.js") headers.set("Service-Worker-Allowed", "/");
  return new Response(request.method === "HEAD" ? null : assetResponse.body, {
    status: 200,
    headers,
  });
}

async function handleRequest(request, env) {
  const url = new URL(request.url);
  if (url.protocol !== "https:") {
    url.protocol = "https:";
    return response(null, 308, { Location: url.toString() });
  }

  if (url.pathname === "/healthz") {
    if (request.method !== "GET" && request.method !== "HEAD") {
      return response("method not allowed\n", 405, { Allow: "GET, HEAD" });
    }
    const body = request.method === "HEAD" ? null : "ok\n";
    return response(body, 200, { "Content-Type": "text/plain; charset=utf-8" });
  }

  if (url.hostname.toLowerCase() === SHORT_HOST) {
    const shortLinkMatch = SHORT_LINK_PATH.exec(url.pathname);
    if (shortLinkMatch && !url.search) return handleShortLinkApi(request, env, shortLinkMatch[1]);
    if (request.method !== "GET" && request.method !== "HEAD") {
      return response("method not allowed\n", 405, { Allow: "GET, HEAD" });
    }
    const shortAsset = SHORT_ASSETS.get(url.pathname);
    if (!shortAsset) {
      return response("not found\n", 404, { "Content-Type": "text/plain; charset=utf-8" });
    }
    return serveAsset(request, env, shortAsset);
  }

  if (request.method !== "GET" && request.method !== "HEAD") {
    return response("method not allowed\n", 405, { Allow: "GET, HEAD" });
  }
  if (!isCanonicalSiteHost(url.hostname)) {
    return response("invalid site hostname\n", 421, {
      "Content-Type": "text/plain; charset=utf-8",
    });
  }
  const asset = ASSETS.get(url.pathname) || LEGACY_V2_ASSETS.get(url.pathname);
  if (!asset) {
    return response("not found\n", 404, { "Content-Type": "text/plain; charset=utf-8" });
  }
  return serveAsset(request, env, asset);
}

export {
  ASSETS,
  LEGACY_V2_ASSETS,
  SHORT_ASSETS,
  SHORT_HOST,
  handleRequest,
  isCanonicalSiteHost,
  isCanonicalZ32KeyLabel,
  validateShortLinkEnvelope,
};

export default {
  fetch: handleRequest,
};
