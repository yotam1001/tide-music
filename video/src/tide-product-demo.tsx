import type {ReactNode} from 'react';
import {AbsoluteFill, Audio, Img, interpolate, staticFile, useCurrentFrame} from 'remotion';
import {fade} from '@remotion/transitions/fade';
import {linearTiming, TransitionSeries} from '@remotion/transitions';
import {TextReveal} from '@/components/snap-cn/text-reveal';
import {ScreenRecording, type CameraMove} from '@/components/snap-cn/screen-recording';
import {LogoFlicker} from '@/components/snap-cn/logo-flicker';

const BG = '#101217';
const PANEL = '#17191f';
const ACCENT = '#ad385e';
const WHITE = '#f6f3f4';
const MUTED = '#b4adb3';
const FONT = 'Inter, "Segoe UI", Arial, sans-serif';
const TRANSITION_FRAMES = 12;
const DURATIONS = [90, 130, 130, 130, 130, 155, 130, 100] as const;
export const TOTAL_FRAMES = DURATIONS.reduce<number>((sum, duration) => sum + duration, 0) - TRANSITION_FRAMES * (DURATIONS.length - 1);

const shot = (name: string) => staticFile(`screens/${name}.png`);

function Stage({children}: {children: ReactNode}) {
  return (
    <AbsoluteFill style={{backgroundColor: BG, color: WHITE, fontFamily: FONT}}>
      {children}
    </AbsoluteFill>
  );
}

function Opening() {
  const frame = useCurrentFrame();
  const markOpacity = interpolate(frame, [8, 24], [0, 1], {extrapolateRight: 'clamp'});
  const subOpacity = interpolate(frame, [50, 68], [0, 1], {extrapolateRight: 'clamp'});
  return (
    <Stage>
      <div style={{position: 'absolute', left: 0, right: 0, top: 165, bottom: 0}}>
        <TextReveal text="Meet Tide Music" fontSize={78} color={WHITE} fontWeight={700} mode="dark" />
      </div>
      <Img src={staticFile('tide-icon.png')} style={{position: 'absolute', width: 92, height: 92, left: 594, top: 120, opacity: markOpacity}} />
      <div style={{position: 'absolute', top: 488, left: 0, right: 0, textAlign: 'center', fontSize: 24, color: MUTED, opacity: subOpacity}}>
        A native Windows music player
      </div>
    </Stage>
  );
}

type FeatureProps = {
  src: string;
  eyebrow: string;
  title: string;
  body: string;
  camera: CameraMove[];
  light?: boolean;
};

function Feature({src, eyebrow, title, body, camera, light = false}: FeatureProps) {
  const frame = useCurrentFrame();
  const copyOpacity = interpolate(frame, [13, 30], [0, 1], {extrapolateRight: 'clamp'});
  const copyX = interpolate(frame, [13, 30], [22, 0], {extrapolateRight: 'clamp'});
  return (
    <Stage>
      <div style={{position: 'absolute', top: 50, left: 28, width: 931, height: 620, overflow: 'hidden', borderRadius: 14, border: '1px solid #343039', background: PANEL, boxShadow: '0 14px 38px #0006'}}>
        <ScreenRecording
          src={src}
          sourceAspect={1.5}
          fit="contain"
          backdropColor={light ? '#f2eee9' : BG}
          camera={camera}
          entrance="fade"
        />
      </div>
      <div style={{position: 'absolute', top: 152, left: 987, width: 254, opacity: copyOpacity, transform: `translateX(${copyX}px)`}}>
        <div style={{width: 35, height: 4, backgroundColor: ACCENT, borderRadius: 2, marginBottom: 24}} />
        <div style={{fontSize: 14, color: '#cf8da3', fontWeight: 700, marginBottom: 18}}>{eyebrow}</div>
        <div style={{fontSize: 38, fontWeight: 700, lineHeight: 1.08, letterSpacing: -1.2, marginBottom: 23}}>{title}</div>
        <div style={{fontSize: 19, lineHeight: 1.4, color: MUTED}}>{body}</div>
      </div>
      <div style={{position: 'absolute', bottom: 22, right: 39, fontSize: 14, color: '#887e86'}}>Tide Music</div>
    </Stage>
  );
}

const shots = [shot('full'), shot('search'), shot('library'), shot('playlist-detail'), shot('queue'), shot('light')];

function Closing() {
  return (
    <Stage>
      <LogoFlicker
        logoSrc={staticFile('tide-icon.png')}
        brandName="Tide Music"
        images={shots}
        background={BG}
        mode="dark"
      />
    </Stage>
  );
}

export function TideProductDemo() {
  const timing = linearTiming({durationInFrames: TRANSITION_FRAMES});
  return (
    <AbsoluteFill style={{backgroundColor: BG}}>
      <Audio src={staticFile('music-bed.wav')} volume={0.8} />
      <TransitionSeries>
        <TransitionSeries.Sequence durationInFrames={DURATIONS[0]}><Opening /></TransitionSeries.Sequence>
        <TransitionSeries.Transition presentation={fade()} timing={timing} />
        <TransitionSeries.Sequence durationInFrames={DURATIONS[1]}>
          <Feature src={shot('full')} eyebrow="01 / HOME" title="Music, at a glance" body="Home, search, and playback in one native window." camera={[{at: 25, duration: 28, zoom: 1.22, x: 0.28, y: 0.29}]} />
        </TransitionSeries.Sequence>
        <TransitionSeries.Transition presentation={fade()} timing={timing} />
        <TransitionSeries.Sequence durationInFrames={DURATIONS[2]}>
          <Feature src={shot('search')} eyebrow="02 / SEARCH" title="Find your next track" body="Search music or paste a link." camera={[{at: 28, duration: 30, zoom: 1.35, x: 0.34, y: 0.23}]} />
        </TransitionSeries.Sequence>
        <TransitionSeries.Transition presentation={fade()} timing={timing} />
        <TransitionSeries.Sequence durationInFrames={DURATIONS[3]}>
          <Feature src={shot('library')} eyebrow="03 / LIBRARY" title="Keep favorites close" body="Open playlists and Liked Songs." camera={[{at: 28, duration: 30, zoom: 1.28, x: 0.40, y: 0.29}]} />
        </TransitionSeries.Sequence>
        <TransitionSeries.Transition presentation={fade()} timing={timing} />
        <TransitionSeries.Sequence durationInFrames={DURATIONS[4]}>
          <Feature src={shot('playlist-detail')} eyebrow="04 / PLAYLISTS" title="Press play or shuffle" body="Preview the tracks before you listen." camera={[{at: 27, duration: 30, zoom: 1.28, x: 0.38, y: 0.28}]} />
        </TransitionSeries.Sequence>
        <TransitionSeries.Transition presentation={fade()} timing={timing} />
        <TransitionSeries.Sequence durationInFrames={DURATIONS[5]}>
          <Feature src={shot('queue')} eyebrow="05 / QUEUE" title="Shape what plays next" body="Reorder tracks, repeat, and control autoplay." camera={[{at: 30, duration: 34, zoom: 1.34, x: 0.77, y: 0.38}, {at: 100, duration: 27, zoom: 1.34, x: 0.77, y: 0.62}]} />
        </TransitionSeries.Sequence>
        <TransitionSeries.Transition presentation={fade()} timing={timing} />
        <TransitionSeries.Sequence durationInFrames={DURATIONS[6]}>
          <Feature src={shot('light')} eyebrow="06 / PERSONALIZE" title="Make it yours" body="Switch between dark and light views." camera={[{at: 26, duration: 30, zoom: 1.18, x: 0.48, y: 0.36}]} light />
        </TransitionSeries.Sequence>
        <TransitionSeries.Transition presentation={fade()} timing={timing} />
        <TransitionSeries.Sequence durationInFrames={DURATIONS[7]}><Closing /></TransitionSeries.Sequence>
      </TransitionSeries>
    </AbsoluteFill>
  );
}
