import type { MetadataRoute } from 'next';
import { source } from '@/lib/source';
import { docsRoute, siteUrl } from '@/lib/shared';

export default function sitemap(): MetadataRoute.Sitemap {
  const abs = (path: string) => new URL(path, siteUrl).toString();

  const staticRoutes: MetadataRoute.Sitemap = [
    { url: abs('/'), changeFrequency: 'weekly', priority: 1 },
    { url: abs(docsRoute), changeFrequency: 'weekly', priority: 0.8 },
  ];

  const seen = new Set(staticRoutes.map((r) => r.url));
  const docRoutes: MetadataRoute.Sitemap = source
    .getPages()
    .map((page) => abs(page.url))
    .filter((url) => !seen.has(url))
    .map((url) => ({ url, changeFrequency: 'weekly', priority: 0.6 }));

  return [...staticRoutes, ...docRoutes];
}
