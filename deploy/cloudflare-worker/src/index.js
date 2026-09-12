const BASE_DOMAIN = "urspace.online";
const Z32_ALPHABET = "ybndrfg8ejkmcpqxot1uwisza345h769";

const ASSETS = new Map([
  ["/.medousa/open/", ["/.medousa/open/index.html", "text/html; charset=utf-8"]],
  ["/.medousa/assets/main.js", ["/.medousa/assets/main.js", "text/javascript; charset=utf-8"]],
  ["/.medousa/assets/socket-shim.js", ["/.medousa/assets/socket-shim.js", "text/javascript; charset=utf-8"]],
  ["/.medousa/assets/style.css", ["/.medousa/assets/style.css", "text/css; charset=utf-8"]],
  ["/sw.js", ["/sw.js", "text/javascript; charset=utf-8"]],
  ["/.medousa/wasm/medousa_site_browser.js", ["/.medousa/wasm/medousa_site_browser.js", "text/javascript; charset=utf-8"]],
  ["/.medousa/wasm/medousa_site_browser_bg.wasm", ["/.medousa/wasm/medousa_site_browser_bg.wasm", "application/wasm"]],
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
  const suffix = `.${BASE_DOMAIN}`;
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

async function handleRequest(request, env) {
  const url = new URL(request.url);

  if (url.protocol !== "https:") {
    url.protocol = "https:";
    return response(null, 308, { Location: url.toString() });
  }

  if (request.method !== "GET" && request.method !== "HEAD") {
    return response("method not allowed\n", 405, { Allow: "GET, HEAD" });
  }

  const body = request.method === "HEAD" ? null : "ok\n";
  if (url.pathname === "/healthz") {
    return response(body, 200, { "Content-Type": "text/plain; charset=utf-8" });
  }

  if (!isCanonicalSiteHost(url.hostname)) {
    return response("invalid site hostname\n", 421, { "Content-Type": "text/plain; charset=utf-8" });
  }

  const asset = ASSETS.get(url.pathname);
  if (!asset) {
    return response("not found\n", 404, { "Content-Type": "text/plain; charset=utf-8" });
  }

  const [assetPath, contentType] = asset;
  const assetUrl = new URL(assetPath, "https://assets.invalid");
  const assetResponse = await env.ASSETS.fetch(new Request(assetUrl, { method: request.method }));
  if (!assetResponse.ok) {
    return response("bootstrap asset unavailable\n", 500, { "Content-Type": "text/plain; charset=utf-8" });
  }

  const headers = new Headers(assetResponse.headers);
  for (const [name, value] of Object.entries(SECURITY_HEADERS)) headers.set(name, value);
  headers.set("Content-Type", contentType);
  if (url.pathname === "/sw.js") headers.set("Service-Worker-Allowed", "/");

  return new Response(request.method === "HEAD" ? null : assetResponse.body, {
    status: 200,
    headers,
  });
}

export { ASSETS, handleRequest, isCanonicalSiteHost, isCanonicalZ32KeyLabel };

export default {
  fetch: handleRequest,
};
