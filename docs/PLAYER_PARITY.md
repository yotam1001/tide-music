# Player improvement and comparison gates

This is an implementation checklist, not a claim that Tide outperforms another
player. Baseline: FeatherPlay's public README, screenshots and source reviewed on
2026-11-04 at https://github.com/candygoogy/featherplay. Recheck its current source
before making public comparisons. Last.fm and ListenBrainz are out of scope by
the maintainer's choice.

Keep Tide's existing logo, berry/terracotta palette, free Jams, signed updater and
bounded artwork loading. These improvements must not silently change recommendation
ranking or replace the working account connection with an unverified login flow.

| Area | Required outcome | Current evidence / remaining work |
| --- | --- | --- |
| Navigation | Direct account, settings, liked songs, Supermix and playlist access; clear labels and keyboard focus | User-selected concept B implemented: compact labeled rail, single Account header action, playlists through Library. Native input tests pass, including small light/dark windows. |
| Home | Useful resume/current-song context, next song and provider recommendations; no duplicate decorative content | Local resume card reuses the current artwork and queue; recommendations retain source order. Review populated Home and avoid excess panel height. |
| Transport | Discoverable shuffle/repeat, lyrics, queue and volume, with accessible names | Local wide-footer controls; narrow layouts retain queue controls. Input tests include Jam permission boundaries. |
| Windows integration | Media keys, taskbar thumbnail buttons, tray restore/transport, correct cleanup | Local native implementation; 40 attach/update/drop cycles preserve USER/GDI counts (3,4 → 3,4). Memory-only SMTC cover publication added; native set/clear probe, PNG size rejection and app encoding checks pass. Explorer restart, visible system-cover presentation and physical keys under game focus still require interactive validation. |
| Login | Ordinary users connect without copying headers or developer-mode extension setup; account choice and reconnection remain reliable | Native helper passed interactive Google sign-in, explicit Music profile choice, verified encrypted import and post-popup Library access; user confirmed correct profile. Candidate CI packages the helper and checks its installed lifecycle. Broader cancellation/crash/adoption and sustained renewal coverage remains. Companion fallback retained. See LOGIN_REPLACEMENT.md. |
| Session longevity | Closed-browser playback, renewal, account switching, expiry recovery; credentials remain local | New native-login session passed a 900-second muted live run after popup shutdown: six decoded starts, four natural transitions, six radio refills, pause/resume, skip/seek, and one persisted cookie update. This does not prove weekly session duration. Longer renewal and account-switch recovery remain. |
| Library and discovery | Saved albums/artists, useful Explore and cross-device listening history, search type filters | Read-only album/artist/history and typed song/album/artist/playlist search implemented. Live responses and fetched-art previews pass for all four search types; native controls/menu actions pass at desktop/minimum sizes in both themes. Search caps at 40; library/history at 200. Home ranking stays separate. Explore new releases, charts, moods and genres remain. |
| Collection management | Create, rename and delete owned playlists, plus existing song add/remove | Song entry add/remove is implemented. Whole-playlist creation, rename and deletion remain; destructive actions need explicit user confirmation and account-bound requests without automatic replay. |
| Music-only content | Prefer song counterparts when appropriate, and optional non-music segment skipping | Native playback is already audio-only and typed Music search requests songs. Counterpart resolution for music-video links and opt-in SponsorBlock segment skipping remain; do not change explicitly selected versions silently. |
| Browse history and queue discovery | Back/forward navigation and related-song queue browsing | Artist Back and radio/autoplay are implemented. General Alt+Left/Right, mouse back/forward, related-song browsing, and measured bounded hover prefetch remain. |
| Resume and performance | Restore queue and paused position; fast skips; bounded prefetch; safe output-device loss | Audit existing behavior, exercise recovery, measure same-PC playback and skipping. Native audio/provider process reductions require benchmarks before selection. |
| Lyrics and appearance | Readable synchronized full-screen lyrics, good light/dark/minimum sizes, no effects running while hidden | Immersive lyrics are locally merged. Native repaint/resource tests exist; verify current combined build. Consider optional system-theme following and restrained artwork accents. |
| Optional desktop features | Useful song notifications and a clear close/background choice without breaking installer shutdown | Not complete. Explicit tray hiding is implemented; closing X still exits normally. Any background-close preference needs an updater bypass and upgrade tests. |
| Privacy and diagnostics | No secrets in logs, payloads or public artifacts; bounded caches and redacted diagnostics | Preserve existing protections; re-audit new login and native code. A remote-control HTTP API is not needed for parity unless it adds a demonstrated user benefit. |
| Release quality | Actual upgrade from a released installation, settings/session preservation, clean install/uninstall | Login-helper payload passed archive/checksum/privacy tests, installer bootstrap tests, and the full isolated Windows install/update lifecycle suite. Candidate CI now includes native login; actual packaged upgrade and installed helper checks gate stable promotion. This batch remains unpublished. |

## Performance evidence

Do not compare one app's advertised memory with another app's unrelated run. Use
the same Windows PC, account, tracks, playback quality, queue, artwork state and
duration. Include the player and all owned provider/audio/WebView helper processes;
report private bytes and working set separately, startup/skip latency, peak login
usage, steady playback and post-login cleanup. Repeat after a sustained session.
Measure cache and build output separately from installed/runtime data.

## Login prototype acceptance

Keep browser-companion fallback while testing the replacement. A native helper
must have a bounded lifetime, verified ownership, no general web-to-native bridge,
no password/form capture or user-agent spoofing, and no credentials in command
lines, stdout, diagnostics or temporary plaintext files. Transfer only required
session data through an authenticated local channel to the existing encrypted
store. Verify the selected Music profile before committing the connection.

Test success, cancel, timeout, network failure, helper crash, parent exit and
account switching. Confirm the whole owned WebView process tree exits after each
path; wait for BrowserProcessExited rather than assuming controller Close frees
all processes immediately. Retain or delete profile data only within Tide's own
verified directory. Test normal Google login first; do not defeat a rejected
embedded flow. OAuth API access alone does not supply the current playback
backend's Music browser session.
