# Gaming integrations (in review)

## Discord presence

Settings → Discord provides an opt-in desktop connection. It publishes only the
current title and artist while playing. Pause, stop, disable and exit clear the
activity. No YouTube credentials, source URLs, account details, artwork downloads,
Jam invitations or Discord bot tokens are included.

Tide defaults to its registered **public Discord application ID**,
`1562782377111982222`. Turn sharing on to connect. The ID field remains available
for custom development applications. This ID is not a secret; never enter a bot token.
The desktop Discord client must be running. There is no browser requirement.

One native worker starts only when sharing is enabled with a valid ID. A single
latest-value slot coalesces changes, updates are limited to once per two seconds,
and reconnect attempts back off for 15 seconds. Pipe operations have a 500 ms
request deadline, incoming frames are limited to 16 KiB and shutdown clears the
activity best-effort. No background request queue or new disk cache is created.
The public ID and opt-in preference are saved in existing Tide settings.

## Pinned Mini

Use Mini, then Pin; or set Pin Mini in Settings → Mini player. This uses the
existing native window, without a second process, browser, injected game hook or
new graphics engine. Opacity is adjustable between 60% and 100% in Settings.
Expanding restores normal opacity and removes the always-on-top state. The
preference is retained for the next time Mini opens. Exclusive fullscreen games
can cover ordinary desktop windows; borderless/windowed games are the target.

Click-through locking and an Xbox Game Bar package are deferred. Mini stays
interactive so there is always a visible route to unpin and expand it.

## Verification required before release

- Hosted Windows: compile, workspace tests, Clippy, installer and native UI checks.
- Fake-pipe tests: fragmented replies, stalled peer cancellation, bounded frames.
- Native window test: topmost, alpha change, full-opacity/unpinned restoration.
- Real Discord with a registered application: song change, pause, resume, close,
  restart Discord, disable while connected, and process-tree RAM comparison.
- Actual Slint Mini: minimize/restore, pin/unpin, opacity and expansion; no faint
  rectangle, transparent restoration or playback interruption.

The synthetic pipe/window tests do not prove live Discord acceptance or a game's
fullscreen behavior. No zero-RAM claim is made; real measurements remain required.

Grove has a separate registered application reserved for future integration:
`1562783299452141711`. Registration alone does not implement Grove presence.
