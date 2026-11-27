#!/usr/bin/env node

import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

const packageDirectory = resolve(process.argv[2] ?? 'web/src/lib/wasm');
const jsPath = resolve(packageDirectory, 'mathema.js');
const wasmPath = resolve(packageDirectory, 'mathema_bg.wasm');
const wasm = await import(pathToFileURL(jsPath).href);
await wasm.default({ module_or_path: await readFile(wasmPath) });
wasm.init();

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
const diagnosticInput = 'é+\\foo{x}';
const diagnostic = diagnosticConverter.parse(diagnosticInput);
const structured = diagnostic.diagnosticsStructured();
const legacy = JSON.parse(diagnostic.diagnostics());
const warning = structured.find((entry) => entry.kind === 'Warning');
const expectedStart = Buffer.byteLength('é+', 'utf8');
const expectedEnd = expectedStart + Buffer.byteLength('\\foo', 'utf8');
if (
  !warning ||
  legacy[0]?.kind !== 'Warning' ||
  warning.span?.start !== expectedStart ||
  warning.span?.end !== expectedEnd
) {
  throw new Error(
    `structured diagnostic did not preserve its Unicode byte span: ${JSON.stringify(structured)}`,
  );
}

const asyncParsed = await converter.parseAsync('x+1');
if (!asyncParsed.toMathml().includes('<math')) {
  throw new Error('parseAsync() did not return a usable ParsedMath result');
}

for (const value of [parsed, diagnostic, asyncParsed]) value.free();
diagnosticConverter.free();
converter.free();
console.log('WASM smoke passed: AST, structured diagnostics, Unicode spans, async parsing, SVG options, disposal');
