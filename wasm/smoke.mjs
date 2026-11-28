#!/usr/bin/env node

import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

async function loadPackage(directory) {
  const jsPath = resolve(directory, 'mathema.js');
  const wasmPath = resolve(directory, 'mathema_bg.wasm');
  const bindings = await import(pathToFileURL(jsPath).href);
  await bindings.default({ module_or_path: await readFile(wasmPath) });
  bindings.init();
  return bindings;
}

const wasm = await loadPackage(process.argv[2] ?? 'web/src/lib/wasm');

const freeSvg = wasm.convertSvg('x^2', 'latex');
const freeTextSvg = wasm.convertSvg('x^2', 'latex', true);
if (!freeSvg.includes('<path') || !freeTextSvg.includes('<text')) {
  throw new Error('convertSvg() did not honor its standalone/text backend modes');
}

const converter = new wasm.MathemaConverter('latex');
const parsed = converter.parse('x^2');
const ast = parsed.toAst();
const labelledSvg = parsed.toSvgWithOptions(16, false, 'Equation', 'x squared', 'eq_1_');
if (
  !labelledSvg.includes('<title>Equation</title>') ||
  !labelledSvg.includes('aria-label="x squared"') ||
  !labelledSvg.includes('id="eq_1_')
) {
  throw new Error('toSvgWithOptions() did not decorate standalone SVG');
}
if (!Array.isArray(ast.nodes) || !ast.nodes.some((node) => 'Superscript' in node)) {
  throw new Error(`toAst() did not return a superscript AST: ${JSON.stringify(ast)}`);
}

const diagnosticConverter = wasm.MathemaConverter.withOptions('latex', false, false);
const diagnosticInput = 'é+\\foo{x}\\color{red}{y}';
const diagnostic = diagnosticConverter.parse(diagnosticInput);
const structured = diagnostic.diagnosticsStructured();
const detailed = diagnostic.diagnosticsFor('unicodemath');
const legacy = JSON.parse(diagnostic.diagnostics());
const warning = structured.find((entry) => entry.kind === 'Warning');
const readWarning = detailed.find((entry) => entry.phase === 'read');
const writeRecord = detailed.find((entry) => entry.phase === 'write');
const expectedStart = Buffer.byteLength('é+', 'utf8');
const expectedEnd = expectedStart + Buffer.byteLength('\\foo', 'utf8');
if (
  !warning ||
  readWarning?.code !== 'undefined_command' ||
  !writeRecord ||
  writeRecord.code !== 'target_loss' ||
  legacy[0]?.kind !== 'Warning' ||
  warning.span?.start !== expectedStart ||
  warning.span?.end !== expectedEnd
) {
  throw new Error(
    `structured diagnostic did not preserve its Unicode byte span: ${JSON.stringify(structured)}`,
  );
}

const fallback = converter.parse('中');
const fallbackRecord = fallback
  .diagnosticsFor('svg')
  .find((entry) => entry.phase === 'write');
if (fallbackRecord?.code !== 'render_missing_glyph') {
  throw new Error(`Unexpected SVG glyph diagnostic: ${JSON.stringify(fallbackRecord)}`);
}
const mathml = new wasm.MathemaConverter('mathml');
const painted = mathml.parse('<math><mstyle mathcolor="var(--ink)"><mi>x</mi></mstyle></math>');
if (!painted.diagnosticsFor('svg').some((record) => record.code === 'render_host_paint')) {
  throw new Error('Missing host CSS diagnostic');
}
painted.free();
mathml.free();

const asyncParsed = await converter.parseAsync('x+1');
if (!asyncParsed.toMathml().includes('<math')) {
  throw new Error('parseAsync() did not return a usable ParsedMath result');
}

for (const value of [parsed, diagnostic, asyncParsed, fallback]) value.free();
diagnosticConverter.free();
converter.free();
console.log('WASM smoke passed: AST, structured diagnostics, Unicode spans, async parsing, SVG options, disposal');

if (process.argv[3]) {
  const simd = await loadPackage(process.argv[3]);
  const latex = String.raw`\frac{\alpha+1}{x}+\sum_{i=0}^{n}i^2`;
  const xmlText = `<math><mtext>${'α &amp; β &lt; γ '.repeat(24)}</mtext></math>`;
  const mtef = wasm.convert(latex, 'latex', 'mtef');
  assert.match(mtef, /^[0-9a-f]{64,}$/);
  for (const [input, from] of [[latex, 'latex'], [xmlText, 'mathml'], [mtef, 'mtef']]) {
    for (const to of ['latex', 'mathml', 'omml', 'unicodemath', 'mtef']) {
      assert.equal(simd.convert(input, from, to), wasm.convert(input, from, to), `${from} to ${to}`);
    }
    for (const textMode of [true, false]) {
      assert.equal(simd.convertSvg(input, from, textMode), wasm.convertSvg(input, from, textMode));
    }
  }

  for (const marker of ['\uFFFE', '\uFFFF']) {
    // EF BF BE/BF straddles a 16-byte lane boundary in the text storage.
    const input = `\\text{${'x'.repeat(127)}${marker}aé&b}`;
    const expected = wasm.convert(input, 'latex', 'mathml');
    assert.ok(expected.includes('\uFFFD') && !expected.includes(marker));
    assert.equal(simd.convert(input, 'latex', 'mathml'), expected);
  }
  console.log('WASM scalar and SIMD128 outputs match');
}
