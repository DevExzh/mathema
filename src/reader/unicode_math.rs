use nom::IResult;
use nom::bytes::complete::{tag, take_while1};
use nom::character::complete::satisfy;
use smallvec::SmallVec;

use crate::ast::*;
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan};
use crate::error::{
    CompactParseResult, Config, Diagnostic, DiagnosticKind, Error, ParseResult, Result, Strictness,
};
use crate::reader::Reader;

pub struct UnicodeMathReader;

const MAX_PARSE_DEPTH: u32 = 128;

impl Reader for UnicodeMathReader {
    fn parse(input: &str, config: &Config) -> Result<ParseResult> {
        Self::parse_compact(input, config).map(|r| r.to_parse_result())
    }

    fn parse_compact(input: &str, config: &Config) -> Result<CompactParseResult> {
        let input = input.trim();
        if input.is_empty() {
            return Ok(CompactParseResult::new(CompactExpr::new(
                AstArena::new(),
                NodeSpan::EMPTY,
                DisplayStyle::Inline,
            )));
        }
        let mut parser = CompactParser::new();
        match parser.parse_expr_list(input) {
            Ok(ids) => {
                let roots = parser.arena.push_children(&ids);
                Ok(CompactParseResult::new(CompactExpr::new(
                    parser.arena,
                    roots,
                    DisplayStyle::Inline,
                )))
            }
            Err(e) => {
                if config.strictness == Strictness::Lenient {
                    let mut arena = AstArena::new();
                    let sid = arena.push_str(input);
                    let id = arena.push_node(CompactNode::Text(TextStyle::Normal, sid));
                    let roots = arena.push_children(&[id]);
                    Ok(CompactParseResult::with_diagnostics(
                        CompactExpr::new(arena, roots, DisplayStyle::Inline),
                        vec![Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: format!(
                                "UnicodeMath parse error (recovered as text): {e}"
                            ),
                            span: None,
                        }],
                    ))
                } else {
                    Err(e)
                }
            }
        }
    }
}

// ── Compact parser ────────────────────────────────────────────────────

struct CompactParser {
    arena: AstArena,
    depth: u32,
}

/// Try a parse method; propagate `Failure`, ignore `Error`, try next.
macro_rules! try_alt {
    ($self:expr, $input:expr, $( $method:ident ),+ $(,)?) => {
        $(
            match $self.$method($input) {
                Ok(r) => return Ok(r),
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => {},
            }
        )+
    }
}

impl CompactParser {
    fn new() -> Self {
        Self {
            arena: AstArena::new(),
            depth: 0,
        }
    }

    fn parse_expr_list(&mut self, input: &str) -> Result<SmallVec<[NodeId; 8]>> {
        let mut ids = SmallVec::new();
        let mut remaining = input;
        loop {
            if remaining.is_empty() {
                break;
            }
            match self.parse_element(remaining) {
                Ok((rest, id)) => {
                    ids.push(id);
                    remaining = rest;
                }
                Err(nom::Err::Failure(_)) => {
                    return Err(Error::Unsupported("UnicodeMath parse error".into()));
                }
                Err(_) => break,
            }
        }
        if !remaining.trim().is_empty() {
            let sid = self.arena.push_str(remaining.trim());
            ids.push(
                self.arena
                    .push_node(CompactNode::Text(TextStyle::Normal, sid)),
            );
        }
        Ok(ids)
    }

    fn parse_element<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        if self.depth >= MAX_PARSE_DEPTH {
            return Err(nom::Err::Failure(nom::error::Error::new(
                input,
                nom::error::ErrorKind::TooLarge,
            )));
        }
        self.depth += 1;
        let result = (|| {
            let (input, base) = self.parse_atom(input)?;
            self.parse_scripts(input, base)
        })();
        self.depth = self.depth.saturating_sub(1);
        result
    }

    fn parse_scripts<'a>(
        &mut self,
        input: &'a str,
        base: NodeId,
    ) -> IResult<&'a str, NodeId> {
        // Try subscript
        let (input, sub) = if let Some(rest) = input.strip_prefix('_') {
            match self.parse_script_arg(rest) {
                Ok((rest, id)) => (rest, Some(id)),
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => (input, None),
            }
        } else {
            (input, None)
        };

        // Try superscript
        let (input, sup) = if let Some(rest) = input.strip_prefix('^') {
            match self.parse_script_arg(rest) {
                Ok((rest, id)) => (rest, Some(id)),
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => (input, None),
            }
        } else {
            (input, None)
        };

        match (sub, sup) {
            (None, None) => {
                // Check for fraction: base / denominator
                if let Some(rest) = input.strip_prefix('/') {
                    match self.parse_script_arg(rest) {
                        Ok((rest, den)) => {
                            let id = self.arena.push_node(CompactNode::Fraction {
                                kind: FractionKind::Normal,
                                num: base,
                                den,
                            });
                            Ok((rest, id))
                        }
                        Err(nom::Err::Failure(e)) => Err(nom::Err::Failure(e)),
                        Err(_) => Ok((input, base)),
                    }
                } else {
                    Ok((input, base))
                }
            }
            (Some(sub), None) => {
                let id = self.arena.push_node(CompactNode::Subscript { base, sub });
                Ok((input, id))
            }
            (None, Some(sup)) => {
                let id = self
                    .arena
                    .push_node(CompactNode::Superscript { base, sup });
                Ok((input, id))
            }
            (Some(sub), Some(sup)) => {
                let id = self
                    .arena
                    .push_node(CompactNode::SubSup { base, sub, sup });
                Ok((input, id))
            }
        }
    }

    fn parse_script_arg<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        try_alt!(self, input, parse_group, parse_single_char_node);
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Alt,
        )))
    }

    fn parse_atom<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        try_alt!(
            self,
            input,
            parse_sqrt,
            parse_cbrt,
            parse_matrix,
            parse_text_quoted,
            parse_nary_op,
            parse_group,
            parse_number,
            parse_operator_word,
            parse_symbol,
            parse_identifier,
        );
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Alt,
        )))
    }

    fn parse_group<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        try_alt!(
            self,
            input,
            parse_paren_group,
            parse_bracket_group,
            parse_brace_group,
        );
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Alt,
        )))
    }

    /// Flatten a single-element slice into its element, or wrap in Row.
    fn flatten_single(&mut self, ids: &[NodeId]) -> NodeId {
        if ids.len() == 1 {
            ids[0]
        } else {
            let span = self.arena.push_children(ids);
            self.arena.push_node(CompactNode::Row(span))
        }
    }

    fn parse_paren_group<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        if !input.starts_with('(') {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Char,
            )));
        }
        let mut remaining = &input['('.len_utf8()..];
        let mut children: SmallVec<[NodeId; 8]> = SmallVec::new();
        loop {
            if remaining.starts_with(')') {
                break;
            }
            match self.parse_element(remaining) {
                Ok((rest, id)) => {
                    children.push(id);
                    remaining = rest;
                }
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => break,
            }
        }
        let (remaining, _) = satisfy(|c| c == ')')(remaining)?;
        let content_id = self.flatten_single(&children);
        let open = self.arena.push_str("(");
        let close = self.arena.push_str(")");
        let content_span = self.arena.push_children(&[content_id]);
        let id = self.arena.push_node(CompactNode::Fenced {
            open,
            close,
            content: content_span,
        });
        Ok((remaining, id))
    }

    fn parse_bracket_group<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        if !input.starts_with('[') {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Char,
            )));
        }
        let mut remaining = &input['['.len_utf8()..];
        let mut children: SmallVec<[NodeId; 8]> = SmallVec::new();
        loop {
            if remaining.starts_with(']') {
                break;
            }
            match self.parse_element(remaining) {
                Ok((rest, id)) => {
                    children.push(id);
                    remaining = rest;
                }
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => break,
            }
        }
        let (remaining, _) = satisfy(|c| c == ']')(remaining)?;
        let content_id = self.flatten_single(&children);
        let open = self.arena.push_str("[");
        let close = self.arena.push_str("]");
        let content_span = self.arena.push_children(&[content_id]);
        let id = self.arena.push_node(CompactNode::Fenced {
            open,
            close,
            content: content_span,
        });
        Ok((remaining, id))
    }

    fn parse_brace_group<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        if !input.starts_with('{') {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Char,
            )));
        }
        let mut remaining = &input['{'.len_utf8()..];
        let mut children: SmallVec<[NodeId; 8]> = SmallVec::new();
        loop {
            if remaining.starts_with('}') {
                break;
            }
            match self.parse_element(remaining) {
                Ok((rest, id)) => {
                    children.push(id);
                    remaining = rest;
                }
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => break,
            }
        }
        let (remaining, _) = satisfy(|c| c == '}')(remaining)?;
        let content_id = self.flatten_single(&children);
        let open = self.arena.push_str("{");
        let close = self.arena.push_str("}");
        let content_span = self.arena.push_children(&[content_id]);
        let id = self.arena.push_node(CompactNode::Fenced {
            open,
            close,
            content: content_span,
        });
        Ok((remaining, id))
    }

    fn parse_sqrt<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = satisfy(|c| c == '\u{221A}')(input)?; // √
        let (input, content) = self.parse_script_arg(input)?;
        let id = self.arena.push_node(CompactNode::Sqrt(content));
        Ok((input, id))
    }

    fn parse_cbrt<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = satisfy(|c| c == '\u{221B}')(input)?; // ∛
        let (input, content) = self.parse_script_arg(input)?;
        let s = self.arena.push_str("3");
        let index = self.arena.push_node(CompactNode::Number(s));
        let id = self.arena.push_node(CompactNode::Root {
            index,
            radicand: content,
        });
        Ok((input, id))
    }

    fn parse_matrix<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = satisfy(|c| c == '\u{25A0}')(input)?; // ■
        let (input, _) = satisfy(|c| c == '(')(input)?;
        let (input, content) = self.parse_matrix_content(input)?;
        let (input, _) = satisfy(|c| c == ')')(input)?;
        Ok((input, content))
    }

    fn parse_matrix_content<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let mut rows: Vec<Vec<SmallVec<[NodeId; 4]>>> = Vec::new();
        let mut current_row: Vec<SmallVec<[NodeId; 4]>> = Vec::new();
        let mut current_cell: SmallVec<[NodeId; 4]> = SmallVec::new();

        let mut remaining = input;
        loop {
            if remaining.is_empty() || remaining.starts_with(')') {
                break;
            }
            if remaining.starts_with('@') {
                current_row.push(std::mem::take(&mut current_cell));
                rows.push(std::mem::take(&mut current_row));
                remaining = &remaining[1..];
                continue;
            }
            if remaining.starts_with('&') {
                current_row.push(std::mem::take(&mut current_cell));
                remaining = &remaining[1..];
                continue;
            }
            match self.parse_element(remaining) {
                Ok((rest, id)) => {
                    current_cell.push(id);
                    remaining = rest;
                }
                Err(_) => {
                    // Skip one char
                    remaining = &remaining[remaining
                        .char_indices()
                        .nth(1)
                        .map_or(remaining.len(), |(i, _)| i)..];
                }
            }
        }
        // Finalize
        if !current_cell.is_empty() || !current_row.is_empty() {
            current_row.push(current_cell);
            rows.push(current_row);
        }

        let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(1) as u16;

        // Column alignments
        let mut col_ids: SmallVec<[NodeId; 8]> = SmallVec::new();
        for _ in 0..num_cols {
            col_ids.push(
                self.arena
                    .push_node(CompactNode::ColAlign(ColumnAlign::Center)),
            );
        }
        let col_aligns = self.arena.push_children(&col_ids);

        // Cells stored flat in row-major order, padded
        let mut cell_ids: SmallVec<[NodeId; 16]> = SmallVec::new();
        for row in &rows {
            for cell in row {
                let span = self.arena.push_children(cell);
                cell_ids.push(self.arena.push_node(CompactNode::Row(span)));
            }
            // Pad short rows
            for _ in row.len()..(num_cols as usize) {
                cell_ids.push(self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)));
            }
        }
        let cells = self.arena.push_children(&cell_ids);

        let id = self.arena.push_node(CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
        });
        Ok((remaining, id))
    }

    fn parse_text_quoted<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        if !input.starts_with('"') {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Char,
            )));
        }
        let rest = &input['"'.len_utf8()..];
        let (rest, text) = take_while1(|c: char| c != '"')(rest)?;
        if !rest.starts_with('"') {
            return Err(nom::Err::Error(nom::error::Error::new(
                rest,
                nom::error::ErrorKind::Char,
            )));
        }
        let rest = &rest['"'.len_utf8()..];
        let sid = self.arena.push_str(text);
        let id = self
            .arena
            .push_node(CompactNode::Text(TextStyle::Normal, sid));
        Ok((rest, id))
    }

    fn parse_nary_op<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, op) = satisfy(is_nary_char)(input)?;

        // Try subscript
        let (input, sub) = if let Some(rest) = input.strip_prefix('_') {
            match self.parse_script_arg(rest) {
                Ok((rest, id)) => (rest, id),
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => (input, NodeId::NONE),
            }
        } else {
            (input, NodeId::NONE)
        };

        // Try superscript
        let (input, sup) = if let Some(rest) = input.strip_prefix('^') {
            match self.parse_script_arg(rest) {
                Ok((rest, id)) => (rest, id),
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => (input, NodeId::NONE),
            }
        } else {
            (input, NodeId::NONE)
        };

        // Skip optional whitespace before body
        let input = input.trim_start();

        let (input, body) = if input.is_empty() {
            let row_id = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
            (input, row_id)
        } else {
            match self.parse_script_arg(input) {
                Ok((rest, id)) => (rest, id),
                Err(_) => {
                    let row_id = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
                    (input, row_id)
                }
            }
        };

        let limits = if is_integral(op) {
            LimitStyle::SubSup
        } else {
            LimitStyle::AboveBelow
        };

        let id = self.arena.push_node(CompactNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        });
        Ok((input, id))
    }

    fn parse_number<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (rest, num) = take_while1(|c: char| c.is_ascii_digit() || c == '.')(input)?;
        if num.chars().all(|c| c == '.') {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Digit,
            )));
        }
        let sid = self.arena.push_str(num);
        let id = self.arena.push_node(CompactNode::Number(sid));
        Ok((rest, id))
    }

    fn parse_operator_word<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let ops = [
            "sin", "cos", "tan", "sec", "csc", "cot", "sinh", "cosh", "tanh", "coth", "arcsin",
            "arccos", "arctan", "log", "ln", "lg", "exp", "lim", "liminf", "limsup", "sup", "inf",
            "max", "min", "det", "dim", "ker", "hom", "arg", "deg", "gcd", "lcm", "sgn", "Pr",
            "Var", "Cov", "supp", "mod",
        ];
        for op in &ops {
            if let Ok((rest, _)) = tag::<&str, &str, nom::error::Error<&str>>(*op)(input)
                && rest.chars().next().is_none_or(|c| !c.is_alphanumeric())
            {
                let has_limits = matches!(
                    *op,
                    "lim" | "liminf" | "limsup" | "sup" | "inf" | "max" | "min"
                );
                let name = self.arena.push_str(op);
                let id = self.arena.push_node(CompactNode::Operator {
                    name,
                    limits: if has_limits {
                        LimitStyle::AboveBelow
                    } else {
                        LimitStyle::Default
                    },
                });
                return Ok((rest, id));
            }
        }
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )))
    }

    fn parse_symbol<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (rest, ch) = satisfy(is_math_symbol)(input)?;
        let kind = classify_unicode_symbol(ch);
        // Skip whitespace after binary/relation operators
        let rest = if matches!(kind, SymbolKind::BinaryOp | SymbolKind::Relation) {
            rest.trim_start()
        } else {
            rest
        };
        let sid = self.arena.push_str(&ch.to_string());
        let id = self.arena.push_node(CompactNode::Symbol(kind, sid));
        Ok((rest, id))
    }

    fn parse_identifier<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (rest, ch) =
            satisfy(|c| c.is_alphabetic() && !is_math_symbol(c) && !is_nary_char(c) && c != '"')(
                input,
            )?;
        let sid = self.arena.push_str(&ch.to_string());
        let id = self.arena.push_node(CompactNode::Identifier(sid));
        Ok((rest, id))
    }

    fn parse_single_char_node<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        try_alt!(self, input, parse_number, parse_symbol, parse_identifier);
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Alt,
        )))
    }
}

// ── Pure helper functions (no arena needed) ───────────────────────────

fn is_nary_char(c: char) -> bool {
    matches!(
        c,
        '\u{2211}' // sum
        | '\u{220F}' // product
        | '\u{2210}' // coproduct
        | '\u{222B}' // integral
        | '\u{222C}' // double integral
        | '\u{222D}' // triple integral
        | '\u{222E}' // contour integral
        | '\u{22C0}' // big wedge
        | '\u{22C1}' // big vee
        | '\u{22C2}' // big intersection
        | '\u{22C3}' // big union
        | '\u{2A00}' // big circled dot
        | '\u{2A01}' // big circled plus
        | '\u{2A02}' // big circled times
    )
}

fn is_integral(c: char) -> bool {
    matches!(c, '\u{222B}' | '\u{222C}' | '\u{222D}' | '\u{222E}')
}

fn is_math_symbol(c: char) -> bool {
    matches!(c,
        '+' | '-' | '=' | '<' | '>' | ',' | ';' | '!' | '?' | ':' |
        '\u{00B1}' | '\u{00D7}' | '\u{00F7}' | // ±, ×, ÷
        '\u{2200}'..='\u{22FF}' | // Mathematical Operators
        '\u{2190}'..='\u{21FF}' | // Arrows
        '\u{27C0}'..='\u{27EF}' | // Misc Math Symbols-A
        '\u{2980}'..='\u{29FF}' | // Misc Math Symbols-B
        '\u{2A00}'..='\u{2AFF}'   // Supplemental Math Operators
    ) && !is_nary_char(c)
}

fn classify_unicode_symbol(ch: char) -> SymbolKind {
    match ch {
        '+' | '-' | '\u{00B1}' | '\u{2213}' | '\u{00D7}' | '\u{00F7}' | '\u{22C5}' | '\u{2227}'
        | '\u{2228}' | '\u{2295}' | '\u{2297}' | '\u{2229}' | '\u{222A}' => SymbolKind::BinaryOp,
        '=' | '<' | '>' | '\u{2260}' | '\u{2264}' | '\u{2265}' | '\u{226A}' | '\u{226B}'
        | '\u{2248}' | '\u{2261}' | '\u{2282}' | '\u{2283}' | '\u{2286}' | '\u{2287}'
        | '\u{2208}' | '\u{2209}' | '\u{21D2}' | '\u{21D4}' | '\u{2192}' | '\u{2190}' => {
            SymbolKind::Relation
        },
        '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\u{2016}' | '\u{27E8}' | '\u{27E9}'
        | '\u{2308}' | '\u{2309}' | '\u{230A}' | '\u{230B}' => SymbolKind::Fence,
        _ => SymbolKind::Ordinary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Vec<MathNode> {
        let config = Config::default();
        UnicodeMathReader::parse(input, &config).unwrap().expr.nodes
    }

    #[test]
    fn number() {
        let nodes = parse("42");
        assert!(matches!(&nodes[0], MathNode::Number(s) if s == "42"));
    }

    #[test]
    fn identifier() {
        let nodes = parse("x");
        assert!(matches!(&nodes[0], MathNode::Identifier(s) if s == "x"));
    }

    #[test]
    fn simple_addition() {
        let nodes = parse("a+b");
        assert_eq!(nodes.len(), 3);
        assert!(matches!(
            &nodes[1],
            MathNode::Symbol(SymbolKind::BinaryOp, _)
        ));
    }

    #[test]
    fn superscript() {
        let nodes = parse("x^2");
        assert!(matches!(&nodes[0], MathNode::Superscript { .. }));
    }

    #[test]
    fn subscript() {
        let nodes = parse("x_i");
        assert!(matches!(&nodes[0], MathNode::Subscript { .. }));
    }

    #[test]
    fn fraction() {
        let nodes = parse("a/b");
        assert!(matches!(&nodes[0], MathNode::Fraction { .. }));
    }

    #[test]
    fn sqrt() {
        let nodes = parse("\u{221A}x");
        assert!(matches!(&nodes[0], MathNode::Sqrt(_)));
    }

    #[test]
    fn parentheses() {
        let nodes = parse("(x)");
        assert!(matches!(&nodes[0], MathNode::Fenced { .. }));
    }

    #[test]
    fn sum_with_limits() {
        let nodes = parse("\u{2211}_(i=0)^n x");
        assert!(matches!(&nodes[0], MathNode::Nary { .. }));
    }

    #[test]
    fn matrix() {
        let nodes = parse("\u{25A0}(a&b@c&d)");
        assert!(matches!(&nodes[0], MathNode::Array { .. }));
        if let MathNode::Array { rows, .. } = &nodes[0] {
            assert_eq!(rows.len(), 2);
        }
    }

    #[test]
    fn quoted_text() {
        let nodes = parse("\"hello\"");
        assert!(matches!(&nodes[0], MathNode::Text(_, s) if s == "hello"));
    }

    #[test]
    fn operator() {
        let nodes = parse("sin x");
        assert!(matches!(&nodes[0], MathNode::Operator(_)));
    }

    #[test]
    fn empty_input() {
        let config = Config::default();
        let result = UnicodeMathReader::parse("", &config).unwrap();
        assert!(result.expr.nodes.is_empty());
    }
}
