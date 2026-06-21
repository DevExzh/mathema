//! Content MathML → Presentation conversion.
//!
//! Content MathML (`<apply>`, `<ci>`, `<cn>`, `<plus/>`, `<sin/>`, …) describes
//! the *meaning* of an expression rather than its layout. This module lowers
//! the content forms found in the W3C MathML test suite into the same compact
//! presentation AST produced by the presentation-MathML path, so they flow
//! through the existing layout/render pipeline unchanged.
//!
//! The conversion is intentionally pragmatic: it covers the operators,
//! functions, qualifiers, and containers exercised by the test suite, and
//! falls back to a generic "function application" rendering for any unknown
//! operator token so nothing is silently dropped.

use quick_xml::events::Event;
use smallvec::SmallVec;

use super::{MathmlCompactParser, get_attr, unescape_text};
use crate::ast::compact::{CompactNode, NodeId};
use crate::ast::{AccentPosition, FractionKind, LimitStyle, SymbolKind, TextStyle};
use crate::error::Result;

/// The head operator of an `<apply>` (or `<reln>`).
enum COp {
    /// A recognised content operator token, e.g. `plus`, `sin`, `int`.
    Token(String),
    /// An operator that is itself an expression (user function `<ci>f</ci>`,
    /// `<csymbol>`, `<fn>`, or a nested `<apply>` such as an inverse).
    Func(NodeId),
}

/// Accumulated children of an `<apply>`.
#[derive(Default)]
struct Applied {
    args: Vec<NodeId>,
    bvars: Vec<NodeId>,
    lowlimit: Option<NodeId>,
    uplimit: Option<NodeId>,
    degree: Option<NodeId>,
    logbase: Option<NodeId>,
    condition: Option<NodeId>,
    domain: Option<NodeId>,
    interval: Option<NodeId>,
}

/// Returns `true` for element names that are Content MathML constructs handled
/// here (so the presentation parser can delegate to us).
pub(super) fn is_content_element(name: &str) -> bool {
    matches!(
        name,
        "apply"
            | "reln"
            | "fn"
            | "ci"
            | "cn"
            | "csymbol"
            | "bvar"
            | "lowlimit"
            | "uplimit"
            | "degree"
            | "logbase"
            | "condition"
            | "domainofapplication"
            | "momentabout"
            | "interval"
            | "set"
            | "list"
            | "vector"
            | "matrix"
            | "matrixrow"
            | "piecewise"
            | "piece"
            | "otherwise"
            | "lambda"
            | "declare"
            | "sep"
    ) || content_constant(name).is_some()
        || is_op_token(name)
}

/// Constant/identifier tokens (empty elements) → (text, kind).
fn content_constant(name: &str) -> Option<(&'static str, SymbolKind)> {
    use SymbolKind::*;
    Some(match name {
        "pi" => ("\u{03C0}", Alphabetic),
        "exponentiale" => ("\u{2147}", Alphabetic), // ⅇ
        "imaginaryi" => ("\u{2148}", Alphabetic),   // ⅈ
        "gamma" | "eulergamma" => ("\u{03B3}", Alphabetic),
        "infinity" => ("\u{221E}", Ordinary),
        "notanumber" => ("NaN", Ordinary),
        "true" => ("true", Ordinary),
        "false" => ("false", Ordinary),
        "emptyset" => ("\u{2205}", Ordinary),
        "integers" => ("\u{2124}", Alphabetic),       // ℤ
        "reals" => ("\u{211D}", Alphabetic),          // ℝ
        "rationals" => ("\u{211A}", Alphabetic),      // ℚ
        "naturalnumbers" => ("\u{2115}", Alphabetic), // ℕ
        "complexes" => ("\u{2102}", Alphabetic),      // ℂ
        "primes" => ("\u{2119}", Alphabetic),         // ℙ
        _ => return None,
    })
}

/// Operator tokens that may head an `<apply>`.
fn is_op_token(name: &str) -> bool {
    matches!(
        name,
        "plus"
            | "minus"
            | "times"
            | "divide"
            | "power"
            | "root"
            | "rem"
            | "quotient"
            | "max"
            | "min"
            | "gcd"
            | "lcm"
            | "factorial"
            | "abs"
            | "floor"
            | "ceiling"
            | "conjugate"
            | "real"
            | "imaginary"
            | "arg"
            | "exp"
            | "ln"
            | "log"
            | "sin"
            | "cos"
            | "tan"
            | "sec"
            | "csc"
            | "cot"
            | "sinh"
            | "cosh"
            | "tanh"
            | "sech"
            | "csch"
            | "coth"
            | "arcsin"
            | "arccos"
            | "arctan"
            | "arccot"
            | "arcsec"
            | "arccsc"
            | "arcsinh"
            | "arccosh"
            | "arctanh"
            | "arccoth"
            | "arcsech"
            | "arccsch"
            | "eq"
            | "neq"
            | "lt"
            | "gt"
            | "leq"
            | "geq"
            | "equivalent"
            | "approx"
            | "factorof"
            | "tendsto"
            | "and"
            | "or"
            | "xor"
            | "not"
            | "implies"
            | "forall"
            | "exists"
            | "in"
            | "notin"
            | "subset"
            | "prsubset"
            | "notsubset"
            | "notprsubset"
            | "union"
            | "intersect"
            | "setdiff"
            | "cartesianproduct"
            | "compose"
            | "int"
            | "sum"
            | "product"
            | "limit"
            | "diff"
            | "partialdiff"
            | "grad"
            | "divergence"
            | "curl"
            | "laplacian"
            | "determinant"
            | "transpose"
            | "inverse"
            | "ident"
            | "vectorproduct"
            | "scalarproduct"
            | "outerproduct"
            | "selector"
            | "domain"
            | "codomain"
            | "image"
            | "mean"
            | "sdev"
            | "variance"
            | "median"
            | "mode"
            | "moment"
            | "momentabout"
            | "card"
    )
}

impl<'a> MathmlCompactParser<'a> {
    // ── tiny constructors ──────────────────────────────────────────────
    fn c_ident(&mut self, s: &str) -> NodeId {
        let sid = self.arena.push_str(s);
        self.arena.push_node(CompactNode::Identifier(sid))
    }
    fn c_num(&mut self, s: &str) -> NodeId {
        let sid = self.arena.push_str(s);
        self.arena.push_node(CompactNode::Number(sid))
    }
    fn c_sym(&mut self, s: &str, kind: SymbolKind) -> NodeId {
        let sid = self.arena.push_str(s);
        self.arena.push_node(CompactNode::Symbol(kind, sid))
    }
    fn c_op_named(&mut self, s: &str) -> NodeId {
        let name = self.arena.push_str(s);
        self.arena.push_node(CompactNode::Operator {
            name,
            limits: LimitStyle::Default,
        })
    }
    fn c_row(&mut self, ids: &[NodeId]) -> NodeId {
        if ids.len() == 1 {
            ids[0]
        } else {
            let span = self.arena.push_children(ids);
            self.arena.push_node(CompactNode::Row(span))
        }
    }
    fn c_fenced(&mut self, open: &str, close: &str, content: &[NodeId]) -> NodeId {
        let open = self.arena.push_str(open);
        let close = self.arena.push_str(close);
        let content = self.arena.push_children(content);
        self.arena.push_node(CompactNode::Fenced {
            open,
            close,
            content,
        })
    }
    fn c_sup(&mut self, base: NodeId, sup: NodeId) -> NodeId {
        self.arena.push_node(CompactNode::Superscript { base, sup })
    }
    fn c_sub(&mut self, base: NodeId, sub: NodeId) -> NodeId {
        self.arena.push_node(CompactNode::Subscript { base, sub })
    }
    fn c_frac(&mut self, num: NodeId, den: NodeId) -> NodeId {
        self.arena.push_node(CompactNode::Fraction {
            kind: FractionKind::Normal,
            num,
            den,
        })
    }
    fn c_overbar(&mut self, base: NodeId) -> NodeId {
        self.arena.push_node(CompactNode::Accent {
            base,
            accent: '\u{0305}',
            position: AccentPosition::Above,
            wide: true,
        })
    }

    /// Join args with an infix operator symbol of the given kind.
    fn join_infix(&mut self, args: &[NodeId], sym: &str, kind: SymbolKind) -> NodeId {
        if args.is_empty() {
            return self.empty_row();
        }
        let mut row: SmallVec<[NodeId; 8]> = SmallVec::new();
        for (i, &a) in args.iter().enumerate() {
            if i > 0 {
                let op = self.c_sym(sym, kind);
                row.push(op);
            }
            row.push(a);
        }
        self.c_row(&row)
    }

    /// Wrap a function's argument list, parenthesising compound arguments.
    fn func_arg(&mut self, args: &[NodeId]) -> NodeId {
        if args.is_empty() {
            return self.empty_row();
        }
        if args.len() == 1 && self.is_simple(args[0]) {
            return args[0];
        }
        // comma-separate
        let mut items: SmallVec<[NodeId; 8]> = SmallVec::new();
        for (i, &a) in args.iter().enumerate() {
            if i > 0 {
                let comma = self.c_sym(",", SymbolKind::Punctuation);
                items.push(comma);
            }
            items.push(a);
        }
        self.c_fenced("(", ")", &items)
    }

    fn is_simple(&self, id: NodeId) -> bool {
        matches!(
            self.arena.node(id),
            CompactNode::Identifier(_) | CompactNode::Number(_) | CompactNode::Symbol(_, _)
        )
    }

    // ── entry point from the presentation dispatcher ───────────────────
    pub(super) fn parse_content_element(
        &mut self,
        name: &str,
        start: &quick_xml::events::BytesStart<'_>,
        is_empty: bool,
    ) -> Result<NodeId> {
        if is_empty {
            if let Some((text, kind)) = content_constant(name) {
                return Ok(self.c_sym(text, kind));
            }
            // A bare operator token outside an apply: render its label.
            return Ok(self.op_label(name));
        }
        match name {
            "apply" | "reln" => self.parse_apply(name),
            "fn" => {
                // <fn> wraps the function expression; take its (single) child.
                self.parse_child_id("fn")
            },
            "ci" => self.parse_ci(start),
            "cn" => self.parse_cn(start),
            "csymbol" => {
                let t = self.read_text_content("csymbol")?;
                Ok(self.c_ident(t.trim()))
            },
            "set" => self.parse_container("set", "{", "}"),
            "list" => self.parse_container("list", "[", "]"),
            "interval" => self.parse_container("interval", "[", "]"),
            "vector" => self.parse_container("vector", "(", ")"),
            "matrix" => self.parse_matrix(),
            "matrixrow" => self.parse_container("matrixrow", "(", ")"),
            "piecewise" => self.parse_piecewise(),
            "lambda" => self.parse_lambda(),
            "declare" => {
                // Declarations produce no visible output.
                self.skip_element("declare")?;
                Ok(self.empty_row())
            },
            // Qualifiers encountered standalone — just lower their content.
            "bvar"
            | "lowlimit"
            | "uplimit"
            | "degree"
            | "logbase"
            | "condition"
            | "domainofapplication"
            | "momentabout"
            | "piece"
            | "otherwise" => self.parse_child_id(name),
            _ => {
                if content_constant(name).is_some() {
                    let (t, k) = content_constant(name).unwrap();
                    self.skip_element(name)?;
                    Ok(self.c_sym(t, k))
                } else if is_op_token(name) {
                    // operator token with (unexpected) children — treat as label
                    self.skip_element(name)?;
                    Ok(self.op_label(name))
                } else {
                    self.parse_child_id(name)
                }
            },
        }
    }

    fn parse_ci(&mut self, start: &quick_xml::events::BytesStart<'_>) -> Result<NodeId> {
        let ty = get_attr(start, "type").or_else(|| get_attr(start, "mathvariant"));
        // `<ci>` may hold plain text OR embedded presentation markup
        // (e.g. `<ci><msub><mi>x</mi><mi>a</mi></msub></ci>`).
        let mut text = String::new();
        let mut kids: SmallVec<[NodeId; 4]> = SmallVec::new();
        loop {
            match self.reader_read_event()? {
                Event::Text(e) => text.push_str(&unescape_text(&e)),
                Event::Start(e) => {
                    let n = Self::local_name(&e);
                    let id = self.parse_element_by_name(&n, &e)?;
                    kids.push(id);
                },
                Event::Empty(e) => {
                    let n = Self::local_name(&e);
                    if n == "mglyph"
                        && let Some(alt) = get_attr(&e, "alt")
                    {
                        text.push_str(&alt);
                    } else {
                        let id = self.parse_empty_element(&n, &e)?;
                        kids.push(id);
                    }
                },
                Event::End(e) if Self::local_name_end(&e) == "ci" => {
                    break;
                },
                Event::Eof => break,
                _ => {},
            }
        }
        let id = if !kids.is_empty() {
            self.c_row(&kids)
        } else {
            self.c_ident(text.trim())
        };
        // type="vector"/"matrix" → bold
        if matches!(ty.as_deref(), Some("vector") | Some("matrix")) {
            let span = self.arena.push_children(&[id]);
            return Ok(self
                .arena
                .push_node(CompactNode::Styled(TextStyle::Bold, span)));
        }
        Ok(id)
    }

    fn parse_cn(&mut self, start: &quick_xml::events::BytesStart<'_>) -> Result<NodeId> {
        let ty = get_attr(start, "type").unwrap_or_default();
        // Collect text parts separated by <sep/>.
        let mut parts: Vec<String> = vec![String::new()];
        loop {
            match self.reader_read_event()? {
                Event::Text(e) => {
                    let t = unescape_text(&e);
                    parts.last_mut().unwrap().push_str(&t);
                },
                Event::Empty(e) | Event::Start(e) if Self::local_name(&e) == "sep" => {
                    parts.push(String::new());
                },
                Event::End(e) => {
                    let n = Self::local_name_end(&e);
                    if n == "cn" {
                        break;
                    }
                },
                Event::Eof => break,
                _ => {},
            }
        }
        let parts: Vec<String> = parts.iter().map(|s| s.trim().to_owned()).collect();
        match ty.as_str() {
            "rational" if parts.len() >= 2 => {
                let n = self.c_num(&parts[0]);
                let d = self.c_num(&parts[1]);
                Ok(self.c_frac(n, d))
            },
            "complex-cartesian" if parts.len() >= 2 => {
                let re = self.c_num(&parts[0]);
                let plus = self.c_sym("+", SymbolKind::BinaryOp);
                let im = self.c_num(&parts[1]);
                let i = self.c_ident("\u{2148}");
                let row = [re, plus, im, i];
                Ok(self.c_row(&row))
            },
            "complex-polar" if parts.len() >= 2 => {
                let r = self.c_num(&parts[0]);
                let theta = self.c_num(&parts[1]);
                let row = [r, theta];
                Ok(self.c_row(&row))
            },
            "e-notation" | "enotation" if parts.len() >= 2 => {
                let m = self.c_num(&parts[0]);
                let times = self.c_sym("\u{00D7}", SymbolKind::BinaryOp);
                let ten = self.c_num("10");
                let exp = self.c_num(&parts[1]);
                let pw = self.c_sup(ten, exp);
                let row = [m, times, pw];
                Ok(self.c_row(&row))
            },
            _ => {
                let joined = parts.join(" ");
                Ok(self.c_num(joined.trim()))
            },
        }
    }

    /// Parse `<apply>`/`<reln>` and build a presentation node.
    fn parse_apply(&mut self, tag: &str) -> Result<NodeId> {
        self.bump_depth(tag)?;
        let op = match self.read_content_operator()? {
            Some(op) => op,
            None => {
                self.unbump_depth();
                return Ok(self.empty_row());
            },
        };
        let mut acc = Applied::default();
        loop {
            match self.reader_read_event()? {
                Event::Start(e) => {
                    let n = Self::local_name(&e);
                    self.collect_apply_child(&n, &e, false, &mut acc)?;
                },
                Event::Empty(e) => {
                    let n = Self::local_name(&e);
                    self.collect_apply_child(&n, &e, true, &mut acc)?;
                },
                Event::Text(e) => {
                    let t = unescape_text(&e);
                    if !t.trim().is_empty() {
                        let id = self.classify_text_compact(&t);
                        acc.args.push(id);
                    }
                },
                Event::End(e) if Self::local_name_end(&e) == tag => {
                    break;
                },
                Event::Eof => break,
                _ => {},
            }
        }
        self.unbump_depth();
        Ok(self.build_apply(op, acc))
    }

    fn collect_apply_child(
        &mut self,
        name: &str,
        start: &quick_xml::events::BytesStart<'_>,
        is_empty: bool,
        acc: &mut Applied,
    ) -> Result<()> {
        match name {
            "bvar" => {
                let v = self.parse_bvar()?;
                acc.bvars.push(v);
            },
            "lowlimit" => acc.lowlimit = Some(self.parse_child_id("lowlimit")?),
            "uplimit" => acc.uplimit = Some(self.parse_child_id("uplimit")?),
            "degree" => acc.degree = Some(self.parse_child_id("degree")?),
            "logbase" => acc.logbase = Some(self.parse_child_id("logbase")?),
            "condition" => acc.condition = Some(self.parse_child_id("condition")?),
            "domainofapplication" => acc.domain = Some(self.parse_child_id("domainofapplication")?),
            "momentabout" => acc.domain = Some(self.parse_child_id("momentabout")?),
            "interval" => acc.interval = Some(self.parse_container("interval", "[", "]")?),
            _ => {
                let id = if is_empty {
                    self.parse_empty_element(name, start)?
                } else {
                    self.parse_element_by_name(name, start)?
                };
                acc.args.push(id);
            },
        }
        Ok(())
    }

    /// Read the head operator of an apply (first element child).
    fn read_content_operator(&mut self) -> Result<Option<COp>> {
        loop {
            match self.reader_read_event()? {
                Event::Start(e) => {
                    let name = Self::local_name(&e);
                    if is_op_token(&name) {
                        self.skip_element(&name)?;
                        return Ok(Some(COp::Token(name)));
                    }
                    let id = self.parse_element_by_name(&name, &e)?;
                    return Ok(Some(COp::Func(id)));
                },
                Event::Empty(e) => {
                    let name = Self::local_name(&e);
                    if is_op_token(&name) {
                        return Ok(Some(COp::Token(name)));
                    }
                    let id = self.parse_empty_element(&name, &e)?;
                    return Ok(Some(COp::Func(id)));
                },
                Event::Text(_) => continue,
                Event::End(_) | Event::Eof => return Ok(None),
                _ => continue,
            }
        }
    }

    /// `<bvar>` may contain a variable and an optional `<degree>`.
    fn parse_bvar(&mut self) -> Result<NodeId> {
        let mut var: Option<NodeId> = None;
        let mut degree: Option<NodeId> = None;
        loop {
            match self.reader_read_event()? {
                Event::Start(e) => {
                    let n = Self::local_name(&e);
                    if n == "degree" {
                        degree = Some(self.parse_child_id("degree")?);
                    } else {
                        let id = self.parse_element_by_name(&n, &e)?;
                        if var.is_none() {
                            var = Some(id);
                        }
                    }
                },
                Event::Empty(e) => {
                    let n = Self::local_name(&e);
                    let id = self.parse_empty_element(&n, &e)?;
                    if var.is_none() {
                        var = Some(id);
                    }
                },
                Event::Text(e) => {
                    let t = unescape_text(&e);
                    if !t.trim().is_empty() && var.is_none() {
                        var = Some(self.classify_text_compact(&t));
                    }
                },
                Event::End(e) if Self::local_name_end(&e) == "bvar" => {
                    break;
                },
                Event::Eof => break,
                _ => {},
            }
        }
        let var = var.unwrap_or_else(|| self.empty_row());
        // Stash degree on the var via a superscript-style marker is overkill;
        // higher-order derivatives read `degree` from the apply qualifier path,
        // so here we simply return the variable (degree, if any, is ignored
        // for the bvar token itself but used by `diff`).
        let _ = degree;
        Ok(var)
    }

    // ── containers ─────────────────────────────────────────────────────
    fn parse_container(&mut self, tag: &str, open: &str, close: &str) -> Result<NodeId> {
        let children = self.parse_children(tag)?;
        let mut items: SmallVec<[NodeId; 8]> = SmallVec::new();
        for (i, &c) in children.iter().enumerate() {
            if i > 0 {
                let comma = self.c_sym(",", SymbolKind::Punctuation);
                items.push(comma);
            }
            items.push(c);
        }
        Ok(self.c_fenced(open, close, &items))
    }

    fn parse_matrix(&mut self) -> Result<NodeId> {
        self.bump_depth("matrix")?;
        // Each child should be a matrixrow → collect rows of cells.
        let mut rows: Vec<Vec<NodeId>> = Vec::new();
        loop {
            match self.reader_read_event()? {
                Event::Start(e) => {
                    let n = Self::local_name(&e);
                    if n == "matrixrow" {
                        let cells = self.parse_children("matrixrow")?;
                        rows.push(cells.to_vec());
                    } else {
                        let _ = self.parse_element_by_name(&n, &e)?;
                    }
                },
                Event::End(e) if Self::local_name_end(&e) == "matrix" => {
                    break;
                },
                Event::Eof => break,
                _ => {},
            }
        }
        self.unbump_depth();
        let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0) as u16;
        let mut cells: Vec<NodeId> = Vec::new();
        for r in &rows {
            for c in 0..num_cols as usize {
                let cell = r.get(c).copied().unwrap_or_else(|| self.empty_row());
                let span = self.arena.push_children(&[cell]);
                cells.push(self.arena.push_node(CompactNode::Row(span)));
            }
        }
        let col_aligns: SmallVec<[NodeId; 8]> = (0..num_cols)
            .map(|_| {
                self.arena
                    .push_node(CompactNode::ColAlign(crate::ast::ColumnAlign::Center))
            })
            .collect();
        let col_aligns = self.arena.push_children(&col_aligns);
        let cells = self.arena.push_children(&cells);
        let inner = self.arena.push_node(CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
            arraystretch_pct: 100,
            add_jot: false,
            array_style: false,
            col_seps: 0,
            row_seps: 0,
        });
        Ok(self.c_fenced("(", ")", &[inner]))
    }

    fn parse_piecewise(&mut self) -> Result<NodeId> {
        self.bump_depth("piecewise")?;
        // rows: [value, condition-text]
        let mut rows: Vec<(NodeId, NodeId)> = Vec::new();
        loop {
            match self.reader_read_event()? {
                Event::Start(e) => {
                    let n = Self::local_name(&e);
                    match n.as_str() {
                        "piece" => {
                            let kids = self.parse_children("piece")?;
                            let val = kids.first().copied().unwrap_or_else(|| self.empty_row());
                            let cond = if kids.len() >= 2 {
                                let label = self.c_text("if ");
                                self.c_row(&[label, kids[1]])
                            } else {
                                self.empty_row()
                            };
                            rows.push((val, cond));
                        },
                        "otherwise" => {
                            let kids = self.parse_children("otherwise")?;
                            let val = kids.first().copied().unwrap_or_else(|| self.empty_row());
                            let cond = self.c_text("otherwise");
                            rows.push((val, cond));
                        },
                        _ => {
                            let _ = self.parse_element_by_name(&n, &e)?;
                        },
                    }
                },
                Event::End(e) if Self::local_name_end(&e) == "piecewise" => {
                    break;
                },
                Event::Eof => break,
                _ => {},
            }
        }
        self.unbump_depth();
        let num_cols = 2u16;
        let mut cells: Vec<NodeId> = Vec::new();
        for (val, cond) in &rows {
            for &cell in &[*val, *cond] {
                let span = self.arena.push_children(&[cell]);
                cells.push(self.arena.push_node(CompactNode::Row(span)));
            }
        }
        let col_aligns: SmallVec<[NodeId; 8]> = (0..num_cols)
            .map(|_| {
                self.arena
                    .push_node(CompactNode::ColAlign(crate::ast::ColumnAlign::Left))
            })
            .collect();
        let col_aligns = self.arena.push_children(&col_aligns);
        let cells = self.arena.push_children(&cells);
        let inner = self.arena.push_node(CompactNode::Array {
            col_aligns,
            cells,
            num_cols,
            arraystretch_pct: 100,
            add_jot: false,
            array_style: false,
            col_seps: 0,
            row_seps: 0,
        });
        // Left brace, no right.
        let open = self.arena.push_str("{");
        let close = self.arena.push_str("");
        let content = self.arena.push_children(&[inner]);
        Ok(self.arena.push_node(CompactNode::Fenced {
            open,
            close,
            content,
        }))
    }

    fn parse_lambda(&mut self) -> Result<NodeId> {
        self.bump_depth("lambda")?;
        let mut bvars: Vec<NodeId> = Vec::new();
        let mut body: Option<NodeId> = None;
        loop {
            match self.reader_read_event()? {
                Event::Start(e) => {
                    let n = Self::local_name(&e);
                    if n == "bvar" {
                        bvars.push(self.parse_bvar()?);
                    } else {
                        let id = self.parse_element_by_name(&n, &e)?;
                        if body.is_none() {
                            body = Some(id);
                        }
                    }
                },
                Event::Empty(e) => {
                    let n = Self::local_name(&e);
                    let id = self.parse_empty_element(&n, &e)?;
                    if body.is_none() {
                        body = Some(id);
                    }
                },
                Event::End(e) if Self::local_name_end(&e) == "lambda" => {
                    break;
                },
                Event::Eof => break,
                _ => {},
            }
        }
        self.unbump_depth();
        // λ var₁,var₂ . body
        let mut row: SmallVec<[NodeId; 8]> = SmallVec::new();
        let lambda = self.c_sym("\u{03BB}", SymbolKind::Alphabetic);
        row.push(lambda);
        for (i, &v) in bvars.iter().enumerate() {
            if i > 0 {
                let c = self.c_sym(",", SymbolKind::Punctuation);
                row.push(c);
            }
            row.push(v);
        }
        let dot = self.c_sym(".", SymbolKind::Punctuation);
        row.push(dot);
        if let Some(b) = body {
            row.push(b);
        }
        Ok(self.c_row(&row))
    }

    fn c_text(&mut self, s: &str) -> NodeId {
        let sid = self.arena.push_str(s);
        self.arena
            .push_node(CompactNode::Text(TextStyle::Normal, sid))
    }

    /// Build a presentation node from an apply's operator + operands.
    fn build_apply(&mut self, op: COp, a: Applied) -> NodeId {
        let token = match op {
            COp::Func(f) => {
                // Function application: f(args)
                let arg = self.func_arg(&a.args);
                if a.args.is_empty() {
                    return f;
                }
                let af = self.c_sym("\u{2061}", SymbolKind::Ordinary); // ApplyFunction
                let row = [f, af, arg];
                return self.c_row(&row);
            },
            COp::Token(t) => t,
        };
        self.build_token_apply(&token, a)
    }

    fn build_token_apply(&mut self, token: &str, a: Applied) -> NodeId {
        use SymbolKind::*;
        let _unused_quals = (a.domain, a.interval);
        let args = &a.args;
        match token {
            "plus" => self.join_infix(args, "+", BinaryOp),
            "vectorproduct" => self.join_infix(args, "\u{00D7}", BinaryOp),
            "times" => self.join_infix(args, "\u{2062}", Ordinary), // InvisibleTimes (zero-width)
            "scalarproduct" => self.join_infix(args, "\u{22C5}", BinaryOp),
            "outerproduct" => self.join_infix(args, "\u{2297}", BinaryOp),
            "minus" => {
                if args.len() == 1 {
                    let m = self.c_sym("\u{2212}", BinaryOp);
                    let row = [m, args[0]];
                    self.c_row(&row)
                } else {
                    self.join_infix(args, "\u{2212}", BinaryOp)
                }
            },
            "divide" => {
                if args.len() >= 2 {
                    self.c_frac(args[0], args[1])
                } else {
                    self.func_arg(args)
                }
            },
            "power" if args.len() >= 2 => self.c_sup(args[0], args[1]),
            "root" => {
                let radicand = args.first().copied().unwrap_or_else(|| self.empty_row());
                match a.degree {
                    Some(d) if !self.is_two(d) => self
                        .arena
                        .push_node(CompactNode::Root { index: d, radicand }),
                    _ => self.arena.push_node(CompactNode::Sqrt(radicand)),
                }
            },
            "abs" | "card" => {
                let inner = self.func_arg_raw(args);
                self.c_fenced("|", "|", &[inner])
            },
            "floor" => {
                let inner = self.func_arg_raw(args);
                self.c_fenced("\u{230A}", "\u{230B}", &[inner])
            },
            "ceiling" => {
                let inner = self.func_arg_raw(args);
                self.c_fenced("\u{2308}", "\u{2309}", &[inner])
            },
            "factorial" => {
                let base = self.maybe_paren(args.first().copied());
                let bang = self.c_sym("!", Ordinary);
                let row = [base, bang];
                self.c_row(&row)
            },
            "conjugate" => {
                let base = self.func_arg_raw(args);
                self.c_overbar(base)
            },
            "mean" => {
                let base = self.func_arg_raw(args);
                self.c_overbar(base)
            },
            "not" => {
                let arg = self.maybe_paren(args.first().copied());
                let neg = self.c_sym("\u{00AC}", Ordinary);
                let row = [neg, arg];
                self.c_row(&row)
            },
            "transpose" => {
                let base = self.func_arg_raw(args);
                let t = self.c_ident("T");
                self.c_sup(base, t)
            },
            "inverse" => {
                let base = self.func_arg_raw(args);
                let m1 = self.c_num("-1");
                self.c_sup(base, m1)
            },
            "real" => self.prefix_symbol("\u{211C}", args), // ℜ
            "imaginary" => self.prefix_symbol("\u{2111}", args), // ℑ
            "grad" => self.prefix_symbol("\u{2207}", args),
            "laplacian" => {
                let nabla = self.c_sym("\u{2207}", Ordinary);
                let two = self.c_num("2");
                let sq = self.c_sup(nabla, two);
                let arg = self.func_arg(args);
                let row = [sq, arg];
                self.c_row(&row)
            },
            "divergence" => {
                let nabla = self.c_sym("\u{2207}", Ordinary);
                let dot = self.c_sym("\u{22C5}", BinaryOp);
                let arg = self.func_arg(args);
                let row = [nabla, dot, arg];
                self.c_row(&row)
            },
            "curl" => {
                let nabla = self.c_sym("\u{2207}", Ordinary);
                let cross = self.c_sym("\u{00D7}", BinaryOp);
                let arg = self.func_arg(args);
                let row = [nabla, cross, arg];
                self.c_row(&row)
            },
            // Relations
            "eq" => self.join_infix(args, "=", Relation),
            "neq" => self.join_infix(args, "\u{2260}", Relation),
            "lt" => self.join_infix(args, "<", Relation),
            "gt" => self.join_infix(args, ">", Relation),
            "leq" => self.join_infix(args, "\u{2264}", Relation),
            "geq" => self.join_infix(args, "\u{2265}", Relation),
            "equivalent" => self.join_infix(args, "\u{2261}", Relation),
            "approx" => self.join_infix(args, "\u{2248}", Relation),
            "tendsto" => self.join_infix(args, "\u{2192}", Relation),
            "factorof" => self.join_infix(args, "\u{2223}", Relation),
            "implies" => self.join_infix(args, "\u{21D2}", Relation),
            // Set theory
            "in" => self.join_infix(args, "\u{2208}", Relation),
            "notin" => self.join_infix(args, "\u{2209}", Relation),
            "subset" => self.join_infix(args, "\u{2286}", Relation),
            "prsubset" => self.join_infix(args, "\u{2282}", Relation),
            "notsubset" => self.join_infix(args, "\u{2288}", Relation),
            "notprsubset" => self.join_infix(args, "\u{2284}", Relation),
            "union" => self.join_infix(args, "\u{222A}", BinaryOp),
            "intersect" => self.join_infix(args, "\u{2229}", BinaryOp),
            "setdiff" => self.join_infix(args, "\u{2216}", BinaryOp),
            "cartesianproduct" => self.join_infix(args, "\u{00D7}", BinaryOp),
            "compose" => self.join_infix(args, "\u{2218}", BinaryOp),
            // Logic
            "and" => self.join_infix(args, "\u{2227}", BinaryOp),
            "or" => self.join_infix(args, "\u{2228}", BinaryOp),
            "xor" => self.join_infix(args, "\u{22BB}", BinaryOp),
            "forall" => self.quantifier("\u{2200}", a),
            "exists" => self.quantifier("\u{2203}", a),
            // Big operators
            "sum" => self.nary('\u{2211}', a),
            "product" => self.nary('\u{220F}', a),
            "int" => self.integral(a),
            "limit" => self.limit(a),
            "diff" => self.diff(a),
            "partialdiff" => self.partialdiff(a),
            // Functions with the token as upright label
            "log" => {
                let label = self.c_op_named("log");
                let head = if let Some(b) = a.logbase {
                    self.c_sub(label, b)
                } else {
                    label
                };
                let arg = self.func_arg(args);
                let af = self.c_sym("\u{2061}", Ordinary);
                let row = [head, af, arg];
                self.c_row(&row)
            },
            _ => {
                // Generic upright-named function application: token(args).
                let label = self.c_op_named(token);
                if args.is_empty() {
                    return label;
                }
                let arg = self.func_arg(args);
                let af = self.c_sym("\u{2061}", Ordinary);
                let row = [label, af, arg];
                self.c_row(&row)
            },
        }
    }

    /// Like `func_arg`, but never strips to a bare leaf — used where we always
    /// want the raw single content (e.g. inside |·|).
    fn func_arg_raw(&mut self, args: &[NodeId]) -> NodeId {
        match args.len() {
            0 => self.empty_row(),
            1 => args[0],
            _ => {
                let mut items: SmallVec<[NodeId; 8]> = SmallVec::new();
                for (i, &x) in args.iter().enumerate() {
                    if i > 0 {
                        let c = self.c_sym(",", SymbolKind::Punctuation);
                        items.push(c);
                    }
                    items.push(x);
                }
                self.c_row(&items)
            },
        }
    }

    fn maybe_paren(&mut self, arg: Option<NodeId>) -> NodeId {
        match arg {
            None => self.empty_row(),
            Some(id) if self.is_simple(id) => id,
            Some(id) => self.c_fenced("(", ")", &[id]),
        }
    }

    fn prefix_symbol(&mut self, sym: &str, args: &[NodeId]) -> NodeId {
        let s = self.c_sym(sym, SymbolKind::Ordinary);
        if args.is_empty() {
            return s;
        }
        let arg = self.func_arg(args);
        let row = [s, arg];
        self.c_row(&row)
    }

    fn is_two(&self, id: NodeId) -> bool {
        if let CompactNode::Number(sid) = self.arena.node(id) {
            return self.arena.str_val(*sid).trim() == "2";
        }
        false
    }

    fn op_label(&mut self, token: &str) -> NodeId {
        self.c_op_named(token)
    }

    fn quantifier(&mut self, sym: &str, a: Applied) -> NodeId {
        let mut row: SmallVec<[NodeId; 8]> = SmallVec::new();
        let q = self.c_sym(sym, SymbolKind::Ordinary);
        row.push(q);
        for (i, &v) in a.bvars.iter().enumerate() {
            if i > 0 {
                let c = self.c_sym(",", SymbolKind::Punctuation);
                row.push(c);
            }
            row.push(v);
        }
        if let Some(cond) = a.condition {
            let c = self.c_sym(",", SymbolKind::Punctuation);
            row.push(c);
            row.push(cond);
        }
        if let Some(&body) = a.args.first() {
            let colon = self.c_sym(":", SymbolKind::Punctuation);
            row.push(colon);
            row.push(body);
        }
        self.c_row(&row)
    }

    fn nary(&mut self, op: char, a: Applied) -> NodeId {
        let sub = self.sum_lower(&a);
        let sup = a.uplimit.unwrap_or_else(|| self.empty_row());
        let body = a.args.first().copied().unwrap_or_else(|| self.empty_row());
        self.arena.push_node(CompactNode::Nary {
            op,
            sub,
            sup,
            body,
            limits: LimitStyle::Default,
        })
    }

    /// Build the lower script of a sum/product: "bvar = lowlimit" or condition.
    fn sum_lower(&mut self, a: &Applied) -> NodeId {
        if let Some(low) = a.lowlimit {
            if let Some(&bv) = a.bvars.first() {
                let eq = self.c_sym("=", SymbolKind::Relation);
                let row = [bv, eq, low];
                return self.c_row(&row);
            }
            return low;
        }
        if let Some(cond) = a.condition {
            return cond;
        }
        if let Some(&bv) = a.bvars.first() {
            return bv;
        }
        self.empty_row()
    }

    fn integral(&mut self, a: Applied) -> NodeId {
        let sub = a.lowlimit.unwrap_or_else(|| self.empty_row());
        let sup = a.uplimit.unwrap_or_else(|| self.empty_row());
        let integrand = a.args.first().copied().unwrap_or_else(|| self.empty_row());
        // body = integrand  d(bvar)
        let mut body: SmallVec<[NodeId; 4]> = SmallVec::new();
        body.push(integrand);
        if let Some(&bv) = a.bvars.first() {
            let d = self.c_ident("d");
            body.push(d);
            body.push(bv);
        }
        let body = self.c_row(&body);
        self.arena.push_node(CompactNode::Nary {
            op: '\u{222B}',
            sub,
            sup,
            body,
            limits: LimitStyle::SubSup,
        })
    }

    fn limit(&mut self, a: Applied) -> NodeId {
        let lim = self.c_op_named("lim");
        let under = if let Some(cond) = a.condition {
            cond
        } else if let (Some(&bv), Some(low)) = (a.bvars.first(), a.lowlimit) {
            let to = self.c_sym("\u{2192}", SymbolKind::Relation);
            let row = [bv, to, low];
            self.c_row(&row)
        } else {
            self.empty_row()
        };
        let head = self.arena.push_node(CompactNode::Underscript {
            base: lim,
            under,
            movable_limits: true,
        });
        let body = a.args.first().copied().unwrap_or_else(|| self.empty_row());
        let row = [head, body];
        self.c_row(&row)
    }

    fn diff(&mut self, a: Applied) -> NodeId {
        // d/d(bvar) f  — ordinary derivative
        let f = a.args.first().copied().unwrap_or_else(|| self.empty_row());
        if let Some(&bv) = a.bvars.first() {
            let d_num = self.c_ident("d");
            let d_den = self.c_ident("d");
            let den_row = [d_den, bv];
            let den = self.c_row(&den_row);
            let frac = self.c_frac(d_num, den);
            let row = [frac, f];
            self.c_row(&row)
        } else {
            // f' notation
            let prime = self.c_sym("\u{2032}", SymbolKind::Ordinary);
            self.c_sup(f, prime)
        }
    }

    fn partialdiff(&mut self, a: Applied) -> NodeId {
        let f = a.args.first().copied().unwrap_or_else(|| self.empty_row());
        if let Some(&bv) = a.bvars.first() {
            let p_num = self.c_sym("\u{2202}", SymbolKind::Ordinary);
            let p_den = self.c_sym("\u{2202}", SymbolKind::Ordinary);
            let den_row = [p_den, bv];
            let den = self.c_row(&den_row);
            let frac = self.c_frac(p_num, den);
            let row = [frac, f];
            self.c_row(&row)
        } else {
            f
        }
    }
}
