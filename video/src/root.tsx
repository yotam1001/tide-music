import {Composition} from 'remotion';
import {TideProductDemo, TOTAL_FRAMES} from './tide-product-demo';

export const Root = () => (
  <Composition
    id="TideProductDemo"
    component={TideProductDemo}
    durationInFrames={TOTAL_FRAMES}
    fps={30}
    width={1280}
    height={720}
  />
);
