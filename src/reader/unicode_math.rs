use nom::branch::alt;
use nom::bytes::complete::{tag, take_while1};
use nom::character::complete::{char as nom_char, satisfy};
use nom::combinator::opt;
use nom::multi::many0;
use nom::sequence::preceded;
use nom::IResult;

use crate::ast::*;
use crate::error::{Config, Diagnostic, DiagnosticKind, Error, ParseResult, Result, Strictness};
use crate::reader::Reader;

pub struct UnicodeMathReader;

impl Reader for UnicodeMathReader {
    fn parse(input: &str, config: &Config) -> Result<ParseResult> {
        let input = input.trim();
        if input.is_empty() {
            return Ok(ParseResult::new(MathExpr::inline(vec![])));
        }
        match parse_expr_list(input) {
            Ok(nodes) => Ok(ParseResult::new(MathExpr::inline(nodes))),
            Err(e) => {
                if config.strictness == Strictness::Lenient {
                    // Total failure: treat entire input as text
                    Ok(ParseResult::with_diagnostics(
                        MathExpr::inline(vec![MathNode::Text(TextStyle::Normal, input.into())]),
                        vec![Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: format!("UnicodeMath parse error (recovered as text): {e}"),
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

fn parse_expr_list(input: &str) -> Result<Vec<MathNode>> {
    match many0(parse_element)(input) {
        Ok((rest, nodes)) => {
            if !rest.trim().is_empty() {
                // If there's leftover, try to consume as text
                let mut result = nodes;
                result.push(MathNode::Text(TextStyle::Normal, rest.trim().into()));
                Ok(result)
            } else {
                Ok(nodes)
            }
        },
        Err(_) => Err(Error::Unsupported("UnicodeMath parse error".into())),
    }
}

fn parse_element(input: &str) -> IResult<&str, MathNode> {
    let (input, base) = parse_atom(input)?;
    parse_scripts(input, base)
}

fn parse_scripts(input: &str, base: MathNode) -> IResult<&str, MathNode> {
    // Check for subscript/superscript
    let (input, sub) = opt(preceded(nom_char('_'), parse_script_arg))(input)?;
    let (input, sup) = opt(preceded(nom_char('^'), parse_script_arg))(input)?;

    match (sub, sup) {
        (None, None) => {
            // Check for fraction: base / denominator
            let (input, frac) = opt(preceded(nom_char('/'), parse_script_arg))(input)?;
            match frac {
                Some(den) => Ok((
                    input,
                    MathNode::Fraction {
                        kind: FractionKind::Normal,
                        num: Box::new(base),
                        den: Box::new(den),
                    },
                )),
                None => Ok((input, base)),
            }
        },
        (Some(sub), None) => Ok((
            input,
            MathNode::Subscript {
                base: Box::new(base),
                sub: Box::new(sub),
            },
        )),
        (None, Some(sup)) => Ok((
            input,
            MathNode::Superscript {
                base: Box::new(base),
                sup: Box::new(sup),
            },
        )),
        (Some(sub), Some(sup)) => Ok((
            input,
            MathNode::SubSup {
                base: Box::new(base),
                sub: Box::new(sub),
                sup: Box::new(sup),
            },
        )),
    }
}

fn parse_script_arg(input: &str) -> IResult<&str, MathNode> {
    alt((parse_group, parse_single_char_node))(input)
}

fn parse_atom(input: &str) -> IResult<&str, MathNode> {
    alt((
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
    ))(input)
}

fn parse_group(input: &str) -> IResult<&str, MathNode> {
    // Parse (content), [content], {content}
    alt((
        parse_paren_group,
        parse_bracket_group,
        parse_brace_group,
    ))(input)
}

fn parse_paren_group(input: &str) -> IResult<&str, MathNode> {
    let (input, _) = nom_char('(')(input)?;
    let (input, children) = many0(parse_element)(input)?;
    let (input, _) = nom_char(')')(input)?;
    let content = if children.len() == 1 {
        children.into_iter().next().unwrap()
    } else {
        MathNode::Row(children)
    };
    Ok((
        input,
        MathNode::Fenced {
            open: "(".into(),
            close: ")".into(),
            content: vec![FencedItem::Element(content)],
        },
    ))
}

fn parse_bracket_group(input: &str) -> IResult<&str, MathNode> {
    let (input, _) = nom_char('[')(input)?;
    let (input, children) = many0(parse_non_bracket_element)(input)?;
    let (input, _) = nom_char(']')(input)?;
    let content = if children.len() == 1 {
        children.into_iter().next().unwrap()
    } else {
        MathNode::Row(children)
    };
    Ok((
        input,
        MathNode::Fenced {
            open: "[".into(),
            close: "]".into(),
            content: vec![FencedItem::Element(content)],
        },
    ))
}

fn parse_non_bracket_element(input: &str) -> IResult<&str, MathNode> {
    // Like parse_element but doesn't consume ]
    if input.starts_with(']') {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )));
    }
    parse_element(input)
}

fn parse_brace_group(input: &str) -> IResult<&str, MathNode> {
    let (input, _) = nom_char('{')(input)?;
    let (input, children) = many0(parse_non_brace_element)(input)?;
    let (input, _) = nom_char('}')(input)?;
    let content = if children.len() == 1 {
        children.into_iter().next().unwrap()
    } else {
        MathNode::Row(children)
    };
    Ok((
        input,
        MathNode::Fenced {
            open: "{".into(),
            close: "}".into(),
            content: vec![FencedItem::Element(content)],
        },
    ))
}

fn parse_non_brace_element(input: &str) -> IResult<&str, MathNode> {
    if input.starts_with('}') {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Tag,
        )));
    }
    parse_element(input)
}

fn parse_sqrt(input: &str) -> IResult<&str, MathNode> {
    let (input, _) = nom_char('\u{221A}')(input)?; // √
    let (input, content) = parse_script_arg(input)?;
    // Check if it's an nth root: √(n&content)
    Ok((input, MathNode::Sqrt(Box::new(content))))
}

fn parse_cbrt(input: &str) -> IResult<&str, MathNode> {
    let (input, _) = nom_char('\u{221B}')(input)?; // ∛
    let (input, content) = parse_script_arg(input)?;
    Ok((
        input,
        MathNode::Root {
            index: Box::new(MathNode::Number("3".into())),
            radicand: Box::new(content),
        },
    ))
}

fn parse_matrix(input: &str) -> IResult<&str, MathNode> {
    let (input, _) = nom_char('\u{25A0}')(input)?; // ■
    let (input, _) = nom_char('(')(input)?;
    let (input, content) = parse_matrix_content(input)?;
    let (input, _) = nom_char(')')(input)?;
    Ok((input, content))
}

fn parse_matrix_content(input: &str) -> IResult<&str, MathNode> {
    // Parse rows separated by @, cells separated by &
    let mut rows: Vec<Vec<Vec<MathNode>>> = Vec::new();
    let mut current_row: Vec<Vec<MathNode>> = Vec::new();
    let mut current_cell: Vec<MathNode> = Vec::new();

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
        match parse_element(remaining) {
            Ok((rest, node)) => {
                current_cell.push(node);
                remaining = rest;
            },
            Err(_) => {
                // Skip one char
                remaining = &remaining[remaining.char_indices().nth(1).map_or(remaining.len(), |(i, _)| i)..];
            },
        }
    }
    // Finalize
    if !current_cell.is_empty() || !current_row.is_empty() {
        current_row.push(current_cell);
        rows.push(current_row);
    }

    let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(1);
    let columns = vec![ColumnAlign::Center; num_cols];

    Ok((
        remaining,
        MathNode::Array { columns, rows },
    ))
}

fn parse_text_quoted(input: &str) -> IResult<&str, MathNode> {
    let (input, _) = nom_char('"')(input)?;
    let (input, text) = take_while1(|c: char| c != '"')(input)?;
    let (input, _) = nom_char('"')(input)?;
    Ok((input, MathNode::Text(TextStyle::Normal, text.into())))
}

fn parse_nary_op(input: &str) -> IResult<&str, MathNode> {
    let (input, op) = satisfy(is_nary_char)(input)?;
    let (input, sub) = opt(preceded(nom_char('_'), parse_script_arg))(input)?;
    let (input, sup) = opt(preceded(nom_char('^'), parse_script_arg))(input)?;

    // Skip optional whitespace before body
    let input = input.trim_start();

    let (input, body) = if input.is_empty() {
        (input, MathNode::Row(vec![]))
    } else {
        match parse_script_arg(input) {
            Ok((rest, node)) => (rest, node),
            Err(_) => (input, MathNode::Row(vec![])),
        }
    };

    let limits = if is_integral(op) {
        LimitStyle::SubSup
    } else {
        LimitStyle::AboveBelow
    };

    Ok((
        input,
        MathNode::Nary {
            op,
            sub: sub.map(Box::new),
            sup: sup.map(Box::new),
            body: Box::new(body),
            limits,
        },
    ))
}

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
    matches!(
        c,
        '\u{222B}' | '\u{222C}' | '\u{222D}' | '\u{222E}'
    )
}

fn parse_number(input: &str) -> IResult<&str, MathNode> {
    let (input, num) = take_while1(|c: char| c.is_ascii_digit() || c == '.')(input)?;
    // Ensure we actually got digits, not just dots
    if num.chars().all(|c| c == '.') {
        return Err(nom::Err::Error(nom::error::Error::new(
            input,
            nom::error::ErrorKind::Digit,
        )));
    }
    Ok((input, MathNode::Number(num.into())))
}

fn parse_operator_word(input: &str) -> IResult<&str, MathNode> {
    // Common operator names
    let ops = [
        "sin", "cos", "tan", "sec", "csc", "cot", "sinh", "cosh", "tanh", "coth",
        "arcsin", "arccos", "arctan", "log", "ln", "lg", "exp", "lim", "liminf",
        "limsup", "sup", "inf", "max", "min", "det", "dim", "ker", "hom", "arg",
        "deg", "gcd", "lcm", "sgn", "Pr", "Var", "Cov", "supp", "mod",
    ];
    for op in &ops {
        if let Ok((rest, _)) = tag::<&str, &str, nom::error::Error<&str>>(*op)(input) {
            // Make sure it's not followed by an alphanumeric char
            if rest.chars().next().map_or(true, |c| !c.is_alphanumeric()) {
                let has_limits = matches!(*op, "lim" | "liminf" | "limsup" | "sup" | "inf" | "max" | "min");
                return Ok((
                    rest,
                    MathNode::Operator(OperatorAttr {
                        name: op.to_string(),
                        limit_style: if has_limits {
                            LimitStyle::AboveBelow
                        } else {
                            LimitStyle::Default
                        },
                    }),
                ));
            }
        }
    }
    Err(nom::Err::Error(nom::error::Error::new(
        input,
        nom::error::ErrorKind::Tag,
    )))
}

fn parse_symbol(input: &str) -> IResult<&str, MathNode> {
    let (input, ch) = satisfy(|c| is_math_symbol(c))(input)?;
    let kind = classify_unicode_symbol(ch);
    // Skip whitespace after binary/relation operators
    let input = if matches!(kind, SymbolKind::BinaryOp | SymbolKind::Relation) {
        input.trim_start()
    } else {
        input
    };
    Ok((input, MathNode::Symbol(kind, ch.to_string())))
}

fn parse_identifier(input: &str) -> IResult<&str, MathNode> {
    let (input, ch) = satisfy(|c| {
        c.is_alphabetic() && !is_math_symbol(c) && !is_nary_char(c) && c != '"'
    })(input)?;
    Ok((input, MathNode::Identifier(ch.to_string())))
}

fn parse_single_char_node(input: &str) -> IResult<&str, MathNode> {
    alt((parse_number, parse_symbol, parse_identifier))(input)
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
        '+' | '-' | '\u{00B1}' | '\u{2213}' | '\u{00D7}' | '\u{00F7}' | '\u{22C5}'
        | '\u{2227}' | '\u{2228}' | '\u{2295}' | '\u{2297}' | '\u{2229}' | '\u{222A}' => {
            SymbolKind::BinaryOp
        },
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
        assert!(matches!(&nodes[1], MathNode::Symbol(SymbolKind::BinaryOp, _)));
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
