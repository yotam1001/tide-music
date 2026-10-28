# In-app updates

The installed Windows app checks stable GitHub releases. When an update is
available, a compact Update Tide control appears in the wide header and in
Settings → Updates. Narrow windows retain the Settings update badge.

1. Choose Update Tide. Downloading and checksum verification happen while music
   keeps playing. The button shows actual byte progress, with a Cancel action.
2. A successfully verified download becomes Restart to update. Tide keeps playing
   until you choose it; downloading never starts setup or closes the player.
3. Restart prepares and verifies setup while Tide keeps playing. Setup confirms
   readiness over a private parent/child pipe; Tide explicitly commits the restart
   before closing normally. Cancel before that commit keeps the player running.
   The setup helper applies the existing validated
   installation transaction in the background and reopens Tide. The successful
   update path shows no separate setup window. A failed installation can reveal
   the recovery window so errors are actionable.

From 0.4.2, a failed setup launch or readiness timeout retains the verified
download and offers Restart again. Reopening Tide can recover an already verified
cached installer after a fresh release/signature check. The cancel control on a
ready update lets you choose a fresh download. If installation succeeds but the
player cannot reopen, setup reports that distinction and keeps Open Tide enabled.
Manual update checks run immediately rather than silently ignoring clicks during
a cooldown. Older clients can still launch the new setup through its legacy
handoff; readiness confirmation applies once the new client is installed.

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


## Publisher authentication from 0.4.1

Tide requires an Ed25519-signed `TideUpdate.json` before downloading an installer.
Its public verification key is built into the app; the download server cannot
supply a replacement key. The signed bytes bind the exact stable version,
Windows x64 platform, installer filename, byte size and SHA-256 digest. Invalid,
missing, mismatched or expired signatures stop the update. Authorization is
checked again before launching setup, together with a fresh installer hash.
There is no unsigned fallback. Network and file-size limits still apply.

The signing key is kept outside the repository and CI in a Windows DPAPI
CurrentUser encrypted store. Release publication requires a separate local
signing step after reviewing the build. This limits what an attacker controlling
only the release server/account can substitute; compromise of the signing
machine or key remains a separate trust risk.

Versions through 0.4.0 do not contain this verification code. Their first upgrade
to 0.4.1 still uses their previous update checks, or a manually obtained installer.
Once 0.4.1 is installed, subsequent updates require signatures. These update
signatures are distinct from Windows Authenticode: they do not remove SmartScreen
or unknown-publisher notices for a manually downloaded setup executable.

Signed metadata expires after at most 90 days. If the latest release has expired
metadata, the maintainer must publish a newly signed manifest after reviewing the
same installer, or publish a newer release. Clients will not silently bypass it.
See [maintainer signing](RELEASE_SIGNING.md) for the publication process.
