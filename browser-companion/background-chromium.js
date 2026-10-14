/* No storage: worker termination discards consent and pending headers. */
importScripts("background.js");
TideSession.install(chrome, { family: "chromium" });
