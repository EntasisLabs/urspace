const status = document.querySelector("#status");

function setStatus(message, failed = false) {
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

function armWorker(worker, invitationUrl) {
  return new Promise((resolve, reject) => {
    const channel = new MessageChannel();
    const timeout = setTimeout(() => reject(new Error("The encrypted connection timed out.")), 30000);
    channel.port1.onmessage = ({ data }) => {
      clearTimeout(timeout);
      data?.ok ? resolve(data) : reject(new Error(data?.error || "The invitation was rejected."));
    };
    worker.postMessage(
      { type: "medousa-arm", invitationUrl, nowUnix: Math.floor(Date.now() / 1000) },
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
    const worker = registration.installing || registration.waiting || registration.active;
    if (!worker) throw new Error("The site worker is unavailable.");
    await waitForWorker(worker);
    setStatus("Connecting over the encrypted mesh…");
    const armed = await armWorker(registration.active || worker, invitationUrl);
    invitationUrl = "";
    location.replace(armed.entryPath || "/");
  } catch (error) {
    invitationUrl = "";
    setStatus(error instanceof Error ? error.message : String(error), true);
  }
}

void openSite();
