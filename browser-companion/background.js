(function (root, factory) {
  "use strict";
  const implementation = factory();
  if (typeof module === "object" && module.exports) module.exports = implementation;
  else root.TideSession = implementation;
})(globalThis, function () {
  "use strict";
  const MUSIC_ORIGIN = "https://music.youtube.com";
  const HOST = "org.tidemusic.session";
  const ARM_MS = 120000;
  const NATIVE_MS = 75000;
  const MESSAGE_BYTES = 65536;
  const COOKIE_BYTES = 32768;
  const ALLOWED = new Set(["cookie", "x-goog-authuser", "x-goog-pageid", "x-goog-visitor-id", "user-agent", "origin"]);
  const NATIVE_CODES = new Set(["imported", "invalid_request", "invalid_headers", "session_rejected", "session_changed", "storage_error", "internal_error"]);
  const bytes = value => new TextEncoder().encode(value).length;

  function musicUrl(value) {
    try {
      const url = new URL(value);
      return url.origin === MUSIC_ORIGIN && !url.username && !url.password;
    } catch { return false; }
  }

  function isBrowse(details) {
    if (details.method !== "POST" || !musicUrl(details.url)) return false;
    return new URL(details.url).pathname === "/youtubei/v1/browse";
  }

  function collectHeaders(input) {
    if (!Array.isArray(input)) return { code: "invalid_headers" };
    const headers = Object.create(null);
    for (const item of input) {
      if (!item || typeof item.name !== "string") continue;
      const name = item.name.toLowerCase();
      if (!ALLOWED.has(name)) continue;
      if (typeof item.value !== "string" || !item.value || /[\x00-\x1f\x7f]/.test(item.value)) return { code: "invalid_headers" };
      if (Object.hasOwn(headers, name) && headers[name] !== item.value) return { code: "conflicting_headers" };
      headers[name] = item.value;
    }
    // Never infer the default account. Preserve the observed account/container headers.
    if (!headers.cookie || !headers["x-goog-authuser"]) return { code: "missing_account" };
    if (!/^\d{1,4}$/.test(headers["x-goog-authuser"])) return { code: "invalid_headers" };
    if (headers["x-goog-pageid"] && !/^\d{1,128}$/.test(headers["x-goog-pageid"])) return { code: "invalid_headers" };
    if (headers["x-goog-visitor-id"] && bytes(headers["x-goog-visitor-id"]) > 8192) return { code: "too_large" };
    if (headers["user-agent"] && bytes(headers["user-agent"]) > 1024) return { code: "too_large" };
    if (headers.origin !== MUSIC_ORIGIN) return { code: "invalid_headers" };
    if (bytes(headers.cookie) > COOKIE_BYTES) return { code: "too_large" };
    const message = { version: 1, action: "import_session", headers };
    if (bytes(JSON.stringify(message)) > MESSAGE_BYTES) return { code: "too_large" };
    return { message };
  }

  function validReply(reply) {
    return reply && typeof reply === "object" && !Array.isArray(reply)
      && Object.keys(reply).length === 3 && reply.version === 1
      && typeof reply.ok === "boolean" && NATIVE_CODES.has(reply.code)
      && (reply.ok ? reply.code === "imported" : reply.code !== "imported");
  }

  function install(api, options = {}) {
    const family = options.family || "chromium";
    const now = options.now || Date.now;
    const schedule = options.setTimeout || setTimeout;
    const unschedule = options.clearTimeout || clearTimeout;
    let arm = null;
    let pending = null;
    let nativePort = null;
    let timer = null;
    let generation = 0;
    let status = { state: "idle", code: "ready" };

    function clear(code, state = "idle") {
      generation += 1;
      arm = null;
      pending = null;
      if (timer !== null) unschedule(timer);
      timer = null;
      const port = nativePort;
      nativePort = null;
      status = { state, code };
      if (port) { try { port.disconnect(); } catch { /* No error details or secrets are displayed. */ } }
    }

    function live() {
      if (arm && now() >= arm.expiresAt) clear("expired");
      return arm !== null;
    }

    function snapshot() {
      live();
      return { ...status, expiresAt: arm ? arm.expiresAt : null };
    }

    async function selectedTab() {
      if (family === "firefox") return (await api.tabs.query({ active: true, currentWindow: true }))[0];
      return new Promise(resolve => api.tabs.query({ active: true, currentWindow: true }, tabs => {
        const error = api.runtime.lastError;
        resolve(error ? null : tabs[0]);
      }));
    }

    async function armSelected() {
      clear("ready");
      const token = generation;
      let tab;
      try { tab = await selectedTab(); } catch { return snapshot(); }
      if (token !== generation) return snapshot();
      if (!tab || !Number.isInteger(tab.id) || tab.id < 0 || !musicUrl(tab.url)) {
        status = { state: "idle", code: "wrong_tab" };
        return snapshot();
      }
      arm = { tabId: tab.id, expiresAt: now() + ARM_MS };
      status = { state: "armed", code: "click_library" };
      timer = schedule(() => clear("expired"), ARM_MS);
      return snapshot();
    }

    function beforeHeaders(details) {
      // Redirects/retries may reuse an ID with new outgoing headers. Do not hand off an earlier snapshot.
      if (live() && pending && details.requestId === pending.requestId) { clear("request_failed", "error"); return; }
      if (!live() || status.state !== "armed" || pending || details.tabId !== arm.tabId || !isBrowse(details)) return;
      const result = collectHeaders(details.requestHeaders);
      if (!result.message) { clear(result.code, "error"); return; }
      pending = { requestId: details.requestId, tabId: details.tabId, message: result.message };
      status = { state: "captured", code: "waiting_response" };
    }

    function transfer(message) {
      const token = generation;
      // Observation consent has been consumed. The host gets its own bounded verification window.
      arm = null;
      if (timer !== null) unschedule(timer);
      timer = schedule(() => clear("native_error", "error"), NATIVE_MS);
      status = { state: "sending", code: "importing" };
      let port;
      let posted = false;
      try {
        port = api.runtime.connectNative(HOST);
        nativePort = port;
        port.onDisconnect.addListener(() => {
          // Reading lastError prevents Chromium's diagnostic output; its content is never used.
          void api.runtime.lastError;
          if (nativePort === port && token === generation) clear("native_error", "error");
        });
        port.onMessage.addListener(reply => {
          if (nativePort !== port || token !== generation) return;
          if (!posted || !validReply(reply)) { clear("native_error", "error"); return; }
          clear(reply.ok ? "imported" : reply.code, reply.ok ? "success" : "error");
        });
        posted = true;
        port.postMessage(message);
      } catch { clear("native_error", "error"); }
      // Neither the message nor its headers are retained after this synchronous send.
    }

    function completed(details) {
      if (!live() || !pending || details.requestId !== pending.requestId) return;
      if (details.tabId !== pending.tabId || !isBrowse(details) || details.statusCode !== 200) {
        clear("request_failed", "error");
        return;
      }
      const message = pending.message;
      pending = null;
      transfer(message);
    }

    function failed(details) {
      if (live() && pending && details.requestId === pending.requestId) clear("request_failed", "error");
    }

    const filter = { urls: [MUSIC_ORIGIN + "/*"] };
    api.webRequest.onBeforeSendHeaders.addListener(beforeHeaders, filter,
      family === "firefox" ? ["requestHeaders"] : ["requestHeaders", "extraHeaders"]);
    api.webRequest.onCompleted.addListener(completed, filter);
    api.webRequest.onErrorOccurred.addListener(failed, filter);
    api.webRequest.onBeforeRequest.addListener(details => {
      if (live() && details.tabId === arm.tabId && details.type === "main_frame") clear("navigation");
    }, filter);
    api.tabs.onRemoved.addListener(tabId => { if (live() && tabId === arm.tabId) clear("navigation"); });
    api.tabs.onUpdated.addListener((tabId, change) => {
      if (!live() || tabId !== arm.tabId) return;
      if (change.status === "loading" || (change.url && !musicUrl(change.url))) clear("navigation");
      else if (change.url && pending) {
        // The Library button may update history within Music; discard any prior capture.
        pending = null;
        status = { state: "armed", code: "click_library" };
      }
    });
    api.runtime.onSuspend?.addListener(() => clear("suspended"));
    api.runtime.onMessage.addListener((message, sender, sendResponse) => {
      if (sender.id !== api.runtime.id || sender.url !== api.runtime.getURL("popup.html")) return false;
      if (!message || Object.keys(message).length !== 1) return false;
      if (message.action === "arm") {
        armSelected().then(sendResponse, () => sendResponse(snapshot()));
        return true;
      }
      if (message.action === "cancel") {
        clear(status.state === "sending" ? "native_cancelled" : "cancelled");
        sendResponse(snapshot());
      }
      else if (message.action === "status") sendResponse(snapshot());
      return false;
    });
    return { snapshot }; // Public state contains no URL, header, request ID, or credentials.
  }
  return { install, collectHeaders, validReply, MUSIC_ORIGIN, HOST, ARM_MS, NATIVE_MS, MESSAGE_BYTES, COOKIE_BYTES };
});
