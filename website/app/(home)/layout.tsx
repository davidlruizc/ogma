import { Tinos, Space_Grotesk, IBM_Plex_Mono } from 'next/font/google';
import './landing.css';

const tinos = Tinos({
  subsets: ['latin'],
  weight: ['400', '700'],
  style: ['normal', 'italic'],
  variable: '--font-tinos',
  display: 'swap',
});

const spaceGrotesk = Space_Grotesk({
  subsets: ['latin'],
  weight: ['500', '700'],
  variable: '--font-space',
  display: 'swap',
});

const plexMono = IBM_Plex_Mono({
  subsets: ['latin'],
  weight: ['400', '500', '600'],
  variable: '--font-plex-mono',
  display: 'swap',
});

export default function Layout({ children }: LayoutProps<'/'>) {
  return (
    <div
      className={`lp ${tinos.variable} ${spaceGrotesk.variable} ${plexMono.variable}`}
    >
      {children}
    </div>
  );
}
