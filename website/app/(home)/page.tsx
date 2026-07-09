import Link from 'next/link';

export default function HomePage() {
  return (
    <div className="flex flex-col items-center justify-center text-center flex-1 px-4">
      <h1 className="text-4xl font-bold mb-4">Ogma</h1>
      <p className="text-fd-muted-foreground max-w-xl mb-8">
        Record in-person meetings, get speaker-labeled transcripts and AI
        meeting notes, and have everything land in Notion — queryable by Claude
        via MCP.
      </p>
      <div className="flex gap-3">
        <Link
          href="/docs"
          className="rounded-full bg-fd-primary px-6 py-2.5 font-medium text-fd-primary-foreground"
        >
          Read the docs
        </Link>
        <Link
          href="/docs/getting-started"
          className="rounded-full border px-6 py-2.5 font-medium"
        >
          Get started
        </Link>
      </div>
    </div>
  );
}
