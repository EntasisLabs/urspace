import init, { SiteClient } from "/.medousa/wasm/medousa_site_browser.js";

const BOOTSTRAP_REVISION = "v3-connection-diagnostics-1";
const RESERVED_PREFIX = "/.medousa/";
const MAX_BROWSER_REQUEST_BYTES = 16 * 1024 * 1024;
let client = null;
let wasmReady = null;

self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (event) => event.waitUntil(self.clients.claim()));

self.addEventListener("message", (event) => {
  if (event.data?.type === "medousa-arm") {
    event.waitUntil(arm(event.data, event.ports[0]));
  } else if (event.data?.type === "medousa-socket") {
    event.waitUntil(openSocket(event.data.path, event.ports[0]));
  }
});

async function arm(message, port) {
  let invitationUrl = String(message.invitationUrl || "");
  try {
    port.postMessage({ type: "progress", stage: "loading-client" });
    wasmReady ||= init();
    await wasmReady;
    port.postMessage({ type: "progress", stage: "connecting-relay" });
    const connected = await SiteClient.connect(invitationUrl, Number(message.nowUnix));
    invitationUrl = "";
    client?.close();
    client = connected;
    port.postMessage({
      ok: true,
      entryPath: connected.entryPath,
      siteId: connected.siteId,
      bootstrapRevision: BOOTSTRAP_REVISION,
    });
  } catch (error) {
    invitationUrl = "";
    port.postMessage({ ok: false, error: error instanceof Error ? error.message : String(error) });
  } finally {
    port.close();
  }
}

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  if (url.origin !== self.location.origin || url.pathname === "/sw.js" || url.pathname.startsWith(RESERVED_PREFIX)) {
    return;
  }
  event.respondWith(meshFetch(event.request));
});

async function meshFetch(request) {
  if (!client) return disconnectedResponse();
  try {
    let body = new Uint8Array();
    if (!["GET", "HEAD"].includes(request.method)) {
      const buffer = await request.arrayBuffer();
      if (buffer.byteLength > MAX_BROWSER_REQUEST_BYTES) {
        return new Response("Request body is too large", { status: 413 });
      }
      body = new Uint8Array(buffer);
    }
    const headers = [];
    request.headers.forEach((value, name) => headers.push({ name, value }));
    const url = new URL(request.url);
    const response = await client.fetch(
      request.method,
      `${url.pathname}${url.search}`,
      headers,
      body,
    );
    const responseHeaders = new Headers();
    for (const header of response.headers || []) {
      if (!["content-length", "content-encoding", "transfer-encoding"].includes(header.name.toLowerCase())) {
        responseHeaders.append(header.name, header.value);
      }
    }
    if (response.content_type && !responseHeaders.has("content-type")) {
      responseHeaders.set("content-type", response.content_type);
    }
    responseHeaders.set("cache-control", "no-store");
    responseHeaders.delete("etag");
    let bytes = new Uint8Array(response.body);
    if (response.status === 200 && response.content_type?.toLowerCase().startsWith("text/html")) {
      const html = new TextDecoder().decode(bytes);
      const shim = '<script src="/.medousa/assets/socket-shim.js"></script>';
      const injected = /<head(?:\s[^>]*)?>/i.test(html)
        ? html.replace(/<head(?:\s[^>]*)?>/i, (head) => `${head}${shim}`)
        : `${shim}${html}`;
      bytes = new TextEncoder().encode(injected);
    }
    const nullBody = request.method === "HEAD" || [101, 204, 205, 304].includes(response.status);
    return new Response(nullBody ? null : bytes, {
      status: response.status,
      headers: responseHeaders,
    });
  } catch (error) {
    return new Response(`Private site request failed: ${error instanceof Error ? error.message : String(error)}`, {
      status: 502,
      headers: { "content-type": "text/plain; charset=utf-8" },
    });
  }
}

function disconnectedResponse() {
  const html = `<!doctype html><meta charset="utf-8"><title>Reconnect private site</title>
    <style>body{font:16px system-ui;max-width:40rem;margin:15vh auto;padding:2rem;background:#090b0c;color:#e7ece8}a{color:#79e2a7}</style>
    <h1>Private connection closed</h1><p>Reopen the original invitation URL to reconnect. The capability was not saved to browser storage.</p>`;
  return new Response(html, {
    status: 503,
    headers: { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" },
  });
}

async function openSocket(path, port) {
  if (!client) {
    port.postMessage({ type: "error" });
    port.postMessage({ type: "close", code: 1006, reason: "Private connection closed", clean: false });
    return;
  }
  try {
    const socket = await client.openSocket(String(path || "/ws"));
    port.postMessage({ type: "open" });
    port.onmessage = async ({ data }) => {
      try {
        if (data.type === "text") await socket.sendText(String(data.data));
        if (data.type === "binary") await socket.sendBinary(new Uint8Array(data.data));
        if (data.type === "close") await socket.close(Number(data.code) || 1000, String(data.reason || ""));
      } catch {
        port.postMessage({ type: "error" });
      }
    };
    while (true) {
      const message = await socket.receive();
      if (Object.hasOwn(message, "Text")) {
        port.postMessage({ type: "text", data: message.Text });
      } else if (Object.hasOwn(message, "Binary")) {
        port.postMessage({ type: "binary", data: new Uint8Array(message.Binary) });
      } else if (Object.hasOwn(message, "Close")) {
        port.postMessage({
          type: "close",
          code: message.Close.code || 1000,
          reason: message.Close.reason || "",
          clean: true,
        });
        break;
      }
    }
  } catch {
    port.postMessage({ type: "error" });
    port.postMessage({ type: "close", code: 1006, reason: "Mesh socket failed", clean: false });
  } finally {
    port.close();
  }
}
