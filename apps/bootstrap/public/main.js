import init, { SiteClient } from "./wasm/medousa_site_browser.js";

const status = document.querySelector("#status");
const siteFrame = document.querySelector("#site");

function setStatus(message, failed = false) {
  status.textContent = message;
  document.body.classList.toggle("failed", failed);
}

async function openSite() {
  let invitationUrl = window.location.href;

  // Remove the capability-bearing fragment before any remote site code runs.
  history.replaceState(null, "", `${location.pathname}${location.search}`);

  try {
    await init();
    setStatus("Connecting over the encrypted mesh…");
    const nowUnix = Math.floor(Date.now() / 1000);
    const client = await SiteClient.connect(invitationUrl, nowUnix);
    invitationUrl = "";
    const response = await client.fetch(client.entryPath);
    if (response.status !== 200) {
      throw new Error(`The site returned status ${response.status}.`);
    }
    if (!response.content_type?.toLowerCase().startsWith("text/html")) {
      throw new Error("The invitation entry point is not an HTML document.");
    }

    const html = new TextDecoder("utf-8", { fatal: true }).decode(
      new Uint8Array(response.body),
    );
    siteFrame.srcdoc = html;
    siteFrame.hidden = false;
    document.querySelector("main").hidden = true;
    document.title = `Private site · ${client.siteId.slice(0, 8)}`;
  } catch (error) {
    invitationUrl = "";
    setStatus(error instanceof Error ? error.message : String(error), true);
  }
}

void openSite();
