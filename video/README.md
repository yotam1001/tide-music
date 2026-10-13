# Tide Music product demo

A 30-second, 1280 × 720, 30 fps product demo built with Remotion and the MIT-licensed [snapcn](https://github.com/shadcn-labs/snapcn) `text-reveal`, `screen-recording`, and `logo-flicker` components.

The UI plates in `public/screens/` are copied from native screenshots with synthetic visual-test metadata; playback is inactive. The video is a visual walkthrough of the current prototype, not a recording of live playback.

```powershell
cd video
npm install
python -m pip install -r requirements.txt
python scripts/generate_music.py
npm run check
npm run render
```

Output: `../dist/tide-product-demo.mp4`. The published copy is `../docs/media/tide-product-demo.mp4`. Use `npm run studio` to edit or preview the composition.

The copied snapcn component source in `components/snap-cn/` and `lib/snap-cn-ui/` remains under snapcn's MIT license, attributed in `SNAPCN_LICENSE.txt`.
