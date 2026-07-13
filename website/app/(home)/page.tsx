import Link from 'next/link';
import { gitConfig } from '@/lib/shared';

const repoUrl = `https://github.com/${gitConfig.user}/${gitConfig.repo}`;

const platforms = [
  {
    name: 'Windows',
    detail: 'Windows 10 or later · .msi installer',
    icon: (
      <svg viewBox="0 0 24 24" fill="currentColor" className="size-5" aria-hidden>
        <path d="M3 5.4 10.3 4.4v7.1H3V5.4Zm0 13.2 7.3 1v-7H3v6Zm8.2 1.1L21 21V12.5h-9.8v8.2Zm0-16.4v8h9.8V3l-9.8 1.3Z" />
      </svg>
    ),
  },
  {
    name: 'macOS',
    detail: 'Apple Silicon · .dmg · experimental',
    icon: (
      <svg viewBox="0 0 24 24" fill="currentColor" className="size-5" aria-hidden>
        <path d="M16.4 12.6c0-2.3 1.9-3.4 2-3.5-1.1-1.6-2.8-1.8-3.4-1.8-1.4-.1-2.8.8-3.5.8-.7 0-1.9-.8-3-.8-1.6 0-3 .9-3.8 2.3-1.6 2.8-.4 7 1.2 9.3.8 1.1 1.7 2.4 2.9 2.3 1.2 0 1.6-.7 3-.7s1.8.7 3 .7c1.2 0 2-1.1 2.8-2.2.9-1.3 1.2-2.5 1.3-2.6-.1 0-2.4-1-2.5-3.8ZM14.1 5.4c.6-.8 1-1.9.9-3-.9 0-2 .6-2.7 1.4-.6.7-1.1 1.8-.9 2.9 1 .1 2-.5 2.7-1.3Z" />
      </svg>
    ),
  },
];

const features = [
  {
    title: 'Record, crash-safe',
    body: 'Capture 1–3 hour in-person meetings from the default mic. Audio is written as rotating 5-minute segments, so a crash loses minutes, not the meeting.',
  },
  {
    title: 'Transcribe & label speakers',
    body: 'OpenAI Whisper stitches one timestamped transcript; Claude attributes speakers and you rename them to real names.',
  },
  {
    title: 'AI notes, synced to Notion',
    body: 'TL;DR, decisions, action items and quote highlights — pushed to a Notion database as the canonical, cross-device store.',
  },
  {
    title: 'Ask Claude via MCP',
    body: 'The same app runs as a local MCP server, so Claude can search your transcripts and pull action items on demand.',
  },
];

export default function HomePage() {
  return (
    <main className="flex flex-col items-center flex-1 px-4">
      {/* Hero */}
      <section className="flex flex-col items-center text-center pt-20 pb-14 max-w-2xl">
        <span className="mb-5 rounded-full border px-3 py-1 text-xs font-medium text-fd-muted-foreground">
          Alpha · Windows &amp; macOS
        </span>
        <h1 className="text-5xl font-bold tracking-tight mb-5">Ogma</h1>
        <p className="text-lg text-fd-muted-foreground mb-8 leading-relaxed">
          Record in-person meetings, get speaker-labeled transcripts and AI
          meeting notes, and have everything land in Notion — queryable by Claude
          via MCP.
        </p>
        <div className="flex flex-wrap gap-3 justify-center">
          <Link
            href="/docs/getting-started"
            className="rounded-full bg-fd-primary px-6 py-2.5 font-medium text-fd-primary-foreground transition-opacity hover:opacity-90"
          >
            Get started
          </Link>
          <Link
            href="/docs"
            className="rounded-full border px-6 py-2.5 font-medium transition-colors hover:bg-fd-accent"
          >
            Read the docs
          </Link>
        </div>
      </section>

      {/* Downloads */}
      <section className="w-full max-w-3xl pb-16">
        <div className="flex items-center justify-center gap-2 mb-5">
          <h2 className="text-sm font-semibold uppercase tracking-wide text-fd-muted-foreground">
            Download
          </h2>
        </div>
        <div className="grid gap-4 sm:grid-cols-2">
          {platforms.map((p) => (
            <div
              key={p.name}
              className="flex items-center gap-4 rounded-xl border bg-fd-card p-4"
            >
              <div className="flex size-11 shrink-0 items-center justify-center rounded-lg bg-fd-accent text-fd-accent-foreground">
                {p.icon}
              </div>
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="font-medium">{p.name}</span>
                  <span className="rounded-full bg-fd-accent px-2 py-0.5 text-[11px] font-medium text-fd-muted-foreground">
                    Coming soon
                  </span>
                </div>
                <p className="truncate text-sm text-fd-muted-foreground">
                  {p.detail}
                </p>
              </div>
              <button
                type="button"
                disabled
                aria-disabled="true"
                className="shrink-0 cursor-not-allowed rounded-full border px-4 py-2 text-sm font-medium text-fd-muted-foreground opacity-60"
              >
                Download
              </button>
            </div>
          ))}
        </div>
        <p className="mt-4 text-center text-sm text-fd-muted-foreground">
          Installers aren&apos;t published yet. Watch{' '}
          <a
            href={`${repoUrl}/releases`}
            className="font-medium text-fd-foreground underline underline-offset-4"
            target="_blank"
            rel="noreferrer"
          >
            GitHub Releases
          </a>{' '}
          for the first build.
        </p>
      </section>

      {/* Features */}
      <section className="w-full max-w-4xl border-t pt-14 pb-20">
        <div className="grid gap-6 sm:grid-cols-2">
          {features.map((f) => (
            <div key={f.title} className="rounded-xl border bg-fd-card p-6">
              <h3 className="mb-2 font-semibold">{f.title}</h3>
              <p className="text-sm leading-relaxed text-fd-muted-foreground">
                {f.body}
              </p>
            </div>
          ))}
        </div>
        <div className="mt-12 flex flex-col items-center gap-4 text-center">
          <p className="text-fd-muted-foreground">
            Cloud processing runs about $1.25 per 3-hour meeting — a deliberate
            trade so the app works the same on desktop and phone.
          </p>
          <div className="flex flex-wrap gap-3 justify-center">
            <Link
              href="/docs"
              className="rounded-full bg-fd-primary px-6 py-2.5 font-medium text-fd-primary-foreground transition-opacity hover:opacity-90"
            >
              Read the docs
            </Link>
            <a
              href={repoUrl}
              target="_blank"
              rel="noreferrer"
              className="rounded-full border px-6 py-2.5 font-medium transition-colors hover:bg-fd-accent"
            >
              View on GitHub
            </a>
          </div>
        </div>
      </section>
    </main>
  );
}
