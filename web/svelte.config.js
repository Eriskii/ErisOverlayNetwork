import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

export default {
  preprocess: vitePreprocess(),
  kit: {
    // build.rs sends the output to Cargo's OUT_DIR, from where erisauth embeds it.
    adapter: adapter({ pages: process.env.ERISAUTH_WEB_OUT ?? 'build' }),
    // Everything but the page and its icon lives under /ui/, beside the page's /api/.
    appDir: 'ui',
    // The page's one inline script, SvelteKit's bootstrap, is allowed by its hash.
    csp: {
      mode: 'hash',
      directives: {
        'default-src': ['self'],
        'script-src': ['self'],
        'style-src': ['self', 'unsafe-inline'],
        'font-src': ['self'],
        'img-src': ['self'],
        'connect-src': ['self'],
      },
    },
  },
};
