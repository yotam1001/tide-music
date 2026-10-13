# Tide demo video brief

Create a 45–60 second demo for **Tide**, an open-source Windows music player built
for people who listen to YouTube Music while gaming. Its purpose is to offer a
clear, compact desktop player with native audio playback and modest resource
use. It is built with Rust and Slint, with mpv handling audio. The current build
is a preview under reliability testing, not a finished stable release.

Use actual screen recordings from the supplied build. Make the edit calm,
polished and easy to follow: readable captions, short cursor movements, clean
cuts and subtle transitions. Let the interface do most of the explaining.

## Suggested sequence

- **0–5 seconds:** Tide logo and a short hook: “Music beside your game.” Show
  the full player briefly, followed by its small mini-player view.
- **5–15 seconds:** Show Home, search for a song, then start playback. Highlight
  the centered playback controls and the clear browsing layout.
- **15–28 seconds:** Open Library, choose a playlist and show its Play and
  Shuffle actions. Open Queue and drag an upcoming song to reorder it. Show
  Play next or Add to queue through the song menu.
- **28–40 seconds:** Show song radio and Autoplay adding recommendations as
  the queue runs low. Record a real transition between songs; do not simulate
  an instant or gapless transition.
- **40–50 seconds:** Switch between light and dark themes without changing
  window size. Show Mini as a separate layout control, then place the mini
  player beside gameplay or a neutral game scene.
- **50–60 seconds:** End with “Open source. Built for listening while gaming.”
  Add “Preview — in development” and the repository link:
  https://github.com/yotam1001/tide-music

## Visual direction and delivery

Use Tide's current logo and interface rather than inventing replacement screens.
Its dark theme uses muted burgundy accents; the light theme uses warm cream and
earthy colors. Keep overlays restrained and large enough to read. Deliver a
1920×1080 master, an optional vertical cut and the editable project.

Use music cleared for this video. Use a synthetic or consenting demo library;
hide personal account details, notifications and private playlist names. Never
show developer tools, request headers, cookies, tokens or local session files.

Avoid claims of guaranteed gapless playback, a fixed RAM figure, guaranteed
maximum YouTube Music quality, guaranteed Premium/ad-free compatibility, or
release readiness. If a resource measurement is included, use a verified
measurement for the recorded build, label the test conditions and include the
player's helper processes. Login is being improved; omit its walkthrough until
the actual flow passes browser testing, or label it clearly as a prototype.
Keep proposed features out of footage unless marked as concepts.
