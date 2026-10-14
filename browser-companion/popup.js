"use strict";
const api = typeof browser === "object" ? browser : chrome;
const importButton = document.getElementById("import");
const cancelButton = document.getElementById("cancel");
const checkHostButton = document.getElementById("check-host");
const statusNode = document.getElementById("status");
const messages = {
  ready: "Ready. Import the selected YouTube Music tab when you choose.",
  wrong_tab: "Select a tab at https://music.youtube.com, then retry.",
  click_library: "Capture armed. Close this popup and click Library in YouTube Music within two minutes.",
  waiting_response: "Waiting for the selected Music request to succeed…",
  importing: "Importing into the local Tide host…",
  imported: "Session imported. Open Tide to use your Library.",
  cancelled: "Capture cancelled. You can retry manually.",
  native_cancelled: "Disconnected from the local host. It may already have saved the session; check Tide before retrying.",
  navigation: "The tab navigated or closed. Select your Music tab and retry.",
  expired: "Capture expired. Retry, then click Library in YouTube Music.",
  suspended: "The background stopped. Retry to arm capture again.",
  missing_account: "No explicit signed-in account was observed. Check the selected Music account and retry.",
  conflicting_headers: "Ambiguous session headers. Capture stopped; retry manually.",
  invalid_headers: "The observed request could not be imported. Retry manually.",
  too_large: "The session exceeds the import size limit. Capture stopped.",
  request_failed: "The Music request did not succeed. Check the tab and retry.",
  native_error: "The local Tide host did not return a valid reply. Check its setup and retry.",
  native_missing: "The browser could not find or accept Tide's local host registration. Check the setup and browser compatibility, then retry.",
  native_forbidden: "The local host does not allow this companion's extension ID. Check the matching host setup.",
  native_start_failed: "The browser found Tide's host but could not start it. Check its executable and setup.",
  native_timeout: "Tide's local host did not reply before the time limit. Check its setup and retry.",
  native_outdated: "The local host is an older version. Update Tide's host, then retry.",
  checking_host: "Checking the local Tide connection without reading your Music session…",
  host_ready: "The browser can reach Tide's local host. No session was read or imported by this check.",
  invalid_request: "The local host rejected the import request. Check its version and retry.",
  session_rejected: "The local host could not verify this session. Check the selected account and retry.",
  session_changed: "Tide's saved session changed during import. Retry manually.",
  storage_error: "The local host could not save the session. Check Tide and retry.",
  internal_error: "The local host could not finish the import. Retry manually."
};
function send(action) {
  if (typeof browser === "object") return api.runtime.sendMessage({ action });
  return new Promise((resolve, reject) => api.runtime.sendMessage({ action }, reply => {
    if (api.runtime.lastError) reject(new Error("companion_unavailable"));
    else resolve(reply);
  }));
}
function render(reply) {
  const busy = reply && ["armed", "captured", "sending"].includes(reply.state);
  statusNode.textContent = reply && messages[reply.code] || "Companion unavailable. Reopen this popup to retry.";
  importButton.disabled = Boolean(busy);
  importButton.textContent = reply && ["ready", "host_ready"].includes(reply.code) ? "Import selected tab" : "Retry import";
  cancelButton.disabled = !busy;
  checkHostButton.disabled = Boolean(busy);
}
async function update(action) {
  try { render(await send(action)); }
  catch { render(null); }
}
importButton.addEventListener("click", () => update("arm"));
cancelButton.addEventListener("click", () => update("cancel"));
checkHostButton.addEventListener("click", () => update("check_host"));
update("status");
const poll = setInterval(() => update("status"), 1000);
window.addEventListener("pagehide", () => clearInterval(poll));
