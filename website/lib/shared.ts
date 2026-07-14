export const appName = 'Ogma';
export const docsRoute = '/docs';
export const docsImageRoute = '/og/docs';
export const docsContentRoute = '/llms.mdx/docs';

// Canonical site origin — feeds metadataBase, canonical URLs, OG/Twitter image
// URLs, robots.txt, and sitemap.xml. Override per-environment with
// NEXT_PUBLIC_SITE_URL (e.g. a preview deploy) without touching code.
export const siteUrl = (
  process.env.NEXT_PUBLIC_SITE_URL ?? 'https://ogma.my'
).replace(/\/$/, '');

export const siteTagline = 'Every meeting, remembered.';
export const siteDescription =
  'Record in-person meetings, get speaker-labeled transcripts and AI meeting notes, and have everything land in Notion — queryable by Claude via MCP. Crash-safe, cross-platform, and driven by your own API keys.';

export const siteKeywords = [
  'meeting recorder',
  'AI meeting notes',
  'meeting transcription',
  'speaker-labeled transcript',
  'Whisper transcription',
  'Claude AI notes',
  'Notion sync',
  'MCP server',
  'desktop meeting app',
  'action items',
  'macOS meeting recorder',
  'Windows meeting recorder',
];

export const gitConfig = {
  user: 'davidlruizc',
  repo: 'ogma',
  branch: 'main',
};
