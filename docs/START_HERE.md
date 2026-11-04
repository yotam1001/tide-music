# Start using Tide

Tide is a native Windows music player for listening while gaming. The installer
is unsigned. You need Windows x64 and internet access. No Rust, Python installation or API key is needed with the installer.

## 1. Install and open

Download the installer from the latest release. You can connect YouTube Music
after installation; the browser companion is an optional fallback.

Open **TideSetup.exe**, choose **Install Tide**, and wait for setup to finish.
Choose **Open Tide**, or find **Tide** in the Windows Start menu.

Reading this inside the advanced ZIP and cannot find the EXE? Get the installer
from the [Windows release](https://github.com/yotam1001/tide-music/releases/latest).
Advanced users can instead run the ZIP's **Install Tide.cmd**.

## 2. Connect YouTube Music

Open the profile icon at the top right and choose **Sign in with Google**.
Complete sign-in in the temporary window, select your YouTube Music profile,
open **Library**, then choose **Connect this profile**. The window closes after
verification. No browser needs to remain open while music plays.

If native sign-in cannot open, choose **Use browser companion instead** and follow
the [Chrome connection guide](https://github.com/yotam1001/tide-music/blob/main/docs/BROWSER_COMPANION_BETA.md).
The companion is loaded manually rather than from the Chrome Web Store.
See [account setup](https://github.com/yotam1001/tide-music/blob/main/docs/ACCOUNT_SETUP.md).

To try Tide without an account, skip this step and play a local audio file.

## 3. Play music

- After connecting, search in the top bar, choose a playlist in **Library**, or
  pick a **Home** recommendation.
- Open **Queue** to see upcoming tracks. Drag an upcoming song's handle to reorder.
- Use **Settings → Advanced → Open audio file** for music on your PC.
- The sun/moon button changes theme. **More (…) → Open mini player** makes the player smaller.

## Updates and help

**Settings → Check for updates** finds published stable releases. **Install and
restart** verifies the download and keeps your account and preferences. Preview
releases can update to the latest stable version here too. Future preview-only updates need
their installer.

[Installation and removal](https://github.com/yotam1001/tide-music/blob/main/docs/INSTALLATION.md) ·
[Troubleshooting](https://github.com/yotam1001/tide-music/blob/main/docs/TROUBLESHOOTING.md) ·
[Privacy](https://github.com/yotam1001/tide-music/blob/main/PRIVACY.md)

Never share cookies, request headers, session files or screenshots containing
them. Report the app version and exact error message instead.

## Listen together with Jams

Both people install the latest Tide and connect their own YouTube Music accounts.
Host: play a song, open **Jams** below **Library**, press **Start Jam**, then **Copy invite**.
Friend: open **Jams**, paste the invite and press **Join Jam**. Close the panel to
browse and press **+** beside a song to add it to the shared queue.

The host controls playback; each person controls their own volume. No browser
needs to stay open. **End Jam** keeps the host's music playing. A listener's
**Leave Jam** restores their original queue paused.

The free shared server can take about a minute to wake and currently supports
two rooms at once. **Advanced setup** lets you use a self-hosted server instead.
