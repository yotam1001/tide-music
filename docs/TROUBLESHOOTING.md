# Get help with Tide

Start with the app version under **Settings → About Tide** (scroll if needed)
and the exact error message. If the app cannot open, give the installer release.
Use downloads from the [official repository](https://github.com/yotam1001/tide-music/releases).
The current public installer is in the
[Windows preview release](https://github.com/yotam1001/tide-music/releases/tag/v0.1.0-preview.2).

## Installation and opening

| Problem | Next step |
| --- | --- |
| I cannot find a public download | Open the Windows preview release above, expand Assets and choose **TideSetup.exe**. Source code ZIPs are not installers. |
| I cannot find Tide after installing | Open Start and search **Tide**. Confirm setup reached its completion screen; merely downloading the EXE does not install it. |
| Windows blocked the installer | Check the repository source and record the exact message. Do not disable protections or device policies. Report the block. |
| Setup failed during downloads | Check your connection and free disk space, close Tide, and use **Try again**. Keep the error text if it fails again. |
| Setup says Tide is still running | Close its windows and finish or cancel companion imports. Setup deliberately does not kill running processes. |
| Browser helper registration conflicts | Use matching app/extension packages and the actual extension ID. Report the conflict; do not edit the registry to force it. |

## Connecting your account

Follow the [Chrome connection guide](BROWSER_COMPANION_BETA.md) in order. Loading
the extension, registering its local connection, and importing the Music tab are
three separate steps.

| Problem | Next step |
| --- | --- |
| Extension folder is invalid | Extract the Chromium ZIP. Select the folder containing `manifest.json` directly. |
| Check local connection fails | Rerun Tide setup with the Chrome checkbox and the exact ID from `chrome://extensions`. |
| No session was imported | Open Music Home, arm **Import selected tab**, close the popup and click Library within two minutes. |
| Tide shows the wrong account | Select the correct Music profile in Chrome and import its tab again. Check Account and Library in Tide. |
| Account data is missing after a successful import | Close and reopen Tide, then refresh Library. Report the popup status if it stays empty. |
| Reconnect is requested repeatedly | Confirm Library works in Chrome and try one fresh import. If it returns, report it as a bug. A minimum session lifetime is not guaranteed. |

Zen's native connection remains unresolved. Edge and Firefox have not passed
their own recommended setup flow. [Advanced imports](ACCOUNT_SETUP_ADVANCED.md)
are optional fallbacks, not the normal getting-started path.

## Playback and the window

- **No sound:** check Tide's pause state, its volume, the Windows volume mixer
  and the selected output device. Try a local audio file to separate audio-device
  issues from YouTube connection problems.
- **A song will not load:** check the account state and try another song. Some
  content or formats may be unavailable through the unofficial integration.
- **Music stops when the queue ends:** open Queue and check Autoplay. Related
  recommendations require a working account connection.
- **Delay between songs:** the next address is prepared when possible, but
  network/extraction startup can still leave a gap. Record whether it happens
  on manual skips, natural transitions or both.
- **Transparent/blank window or frozen labels:** report the app version and
  whether minimizing, restoring or resizing changes it. These should be treated
  as rendering bugs, not normal behavior.
- **Media/Fn keys do not respond:** use Tide's controls and report the keyboard,
  Windows version and whether a game had focus. Hardware behavior remains a
  manual test item.

## Updates and disk space

The in-app updater checks published **stable** releases only. It is normal not
to see an update for a draft or a newer beta. Update previews with their newer
installer; update unpacked extension files separately.

Installed Tide does not create a Rust `target` folder. If you built the source,
compiler output belongs to your development checkout, not the installed player.
Use the guarded workflow in [Development](DEVELOPMENT.md). Do not delete account
folders to clear build output.

## Report a bug safely

Open an [issue](https://github.com/yotam1001/tide-music/issues/new) with:

- Tide version and Windows version.
- The exact error and the steps that caused it.
- Expected and actual behavior.
- Browser/version for connection problems; manual skip versus natural transition
  for playback problems.

Never include cookies, request headers, session files, signed media URLs or
developer-tools screenshots containing them. Review screenshots for account names
and private library information. Security reports belong in [SECURITY.md](../SECURITY.md).
