// mathema WASM Benchmark Suite
//
// Build: wasm-pack build --target web --no-default-features --features wasm
// Serve: python3 -m http.server 8080 (from wasm/ directory)
// Open:  http://localhost:8080/bench/

import init, {
  init as wasmInit,
  convert,
  latex_to_mathml,
  MathemaConverter,
} from '../pkg/mathema.js';

const WARMUP = 100;
const ITERATIONS = 5000;

const SIMPLE = 'x^2 + 1';
const QUADRATIC = 'x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}';
const NAVIER_STOKES =
  '\\rho \\left( \\frac{\\partial \\mathbf{v}}{\\partial t} + \\mathbf{v} \\cdot \\nabla \\mathbf{v} \\right) = -\\nabla p + \\mu \\nabla^2 \\mathbf{v} + \\mathbf{f}';
const NESTED_FRAC = '\\frac{\\frac{a}{b}}{\\frac{c}{d}}';
const SUM = '\\sum_{i=0}^{n} \\frac{1}{i^2}';

function benchmark(name, fn, iterations = ITERATIONS) {
  // Warmup
  for (let i = 0; i < WARMUP; i++) fn();

  const times = new Float64Array(iterations);
  for (let i = 0; i < iterations; i++) {
    const start = performance.now();
    fn();
    times[i] = (performance.now() - start) * 1000; // ms -> us
  }

  times.sort();
  const mean = times.reduce((a, b) => a + b, 0) / iterations;
  const median = times[Math.floor(iterations / 2)];
  const min = times[0];
  const max = times[iterations - 1];
  const variance = times.reduce((s, t) => s + (t - mean) ** 2, 0) / iterations;
  const stddev = Math.sqrt(variance);

  return { name, iterations, mean, median, min, max, stddev };
}

function colorClass(us) {
  if (us < 2) return 'fast';
  if (us < 10) return 'ok';
  return 'slow';
}

function addRow(result) {
  const tbody = document.querySelector('#results tbody');
  const tr = document.createElement('tr');
  const cls = colorClass(result.median);

  const cells = [
    result.name,
    result.iterations,
    result.mean.toFixed(2),
    result.median.toFixed(2),
    result.min.toFixed(2),
    result.max.toFixed(2),
    result.stddev.toFixed(2),
  ];

  cells.forEach((text, i) => {
    const td = document.createElement('td');
    td.textContent = text;
    if (i >= 2 && i <= 3) td.className = cls;
    tr.appendChild(td);
  });

  tbody.appendChild(tr);
}

async function runBenchmarks() {
  const status = document.getElementById('status');
  const btn = document.getElementById('run');
  btn.disabled = true;
  document.querySelector('#results tbody').replaceChildren();

  status.textContent = 'Running benchmarks...';

  // Allow UI to update
  await new Promise((r) => setTimeout(r, 50));

  const benchmarks = [
    // Free function API
    ['convert() simple', () => convert(SIMPLE, 'latex', 'mathml')],
    ['convert() quadratic', () => convert(QUADRATIC, 'latex', 'mathml')],
    ['convert() Navier-Stokes', () => convert(NAVIER_STOKES, 'latex', 'mathml')],
    ['latex_to_mathml() simple', () => latex_to_mathml(SIMPLE)],

    // Builder API: parse + write
    ['Builder parse simple', () => {
      const c = new MathemaConverter('latex');
      const r = c.parse(SIMPLE);
      r.free();
    }],
    ['Builder parse Navier-Stokes', () => {
      const c = new MathemaConverter('latex');
      const r = c.parse(NAVIER_STOKES);
      r.free();
    }],
    ['Builder parse+write multi-format', () => {
      const c = new MathemaConverter('latex');
      const r = c.parse(QUADRATIC);
      r.toMathml();
      r.toOmml();
      r.toUnicodeMath();
      r.free();
    }],

    // AST inspection
    ['Builder toAst() simple', () => {
      const c = new MathemaConverter('latex');
      const r = c.parse(SIMPLE);
      r.toAst();
      r.free();
    }],
    ['Builder toAst() Navier-Stokes', () => {
      const c = new MathemaConverter('latex');
      const r = c.parse(NAVIER_STOKES);
      r.toAst();
      r.free();
    }],

    // Reuse pattern (parse once, write many)
    ['Reuse: parse once + 5 writes', () => {
      const c = new MathemaConverter('latex');
      const r = c.parse(NAVIER_STOKES);
      r.toLatex();
      r.toMathml();
      r.toOmml();
      r.toUnicodeMath();
      r.toMtef();
      r.free();
    }],
  ];

  for (const [name, fn] of benchmarks) {
    status.textContent = 'Running: ' + name;
    await new Promise((r) => setTimeout(r, 10));
    const result = benchmark(name, fn);
    addRow(result);
  }

  status.textContent = 'Done. ' + benchmarks.length + ' benchmarks completed.';
  btn.disabled = false;
}

async function main() {
  const status = document.getElementById('status');
  const btn = document.getElementById('run');

  try {
    await init();
    wasmInit(); // Set up panic hook
    status.textContent = 'WASM module loaded. Ready to benchmark.';
    btn.disabled = false;
    btn.addEventListener('click', runBenchmarks);
  } catch (e) {
    status.textContent = 'Failed to load WASM: ' + e.message;
    console.error(e);
  }
}

main();
