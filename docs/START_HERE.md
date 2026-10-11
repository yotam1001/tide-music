# Tide Windows preview

This is a preview, not a verified stable release. Current evidence and remaining
release checks are available in the [source repository](https://github.com/yotam1001/tide-music).

Extract the entire ZIP into a writable folder. The ZIP contains Tide, setup
scripts, the optional account helper source and license notices. Playback tools,
Python, credentials and browser profiles are not included.

## Install playback tools

Open PowerShell in the extracted folder and run:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/setup-runtime.ps1 -Destination tools
```

The script downloads pinned mpv, yt-dlp and Deno releases. It writes only into
this folder; it needs internet access but no administrator privileges. Then
double-click `tide-music.exe`. Open a local audio file, paste a music link, or
search from the header. Home and Library are the main destinations; Queue opens
the upcoming tracks. Account contains theme and window-size controls. Mini has
an expand icon to return to the full window.

## Connect YouTube Music

Account features also require an installed Python 3.10 or newer. With Python
available as `python`, run:

```powershell
powershell -ExecutionPolicy Bypass -File scripts/setup-provider.ps1
```

Sign in to YouTube Music in your existing Zen profile. In Tide, open Account,
select the appropriate account number and choose Connect/Reconnect. Select
`cookies.sqlite` from that Zen profile. Tide verifies the account before showing
Connected. Reconnect preserves the selected account number. See
[the account guide](docs/ACCOUNT_SETUP.md) for profile selection and manual import.

Credentials remain on your computer, encrypted for your Windows user. Never
upload cookies, session files or your browser profile to GitHub or send them in
chat. Premium playback depends on the supplied account and service compatibility.

## Updates and removal

Close Tide before replacing its files. Install each preview into a separate
folder, and run the setup scripts there; Python virtual environments should not
be copied between folders. Saved preferences and encrypted sessions are kept
in your user application-data directory. Disconnect inside Account before
removing the app if you want to remove Tide's saved credentials. Removing the
extracted folder does not delete the shared preferences or encrypted session.

The app source uses MIT; third-party licenses are included in LICENSE,
THIRD_PARTY_NOTICES.md and DEPENDENCIES.md. Downloaded tools retain their own
licenses and are obtained directly from upstream rather than bundled in this ZIP.
