import { execFileSync } from 'node:child_process';

// wasm32 instruction support is decided when the module is loaded. Build a
// separate wasm-bindgen package for each module so each has its matching glue.
for (const [name, features] of [['wasm', '-relaxed-simd,-simd128'], ['wasm-simd', '+simd128']]) {
  const env = { ...process.env };
  const flag = `-Ctarget-feature=${features}`;
  if (env.CARGO_ENCODED_RUSTFLAGS !== undefined) {
    env.CARGO_ENCODED_RUSTFLAGS = [env.CARGO_ENCODED_RUSTFLAGS, flag].filter(Boolean).join('\x1f');
  } else {
    env.RUSTFLAGS = [env.RUSTFLAGS, flag].filter(Boolean).join(' ');
  }
  execFileSync('wasm-pack', [
    'build', '..', '--target', 'web', '--out-dir', `web/src/lib/${name}`, '--release',
    '--', '--no-default-features', '--features', 'wasm,render',
  ], { cwd: new URL('.', import.meta.url), env, stdio: 'inherit' });
}
