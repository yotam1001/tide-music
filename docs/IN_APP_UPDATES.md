# In-app updates

The installed Windows app checks stable GitHub releases. When an update is
available, a compact Update Tide control appears in the wide header and in
Settings → Updates. Narrow windows retain the Settings update badge.

1. Choose Update Tide. Downloading and checksum verification happen while music
   keeps playing. The button shows actual byte progress, with a Cancel action.
2. A successfully verified download becomes Restart to update. Tide keeps playing
   until you choose it; downloading never starts setup or closes the player.
3. Restart closes Tide normally. The setup helper applies the existing validated
   installation transaction in the background and reopens Tide. The successful
   update path shows no separate setup window. A failed installation can reveal
   the recovery window so errors are actionable.

Accounts, settings, browser helper registration, ownership checks, rollback and
checksum verification remain part of the existing installation path. The updater
still accepts only newer stable releases and the expected GitHub installer asset.
No WebView or browser is introduced.

Downloads use one worker and a single atomic progress value, not a queue of
progress events. The existing owned cache still contains at most one installer
and one partial, each capped at 96 MiB. Cancellation is cooperative: a pending
network read must return before the worker cleans up, with the existing request
timeout bounding a stalled connection. The UI remains responsive and cannot
start overlapping download workers. A late cancel never launches setup.

Validation covers cancellation preserving a previous installer, rejection of bad
checksums, waiting for explicit restart, independent native controls in both
themes, and the actual Windows installer update/restart on hosted CI.
