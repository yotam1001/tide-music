# Optional Tide browser session companion

This is an isolated **source prototype**, not a signed, published, installed, or browser-verified login solution. No extension installation, profile access, native-host registration, or live Google request is performed by this source or its tests. The native receiver is implemented as a separate source prototype in `crates/provider/src/bin/tide-session-host.rs`; installation and registration remain pending. This alternative does not choose the application's default login backend.

## Contents and local tests

- `manifest.firefox.json`: Firefox Manifest V2, fixed ID `tide-session@tide-music`.
- `manifest.chromium.json`: Chrome/Edge Manifest V3; the installed extension ID must be explicitly supplied to the native-host installer. There is no guessed or wildcard ID.
- `background.js`: shared capture controller, native protocol and validation.
- `background-firefox.js` / `background-chromium.js`: browser-specific entry points.
- `popup.html`, `popup.js`, `popup.css`: local UI with Import, Retry and Cancel.
- `tests/background.test.cjs`: deterministic synthetic events, timers, browser APIs and native ports.

Run with Node.js (no dependencies):

```text
node --test browser-companion/tests/background.test.cjs
```

To prepare a future reviewable browser package, copy this folder into a staging directory and copy the appropriate `manifest.*.json` to `manifest.json` there. Installation and distribution are not part of this prototype. Firefox release distribution needs the relevant signing process; Chrome/Edge registration needs the actual extension ID and a matching allowed native-host origin.

## Explicit flow

1. Select an already signed-in tab whose exact origin is `https://music.youtube.com`.
2. Open the companion popup and click **Import selected tab**. Opening the popup alone does not arm capture.
3. Close the popup and click **Library** in YouTube Music within 120 seconds. Only the first matching `POST /youtubei/v1/browse` request in that exact tab is captured. No synthetic browser request or automatic refresh is made.
4. Only after the same request ID, tab, method, origin and path completes with status 200 does the companion send one native message to `org.tidemusic.session`.
5. Reopen the popup to see the fixed status, or use **Cancel**. Failures require manual retry.

The controller observes requests without blocking or modifying them. URL query values, including account hints, are never transmitted. The six allowed request headers are `cookie`, `x-goog-authuser`, `x-goog-pageid`, `x-goog-visitor-id`, `user-agent` and `origin`. `Authorization` and every other header are ignored. The origin header must be exactly the Music origin; cookie and an explicit numeric account header are required. Missing account information stops capture; there is no account-zero fallback.

Existing Firefox container and account selection is preserved implicitly by using the request's own headers in its exact tab. No cookie jar or profile database is accessed. Conflicting duplicate values in any allowed header are rejected; byte-identical duplicates are deduplicated. This is deliberately conservative for split cookie headers.

## Native protocol

The port posts exactly one object:

```json
{"version":1,"action":"import_session","headers":{"cookie":"synthetic only","x-goog-authuser":"2","origin":"https://music.youtube.com"}}
```

UTF-8 limits match the receiver: full serialized request at most 65,536 bytes; cookie value at most 32,768 bytes; visitor ID at most 8,192 bytes; user agent at most 1,024 bytes. Account index must contain one to four digits; page ID, when present, must contain one to 128 digits. Every supplied allowed header must be nonempty. Native framing and independent host-side validation remain the native host's responsibility. The controller accepts only a three-field response `{ "version": 1, "ok": true|false, "code": "fixed_code" }`. Accepted codes are `imported`, `invalid_request`, `invalid_headers`, `session_rejected`, `session_changed`, `storage_error` and `internal_error`; only `imported` may accompany success. Malformed/unknown replies become a generic UI error. The port is closed after one reply, cancellation, expiry or native failure. No unsolicited native connection or refresh occurs.

The popup never receives headers or native error text. There is no logging, storage API, content script, remote script, telemetry or browser fetch. Header references are dropped on cancel, failure, expiry, full navigation, tab closure, suspension and immediately after posting to the native port. JavaScript garbage collection and browser-internal message buffers do not provide secure memory erasure. After posting, the host has a separate 75-second reply window for verification, without retry or a second import. The native receiver verifies the session through the provider before committing, checks for concurrent saved-session changes, and watches port/stdin closure to signal cancellation before verification and commit. Cancel closes the native port; a cancellation arriving after commit cannot recall the saved session, so the popup honestly says the host may already have saved it.

Full page navigation cancels consent; navigating away from Music cancels consent. A same-origin Music history update discards an already pending capture while leaving the short-lived arm active, so the Library SPA interaction can produce a fresh capture. Other tabs cannot consume the arm.

## Browser differences and limits

Chrome/Edge register `onBeforeSendHeaders` with `requestHeaders` and `extraHeaders`: [Chrome's webRequest documentation](https://developer.chrome.com/docs/extensions/reference/api/webRequest) requires `extraHeaders` to expose Cookie and Origin. Firefox uses `requestHeaders` only; [Mozilla's event documentation](https://developer.mozilla.org/en-US/docs/Mozilla/Add-ons/WebExtensions/API/webRequest/onBeforeSendHeaders) lists Firefox's supported extra options. Neither uses `blocking` or requests `webRequestBlocking`.

The requested `onBeforeSendHeaders` event is an observation point, not proof of final network bytes. Another extension can modify headers later, and browsers do not expose every header. A 200 browse response also does not prove every requested Library function works. The native host must independently validate the session before replacing any saved credentials.

Firefox uses a persistent MV2 background page. Chrome/Edge use an MV3 service worker. There is deliberately no persisted arm or secret, including no `storage.session`: [Chrome's worker lifecycle](https://developer.chrome.com/docs/extensions/develop/concepts/service-workers/lifecycle) allows worker termination and loses globals. Capture can therefore stop before 120 seconds if the worker is suspended; reopening the popup and manually retrying is required. A native port exists only after a successful observed request and is closed promptly. The absolute expiry is checked on each event even if a timer is delayed. No keepalive workaround is used.

The mocked tests do not verify real browser request visibility, Firefox containers, Google account switching, extension signing/loading, native-host OS registration, or end-to-end imports. Those checks remain necessary before claiming usable browser login.
