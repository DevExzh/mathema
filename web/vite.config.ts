import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

export default defineConfig({
  plugins: [sveltekit()],
  // The wasm-pack `.wasm` artifact lives under src/lib; exclude it from
  // dependency optimization so Vite serves it as a static asset.
  optimizeDeps: {
    exclude: ['$lib/wasm/mathema.js']
  },
  server: {
    fs: {
      // Allow serving the generated wasm package.
      allow: ['..']
    }
  }
});
