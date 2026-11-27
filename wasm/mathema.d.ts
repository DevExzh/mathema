/**
 * mathema WASM TypeScript Definitions
 *
 * Shared declarations for the AST and structured diagnostics returned by the
 * WASM bindings. The Rust binding embeds this file into the wasm-bindgen
 * generated declaration so the runtime API and these shapes stay together.
 */

/** Top-level expression returned by toAst() */
export interface MathExpr {
  nodes: MathNode[];
  display: "Block" | "Inline";
  meta?: ExprMeta;
}

/**
 * Discriminated union of the externally tagged `MathNode` enum.
 *
 * Tuple variants are represented by arrays and boxed values are transparent
 * under serde. Rust option fields are present on the returned object with
 * the value `undefined` when they are empty.
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
  | { PreScripts: { base: MathNode; pre_sub: MathNode | undefined; pre_sup: MathNode | undefined } }
  | { Overscript: { base: MathNode; over: MathNode; movable_limits: boolean } }
  | { Underscript: { base: MathNode; under: MathNode; movable_limits: boolean } }
  | { UnderOver: { base: MathNode; under: MathNode; over: MathNode; movable_limits: boolean } }
  | { Fraction: { kind: FractionKind; num: MathNode; den: MathNode } }
  | { Sqrt: MathNode }
  | { Root: { index: MathNode; radicand: MathNode } }
  | { Nary: { op: string; sub: MathNode | undefined; sup: MathNode | undefined; body: MathNode; limits: LimitStyle } }
  | { Accent: { base: MathNode; accent: string; position: AccentPosition; wide: boolean } }
  | {
      Array: {
        columns: ColumnAlign[];
        rows: MathNode[][][];
        arraystretch: number | undefined;
        add_jot: boolean;
      };
    }
  | {
      Matrix: {
        kind: MatrixKind;
        alignment: ColumnAlign;
        rows: MathNode[][][];
      };
    }
  | { AlignedCell: { alignment: ColumnAlign; content: MathNode } }
  | {
      SpannedCell: {
        alignment: ColumnAlign;
        span: number;
        left_boundaries: SpanBoundary[];
        right_boundaries: SpanBoundary[];
        content: MathNode;
      };
    }
  | {
      NativeArray: {
        columns: ColumnAlign[];
        rows: MathNode[][][];
        arraystretch: number | undefined;
        add_jot: boolean;
        array_style: boolean;
        col_seps: number;
        row_seps: number;
        dashed_col_seps: number;
        dashed_row_seps: number;
      };
    }
  | { Phantom: [MathNode, PhantomKind] }
  | { Reflect: { content: MathNode; text_mode: boolean } }
  | { Rule: { width: Rational; height: Rational; lift: Rational } }
  | {
      Boxed: {
        content: MathNode;
        bg_color: string | undefined;
        border_color: string | undefined;
      };
    }
  | { Actuarial: { content: MathNode; text_mode: boolean } }
  | { Cancel: { stroke: StrokeKind; content: MathNode } }
  | { Scaled: { factor: Rational; content: MathNode } }
  | { RaiseBox: { shift: Rational; height: Rational | undefined; depth: Rational | undefined; content: MathNode } }
  | { Styled: [TextStyle, MathNode[]] }
  | {
      MathChoice: {
        display: MathNode;
        text: MathNode;
        script: MathNode;
        script_script: MathNode;
      };
    }
  | { StyleSwitch: number }
  | { MathAtom: { atom_type: MathAtomType; content: MathNode } }
  | { StructuredOperator: { content: MathNode; form: OperatorNameForm } }
  | { Color: { color: string; content: MathNode } }
  | { UndefinedCommand: { name: string; arguments: MathNode[][] } }
  | { SizeSwitch: number }
  | "LineBreak"
  | { Attributed: { attrs: NodeAttrs; content: MathNode } };

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

export type MatrixKind = "Matrix" | "SmallMatrix";

export type TextStyle =
  | "Normal" | "Bold" | "Italic" | "BoldItalic" | "Monospace"
  | "SansSerif" | "SansSerifBold" | "SansSerifItalic" | "SansSerifBoldItalic"
  | "DoubleStruck" | "Script" | "BoldScript" | "Fraktur" | "BoldFraktur";

/** Fraction spelling. Continued is LaTeX `\cfrac`, including its display-style strut. */
export type FractionKind = "Normal" | "Display" | "Continued" | "Inline" | "NoBar" | "Skewed";
export type ColumnAlign = "Left" | "Center" | "Right";
export type SpanBoundary = "SuppressGlue" | "Rule";
export type AccentPosition = "Above" | "Below";
export type StrokeKind = "Forward" | "Backward" | "Cross" | "Horizontal" | "Vertical";
export type PhantomKind = "Full" | "Vertical" | "Horizontal" | "SmashBoth" | "SmashBottom" | "SmashTop";
export type MathAtomType = "Ord" | "Op" | "Bin" | "Rel" | "Open" | "Close" | "Punct" | "Inner";
export type LimitStyle = "Default" | "AboveBelow" | "SubSup";

export type OperatorNameForm =
  | "Unstarred"
  | { Starred: LimitStyle };
export type SpaceBasis = "TextEm" | "MathMu" | "Physical";

/** Typesetting attributes preserved by the heap AST. */
export interface NodeAttrs {
  size?: FontSize;
  glyph_size?: FontSize;
}

/** Facts about an expression as a whole. */
export interface ExprMeta {
  base_size_pt?: Rational;
}

export type FontSize =
  | { Points: Rational }
  | { Logical: { level: TypeSize; delta_pt: Rational } };

export type TypeSize =
  | "Full"
  | "Sub"
  | "Sub2"
  | "Sym"
  | "SubSym"
  | "User1"
  | "User2";

/** A source byte range associated with a diagnostic, when known. */
export interface Span {
  start: number;
  end: number;
}

/** Diagnostic message from lenient parsing. */
export interface Diagnostic {
  kind: "Warning" | "Info";
  message: string;
  span?: Span;
  /** Stable machine-readable category, e.g. `target_loss`. */
  code:
    | "reader_recovery"
    | "reader_interpretation"
    | "undefined_command"
    | "target_loss"
    | "render_missing_glyph"
    | "render_color_fallback"
    | "render_host_paint"
    | "render_unrecognized_paint";
  /** Conversion phase, either `read` or `write`. */
  phase: "read" | "write";
}
