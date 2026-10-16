# Promo video source

The [HyperFrames](https://hyperframes.heygen.com) project for the promo video in [`docs/media/`](../docs/media/). [`index.html`](index.html) is the whole composition: 48.6 s, 1920×1080, rendered at 60 fps. Animation uses GSAP on one paused timeline, cut to the music's beat grid.

## Assets not in the repo

Large or third-party media is not committed. Put these files in place before you preview or render:

| Path | What it is |
|---|---|
| `assets/fonts/segoeui.ttf`, `seguisb.ttf`, `segoeuib.ttf`, `seguibl.ttf` | Segoe UI, copied from `C:\Windows\Fonts` (Microsoft-licensed, not redistributable) |
| `assets/ui/{playlist,transport,radio,queue,mini}.mp4` | Screen recordings of Tide (dark theme, 1140×800 window, 60 fps) |
| `assets/game/s1-night.mp4`, `s6-day.mp4` | Gameplay footage, 1920×1080 at 60 fps |
| `assets/audio/bed.wav` | Edit of Lensko – "Let's Go!" [NCS Release]. The cut points are in `assets/audio/bed.json`; see below |
| `assets/audio/sfx/*.wav` | Generated with `python scripts/make_sfx.py assets/audio/sfx` (numpy, seeded) |

To build the music bed from the NCS track:

```bash
ffmpeg -i lensko-lets-go.mp3 -filter_complex "[0]atrim=84.683:112.832,asetpts=PTS-STARTPTS[a];[0]atrim=151.957:163.22,asetpts=PTS-STARTPTS[b];[0]atrim=176.332:185.602,asetpts=PTS-STARTPTS[c];[a][b]acrossfade=d=0.03[ab];[ab][c]acrossfade=d=0.03,afade=t=in:d=0.3,afade=t=out:st=47.02:d=1.6" -ar 48000 assets/audio/bed.wav
```

`scripts/beats.py` (librosa) writes beat times; the edited bed's beats are in `assets/audio/bed_beats.txt`.

## Preview and render

Needs Node 22+ and ffmpeg.

```bash
npx --yes hyperframes@0.8.157 preview
npx --yes hyperframes@0.8.157 check
npx --yes hyperframes@0.8.157 render -q delivery -f 60 -o renders/tide-promo.mp4
```

The published MP4 normalizes the render to about −14 LUFS with `-af "volume=-3.1dB,alimiter=limit=0.89:level=false"`, then re-encodes it for the web with x264 at CRF 24.

Music: Lensko – Let's Go! [NCS Release], provided by NoCopyrightSounds.
