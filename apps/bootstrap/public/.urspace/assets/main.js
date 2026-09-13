const status = globalThis.document?.querySelector("#status");
const title = globalThis.document?.querySelector("h1");

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
  history.replaceState(null, "", `${location.pathname}${location.search}`);
  try {
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
