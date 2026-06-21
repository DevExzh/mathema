import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

const basePath = process.env.BASE_PATH || '';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    // Fully static build — the app is a pure client-side WASM renderer,
    // so there is no server runtime to deploy.
    adapter: adapter({ fallback: 'index.html' }),
    paths: {
      base: basePath
    },
    prerender: {
      entries: ['*']
    }
  }
};

export default config;
