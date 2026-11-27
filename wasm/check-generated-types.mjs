#!/usr/bin/env node

import { readFile } from 'node:fs/promises';
import { resolve } from 'node:path';

const generatedPath = process.argv[2];
if (!generatedPath) {
  console.error('usage: node wasm/check-generated-types.mjs <generated-mathema.d.ts>');
  process.exit(2);
}

const rustAstPath = new URL('../src/ast/mod.rs', import.meta.url);
const generated = await readFile(resolve(generatedPath), 'utf8');
const rustAst = await readFile(rustAstPath, 'utf8');

for (const declaration of [
  'export interface MathExpr',
  'export type MathNode',
  'export interface Diagnostic',
  'export interface Span',
]) {
  if (!generated.includes(declaration)) {
    throw new Error(`generated declaration is missing declaration: ${declaration}`);
  }
}

const stripRustComments = (source) =>
  source.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/[^\n]*$/gm, '');

const enumMatch = rustAst.match(/pub enum MathNode\s*\{([\s\S]*?)\n\}\n\n\/\/\/ TeX math class/);
if (!enumMatch) throw new Error(`could not find MathNode in ${rustAstPath.pathname}`);

const rustEnum = stripRustComments(enumMatch[1]);
const variants = [];
for (const match of rustEnum.matchAll(
  /(?:^|\n)\s*([A-Z][A-Za-z0-9_]*)\s*(\{[\s\S]*?\}|\([^;\n]*\))?\s*,/g,
)) {
  const shape = match[2] ?? '';
  const fields = shape.startsWith('{')
    ? [...shape.matchAll(/\b([a-z][A-Za-z0-9_]*)\s*:/g)].map(([, field]) => field)
    : [];
  variants.push({ name: match[1], fields });
}
if (!variants.length) throw new Error(`could not parse MathNode variants in ${rustAstPath.pathname}`);

const generatedNodeStart = generated.indexOf('export type MathNode =');
const generatedNodeEnd = generated.indexOf('export type FencedItem', generatedNodeStart);
const generatedNode = generated.slice(generatedNodeStart, generatedNodeEnd);
const generatedVariants = [...generatedNode.matchAll(
  /\|\s*(?:\{\s*([A-Z][A-Za-z0-9_]*)\s*:|"([A-Z][A-Za-z0-9_]*)")/g,
)].map(([, objectVariant, unitVariant]) => objectVariant ?? unitVariant);
const expectedNames = variants.map(({ name }) => name);
const expectedSet = new Set(expectedNames);
const generatedSet = new Set(generatedVariants);
const missingVariants = expectedNames.filter((name) => !generatedSet.has(name));
const extraVariants = generatedVariants.filter((name) => !expectedSet.has(name));
if (missingVariants.length || extraVariants.length || generatedVariants.length !== expectedNames.length) {
  throw new Error(
    `generated MathNode variants differ from Rust serde variants:\nMissing: ${missingVariants.join(', ') || '(none)'}\nExtra: ${extraVariants.join(', ') || '(none)'}`,
  );
}

for (const { name, fields } of variants) {
  const marker = new RegExp(`\\|\\s*(?:\\{\\s*${name}\\s*:|"${name}")`);
  const match = marker.exec(generatedNode);
  if (!match) throw new Error(`generated declaration is missing Rust MathNode variant: ${name}`);
  const next = generatedNode.indexOf('\n|', match.index + match[0].length);
  const snippet = generatedNode.slice(match.index, next < 0 ? generatedNode.length : next);
  for (const field of fields) {
    if (!new RegExp(`\\b${field}\\??\\s*:`).test(snippet)) {
      throw new Error(`generated ${name} variant is missing Rust field: ${field}`);
    }
  }
}

const structMatch = rustAst.match(/pub struct MathExpr\s*\{([\s\S]*?)\n\}/);
if (!structMatch) throw new Error(`could not find MathExpr in ${rustAstPath.pathname}`);
const mathExprFields = [...stripRustComments(structMatch[1]).matchAll(
  /^\s*([a-z][A-Za-z0-9_]*)\s*:/gm,
)].map(([, field]) => field);
const mathExprStart = generated.indexOf('export interface MathExpr');
const mathExprEnd = generated.indexOf('\n}', mathExprStart);
const mathExprType = generated.slice(mathExprStart, mathExprEnd);
for (const field of mathExprFields) {
  if (!new RegExp(`\\b${field}\\??\\s*:`).test(mathExprType)) {
    throw new Error(`generated MathExpr is missing Rust field: ${field}`);
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
