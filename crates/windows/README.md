# tide-windows

Native Windows System Media Transport Controls (SMTC) for Tide's existing
application window. This crate uses Microsoft's `windows` 0.62 bindings and
`ISystemMediaTransportControlsInterop::GetForWindow`; it does not install a
keyboard hook, intercept other apps, or change persistent Windows settings.

After the UI window exists, pass its HWND to `start(hwnd, callback)`. Retain the
returned `MediaSession` on the UI thread and call:

```rust,ignore
session.update(title, artist, album, playing, position_seconds, duration_seconds)?;
```

The callback receives `MediaAction::{Play, Pause, Next, Previous, Stop, Seek}`.
`Seek` is an absolute position in seconds. Windows routes the hardware
play/pause key into distinct Play or Pause requests according to the published
state. `Toggle` is also present in the public enum for app use; the SMTC button
API does not emit a separate toggle event. Other transport buttons are ignored.
Seek requests and timeline publication are enabled when the OS exposes the
SMTC version 2 interface.

Callbacks may run on a Windows event thread. They must be brief and post their
work into the application event loop, for example through Slint's
`Weak::upgrade_in_event_loop`; they must not wait for the UI thread. The crate
serializes callbacks so only `Send`, rather than `Sync`, is required. Callback
panics are contained at the COM boundary.

Metadata and playback status use equality caches. Playback position is
coalesced to half-second updates while playing; paused positions retain full
WinRT time-span precision. State changes and changed metadata force a fresh
timeline publication even when the rounded position matches. Unknown duration
publishes an empty timeline; finite known durations bound the seek range.
Negative/non-finite timeline values are rejected before any state is changed.

The session is intentionally `!Send` and `!Sync`, balances its WinRT apartment
initialization on the creation thread, and uses the event loop's existing
apartment when one is already initialized differently. Drop removes both
subscription tokens, disables the controls, clears metadata, and waits for an
already-running callback to finish. Drop the session before destroying its
window. Invalid handles produce a clear error. Non-Windows builds expose the
same entry point and return an unsupported-platform error.

## Validation

```powershell
.\scripts\cargo-local.ps1 -CargoArgs @('test','-p','tide-windows')
.\scripts\cargo-local.ps1 -CargoArgs @('clippy','-p','tide-windows','--all-targets','--','-D','warnings')
.\scripts\cargo-local.ps1 -CargoArgs @('test','-p','tide-windows','native_window_session_lifecycle','--','--ignored')
```

The ignored native lifecycle test requires an interactive Windows desktop.
It creates an invisible owned window, registers a real SMTC session, publishes
metadata and playing/paused state, reads those values back, and verifies that
Drop disables the controls. It plays no audio and injects no hardware keys.
The ordinary tests cover timeline coalescing/clamping, action mapping, callback
deactivation, and invalid-HWND rejection.

On the development Windows machine, all four ordinary tests and the explicitly
run native lifecycle test passed. The native test completed in 0.41 seconds.
The crate also passed `cargo check` and all-target Clippy with warnings denied.

Physical keyboard and headset transport buttons, shell media-panel rendering,
and seek gestures still require a user end-to-end test with the wired app.
Windows chooses the active media session when several players are running;
this crate does not override that choice. No tray integration or artwork is
implemented here.

Primary references: Microsoft's [desktop HWND interop API](https://learn.microsoft.com/en-us/windows/win32/api/systemmediatransportcontrolsinterop/nf-systemmediatransportcontrolsinterop-isystemmediatransportcontrolsinterop-getforwindow),
[SMTC integration documentation](https://learn.microsoft.com/en-us/windows/apps/develop/media-playback/integrate-with-systemmediatransportcontrols),
and [playback-position requests](https://learn.microsoft.com/en-us/uwp/api/windows.media.systemmediatransportcontrols.playbackpositionchangerequested).
