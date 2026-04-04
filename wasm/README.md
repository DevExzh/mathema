# mathema WASM

WebAssembly bindings for the mathema math format converter.

## Build

```bash
# Install wasm-pack (one-time)
cargo install wasm-pack

# Build for browser (ES module)
wasm-pack build --target web --no-default-features --features wasm

# Build for Node.js
wasm-pack build --target nodejs --no-default-features --features wasm

# Build for bundler (webpack/vite/etc)
wasm-pack build --target bundler --no-default-features --features wasm
```

Output goes to `pkg/`.

Note: `wasm-opt` is disabled in `Cargo.toml` to avoid requiring the binary at
build time. To produce an optimized (smaller) `.wasm` file, remove the
`[package.metadata.wasm-pack.profile.release]` section and ensure `wasm-opt`
is installed.

## Usage

```javascript
import wasmInit, { init, convert, MathemaConverter } from './pkg/mathema.js';

// Initialize WASM module
await wasmInit();
init(); // Set up panic hook for better error messages

// Simple conversion
const mathml = convert('x^2', 'latex', 'mathml');

// Builder pattern (parse once, write many)
const conv = new MathemaConverter('latex');
const result = conv.parse('\\frac{1}{2}');
console.log(result.toMathml());
console.log(result.toOmml());
console.log(result.toUnicodeMath());

// AST inspection (zero-copy via serde-wasm-bindgen)
const ast = result.toAst();
console.log(JSON.stringify(ast, null, 2));

// Async parsing (for large formulas)
const asyncResult = await conv.parseAsync(largeFormula);

// Clean up WASM memory
result.free();
conv.free();
```

## Demo

```bash
# From the repo root, after building:
wasm-pack build --target web --no-default-features --features wasm
cd wasm
python3 -m http.server 8080
# Open http://localhost:8080/example/ for the live demo
# Open http://localhost:8080/bench/ for benchmarks
```

## TypeScript

wasm-pack generates `pkg/mathema.d.ts` with types for all exported functions
and classes. For AST object shapes (returned by `toAst()`), see `mathema.d.ts`
in this directory.
