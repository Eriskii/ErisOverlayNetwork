import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vitest/config';

// `npm run dev` proxies the API to a running daemon: EON_URL=http://127.0.0.1:PORT.
const daemon = process.env.EON_URL;

export default defineConfig({
  plugins: [sveltekit()],
  // Every asset is a file under /ui/: the page's policy allows no data: fonts or images.
  build: { assetsInlineLimit: 0 },
  server: daemon ? { proxy: { '/api': daemon } } : {},
  test: { include: ['src/**/*.test.ts'] },
});
