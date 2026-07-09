import { RootProvider } from 'fumadocs-ui/provider/next';
import './global.css';
import { Inter } from 'next/font/google';
import type { Metadata } from 'next';

export const metadata: Metadata = {
  title: {
    template: '%s | Ogma',
    default: 'Ogma — meeting recorder with AI notes',
  },
  description:
    'Record in-person meetings, get speaker-labeled transcripts and AI meeting notes, and have everything land in Notion — queryable by Claude via MCP.',
};

const inter = Inter({
  subsets: ['latin'],
});

export default function Layout({ children }: LayoutProps<'/'>) {
  return (
    <html lang="en" className={inter.className} suppressHydrationWarning>
      <body className="flex flex-col min-h-screen">
        <RootProvider>{children}</RootProvider>
      </body>
    </html>
  );
}
