#!/usr/bin/env node

import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const generatedPath = process.argv[2];
if (!generatedPath) {
  console.error('usage: node wasm/check-generated-types.mjs <generated-mathema.d.ts>');
  process.exit(2);
}

const maintainedPath = new URL('./mathema.d.ts', import.meta.url);
const [maintained, generated] = await Promise.all([
  readFile(maintainedPath, 'utf8'),
  readFile(resolve(generatedPath), 'utf8'),
]);

const normalizeTypeScript = (source) =>
  source
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/\/\/[^\n]*/g, '')
    .replace(/\s+/g, '');

if (!normalizeTypeScript(generated).includes(normalizeTypeScript(maintained))) {
  throw new Error(
    `generated declaration does not contain the maintained AST definitions from ${maintainedPath.pathname}`,
  );
}

for (const declaration of [
  'export interface MathExpr',
  'export type MathNode',
  'export interface Diagnostic',
  'export interface Span',
]) {
  if (!generated.includes(declaration)) {
    throw new Error(`generated declaration is missing maintained declaration: ${declaration}`);
  }
}

const variants = [...maintained.matchAll(/^\s*\|\s*(?:\{\s*([A-Za-z]+)|"([A-Za-z]+)")/gm)].map(
  ([, objectVariant, unitVariant]) => objectVariant ?? unitVariant,
);
for (const variant of variants) {
  if (!generated.includes(variant)) {
    throw new Error(`generated declaration is missing maintained MathNode variant: ${variant}`);
  }
}

for (const signature of [
  'toAst(): MathExpr;',
  'diagnosticsStructured(): Diagnostic[];',
  'parseAsync(input: string): Promise<ParsedMath>;',
]) {
  if (!generated.includes(signature)) {
    throw new Error(`generated declaration is missing required signature: ${signature}`);
  }
}

if (generated.includes('toAst(): any;') || generated.includes('parseAsync(input: string): Promise<any>;')) {
  throw new Error('generated declaration regressed to an untyped AST or async result');
}

console.log(`checked generated declarations: ${resolve(generatedPath)}`);
