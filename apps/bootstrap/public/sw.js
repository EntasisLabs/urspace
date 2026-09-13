import init, { SiteClient } from "/.urspace/wasm/urspace_browser.js";
import {
  authorizeResumeBootstrap,
  createResumeNonce,
  createResumePayload,
  injectResumeShim,
  injectSocketShim,
  parseResumePayload,
} from "/.urspace/assets/session.js";

const BOOTSTRAP_REVISION = "v8-iroh-session-grants-1";
const RESERVED_PREFIXES = ["/.urspace/", "/.medousa/"];
const MAX_BROWSER_REQUEST_BYTES = 16 * 1024 * 1024;
const RESUME_REQUEST_TIMEOUT_MS = 8000;
let client = null;
let wasmReady = null;
let resumePayload = "";
let recovery = null;

self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (event) => event.waitUntil(self.clients.claim()));

self.addEventListener("message", (event) => {
  if (["urspace-arm", "medousa-arm"].includes(event.data?.type)) {
    event.waitUntil(arm(event.data, event.ports[0]));
  } else if (event.data?.type === "urspace-resume-handoff") {
    acceptResumeHandoff(event);
  } else if (["urspace-socket", "medousa-socket"].includes(event.data?.type)) {
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
    const endpointSecret = connected.exportResumeKey();
    let nextResumePayload;
    try {
      nextResumePayload = createResumePayload(
        connected.resumeCredential || invitationUrl,
        endpointSecret,
      );
    } finally {
      endpointSecret.fill(0);
    }
    invitationUrl = "";
    client?.close();
    client = connected;
    resumePayload = nextResumePayload;
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

function acceptResumeHandoff(event) {
  if (!event.source || typeof event.source.url !== "string") return;
  let sourceOrigin;
  try {
    sourceOrigin = new URL(event.source.url).origin;
  } catch {
    return;
  }
  if (sourceOrigin !== self.location.origin || typeof event.data.payload !== "string") return;
  try {
    parseResumePayload(event.data.payload, self.location.origin);
    resumePayload = event.data.payload;
  } catch {
    // Ignore malformed or cross-origin handoffs. Recovery remains fail closed.
  }
}

async function ensureClient() {
  if (client) return client;
  recovery ||= recoverClient().finally(() => {
    recovery = null;
  });
  return recovery;
}

async function recoverClient() {
  let payload = resumePayload;
  if (!payload) {
    const windows = await self.clients.matchAll({ type: "window", includeUncontrolled: false });
    if (windows.length === 0) throw new Error("No live browser page can resume this session.");
    payload = await Promise.any(windows.map(requestResumePayload));
  }
  const session = parseResumePayload(payload, self.location.origin);
  try {
    wasmReady ||= init();
    await wasmReady;
    const connected = await SiteClient.resume(
      session.resumeCredential,
      session.sessionSecret,
      Math.floor(Date.now() / 1000),
      self.location.origin,
    );
    const sessionSecret = connected.exportResumeKey();
    let nextResumePayload;
    try {
      nextResumePayload = createResumePayload(
        connected.resumeCredential || session.resumeCredential,
        sessionSecret,
      );
    } finally {
      sessionSecret.fill(0);
    }
    client?.close();
    client = connected;
    resumePayload = nextResumePayload;
    return connected;
  } finally {
    session.resumeCredential = "";
    session.sessionSecret.fill(0);
  }
}

function requestResumePayload(windowClient) {
  return new Promise((resolve, reject) => {
    const channel = new MessageChannel();
    const timeout = setTimeout(() => {
      channel.port1.close();
      reject(new Error("A live browser page did not answer the resume request."));
    }, RESUME_REQUEST_TIMEOUT_MS);
    channel.port1.onmessage = ({ data }) => {
      if (data?.type !== "urspace-resume" || typeof data.payload !== "string") return;
      clearTimeout(timeout);
      channel.port1.close();
      resolve(data.payload);
    };
    windowClient.postMessage({ type: "urspace-resume-request" }, [channel.port2]);
  });
}

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  if (url.origin !== self.location.origin || url.pathname === "/sw.js" || RESERVED_PREFIXES.some((prefix) => url.pathname.startsWith(prefix))) {
    return;
  }
  event.respondWith(meshFetch(event.request));
});

async function meshFetch(request) {
  let activeClient;
  try {
    activeClient = await ensureClient();
  } catch {
    return disconnectedResponse();
  }
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
    const fetchFromMesh = () => activeClient.fetch(
      request.method,
      `${url.pathname}${url.search}`,
      headers,
      body,
    );
    let response;
    try {
      response = await fetchFromMesh();
    } catch (error) {
      if (!["GET", "HEAD"].includes(request.method)) throw error;
      response = await fetchFromMesh();
    }
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
      let injected = injectSocketShim(html);
      if (request.mode === "navigate" && resumePayload) {
        const nonce = createResumeNonce();
        const authorization = authorizeResumeBootstrap(
          responseHeaders.get("content-security-policy"),
          nonce,
        );
        if (authorization.allowed) {
          if (authorization.policy) {
            responseHeaders.set("content-security-policy", authorization.policy);
          }
          injected = injectResumeShim(html, resumePayload, nonce);
        }
      }
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
    <h1>Private connection closed</h1><p>No live Urspace page could resume this browser session. Reopen the invitation to reconnect.</p>`;
  return new Response(html, {
    status: 503,
    headers: { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" },
  });
}

async function openSocket(path, port) {
  let activeClient;
  try {
    activeClient = await ensureClient();
  } catch {
    port.postMessage({ type: "error" });
    port.postMessage({ type: "close", code: 1006, reason: "Private connection closed", clean: false });
    return;
  }
  try {
    const socket = await activeClient.openSocket(String(path || "/ws"));
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
