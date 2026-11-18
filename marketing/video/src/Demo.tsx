import {
  AbsoluteFill,
  Img,
  interpolate,
  OffthreadVideo,
  Sequence,
  spring,
  staticFile,
  useCurrentFrame,
  useVideoConfig,
} from 'remotion';
import { loadFont } from '@remotion/google-fonts/Inter';
import marks from '../public/recording-light.json';

const { fontFamily } = loadFont('normal', { weights: ['400', '600', '800'], subsets: ['latin'] });

const FPS = 30;
const SRC = staticFile('recording-light.mp4');
// The recording is a 1440x810 desktop at scale 2.
const REC_W = 2880;
const REC_H = 1620;

const sec = (ms: number) => ms / 1000;
const f = (s: number) => Math.round(s * FPS);

type Cam = { x: number; y: number; zoom: number };

/**
 * A part of the recording, from `from` to `to` seconds, seen through one fixed
 * camera. The camera only changes at a cut, never inside a shot. `sq` places it
 * for the square cut, which sees less width.
 */
type Shot = { from: number; to: number; cam: Cam; sq?: Partial<Cam>; caption?: Caption[] };
type Caption = { at: number; until?: number; title: string; sub?: string; place?: 'bottom-left' | 'bottom-right' | 'left' };

// Points are in recording pixels. The Sheaf window spans x 340-2540, y 122-1562.
const WINDOW: Cam = { x: 1440, y: 805, zoom: 1.13 };
const NOTE: Cam = { x: 1240, y: 700, zoom: 1.7 };
const SEARCH: Cam = { x: 1240, y: 640, zoom: 1.4 };
const CONSOLE: Cam = { x: 1220, y: 845, zoom: 1.35 };

const shots: Shot[] = [
  {
    from: sec(marks.gallery) + 0.3,
    to: sec(marks.opened),
    cam: WINDOW,
    sq: { x: 1110, zoom: 1.0 },
    caption: [{ at: 0.5, title: 'Your notes as a gallery of cards', sub: 'Fast, native notes for GNOME' }],
  },
  {
    from: sec(marks.opened),
    to: sec(marks.back) + 0.2,
    cam: NOTE,
    sq: { x: 1150, zoom: 1.5 },
    caption: [{ at: 0.6, title: 'Edit right in place', sub: 'Checklists, tables, links. Saved as you type.', place: 'bottom-right' }],
  },
  {
    from: sec(marks.search) - 0.6,
    to: sec(marks.clear),
    cam: SEARCH,
    sq: { x: 1030 },
    caption: [
      { at: 0.6, until: sec(marks.filter) - sec(marks.search) + 0.4, title: 'Search that forgives typos', sub: 'Results with every key' },
      { at: sec(marks.filter) - sec(marks.search) + 3.2, title: '#tags and filters, in milliseconds', sub: '~6 ms with 10,000 notes' },
    ],
  },
  {
    from: sec(marks.ls) - 0.8,
    to: sec(marks.end) + 0.3,
    cam: CONSOLE,
    sq: { x: 1340, zoom: 1.2 },
    caption: [{ at: 0.6, title: 'Every note is a plain Markdown file', sub: 'Sync it, grep it, open it in any editor.', place: 'left' }],
  },
];

const INTRO = f(2.6);
const OUTRO = f(4);
// Shots overlap by this much and cross-fade.
const FADE = 14;

const shotFrames = (s: Shot) => f(s.to - s.from);
const sequenceLength = (list: Shot[]) => list.reduce((sum, s) => sum + shotFrames(s), 0) - FADE * (list.length - 1);
const shotsDuration = sequenceLength(shots);
export const demoDuration = INTRO + shotsDuration + OUTRO - FADE;

const camFor = (shot: Shot, square: boolean): Cam => {
  if (!square) return shot.cam;
  const c = { ...shot.cam, ...shot.sq };
  return { ...c, zoom: 1 + (c.zoom - 1) * 0.5 };
};

/** Shows the recording through the shot's fixed camera, filling the frame. */
const Footage: React.FC<{ shot: Shot }> = ({ shot }) => {
  const { width, height } = useVideoConfig();
  const square = width / height < 1.2;
  const cam = camFor(shot, square);
  // Zoom 1 shows the whole recording height.
  const base = Math.max(width / REC_W, height / REC_H) * (square ? 1.08 : 1);
  const s = base * cam.zoom;
  // Keep the camera inside the recording.
  const halfW = width / 2 / s;
  const halfH = height / 2 / s;
  const x = Math.min(Math.max(cam.x, halfW), REC_W - halfW);
  const y = Math.min(Math.max(cam.y, halfH + 64), REC_H - halfH);
  return (
    <AbsoluteFill style={{ overflow: 'hidden', backgroundColor: '#1c3f8f' }}>
      <div
        style={{
          position: 'absolute',
          width: REC_W,
          height: REC_H,
          transformOrigin: '0 0',
          transform: `translate(${width / 2 - x * s}px, ${height / 2 - y * s}px) scale(${s})`,
        }}
      >
        <OffthreadVideo src={SRC} startFrom={f(shot.from)} muted style={{ width: REC_W, height: REC_H }} />
      </div>
    </AbsoluteFill>
  );
};

const CaptionView: React.FC<{ caption: Caption; end: number }> = ({ caption, end }) => {
  const frame = useCurrentFrame();
  const { fps, width, height } = useVideoConfig();
  const start = f(caption.at);
  const stop = caption.until !== undefined ? f(caption.until) : end - 4;
  const enter = spring({ frame: frame - start, fps, config: { damping: 200 }, durationInFrames: 14 });
  const leave = interpolate(frame, [stop - 8, stop], [1, 0], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' });
  const opacity = Math.min(enter, leave);
  if (frame < start || frame > stop) return null;
  const square = width / height < 1.2;
  // The square cut always uses the bottom, the 16:9 cut the place the shot leaves free.
  const place = square ? 'bottom-left' : (caption.place ?? 'bottom-left');
  const where: React.CSSProperties =
    place === 'left'
      ? { left: 56, top: '50%', maxWidth: 400, transform: `translate(0, calc(-50% + ${(1 - enter) * 24}px))` }
      : place === 'bottom-right'
        ? { right: 64, bottom: 64, maxWidth: 760, transform: `translateY(${(1 - enter) * 24}px)` }
        : square
          ? { left: 40, right: 40, bottom: 44, transform: `translateY(${(1 - enter) * 24}px)` }
          : { left: 64, bottom: 64, maxWidth: 820, transform: `translateY(${(1 - enter) * 24}px)` };
  return (
    <div
      style={{
        position: 'absolute',
        ...where,
        opacity,
        padding: '18px 26px',
        borderRadius: 18,
        background: 'rgba(20, 22, 30, 0.84)',
        boxShadow: '0 14px 40px rgba(0,0,0,0.3)',
        color: 'white',
        fontFamily,
      }}
    >
      <div style={{ fontSize: square ? 42 : 44, fontWeight: 800, letterSpacing: -0.6, lineHeight: 1.12 }}>{caption.title}</div>
      {caption.sub ? (
        <div style={{ fontSize: square ? 26 : 27, fontWeight: 400, opacity: 0.82, marginTop: 8, lineHeight: 1.3 }}>{caption.sub}</div>
      ) : null}
    </div>
  );
};

const Fade: React.FC<{ duration: number; children: React.ReactNode; fadeIn?: boolean; fadeOut?: boolean }> = ({
  duration,
  children,
  fadeIn = true,
  fadeOut = true,
}) => {
  const frame = useCurrentFrame();
  const a = fadeIn ? interpolate(frame, [0, FADE], [0, 1], { extrapolateRight: 'clamp' }) : 1;
  const b = fadeOut ? interpolate(frame, [duration - FADE, duration], [1, 0], { extrapolateLeft: 'clamp' }) : 1;
  return <AbsoluteFill style={{ opacity: Math.min(a, b) }}>{children}</AbsoluteFill>;
};

const Title: React.FC<{ lines: string[]; small?: string[]; duration: number }> = ({ lines, small = [], duration }) => {
  const frame = useCurrentFrame();
  const { fps, width, height } = useVideoConfig();
  const pop = spring({ frame, fps, config: { damping: 14, mass: 0.7 } });
  const square = width / height < 1.2;
  return (
    <Fade duration={duration} fadeIn={false}>
      <AbsoluteFill
        style={{
          background: 'radial-gradient(circle at 50% 35%, #2a6fd6 0%, #123a8a 55%, #0b2459 100%)',
          alignItems: 'center',
          justifyContent: 'center',
          fontFamily,
          color: 'white',
          textAlign: 'center',
        }}
      >
        <Img src={staticFile('icon.svg')} style={{ width: square ? 200 : 220, transform: `scale(${pop})`, marginBottom: 28 }} />
        {lines.map((l, i) => (
          <div
            key={l}
            style={{
              fontSize: i === 0 ? 120 : 52,
              fontWeight: i === 0 ? 800 : 600,
              letterSpacing: i === 0 ? -3 : -0.5,
              opacity: interpolate(frame, [6 + i * 6, 18 + i * 6], [0, 1], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' }),
              padding: '0 60px',
              lineHeight: 1.15,
            }}
          >
            {l}
          </div>
        ))}
        {small.map((l, i) => (
          <div
            key={l}
            style={{
              fontSize: 38,
              marginTop: i === 0 ? 34 : 8,
              opacity: interpolate(frame, [20 + i * 6, 32 + i * 6], [0, 0.9], { extrapolateLeft: 'clamp', extrapolateRight: 'clamp' }),
              fontFamily: i === 0 ? fontFamily : 'monospace',
            }}
          >
            {l}
          </div>
        ))}
      </AbsoluteFill>
    </Fade>
  );
};

const Shots: React.FC<{ list: Shot[]; captions: boolean }> = ({ list, captions }) => {
  let start = 0;
  return (
    <>
      {list.map((shot, i) => {
        const d = shotFrames(shot);
        const el = (
          <Sequence key={i} from={start} durationInFrames={d}>
            {/* Each shot fades in over the previous one; only the last fades out. */}
            <Fade duration={d} fadeIn={i > 0} fadeOut={false}>
              <Footage shot={shot} />
              {captions ? shot.caption?.map((c) => <CaptionView key={c.title} caption={c} end={d - FADE} />) : null}
            </Fade>
          </Sequence>
        );
        start += d - FADE;
        return el;
      })}
    </>
  );
};

export const Demo: React.FC = () => (
  <AbsoluteFill style={{ backgroundColor: '#0b2459' }}>
    <Sequence from={INTRO - FADE} durationInFrames={shotsDuration}>
      <Shots list={shots} captions />
    </Sequence>
    <Sequence durationInFrames={INTRO}>
      <Title lines={['Sheaf', 'Fast, native notes for GNOME']} duration={INTRO} />
    </Sequence>
    <Sequence from={INTRO - FADE + shotsDuration - FADE} durationInFrames={OUTRO}>
      <Fade duration={OUTRO} fadeOut={false}>
        <Title
          lines={['Sheaf', 'Plain Markdown. Native speed.']}
          small={['Free and open source · GPL-3.0', 'github.com/taulit/sheaf']}
          duration={OUTRO + FADE}
        />
      </Fade>
    </Sequence>
  </AbsoluteFill>
);

// The GIF: gallery and opening a note, editing it, then the typo search. No captions.
const gifShots: Shot[] = [
  { from: sec(marks.open) - 2.2, to: sec(marks.opened), cam: WINDOW },
  { from: sec(marks.check) - 0.4, to: sec(marks.type) + 5.6, cam: NOTE },
  { from: sec(marks.search) - 0.3, to: sec(marks.filter) - 0.5, cam: SEARCH },
];
export const gifDuration = sequenceLength(gifShots);

export const Gif: React.FC = () => (
  <AbsoluteFill style={{ backgroundColor: '#0b2459' }}>
    <Shots list={gifShots} captions={false} />
  </AbsoluteFill>
);
