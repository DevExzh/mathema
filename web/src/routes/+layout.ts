// Static single-page app: prerender the shell, run everything client-side
// (the renderer/converter is WebAssembly, which is browser-only).
export const prerender = true;
export const ssr = false;
