<script>
  let { children } = $props();
</script>

<svelte:head>
  <title>Mathema — Formula Renderer & Converter</title>
</svelte:head>

{@render children()}

<style>
  :global(:root) {
    --bg: #0f172a;
    --panel: #1e293b;
    --panel-2: #273449;
    --border: #334155;
    --text: #e2e8f0;
    --muted: #94a3b8;
    --accent: #38bdf8;
    --accent-2: #818cf8;
    --danger: #f87171;
    --ok: #4ade80;
    color-scheme: dark;
  }

  :global(*) {
    box-sizing: border-box;
  }

  :global(html, body) {
    margin: 0;
    padding: 0;
    background: var(--bg);
    color: var(--text);
    font-family: system-ui, -apple-system, 'Segoe UI', Roboto, sans-serif;
    -webkit-font-smoothing: antialiased;
  }

  :global(button) {
    font-family: inherit;
  }
</style>
