# Publish Windows releases

This is the maintainer checklist. For normal use, start with [Install Tide](INSTALLATION.md).

## What is available now?

- The maintainer requested stable **v0.1.1** on 2026-10-18, including the merged
  gaming controls and default-output recovery changes. Publish only the matching
  checked app/installer as Latest. Physical headset/game tests remain pending.
- [v0.1.0](https://github.com/yotam1001/tide-music/releases/tag/v0.1.0) is the
  stable-channel package requested by the maintainer to test public in-app
  updating from previews. Publish only its matching checked build as Latest.
  This decision does not mark the outstanding human checks below as passed.
- [v0.1.0-preview.2](https://github.com/yotam1001/tide-music/releases/tag/v0.1.0-preview.2)
  is the Windows preview for the Home recommendation fix. Publication requires
  its matching automated checks and package verification to pass.
  [Preview 1](https://github.com/yotam1001/tide-music/releases/tag/v0.1.0-preview.1)
  remains available as the previous public version.
- On 2026-10-17 the maintainer confirmed that the installer worked in their test
  and authorized publication. That report is scoped to the installer; the broader
  manual checks below remain useful for subsequent previews and stable readiness.
- `v0.1.0-beta.1` is an unpublished, older draft with binaries from `f026915`.
  It does not contain the latest UI or in-app updater. Do not publish it as the
  current build or silently replace its reviewed assets.
- Current source identifies itself as `0.1.1`. Never replace the
  published preview assets in place. The verified updater
  implementation is in merged PR #13 (`ce0d142`). Its source `e02e99e` passed
  [push CI](https://github.com/yotam1001/tide-music/actions/runs/39041897082) and
  [PR CI](https://github.com/yotam1001/tide-music/actions/runs/39041899612), including
  the real graphical update/restart lifecycle. Release packages must also pass
  the checks below with their matching documentation.

Use the following checklist for future preview candidates. It is a release
procedure, not a claim that every manual test has already passed.

## 1. Prepare a matching candidate

- [ ] Choose the reviewed `main` commit and record it. Keep experimental Jams
  unmerged until the user has tested it.
- [ ] Make the release tag match `v` plus the Cargo workspace version. For the
  current candidate, use a new stable `v0.1.1` release rather than modifying
  any previously published assets.
- [ ] Require the Windows source checks, preview package and real install/update/
  removal jobs to pass for the candidate. Do not build large Rust caches locally.
- [ ] Get the installer and companion from that same successful run. Verify the
  preview ZIP's allowlist/checksums/privacy and setup's embedded payload.
- [ ] Prepare a new draft with the [release notes](RELEASE_NOTES_DRAFT.md) and:
  **TideSetup.exe**, **tide-browser-chromium.zip**, **CHROME_SETUP.md** copied from
  the current Chrome guide, and **SHA256SUMS.txt** for the downloadable files.
  The full preview ZIP is an optional advanced download. Do not offer Firefox/Zen
  packages as a verified onboarding path.

Use the companion's inner ZIP, whose extracted files include `manifest.json`
directly. A GitHub Actions artifact is an outer download and is not itself the
Chrome loadable folder.

## 2. Finish the human preview check

Record the Windows version, candidate commit, result and exact nonsensitive error
for each step. Use a clean Windows user or machine when possible.

- [ ] Follow only the public getting-started guides. Install through the GUI,
  reach its completion screen, and find Tide in Start.
- [ ] Load the matching Chrome companion, enter its actual ID in setup, pass
  **Check local connection**, and import the intended Google/Music identity.
- [ ] Confirm the account name and expected playlists. Play several songs,
  including natural transitions, manual skips, pause/resume and seek. Close the
  Music tab and continue playback. Test radio/Autoplay beyond the initial queue.
- [ ] Update the installed app through its GUI; verify account and preferences
  remain. Remove it through Windows Settings and check owned integration cleanup.

Automated Windows CI covers real files, Start-menu shortcuts, HKCU registration,
private runtimes, update/restart, local-audio UI and removal. It uses a synthetic
Chrome identity, not a signed-in browser. That is why it does not replace the
fresh-user connection check above.

Physical Fn keys in a focused game, monitor/DPI changes and long-term account
continuity remain tracked in [Release readiness](RELEASE_READINESS.md). Record
results or explicitly accepted preview limitations; do not describe pending
checks as passed. Stable release needs the broader reliability evidence there.

## 3. Publish, then announce

- [ ] Record the concrete candidate, manual results and publication authorization.
  Never infer that a passing installer test completes unrelated manual checks.
- [ ] Update the download links in README, START_HERE, Installation, Chrome
  connection and Troubleshooting. Make sure packaged copies match. If runtime/
  installer code changes, repeat the relevant acceptance checks.
- [ ] Validate the final docs, privacy scan and matching packages. Publish as a
  stable **0.1.1** as Latest for the authorized update. Future preview
  candidates should be prereleases. Do not modify historical assets in place.
- [ ] Check the release and download links without maintainer access. Confirm the
  EXE and companion filenames, checksums and Chrome guide are clear.
- [ ] Review [launch copy and channel notes](LAUNCH_PLAN.md). Approve each external
  post before sending it; keep time available to answer setup reports.

The in-app updater intentionally ignores prereleases. Preview users still install
newer preview installers manually. Do not advertise automatic beta updates.

[Full evidence](VALIDATION.md) · [Security](../SECURITY.md) · [Privacy](../PRIVACY.md)
