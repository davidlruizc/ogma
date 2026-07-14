import { appName, gitConfig, siteDescription, siteUrl } from '@/lib/shared';
import { AppDemo } from './app-demo';

const repoUrl = `https://github.com/${gitConfig.user}/${gitConfig.repo}`;

// Structured data for rich results — Ogma is a free cross-platform desktop app.
const jsonLd = {
  '@context': 'https://schema.org',
  '@type': 'SoftwareApplication',
  name: appName,
  applicationCategory: 'BusinessApplication',
  operatingSystem: 'macOS, Windows',
  description: siteDescription,
  url: siteUrl,
  image: `${siteUrl}/opengraph-image`,
  offers: {
    '@type': 'Offer',
    price: '0',
    priceCurrency: 'USD',
  },
  author: {
    '@type': 'Person',
    name: 'David Ruiz',
    url: repoUrl,
  },
};

// Hero equalizer — matches the app's live waveform; CSS animates each bar.
const heroBars = [12, 22, 34, 44, 30, 18, 38, 26, 42, 20, 32, 14];

// Platform brand marks, inlined so they need no asset requests. The Apple
// mark (simple-icons path) inherits currentColor; the Microsoft mark keeps
// its official four brand colors.
const AppleIcon = () => (
  <svg viewBox="0 0 24 24" width="20" height="24" fill="currentColor" aria-hidden>
    <path d="M12.152 6.896c-.948 0-2.415-1.078-3.96-1.04-2.04.027-3.91 1.183-4.961 3.014-2.117 3.675-.546 9.103 1.519 12.09 1.013 1.454 2.208 3.09 3.792 3.039 1.52-.065 2.09-.987 3.935-.987 1.831 0 2.35.987 3.96.948 1.637-.026 2.676-1.48 3.676-2.948 1.156-1.688 1.636-3.325 1.662-3.415-.039-.013-3.182-1.221-3.22-4.857-.026-3.04 2.48-4.494 2.597-4.559-1.429-2.09-3.623-2.324-4.39-2.376-2-.156-3.675 1.09-4.61 1.09zM15.53 3.83c.843-1.012 1.4-2.427 1.245-3.83-1.207.052-2.662.805-3.532 1.818-.78.896-1.454 2.338-1.273 3.714 1.338.104 2.715-.688 3.559-1.701"/>
  </svg>
);
const MicrosoftIcon = () => (
  <svg viewBox="0 0 24 24" width="19" height="19" aria-hidden>
    <rect x="0" y="0" width="11.2" height="11.2" fill="#f25022" />
    <rect x="12.8" y="0" width="11.2" height="11.2" fill="#7fba00" />
    <rect x="0" y="12.8" width="11.2" height="11.2" fill="#00a4ef" />
    <rect x="12.8" y="12.8" width="11.2" height="11.2" fill="#ffb900" />
  </svg>
);

const platforms = [
  { icon: <AppleIcon />, name: 'macOS' },
  { icon: <MicrosoftIcon />, name: 'Windows' },
];

const features = [
  {
    tag: 'RECORD',
    title: 'Crash-safe capture',
    body: 'Audio is written in rotating 5-minute WAV segments, so a crash or power loss never costs you more than moments.',
  },
  {
    tag: 'TRANSCRIBE',
    title: 'Transcripts with speakers',
    body: 'Whisper stitches one timestamped transcript; Claude attributes speakers, and you rename them to real names in a click.',
  },
  {
    tag: 'SUMMARIZE',
    title: 'Highlights & action items',
    body: 'Every meeting is distilled into key decisions, timestamped highlights, and a clean, checkable action-item list.',
  },
  {
    tag: 'SYNC',
    title: 'Straight to Notion',
    body: 'Finished notes land in your Notion workspace automatically — formatted, searchable, and canonical across devices.',
  },
  {
    tag: 'CONNECT',
    title: 'MCP server built in',
    body: 'The same app runs as a local MCP server, so Claude can search every conversation you have recorded and pull action items on demand.',
  },
  {
    tag: 'YOUR DATA',
    title: 'Your data, your keys',
    body: 'Meetings are stored locally in SQLite; transcription and notes run through OpenAI and Anthropic with your own API keys. You choose what syncs to Notion.',
  },
];

export default function HomePage() {
  return (
    <>
      <script
        type="application/ld+json"
        dangerouslySetInnerHTML={{ __html: JSON.stringify(jsonLd) }}
      />

      {/* nav */}
      <nav className="lp-nav">
        <a href="#top" className="lp-brand" aria-label="Ogma home">
          <img className="lp-logo" src="/ogma-logo.png" alt="Ogma logo" />
          <span className="lp-wordmark">ogma</span>
        </a>
        <span className="lp-spacer" />
        <div className="lp-nav-links">
          <a href="#features">FEATURES</a>
          <a href="#download">DOWNLOAD</a>
          <a href="/docs">DOCS</a>
          <a href={repoUrl} target="_blank" rel="noreferrer">
            GITHUB
          </a>
        </div>
      </nav>

      {/* hero */}
      <header className="lp-hero" id="top">
        <span className="lp-eyebrow">DESKTOP MEETING RECORDER</span>
        <h1 className="lp-title">Every meeting, remembered.</h1>
        <p className="lp-subtitle">
          Ogma records, transcribes, and summarizes your meetings — then syncs
          the notes straight to Notion, and lets Claude query them over MCP.
          Crash-safe. Your own API keys. Yours.
        </p>

        <div className="lp-wave" aria-hidden>
          {heroBars.map((hgt, i) => (
            <span
              key={i}
              style={{
                height: `${hgt}px`,
                animationDuration: `${1.1 + (i % 5) * 0.22}s`,
                animationDelay: `${i * 0.09}s`,
              }}
            />
          ))}
        </div>

        <div className="lp-downloads" id="download">
          {platforms.map((p) => (
            <div key={p.name} className="lp-dl">
              <button type="button" className="lp-dl-btn" disabled aria-disabled>
                <span className="lp-dl-glyph">{p.icon}</span>
                <span className="lp-dl-meta">
                  <span className="lp-dl-for">DOWNLOAD FOR</span>
                  <span className="lp-dl-name">{p.name}</span>
                </span>
              </button>
              <span className="lp-badge-soon">COMING SOON</span>
            </div>
          ))}
        </div>

        <p className="lp-fineprint">
          crash-safe · rotating 5-min WAV segments · 16 kHz mono · watch{' '}
          <a href={`${repoUrl}/releases`} target="_blank" rel="noreferrer">
            GitHub Releases
          </a>{' '}
          for the first build
        </p>
      </header>

      {/* animated product demo */}
      <section className="lp-shot">
        <div className="lp-shot-glow" aria-hidden />
        <div className="lp-shot-frame">
          <AppDemo />
        </div>
      </section>

      {/* features */}
      <section className="lp-features" id="features">
        <h2 className="lp-features-title">What it does</h2>
        <div className="lp-feature-grid">
          {features.map((f) => (
            <div key={f.tag} className="lp-feature">
              <span className="lp-feature-tag">{f.tag}</span>
              <h3 className="lp-feature-title">{f.title}</h3>
              <p className="lp-feature-body">{f.body}</p>
            </div>
          ))}
        </div>
      </section>

      {/* footer */}
      <footer className="lp-footer">
        <img className="lp-logo" src="/ogma-logo.png" alt="Ogma logo" />
        <span className="lp-wordmark">ogma</span>
        <span className="lp-spacer" />
        <span className="lp-footer-copy">© 2026 Ogma · every meeting, remembered</span>
      </footer>
    </>
  );
}
