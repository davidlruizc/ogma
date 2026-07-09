# Ogma docs site

Public documentation for [Ogma](https://github.com/davidlruizc/ogma), built with [Fumadocs](https://fumadocs.dev) on Next.js.

## Develop

```sh
npm install
npm run dev
```

Content lives in `content/docs/` as MDX; sidebar order is `content/docs/meta.json`. Site name and GitHub links are in `lib/shared.ts`.

## Checks

```sh
npm run types:check   # fumadocs-mdx + next typegen + tsc
npm run build         # production build
```

## Deploy

Deploy the `website/` directory as a Next.js app (e.g. on Vercel, set the project's Root Directory to `website`).
