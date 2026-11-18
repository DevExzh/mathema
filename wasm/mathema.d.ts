/**
 * mathema WASM TypeScript Definitions
 *
 * Supplementary types for the AST structure returned by ParsedMath.toAst().
 * The wasm-pack generated .d.ts covers MathemaConverter and ParsedMath methods;
 * this file types the AST object shape produced by serde-wasm-bindgen.
 */

/** Top-level expression returned by toAst() */
export interface MathExpr {
  nodes: MathNode[];
  display: "Block" | "Inline";
}

/**
 * Discriminated union of the externally tagged `MathNode` enum.
 *
 * Tuple variants are represented by arrays and boxed values are transparent
 * under serde. The `serde-wasm-bindgen` serializer omits `None` object
 * properties, so optional fields below are absent when their Rust value is
 * `None`.
 */
export type MathNode =
  | { Number: string }
  | { Identifier: string }
  | { Operator: OperatorAttr }
  | { Symbol: [SymbolKind, string] }
  | { Text: [TextStyle, string] }
  | { Space: Rational }
  | { ExplicitSpace: { width: Rational; basis: SpaceBasis } }
  | { Row: MathNode[] }
  | { Fenced: { open: string; close: string; content: FencedItem[] } }
  | { Subscript: { base: MathNode; sub: MathNode } }
  | { Superscript: { base: MathNode; sup: MathNode } }
  | { SubSup: { base: MathNode; sub: MathNode; sup: MathNode } }
  | { PreScripts: { base: MathNode; pre_sub?: MathNode; pre_sup?: MathNode } }
  | { Overscript: { base: MathNode; over: MathNode; movable_limits: boolean } }
  | { Underscript: { base: MathNode; under: MathNode; movable_limits: boolean } }
  | { UnderOver: { base: MathNode; under: MathNode; over: MathNode; movable_limits: boolean } }
  | { Fraction: { kind: FractionKind; num: MathNode; den: MathNode } }
  | { Sqrt: MathNode }
  | { Root: { index: MathNode; radicand: MathNode } }
  | { Nary: { op: string; sub: MathNode | null; sup: MathNode | null; body: MathNode; limits: LimitStyle } }
  | { Accent: { base: MathNode; accent: string; position: AccentPosition; wide: boolean } }
  | {
      Array: {
        columns: ColumnAlign[];
        rows: MathNode[][][];
        arraystretch?: number;
        add_jot: boolean;
      };
    }
  | { Phantom: [MathNode, PhantomKind] }
  | { Reflect: { content: MathNode; text_mode: boolean } }
  | { Rule: { width: Rational; height: Rational; lift: Rational } }
  | {
      Boxed: {
        content: MathNode;
        bg_color?: string;
        border_color?: string;
      };
    }
  | { Actuarial: { content: MathNode; text_mode: boolean } }
  | { Cancel: { stroke: StrokeKind; content: MathNode } }
  | { Scaled: { factor: Rational; content: MathNode } }
  | { RaiseBox: { shift: Rational; height?: Rational; depth?: Rational; content: MathNode } }
  | { Styled: [TextStyle, MathNode[]] }
  | { Color: { color: string; content: MathNode } }
  | {
      MathChoice: {
        display: MathNode;
        text: MathNode;
        script: MathNode;
        script_script: MathNode;
      };
    }
  | { StyleSwitch: number }
  | { MathAtom: { atom_type: MathAtomType; content: MathNode } };

export type FencedItem =
  | { Element: MathNode }
  | { Middle: string };

export interface OperatorAttr {
  name: string;
  limit_style: LimitStyle;
}

export interface Rational {
  num: number;
  den: number;
}

export type SymbolKind =
  | "Ordinary" | "LargeOp" | "BinaryOp" | "Relation"
  | "Open" | "Close" | "Punctuation" | "Fence" | "Alphabetic";

export type TextStyle =
  | "Normal" | "Bold" | "Italic" | "BoldItalic" | "Monospace"
  | "SansSerif" | "SansSerifBold" | "SansSerifItalic" | "SansSerifBoldItalic"
  | "DoubleStruck" | "Script" | "BoldScript" | "Fraktur" | "BoldFraktur";

export type FractionKind = "Normal" | "Display" | "Inline" | "NoBar" | "Skewed";
export type ColumnAlign = "Left" | "Center" | "Right";
export type AccentPosition = "Above" | "Below";
export type StrokeKind = "Forward" | "Backward" | "Cross" | "Horizontal" | "Vertical";
export type PhantomKind = "Full" | "Vertical" | "Horizontal" | "SmashBoth" | "SmashBottom" | "SmashTop";
export type MathAtomType = "Ord" | "Op" | "Bin" | "Rel" | "Open" | "Close" | "Punct" | "Inner";
export type LimitStyle = "Default" | "AboveBelow" | "SubSup";
export type SpaceBasis = "TextEm" | "MathMu" | "Physical";

/** Diagnostic message from lenient parsing */
export interface Diagnostic {
  kind: string;
  message: string;
}
