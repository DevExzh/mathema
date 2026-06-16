// Vite "?url" asset imports (used to locate the .wasm binary).
declare module '*?url' {
  const url: string;
  export default url;
}

// SvelteKit app namespace (kept minimal; extend as needed).
declare global {
  namespace App {}
}

export {};
