use quick_xml::Reader;
use quick_xml::events::Event;

use crate::ast::*;
use crate::error::{Config, Diagnostic, DiagnosticKind, Error, ParseResult, Result, Strictness};
use crate::reader::Reader as MathReader;

pub struct MathmlReader;

impl MathReader for MathmlReader {
    fn parse(input: &str, config: &Config) -> Result<ParseResult> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(ParseResult::new(MathExpr::new(vec![], config.display)));
        }
        let parser_result = MathmlParser::new(trimmed);
        let mut parser = match parser_result {
            Ok(p) => p,
            Err(e) => {
                if config.strictness == Strictness::Lenient {
                    return Ok(ParseResult::with_diagnostics(
                        MathExpr::new(vec![], config.display),
                        vec![Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: format!("MathML initialization failed (recovered): {e}"),
                            span: None,
                        }],
                    ));
                }
                return Err(e);
            }
        };
        match parser.parse_root() {
            Ok(nodes) => {
                let display = parser.display.unwrap_or(config.display);
                Ok(ParseResult::new(MathExpr::new(nodes, display)))
            }
            Err(e) => {
                if config.strictness == Strictness::Lenient {
                    let display = parser.display.unwrap_or(config.display);
                    Ok(ParseResult::with_diagnostics(
                        MathExpr::new(vec![], display),
                        vec![Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: format!("MathML parse error (recovered): {e}"),
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

struct MathmlParser<'a> {
    reader: Reader<&'a [u8]>,
    display: Option<DisplayStyle>,
}

impl<'a> MathmlParser<'a> {
    fn new(input: &'a str) -> Result<Self> {
        let mut reader = Reader::from_str(input);
        reader.config_mut().trim_text(true);
        Ok(Self {
            reader,
            display: None,
        })
    }

    fn parse_root(&mut self) -> Result<Vec<MathNode>> {
        // Skip to the <math> element (or just parse content if no <math> wrapper)
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    if name == "math" {
                        // Check display attribute
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"display" {
                                let val =
                                    String::from_utf8_lossy(&attr.value).to_string();
                                self.display = Some(if val == "block" {
                                    DisplayStyle::Block
                                } else {
                                    DisplayStyle::Inline
                                });
                            }
                        }
                        return self.parse_children("math");
                    } else {
                        // Not wrapped in <math>, parse this element directly
                        let node = self.parse_element_by_name(&name, &e)?;
                        let mut nodes = vec![node];
                        nodes.extend(self.collect_remaining()?);
                        return Ok(nodes);
                    }
                },
                Ok(Event::Eof) => return Ok(vec![]),
                Ok(_) => continue,
                Err(e) => {
                    return Err(Error::MathmlParse {
                        message: format!("XML error: {e}"),
                    });
                },
            }
        }
    }

    fn collect_remaining(&mut self) -> Result<Vec<MathNode>> {
        let mut nodes = Vec::new();
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    nodes.push(self.parse_element_by_name(&name, &e)?);
                },
                Ok(Event::Empty(e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    nodes.push(self.parse_empty_element(&name, &e)?);
                },
                Ok(Event::Text(e)) => {
                    let text = e.unescape().unwrap_or_default().to_string();
                    if !text.trim().is_empty() {
                        nodes.push(classify_text(&text));
                    }
                },
                Ok(Event::Eof) | Ok(Event::End(_)) => break,
                _ => continue,
            }
        }
        Ok(nodes)
    }

    fn parse_children(&mut self, parent_tag: &str) -> Result<Vec<MathNode>> {
        let mut nodes = Vec::new();
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    nodes.push(self.parse_element_by_name(&name, &e)?);
                },
                Ok(Event::Empty(e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    nodes.push(self.parse_empty_element(&name, &e)?);
                },
                Ok(Event::Text(e)) => {
                    let text = e.unescape().unwrap_or_default().to_string();
                    if !text.trim().is_empty() {
                        nodes.push(classify_text(&text));
                    }
                },
                Ok(Event::End(e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    if name == parent_tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                Err(e) => {
                    return Err(Error::MathmlParse {
                        message: format!("XML error in <{parent_tag}>: {e}"),
                    });
                },
                _ => continue,
            }
        }
        Ok(nodes)
    }

    fn parse_child_node(&mut self, parent_tag: &str) -> Result<MathNode> {
        let children = self.parse_children(parent_tag)?;
        Ok(flatten(children))
    }

    fn parse_element_by_name(
        &mut self,
        name: &str,
        start: &quick_xml::events::BytesStart<'_>,
    ) -> Result<MathNode> {
        match name {
            "mn" => {
                let text = self.read_text_content(name)?;
                Ok(MathNode::Number(text))
            },
            "mi" => {
                let mathvariant = get_attr(start, "mathvariant");
                let text = self.read_text_content(name)?;
                if let Some(variant) = mathvariant
                    && let Some(style) = mathvariant_to_style(&variant)
                    && style != TextStyle::Italic
                {
                    return Ok(MathNode::Styled(style, vec![MathNode::Identifier(text)]));
                }
                Ok(MathNode::Identifier(text))
            },
            "mo" => {
                let text = self.read_text_content(name)?;
                let kind = classify_mo(&text);
                Ok(MathNode::Symbol(kind, text))
            },
            "mtext" => {
                let text = self.read_text_content(name)?;
                Ok(MathNode::Text(TextStyle::Normal, text))
            },
            "mspace" => {
                let width = get_attr(start, "width").unwrap_or_default();
                let r = parse_em_width(&width);
                Ok(MathNode::Space(r))
            },
            "mrow" => {
                let children = self.parse_children(name)?;
                Ok(MathNode::Row(children))
            },
            "mfrac" => {
                let children = self.parse_children(name)?;
                let (num, den) = two_children(children)?;
                let kind = if get_attr(start, "linethickness")
                    .as_deref()
                    == Some("0")
                {
                    FractionKind::NoBar
                } else {
                    FractionKind::Normal
                };
                Ok(MathNode::Fraction {
                    kind,
                    num: Box::new(num),
                    den: Box::new(den),
                })
            },
            "msqrt" => {
                let node = self.parse_child_node(name)?;
                Ok(MathNode::Sqrt(Box::new(node)))
            },
            "mroot" => {
                let children = self.parse_children(name)?;
                let (radicand, index) = two_children(children)?;
                Ok(MathNode::Root {
                    index: Box::new(index),
                    radicand: Box::new(radicand),
                })
            },
            "msub" => {
                let children = self.parse_children(name)?;
                let (base, sub) = two_children(children)?;
                Ok(MathNode::Subscript {
                    base: Box::new(base),
                    sub: Box::new(sub),
                })
            },
            "msup" => {
                let children = self.parse_children(name)?;
                let (base, sup) = two_children(children)?;
                Ok(MathNode::Superscript {
                    base: Box::new(base),
                    sup: Box::new(sup),
                })
            },
            "msubsup" => {
                let children = self.parse_children(name)?;
                let (base, sub, sup) = three_children(children)?;
                Ok(MathNode::SubSup {
                    base: Box::new(base),
                    sub: Box::new(sub),
                    sup: Box::new(sup),
                })
            },
            "mover" => {
                let movable = get_attr(start, "accent").as_deref() != Some("true");
                let children = self.parse_children(name)?;
                let (base, over) = two_children(children)?;
                // Check if this is an accent
                if let MathNode::Symbol(_, ref s) = over
                    && let Some(ch) = s.chars().next()
                    && is_combining_accent(ch)
                {
                    return Ok(MathNode::Accent {
                        base: Box::new(base),
                        accent: ch,
                        position: AccentPosition::Above,
                    });
                }
                Ok(MathNode::Overscript {
                    base: Box::new(base),
                    over: Box::new(over),
                    movable_limits: movable,
                })
            },
            "munder" => {
                let movable = get_attr(start, "accentunder").as_deref() != Some("true");
                let children = self.parse_children(name)?;
                let (base, under) = two_children(children)?;
                Ok(MathNode::Underscript {
                    base: Box::new(base),
                    under: Box::new(under),
                    movable_limits: movable,
                })
            },
            "munderover" => {
                let movable = get_attr(start, "accent").as_deref() != Some("true");
                let children = self.parse_children(name)?;
                let (base, under, over) = three_children(children)?;
                Ok(MathNode::UnderOver {
                    base: Box::new(base),
                    under: Box::new(under),
                    over: Box::new(over),
                    movable_limits: movable,
                })
            },
            "mfenced" => {
                let open = get_attr(start, "open").unwrap_or_else(|| "(".into());
                let close = get_attr(start, "close").unwrap_or_else(|| ")".into());
                let children = self.parse_children(name)?;
                let content = children.into_iter().map(FencedItem::Element).collect();
                Ok(MathNode::Fenced {
                    open,
                    close,
                    content,
                })
            },
            "mtable" => {
                let rows = self.parse_table(name)?;
                let columns = if let Some(first_row) = rows.first() {
                    vec![ColumnAlign::Center; first_row.len()]
                } else {
                    vec![]
                };
                Ok(MathNode::Array { columns, rows })
            },
            "mphantom" => {
                let node = self.parse_child_node(name)?;
                Ok(MathNode::Phantom(Box::new(node)))
            },
            "menclose" => {
                let notation = get_attr(start, "notation").unwrap_or_default();
                let node = self.parse_child_node(name)?;
                if notation.contains("box") || notation.contains("roundedbox") {
                    Ok(MathNode::Boxed(Box::new(node)))
                } else if notation.contains("updiagonalstrike") {
                    Ok(MathNode::Cancel {
                        stroke: StrokeKind::Forward,
                        content: Box::new(node),
                    })
                } else if notation.contains("downdiagonalstrike") {
                    Ok(MathNode::Cancel {
                        stroke: StrokeKind::Backward,
                        content: Box::new(node),
                    })
                } else if notation.contains("horizontalstrike") {
                    Ok(MathNode::Cancel {
                        stroke: StrokeKind::Horizontal,
                        content: Box::new(node),
                    })
                } else {
                    Ok(MathNode::Boxed(Box::new(node)))
                }
            },
            "mstyle" => {
                let mathvariant = get_attr(start, "mathvariant");
                let children = self.parse_children(name)?;
                if let Some(variant) = mathvariant
                    && let Some(style) = mathvariant_to_style(&variant)
                {
                    return Ok(MathNode::Styled(style, children));
                }
                Ok(flatten(children))
            },
            "mpadded" => {
                let node = self.parse_child_node(name)?;
                Ok(node)
            },
            "mmultiscripts" => {
                // Parse children, looking for <mprescripts/> separator
                let children = self.parse_children(name)?;
                parse_multiscripts(children)
            },
            "semantics" => {
                // Just parse first child (presentation)
                let children = self.parse_children(name)?;
                Ok(children.into_iter().next().unwrap_or(MathNode::Row(vec![])))
            },
            "annotation" | "annotation-xml" => {
                // Skip annotation content
                let _text = self.read_text_content(name)?;
                Ok(MathNode::Row(vec![]))
            },
            _ => {
                // Unknown element: parse children and wrap in Row
                let children = self.parse_children(name)?;
                Ok(flatten(children))
            },
        }
    }

    fn parse_empty_element(
        &mut self,
        name: &str,
        start: &quick_xml::events::BytesStart<'_>,
    ) -> Result<MathNode> {
        match name {
            "mspace" => {
                let width = get_attr(start, "width").unwrap_or_default();
                let r = parse_em_width(&width);
                Ok(MathNode::Space(r))
            },
            "mprescripts" => {
                // Marker for mmultiscripts - return a sentinel
                Ok(MathNode::Space(Rational::new(0, 1)))
            },
            "none" => Ok(MathNode::Row(vec![])),
            _ => Ok(MathNode::Row(vec![])),
        }
    }

    fn read_text_content(&mut self, tag_name: &str) -> Result<String> {
        let mut text = String::new();
        loop {
            match self.reader.read_event() {
                Ok(Event::Text(e)) => {
                    text.push_str(&e.unescape().unwrap_or_default());
                },
                Ok(Event::End(e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    if name == tag_name {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                Ok(Event::Start(_)) => {
                    // Nested element inside text content - skip it
                    // This handles cases like <mi><mstyle>x</mstyle></mi>
                    continue;
                },
                _ => continue,
            }
        }
        Ok(text.trim().to_string())
    }

    fn parse_table(&mut self, parent_tag: &str) -> Result<Vec<Vec<Vec<MathNode>>>> {
        let mut rows = Vec::new();
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    if name == "mtr" || name == "mlabeledtr" {
                        rows.push(self.parse_table_row(&name)?);
                    }
                },
                Ok(Event::End(e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    if name == parent_tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(rows)
    }

    fn parse_table_row(&mut self, parent_tag: &str) -> Result<Vec<Vec<MathNode>>> {
        let mut cells = Vec::new();
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    if name == "mtd" {
                        cells.push(self.parse_children("mtd")?);
                    }
                },
                Ok(Event::End(e)) => {
                    let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                    if name == parent_tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(cells)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn get_attr(start: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<String> {
    start
        .attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key.as_bytes())
        .map(|a| String::from_utf8_lossy(&a.value).to_string())
}

fn flatten(mut nodes: Vec<MathNode>) -> MathNode {
    if nodes.len() == 1 {
        nodes.remove(0)
    } else {
        MathNode::Row(nodes)
    }
}

fn two_children(mut children: Vec<MathNode>) -> Result<(MathNode, MathNode)> {
    if children.len() < 2 {
        while children.len() < 2 {
            children.push(MathNode::Row(vec![]));
        }
    }
    let mut it = children.into_iter();
    Ok((it.next().unwrap(), it.next().unwrap()))
}

fn three_children(mut children: Vec<MathNode>) -> Result<(MathNode, MathNode, MathNode)> {
    while children.len() < 3 {
        children.push(MathNode::Row(vec![]));
    }
    let mut it = children.into_iter();
    Ok((it.next().unwrap(), it.next().unwrap(), it.next().unwrap()))
}

fn classify_text(text: &str) -> MathNode {
    let trimmed = text.trim();
    if trimmed.chars().all(|c| c.is_ascii_digit() || c == '.') && !trimmed.is_empty() {
        MathNode::Number(trimmed.to_string())
    } else if trimmed.len() == 1 && trimmed.chars().next().unwrap().is_alphabetic() {
        MathNode::Identifier(trimmed.to_string())
    } else {
        MathNode::Text(TextStyle::Normal, trimmed.to_string())
    }
}

fn classify_mo(text: &str) -> SymbolKind {
    let ch = text.trim().chars().next().unwrap_or(' ');
    match ch {
        '+' | '-' | '\u{00B1}' | '\u{2213}' | '\u{00D7}' | '\u{00F7}' | '\u{22C5}'
        | '\u{2217}' | '\u{2218}' | '*' => SymbolKind::BinaryOp,
        '=' | '<' | '>' | '\u{2264}' | '\u{2265}' | '\u{2260}' | '\u{2248}' | '\u{2261}'
        | '\u{2282}' | '\u{2283}' | '\u{2286}' | '\u{2287}' | '\u{2208}' | '\u{2192}'
        | '\u{2190}' | '\u{21D2}' | '\u{21D0}' => SymbolKind::Relation,
        '(' | '[' | '{' | '\u{27E8}' | '\u{230A}' | '\u{2308}' => SymbolKind::Open,
        ')' | ']' | '}' | '\u{27E9}' | '\u{230B}' | '\u{2309}' => SymbolKind::Close,
        ',' | ';' => SymbolKind::Punctuation,
        '\u{2211}' | '\u{220F}' | '\u{2210}' | '\u{222B}' | '\u{222C}' | '\u{222D}'
        | '\u{222E}' | '\u{22C2}' | '\u{22C3}' => SymbolKind::LargeOp,
        '|' | '\u{2016}' => SymbolKind::Fence,
        _ => SymbolKind::Ordinary,
    }
}

fn is_combining_accent(ch: char) -> bool {
    matches!(
        ch,
        '\u{0300}'..='\u{036F}' | '\u{20D0}'..='\u{20FF}'
    ) || ch == '\u{0302}'
        || ch == '\u{0303}'
        || ch == '\u{0304}'
        || ch == '\u{0307}'
        || ch == '\u{0308}'
}

fn parse_em_width(s: &str) -> Rational {
    let s = s.trim();
    if s.ends_with("em") {
        let num_str = s.trim_end_matches("em").trim();
        if let Ok(f) = num_str.parse::<f64>() {
            // Approximate as rational
            let scaled = (f * 18.0).round() as i32;
            return Rational::new(scaled, 18);
        }
    }
    Rational::zero()
}

fn mathvariant_to_style(variant: &str) -> Option<TextStyle> {
    match variant {
        "normal" => Some(TextStyle::Normal),
        "bold" => Some(TextStyle::Bold),
        "italic" => Some(TextStyle::Italic),
        "bold-italic" => Some(TextStyle::BoldItalic),
        "double-struck" => Some(TextStyle::DoubleStruck),
        "script" => Some(TextStyle::Script),
        "bold-script" => Some(TextStyle::BoldScript),
        "fraktur" => Some(TextStyle::Fraktur),
        "bold-fraktur" => Some(TextStyle::BoldFraktur),
        "sans-serif" => Some(TextStyle::SansSerif),
        "bold-sans-serif" => Some(TextStyle::SansSerifBold),
        "sans-serif-italic" => Some(TextStyle::SansSerifItalic),
        "sans-serif-bold-italic" => Some(TextStyle::SansSerifBoldItalic),
        "monospace" => Some(TextStyle::Monospace),
        _ => None,
    }
}

fn parse_multiscripts(children: Vec<MathNode>) -> Result<MathNode> {
    // mmultiscripts format: base postscripts... <mprescripts/> prescripts...
    // where scripts come in pairs (sub, sup), using <none/> for absent scripts
    let mut iter = children.into_iter();
    let base = iter.next().unwrap_or(MathNode::Row(vec![]));

    // For simplicity, just handle the pre-scripts case
    // Skip to prescripts marker (Space(0/1) is our sentinel)
    let mut found_pre = false;
    let mut remaining = Vec::new();
    for node in iter {
        if matches!(node, MathNode::Space(r) if r.num == 0 && r.den == 1) {
            found_pre = true;
            continue;
        }
        remaining.push(node);
    }

    if found_pre && remaining.len() >= 2 {
        let pre_sub = remaining.first().cloned();
        let pre_sup = remaining.get(1).cloned();
        let is_empty = |n: &MathNode| matches!(n, MathNode::Row(v) if v.is_empty());
        Ok(MathNode::PreScripts {
            base: Box::new(base),
            pre_sub: pre_sub.filter(|n| !is_empty(n)).map(Box::new),
            pre_sup: pre_sup.filter(|n| !is_empty(n)).map(Box::new),
        })
    } else {
        Ok(base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Vec<MathNode> {
        let config = Config::default();
        MathmlReader::parse(input, &config).unwrap().expr.nodes
    }

    fn parse_one(input: &str) -> MathNode {
        let nodes = parse(input);
        if nodes.len() == 1 {
            nodes.into_iter().next().unwrap()
        } else {
            MathNode::Row(nodes)
        }
    }

    #[test]
    fn simple_number() {
        let node = parse_one("<math><mn>42</mn></math>");
        assert_eq!(node, MathNode::Number("42".into()));
    }

    #[test]
    fn simple_identifier() {
        let node = parse_one("<math><mi>x</mi></math>");
        assert_eq!(node, MathNode::Identifier("x".into()));
    }

    #[test]
    fn simple_operator() {
        let node = parse_one("<math><mo>+</mo></math>");
        assert_eq!(node, MathNode::Symbol(SymbolKind::BinaryOp, "+".into()));
    }

    #[test]
    fn fraction() {
        let node = parse_one("<math><mfrac><mi>a</mi><mi>b</mi></mfrac></math>");
        assert!(matches!(node, MathNode::Fraction { .. }));
    }

    #[test]
    fn sqrt() {
        let node = parse_one("<math><msqrt><mi>x</mi></msqrt></math>");
        assert!(matches!(node, MathNode::Sqrt(_)));
    }

    #[test]
    fn subscript() {
        let node = parse_one("<math><msub><mi>x</mi><mn>2</mn></msub></math>");
        assert!(matches!(node, MathNode::Subscript { .. }));
    }

    #[test]
    fn superscript() {
        let node = parse_one("<math><msup><mi>x</mi><mn>2</mn></msup></math>");
        assert!(matches!(node, MathNode::Superscript { .. }));
    }

    #[test]
    fn subsup() {
        let node =
            parse_one("<math><msubsup><mi>x</mi><mi>i</mi><mn>2</mn></msubsup></math>");
        assert!(matches!(node, MathNode::SubSup { .. }));
    }

    #[test]
    fn mrow() {
        let node = parse_one(
            "<math><mrow><mi>a</mi><mo>+</mo><mi>b</mi></mrow></math>",
        );
        assert!(matches!(node, MathNode::Row(_)));
    }

    #[test]
    fn mtext() {
        let node = parse_one("<math><mtext>hello</mtext></math>");
        assert_eq!(
            node,
            MathNode::Text(TextStyle::Normal, "hello".into())
        );
    }

    #[test]
    fn mfenced() {
        let node = parse_one(
            "<math><mfenced open=\"(\" close=\")\"><mi>x</mi></mfenced></math>",
        );
        assert!(matches!(node, MathNode::Fenced { .. }));
    }

    #[test]
    fn display_block() {
        let config = Config::default();
        let result = MathmlReader::parse(
            "<math display=\"block\"><mn>1</mn></math>",
            &config,
        )
        .unwrap();
        assert_eq!(result.expr.display, DisplayStyle::Block);
    }

    #[test]
    fn empty_input() {
        let config = Config::default();
        let result = MathmlReader::parse("", &config).unwrap();
        assert!(result.expr.nodes.is_empty());
    }

    #[test]
    fn table() {
        let input = "<math><mtable><mtr><mtd><mn>1</mn></mtd><mtd><mn>2</mn></mtd></mtr><mtr><mtd><mn>3</mn></mtd><mtd><mn>4</mn></mtd></mtr></mtable></math>";
        let node = parse_one(input);
        if let MathNode::Array { rows, .. } = &node {
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].len(), 2);
        } else {
            panic!("expected Array");
        }
    }

    #[test]
    fn phantom() {
        let node = parse_one("<math><mphantom><mi>x</mi></mphantom></math>");
        assert!(matches!(node, MathNode::Phantom(_)));
    }

    #[test]
    fn mover_accent() {
        let node = parse_one(
            "<math><mover accent=\"true\"><mi>x</mi><mo>\u{0302}</mo></mover></math>",
        );
        assert!(matches!(node, MathNode::Accent { .. }));
    }
}
