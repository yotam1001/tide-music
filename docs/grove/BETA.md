# Grove 0.1.0-beta.1

Private Windows beta candidate, currently held from release because reliable
Spotify access and audible playback have not passed live testing. See the
separate beta status report supplied with the download.

## Install

Run GroveSetup.exe on Windows 10/11 x64. It installs for your Windows user,
adds Grove to Start and Windows Settings → Apps, and downloads a pinned,
checksum-verified mpv runtime for local audio. No administrator access, Python
installation, browser extension or separate Spotify playback daemon is required.
This private build is unsigned. Verify its SHA-256 against the supplied manifest;
code signing and automatic updates are not included in this beta.

Keep the original installer for another installation. Updates use a fresh
versioned folder and preserve preferences and Spotify approvals. Close Grove
before installing an update. Remove it through Windows Settings → Apps.
Disconnect inside Grove before uninstalling if you also want to remove the
saved Spotify approvals; uninstall preserves them for a later reinstall.

The ZIP is an app-only portable alternative: extract it and run grove-music.exe.
Spotify playback needs no additional runtime. Local files in the portable ZIP
need a recent mpv.exe in its tools folder; the installer prepares that support.

## Connect Spotify

1. Account → Connect Spotify. Approve access in the browser.
2. Return to Account → Enable playback. Approve with the same account.
3. Spotify Premium is required for streaming. Start with a song in Search or
   paste a Spotify song link into Search.

Grove opens Zen if installed, otherwise the default browser. Chrome also works:
copy the approval page into Chrome while Grove remains open. Do this for both
approvals. Password entry stays on Spotify's website. Reusable approvals are
stored separately in Windows Credential Manager.

The default sign-in uses a shared public Spotify app identity. Spotify can show
that app's name and rate-limit it. Grove waits through short profile-check
cooldowns automatically, with a bounded retry and Cancel. A long or recurring
cooldown requires trying later or a dedicated Spotify client ID under Account →
Connection options, with http://127.0.0.1:8989/login registered as its redirect.
If Spotify explicitly reports an exhausted app quota, Grove stops the short
automatic retry. Repeating consent cannot remove that quota restriction.
Do not send passwords, approval URLs, codes, cookies or tokens in feedback.

## Try these first

- Find a song, play, pause, resume, seek, change volume, skip and repeat.
- Browse Liked Songs and playlists, add to the Grove queue, reorder and remove.
- Close and reopen Grove; check restored sign-in and Enable playback.
- Try dark/light, compact/mini and media keys. Check playback after sleep and
  after changing headphones or speakers; reconnect from Account if needed.
- Try a local file after installation.

Likes and playlist edits change your real Spotify library. Test them deliberately
on a disposable playlist. Removal affects all matching copies of a song.

## Experimental jams

Only trusted local networks are supported. Start or join from Shared jams.
Each Spotify listener needs their own Premium account. Cross-service Tide/Grove
listeners confirm a matching recording on their service, then follow host
play/pause and approximate timing. Use matching audio recordings; videos, intros
and different edits can shift the audio. Both apps need the updated version-2
room protocol. Choose songs directly in the host app for mixed rooms.

Local room traffic is unencrypted; keep invitations private. There is no
internet relay, automatic reconnect or promise of sample-accurate audio sync.

## Feedback

Describe what you clicked, what you expected and what happened. Include the
version from Settings, and optionally Settings → Save support info. That report
contains fixed connection and appearance flags, no account IDs, tokens, song
names, listening history, file paths or invitations. Nothing is sent automatically.
Send feedback privately to the person who provided this beta.
