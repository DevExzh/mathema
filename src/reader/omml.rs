use quick_xml::Reader;
use quick_xml::events::Event;

use crate::ast::*;
use crate::error::{Config, Diagnostic, DiagnosticKind, Error, ParseResult, Result, Strictness};
use crate::reader::Reader as MathReader;

pub struct OmmlReader;

impl MathReader for OmmlReader {
    fn parse(input: &str, config: &Config) -> Result<ParseResult> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(ParseResult::new(MathExpr::new(vec![], config.display)));
        }
        let parser_result = OmmlParser::new(trimmed);
        let mut parser = match parser_result {
            Ok(p) => p,
            Err(e) => {
                if config.strictness == Strictness::Lenient {
                    return Ok(ParseResult::with_diagnostics(
                        MathExpr::new(vec![], config.display),
                        vec![Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: format!("OMML initialization failed (recovered): {e}"),
                            span: None,
                        }],
                    ));
                }
                return Err(e);
            }
        };
        match parser.parse_root() {
            Ok(nodes) => Ok(ParseResult::new(MathExpr::new(nodes, config.display))),
            Err(e) => {
                if config.strictness == Strictness::Lenient {
                    Ok(ParseResult::with_diagnostics(
                        MathExpr::new(vec![], config.display),
                        vec![Diagnostic {
                            kind: DiagnosticKind::Warning,
                            message: format!("OMML parse error (recovered): {e}"),
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

struct OmmlParser<'a> {
    reader: Reader<&'a [u8]>,
}

impl<'a> OmmlParser<'a> {
    fn new(input: &'a str) -> Result<Self> {
        let mut reader = Reader::from_str(input);
        reader.config_mut().trim_text(true);
        Ok(Self { reader })
    }

    fn parse_root(&mut self) -> Result<Vec<MathNode>> {
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    if name == "oMathPara" || name == "oMath" {
                        return self.parse_children(&name);
                    } else {
                        let node = self.parse_element_by_name(&name, &e)?;
                        let mut nodes = vec![node];
                        nodes.extend(self.collect_remaining()?);
                        return Ok(nodes);
                    }
                },
                Ok(Event::Eof) => return Ok(vec![]),
                Ok(_) => continue,
                Err(e) => {
                    return Err(Error::OmmlParse {
                        message: format!("OMML XML error: {e}"),
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
                    let name = local_name(&e);
                    nodes.push(self.parse_element_by_name(&name, &e)?);
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
                    let name = local_name(&e);
                    let node = self.parse_element_by_name(&name, &e)?;
                    // Filter out empty rows from property elements
                    if !is_empty_row(&node) {
                        nodes.push(node);
                    }
                },
                Ok(Event::Empty(_)) => {},
                Ok(Event::Text(e)) => {
                    let text = e.unescape().unwrap_or_default().to_string();
                    if !text.trim().is_empty() {
                        nodes.push(classify_run_text(text.trim()));
                    }
                },
                Ok(Event::End(e)) => {
                    let name = local_name_end(&e);
                    if name == parent_tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                Err(e) => {
                    return Err(Error::OmmlParse {
                        message: format!("OMML XML error in <{parent_tag}>: {e}"),
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
        _start: &quick_xml::events::BytesStart<'_>,
    ) -> Result<MathNode> {
        match name {
            // Run: <m:r><m:t>text</m:t></m:r>
            "r" => self.parse_run(),

            // Fraction: <m:f><m:fPr/><m:num>...</m:num><m:den>...</m:den></m:f>
            "f" => self.parse_fraction(),

            // Radical: <m:rad><m:radPr/><m:deg>...</m:deg><m:e>...</m:e></m:rad>
            "rad" => self.parse_radical(),

            // Subscript
            "sSub" => self.parse_ssub(),

            // Superscript
            "sSup" => self.parse_ssup(),

            // Sub+superscript
            "sSubSup" => self.parse_ssubsup(),

            // Pre-scripts
            "sPre" => self.parse_spre(),

            // N-ary
            "nary" => self.parse_nary(),

            // Accent
            "acc" => self.parse_accent(),

            // Bar (over/under)
            "bar" => self.parse_bar(),

            // Delimiter/fence
            "d" => self.parse_delimiter(),

            // Matrix
            "m" => self.parse_matrix(),

            // Equation array
            "eqArr" => self.parse_eq_array(),

            // Function
            "func" => self.parse_func(),

            // Lower limit
            "limLow" => self.parse_lim_low(),

            // Upper limit
            "limUpp" => self.parse_lim_upp(),

            // Group character
            "groupChr" => self.parse_group_chr(),

            // Box
            "box" | "boxedFormula" => {
                let node = self.parse_child_node(name)?;
                Ok(node)
            },

            // Border box
            "borderBox" => {
                let node = self.parse_child_node(name)?;
                Ok(MathNode::Boxed(Box::new(node)))
            },

            // Phantom
            "phant" => {
                let node = self.parse_child_node(name)?;
                Ok(MathNode::Phantom(Box::new(node)))
            },

            // Math paragraph / math zone - recurse
            "oMath" | "oMathPara" => {
                let children = self.parse_children(name)?;
                Ok(flatten(children))
            },

            // Property elements - skip contents, return empty
            "fPr" | "radPr" | "sSubPr" | "sSupPr" | "sSubSupPr" | "sPrePr" | "naryPr"
            | "accPr" | "barPr" | "dPr" | "mPr" | "eqArrPr" | "funcPr" | "limLowPr"
            | "limUppPr" | "groupChrPr" | "boxPr" | "borderBoxPr" | "phantPr" | "rPr"
            | "ctrlPr" | "mathPr" => {
                self.skip_element(name)?;
                Ok(MathNode::Row(vec![]))
            },

            // Content containers
            "e" | "num" | "den" | "deg" | "sub" | "sup" | "lim" | "fName" => {
                let children = self.parse_children(name)?;
                Ok(flatten(children))
            },

            // Matrix row
            "mr" => {
                // Should be handled by parse_matrix
                let children = self.parse_children(name)?;
                Ok(flatten(children))
            },

            _ => {
                // Unknown element: parse children
                let children = self.parse_children(name)?;
                Ok(flatten(children))
            },
        }
    }

    // --- Specific element parsers ---

    fn parse_run(&mut self) -> Result<MathNode> {
        let mut text = String::new();
        let mut style = TextStyle::Normal;
        let tag = "r";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "t" => {
                            text = self.read_text_content("t")?;
                        },
                        "rPr" => {
                            style = self.parse_run_properties()?;
                        },
                        _ => {
                            self.skip_element(&name)?;
                        },
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(classify_run_text_styled(text.trim(), style))
    }

    fn parse_run_properties(&mut self) -> Result<TextStyle> {
        let mut sty = "p"; // default: plain
        let mut scr = "roman"; // default: roman
        let tag = "rPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = local_name(&e);
                    if name == "sty" && let Some(val) = get_val_attr(&e) {
                        sty = match val.as_str() {
                            "p" => "p",
                            "i" => "i",
                            "b" => "b",
                            "bi" => "bi",
                            _ => "p",
                        };
                    } else if name == "scr" && let Some(val) = get_val_attr(&e) {
                        scr = match val.as_str() {
                            "roman" => "roman",
                            "script" => "script",
                            "fraktur" => "fraktur",
                            "double-struck" => "double-struck",
                            "sans-serif" => "sans-serif",
                            "monospace" => "monospace",
                            _ => "roman",
                        };
                    }
                    if matches!(self.reader.read_event(), Ok(Event::Start(_))) {
                        // Skip nested content
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(combine_omml_style(sty, scr))
    }

    fn parse_fraction(&mut self) -> Result<MathNode> {
        let mut kind = FractionKind::Normal;
        let mut num = MathNode::Row(vec![]);
        let mut den = MathNode::Row(vec![]);
        let tag = "f";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "fPr" => kind = self.parse_fraction_props()?,
                        "num" => num = self.parse_child_node("num")?,
                        "den" => den = self.parse_child_node("den")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(MathNode::Fraction {
            kind,
            num: Box::new(num),
            den: Box::new(den),
        })
    }

    fn parse_fraction_props(&mut self) -> Result<FractionKind> {
        let mut kind = FractionKind::Normal;
        let tag = "fPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = local_name(&e);
                    if name == "type" && let Some(val) = get_val_attr(&e) {
                        kind = match val.as_str() {
                            "bar" => FractionKind::Normal,
                            "noBar" => FractionKind::NoBar,
                            "skw" => FractionKind::Skewed,
                            "lin" => FractionKind::Inline,
                            _ => FractionKind::Normal,
                        };
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(kind)
    }

    fn parse_radical(&mut self) -> Result<MathNode> {
        let mut deg_hide = false;
        let mut deg = MathNode::Row(vec![]);
        let mut content = MathNode::Row(vec![]);
        let tag = "rad";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "radPr" => deg_hide = self.parse_radical_props()?,
                        "deg" => deg = self.parse_child_node("deg")?,
                        "e" => content = self.parse_child_node("e")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        if deg_hide || is_empty_row(&deg) {
            Ok(MathNode::Sqrt(Box::new(content)))
        } else {
            Ok(MathNode::Root {
                index: Box::new(deg),
                radicand: Box::new(content),
            })
        }
    }

    fn parse_radical_props(&mut self) -> Result<bool> {
        let mut deg_hide = false;
        let tag = "radPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = local_name(&e);
                    if name == "degHide" && let Some(val) = get_val_attr(&e) {
                        deg_hide = val == "1" || val == "on" || val == "true";
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(deg_hide)
    }

    fn parse_ssub(&mut self) -> Result<MathNode> {
        let (base, sub, _) = self.parse_script_element("sSub")?;
        Ok(MathNode::Subscript {
            base: Box::new(base),
            sub: Box::new(sub),
        })
    }

    fn parse_ssup(&mut self) -> Result<MathNode> {
        let (base, _, sup) = self.parse_script_element("sSup")?;
        Ok(MathNode::Superscript {
            base: Box::new(base),
            sup: Box::new(sup),
        })
    }

    fn parse_ssubsup(&mut self) -> Result<MathNode> {
        let (base, sub, sup) = self.parse_script_element("sSubSup")?;
        Ok(MathNode::SubSup {
            base: Box::new(base),
            sub: Box::new(sub),
            sup: Box::new(sup),
        })
    }

    fn parse_spre(&mut self) -> Result<MathNode> {
        let (base, pre_sub, pre_sup) = self.parse_script_element("sPre")?;
        let is_empty = |n: &MathNode| matches!(n, MathNode::Row(v) if v.is_empty());
        Ok(MathNode::PreScripts {
            base: Box::new(base),
            pre_sub: if is_empty(&pre_sub) {
                None
            } else {
                Some(Box::new(pre_sub))
            },
            pre_sup: if is_empty(&pre_sup) {
                None
            } else {
                Some(Box::new(pre_sup))
            },
        })
    }

    fn parse_script_element(
        &mut self,
        tag: &str,
    ) -> Result<(MathNode, MathNode, MathNode)> {
        let mut base = MathNode::Row(vec![]);
        let mut sub = MathNode::Row(vec![]);
        let mut sup = MathNode::Row(vec![]);
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "e" => base = self.parse_child_node("e")?,
                        "sub" => sub = self.parse_child_node("sub")?,
                        "sup" => sup = self.parse_child_node("sup")?,
                        n if n.ends_with("Pr") => self.skip_element(&name)?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok((base, sub, sup))
    }

    fn parse_nary(&mut self) -> Result<MathNode> {
        let mut op = '\u{222B}'; // default: integral
        let mut limits = LimitStyle::Default;
        let mut sub_hide = false;
        let mut sup_hide = false;
        let mut sub = MathNode::Row(vec![]);
        let mut sup = MathNode::Row(vec![]);
        let mut body = MathNode::Row(vec![]);
        let tag = "nary";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "naryPr" => {
                            let props = self.parse_nary_props()?;
                            op = props.0;
                            limits = props.1;
                            sub_hide = props.2;
                            sup_hide = props.3;
                        },
                        "sub" => sub = self.parse_child_node("sub")?,
                        "sup" => sup = self.parse_child_node("sup")?,
                        "e" => body = self.parse_child_node("e")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(MathNode::Nary {
            op,
            sub: if sub_hide || is_empty_row(&sub) {
                None
            } else {
                Some(Box::new(sub))
            },
            sup: if sup_hide || is_empty_row(&sup) {
                None
            } else {
                Some(Box::new(sup))
            },
            body: Box::new(body),
            limits,
        })
    }

    fn parse_nary_props(&mut self) -> Result<(char, LimitStyle, bool, bool)> {
        let mut chr = '\u{222B}';
        let mut limits = LimitStyle::Default;
        let mut sub_hide = false;
        let mut sup_hide = false;
        let tag = "naryPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "chr" => {
                            if let Some(val) = get_val_attr(&e)
                                && let Some(c) = val.chars().next()
                            {
                                chr = c;
                            }
                        },
                        "limLoc" => {
                            if let Some(val) = get_val_attr(&e) {
                                limits = match val.as_str() {
                                    "undOvr" => LimitStyle::AboveBelow,
                                    "subSup" => LimitStyle::SubSup,
                                    _ => LimitStyle::Default,
                                };
                            }
                        },
                        "subHide" => {
                            if let Some(val) = get_val_attr(&e) {
                                sub_hide = val == "1" || val == "on" || val == "true";
                            }
                        },
                        "supHide" => {
                            if let Some(val) = get_val_attr(&e) {
                                sup_hide = val == "1" || val == "on" || val == "true";
                            }
                        },
                        _ => {},
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok((chr, limits, sub_hide, sup_hide))
    }

    fn parse_accent(&mut self) -> Result<MathNode> {
        let mut accent_char = '\u{0302}'; // default: hat
        let mut base = MathNode::Row(vec![]);
        let tag = "acc";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "accPr" => accent_char = self.parse_accent_props()?,
                        "e" => base = self.parse_child_node("e")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(MathNode::Accent {
            base: Box::new(base),
            accent: accent_char,
            position: AccentPosition::Above,
        })
    }

    fn parse_accent_props(&mut self) -> Result<char> {
        let mut chr = '\u{0302}';
        let tag = "accPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = local_name(&e);
                    if name == "chr"
                        && let Some(val) = get_val_attr(&e)
                        && let Some(c) = val.chars().next()
                    {
                        chr = c;
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(chr)
    }

    fn parse_bar(&mut self) -> Result<MathNode> {
        let mut pos_top = true;
        let mut base = MathNode::Row(vec![]);
        let tag = "bar";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "barPr" => pos_top = self.parse_bar_props()?,
                        "e" => base = self.parse_child_node("e")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        let line = MathNode::Symbol(SymbolKind::Ordinary, "\u{203E}".into());
        if pos_top {
            Ok(MathNode::Overscript {
                base: Box::new(base),
                over: Box::new(line),
                movable_limits: false,
            })
        } else {
            Ok(MathNode::Underscript {
                base: Box::new(base),
                under: Box::new(line),
                movable_limits: false,
            })
        }
    }

    fn parse_bar_props(&mut self) -> Result<bool> {
        let mut top = true;
        let tag = "barPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = local_name(&e);
                    if name == "pos" && let Some(val) = get_val_attr(&e) {
                        top = val != "bot";
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(top)
    }

    fn parse_delimiter(&mut self) -> Result<MathNode> {
        let mut open = "(".to_string();
        let mut close = ")".to_string();
        let mut elements = Vec::new();
        let tag = "d";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "dPr" => {
                            let (o, c) = self.parse_delimiter_props()?;
                            open = o;
                            close = c;
                        },
                        "e" => {
                            let node = self.parse_child_node("e")?;
                            elements.push(FencedItem::Element(node));
                        },
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(MathNode::Fenced {
            open,
            close,
            content: elements,
        })
    }

    fn parse_delimiter_props(&mut self) -> Result<(String, String)> {
        let mut open = "(".to_string();
        let mut close = ")".to_string();
        let tag = "dPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "begChr" => {
                            if let Some(val) = get_val_attr(&e) {
                                open = val;
                            }
                        },
                        "endChr" => {
                            if let Some(val) = get_val_attr(&e) {
                                close = val;
                            }
                        },
                        _ => {},
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok((open, close))
    }

    fn parse_matrix(&mut self) -> Result<MathNode> {
        let mut rows: Vec<Vec<Vec<MathNode>>> = Vec::new();
        let tag = "m";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "mr" => {
                            let row = self.parse_matrix_row()?;
                            rows.push(row);
                        },
                        "mPr" => self.skip_element("mPr")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        let columns = if let Some(first) = rows.first() {
            vec![ColumnAlign::Center; first.len()]
        } else {
            vec![]
        };
        Ok(MathNode::Array { columns, rows })
    }

    fn parse_matrix_row(&mut self) -> Result<Vec<Vec<MathNode>>> {
        let mut cells = Vec::new();
        let tag = "mr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    if name == "e" {
                        let children = self.parse_children("e")?;
                        cells.push(children);
                    } else {
                        self.skip_element(&name)?;
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(cells)
    }

    fn parse_eq_array(&mut self) -> Result<MathNode> {
        let mut rows: Vec<Vec<Vec<MathNode>>> = Vec::new();
        let tag = "eqArr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "e" => {
                            let children = self.parse_children("e")?;
                            rows.push(vec![children]);
                        },
                        "eqArrPr" => self.skip_element("eqArrPr")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        let columns = vec![ColumnAlign::Left];
        Ok(MathNode::Array { columns, rows })
    }

    fn parse_func(&mut self) -> Result<MathNode> {
        let mut fname = MathNode::Row(vec![]);
        let mut arg = MathNode::Row(vec![]);
        let tag = "func";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "fName" => fname = self.parse_child_node("fName")?,
                        "e" => arg = self.parse_child_node("e")?,
                        "funcPr" => self.skip_element("funcPr")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        // The function name might be a run with text like "sin"
        // Convert it to an Operator node if possible
        let op = extract_operator_name(&fname);
        if let Some(name) = op {
            Ok(MathNode::Row(vec![
                MathNode::Operator(OperatorAttr {
                    name,
                    limit_style: LimitStyle::SubSup,
                }),
                arg,
            ]))
        } else {
            Ok(MathNode::Row(vec![fname, arg]))
        }
    }

    fn parse_lim_low(&mut self) -> Result<MathNode> {
        let mut base = MathNode::Row(vec![]);
        let mut lim = MathNode::Row(vec![]);
        let tag = "limLow";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "e" => base = self.parse_child_node("e")?,
                        "lim" => lim = self.parse_child_node("lim")?,
                        "limLowPr" => self.skip_element("limLowPr")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(MathNode::Underscript {
            base: Box::new(base),
            under: Box::new(lim),
            movable_limits: true,
        })
    }

    fn parse_lim_upp(&mut self) -> Result<MathNode> {
        let mut base = MathNode::Row(vec![]);
        let mut lim = MathNode::Row(vec![]);
        let tag = "limUpp";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "e" => base = self.parse_child_node("e")?,
                        "lim" => lim = self.parse_child_node("lim")?,
                        "limUppPr" => self.skip_element("limUppPr")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(MathNode::Overscript {
            base: Box::new(base),
            over: Box::new(lim),
            movable_limits: true,
        })
    }

    fn parse_group_chr(&mut self) -> Result<MathNode> {
        let mut chr = '\u{23DF}'; // default: bottom brace
        let mut pos_top = false;
        let mut base = MathNode::Row(vec![]);
        let tag = "groupChr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "groupChrPr" => {
                            let (c, t) = self.parse_group_chr_props()?;
                            chr = c;
                            pos_top = t;
                        },
                        "e" => base = self.parse_child_node("e")?,
                        _ => self.skip_element(&name)?,
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        let sym = MathNode::Symbol(SymbolKind::Ordinary, chr.to_string());
        if pos_top {
            Ok(MathNode::Overscript {
                base: Box::new(base),
                over: Box::new(sym),
                movable_limits: false,
            })
        } else {
            Ok(MathNode::Underscript {
                base: Box::new(base),
                under: Box::new(sym),
                movable_limits: false,
            })
        }
    }

    fn parse_group_chr_props(&mut self) -> Result<(char, bool)> {
        let mut chr = '\u{23DF}';
        let mut top = false;
        let tag = "groupChrPr";
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                    let name = local_name(&e);
                    match name.as_str() {
                        "chr" => {
                            if let Some(val) = get_val_attr(&e)
                                && let Some(c) = val.chars().next()
                            {
                                chr = c;
                            }
                        },
                        "pos" => {
                            if let Some(val) = get_val_attr(&e) {
                                top = val == "top";
                            }
                        },
                        _ => {},
                    }
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok((chr, top))
    }

    fn read_text_content(&mut self, tag_name: &str) -> Result<String> {
        let mut text = String::new();
        loop {
            match self.reader.read_event() {
                Ok(Event::Text(e)) => {
                    text.push_str(&e.unescape().unwrap_or_default());
                },
                Ok(Event::End(e)) => {
                    if local_name_end(&e) == tag_name {
                        break;
                    }
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(text)
    }

    fn skip_element(&mut self, _tag_name: &str) -> Result<()> {
        let mut depth = 1u32;
        loop {
            match self.reader.read_event() {
                Ok(Event::Start(_)) => depth += 1,
                Ok(Event::End(e)) => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    // Also break if this ends our tag at depth 0
                    let _ = e;
                },
                Ok(Event::Eof) => break,
                _ => continue,
            }
        }
        Ok(())
    }
}

// --- Helpers ---

fn local_name(e: &quick_xml::events::BytesStart<'_>) -> String {
    let full = String::from_utf8_lossy(e.name().as_ref()).to_string();
    strip_ns_prefix(&full)
}

fn local_name_end(e: &quick_xml::events::BytesEnd<'_>) -> String {
    let full = String::from_utf8_lossy(e.name().as_ref()).to_string();
    strip_ns_prefix(&full)
}

fn strip_ns_prefix(name: &str) -> String {
    if let Some(pos) = name.find(':') {
        name[pos + 1..].to_string()
    } else {
        name.to_string()
    }
}

fn get_val_attr(e: &quick_xml::events::BytesStart<'_>) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| {
            let key = String::from_utf8_lossy(a.key.as_ref());
            key == "m:val" || key == "val" || key == "w:val"
        })
        .map(|a| String::from_utf8_lossy(&a.value).to_string())
}

fn flatten(mut nodes: Vec<MathNode>) -> MathNode {
    if nodes.len() == 1 {
        nodes.remove(0)
    } else {
        MathNode::Row(nodes)
    }
}

fn is_empty_row(node: &MathNode) -> bool {
    matches!(node, MathNode::Row(v) if v.is_empty())
}

fn classify_run_text(text: &str) -> MathNode {
    classify_run_text_styled(text, TextStyle::Normal)
}

fn classify_run_text_styled(text: &str, style: TextStyle) -> MathNode {
    if text.is_empty() {
        return MathNode::Row(vec![]);
    }
    // Check if it's a number
    if text
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
        && text.chars().any(|c| c.is_ascii_digit())
    {
        return MathNode::Number(text.to_string());
    }
    // Single character
    if text.chars().count() == 1 {
        let ch = text.chars().next().unwrap();
        return match classify_omml_char(ch) {
            CharClass::Letter => {
                if style != TextStyle::Normal && style != TextStyle::Italic {
                    MathNode::Styled(style, vec![MathNode::Identifier(text.to_string())])
                } else {
                    MathNode::Identifier(text.to_string())
                }
            },
            CharClass::Operator => MathNode::Symbol(classify_op_char(ch), text.to_string()),
            CharClass::Digit => MathNode::Number(text.to_string()),
        };
    }
    // Multi-character: likely an operator name or text
    if crate::unicode::symbols::is_operator_command(text) {
        return MathNode::Operator(OperatorAttr {
            name: text.to_string(),
            limit_style: if crate::unicode::symbols::operator_has_limits(text) {
                LimitStyle::Default
            } else {
                LimitStyle::SubSup
            },
        });
    }
    if style != TextStyle::Normal {
        MathNode::Styled(style, vec![MathNode::Text(TextStyle::Normal, text.to_string())])
    } else {
        MathNode::Text(TextStyle::Normal, text.to_string())
    }
}

enum CharClass {
    Letter,
    Operator,
    Digit,
}

fn classify_omml_char(ch: char) -> CharClass {
    if ch.is_ascii_digit() {
        CharClass::Digit
    } else if ch.is_alphabetic() {
        CharClass::Letter
    } else {
        CharClass::Operator
    }
}

fn classify_op_char(ch: char) -> SymbolKind {
    match ch {
        '+' | '-' | '*' | '\u{00D7}' | '\u{00F7}' | '\u{22C5}' => SymbolKind::BinaryOp,
        '=' | '<' | '>' | '\u{2264}' | '\u{2265}' | '\u{2260}' => SymbolKind::Relation,
        '(' | '[' | '{' | '\u{27E8}' => SymbolKind::Open,
        ')' | ']' | '}' | '\u{27E9}' => SymbolKind::Close,
        ',' | ';' => SymbolKind::Punctuation,
        '|' | '\u{2016}' => SymbolKind::Fence,
        '\u{2211}' | '\u{220F}' | '\u{222B}' => SymbolKind::LargeOp,
        _ => SymbolKind::Ordinary,
    }
}

/// Combine OMML `sty` (p/i/b/bi) and `scr` (roman/script/fraktur/...) into a TextStyle.
fn combine_omml_style(sty: &str, scr: &str) -> TextStyle {
    match scr {
        "script" => match sty {
            "b" | "bi" => TextStyle::BoldScript,
            _ => TextStyle::Script,
        },
        "fraktur" => match sty {
            "b" | "bi" => TextStyle::BoldFraktur,
            _ => TextStyle::Fraktur,
        },
        "double-struck" => TextStyle::DoubleStruck,
        "sans-serif" => match sty {
            "b" => TextStyle::SansSerifBold,
            "i" => TextStyle::SansSerifItalic,
            "bi" => TextStyle::SansSerifBoldItalic,
            _ => TextStyle::SansSerif,
        },
        "monospace" => TextStyle::Monospace,
        _ => match sty {
            "i" => TextStyle::Italic,
            "b" => TextStyle::Bold,
            "bi" => TextStyle::BoldItalic,
            _ => TextStyle::Normal,
        },
    }
}

fn extract_operator_name(node: &MathNode) -> Option<String> {
    match node {
        MathNode::Operator(attr) => Some(attr.name.clone()),
        MathNode::Text(_, s) => {
            if crate::unicode::symbols::is_operator_command(s.trim()) {
                Some(s.trim().to_string())
            } else {
                None
            }
        },
        MathNode::Row(children) if children.len() == 1 => extract_operator_name(&children[0]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Vec<MathNode> {
        let config = Config::default();
        OmmlReader::parse(input, &config).unwrap().expr.nodes
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
    fn simple_run_number() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r><m:t>42</m:t></m:r></m:oMath>"#;
        let node = parse_one(input);
        assert_eq!(node, MathNode::Number("42".into()));
    }

    #[test]
    fn simple_run_identifier() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:r><m:t>x</m:t></m:r></m:oMath>"#;
        let node = parse_one(input);
        assert_eq!(node, MathNode::Identifier("x".into()));
    }

    #[test]
    fn fraction() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:f><m:num><m:r><m:t>a</m:t></m:r></m:num><m:den><m:r><m:t>b</m:t></m:r></m:den></m:f></m:oMath>"#;
        let node = parse_one(input);
        assert!(matches!(node, MathNode::Fraction { kind: FractionKind::Normal, .. }));
    }

    #[test]
    fn subscript() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:sSub><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sub><m:r><m:t>i</m:t></m:r></m:sub></m:sSub></m:oMath>"#;
        let node = parse_one(input);
        assert!(matches!(node, MathNode::Subscript { .. }));
    }

    #[test]
    fn superscript() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:sSup><m:e><m:r><m:t>x</m:t></m:r></m:e><m:sup><m:r><m:t>2</m:t></m:r></m:sup></m:sSup></m:oMath>"#;
        let node = parse_one(input);
        assert!(matches!(node, MathNode::Superscript { .. }));
    }

    #[test]
    fn sqrt() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:rad><m:radPr><m:degHide m:val="1"/></m:radPr><m:deg/><m:e><m:r><m:t>x</m:t></m:r></m:e></m:rad></m:oMath>"#;
        let node = parse_one(input);
        assert!(matches!(node, MathNode::Sqrt(_)));
    }

    #[test]
    fn delimiter() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:d><m:dPr><m:begChr m:val="("/><m:endChr m:val=")"/></m:dPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:d></m:oMath>"#;
        let node = parse_one(input);
        assert!(matches!(node, MathNode::Fenced { .. }));
    }

    #[test]
    fn accent() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:acc><m:accPr><m:chr m:val="̂"/></m:accPr><m:e><m:r><m:t>x</m:t></m:r></m:e></m:acc></m:oMath>"#;
        let node = parse_one(input);
        assert!(matches!(node, MathNode::Accent { .. }));
    }

    #[test]
    fn empty_input() {
        let config = Config::default();
        let result = OmmlReader::parse("", &config).unwrap();
        assert!(result.expr.nodes.is_empty());
    }

    #[test]
    fn border_box() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:borderBox><m:e><m:r><m:t>x</m:t></m:r></m:e></m:borderBox></m:oMath>"#;
        let node = parse_one(input);
        assert!(matches!(node, MathNode::Boxed(_)));
    }

    #[test]
    fn phantom() {
        let input = r#"<m:oMath xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math"><m:phant><m:e><m:r><m:t>x</m:t></m:r></m:e></m:phant></m:oMath>"#;
        let node = parse_one(input);
        assert!(matches!(node, MathNode::Phantom(_)));
    }
}
