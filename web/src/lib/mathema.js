// @ts-check
// Thin loader/wrapper around the wasm-pack generated bindings.
//
// `wasm-pack build --target web` emits a default export that initialises the
// `.wasm` module (`__wbg_init`) plus named exports for the bindings. We load
// the module once (lazily, browser-only) and expose a small, typed surface to
// the rest of the app.

import initWasm, {
  init as installPanicHook,
  MathemaConverter,
  convert as wasmConvert
} from './wasm/mathema.js';
// Vite resolves this to a served URL for the `.wasm` binary.
import wasmUrl from './wasm/mathema_bg.wasm?url';

/** @type {Promise<void> | null} */
let readyPromise = null;

/**
 * Initialise the WebAssembly module exactly once. Safe to call repeatedly.
 * @returns {Promise<void>}
 */
export function loadMathema() {
  if (!readyPromise) {
    readyPromise = initWasm({ module_or_path: wasmUrl }).then(() => {
      installPanicHook();
    });
  }
  return /** @type {Promise<void>} */ (readyPromise);
}
/** Supported input formats (value → human label). */
export const INPUT_FORMATS = [
  { value: 'latex', label: 'LaTeX' },
  { value: 'mathml', label: 'MathML' },
  { value: 'omml', label: 'OMML' },
  { value: 'unicodemath', label: 'UnicodeMath' },
  { value: 'mtef', label: 'MTEF (hex)' }
];

/** Output formats produced by the converter, with the ParsedMath method name. */
export const OUTPUT_FORMATS = [
  { key: 'latex', label: 'LaTeX', method: 'toLatex', lang: 'tex' },
  { key: 'mathml', label: 'MathML', method: 'toMathml', lang: 'xml' },
  { key: 'omml', label: 'OMML', method: 'toOmml', lang: 'xml' },
  { key: 'unicodemath', label: 'UnicodeMath', method: 'toUnicodeMath', lang: 'text' },
  { key: 'mtef', label: 'MTEF', method: 'toMtef', lang: 'text' }
];

/**
 * Parse `input` (in `format`) and produce an SVG render plus every output
 * format. Throws a string error on failure (e.g. malformed input in strict
 * mode).
 *
 * @param {string} input
 * @param {string} format
 * @param {{ display?: boolean, strict?: boolean, fontSize?: number }} [opts]
 * @returns {{ svg: string, outputs: Record<string, string>, nodeCount: number, diagnostics: Array<{kind:string,message:string}> }}
 */
export function process(input, format, opts = {}) {
  const { display = true, strict = false, fontSize = 28 } = opts;
  const converter = MathemaConverter.withOptions(format, strict, display);
  let parsed;
  try {
    parsed = converter.parse(input);
    const svg = parsed.toSvg(fontSize, display);
    /** @type {Record<string, string>} */
    const outputs = {};
    for (const fmt of OUTPUT_FORMATS) {
      try {
        outputs[fmt.key] = /** @type {any} */ (parsed)[fmt.method]();
      } catch (e) {
        outputs[fmt.key] = `⚠ ${stringifyErr(e)}`;
      }
    }
    let diagnostics = [];
    try {
      diagnostics = JSON.parse(parsed.diagnostics());
    } catch {
      diagnostics = [];
    }
    return { svg, outputs, nodeCount: parsed.nodeCount(), diagnostics };
  } finally {
    // Always release the WASM-side allocation.
    parsed?.free?.();
    converter.free?.();
  }
}

/**
 * Direct one-shot conversion between two formats.
 * @param {string} input
 * @param {string} from
 * @param {string} to
 * @returns {string}
 */
export function convert(input, from, to) {
  return wasmConvert(input, from, to);
}

/**
 * Normalise a thrown JsValue/Error/string into a readable message.
 * @param {unknown} e
 * @returns {string}
 */
export function stringifyErr(e) {
  if (e == null) return 'unknown error';
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  return String(e);
}
