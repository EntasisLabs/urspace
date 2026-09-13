(() => {
  class MeshWebSocket extends EventTarget {
    static CONNECTING = 0;
    static OPEN = 1;
    static CLOSING = 2;
    static CLOSED = 3;

    constructor(rawUrl, protocols) {
      super();
      this.url = new URL(rawUrl, location.href).href;
      this.protocol = "";
      this.extensions = "";
      this.binaryType = "blob";
      this.bufferedAmount = 0;
      this.readyState = MeshWebSocket.CONNECTING;
      this.onopen = null;
      this.onmessage = null;
      this.onerror = null;
      this.onclose = null;
      const url = new URL(this.url);
      if (!['ws:', 'wss:'].includes(url.protocol) || url.host !== location.host) {
        throw new DOMException("Urspace sockets must be same-origin", "SecurityError");
      }
      if (protocols && (Array.isArray(protocols) ? protocols.length : String(protocols))) {
        throw new DOMException("WebSocket subprotocols are not supported yet", "NotSupportedError");
      }
      const channel = new MessageChannel();
      this._port = channel.port1;
      this._port.onmessage = ({ data }) => this._receive(data);
      navigator.serviceWorker.controller.postMessage(
        { type: "urspace-socket", path: `${url.pathname}${url.search}` },
        [channel.port2],
      );
    }

    _emit(event) {
      this.dispatchEvent(event);
      const handler = this[`on${event.type}`];
      if (typeof handler === "function") handler.call(this, event);
    }

    _receive(message) {
      if (message.type === "open") {
        this.readyState = MeshWebSocket.OPEN;
        this._emit(new Event("open"));
      } else if (message.type === "text") {
        this._emit(new MessageEvent("message", { data: message.data }));
      } else if (message.type === "binary") {
        const bytes = new Uint8Array(message.data);
        const data = this.binaryType === "arraybuffer" ? bytes.buffer : new Blob([bytes]);
        this._emit(new MessageEvent("message", { data }));
      } else if (message.type === "error") {
        this._emit(new Event("error"));
      } else if (message.type === "close") {
        this.readyState = MeshWebSocket.CLOSED;
        this._emit(new CloseEvent("close", {
          code: message.code || 1006,
          reason: message.reason || "",
          wasClean: Boolean(message.clean),
        }));
        this._port.close();
      }
    }

    send(data) {
      if (this.readyState !== MeshWebSocket.OPEN) throw new DOMException("Socket is not open", "InvalidStateError");
      if (typeof data === "string") {
        this._port.postMessage({ type: "text", data });
      } else if (data instanceof ArrayBuffer || ArrayBuffer.isView(data)) {
        const bytes = data instanceof ArrayBuffer
          ? new Uint8Array(data)
          : new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
        this._port.postMessage({ type: "binary", data: bytes });
      } else {
        throw new TypeError("Only string and binary WebSocket messages are supported");
      }
    }

    close(code = 1000, reason = "") {
      if (this.readyState >= MeshWebSocket.CLOSING) return;
      this.readyState = MeshWebSocket.CLOSING;
      this._port.postMessage({ type: "close", code, reason });
    }
  }

  for (const [name, value] of Object.entries({ CONNECTING: 0, OPEN: 1, CLOSING: 2, CLOSED: 3 })) {
    Object.defineProperty(MeshWebSocket.prototype, name, { value });
  }
  globalThis.WebSocket = MeshWebSocket;
})();
