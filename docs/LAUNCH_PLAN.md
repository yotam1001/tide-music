# First preview launch

Early source-preview introductions have been shared. The Windows installer is
available in [v0.1.0-preview.1](https://github.com/yotam1001/tide-music/releases/tag/v0.1.0-preview.1).
Keep future posts consistent with the [release checklist](BETA_RELEASE.md) and
the preview's known limitations.

## Start small

1. Publish the reviewed GitHub prerelease with the EXE, matching Chrome companion,
   connection guide and checksums. Check the public download without maintainer
   access. Update README's publication notice and link to that exact release.
2. Share the demo and repository on the project's or maintainer's own channels,
   using the short draft below after review. Ask for Windows setup/playback
   feedback. Keep the manual Chrome setup visible.
3. Respond to the first setup reports and fix common problems before wider
   promotion. A successful first song and understandable account setup matter
   more than collecting many download clicks.

## Short announcement draft for owned channels

**Tide: a native music player for listening while gaming**

I started building Tide to keep music's RAM use low while gaming, without keeping
a full music website running. It's an open-source Windows player
with native audio, your existing playlists, song radio, a draggable queue, and
a mini mode.

The first preview is for people who are comfortable trying an early app and
giving feedback. YouTube Music connection currently uses a manually loaded Chrome
companion. It's unofficial, and session reliability is still being tested.

The installer handles the playback tools; you don't need Rust, Python or API keys.
I'd especially like feedback on first-time setup, song transitions and the UI.

[Project, setup and downloads](https://github.com/yotam1001/tide-music)

Verify the public release and README link before sharing download announcements.
Use the current demo with its existing attribution; describe its memory comparison as
one measured example rather than a guaranteed saving or gaming-FPS improvement.

## Community outreach needs its own review

Rules reviewed on 2026-10-17; recheck them immediately before posting. These are
possible discussion venues, not permission to cross-post this generated draft.

| Community | Relevant rule and next step |
| --- | --- |
| r/rust | [Rules](https://www.reddit.com/r/rust/about/rules.json) require Rust relevance and useful context, and allow removal of content appearing AI-generated. A maintainer-written technical discussion could cover the Rust/Slint/native-audio design and measured tradeoffs. Do not paste this generated announcement there. |
| r/opensource | [Rules](https://www.reddit.com/r/opensource/about/rules.json) prohibit AI-generated content and excessive promotion. Do not send this generated copy there. Any original human submission needs its own rules review, an open-source license and genuine follow-up discussion. |

Avoid identical posts across communities, unrelated issue comments, unsolicited
messages or promises of ad blocking/Premium access. No external post is authorized
by preparing this file; the user reviews the actual text and destination first.
