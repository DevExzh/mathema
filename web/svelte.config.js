import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    // Fully static build — the app is a pure client-side WASM renderer,
    // so there is no server runtime to deploy.
    adapter: adapter({ fallback: 'index.html' })
  }
};

export default config;
