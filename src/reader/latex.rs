use nom::IResult;
use nom::branch::alt;
use nom::bytes::complete::{tag, take_while1};
use nom::character::complete::{char, satisfy};
use nom::combinator::{map, opt, recognize};
use nom::multi::{many0, separated_list0};
use nom::sequence::{delimited, preceded, tuple};
use std::cell::Cell;

use crate::ast::*;
use crate::error::{
    Config, Diagnostic, DiagnosticKind, Error, ParseResult, Result, Span, Strictness,
};
use crate::reader::Reader;
use crate::unicode::fonts::latex_font_to_style;
use crate::unicode::symbols::{
    is_nary_command, is_operator_command, latex_to_unicode, lookup_command, operator_has_limits,
};

pub struct LatexReader;

const MAX_PARSE_DEPTH: u32 = 128;

thread_local! {
    static LATEX_DEPTH: Cell<u32> = const { Cell::new(0) };
}

impl Reader for LatexReader {
    fn parse(input: &str, config: &Config) -> Result<ParseResult> {
        LATEX_DEPTH.with(|d| d.set(0));
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(ParseResult::new(MathExpr::new(vec![], config.display)));
        }
        match parse_node_list(trimmed) {
            Ok(("", nodes)) => Ok(ParseResult::new(MathExpr::new(nodes, config.display))),
            Ok((rest, nodes)) => {
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
                    let mut result_nodes = nodes;
                    // Try to recover more content from the remainder
                    let recovered = parse_lenient_remainder(rest, &mut diagnostics);
                    result_nodes.extend(recovered);
                    Ok(ParseResult::with_diagnostics(
                        MathExpr::new(result_nodes, config.display),
                        diagnostics,
                    ))
                }
            },
            Err(e) => {
                if config.strictness == Strictness::Lenient {
                    let mut diagnostics = Vec::new();
                    let nodes = parse_lenient_remainder(trimmed, &mut diagnostics);
                    diagnostics.push(Diagnostic {
                        kind: DiagnosticKind::Warning,
                        message: format!("parse error (recovered): {e}"),
                        span: None,
                    });
                    Ok(ParseResult::with_diagnostics(
                        MathExpr::new(nodes, config.display),
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

/// Lenient recovery: try to parse as much as possible, skipping bad characters.
fn parse_lenient_remainder(mut input: &str, diagnostics: &mut Vec<Diagnostic>) -> Vec<MathNode> {
    let mut nodes = Vec::new();
    while !input.is_empty() {
        match parse_node_list(input) {
            Ok(("", parsed)) => {
                nodes.extend(parsed);
                break;
            },
            Ok((rest, parsed)) => {
                nodes.extend(parsed);
                // Skip one character and continue
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
                // Skip one character and try again
                if let Some(ch) = input.chars().next() {
                    nodes.push(MathNode::Text(TextStyle::Normal, ch.to_string()));
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
    nodes
}

// ---------------------------------------------------------------------------
// Top-level combinators
// ---------------------------------------------------------------------------

fn ws(input: &str) -> IResult<&str, ()> {
    let res: IResult<&str, &str> =
        take_while1(|c: char| c == ' ' || c == '\t' || c == '\n' || c == '\r')(input);
    match res {
        Ok((rest, _)) => Ok((rest, ())),
        Err(_) => Ok((input, ())),
    }
}

fn parse_node_list(input: &str) -> IResult<&str, Vec<MathNode>> {
    LATEX_DEPTH.with(|d| {
        let current = d.get();
        if current >= MAX_PARSE_DEPTH {
            return Err(nom::Err::Failure(nom::error::Error::new(
                input,
                nom::error::ErrorKind::TooLarge,
            )));
        }
        d.set(current + 1);
        let result = many0(preceded(|i| ws(i), parse_element))(input);
        d.set(d.get().saturating_sub(1));
        result
    })
}

/// Parse one element: an atom optionally followed by scripts.
fn parse_element(input: &str) -> IResult<&str, MathNode> {
    let (input, base) = parse_atom(input)?;

    // N-ary operators consume scripts + body specially
    if is_nary_node(&base) {
        return parse_nary_rest(input, base);
    }

    // Check for \limits/\nolimits after Operator nodes
    if let MathNode::Operator(ref attr) = base {
        let (trimmed, _) = ws(input)?;
        if let Ok((rest, _)) = tag::<&str, &str, nom::error::Error<&str>>("\\limits")(trimmed) {
            // Ensure \limits is not followed by more alpha (e.g. \limitset)
            if rest.chars().next().is_none_or(|c| !c.is_ascii_alphabetic()) {
                let mut new_attr = attr.clone();
                new_attr.limit_style = LimitStyle::AboveBelow;
                return parse_scripts(rest, MathNode::Operator(new_attr));
            }
        }
        if let Ok((rest, _)) = tag::<&str, &str, nom::error::Error<&str>>("\\nolimits")(trimmed)
            && rest.chars().next().is_none_or(|c| !c.is_ascii_alphabetic())
        {
            let mut new_attr = attr.clone();
            new_attr.limit_style = LimitStyle::SubSup;
            return parse_scripts(rest, MathNode::Operator(new_attr));
        }
    }

    parse_scripts(input, base)
}

fn is_nary_node(node: &MathNode) -> bool {
    matches!(node, MathNode::Symbol(SymbolKind::LargeOp, _))
}

/// Attach optional sub/super scripts to a base node.
fn parse_scripts(input: &str, base: MathNode) -> IResult<&str, MathNode> {
    let (input, _) = ws(input)?;
    let (input, first_script) = opt(alt((char('^'), char('_'))))(input)?;

    let Some(first) = first_script else {
        return Ok((input, base));
    };

    let (input, first_arg) = parse_script_arg(input)?;
    let (input, _) = ws(input)?;
    let (input, second_script) = opt(if first == '^' { char('_') } else { char('^') })(input)?;

    match (first, second_script) {
        ('^', None) => Ok((
            input,
            MathNode::Superscript {
                base: Box::new(base),
                sup: Box::new(first_arg),
            },
        )),
        ('_', None) => Ok((
            input,
            MathNode::Subscript {
                base: Box::new(base),
                sub: Box::new(first_arg),
            },
        )),
        ('^', Some('_')) => {
            let (input, second_arg) = parse_script_arg(input)?;
            Ok((
                input,
                MathNode::SubSup {
                    base: Box::new(base),
                    sub: Box::new(second_arg),
                    sup: Box::new(first_arg),
                },
            ))
        },
        ('_', Some('^')) => {
            let (input, second_arg) = parse_script_arg(input)?;
            Ok((
                input,
                MathNode::SubSup {
                    base: Box::new(base),
                    sub: Box::new(first_arg),
                    sup: Box::new(second_arg),
                },
            ))
        },
        _ => Ok((input, base)),
    }
}

/// Parse an n-ary operator with optional limits and a body.
fn parse_nary_rest(input: &str, op_node: MathNode) -> IResult<&str, MathNode> {
    let MathNode::Symbol(_, ref s) = op_node else {
        return Ok((input, op_node));
    };
    let op_char = s.chars().next().unwrap_or('\u{2211}');

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
    } else if let Ok((rest, _)) = tag::<&str, &str, nom::error::Error<&str>>("\\nolimits")(input) {
        if rest.chars().next().is_none_or(|c| !c.is_ascii_alphabetic()) {
            (rest, LimitStyle::SubSup)
        } else {
            (input, LimitStyle::Default)
        }
    } else {
        (input, LimitStyle::Default)
    };

    let (input, _) = ws(input)?;
    let (input, limits) = parse_optional_limits(input)?;
    let (input, _) = ws(input)?;

    // The body is the next element (or empty row if nothing follows)
    let (input, body) = opt(parse_atom)(input)?;
    let body = body.unwrap_or(MathNode::Row(vec![]));

    Ok((
        input,
        MathNode::Nary {
            op: op_char,
            sub: limits.0.map(Box::new),
            sup: limits.1.map(Box::new),
            body: Box::new(body),
            limits: limit_style,
        },
    ))
}

/// Parse optional _{...} and ^{...} in any order, returning (sub, sup).
fn parse_optional_limits(input: &str) -> IResult<&str, (Option<MathNode>, Option<MathNode>)> {
    let (input, first) = opt(alt((char('_'), char('^'))))(input)?;
    let Some(first_ch) = first else {
        return Ok((input, (None, None)));
    };
    let (input, first_arg) = parse_script_arg(input)?;
    let (input, _) = ws(input)?;
    let second_ch = if first_ch == '_' { '^' } else { '_' };
    let (input, second) = opt(char(second_ch))(input)?;
    if second.is_some() {
        let (input, second_arg) = parse_script_arg(input)?;
        if first_ch == '_' {
            Ok((input, (Some(first_arg), Some(second_arg))))
        } else {
            Ok((input, (Some(second_arg), Some(first_arg))))
        }
    } else if first_ch == '_' {
        Ok((input, (Some(first_arg), None)))
    } else {
        Ok((input, (None, Some(first_arg))))
    }
}

/// Parse a single script argument: either a single token or a braced group.
fn parse_script_arg(input: &str) -> IResult<&str, MathNode> {
    let (input, _) = ws(input)?;
    parse_atom(input)
}

// ---------------------------------------------------------------------------
// Atoms
// ---------------------------------------------------------------------------

fn parse_atom(input: &str) -> IResult<&str, MathNode> {
    alt((
        parse_braced_group,
        parse_command,
        parse_number,
        parse_symbol_char,
        parse_letter,
    ))(input)
}

/// Parse a braced group `{ ... }` into a Row.
fn parse_braced_group(input: &str) -> IResult<&str, MathNode> {
    let (input, _) = char('{')(input)?;
    let (input, nodes) = parse_node_list(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char('}')(input)?;
    Ok((input, flatten_row(nodes)))
}

fn flatten_row(nodes: Vec<MathNode>) -> MathNode {
    if nodes.len() == 1 {
        nodes.into_iter().next().unwrap()
    } else {
        MathNode::Row(nodes)
    }
}

/// Parse a number: sequence of digits with optional decimal point.
fn parse_number(input: &str) -> IResult<&str, MathNode> {
    let (input, num) = recognize(tuple((
        take_while1(|c: char| c.is_ascii_digit()),
        opt(tuple((
            char('.'),
            take_while1(|c: char| c.is_ascii_digit()),
        ))),
    )))(input)?;
    Ok((input, MathNode::Number(num.to_string())))
}

/// Parse a single letter as an identifier.
fn parse_letter(input: &str) -> IResult<&str, MathNode> {
    let (input, c) = satisfy(|c| c.is_ascii_alphabetic())(input)?;
    Ok((input, MathNode::Identifier(c.to_string())))
}

/// Parse a symbol character: +, -, =, <, >, !, comma, semicolon, etc.
fn parse_symbol_char(input: &str) -> IResult<&str, MathNode> {
    let (input, c) = satisfy(|c| "+-*/=<>!,;:.?'|@".contains(c))(input)?;
    let kind = classify_char(c);
    Ok((input, MathNode::Symbol(kind, c.to_string())))
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

// ---------------------------------------------------------------------------
// Commands (backslash-prefixed)
// ---------------------------------------------------------------------------

fn parse_command(input: &str) -> IResult<&str, MathNode> {
    let (input, _) = char('\\')(input)?;
    // Special single-character commands
    if let Ok((input, node)) = parse_special_command(input) {
        return Ok((input, node));
    }
    let (input, name) = recognize(take_while1(|c: char| c.is_ascii_alphabetic()))(input)?;
    dispatch_command(input, name)
}

fn parse_special_command(input: &str) -> IResult<&str, MathNode> {
    alt((
        // \{ and \} -> delimiter symbols
        map(char('{'), |_| {
            MathNode::Symbol(SymbolKind::Open, "{".into())
        }),
        map(char('}'), |_| {
            MathNode::Symbol(SymbolKind::Close, "}".into())
        }),
        // \| -> double vertical bar
        map(char('|'), |_| {
            MathNode::Symbol(SymbolKind::Fence, "\u{2016}".into())
        }),
        // Spacing: \, \; \: \!
        map(char(','), |_| MathNode::Space(Rational::new(3, 18))),
        map(char(';'), |_| MathNode::Space(Rational::new(5, 18))),
        map(char(':'), |_| MathNode::Space(Rational::new(4, 18))),
        map(char('!'), |_| MathNode::Space(Rational::new(-3, 18))),
        // \\ (newline in arrays - handled at array level, but parse as space elsewhere)
        map(char('\\'), |_| MathNode::Space(Rational::zero())),
        // \ (backslash space)
        map(char(' '), |_| MathNode::Space(Rational::new(1, 4))),
    ))(input)
}

fn dispatch_command<'a>(input: &'a str, name: &str) -> IResult<&'a str, MathNode> {
    match name {
        // --- Fractions ---
        "frac" => parse_frac(input, FractionKind::Normal),
        "dfrac" => parse_frac(input, FractionKind::Display),
        "tfrac" => parse_frac(input, FractionKind::Inline),
        "binom" => parse_binom(input),

        // --- Roots ---
        "sqrt" => parse_sqrt(input),

        // --- Accents (above) ---
        "hat" => parse_accent(input, '\u{0302}', AccentPosition::Above),
        "widehat" => parse_accent(input, '\u{0302}', AccentPosition::Above),
        "bar" => parse_accent(input, '\u{0304}', AccentPosition::Above),
        "overline" => parse_overunder(input, true),
        "underline" => parse_overunder(input, false),
        "vec" => parse_accent(input, '\u{20D7}', AccentPosition::Above),
        "dot" => parse_accent(input, '\u{0307}', AccentPosition::Above),
        "ddot" => parse_accent(input, '\u{0308}', AccentPosition::Above),
        "tilde" => parse_accent(input, '\u{0303}', AccentPosition::Above),
        "widetilde" => parse_accent(input, '\u{0303}', AccentPosition::Above),
        "breve" => parse_accent(input, '\u{0306}', AccentPosition::Above),
        "check" => parse_accent(input, '\u{030C}', AccentPosition::Above),
        "acute" => parse_accent(input, '\u{0301}', AccentPosition::Above),
        "grave" => parse_accent(input, '\u{0300}', AccentPosition::Above),

        // --- Overbrace / underbrace ---
        "overbrace" => parse_overbrace(input),
        "underbrace" => parse_underbrace(input),

        // --- Delimiters ---
        "left" => parse_left_right(input),

        // --- Text and font ---
        "text" | "textrm" | "textit" | "textbf" | "texttt" | "textsf" => parse_text(input, name),
        "mathrm" | "mathbf" | "mathit" | "mathtt" | "mathsf" | "mathbb" | "mathcal" | "mathscr"
        | "mathfrak" | "boldsymbol" | "bm" => parse_math_font(input, name),

        // --- Spacing ---
        "quad" => Ok((input, MathNode::Space(Rational::new(1, 1)))),
        "qquad" => Ok((input, MathNode::Space(Rational::new(2, 1)))),
        "enspace" => Ok((input, MathNode::Space(Rational::new(1, 2)))),

        // --- Decorators ---
        "phantom" => parse_boxed_wrapper(input, |arg| MathNode::Phantom(Box::new(arg))),
        "boxed" | "fbox" => parse_boxed_wrapper(input, |arg| MathNode::Boxed(Box::new(arg))),
        "cancel" => parse_cancel(input, StrokeKind::Forward),
        "bcancel" => parse_cancel(input, StrokeKind::Backward),
        "xcancel" => parse_cancel(input, StrokeKind::Cross),

        // --- Environments ---
        "begin" => parse_environment(input),

        // --- Overset / Underset ---
        "overset" => parse_overset(input),
        "underset" => parse_underset(input),
        "stackrel" => parse_overset(input),

        // --- Operatorname ---
        "operatorname" => parse_operatorname(input),

        // --- Additional binomials ---
        "dbinom" => parse_styled_binom(input, FractionKind::Display),
        "tbinom" => parse_styled_binom(input, FractionKind::Inline),

        // --- Negation ---
        "not" => parse_not(input),

        // --- Mod operators ---
        "bmod" => Ok((
            input,
            MathNode::Operator(OperatorAttr {
                name: "mod".into(),
                limit_style: LimitStyle::Default,
            }),
        )),
        "pmod" => parse_pmod(input),
        "pod" => parse_pmod(input),

        // --- Extensible arrows ---
        "xleftarrow" => parse_extensible_arrow(input, '\u{2190}'),
        "xrightarrow" => parse_extensible_arrow(input, '\u{2192}'),

        // --- Substack ---
        "substack" => parse_substack(input),

        // --- Sized delimiters ---
        "big" | "Big" | "bigg" | "Bigg" | "bigl" | "bigr" | "Bigl" | "Bigr" | "biggl" | "biggr"
        | "Biggl" | "Biggr" | "bigm" | "Bigm" | "biggm" | "Biggm" => parse_big_delim(input, name),

        // --- Style commands (silently consume) ---
        "displaystyle" | "textstyle" | "scriptstyle" | "scriptscriptstyle" => {
            Ok((input, MathNode::Row(vec![])))
        },

        // --- Color (consume color arg, return content) ---
        "color" => parse_color_discard(input),
        "textcolor" => parse_textcolor(input),

        // --- N-ary operators ---
        name if is_nary_command(name) => {
            let ch = latex_to_unicode(name).unwrap_or('\u{2211}');
            Ok((input, MathNode::Symbol(SymbolKind::LargeOp, ch.to_string())))
        },

        // --- Named operators (sin, cos, lim, ...) ---
        name_str if is_operator_command(name_str) => {
            let limit_style = if operator_has_limits(name_str) {
                LimitStyle::Default
            } else {
                LimitStyle::SubSup
            };
            Ok((
                input,
                MathNode::Operator(OperatorAttr {
                    name: name_str.to_string(),
                    limit_style,
                }),
            ))
        },

        // --- Greek letters and other symbols ---
        _ => {
            if let Some((unicode, kind)) = lookup_command(name) {
                if kind == SymbolKind::Alphabetic {
                    Ok((input, MathNode::Identifier(unicode.to_string())))
                } else {
                    Ok((input, MathNode::Symbol(kind, unicode.to_string())))
                }
            } else {
                // Unknown command: emit as identifier
                Ok((input, MathNode::Identifier(name.to_string())))
            }
        },
    }
}

// ---------------------------------------------------------------------------
// Command sub-parsers
// ---------------------------------------------------------------------------

fn parse_boxed_wrapper(input: &str, wrap: fn(MathNode) -> MathNode) -> IResult<&str, MathNode> {
    let (input, arg) = parse_required_group(input)?;
    Ok((input, wrap(arg)))
}

fn parse_cancel(input: &str, stroke: StrokeKind) -> IResult<&str, MathNode> {
    let (input, arg) = parse_required_group(input)?;
    Ok((
        input,
        MathNode::Cancel {
            stroke,
            content: Box::new(arg),
        },
    ))
}

fn parse_required_group(input: &str) -> IResult<&str, MathNode> {
    let (input, _) = ws(input)?;
    parse_braced_group(input)
}

fn parse_frac(input: &str, kind: FractionKind) -> IResult<&str, MathNode> {
    let (input, num) = parse_required_group(input)?;
    let (input, den) = parse_required_group(input)?;
    Ok((
        input,
        MathNode::Fraction {
            kind,
            num: Box::new(num),
            den: Box::new(den),
        },
    ))
}

fn parse_binom(input: &str) -> IResult<&str, MathNode> {
    let (input, top) = parse_required_group(input)?;
    let (input, bottom) = parse_required_group(input)?;
    Ok((
        input,
        MathNode::Fenced {
            open: "(".into(),
            close: ")".into(),
            content: vec![FencedItem::Element(MathNode::Fraction {
                kind: FractionKind::NoBar,
                num: Box::new(top),
                den: Box::new(bottom),
            })],
        },
    ))
}

fn parse_sqrt(input: &str) -> IResult<&str, MathNode> {
    let (input, _) = ws(input)?;
    // Optional index: \sqrt[n]{...}
    let (input, index) = opt(delimited(char('['), parse_node_list, char(']')))(input)?;
    let (input, content) = parse_required_group(input)?;
    match index {
        Some(idx_nodes) => Ok((
            input,
            MathNode::Root {
                index: Box::new(flatten_row(idx_nodes)),
                radicand: Box::new(content),
            },
        )),
        None => Ok((input, MathNode::Sqrt(Box::new(content)))),
    }
}

fn parse_accent(
    input: &str,
    accent_char: char,
    position: AccentPosition,
) -> IResult<&str, MathNode> {
    let (input, base) = parse_required_group(input)?;
    Ok((
        input,
        MathNode::Accent {
            base: Box::new(base),
            accent: accent_char,
            position,
        },
    ))
}

fn parse_overunder(input: &str, is_over: bool) -> IResult<&str, MathNode> {
    let (input, base) = parse_required_group(input)?;
    let line = MathNode::Symbol(SymbolKind::Ordinary, "\u{203E}".into());
    if is_over {
        Ok((
            input,
            MathNode::Overscript {
                base: Box::new(base),
                over: Box::new(line),
                movable_limits: false,
            },
        ))
    } else {
        Ok((
            input,
            MathNode::Underscript {
                base: Box::new(base),
                under: Box::new(line),
                movable_limits: false,
            },
        ))
    }
}

fn parse_overbrace(input: &str) -> IResult<&str, MathNode> {
    let (input, base) = parse_required_group(input)?;
    Ok((
        input,
        MathNode::Overscript {
            base: Box::new(base),
            over: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "\u{23DE}".into())),
            movable_limits: false,
        },
    ))
}

fn parse_underbrace(input: &str) -> IResult<&str, MathNode> {
    let (input, base) = parse_required_group(input)?;
    Ok((
        input,
        MathNode::Underscript {
            base: Box::new(base),
            under: Box::new(MathNode::Symbol(SymbolKind::Ordinary, "\u{23DF}".into())),
            movable_limits: false,
        },
    ))
}

/// Parse \left delim ... \right delim
fn parse_left_right(input: &str) -> IResult<&str, MathNode> {
    let (input, _) = ws(input)?;
    let (input, open) = parse_delimiter(input)?;
    let (input, content_nodes) = parse_until_right(input)?;
    let (input, close) = parse_delimiter(input)?;

    let content = content_nodes.into_iter().map(FencedItem::Element).collect();

    Ok((
        input,
        MathNode::Fenced {
            open,
            close,
            content,
        },
    ))
}

fn parse_delimiter(input: &str) -> IResult<&str, String> {
    alt((
        map(char('('), |_| "(".into()),
        map(char(')'), |_| ")".into()),
        map(char('['), |_| "[".into()),
        map(char(']'), |_| "]".into()),
        map(char('|'), |_| "|".into()),
        map(char('.'), |_| "".into()), // invisible delimiter
        map(preceded(char('\\'), char('{')), |_| "{".into()),
        map(preceded(char('\\'), char('}')), |_| "}".into()),
        map(preceded(char('\\'), char('|')), |_| "\u{2016}".into()),
        map(
            preceded(char('\\'), take_while1(|c: char| c.is_ascii_alphabetic())),
            |name: &str| {
                latex_to_unicode(name)
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| name.to_string())
            },
        ),
    ))(input)
}

fn parse_until_right(input: &str) -> IResult<&str, Vec<MathNode>> {
    let mut nodes = Vec::new();
    let mut rest = input;
    loop {
        let (r, _) = ws(rest)?;
        rest = r;
        // Check for \right
        if let Ok((after_right, _)) = tag::<&str, &str, nom::error::Error<&str>>("\\right")(rest) {
            return Ok((after_right, nodes));
        }
        // Check for \middle
        if let Ok((after_mid, _)) = tag::<&str, &str, nom::error::Error<&str>>("\\middle")(rest) {
            let (after_mid, _) = ws(after_mid)?;
            let (after_delim, delim) = parse_delimiter(after_mid)?;
            // Insert a Middle item - but we return Vec<MathNode>, so we handle this
            // by returning what we have and letting the caller deal with it.
            // For simplicity, we push a symbol marker and continue.
            nodes.push(MathNode::Symbol(SymbolKind::Fence, delim));
            rest = after_delim;
            continue;
        }
        if rest.is_empty() {
            return Err(nom::Err::Error(nom::error::Error::new(
                rest,
                nom::error::ErrorKind::Tag,
            )));
        }
        let (r, node) = parse_element(rest)?;
        nodes.push(node);
        rest = r;
    }
}

fn parse_text<'a>(input: &'a str, cmd_name: &str) -> IResult<&'a str, MathNode> {
    let style = match cmd_name {
        "textbf" => TextStyle::Bold,
        "textit" => TextStyle::Italic,
        "texttt" => TextStyle::Monospace,
        "textsf" => TextStyle::SansSerif,
        _ => TextStyle::Normal,
    };
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;
    let (input, text) = take_while1::<_, _, nom::error::Error<&str>>(|c: char| c != '}')(input)
        .unwrap_or((input, ""));
    let (input, _) = char('}')(input)?;
    Ok((input, MathNode::Text(style, text.to_string())))
}

fn parse_math_font<'a>(input: &'a str, cmd_name: &str) -> IResult<&'a str, MathNode> {
    let style = latex_font_to_style(cmd_name).unwrap_or(TextStyle::Normal);
    let (input, content) = parse_required_group(input)?;
    match content {
        MathNode::Row(children) => Ok((input, MathNode::Styled(style, children))),
        single => Ok((input, MathNode::Styled(style, vec![single]))),
    }
}

fn parse_overset(input: &str) -> IResult<&str, MathNode> {
    let (input, over) = parse_required_group(input)?;
    let (input, base) = parse_required_group(input)?;
    Ok((
        input,
        MathNode::Overscript {
            base: Box::new(base),
            over: Box::new(over),
            movable_limits: false,
        },
    ))
}

fn parse_underset(input: &str) -> IResult<&str, MathNode> {
    let (input, under) = parse_required_group(input)?;
    let (input, base) = parse_required_group(input)?;
    Ok((
        input,
        MathNode::Underscript {
            base: Box::new(base),
            under: Box::new(under),
            movable_limits: false,
        },
    ))
}

fn parse_operatorname(input: &str) -> IResult<&str, MathNode> {
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;
    let (input, name) = take_while1::<_, _, nom::error::Error<&str>>(|c: char| c != '}')(input)
        .unwrap_or((input, ""));
    let (input, _) = char('}')(input)?;
    Ok((
        input,
        MathNode::Operator(OperatorAttr {
            name: name.to_string(),
            limit_style: LimitStyle::Default,
        }),
    ))
}

fn parse_styled_binom(input: &str, kind: FractionKind) -> IResult<&str, MathNode> {
    let (input, top) = parse_required_group(input)?;
    let (input, bottom) = parse_required_group(input)?;
    Ok((
        input,
        MathNode::Fenced {
            open: "(".into(),
            close: ")".into(),
            content: vec![FencedItem::Element(MathNode::Fraction {
                kind,
                num: Box::new(top),
                den: Box::new(bottom),
            })],
        },
    ))
}

fn parse_not(input: &str) -> IResult<&str, MathNode> {
    let (input, _) = ws(input)?;
    // Try direct character negation: \not= \not< \not>
    if let Some(first) = input.chars().next()
        && let Some(negated) = negate_char(first)
    {
        let rest = &input[first.len_utf8()..];
        return Ok((
            rest,
            MathNode::Symbol(SymbolKind::Relation, negated.to_string()),
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
                return Ok((
                    rest,
                    MathNode::Symbol(SymbolKind::Relation, negated.to_string()),
                ));
            }
        }
    }
    // Fallback: combining solidus overlay
    Ok((
        input,
        MathNode::Symbol(SymbolKind::Ordinary, "\u{0338}".to_string()),
    ))
}

fn negate_char(ch: char) -> Option<char> {
    match ch {
        '=' => Some('\u{2260}'),        // ≠
        '<' => Some('\u{226E}'),        // ≮
        '>' => Some('\u{226F}'),        // ≯
        '\u{2208}' => Some('\u{2209}'), // ∉
        '\u{2282}' => Some('\u{2284}'), // ⊄
        '\u{2283}' => Some('\u{2285}'), // ⊅
        '\u{2286}' => Some('\u{2288}'), // ⊈
        '\u{2287}' => Some('\u{2289}'), // ⊉
        '\u{2261}' => Some('\u{2262}'), // ≢
        '\u{223C}' => Some('\u{2241}'), // ≁
        '\u{2264}' => Some('\u{2270}'), // ≰
        '\u{2265}' => Some('\u{2271}'), // ≱
        '\u{227A}' => Some('\u{2280}'), // ⊀
        '\u{227B}' => Some('\u{2281}'), // ⊁
        '\u{2203}' => Some('\u{2204}'), // ∄
        _ => None,
    }
}

fn parse_pmod(input: &str) -> IResult<&str, MathNode> {
    let (input, content) = parse_required_group(input)?;
    Ok((
        input,
        MathNode::Fenced {
            open: "(".into(),
            close: ")".into(),
            content: vec![
                FencedItem::Element(MathNode::Operator(OperatorAttr {
                    name: "mod".into(),
                    limit_style: LimitStyle::Default,
                })),
                FencedItem::Element(MathNode::Space(Rational::new(5, 18))),
                FencedItem::Element(content),
            ],
        },
    ))
}

fn parse_extensible_arrow(input: &str, arrow: char) -> IResult<&str, MathNode> {
    let (input, _) = ws(input)?;
    // Optional below: \xrightarrow[below]{above}
    let (input, below) = opt(delimited(char('['), parse_node_list, char(']')))(input)?;
    let (input, above) = parse_required_group(input)?;

    let base = MathNode::Symbol(SymbolKind::Relation, arrow.to_string());

    match below {
        Some(below_nodes) => Ok((
            input,
            MathNode::UnderOver {
                base: Box::new(base),
                under: Box::new(flatten_row(below_nodes)),
                over: Box::new(above),
                movable_limits: false,
            },
        )),
        None => Ok((
            input,
            MathNode::Overscript {
                base: Box::new(base),
                over: Box::new(above),
                movable_limits: false,
            },
        )),
    }
}

fn parse_substack(input: &str) -> IResult<&str, MathNode> {
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;
    let (input, rows) = separated_list0(parse_row_separator, parse_table_cell)(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char('}')(input)?;

    let table_rows: Vec<Vec<Vec<MathNode>>> = rows.into_iter().map(|cell| vec![cell]).collect();
    let columns = vec![ColumnAlign::Center];

    Ok((
        input,
        MathNode::Array {
            columns,
            rows: table_rows,
        },
    ))
}

fn parse_big_delim<'a>(input: &'a str, name: &str) -> IResult<&'a str, MathNode> {
    let factor = match name {
        "big" | "bigl" | "bigr" | "bigm" => Rational::new(6, 5),
        "Big" | "Bigl" | "Bigr" | "Bigm" => Rational::new(9, 5),
        "bigg" | "biggl" | "biggr" | "biggm" => Rational::new(12, 5),
        "Bigg" | "Biggl" | "Biggr" | "Biggm" => Rational::new(3, 1),
        _ => Rational::new(6, 5),
    };
    let (input, _) = ws(input)?;
    let (input, delim_node) = parse_big_delim_arg(input)?;
    Ok((
        input,
        MathNode::Scaled {
            factor,
            content: Box::new(delim_node),
        },
    ))
}

fn parse_big_delim_arg(input: &str) -> IResult<&str, MathNode> {
    alt((
        map(char('('), |_| {
            MathNode::Symbol(SymbolKind::Open, "(".into())
        }),
        map(char(')'), |_| {
            MathNode::Symbol(SymbolKind::Close, ")".into())
        }),
        map(char('['), |_| {
            MathNode::Symbol(SymbolKind::Open, "[".into())
        }),
        map(char(']'), |_| {
            MathNode::Symbol(SymbolKind::Close, "]".into())
        }),
        map(char('|'), |_| {
            MathNode::Symbol(SymbolKind::Fence, "|".into())
        }),
        map(char('/'), |_| {
            MathNode::Symbol(SymbolKind::Ordinary, "/".into())
        }),
        parse_command,
    ))(input)
}

fn parse_color_discard(input: &str) -> IResult<&str, MathNode> {
    // \color{name} — consume the color argument, return empty
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;
    let (input, _color) = take_while1::<_, _, nom::error::Error<&str>>(|c: char| c != '}')(input)
        .unwrap_or((input, ""));
    let (input, _) = char('}')(input)?;
    Ok((input, MathNode::Row(vec![])))
}

fn parse_textcolor(input: &str) -> IResult<&str, MathNode> {
    // \textcolor{name}{content} — consume color, return content
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;
    let (input, _color) = take_while1::<_, _, nom::error::Error<&str>>(|c: char| c != '}')(input)
        .unwrap_or((input, ""));
    let (input, _) = char('}')(input)?;
    let (input, content) = parse_required_group(input)?;
    Ok((input, content))
}

// ---------------------------------------------------------------------------
// Environments
// ---------------------------------------------------------------------------

fn parse_environment(input: &str) -> IResult<&str, MathNode> {
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;
    let (input, env_name) = take_while1(|c: char| c.is_ascii_alphanumeric() || c == '*')(input)?;
    let (input, _) = char('}')(input)?;

    match env_name {
        "array" => parse_array_env(input, env_name),
        "matrix" | "pmatrix" | "bmatrix" | "vmatrix" | "Bmatrix" | "Vmatrix" => {
            parse_matrix_env(input, env_name)
        },
        "cases" => parse_cases_env(input),
        "aligned" | "align" | "align*" => parse_align_env(input, env_name),
        "gather" | "gather*" => parse_gather_env(input, env_name),
        "equation" | "equation*" => parse_equation_env(input, env_name),
        "multline" | "multline*" => parse_multline_env(input, env_name),
        "split" => parse_align_env(input, env_name),
        _ => {
            // Unknown environment - try to parse as generic array
            parse_array_env(input, env_name)
        },
    }
}

fn parse_array_env<'a>(input: &'a str, env_name: &str) -> IResult<&'a str, MathNode> {
    // Parse optional column spec
    let (input, _) = ws(input)?;
    let (input, col_spec) = opt(delimited(
        char('{'),
        take_while1(|c: char| c != '}'),
        char('}'),
    ))(input)?;
    let columns = col_spec.map(parse_column_spec).unwrap_or_default();

    let (input, rows) = parse_table_body(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = tag("\\end")(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;
    let (input, _) = tag(env_name)(input)?;
    let (input, _) = char('}')(input)?;

    Ok((input, MathNode::Array { columns, rows }))
}

fn parse_matrix_env<'a>(input: &'a str, env_name: &str) -> IResult<&'a str, MathNode> {
    let (input, rows) = parse_table_body(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = tag("\\end")(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;
    let (input, _) = tag(env_name)(input)?;
    let (input, _) = char('}')(input)?;

    let (open, close) = match env_name {
        "pmatrix" => ("(", ")"),
        "bmatrix" => ("[", "]"),
        "vmatrix" => ("|", "|"),
        "Bmatrix" => ("{", "}"),
        "Vmatrix" => ("\u{2016}", "\u{2016}"),
        _ => ("", ""),
    };

    let columns = if let Some(first_row) = rows.first() {
        vec![ColumnAlign::Center; first_row.len()]
    } else {
        vec![]
    };

    let array = MathNode::Array { columns, rows };

    if open.is_empty() {
        Ok((input, array))
    } else {
        Ok((
            input,
            MathNode::Fenced {
                open: open.into(),
                close: close.into(),
                content: vec![FencedItem::Element(array)],
            },
        ))
    }
}

fn parse_cases_env(input: &str) -> IResult<&str, MathNode> {
    let (input, rows) = parse_table_body(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = tag("\\end")(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;
    let (input, _) = tag("cases")(input)?;
    let (input, _) = char('}')(input)?;

    let columns = vec![ColumnAlign::Left, ColumnAlign::Left];
    Ok((
        input,
        MathNode::Fenced {
            open: "{".into(),
            close: "".into(),
            content: vec![FencedItem::Element(MathNode::Array { columns, rows })],
        },
    ))
}

fn parse_align_env<'a>(input: &'a str, env_name: &str) -> IResult<&'a str, MathNode> {
    let (input, rows) = parse_table_body(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = tag("\\end")(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;
    let (input, _) = tag(env_name)(input)?;
    let (input, _) = char('}')(input)?;

    let columns = vec![ColumnAlign::Right, ColumnAlign::Left];
    Ok((input, MathNode::Array { columns, rows }))
}

fn parse_gather_env<'a>(input: &'a str, env_name: &str) -> IResult<&'a str, MathNode> {
    let (input, rows) = parse_table_body(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = tag("\\end")(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;
    let (input, _) = tag(env_name)(input)?;
    let (input, _) = char('}')(input)?;

    let columns = vec![ColumnAlign::Center];
    Ok((input, MathNode::Array { columns, rows }))
}

fn parse_equation_env<'a>(input: &'a str, env_name: &str) -> IResult<&'a str, MathNode> {
    // equation environment: parse content (stopping at \end), strip the wrapper
    let (input, nodes) = many0(preceded(|i| ws(i), parse_cell_element))(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = tag("\\end")(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;
    let (input, _) = tag(env_name)(input)?;
    let (input, _) = char('}')(input)?;

    Ok((input, flatten_row(nodes)))
}

fn parse_multline_env<'a>(input: &'a str, env_name: &str) -> IResult<&'a str, MathNode> {
    let (input, rows) = parse_table_body(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = tag("\\end")(input)?;
    let (input, _) = ws(input)?;
    let (input, _) = char('{')(input)?;
    let (input, _) = tag(env_name)(input)?;
    let (input, _) = char('}')(input)?;

    // First row left, last row right, middle centered
    let num_rows = rows.len();
    let columns: Vec<ColumnAlign> = if num_rows <= 1 {
        vec![ColumnAlign::Center]
    } else {
        vec![ColumnAlign::Left] // simplified: single column, left-aligned
    };
    Ok((input, MathNode::Array { columns, rows }))
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

fn parse_table_body(input: &str) -> IResult<&str, Vec<Vec<Vec<MathNode>>>> {
    let (input, rows) = separated_list0(parse_row_separator, parse_table_row)(input)?;
    Ok((input, rows))
}

fn parse_row_separator(input: &str) -> IResult<&str, ()> {
    let (input, _) = ws(input)?;
    let (input, _) = tag("\\\\")(input)?;
    let (input, _) = ws(input)?;
    Ok((input, ()))
}

fn parse_table_row(input: &str) -> IResult<&str, Vec<Vec<MathNode>>> {
    separated_list0(parse_cell_separator, parse_table_cell)(input)
}

fn parse_cell_separator(input: &str) -> IResult<&str, ()> {
    let (input, _) = ws(input)?;
    let (input, _) = char('&')(input)?;
    let (input, _) = ws(input)?;
    Ok((input, ()))
}

fn parse_table_cell(input: &str) -> IResult<&str, Vec<MathNode>> {
    many0(preceded(|i| ws(i), parse_cell_element))(input)
}

fn parse_cell_element(input: &str) -> IResult<&str, MathNode> {
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
    parse_element(input)
}

#[cfg(test)]
mod tests {
    use super::*;

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
