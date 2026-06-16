<script lang="ts">
  import { onMount } from 'svelte';
  import {
    loadMathema,
    process,
    stringifyErr,
    INPUT_FORMATS,
    OUTPUT_FORMATS,
    type Diagnostic
  } from '$lib/mathema';

  const EXAMPLES: Record<string, string[]> = {
    latex: [
      '\\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}',
      '\\int_0^\\infty e^{-x^2}\\,dx = \\frac{\\sqrt{\\pi}}{2}',
      '\\sum_{n=1}^{\\infty} \\frac{1}{n^2} = \\frac{\\pi^2}{6}',
      '\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}'
    ],
    mathml: [
      '<math><apply><eq/><apply><int/><bvar><ci>x</ci></bvar><lowlimit><cn>0</cn></lowlimit><uplimit><cn>1</cn></uplimit><apply><power/><ci>x</ci><cn>2</cn></apply></apply><apply><divide/><cn>1</cn><cn>3</cn></apply></apply></math>',
      '<math><msup><mi>e</mi><mrow><mi>i</mi><mi>&pi;</mi></mrow></msup><mo>+</mo><mn>1</mn><mo>=</mo><mn>0</mn></math>'
    ],
    omml: [],
    unicodemath: ['x^2 + y^2 = z^2', '√(a+b)/2'],
    mtef: []
  };

  let ready = $state(false);
  let loadError = $state('');

  let format = $state('latex');
  let input = $state(EXAMPLES.latex[0]);
  let display = $state(true);
  let strict = $state(false);
  let fontSize = $state(30);

  let svg = $state('');
  let outputs = $state<Record<string, string>>({});
  let nodeCount = $state(0);
  let diagnostics = $state<Diagnostic[]>([]);
  let error = $state('');
  let copied = $state('');

  onMount(async () => {
    try {
      await loadMathema();
      ready = true;
    } catch (e) {
      loadError = stringifyErr(e);
    }
  });

  // Recompute whenever inputs change and the module is ready.
  $effect(() => {
    if (!ready) return;
    // Reference reactive deps so the effect re-runs on change.
    const args = { input, format, display, strict, fontSize };
    try {
      const r = process(args.input, args.format, {
        display: args.display,
        strict: args.strict,
        fontSize: args.fontSize
      });
      svg = r.svg;
      outputs = r.outputs;
      nodeCount = r.nodeCount;
      diagnostics = r.diagnostics;
      error = '';
    } catch (e) {
      error = stringifyErr(e);
      svg = '';
      outputs = {};
      nodeCount = 0;
      diagnostics = [];
    }
  });

  function pickFormat(f: string) {
    format = f;
    const ex = EXAMPLES[f];
    if (ex && ex.length) input = ex[0];
  }

  async function copy(key: string, text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = key;
      setTimeout(() => (copied === key ? (copied = '') : null), 1200);
    } catch {
      /* clipboard unavailable */
    }
  }

  const examples = $derived(EXAMPLES[format] ?? []);
</script>

<main>
  <header>
    <h1><span class="logo">∑</span> Mathema</h1>
    <p class="tagline">
      Math formula <strong>renderer</strong> &amp; <strong>converter</strong> — runs entirely in your
      browser via WebAssembly.
    </p>
  </header>

  {#if loadError}
    <div class="banner error">Failed to load WebAssembly module: {loadError}</div>
  {:else if !ready}
    <div class="banner">Loading WebAssembly module…</div>
  {:else}
    <section class="controls">
      <div class="field">
        <span class="label">Input format</span>
        <div class="segmented">
          {#each INPUT_FORMATS as f (f.value)}
            <button class:active={format === f.value} onclick={() => pickFormat(f.value)}>
              {f.label}
            </button>
          {/each}
        </div>
      </div>

      <div class="toggles">
        <label><input type="checkbox" bind:checked={display} /> Display mode</label>
        <label><input type="checkbox" bind:checked={strict} /> Strict parsing</label>
        <label class="slider">
          Size {fontSize}px
          <input type="range" min="14" max="64" bind:value={fontSize} />
        </label>
      </div>
    </section>

    <section class="io">
      <div class="panel input-panel">
        <div class="panel-head">
          <span>Input</span>
          {#if examples.length}
            <div class="examples">
              {#each examples as ex, i (i)}
                <button title={ex} onclick={() => (input = ex)}>e.g. {i + 1}</button>
              {/each}
            </div>
          {/if}
        </div>
        <textarea
          spellcheck="false"
          bind:value={input}
          placeholder="Enter a formula…"
        ></textarea>
        {#if error}
          <div class="banner error compact">{error}</div>
        {:else}
          <div class="meta">AST nodes: {nodeCount}</div>
        {/if}
        {#if diagnostics.length}
          <ul class="diagnostics">
            {#each diagnostics as d, i (i)}
              <li><span class="dk">{d.kind}</span> {d.message}</li>
            {/each}
          </ul>
        {/if}
      </div>

      <div class="panel render-panel">
        <div class="panel-head"><span>Rendered</span></div>
        <div class="render-area" class:empty={!svg}>
          {#if svg}
            <!-- SVG is produced by our own trusted WASM renderer. -->
            {@html svg}
          {:else}
            <span class="placeholder">No render</span>
          {/if}
        </div>
      </div>
    </section>

    <section class="outputs">
      <h2>Conversions</h2>
      <div class="output-grid">
        {#each OUTPUT_FORMATS as out (out.key)}
          <div class="panel output-panel">
            <div class="panel-head">
              <span>{out.label}</span>
              <button
                class="copy"
                class:done={copied === out.key}
                onclick={() => copy(out.key, outputs[out.key] ?? '')}
              >
                {copied === out.key ? 'Copied ✓' : 'Copy'}
              </button>
            </div>
            <pre><code>{outputs[out.key] ?? ''}</code></pre>
          </div>
        {/each}
      </div>
    </section>

    <footer>
      <p>
        Powered by the <code>mathema</code> Rust crate compiled to WebAssembly. SVG output embeds
        Latin Modern Math glyph outlines, so it renders identically everywhere.
      </p>
    </footer>
  {/if}
</main>

<style>
  main {
    max-width: 1100px;
    margin: 0 auto;
    padding: 2rem 1.25rem 4rem;
  }

  header {
    text-align: center;
    margin-bottom: 1.75rem;
  }
  h1 {
    margin: 0;
    font-size: 2.25rem;
    letter-spacing: -0.02em;
  }
  .logo {
    color: var(--accent);
    font-family: Georgia, serif;
    font-style: italic;
  }
  .tagline {
    color: var(--muted);
    margin: 0.4rem 0 0;
  }

  .banner {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    padding: 0.9rem 1.1rem;
    color: var(--muted);
    text-align: center;
  }
  .banner.error {
    border-color: var(--danger);
    color: var(--danger);
    text-align: left;
  }
  .banner.compact {
    padding: 0.5rem 0.75rem;
    font-size: 0.85rem;
  }

  .controls {
    display: flex;
    flex-wrap: wrap;
    gap: 1rem 2rem;
    align-items: flex-end;
    justify-content: space-between;
    margin-bottom: 1.25rem;
  }
  .label {
    display: block;
    font-size: 0.75rem;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    color: var(--muted);
    margin-bottom: 0.4rem;
  }
  .segmented {
    display: inline-flex;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
    overflow: hidden;
  }
  .segmented button {
    background: transparent;
    color: var(--muted);
    border: 0;
    padding: 0.5rem 0.9rem;
    cursor: pointer;
    font-size: 0.85rem;
    transition: background 0.12s, color 0.12s;
  }
  .segmented button.active {
    background: var(--accent);
    color: #04202e;
    font-weight: 600;
  }
  .segmented button:not(.active):hover {
    color: var(--text);
  }

  .toggles {
    display: flex;
    gap: 1.25rem;
    align-items: center;
    flex-wrap: wrap;
    color: var(--muted);
    font-size: 0.9rem;
  }
  .toggles label {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    cursor: pointer;
  }
  .toggles .slider {
    gap: 0.6rem;
  }

  .io {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 1.25rem;
    margin-bottom: 1.75rem;
  }
  @media (max-width: 760px) {
    .io {
      grid-template-columns: 1fr;
    }
  }

  .panel {
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 12px;
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }
  .panel-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.55rem 0.85rem;
    background: var(--panel-2);
    border-bottom: 1px solid var(--border);
    font-size: 0.78rem;
    text-transform: uppercase;
    letter-spacing: 0.07em;
    color: var(--muted);
  }

  textarea {
    border: 0;
    background: transparent;
    color: var(--text);
    font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', Menlo, monospace;
    font-size: 0.95rem;
    line-height: 1.5;
    padding: 0.85rem;
    resize: vertical;
    min-height: 150px;
    outline: none;
  }
  .meta {
    padding: 0.4rem 0.85rem 0.6rem;
    font-size: 0.78rem;
    color: var(--muted);
  }

  .examples {
    display: flex;
    gap: 0.35rem;
  }
  .examples button {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--accent);
    padding: 0.1rem 0.45rem;
    font-size: 0.72rem;
    cursor: pointer;
  }
  .examples button:hover {
    border-color: var(--accent);
  }

  .render-area {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 1.5rem 1rem;
    background: #f8fafc;
    color: #0f172a;
    min-height: 150px;
    overflow: auto;
  }
  .render-area.empty {
    background: var(--panel);
  }
  .render-area :global(svg) {
    max-width: 100%;
    height: auto;
  }
  .placeholder {
    color: var(--muted);
  }

  .diagnostics {
    margin: 0;
    padding: 0.4rem 0.85rem 0.7rem 1.6rem;
    font-size: 0.78rem;
    color: #fbbf24;
  }
  .dk {
    color: var(--muted);
    text-transform: lowercase;
  }

  .outputs h2 {
    font-size: 1.05rem;
    color: var(--muted);
    font-weight: 600;
    margin: 0 0 0.75rem;
  }
  .output-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
    gap: 1rem;
  }
  .copy {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--accent);
    border-radius: 6px;
    padding: 0.15rem 0.55rem;
    font-size: 0.72rem;
    cursor: pointer;
  }
  .copy.done {
    color: var(--ok);
    border-color: var(--ok);
  }
  pre {
    margin: 0;
    padding: 0.75rem 0.85rem;
    overflow: auto;
    max-height: 220px;
  }
  code {
    font-family: 'JetBrains Mono', ui-monospace, 'SF Mono', Menlo, monospace;
    font-size: 0.82rem;
    line-height: 1.45;
    color: #cbd5e1;
    white-space: pre-wrap;
    word-break: break-word;
  }

  footer {
    margin-top: 2.5rem;
    text-align: center;
    color: var(--muted);
    font-size: 0.82rem;
  }
  footer code {
    color: var(--accent-2);
  }
</style>
