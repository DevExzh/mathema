import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [sveltekit({
    preprocess: vitePreprocess(),
    // The app is a static client-side WASM renderer.
    adapter: adapter({ fallback: 'index.html' }),
    paths: {
      base: (process.env.BASE_PATH || '') as '' | `/${string}`
    },
    prerender: {
      entries: ['*']
    }
  })],
  // The wasm-pack `.wasm` artifact lives under src/lib; exclude it from
  // dependency optimization so Vite serves it as a static asset.
  optimizeDeps: {
    exclude: ['#lib/wasm/mathema.js']
  },
  server: {
    fs: {
      // Allow serving the generated wasm package.
      allow: ['..']
    }
  }
});
