import { ImageResponse } from 'next/og';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { appName, siteTagline } from '@/lib/shared';

export const alt = `${appName} — ${siteTagline}`;
export const size = { width: 1200, height: 630 };
export const contentType = 'image/png';

// Inline the logo so the card renders with no network fetch at build time.
const logoDataUri = `data:image/png;base64,${readFileSync(
  join(process.cwd(), 'app', 'icon.png'),
).toString('base64')}`;

export default function OpengraphImage() {
  return new ImageResponse(
    (
      <div
        style={{
          width: '100%',
          height: '100%',
          display: 'flex',
          flexDirection: 'column',
          justifyContent: 'space-between',
          padding: '80px',
          background:
            'radial-gradient(120% 120% at 15% 10%, #14161c 0%, #0a0a0a 60%)',
          color: '#f5f5f4',
          fontFamily: 'sans-serif',
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: '28px' }}>
          <img src={logoDataUri} width={104} height={104} alt="" />
          <span
            style={{ fontSize: 60, fontWeight: 700, letterSpacing: '-0.02em' }}
          >
            {appName.toLowerCase()}
          </span>
        </div>

        <div style={{ display: 'flex', flexDirection: 'column' }}>
          <span
            style={{
              fontSize: 84,
              fontWeight: 700,
              lineHeight: 1.05,
              letterSpacing: '-0.03em',
            }}
          >
            {siteTagline}
          </span>
          <span
            style={{
              marginTop: 28,
              fontSize: 32,
              lineHeight: 1.35,
              color: '#a1a1aa',
              maxWidth: 900,
            }}
          >
            Record, transcribe, and summarize meetings — synced to Notion and
            queryable by Claude over MCP.
          </span>
        </div>

        <div
          style={{
            display: 'flex',
            gap: '16px',
            fontSize: 24,
            color: '#71717a',
            letterSpacing: '0.04em',
            textTransform: 'uppercase',
          }}
        >
          <span>Crash-safe capture</span>
          <span>·</span>
          <span>Your own API keys</span>
          <span>·</span>
          <span>macOS &amp; Windows</span>
        </div>
      </div>
    ),
    size,
  );
}
