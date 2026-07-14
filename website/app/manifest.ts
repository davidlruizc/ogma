import type { MetadataRoute } from 'next';
import { appName, siteDescription } from '@/lib/shared';

export default function manifest(): MetadataRoute.Manifest {
  return {
    name: `${appName} — meeting recorder with AI notes`,
    short_name: appName,
    description: siteDescription,
    start_url: '/',
    display: 'standalone',
    background_color: '#0a0a0a',
    theme_color: '#0a0a0a',
    categories: ['productivity', 'utilities'],
    icons: [
      { src: '/icon.png', sizes: '512x512', type: 'image/png' },
      {
        src: '/apple-icon.png',
        sizes: '180x180',
        type: 'image/png',
        purpose: 'maskable',
      },
    ],
  };
}
