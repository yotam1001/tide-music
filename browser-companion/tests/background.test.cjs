"use strict";
const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const companion = require("../background.js");

function event() {
  const listeners = [];
  const registrations = [];
  return {
    listeners, registrations,
    addListener(fn, ...args) { listeners.push(fn); registrations.push(args); },
    emit(...args) { return listeners.map(fn => fn(...args)); }
  };
}
function fixture(family = "chromium") {
  let time = 10000;
  const timers = new Map();
  let timerId = 0;
  const ports = [];
  const api = {
    runtime: {
      id: "test-extension", lastError: undefined,
      getURL: file => "extension://test/" + file,
      onMessage: event(), onSuspend: event(),
      connectNative(name) {
        assert.equal(name, companion.HOST);
        if (api.failConnect) throw new Error("mock host absent");
        const port = { onMessage: event(), onDisconnect: event(), sent: [], disconnected: false,
          postMessage(message) {
            if (api.failPost) throw new Error("mock send failure");
            this.sent.push(JSON.parse(JSON.stringify(message)));
          },
          disconnect() { this.disconnected = true; this.onDisconnect.emit(); }
        };
        ports.push(port);
        return port;
      }
    },
    tabs: {
      selected: { id: 7, url: "https://music.youtube.com/" },
      query(filter, callback) {
        assert.deepEqual(filter, { active: true, currentWindow: true });
        const tabs = [this.selected];
        if (callback) callback(tabs);
        else return Promise.resolve(tabs);
      },
      onUpdated: event(), onRemoved: event()
    },
    webRequest: { onBeforeSendHeaders: event(), onCompleted: event(), onErrorOccurred: event(), onBeforeRequest: event() }
  };
  const controller = companion.install(api, {
    family, now: () => time,
    setTimeout(fn, delay) { const id = ++timerId; timers.set(id, { fn, at: time + delay }); return id; },
    clearTimeout(id) { timers.delete(id); }
  });
  const sender = { id: api.runtime.id, url: api.runtime.getURL("popup.html") };
  function message(action, from = sender) {
    return new Promise(resolve => {
      const result = api.runtime.onMessage.emit({ action }, from, resolve);
      if (!result.includes(true)) resolve(undefined);
    });
  }
  function advance(ms, runTimers = true) {
    time += ms;
    if (runTimers) for (const [id, task] of [...timers]) {
      if (task.at <= time) { timers.delete(id); task.fn(); }
    }
  }
  const headers = [
    { name: "Cookie", value: "SID=synthetic; SAPISID=fixture" },
    { name: "X-Goog-AuthUser", value: "2" },
    { name: "X-Goog-PageId", value: "123456789012345678901" },
    { name: "X-Goog-Visitor-Id", value: "visitor-fixture" },
    { name: "User-Agent", value: "Synthetic test UA" },
    { name: "Origin", value: companion.MUSIC_ORIGIN },
    { name: "Authorization", value: "must never transfer" },
    { name: "X-Unrelated", value: "must never transfer either" }
  ];
  function request(overrides = {}) {
    return { tabId: 7, requestId: "request-1", method: "POST", url: companion.MUSIC_ORIGIN + "/youtubei/v1/browse?key=must-not-transfer&authuser=0", requestHeaders: headers, ...overrides };
  }
  function capture(overrides) { api.webRequest.onBeforeSendHeaders.emit(request(overrides)); }
  function complete(overrides = {}) { api.webRequest.onCompleted.emit(request({ statusCode: 200, ...overrides })); }
  return { api, controller, ports, message, advance, headers, request, capture, complete };
}

test("listener permissions are observational and browser-specific", () => {
  for (const family of ["firefox", "chromium"]) {
    const f = fixture(family);
    const [filter, options] = f.api.webRequest.onBeforeSendHeaders.registrations[0];
    assert.deepEqual(filter, { urls: ["https://music.youtube.com/*"] });
    assert.deepEqual(options, family === "firefox" ? ["requestHeaders"] : ["requestHeaders", "extraHeaders"]);
    assert.ok(!options.includes("blocking"));
  }
});
test("idle and untrusted messages cannot capture or arm", async () => {
  const f = fixture();
  f.capture(); f.complete();
  assert.equal(f.ports.length, 0);
  for (const sender of [ { id: "other", url: "extension://test/popup.html" }, { id: "test-extension", url: "https://music.youtube.com" } ]) {
    assert.equal(await f.message("arm", sender), undefined);
    assert.equal(f.controller.snapshot().state, "idle");
  }
});
test("selected tab requires the exact HTTPS Music origin", async () => {
  const f = fixture();
  for (const url of ["https://youtube.com", "https://music.youtube.com.evil.invalid", "http://music.youtube.com", "https://music.youtube.com:444", "https://user@music.youtube.com"]) {
    f.api.tabs.selected.url = url;
    assert.equal((await f.message("arm")).code, "wrong_tab");
  }
});
test("one successful request sends exact allowlist, account and container session", async () => {
  const f = fixture("firefox");
  assert.equal((await f.message("arm")).state, "armed");
  f.capture();
  f.capture({ requestId: "other-request" });
  f.complete({ requestId: "other-request" });
  assert.equal(f.ports.length, 0);
  f.complete();
  const port = f.ports[0];
  assert.equal(port.sent.length, 1);
  assert.deepEqual(port.sent[0], {
    version: 1, action: "import_session", headers: {
      cookie: "SID=synthetic; SAPISID=fixture", "x-goog-authuser": "2", "x-goog-pageid": "123456789012345678901",
      "x-goog-visitor-id": "visitor-fixture", "user-agent": "Synthetic test UA", origin: companion.MUSIC_ORIGIN
    }
  });
  assert.ok(!JSON.stringify(port.sent).includes("must"));
  assert.ok(!JSON.stringify(f.controller.snapshot()).includes("fixture"));
  port.onMessage.emit({ version: 1, ok: true, code: "imported" });
  assert.equal(f.controller.snapshot().state, "success");
  assert.equal(port.disconnected, true);
  f.capture(); f.complete();
  assert.equal(f.ports.length, 1);
});
test("ignores other tabs, origins, methods and browse-like paths", async () => {
  const f = fixture(); await f.message("arm");
  for (const overrides of [ { tabId: 8 }, { method: "GET" }, { method: "OPTIONS" }, { url: "https://www.youtube.com/youtubei/v1/browse" }, { url: "https://music.youtube.com/youtubei/v1/browse/" }, { url: "https://music.youtube.com/youtubei/v1/search" } ]) f.capture(overrides);
  f.complete();
  assert.equal(f.ports.length, 0);
  assert.equal(f.controller.snapshot().state, "armed");
});
test("completion must match captured request tab, path, method and status 200", async () => {
  for (const overrides of [ { tabId: 8 }, { method: "GET" }, { url: "https://music.youtube.com/youtubei/v1/search" }, { statusCode: 204 }, { statusCode: 401 }, { statusCode: 302 } ]) {
    const f = fixture(); await f.message("arm"); f.capture(); f.complete(overrides);
    assert.equal(f.ports.length, 0);
    assert.equal(f.controller.snapshot().code, "request_failed");
  }
});
test("a repeated before-header event for the bound ID rejects a stale snapshot", async () => {
  const f = fixture(); await f.message("arm"); f.capture();
  f.capture({ requestHeaders: f.headers.map(h => h.name === "X-Goog-AuthUser" ? { name: h.name, value: "3" } : h) });
  f.complete();
  assert.equal(f.ports.length, 0);
  assert.equal(f.controller.snapshot().code, "request_failed");
});
test("no default account guessing from missing header or URL query", async () => {
  const f = fixture(); await f.message("arm");
  f.capture({ requestHeaders: f.headers.filter(h => h.name !== "X-Goog-AuthUser") }); f.complete();
  assert.equal(f.controller.snapshot().code, "missing_account");
  assert.equal(f.ports.length, 0);
});
test("conflicting duplicates stop capture; identical duplicates preserve value", () => {
  const f = fixture();
  for (const name of ["Cookie", "X-Goog-AuthUser", "X-Goog-PageId", "Origin", "User-Agent"]) {
    assert.equal(companion.collectHeaders([...f.headers, { name, value: "conflicting" }]).code, "conflicting_headers");
  }
  const message = companion.collectHeaders([...f.headers, { name: "cookie", value: f.headers[0].value }]).message;
  assert.equal(message.headers.cookie, f.headers[0].value);
});
test("binary/control/invalid-origin values are rejected", () => {
  const f = fixture();
  for (const value of [undefined, "abc\r\ninjected", "abc\u0000", "abc\u007f"]) {
    const headers = f.headers.map(h => h.name === "Cookie" ? { name: h.name, value } : h);
    assert.equal(companion.collectHeaders(headers).code, "invalid_headers");
  }
  assert.equal(companion.collectHeaders(f.headers.filter(h => h.name !== "Origin")).code, "invalid_headers");
});
test("UTF-8 cookie and complete serialized message limits", () => {
  const f = fixture();
  const withCookie = value => f.headers.map(h => h.name === "Cookie" ? { name: h.name, value } : h);
  assert.ok(companion.collectHeaders(withCookie("x".repeat(32768))).message);
  assert.equal(companion.collectHeaders(withCookie("x".repeat(32769))).code, "too_large");
  assert.equal(companion.collectHeaders(withCookie("é".repeat(16385))).code, "too_large");
  // JSON escaping may expand a valid bounded cookie beyond the full-message limit.
  assert.equal(companion.collectHeaders(withCookie("\\".repeat(32768))).code, "too_large");
  const minimal = [ { name: "Cookie", value: "" }, { name: "X-Goog-AuthUser", value: "2" }, { name: "Origin", value: companion.MUSIC_ORIGIN } ];
  const baseCookie = "\\".repeat(32700);
  const serialize = value => JSON.stringify({ version: 1, action: "import_session", headers: { cookie: value, "x-goog-authuser": "2", origin: companion.MUSIC_ORIGIN } });
  const remainder = companion.MESSAGE_BYTES - Buffer.byteLength(serialize(baseCookie));
  const exactCookie = baseCookie + "x".repeat(remainder);
  assert.ok(Buffer.byteLength(exactCookie) <= companion.COOKIE_BYTES);
  const exact = minimal.map(h => h.name === "Cookie" ? { name: h.name, value: exactCookie } : h);
  assert.ok(companion.collectHeaders(exact).message);
  const beyond = exact.map(h => h.name === "Cookie" ? { name: h.name, value: exactCookie + "x" } : h);
  assert.equal(companion.collectHeaders(beyond).code, "too_large");
});
test("native receiver per-header format and UTF-8 size bounds", () => {
  const f = fixture();
  const replace = (name, value) => f.headers.map(h => h.name.toLowerCase() === name ? { name: h.name, value } : h);
  for (const value of ["0", "9999"]) assert.ok(companion.collectHeaders(replace("x-goog-authuser", value)).message);
  for (const value of ["10000", "-1", " 2", "2.0", "account"]) assert.equal(companion.collectHeaders(replace("x-goog-authuser", value)).code, "invalid_headers");
  for (const value of ["0", "9".repeat(128)]) assert.ok(companion.collectHeaders(replace("x-goog-pageid", value)).message);
  for (const value of ["9".repeat(129), "brand-channel", " 123", "12.3"]) assert.equal(companion.collectHeaders(replace("x-goog-pageid", value)).code, "invalid_headers");
  for (const [name, limit] of [["x-goog-visitor-id", 8192], ["user-agent", 1024]]) {
    assert.ok(companion.collectHeaders(replace(name, "x".repeat(limit))).message);
    assert.equal(companion.collectHeaders(replace(name, "x".repeat(limit + 1))).code, "too_large");
    assert.ok(companion.collectHeaders(replace(name, "é".repeat(limit / 2))).message);
    assert.equal(companion.collectHeaders(replace(name, "é".repeat(limit / 2 + 1))).code, "too_large");
  }
  for (const name of ["cookie", "x-goog-authuser", "x-goog-pageid", "x-goog-visitor-id", "user-agent", "origin"]) {
    assert.equal(companion.collectHeaders(replace(name, "")).code, "invalid_headers");
  }
});
test("cancel, expiry, suspension and navigation discard pending headers", async () => {
  for (const drop of [ f => f.message("cancel"), f => f.advance(120000), f => f.api.runtime.onSuspend.emit(), f => f.api.tabs.onRemoved.emit(7), f => f.api.tabs.onUpdated.emit(7, { status: "loading" }), f => f.api.tabs.onUpdated.emit(7, { url: "https://accounts.google.com" }), f => f.api.webRequest.onBeforeRequest.emit(f.request({ type: "main_frame" })) ]) {
    const f = fixture(); await f.message("arm"); f.capture(); await drop(f); f.complete();
    assert.equal(f.ports.length, 0);
    assert.equal(f.controller.snapshot().state, "idle");
  }
});
test("wall clock guards expiry even if worker timer has not run", async () => {
  const f = fixture(); await f.message("arm"); f.capture(); f.advance(120000, false); f.complete();
  assert.equal(f.controller.snapshot().code, "expired"); assert.equal(f.ports.length, 0);
});
test("Music history navigation discards old capture but permits fresh Library request", async () => {
  const f = fixture(); await f.message("arm"); f.capture();
  f.api.tabs.onUpdated.emit(7, { url: "https://music.youtube.com/library" }); f.complete();
  assert.equal(f.ports.length, 0);
  f.capture({ requestId: "fresh" }); f.complete({ requestId: "fresh" }); assert.equal(f.ports.length, 1);
});
test("matching network error stops capture, unrelated network error is ignored", async () => {
  const f = fixture(); await f.message("arm"); f.capture();
  f.api.webRequest.onErrorOccurred.emit(f.request({ requestId: "other" }));
  assert.equal(f.controller.snapshot().state, "captured");
  f.api.webRequest.onErrorOccurred.emit(f.request()); f.complete();
  assert.equal(f.controller.snapshot().code, "request_failed"); assert.equal(f.ports.length, 0);
});
test("native failures, malformed replies and late replies do not retain consent", async () => {
  for (const reply of [ { version: 2, ok: true, code: "imported" }, { version: 1, ok: true, code: "unknown" }, { version: 1, ok: false, code: "imported" }, { version: 1, ok: true, code: "imported", headers: "must not expose" }, null ]) {
    const f = fixture(); await f.message("arm"); f.capture(); f.complete(); const p = f.ports[0];
    p.onMessage.emit(reply);
    assert.equal(f.controller.snapshot().code, "native_error"); assert.equal(p.disconnected, true);
    p.onMessage.emit({ version: 1, ok: true, code: "imported" });
    assert.equal(f.controller.snapshot().code, "native_error");
  }
  for (const cause of ["failConnect", "failPost"]) {
    const f = fixture(); await f.message("arm"); f.api[cause] = true; f.capture(); f.complete();
    assert.equal(f.controller.snapshot().code, "native_error");
  }
});
test("fixed host failures and disconnects are handled without automatic retry", async () => {
  const codes = ["invalid_request", "invalid_headers", "session_rejected", "session_changed", "storage_error", "internal_error"];
  for (const code of codes) {
    const f = fixture(); await f.message("arm"); f.capture(); f.complete();
    f.ports[0].onMessage.emit({ version: 1, ok: false, code });
    assert.equal(f.controller.snapshot().code, code); assert.equal(f.controller.snapshot().state, "error");
    f.capture(); f.complete(); assert.equal(f.ports.length, 1);
  }
  const f = fixture(); await f.message("arm"); f.capture(); f.complete(); f.ports[0].onDisconnect.emit();
  assert.equal(f.controller.snapshot().code, "native_error");
});
test("cancel and native timeout disconnect transfer; later reply is ignored", async () => {
  for (const stop of [f => f.message("cancel"), f => f.advance(companion.NATIVE_MS)]) {
    const f = fixture(); await f.message("arm"); f.capture(); f.complete(); await stop(f);
    assert.equal(f.ports[0].disconnected, true);
    f.ports[0].onMessage.emit({ version: 1, ok: true, code: "imported" });
    assert.notEqual(f.controller.snapshot().state, "success");
  }
});
test("Cancel after posting says only that the native connection was closed", async () => {
  const f = fixture(); await f.message("arm"); f.capture(); f.complete();
  assert.equal((await f.message("cancel")).code, "native_cancelled");
});
test("capture near consent deadline still permits 75 seconds of host verification", async () => {
  const f = fixture(); await f.message("arm"); f.advance(119000); f.capture(); f.complete();
  f.advance(74000);
  assert.equal(f.ports[0].disconnected, false);
  assert.equal(f.controller.snapshot().state, "sending");
  f.ports[0].onMessage.emit({ version: 1, ok: true, code: "imported" });
  assert.equal(f.controller.snapshot().state, "success");
});
test("cancel during asynchronous tab query prevents stale arm", async () => {
  const f = fixture("firefox");
  let resolve;
  f.api.tabs.query = () => new Promise(r => { resolve = r; });
  const arm = f.message("arm"); await f.message("cancel"); resolve([f.api.tabs.selected]);
  assert.equal((await arm).code, "cancelled"); assert.equal(f.controller.snapshot().state, "idle");
});
test("manifests have no storage, cookie, content scripts, remote code or broad hosts", () => {
  for (const family of ["firefox", "chromium"]) {
    const manifest = JSON.parse(fs.readFileSync(path.join(__dirname, "..", "manifest." + family + ".json")));
    assert.ok(!manifest.permissions.includes("cookies")); assert.ok(!manifest.permissions.includes("storage"));
    assert.ok(!manifest.permissions.includes("webRequestBlocking"));
    assert.equal(manifest.content_scripts, undefined);
    const hosts = family === "firefox" ? manifest.permissions.filter(p => p.includes("://")) : manifest.host_permissions;
    assert.deepEqual(hosts, ["https://music.youtube.com/*"]);
  }
});
