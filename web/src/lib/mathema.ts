// Thin loader/wrapper around the wasm-pack generated bindings.
//
// `wasm-pack build --target web` emits a default export that initialises the
// `.wasm` module (`__wbg_init`) plus named exports for the bindings. We load
// the module once (lazily, browser-only) and expose a small, typed surface to
// the rest of the app.

import portableUrl from './wasm/mathema_bg.wasm?url';
import simdUrl from './wasm-simd/mathema_bg.wasm?url';

type Bindings = typeof import('./wasm/mathema.js') | typeof import('./wasm-simd/mathema.js');
let wasm: Bindings;
let readyPromise: Promise<void> | null = null;

async function initialize(): Promise<void> {
  // (module (func (drop (i8x16.splat (i32.const 0)))))
  const simd = WebAssembly.validate(Uint8Array.of(
    0, 97, 115, 109, 1, 0, 0, 0, 1, 4, 1, 96, 0, 0, 3, 2, 1, 0,
    10, 9, 1, 7, 0, 65, 0, 253, 15, 26, 11
  ));
  const bindings = simd ? await import('./wasm-simd/mathema.js') : await import('./wasm/mathema.js');
  await bindings.default({ module_or_path: simd ? simdUrl : portableUrl });
  bindings.init();
  wasm = bindings;
}

/** Initialise the WebAssembly module exactly once. Safe to call repeatedly. */
export function loadMathema(): Promise<void> {
  return readyPromise ??= initialize();
}

export interface InputFormat {
  value: string;
  label: string;
}

/** Supported input formats (value → human label). */
export const INPUT_FORMATS: readonly InputFormat[] = [
  { value: 'latex', label: 'LaTeX' },
  { value: 'mathml', label: 'MathML' },
  { value: 'omml', label: 'OMML' },
  { value: 'unicodemath', label: 'UnicodeMath' },
  { value: 'mtef', label: 'MTEF (hex)' }
];

/** Names of the `ParsedMath` methods that serialize to a string format. */
type OutputMethod = 'toLatex' | 'toMathml' | 'toOmml' | 'toUnicodeMath' | 'toMtef';

export interface OutputFormat {
  key: string;
  label: string;
  method: OutputMethod;
  lang: string;
}

/** Output formats produced by the converter, with the ParsedMath method name. */
export const OUTPUT_FORMATS: readonly OutputFormat[] = [
  { key: 'latex', label: 'LaTeX', method: 'toLatex', lang: 'tex' },
  { key: 'mathml', label: 'MathML', method: 'toMathml', lang: 'xml' },
  { key: 'omml', label: 'OMML', method: 'toOmml', lang: 'xml' },
  { key: 'unicodemath', label: 'UnicodeMath', method: 'toUnicodeMath', lang: 'text' },
  { key: 'mtef', label: 'MTEF', method: 'toMtef', lang: 'text' }
];

export interface Diagnostic {
  kind: string;
  message: string;
  span?: { start: number; end: number };
}

export interface ProcessOptions {
  display?: boolean;
  strict?: boolean;
  fontSize?: number;
}

export interface ProcessResult {
  svg: string;
  outputs: Record<string, string>;
  nodeCount: number;
  diagnostics: Diagnostic[];
}

/**
 * Parse `input` (in `format`) and produce an SVG render plus every output
 * format. Throws on failure (e.g. malformed input in strict mode).
 */
export function process(
  input: string,
  format: string,
  opts: ProcessOptions = {}
): ProcessResult {
  const { display = true, strict = false, fontSize = 28 } = opts;
  const converter = wasm.MathemaConverter.withOptions(format, strict, display);
  let parsed: ReturnType<typeof converter.parse> | undefined;
  try {
    parsed = converter.parse(input);
    const svg = parsed.toSvg(fontSize, display);
    const outputs: Record<string, string> = {};
    for (const fmt of OUTPUT_FORMATS) {
      try {
        outputs[fmt.key] = parsed[fmt.method]();
      } catch (e) {
        outputs[fmt.key] = `⚠ ${stringifyErr(e)}`;
      }
    }
    let diagnostics: Diagnostic[] = [];
    try {
      // The structured binding keeps source byte spans and avoids a JSON
      // round-trip. Keep the string method as a compatibility fallback for
      // older generated WASM packages.
      diagnostics = parsed.diagnosticsStructured() as Diagnostic[];
    } catch {
      try {
        diagnostics = JSON.parse(parsed.diagnostics()) as Diagnostic[];
      } catch {
        diagnostics = [];
      }
    }
    return { svg, outputs, nodeCount: parsed.nodeCount(), diagnostics };
  } finally {
    // Always release the WASM-side allocation.
    parsed?.free();
    converter.free();
  }
}

/** Direct one-shot conversion between two formats. */
export function convert(input: string, from: string, to: string): string {
  return wasm.convert(input, from, to);
}

/** Normalise a thrown JsValue/Error/string into a readable message. */
export function stringifyErr(e: unknown): string {
  if (e == null) return 'unknown error';
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
