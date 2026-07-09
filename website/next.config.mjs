import { createMDX } from 'fumadocs-mdx/next';

const withMDX = createMDX();

/** @type {import('next').NextConfig} */
const config = {
  reactStrictMode: true,
  // The Ogma repo root has its own package-lock.json (Tauri frontend); pin the
  // workspace root so Next.js doesn't resolve against it.
  turbopack: {
    root: import.meta.dirname,
  },
};

export default withMDX(config);
