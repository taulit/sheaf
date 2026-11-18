import { Composition } from 'remotion';
import { Demo, demoDuration, gifDuration, Gif } from './Demo';
import { OgImage } from './OgImage';

export const FPS = 30;

export const RemotionRoot: React.FC = () => (
  <>
    {/* Reddit and the docs site: 16:9, captions burned in, no sound. */}
    <Composition id="SheafDemo" component={Demo} durationInFrames={demoDuration} fps={FPS} width={1920} height={1080} />
    {/* Square cut for feeds that crop to 1:1. */}
    <Composition id="SheafDemoSquare" component={Demo} durationInFrames={demoDuration} fps={FPS} width={1080} height={1080} />
    {/* Short loop without intro and outro, turned into the README GIF by scripts/make-gif.sh. */}
    <Composition id="SheafGif" component={Gif} durationInFrames={gifDuration} fps={FPS} width={1200} height={750} />
    <Composition id="OgImage" component={OgImage} durationInFrames={1} fps={FPS} width={1200} height={630} />
  </>
);
