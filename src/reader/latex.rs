use nom::IResult;
use nom::bytes::complete::{tag, take_while1};
use nom::character::complete::{char, satisfy};
use nom::combinator::{opt, recognize};
use nom::sequence::tuple;
use smallvec::SmallVec;

use crate::ast::*;
use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId, NodeSpan, StrId};
use crate::error::{
    CompactParseResult, Config, Diagnostic, DiagnosticKind, Error, ParseResult, Result, Span,
    Strictness,
};
use crate::reader::Reader;
use crate::unicode::fonts::latex_font_to_style;
use crate::unicode::symbols::{
    is_nary_command, is_operator_command, latex_to_unicode, lookup_command, operator_has_limits,
};

pub struct LatexReader;

const MAX_PARSE_DEPTH: u32 = 128;

impl Reader for LatexReader {
    fn parse(input: &str, config: &Config) -> Result<ParseResult> {
        Self::parse_compact(input, config).map(|r| r.to_parse_result())
    }

    fn parse_compact(input: &str, config: &Config) -> Result<CompactParseResult> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(CompactParseResult::new(CompactExpr::new(
                AstArena::new(),
                NodeSpan::EMPTY,
                config.display,
            )));
        }
        let mut parser = LatexCompactParser::new();
        match parser.parse_node_list(trimmed) {
            Ok(("", ids)) => {
                let roots = parser.arena.push_children(&ids);
                Ok(CompactParseResult::new(CompactExpr::new(
                    parser.arena,
                    roots,
                    config.display,
                )))
            },
            Ok((rest, ids)) => {
                if config.strictness == Strictness::Strict {
                    Err(Error::LatexParse {
                        message: format!("unexpected trailing input: {rest:?}"),
                        span: Some(Span::new(input.len() - rest.len(), input.len())),
                    })
                } else {
                    let mut diagnostics = vec![Diagnostic {
                        kind: DiagnosticKind::Warning,
                        message: format!("ignored trailing input: {rest:?}"),
                        span: Some(Span::new(input.len() - rest.len(), input.len())),
                    }];
                    let mut result_ids = ids;
                    let recovered = parser.parse_lenient_remainder(rest, &mut diagnostics);
                    result_ids.extend(recovered);
                    let roots = parser.arena.push_children(&result_ids);
                    Ok(CompactParseResult::with_diagnostics(
                        CompactExpr::new(parser.arena, roots, config.display),
                        diagnostics,
                    ))
                }
            },
            Err(e) => {
                if config.strictness == Strictness::Lenient {
                    let mut diagnostics = Vec::new();
                    let ids = parser.parse_lenient_remainder(trimmed, &mut diagnostics);
                    diagnostics.push(Diagnostic {
                        kind: DiagnosticKind::Warning,
                        message: format!("parse error (recovered): {e}"),
                        span: None,
                    });
                    let roots = parser.arena.push_children(&ids);
                    Ok(CompactParseResult::with_diagnostics(
                        CompactExpr::new(parser.arena, roots, config.display),
                        diagnostics,
                    ))
                } else {
                    Err(Error::LatexParse {
                        message: format!("parse error: {e}"),
                        span: None,
                    })
                }
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Standalone helpers (stateless — no arena access needed)
// ---------------------------------------------------------------------------

fn ws(input: &str) -> IResult<&str, ()> {
    let res: IResult<&str, &str> =
        take_while1(|c: char| c == ' ' || c == '\t' || c == '\n' || c == '\r')(input);
    match res {
        Ok((rest, _)) => Ok((rest, ())),
        Err(_) => Ok((input, ())),
    }
}

fn classify_char(c: char) -> SymbolKind {
    match c {
        '+' | '-' | '*' => SymbolKind::BinaryOp,
        '/' => SymbolKind::BinaryOp,
        '=' | '<' | '>' | '!' => SymbolKind::Relation,
        ',' | ';' => SymbolKind::Punctuation,
        '.' | ':' | '?' | '\'' | '@' => SymbolKind::Ordinary,
        '|' => SymbolKind::Fence,
        _ => SymbolKind::Ordinary,
    }
}

fn negate_char(ch: char) -> Option<char> {
    match ch {
        '=' => Some('\u{2260}'),
        '<' => Some('\u{226E}'),
        '>' => Some('\u{226F}'),
        '\u{2208}' => Some('\u{2209}'),
        '\u{2282}' => Some('\u{2284}'),
        '\u{2283}' => Some('\u{2285}'),
        '\u{2286}' => Some('\u{2288}'),
        '\u{2287}' => Some('\u{2289}'),
        '\u{2261}' => Some('\u{2262}'),
        '\u{223C}' => Some('\u{2241}'),
        '\u{2264}' => Some('\u{2270}'),
        '\u{2265}' => Some('\u{2271}'),
        '\u{227A}' => Some('\u{2280}'),
        '\u{227B}' => Some('\u{2281}'),
        '\u{2203}' => Some('\u{2204}'),
        _ => None,
    }
}

fn parse_column_spec(spec: &str) -> Vec<ColumnAlign> {
    spec.chars()
        .filter_map(|c| match c {
            'l' => Some(ColumnAlign::Left),
            'c' => Some(ColumnAlign::Center),
            'r' => Some(ColumnAlign::Right),
            _ => None,
        })
        .collect()
}

fn parse_delimiter(input: &str) -> IResult<&str, String> {
    // Try single-char delimiters
    if let Some(c) = input.chars().next() {
        let rest = &input[c.len_utf8()..];
        match c {
            '(' => return Ok((rest, "(".into())),
            ')' => return Ok((rest, ")".into())),
            '[' => return Ok((rest, "[".into())),
            ']' => return Ok((rest, "]".into())),
            '|' => return Ok((rest, "|".into())),
            '.' => return Ok((rest, "".into())),
            '\\' => {
                // \{, \}, \|, or \name
                if let Some(c2) = rest.chars().next() {
                    let rest2 = &rest[c2.len_utf8()..];
                    match c2 {
                        '{' => return Ok((rest2, "{".into())),
                        '}' => return Ok((rest2, "}".into())),
                        '|' => return Ok((rest2, "\u{2016}".into())),
                        _ if c2.is_ascii_alphabetic() => {
                            let end = rest
                                .find(|ch: char| !ch.is_ascii_alphabetic())
                                .unwrap_or(rest.len());
                            let name = &rest[..end];
                            let after = &rest[end..];
                            let s = latex_to_unicode(name)
                                .map(|ch| ch.to_string())
                                .unwrap_or_else(|| name.to_string());
                            return Ok((after, s));
                        },
                        _ => {},
                    }
                }
            },
            _ => {},
        }
    }
    Err(nom::Err::Error(nom::error::Error::new(
        input,
        nom::error::ErrorKind::Char,
    )))
}

fn parse_row_separator(input: &str) -> IResult<&str, ()> {
    let (input, _) = ws(input)?;
    let (input, _) = tag("\\\\")(input)?;
    let (input, _) = ws(input)?;
    Ok((input, ()))
}

fn parse_cell_separator(input: &str) -> IResult<&str, ()> {
    let (input, _) = ws(input)?;
    let (input, _) = char('&')(input)?;
    let (input, _) = ws(input)?;
    Ok((input, ()))
}

// ---------------------------------------------------------------------------
// Compact parser
// ---------------------------------------------------------------------------

struct LatexCompactParser {
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

impl LatexCompactParser {
    fn new() -> Self {
        Self {
            arena: AstArena::new(),
            depth: 0,
        }
    }

    /// Intern a single char as a string, avoiding heap allocation.
    fn push_char_str(&mut self, c: char) -> StrId {
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        self.arena.push_str(s)
    }

    /// If exactly one element, return it; otherwise wrap in a Row.
    fn flatten_single(&mut self, ids: &[NodeId]) -> NodeId {
        if ids.len() == 1 {
            ids[0]
        } else {
            let span = self.arena.push_children(ids);
            self.arena.push_node(CompactNode::Row(span))
        }
    }

    /// Check whether a node is a large-operator symbol (n-ary).
    fn is_nary_id(&self, id: NodeId) -> bool {
        matches!(*self.arena.node(id), CompactNode::Symbol(SymbolKind::LargeOp, _))
    }

    /// Build an Array node from column alignments and a row-major grid of cells.
    fn build_array(
        &mut self,
        columns: &[ColumnAlign],
        rows: Vec<Vec<SmallVec<[NodeId; 8]>>>,
    ) -> NodeId {
        let mut align_ids: SmallVec<[NodeId; 4]> = SmallVec::new();
        for &col in columns {
            align_ids.push(self.arena.push_node(CompactNode::ColAlign(col)));
        }
        let col_aligns = self.arena.push_children(&align_ids);
        let num_cols = columns.len().max(1) as u16;

        let mut cell_ids: SmallVec<[NodeId; 16]> = SmallVec::new();
        for row in &rows {
            for cell in row {
                let span = self.arena.push_children(cell);
                cell_ids.push(self.arena.push_node(CompactNode::Row(span)));
            }
            // Pad short rows to num_cols
            for _ in row.len()..(num_cols as usize) {
                cell_ids.push(self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)));
            }
        }
        let cells = self.arena.push_children(&cell_ids);
        self.arena
            .push_node(CompactNode::Array { col_aligns, cells, num_cols })
    }

    // -----------------------------------------------------------------------
    // Top-level combinators
    // -----------------------------------------------------------------------

    fn parse_node_list<'a>(
        &mut self,
        input: &'a str,
    ) -> IResult<&'a str, SmallVec<[NodeId; 8]>> {
        if self.depth >= MAX_PARSE_DEPTH {
            return Err(nom::Err::Failure(nom::error::Error::new(
                input,
                nom::error::ErrorKind::TooLarge,
            )));
        }
        self.depth += 1;
        let mut ids = SmallVec::new();
        let mut rest = input;
        loop {
            let (r, _) = ws(rest)?;
            rest = r;
            match self.parse_element(rest) {
                Ok((r, id)) => {
                    ids.push(id);
                    rest = r;
                },
                Err(nom::Err::Failure(e)) => {
                    self.depth = self.depth.saturating_sub(1);
                    return Err(nom::Err::Failure(e));
                },
                Err(_) => break,
            }
        }
        self.depth = self.depth.saturating_sub(1);
        Ok((rest, ids))
    }

    /// Parse one element: an atom optionally followed by scripts.
    fn parse_element<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, base) = self.parse_atom(input)?;

        // N-ary operators consume scripts + body specially
        if self.is_nary_id(base) {
            return self.parse_nary_rest(input, base);
        }

        // Check for \limits/\nolimits after Operator nodes
        if let CompactNode::Operator { name, .. } = *self.arena.node(base) {
            let (trimmed, _) = ws(input)?;
            if let Ok((rest, _)) =
                tag::<&str, &str, nom::error::Error<&str>>("\\limits")(trimmed)
                && rest.chars().next().is_none_or(|c| !c.is_ascii_alphabetic())
            {
                let new_id = self.arena.push_node(CompactNode::Operator {
                    name,
                    limits: LimitStyle::AboveBelow,
                });
                return self.parse_scripts(rest, new_id);
            }
            if let Ok((rest, _)) =
                tag::<&str, &str, nom::error::Error<&str>>("\\nolimits")(trimmed)
                && rest.chars().next().is_none_or(|c| !c.is_ascii_alphabetic())
            {
                let new_id = self.arena.push_node(CompactNode::Operator {
                    name,
                    limits: LimitStyle::SubSup,
                });
                return self.parse_scripts(rest, new_id);
            }
        }

        self.parse_scripts(input, base)
    }

    /// Attach optional sub/super scripts to a base node.
    fn parse_scripts<'a>(
        &mut self,
        input: &'a str,
        base: NodeId,
    ) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, first_script) = opt(satisfy(|c| c == '^' || c == '_'))(input)?;

        let Some(first) = first_script else {
            return Ok((input, base));
        };

        let (input, first_arg) = self.parse_script_arg(input)?;
        let (input, _) = ws(input)?;
        let (input, second_script) =
            opt(if first == '^' { char('_') } else { char('^') })(input)?;

        match (first, second_script) {
            ('^', None) => Ok((
                input,
                self.arena.push_node(CompactNode::Superscript {
                    base,
                    sup: first_arg,
                }),
            )),
            ('_', None) => Ok((
                input,
                self.arena.push_node(CompactNode::Subscript {
                    base,
                    sub: first_arg,
                }),
            )),
            ('^', Some('_')) => {
                let (input, second_arg) = self.parse_script_arg(input)?;
                Ok((
                    input,
                    self.arena.push_node(CompactNode::SubSup {
                        base,
                        sub: second_arg,
                        sup: first_arg,
                    }),
                ))
            },
            ('_', Some('^')) => {
                let (input, second_arg) = self.parse_script_arg(input)?;
                Ok((
                    input,
                    self.arena.push_node(CompactNode::SubSup {
                        base,
                        sub: first_arg,
                        sup: second_arg,
                    }),
                ))
            },
            _ => Ok((input, base)),
        }
    }

    /// Parse an n-ary operator with optional limits and a body.
    fn parse_nary_rest<'a>(
        &mut self,
        input: &'a str,
        op_id: NodeId,
    ) -> IResult<&'a str, NodeId> {
        let CompactNode::Symbol(_, sid) = *self.arena.node(op_id) else {
            return Ok((input, op_id));
        };
        let op_char = self.arena.str_val(sid).chars().next().unwrap_or('\u{2211}');

        let (input, _) = ws(input)?;

        // Check for \limits or \nolimits
        let (input, limit_style) = if let Ok((rest, _)) =
            tag::<&str, &str, nom::error::Error<&str>>("\\limits")(input)
        {
            if rest.chars().next().is_none_or(|c| !c.is_ascii_alphabetic()) {
                (rest, LimitStyle::AboveBelow)
            } else {
                (input, LimitStyle::Default)
            }
        } else if let Ok((rest, _)) =
            tag::<&str, &str, nom::error::Error<&str>>("\\nolimits")(input)
        {
            if rest.chars().next().is_none_or(|c| !c.is_ascii_alphabetic()) {
                (rest, LimitStyle::SubSup)
            } else {
                (input, LimitStyle::Default)
            }
        } else {
            (input, LimitStyle::Default)
        };

        let (input, _) = ws(input)?;
        let (input, (sub_id, sup_id)) = self.parse_optional_limits(input)?;
        let (input, _) = ws(input)?;

        // The body is the next atom (or empty row if nothing follows)
        let (input, body_id) = match self.parse_atom(input) {
            Ok((rest, id)) => (rest, id),
            Err(_) => {
                let row_id = self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY));
                (input, row_id)
            },
        };

        Ok((
            input,
            self.arena.push_node(CompactNode::Nary {
                op: op_char,
                sub: sub_id,
                sup: sup_id,
                body: body_id,
                limits: limit_style,
            }),
        ))
    }

    /// Parse optional _{...} and ^{...} in any order, returning (sub, sup).
    /// Uses `NodeId::NONE` for absent limits.
    fn parse_optional_limits<'a>(
        &mut self,
        input: &'a str,
    ) -> IResult<&'a str, (NodeId, NodeId)> {
        let (input, first) = opt(satisfy(|c| c == '_' || c == '^'))(input)?;
        let Some(first_ch) = first else {
            return Ok((input, (NodeId::NONE, NodeId::NONE)));
        };
        let (input, first_arg) = self.parse_script_arg(input)?;
        let (input, _) = ws(input)?;
        let second_ch = if first_ch == '_' { '^' } else { '_' };
        let (input, second) = opt(char(second_ch))(input)?;
        if second.is_some() {
            let (input, second_arg) = self.parse_script_arg(input)?;
            if first_ch == '_' {
                Ok((input, (first_arg, second_arg)))
            } else {
                Ok((input, (second_arg, first_arg)))
            }
        } else if first_ch == '_' {
            Ok((input, (first_arg, NodeId::NONE)))
        } else {
            Ok((input, (NodeId::NONE, first_arg)))
        }
    }

    /// Parse a single script argument: either a single token or a braced group.
    fn parse_script_arg<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        self.parse_atom(input)
    }

    // -----------------------------------------------------------------------
    // Atoms
    // -----------------------------------------------------------------------

    fn parse_atom<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        try_alt!(
            self,
            input,
            parse_braced_group,
            parse_command,
            parse_number,
            parse_symbol_char,
            parse_letter,
        );
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Alt,
        )))
    }

    /// Parse a braced group `{ ... }` into a Row (or single node).
    fn parse_braced_group<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = char('{')(input)?;
        let (input, ids) = self.parse_node_list(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('}')(input)?;
        Ok((input, self.flatten_single(&ids)))
    }

    /// Parse a number: sequence of digits with optional decimal point.
    fn parse_number<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, num) = recognize(tuple((
            take_while1(|c: char| c.is_ascii_digit()),
            opt(tuple((
                char('.'),
                take_while1(|c: char| c.is_ascii_digit()),
            ))),
        )))(input)?;
        let sid = self.arena.push_str(num);
        Ok((input, self.arena.push_node(CompactNode::Number(sid))))
    }

    /// Parse a single letter as an identifier.
    fn parse_letter<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, c) = satisfy(|c| c.is_ascii_alphabetic())(input)?;
        let sid = self.push_char_str(c);
        Ok((input, self.arena.push_node(CompactNode::Identifier(sid))))
    }

    /// Parse a symbol character: +, -, =, <, >, !, comma, semicolon, etc.
    fn parse_symbol_char<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, c) = satisfy(|c| "+-*/=<>!,;:.?'|@".contains(c))(input)?;
        let kind = classify_char(c);
        let sid = self.push_char_str(c);
        Ok((input, self.arena.push_node(CompactNode::Symbol(kind, sid))))
    }

    // -----------------------------------------------------------------------
    // Commands (backslash-prefixed)
    // -----------------------------------------------------------------------

    fn parse_command<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = char('\\')(input)?;
        // Special single-character commands
        if let Ok((input, id)) = self.parse_special_command(input) {
            return Ok((input, id));
        }
        let (input, name) = recognize(take_while1(|c: char| c.is_ascii_alphabetic()))(input)?;
        self.dispatch_command(input, name)
    }

    fn parse_special_command<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let c = input.chars().next().ok_or_else(|| {
            nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Char))
        })?;
        let rest = &input[c.len_utf8()..];
        let node = match c {
            '{' => {
                let sid = self.arena.push_str("{");
                CompactNode::Symbol(SymbolKind::Open, sid)
            },
            '}' => {
                let sid = self.arena.push_str("}");
                CompactNode::Symbol(SymbolKind::Close, sid)
            },
            '|' => {
                let sid = self.arena.push_str("\u{2016}");
                CompactNode::Symbol(SymbolKind::Fence, sid)
            },
            ',' => CompactNode::Space(Rational::new(3, 18)),
            ';' => CompactNode::Space(Rational::new(5, 18)),
            ':' => CompactNode::Space(Rational::new(4, 18)),
            '!' => CompactNode::Space(Rational::new(-3, 18)),
            '\\' => CompactNode::Space(Rational::zero()),
            ' ' => CompactNode::Space(Rational::new(1, 4)),
            _ => {
                return Err(nom::Err::Error(nom::error::Error::new(
                    input,
                    nom::error::ErrorKind::Char,
                )));
            },
        };
        Ok((rest, self.arena.push_node(node)))
    }

    fn dispatch_command<'a>(
        &mut self,
        input: &'a str,
        name: &str,
    ) -> IResult<&'a str, NodeId> {
        match name {
            // --- Fractions ---
            "frac" => self.parse_frac(input, FractionKind::Normal),
            "dfrac" => self.parse_frac(input, FractionKind::Display),
            "tfrac" => self.parse_frac(input, FractionKind::Inline),
            "binom" => self.parse_binom(input),

            // --- Roots ---
            "sqrt" => self.parse_sqrt(input),

            // --- Accents (above) ---
            "hat" | "widehat" => self.parse_accent(input, '\u{0302}', AccentPosition::Above),
            "bar" => self.parse_accent(input, '\u{0304}', AccentPosition::Above),
            "overline" => self.parse_overunder(input, true),
            "underline" => self.parse_overunder(input, false),
            "vec" => self.parse_accent(input, '\u{20D7}', AccentPosition::Above),
            "dot" => self.parse_accent(input, '\u{0307}', AccentPosition::Above),
            "ddot" => self.parse_accent(input, '\u{0308}', AccentPosition::Above),
            "tilde" | "widetilde" => self.parse_accent(input, '\u{0303}', AccentPosition::Above),
            "breve" => self.parse_accent(input, '\u{0306}', AccentPosition::Above),
            "check" => self.parse_accent(input, '\u{030C}', AccentPosition::Above),
            "acute" => self.parse_accent(input, '\u{0301}', AccentPosition::Above),
            "grave" => self.parse_accent(input, '\u{0300}', AccentPosition::Above),

            // --- Overbrace / underbrace ---
            "overbrace" => self.parse_overbrace(input),
            "underbrace" => self.parse_underbrace(input),

            // --- Delimiters ---
            "left" => self.parse_left_right(input),

            // --- Text and font ---
            "text" | "textrm" | "textit" | "textbf" | "texttt" | "textsf" => {
                self.parse_text(input, name)
            },
            "mathrm" | "mathbf" | "mathit" | "mathtt" | "mathsf" | "mathbb" | "mathcal"
            | "mathscr" | "mathfrak" | "boldsymbol" | "bm" => self.parse_math_font(input, name),

            // --- Spacing ---
            "quad" => Ok((
                input,
                self.arena.push_node(CompactNode::Space(Rational::new(1, 1))),
            )),
            "qquad" => Ok((
                input,
                self.arena.push_node(CompactNode::Space(Rational::new(2, 1))),
            )),
            "enspace" => Ok((
                input,
                self.arena.push_node(CompactNode::Space(Rational::new(1, 2))),
            )),

            // --- Decorators ---
            "phantom" => {
                let (input, arg_id) = self.parse_required_group(input)?;
                Ok((input, self.arena.push_node(CompactNode::Phantom(arg_id))))
            },
            "boxed" | "fbox" => {
                let (input, arg_id) = self.parse_required_group(input)?;
                Ok((input, self.arena.push_node(CompactNode::Boxed(arg_id))))
            },
            "cancel" => self.parse_cancel(input, StrokeKind::Forward),
            "bcancel" => self.parse_cancel(input, StrokeKind::Backward),
            "xcancel" => self.parse_cancel(input, StrokeKind::Cross),

            // --- Environments ---
            "begin" => self.parse_environment(input),

            // --- Overset / Underset ---
            "overset" | "stackrel" => self.parse_overset(input),
            "underset" => self.parse_underset(input),

            // --- Operatorname ---
            "operatorname" => self.parse_operatorname(input),

            // --- Additional binomials ---
            "dbinom" => self.parse_styled_binom(input, FractionKind::Display),
            "tbinom" => self.parse_styled_binom(input, FractionKind::Inline),

            // --- Negation ---
            "not" => self.parse_not(input),

            // --- Mod operators ---
            "bmod" => {
                let sid = self.arena.push_str("mod");
                Ok((
                    input,
                    self.arena.push_node(CompactNode::Operator {
                        name: sid,
                        limits: LimitStyle::Default,
                    }),
                ))
            },
            "pmod" | "pod" => self.parse_pmod(input),

            // --- Extensible arrows ---
            "xleftarrow" => self.parse_extensible_arrow(input, '\u{2190}'),
            "xrightarrow" => self.parse_extensible_arrow(input, '\u{2192}'),

            // --- Substack ---
            "substack" => self.parse_substack(input),

            // --- Sized delimiters ---
            "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl"
            | "biggr" | "Biggl" | "Biggr" | "bigm" | "Bigm" | "biggm" | "Biggm" => {
                self.parse_big_delim(input, name)
            },

            // --- Style commands (silently consume) ---
            "displaystyle" | "textstyle" | "scriptstyle" | "scriptscriptstyle" => Ok((
                input,
                self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
            )),

            // --- Color (consume color arg, return content) ---
            "color" => self.parse_color_discard(input),
            "textcolor" => self.parse_textcolor(input),

            // --- N-ary operators ---
            name if is_nary_command(name) => {
                let ch = latex_to_unicode(name).unwrap_or('\u{2211}');
                let sid = self.push_char_str(ch);
                Ok((
                    input,
                    self.arena
                        .push_node(CompactNode::Symbol(SymbolKind::LargeOp, sid)),
                ))
            },

            // --- Named operators (sin, cos, lim, ...) ---
            name_str if is_operator_command(name_str) => {
                let limit_style = if operator_has_limits(name_str) {
                    LimitStyle::Default
                } else {
                    LimitStyle::SubSup
                };
                let sid = self.arena.push_str(name_str);
                Ok((
                    input,
                    self.arena.push_node(CompactNode::Operator {
                        name: sid,
                        limits: limit_style,
                    }),
                ))
            },

            // --- Greek letters and other symbols ---
            _ => {
                if let Some((unicode, kind)) = lookup_command(name) {
                    let sid = self.push_char_str(unicode);
                    if kind == SymbolKind::Alphabetic {
                        Ok((input, self.arena.push_node(CompactNode::Identifier(sid))))
                    } else {
                        Ok((input, self.arena.push_node(CompactNode::Symbol(kind, sid))))
                    }
                } else {
                    // Unknown command: emit as identifier
                    let sid = self.arena.push_str(name);
                    Ok((input, self.arena.push_node(CompactNode::Identifier(sid))))
                }
            },
        }
    }

    // -----------------------------------------------------------------------
    // Command sub-parsers
    // -----------------------------------------------------------------------

    fn parse_required_group<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        self.parse_braced_group(input)
    }

    fn parse_cancel<'a>(
        &mut self,
        input: &'a str,
        stroke: StrokeKind,
    ) -> IResult<&'a str, NodeId> {
        let (input, arg_id) = self.parse_required_group(input)?;
        Ok((
            input,
            self.arena.push_node(CompactNode::Cancel {
                stroke,
                content: arg_id,
            }),
        ))
    }

    fn parse_frac<'a>(
        &mut self,
        input: &'a str,
        kind: FractionKind,
    ) -> IResult<&'a str, NodeId> {
        let (input, num_id) = self.parse_required_group(input)?;
        let (input, den_id) = self.parse_required_group(input)?;
        Ok((
            input,
            self.arena.push_node(CompactNode::Fraction {
                kind,
                num: num_id,
                den: den_id,
            }),
        ))
    }

    fn parse_binom<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, top_id) = self.parse_required_group(input)?;
        let (input, bottom_id) = self.parse_required_group(input)?;
        let frac_id = self.arena.push_node(CompactNode::Fraction {
            kind: FractionKind::NoBar,
            num: top_id,
            den: bottom_id,
        });
        let open = self.arena.push_str("(");
        let close = self.arena.push_str(")");
        let content = self.arena.push_children(&[frac_id]);
        Ok((
            input,
            self.arena
                .push_node(CompactNode::Fenced { open, close, content }),
        ))
    }

    fn parse_sqrt<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        // Optional index: \sqrt[n]{...}
        let (input, index) = if input.starts_with('[') {
            let (rest, _) = char('[')(input)?;
            let (rest, idx_ids) = self.parse_node_list(rest)?;
            let (rest, _) = char(']')(rest)?;
            (rest, Some(idx_ids))
        } else {
            (input, None)
        };
        let (input, content_id) = self.parse_required_group(input)?;
        match index {
            Some(idx_ids) => {
                let index_id = self.flatten_single(&idx_ids);
                Ok((
                    input,
                    self.arena.push_node(CompactNode::Root {
                        index: index_id,
                        radicand: content_id,
                    }),
                ))
            },
            None => Ok((input, self.arena.push_node(CompactNode::Sqrt(content_id)))),
        }
    }

    fn parse_accent<'a>(
        &mut self,
        input: &'a str,
        accent_char: char,
        position: AccentPosition,
    ) -> IResult<&'a str, NodeId> {
        let (input, base_id) = self.parse_required_group(input)?;
        Ok((
            input,
            self.arena.push_node(CompactNode::Accent {
                base: base_id,
                accent: accent_char,
                position,
            }),
        ))
    }

    fn parse_overunder<'a>(
        &mut self,
        input: &'a str,
        is_over: bool,
    ) -> IResult<&'a str, NodeId> {
        let (input, base_id) = self.parse_required_group(input)?;
        let line_sid = self.arena.push_str("\u{203E}");
        let line_id = self
            .arena
            .push_node(CompactNode::Symbol(SymbolKind::Ordinary, line_sid));
        if is_over {
            Ok((
                input,
                self.arena.push_node(CompactNode::Overscript {
                    base: base_id,
                    over: line_id,
                    movable_limits: false,
                }),
            ))
        } else {
            Ok((
                input,
                self.arena.push_node(CompactNode::Underscript {
                    base: base_id,
                    under: line_id,
                    movable_limits: false,
                }),
            ))
        }
    }

    fn parse_overbrace<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, base_id) = self.parse_required_group(input)?;
        let sid = self.arena.push_str("\u{23DE}");
        let over_id = self
            .arena
            .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid));
        Ok((
            input,
            self.arena.push_node(CompactNode::Overscript {
                base: base_id,
                over: over_id,
                movable_limits: false,
            }),
        ))
    }

    fn parse_underbrace<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, base_id) = self.parse_required_group(input)?;
        let sid = self.arena.push_str("\u{23DF}");
        let under_id = self
            .arena
            .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid));
        Ok((
            input,
            self.arena.push_node(CompactNode::Underscript {
                base: base_id,
                under: under_id,
                movable_limits: false,
            }),
        ))
    }

    /// Parse \left delim ... \right delim
    fn parse_left_right<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, open_str) = parse_delimiter(input)?;
        let (input, content_ids) = self.parse_until_right(input)?;
        let (input, close_str) = parse_delimiter(input)?;

        let open = self.arena.push_str(&open_str);
        let close = self.arena.push_str(&close_str);
        let content = self.arena.push_children(&content_ids);
        Ok((
            input,
            self.arena
                .push_node(CompactNode::Fenced { open, close, content }),
        ))
    }

    fn parse_until_right<'a>(
        &mut self,
        input: &'a str,
    ) -> IResult<&'a str, SmallVec<[NodeId; 8]>> {
        let mut ids = SmallVec::new();
        let mut rest = input;
        loop {
            let (r, _) = ws(rest)?;
            rest = r;
            // Check for \right
            if let Ok((after_right, _)) =
                tag::<&str, &str, nom::error::Error<&str>>("\\right")(rest)
            {
                return Ok((after_right, ids));
            }
            // Check for \middle
            if let Ok((after_mid, _)) =
                tag::<&str, &str, nom::error::Error<&str>>("\\middle")(rest)
            {
                let (after_mid, _) = ws(after_mid)?;
                let (after_delim, delim) = parse_delimiter(after_mid)?;
                let sid = self.arena.push_str(&delim);
                ids.push(
                    self.arena
                        .push_node(CompactNode::Symbol(SymbolKind::Fence, sid)),
                );
                rest = after_delim;
                continue;
            }
            if rest.is_empty() {
                return Err(nom::Err::Error(nom::error::Error::new(
                    rest,
                    nom::error::ErrorKind::Tag,
                )));
            }
            let (r, id) = self.parse_element(rest)?;
            ids.push(id);
            rest = r;
        }
    }

    fn parse_text<'a>(
        &mut self,
        input: &'a str,
        cmd_name: &str,
    ) -> IResult<&'a str, NodeId> {
        let style = match cmd_name {
            "textbf" => TextStyle::Bold,
            "textit" => TextStyle::Italic,
            "texttt" => TextStyle::Monospace,
            "textsf" => TextStyle::SansSerif,
            _ => TextStyle::Normal,
        };
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, text) =
            take_while1::<_, _, nom::error::Error<&str>>(|c: char| c != '}')(input)
                .unwrap_or((input, ""));
        let (input, _) = char('}')(input)?;
        let sid = self.arena.push_str(text);
        Ok((input, self.arena.push_node(CompactNode::Text(style, sid))))
    }

    fn parse_math_font<'a>(
        &mut self,
        input: &'a str,
        cmd_name: &str,
    ) -> IResult<&'a str, NodeId> {
        let style = latex_font_to_style(cmd_name).unwrap_or(TextStyle::Normal);
        let (input, content_id) = self.parse_required_group(input)?;
        // If content is a Row, use its children span; otherwise wrap in single-element span
        let span = match *self.arena.node(content_id) {
            CompactNode::Row(s) => s,
            _ => self.arena.push_children(&[content_id]),
        };
        Ok((
            input,
            self.arena.push_node(CompactNode::Styled(style, span)),
        ))
    }

    fn parse_overset<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, over_id) = self.parse_required_group(input)?;
        let (input, base_id) = self.parse_required_group(input)?;
        Ok((
            input,
            self.arena.push_node(CompactNode::Overscript {
                base: base_id,
                over: over_id,
                movable_limits: false,
            }),
        ))
    }

    fn parse_underset<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, under_id) = self.parse_required_group(input)?;
        let (input, base_id) = self.parse_required_group(input)?;
        Ok((
            input,
            self.arena.push_node(CompactNode::Underscript {
                base: base_id,
                under: under_id,
                movable_limits: false,
            }),
        ))
    }

    fn parse_operatorname<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, name) =
            take_while1::<_, _, nom::error::Error<&str>>(|c: char| c != '}')(input)
                .unwrap_or((input, ""));
        let (input, _) = char('}')(input)?;
        let sid = self.arena.push_str(name);
        Ok((
            input,
            self.arena.push_node(CompactNode::Operator {
                name: sid,
                limits: LimitStyle::Default,
            }),
        ))
    }

    fn parse_styled_binom<'a>(
        &mut self,
        input: &'a str,
        kind: FractionKind,
    ) -> IResult<&'a str, NodeId> {
        let (input, top_id) = self.parse_required_group(input)?;
        let (input, bottom_id) = self.parse_required_group(input)?;
        let frac_id = self.arena.push_node(CompactNode::Fraction {
            kind,
            num: top_id,
            den: bottom_id,
        });
        let open = self.arena.push_str("(");
        let close = self.arena.push_str(")");
        let content = self.arena.push_children(&[frac_id]);
        Ok((
            input,
            self.arena
                .push_node(CompactNode::Fenced { open, close, content }),
        ))
    }

    fn parse_not<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        // Try direct character negation: \not= \not< \not>
        if let Some(first) = input.chars().next()
            && let Some(negated) = negate_char(first)
        {
            let rest = &input[first.len_utf8()..];
            let sid = self.push_char_str(negated);
            return Ok((
                rest,
                self.arena
                    .push_node(CompactNode::Symbol(SymbolKind::Relation, sid)),
            ));
        }
        // Try command negation: \not\in, \not\subset, etc.
        if let Some(after_backslash) = input.strip_prefix('\\') {
            let cmd_end = after_backslash
                .find(|c: char| !c.is_ascii_alphabetic())
                .unwrap_or(after_backslash.len());
            if cmd_end > 0 {
                let cmd = &after_backslash[..cmd_end];
                if let Some(unicode) = latex_to_unicode(cmd)
                    && let Some(negated) = negate_char(unicode)
                {
                    let rest = &after_backslash[cmd_end..];
                    let sid = self.push_char_str(negated);
                    return Ok((
                        rest,
                        self.arena
                            .push_node(CompactNode::Symbol(SymbolKind::Relation, sid)),
                    ));
                }
            }
        }
        // Fallback: combining solidus overlay
        let sid = self.arena.push_str("\u{0338}");
        Ok((
            input,
            self.arena
                .push_node(CompactNode::Symbol(SymbolKind::Ordinary, sid)),
        ))
    }

    fn parse_pmod<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, content_id) = self.parse_required_group(input)?;
        let mod_name = self.arena.push_str("mod");
        let mod_id = self.arena.push_node(CompactNode::Operator {
            name: mod_name,
            limits: LimitStyle::Default,
        });
        let space_id = self
            .arena
            .push_node(CompactNode::Space(Rational::new(5, 18)));
        let open = self.arena.push_str("(");
        let close = self.arena.push_str(")");
        let content = self.arena.push_children(&[mod_id, space_id, content_id]);
        Ok((
            input,
            self.arena
                .push_node(CompactNode::Fenced { open, close, content }),
        ))
    }

    fn parse_extensible_arrow<'a>(
        &mut self,
        input: &'a str,
        arrow: char,
    ) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        // Optional below: \xrightarrow[below]{above}
        let (input, below) = if input.starts_with('[') {
            let (rest, _) = char('[')(input)?;
            let (rest, ids) = self.parse_node_list(rest)?;
            let (rest, _) = char(']')(rest)?;
            (rest, Some(ids))
        } else {
            (input, None)
        };
        let (input, above_id) = self.parse_required_group(input)?;

        let arrow_sid = self.push_char_str(arrow);
        let base_id = self
            .arena
            .push_node(CompactNode::Symbol(SymbolKind::Relation, arrow_sid));

        match below {
            Some(below_ids) => {
                let under_id = self.flatten_single(&below_ids);
                Ok((
                    input,
                    self.arena.push_node(CompactNode::UnderOver {
                        base: base_id,
                        under: under_id,
                        over: above_id,
                        movable_limits: false,
                    }),
                ))
            },
            None => Ok((
                input,
                self.arena.push_node(CompactNode::Overscript {
                    base: base_id,
                    over: above_id,
                    movable_limits: false,
                }),
            )),
        }
    }

    fn parse_substack<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;

        // separated_list0(row_separator, table_cell)
        let mut cells: Vec<SmallVec<[NodeId; 8]>> = Vec::new();
        let mut rest = input;
        let (r, first) = self.parse_table_cell(rest)?;
        cells.push(first);
        rest = r;
        loop {
            let saved = rest;
            match parse_row_separator(rest) {
                Ok((after_sep, ())) => {
                    let (r, cell) = self.parse_table_cell(after_sep)?;
                    cells.push(cell);
                    rest = r;
                },
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => {
                    rest = saved;
                    break;
                },
            }
        }

        let (input, _) = ws(rest)?;
        let (input, _) = char('}')(input)?;

        let table_rows: Vec<Vec<SmallVec<[NodeId; 8]>>> =
            cells.into_iter().map(|cell| vec![cell]).collect();
        Ok((
            input,
            self.build_array(&[ColumnAlign::Center], table_rows),
        ))
    }

    fn parse_big_delim<'a>(
        &mut self,
        input: &'a str,
        name: &str,
    ) -> IResult<&'a str, NodeId> {
        let factor = match name {
            "big" | "bigl" | "bigr" | "bigm" => Rational::new(6, 5),
            "Big" | "Bigl" | "Bigr" | "Bigm" => Rational::new(9, 5),
            "bigg" | "biggl" | "biggr" | "biggm" => Rational::new(12, 5),
            "Bigg" | "Biggl" | "Biggr" | "Biggm" => Rational::new(3, 1),
            _ => Rational::new(6, 5),
        };
        let (input, _) = ws(input)?;
        let (input, delim_id) = self.parse_big_delim_arg(input)?;
        Ok((
            input,
            self.arena.push_node(CompactNode::Scaled {
                factor,
                content: delim_id,
            }),
        ))
    }

    fn parse_big_delim_arg<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        if let Some(c) = input.chars().next() {
            let rest = &input[c.len_utf8()..];
            let (kind, s) = match c {
                '(' => (SymbolKind::Open, "("),
                ')' => (SymbolKind::Close, ")"),
                '[' => (SymbolKind::Open, "["),
                ']' => (SymbolKind::Close, "]"),
                '|' => (SymbolKind::Fence, "|"),
                '/' => (SymbolKind::Ordinary, "/"),
                _ => return self.parse_command(input),
            };
            let sid = self.arena.push_str(s);
            return Ok((rest, self.arena.push_node(CompactNode::Symbol(kind, sid))));
        }
        self.parse_command(input)
    }

    fn parse_color_discard<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        // \color{name} — consume the color argument, return empty
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _color) =
            take_while1::<_, _, nom::error::Error<&str>>(|c: char| c != '}')(input)
                .unwrap_or((input, ""));
        let (input, _) = char('}')(input)?;
        Ok((
            input,
            self.arena.push_node(CompactNode::Row(NodeSpan::EMPTY)),
        ))
    }

    fn parse_textcolor<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        // \textcolor{name}{content} — consume color, return content
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _color) =
            take_while1::<_, _, nom::error::Error<&str>>(|c: char| c != '}')(input)
                .unwrap_or((input, ""));
        let (input, _) = char('}')(input)?;
        let (input, content_id) = self.parse_required_group(input)?;
        Ok((input, content_id))
    }

    // -----------------------------------------------------------------------
    // Environments
    // -----------------------------------------------------------------------

    fn parse_environment<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, env_name) =
            take_while1(|c: char| c.is_ascii_alphanumeric() || c == '*')(input)?;
        let (input, _) = char('}')(input)?;

        match env_name {
            "array" => self.parse_array_env(input, env_name),
            "matrix" | "pmatrix" | "bmatrix" | "vmatrix" | "Bmatrix" | "Vmatrix" => {
                self.parse_matrix_env(input, env_name)
            },
            "cases" => self.parse_cases_env(input),
            "aligned" | "align" | "align*" => self.parse_align_env(input, env_name),
            "gather" | "gather*" => self.parse_gather_env(input, env_name),
            "equation" | "equation*" => self.parse_equation_env(input, env_name),
            "multline" | "multline*" => self.parse_multline_env(input, env_name),
            "split" => self.parse_align_env(input, env_name),
            _ => {
                // Unknown environment - try to parse as generic array
                self.parse_array_env(input, env_name)
            },
        }
    }

    fn parse_array_env<'a>(
        &mut self,
        input: &'a str,
        env_name: &str,
    ) -> IResult<&'a str, NodeId> {
        let (input, _) = ws(input)?;
        // Optional column spec: {lcr}
        let (input, columns) = 'col: {
            if let Ok((r1, _)) = char::<_, nom::error::Error<&str>>('{')(input)
                && let Ok((r2, spec)) =
                    take_while1::<_, _, nom::error::Error<&str>>(|c: char| c != '}')(r1)
                && let Ok((r3, _)) = char::<_, nom::error::Error<&str>>('}')(r2)
            {
                break 'col (r3, parse_column_spec(spec));
            }
            (input, Vec::new())
        };

        let (input, rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        Ok((input, self.build_array(&columns, rows)))
    }

    fn parse_matrix_env<'a>(
        &mut self,
        input: &'a str,
        env_name: &str,
    ) -> IResult<&'a str, NodeId> {
        let (input, rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        let (open_s, close_s) = match env_name {
            "pmatrix" => ("(", ")"),
            "bmatrix" => ("[", "]"),
            "vmatrix" => ("|", "|"),
            "Bmatrix" => ("{", "}"),
            "Vmatrix" => ("\u{2016}", "\u{2016}"),
            _ => ("", ""),
        };

        let columns: Vec<ColumnAlign> = if let Some(first_row) = rows.first() {
            vec![ColumnAlign::Center; first_row.len()]
        } else {
            vec![]
        };

        let array_id = self.build_array(&columns, rows);

        if open_s.is_empty() {
            Ok((input, array_id))
        } else {
            let open = self.arena.push_str(open_s);
            let close = self.arena.push_str(close_s);
            let content = self.arena.push_children(&[array_id]);
            Ok((
                input,
                self.arena
                    .push_node(CompactNode::Fenced { open, close, content }),
            ))
        }
    }

    fn parse_cases_env<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        let (input, rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag("cases")(input)?;
        let (input, _) = char('}')(input)?;

        let array_id = self.build_array(&[ColumnAlign::Left, ColumnAlign::Left], rows);
        let open = self.arena.push_str("{");
        let close = self.arena.push_str("");
        let content = self.arena.push_children(&[array_id]);
        Ok((
            input,
            self.arena
                .push_node(CompactNode::Fenced { open, close, content }),
        ))
    }

    fn parse_align_env<'a>(
        &mut self,
        input: &'a str,
        env_name: &str,
    ) -> IResult<&'a str, NodeId> {
        let (input, rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        Ok((
            input,
            self.build_array(&[ColumnAlign::Right, ColumnAlign::Left], rows),
        ))
    }

    fn parse_gather_env<'a>(
        &mut self,
        input: &'a str,
        env_name: &str,
    ) -> IResult<&'a str, NodeId> {
        let (input, rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        Ok((input, self.build_array(&[ColumnAlign::Center], rows)))
    }

    fn parse_equation_env<'a>(
        &mut self,
        input: &'a str,
        env_name: &str,
    ) -> IResult<&'a str, NodeId> {
        // equation environment: parse content, strip the wrapper
        let mut ids = SmallVec::<[NodeId; 8]>::new();
        let mut rest = input;
        loop {
            let (r, _) = ws(rest)?;
            rest = r;
            match self.parse_cell_element(rest) {
                Ok((r, id)) => {
                    ids.push(id);
                    rest = r;
                },
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => break,
            }
        }
        let (input, _) = ws(rest)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        Ok((input, self.flatten_single(&ids)))
    }

    fn parse_multline_env<'a>(
        &mut self,
        input: &'a str,
        env_name: &str,
    ) -> IResult<&'a str, NodeId> {
        let (input, rows) = self.parse_table_body(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = tag("\\end")(input)?;
        let (input, _) = ws(input)?;
        let (input, _) = char('{')(input)?;
        let (input, _) = tag(env_name)(input)?;
        let (input, _) = char('}')(input)?;

        let num_rows = rows.len();
        let columns: Vec<ColumnAlign> = if num_rows <= 1 {
            vec![ColumnAlign::Center]
        } else {
            vec![ColumnAlign::Left]
        };
        Ok((input, self.build_array(&columns, rows)))
    }

    // -----------------------------------------------------------------------
    // Table parsing helpers
    // -----------------------------------------------------------------------

    fn parse_table_body<'a>(
        &mut self,
        input: &'a str,
    ) -> IResult<&'a str, Vec<Vec<SmallVec<[NodeId; 8]>>>> {
        let mut rows = Vec::new();
        let mut rest = input;
        // First row
        let (r, first) = self.parse_table_row(rest)?;
        rows.push(first);
        rest = r;
        // Subsequent rows
        loop {
            let saved = rest;
            match parse_row_separator(rest) {
                Ok((after_sep, ())) => {
                    let (r, row) = self.parse_table_row(after_sep)?;
                    rows.push(row);
                    rest = r;
                },
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => {
                    rest = saved;
                    break;
                },
            }
        }
        Ok((rest, rows))
    }

    fn parse_table_row<'a>(
        &mut self,
        input: &'a str,
    ) -> IResult<&'a str, Vec<SmallVec<[NodeId; 8]>>> {
        let mut cells = Vec::new();
        let mut rest = input;
        // First cell
        let (r, first) = self.parse_table_cell(rest)?;
        cells.push(first);
        rest = r;
        // Subsequent cells
        loop {
            let saved = rest;
            match parse_cell_separator(rest) {
                Ok((after_sep, ())) => {
                    let (r, cell) = self.parse_table_cell(after_sep)?;
                    cells.push(cell);
                    rest = r;
                },
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => {
                    rest = saved;
                    break;
                },
            }
        }
        Ok((rest, cells))
    }

    fn parse_table_cell<'a>(
        &mut self,
        input: &'a str,
    ) -> IResult<&'a str, SmallVec<[NodeId; 8]>> {
        let mut ids = SmallVec::new();
        let mut rest = input;
        loop {
            let (r, _) = ws(rest)?;
            rest = r;
            match self.parse_cell_element(rest) {
                Ok((r, id)) => {
                    ids.push(id);
                    rest = r;
                },
                Err(nom::Err::Failure(e)) => return Err(nom::Err::Failure(e)),
                Err(_) => break,
            }
        }
        Ok((rest, ids))
    }

    fn parse_cell_element<'a>(&mut self, input: &'a str) -> IResult<&'a str, NodeId> {
        // Stop at &, \\, or \end
        if input.starts_with('&')
            || input.starts_with("\\\\")
            || input.starts_with("\\end")
            || input.is_empty()
        {
            return Err(nom::Err::Error(nom::error::Error::new(
                input,
                nom::error::ErrorKind::Eof,
            )));
        }
        self.parse_element(input)
    }

    // -----------------------------------------------------------------------
    // Lenient recovery
    // -----------------------------------------------------------------------

    fn parse_lenient_remainder(
        &mut self,
        mut input: &str,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> SmallVec<[NodeId; 8]> {
        let mut ids = SmallVec::new();
        while !input.is_empty() {
            match self.parse_node_list(input) {
                Ok(("", parsed)) => {
                    ids.extend(parsed);
                    break;
                },
                Ok((rest, parsed)) => {
                    ids.extend(parsed);
                    if let Some(ch) = rest.chars().next() {
                        diagnostics.push(Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: format!("skipped unexpected character: {ch:?}"),
                            span: None,
                        });
                        input = &rest[ch.len_utf8()..];
                    } else {
                        break;
                    }
                },
                Err(_) => {
                    if let Some(ch) = input.chars().next() {
                        let sid = self.push_char_str(ch);
                        ids.push(
                            self.arena
                                .push_node(CompactNode::Text(TextStyle::Normal, sid)),
                        );
                        diagnostics.push(Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: format!("treated as text: {ch:?}"),
                            span: None,
                        });
                        input = &input[ch.len_utf8()..];
                    } else {
                        break;
                    }
                },
            }
        }
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Config;

    fn parse(input: &str) -> Vec<MathNode> {
        let config = Config::default();
        LatexReader::parse(input, &config).unwrap().expr.nodes
    }

    fn parse_one(input: &str) -> MathNode {
        let mut nodes = parse(input);
        assert_eq!(nodes.len(), 1, "expected 1 node, got {}", nodes.len());
        nodes.remove(0)
    }

    #[test]
    fn simple_number() {
        assert_eq!(parse_one("42"), MathNode::Number("42".into()));
        assert_eq!(parse_one("3.14"), MathNode::Number("3.14".into()));
    }

    #[test]
    fn simple_identifier() {
        assert_eq!(parse_one("x"), MathNode::Identifier("x".into()));
    }

    #[test]
    fn greek_letter() {
        assert_eq!(
            parse_one("\\alpha"),
            MathNode::Identifier("\u{03B1}".into())
        );
    }

    #[test]
    fn binary_op() {
        let nodes = parse("x + y");
        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[1], MathNode::Symbol(SymbolKind::BinaryOp, "+".into()));
    }

    #[test]
    fn fraction() {
        let node = parse_one("\\frac{a}{b}");
        assert!(matches!(
            node,
            MathNode::Fraction {
                kind: FractionKind::Normal,
                ..
            }
        ));
    }

    #[test]
    fn sqrt_simple() {
        let node = parse_one("\\sqrt{x}");
        assert!(matches!(node, MathNode::Sqrt(_)));
    }

    #[test]
    fn sqrt_with_index() {
        let node = parse_one("\\sqrt[3]{x}");
        assert!(matches!(node, MathNode::Root { .. }));
    }

    #[test]
    fn superscript() {
        let node = parse_one("x^2");
        assert!(matches!(node, MathNode::Superscript { .. }));
    }

    #[test]
    fn subscript() {
        let node = parse_one("x_i");
        assert!(matches!(node, MathNode::Subscript { .. }));
    }

    #[test]
    fn subsup() {
        let node = parse_one("x_i^2");
        assert!(matches!(node, MathNode::SubSup { .. }));
    }

    #[test]
    fn subsup_reversed() {
        let node = parse_one("x^2_i");
        assert!(matches!(node, MathNode::SubSup { .. }));
    }

    #[test]
    fn named_operator() {
        let node = parse_one("\\sin");
        assert!(matches!(node, MathNode::Operator(_)));
    }

    #[test]
    fn nary_sum() {
        let nodes = parse("\\sum_{i=0}^{n} a");
        assert_eq!(nodes.len(), 1);
        assert!(matches!(nodes[0], MathNode::Nary { .. }));
    }

    #[test]
    fn left_right_parens() {
        let node = parse_one("\\left( x \\right)");
        assert!(matches!(node, MathNode::Fenced { .. }));
    }

    #[test]
    fn text_command() {
        let node = parse_one("\\text{hello world}");
        assert_eq!(
            node,
            MathNode::Text(TextStyle::Normal, "hello world".into())
        );
    }

    #[test]
    fn mathbb_command() {
        let node = parse_one("\\mathbb{R}");
        assert!(matches!(node, MathNode::Styled(TextStyle::DoubleStruck, _)));
    }

    #[test]
    fn hat_accent() {
        let node = parse_one("\\hat{x}");
        assert!(matches!(
            node,
            MathNode::Accent {
                position: AccentPosition::Above,
                ..
            }
        ));
    }

    #[test]
    fn braced_group() {
        let node = parse_one("{a + b}");
        assert!(matches!(node, MathNode::Row(_)));
    }

    #[test]
    fn phantom() {
        let node = parse_one("\\phantom{x}");
        assert!(matches!(node, MathNode::Phantom(_)));
    }

    #[test]
    fn boxed() {
        let node = parse_one("\\boxed{x}");
        assert!(matches!(node, MathNode::Boxed(_)));
    }

    #[test]
    fn empty_input() {
        let config = Config::default();
        let result = LatexReader::parse("", &config).unwrap();
        assert!(result.expr.nodes.is_empty());
    }

    #[test]
    fn spacing_commands() {
        let node = parse_one("\\quad");
        assert!(matches!(node, MathNode::Space(_)));
    }

    #[test]
    fn nested_fraction() {
        let node = parse_one("\\frac{\\frac{a}{b}}{c}");
        if let MathNode::Fraction { num, .. } = &node {
            assert!(matches!(**num, MathNode::Fraction { .. }));
        } else {
            panic!("expected fraction");
        }
    }

    #[test]
    fn matrix_pmatrix() {
        let node = parse_one("\\begin{pmatrix} 1 & 2 \\\\ 3 & 4 \\end{pmatrix}");
        assert!(matches!(node, MathNode::Fenced { .. }));
    }

    #[test]
    fn quadratic_formula() {
        // End-to-end test
        let input = "x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}";
        let config = Config::default();
        let result = LatexReader::parse(input, &config);
        assert!(result.is_ok());
        let nodes = result.unwrap().expr.nodes;
        assert!(nodes.len() >= 3); // x, =, fraction
    }

    // === Phase B: New command tests ===

    #[test]
    fn operatorname() {
        let node = parse_one("\\operatorname{Tr}");
        if let MathNode::Operator(attr) = &node {
            assert_eq!(attr.name, "Tr");
        } else {
            panic!("expected Operator, got {node:?}");
        }
    }

    #[test]
    fn overset() {
        let node = parse_one("\\overset{\\sim}{=}");
        assert!(matches!(node, MathNode::Overscript { .. }));
    }

    #[test]
    fn underset() {
        let node = parse_one("\\underset{n}{\\max}");
        assert!(matches!(node, MathNode::Underscript { .. }));
    }

    #[test]
    fn stackrel() {
        let node = parse_one("\\stackrel{\\text{def}}{=}");
        assert!(matches!(node, MathNode::Overscript { .. }));
    }

    #[test]
    fn not_equals() {
        let nodes = parse("\\not=");
        assert_eq!(nodes.len(), 1);
        assert!(matches!(&nodes[0], MathNode::Symbol(SymbolKind::Relation, s) if s == "\u{2260}"));
    }

    #[test]
    fn not_in() {
        let nodes = parse("\\not\\in");
        assert_eq!(nodes.len(), 1);
        assert!(matches!(&nodes[0], MathNode::Symbol(SymbolKind::Relation, s) if s == "\u{2209}"));
    }

    #[test]
    fn sum_limits() {
        let nodes = parse("\\sum\\limits_{i=0}^{n} a");
        assert_eq!(nodes.len(), 1);
        if let MathNode::Nary { limits, .. } = &nodes[0] {
            assert_eq!(*limits, LimitStyle::AboveBelow);
        } else {
            panic!("expected Nary");
        }
    }

    #[test]
    fn sum_nolimits() {
        let nodes = parse("\\sum\\nolimits_{i=0}^{n} a");
        assert_eq!(nodes.len(), 1);
        if let MathNode::Nary { limits, .. } = &nodes[0] {
            assert_eq!(*limits, LimitStyle::SubSup);
        } else {
            panic!("expected Nary");
        }
    }

    #[test]
    fn lim_limits() {
        let nodes = parse("\\lim\\limits_{x \\to 0}");
        // \lim is an Operator; with \limits it gets AboveBelow, then scripts attach
        assert!(!nodes.is_empty());
    }

    #[test]
    fn big_paren() {
        let node = parse_one("\\big(");
        assert!(matches!(node, MathNode::Scaled { .. }));
    }

    #[test]
    fn bigg_bracket() {
        let node = parse_one("\\Bigg[");
        if let MathNode::Scaled { factor, .. } = &node {
            assert_eq!(*factor, Rational::new(3, 1));
        } else {
            panic!("expected Scaled");
        }
    }

    #[test]
    fn pmod() {
        let node = parse_one("\\pmod{n}");
        assert!(matches!(node, MathNode::Fenced { .. }));
    }

    #[test]
    fn bmod() {
        let node = parse_one("\\bmod");
        assert!(matches!(node, MathNode::Operator(_)));
    }

    #[test]
    fn dbinom() {
        let node = parse_one("\\dbinom{n}{k}");
        assert!(matches!(node, MathNode::Fenced { .. }));
    }

    #[test]
    fn xrightarrow() {
        let node = parse_one("\\xrightarrow{f}");
        assert!(matches!(node, MathNode::Overscript { .. }));
    }

    #[test]
    fn xrightarrow_with_below() {
        let node = parse_one("\\xrightarrow[g]{f}");
        assert!(matches!(node, MathNode::UnderOver { .. }));
    }

    #[test]
    fn substack() {
        let node = parse_one("\\substack{i < n \\\\ j > 0}");
        assert!(matches!(node, MathNode::Array { .. }));
    }

    #[test]
    fn color_ignored() {
        let nodes = parse("\\color{red} x");
        // \color{red} produces an empty Row, then x
        assert!(!nodes.is_empty());
    }

    #[test]
    fn textcolor() {
        let node = parse_one("\\textcolor{blue}{x}");
        assert!(matches!(node, MathNode::Identifier(_)));
    }

    #[test]
    fn displaystyle_ignored() {
        let nodes = parse("\\displaystyle x");
        // \displaystyle produces an empty Row, then x
        assert!(!nodes.is_empty());
    }

    #[test]
    fn gather_env() {
        let node = parse_one("\\begin{gather} a \\\\ b \\end{gather}");
        assert!(matches!(node, MathNode::Array { .. }));
    }

    #[test]
    fn equation_env() {
        let node = parse_one("\\begin{equation} x = 1 \\end{equation}");
        // equation env returns its content directly
        assert!(!matches!(node, MathNode::Array { .. }));
    }
}
