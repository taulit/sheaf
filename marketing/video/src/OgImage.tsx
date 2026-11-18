import { AbsoluteFill, Img, staticFile } from 'remotion';
import { loadFont } from '@remotion/google-fonts/Inter';

const { fontFamily } = loadFont('normal', { weights: ['400', '800'], subsets: ['latin'] });

/** Open Graph image of the docs site (1200x630), rendered with `pnpm og`. */
export const OgImage: React.FC = () => (
  <AbsoluteFill
    style={{
      background: 'radial-gradient(circle at 20% 30%, #2a6fd6 0%, #123a8a 60%, #0b2459 100%)',
      fontFamily,
      color: 'white',
      overflow: 'hidden',
    }}
  >
    <div style={{ position: 'absolute', left: 64, top: 70, width: 470 }}>
      <Img src={staticFile('icon.svg')} style={{ width: 112 }} />
      <div style={{ fontSize: 96, fontWeight: 800, letterSpacing: -3, marginTop: 14 }}>Sheaf</div>
      <div style={{ fontSize: 38, fontWeight: 800, lineHeight: 1.18, marginTop: 6, letterSpacing: -0.5 }}>
        Fast, native notes for GNOME
      </div>
      <div style={{ fontSize: 27, opacity: 0.85, lineHeight: 1.35, marginTop: 18 }}>
        Every note is a plain Markdown file. Search in milliseconds.
      </div>
    </div>
    <Img
      src={staticFile('stills/gallery-light.png')}
      style={{
        position: 'absolute',
        left: 560,
        top: 70,
        width: 1000,
        borderRadius: 18,
        boxShadow: '0 30px 80px rgba(0,0,0,0.45)',
        objectFit: 'cover',
        objectPosition: '30% 8%',
        height: 640,
      }}
    />
  </AbsoluteFill>
);
