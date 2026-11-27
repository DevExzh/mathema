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
    /// A degree attached to each corresponding `<bvar>`, when present.
    /// Content MathML puts derivative order in the bvar itself for the
    /// ordinary `diff` form; keeping it parallel to `bvars` avoids losing it
    /// while the other bound-variable users continue to use just the name.
    bvar_degrees: Vec<Option<NodeId>>,
    lowlimit: Option<NodeId>,
    uplimit: Option<NodeId>,
    degree: Option<NodeId>,
    logbase: Option<NodeId>,
    condition: Option<NodeId>,
    domain: Option<NodeId>,
    interval: Option<NodeId>,
}

/// Precedence and associativity class for an infix Content MathML operator.
/// The class is also used to classify a previously lowered child row, so one
/// table controls all grouping decisions instead of each operator family
/// carrying its own list of symbols.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InfixClass {
    Unary,
    Add,
    Subtract,
    Multiply,
    Relation,
    LogicOr,
    LogicXor,
    LogicAnd,
    SetUnion,
    SetDifference,
    SetIntersect,
    Compose,
}

impl InfixClass {
    fn precedence(self) -> u8 {
        match self {
            Self::Relation => 10,
            Self::LogicOr => 20,
            Self::LogicXor => 25,
            Self::LogicAnd => 30,
            Self::SetUnion => 35,
            Self::SetDifference => 36,
            Self::SetIntersect => 40,
            Self::Compose => 45,
            Self::Add | Self::Subtract => 50,
            Self::Multiply => 60,
            Self::Unary => 70,
        }
    }
}

#[derive(Clone, Copy)]
struct InfixSpec {
    symbol: &'static str,
    kind: SymbolKind,
    class: InfixClass,
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
            | "bind"
            | "share"
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

/// Validate the digit spelling of a base-qualified Content MathML integer.
/// MathML permits an optional sign, followed by alphanumeric digits whose
/// values must fit the declared radix.
fn valid_based_integer_literal(value: &str, base: u32) -> bool {
    let digits = value
        .strip_prefix('+')
        .or_else(|| value.strip_prefix('-'))
        .unwrap_or(value);
    !digits.is_empty()
        && digits.chars().all(|ch| {
            let value = match ch {
                '0'..='9' => u32::from(ch as u8 - b'0'),
                'A'..='Z' => u32::from(ch as u8 - b'A') + 10,
                'a'..='z' => u32::from(ch as u8 - b'a') + 10,
                _ => return false,
            };
            value < base
        })
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
            // Content functions such as `ln` and `sin` are ordinary
            // side-script operators; only native limit names retain their
            // context-dependent default placement.
            limits: if crate::unicode::symbols::operator_has_limits(s) {
                LimitStyle::Default
            } else {
                LimitStyle::SubSup
            },
        })
    }
    fn c_row(&mut self, ids: &[NodeId]) -> NodeId {
        if ids.len() == 1 {
            ids[0]
        } else {
            let span = self.arena.push_children_deferred(ids);
            self.arena.push_node(CompactNode::Row(span))
        }
    }
    fn c_fenced(&mut self, open: &str, close: &str, content: &[NodeId]) -> NodeId {
        let open = self.arena.push_str(open);
        let close = self.arena.push_str(close);
        let content = self.arena.push_children_deferred(content);
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

    fn is_empty_node(&self, id: NodeId) -> bool {
        matches!(self.arena.node(id), CompactNode::Row(span) if span.is_empty())
    }

    fn is_negative_number(&self, id: NodeId) -> bool {
        let CompactNode::Number(sid) = *self.arena.node(id) else {
            return false;
        };
        self.arena
            .str_val(sid)
            .trim()
            .chars()
            .next()
            .is_some_and(|ch| ch == '-' || ch == '\u{2212}')
    }

    fn symbol_class(symbol: &str, position: usize) -> Option<InfixClass> {
        Some(match symbol {
            "+" => InfixClass::Add,
            "\u{2212}" | "-" if position > 0 => InfixClass::Subtract,
            "\u{2212}" | "-" => InfixClass::Unary,
            "\u{00D7}" | "\u{2062}" | "\u{22C5}" | "\u{2297}" => InfixClass::Multiply,
            "=" | "\u{2260}" | "<" | ">" | "\u{2264}" | "\u{2265}" | "\u{2261}" | "\u{2248}"
            | "\u{2192}" | "\u{2223}" | "\u{21D2}" | "\u{2208}" | "\u{2209}" | "\u{2286}"
            | "\u{2282}" | "\u{2288}" | "\u{2284}" => InfixClass::Relation,
            "\u{2227}" => InfixClass::LogicAnd,
            "\u{2228}" => InfixClass::LogicOr,
            "\u{22BB}" => InfixClass::LogicXor,
            "\u{222A}" => InfixClass::SetUnion,
            "\u{2216}" => InfixClass::SetDifference,
            "\u{2229}" => InfixClass::SetIntersect,
            "\u{2218}" => InfixClass::Compose,
            _ => return None,
        })
    }

    /// Return the output spelling and precedence class for an infix token.
    fn infix_spec(token: &str) -> Option<InfixSpec> {
        use SymbolKind::{BinaryOp, Ordinary, Relation as RelationKind};
        let (symbol, kind) = match token {
            "plus" => ("+", BinaryOp),
            "minus" => ("\u{2212}", BinaryOp),
            "vectorproduct" => ("\u{00D7}", BinaryOp),
            "times" => ("\u{2062}", Ordinary),
            "scalarproduct" => ("\u{22C5}", BinaryOp),
            "outerproduct" => ("\u{2297}", BinaryOp),
            "eq" => ("=", RelationKind),
            "neq" => ("\u{2260}", RelationKind),
            "lt" => ("<", RelationKind),
            "gt" => (">", RelationKind),
            "leq" => ("\u{2264}", RelationKind),
            "geq" => ("\u{2265}", RelationKind),
            "equivalent" => ("\u{2261}", RelationKind),
            "approx" => ("\u{2248}", RelationKind),
            "tendsto" => ("\u{2192}", RelationKind),
            "factorof" => ("\u{2223}", RelationKind),
            "implies" => ("\u{21D2}", RelationKind),
            "in" => ("\u{2208}", RelationKind),
            "notin" => ("\u{2209}", RelationKind),
            "subset" => ("\u{2286}", RelationKind),
            "prsubset" => ("\u{2282}", RelationKind),
            "notsubset" => ("\u{2288}", RelationKind),
            "notprsubset" => ("\u{2284}", RelationKind),
            "and" => ("\u{2227}", BinaryOp),
            "or" => ("\u{2228}", BinaryOp),
            "xor" => ("\u{22BB}", BinaryOp),
            "union" => ("\u{222A}", BinaryOp),
            "intersect" => ("\u{2229}", BinaryOp),
            "setdiff" => ("\u{2216}", BinaryOp),
            "cartesianproduct" => ("\u{00D7}", BinaryOp),
            "compose" => ("\u{2218}", BinaryOp),
            _ => return None,
        };
        Some(InfixSpec {
            symbol,
            kind,
            // Position one is the binary spelling; unary minus is handled
            // before this table in `build_token_apply`.
            class: Self::symbol_class(symbol, 1)?,
        })
    }

    /// Return the top-level infix class of a row made by one of the Content
    /// MathML operators. Looking at direct children is enough: nested applies
    /// remain one child, so their operation cannot be confused with the
    /// operation of the enclosing apply.
    fn row_infix(&self, id: NodeId) -> Option<InfixClass> {
        let CompactNode::Row(span) = *self.arena.node(id) else {
            return None;
        };
        for (position, &child) in self.arena.children(span).iter().enumerate() {
            let CompactNode::Symbol(_, sid) = *self.arena.node(child) else {
                continue;
            };
            let symbol = self.arena.str_val(sid);
            if let Some(class) = Self::symbol_class(symbol, position) {
                return Some(class);
            }
        }
        None
    }

    fn is_nary_node(&self, id: NodeId) -> bool {
        matches!(self.arena.node(id), CompactNode::Nary { .. })
    }

    /// Add visible parentheses when an apply operand would otherwise be read
    /// with a different precedence.  Existing fences are already sufficient;
    /// leaves are atoms and need no wrapper.
    fn parenthesize(&mut self, id: NodeId) -> NodeId {
        if (self.is_simple(id) && !self.is_negative_number(id))
            || matches!(self.arena.node(id), CompactNode::Fenced { .. })
        {
            id
        } else {
            self.c_fenced("(", ")", &[id])
        }
    }

    fn infix_operand_needs_parentheses(
        &self,
        parent: InfixClass,
        child: Option<InfixClass>,
        position: usize,
        id: NodeId,
    ) -> bool {
        let Some(child) = child else {
            // A large operator carries its own body, but as a multiplication
            // factor it must remain one visible operand (`(∑ …) x`).
            return parent == InfixClass::Multiply && self.is_nary_node(id);
        };
        if child.precedence() < parent.precedence() {
            return true;
        }
        if child.precedence() > parent.precedence() {
            return false;
        }
        match parent {
            InfixClass::Add => child == InfixClass::Subtract && position > 0,
            InfixClass::Subtract => {
                matches!(child, InfixClass::Add | InfixClass::Subtract) && position > 0
            },
            InfixClass::Relation => child == InfixClass::Relation,
            InfixClass::SetDifference => child == InfixClass::SetDifference && position > 0,
            // Same-class logical/set union/intersection and composition are
            // associative at this presentation boundary.
            _ => false,
        }
    }

    fn join_classified_infix(&mut self, args: &[NodeId], spec: InfixSpec) -> NodeId {
        let mut grouped = SmallVec::<[NodeId; 8]>::new();
        for (position, &arg) in args.iter().enumerate() {
            let child = self.row_infix(arg);
            let arg = if self.infix_operand_needs_parentheses(spec.class, child, position, arg) {
                self.parenthesize(arg)
            } else {
                arg
            };
            grouped.push(arg);
        }
        self.join_infix(&grouped, spec.symbol, spec.kind)
    }

    // ── entry point from the presentation dispatcher ───────────────────
    pub(super) fn parse_content_element(
        &mut self,
        name: &str,
        start: &quick_xml::events::BytesStart<'_>,
        is_empty: bool,
    ) -> Result<NodeId> {
        if is_empty {
            return self.parse_empty_content_element(name, start);
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
                self.style_warning(
                    "Content MathML <csymbol> operator identity was lowered to an ordinary identifier"
                        .into(),
                );
                Ok(self.c_ident(t.trim()))
            },
            "set" => self.parse_container("set", "{", "}"),
            "list" => self.parse_container("list", "[", "]"),
            "interval" => self.parse_interval(start),
            "vector" => self.parse_container("vector", "(", ")"),
            "matrix" => self.parse_matrix(),
            "matrixrow" => self.parse_container("matrixrow", "(", ")"),
            "piecewise" => self.parse_piecewise(),
            "lambda" => self.parse_lambda(),
            "bind" => self.parse_bind(),
            "share" => {
                self.style_warning(
                    "Content MathML <share> references are not represented; the referenced expression was lowered inline"
                        .into(),
                );
                self.parse_child_id("share")
            },
            "declare" => {
                self.style_warning(
                    "Content MathML <declare> metadata is not represented and was dropped".into(),
                );
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

    /// Lower an empty Content MathML element without reading another XML
    /// event.  Empty qualifiers and containers are complete elements, so a
    /// following sibling must remain available to their parent `<apply>` or
    /// container parser.
    fn parse_empty_content_element(
        &mut self,
        name: &str,
        start: &quick_xml::events::BytesStart<'_>,
    ) -> Result<NodeId> {
        if let Some((text, kind)) = content_constant(name) {
            return Ok(self.c_sym(text, kind));
        }
        match name {
            "apply"
            | "reln"
            | "fn"
            | "bvar"
            | "lowlimit"
            | "uplimit"
            | "degree"
            | "logbase"
            | "condition"
            | "domainofapplication"
            | "momentabout"
            | "piece"
            | "otherwise"
            | "declare"
            | "sep" => Ok(self.empty_row()),
            "bind" => {
                self.style_warning(
                    "Content MathML empty <bind> has no visible binding body".into(),
                );
                Ok(self.empty_row())
            },
            "share" => {
                self.style_warning("Content MathML empty <share> reference was dropped".into());
                Ok(self.empty_row())
            },
            "ci" => Ok(self.c_ident("")),
            "cn" => Ok(self.c_num("")),
            "csymbol" => {
                self.style_warning(
                    "Content MathML <csymbol> operator identity was lowered to an empty identifier"
                        .into(),
                );
                Ok(self.c_ident(""))
            },
            "set" => Ok(self.c_fenced("{", "}", &[])),
            "list" => Ok(self.c_fenced("[", "]", &[])),
            "interval" => {
                let (open, close) = self.interval_delimiters(start);
                Ok(self.c_fenced(open, close, &[]))
            },
            "vector" | "matrixrow" => Ok(self.c_fenced("(", ")", &[])),
            "matrix" => Ok(self.empty_matrix()),
            "piecewise" => Ok(self.empty_piecewise()),
            "lambda" => Ok(self.empty_lambda()),
            // A bare operator token outside `<apply>` is still a named label.
            _ => Ok(self.op_label(name)),
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
                Event::CData(e) => {
                    text.push_str(e.xml10_content().as_ref());
                },
                Event::GeneralRef(e) => text.push_str(&super::resolve_general_ref(&e)),
                Event::Start(e) => {
                    let n = e.local_name().into_inner();
                    let id = self.parse_element_by_name(n, &e)?;
                    kids.push(id);
                },
                Event::Empty(e) => {
                    let n = e.local_name().into_inner();
                    if n == "mglyph"
                        && let Some(alt) = get_attr(&e, "alt")
                    {
                        text.push_str(&alt);
                    } else {
                        let id = self.parse_empty_element(n, &e)?;
                        kids.push(id);
                    }
                },
                Event::End(e) if e.local_name().into_inner() == "ci" => {
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
            let span = self.arena.push_children_deferred(&[id]);
            return Ok(self
                .arena
                .push_node(CompactNode::Styled(TextStyle::Bold, span)));
        }
        Ok(id)
    }

    fn parse_cn(&mut self, start: &quick_xml::events::BytesStart<'_>) -> Result<NodeId> {
        let ty = get_attr(start, "type").unwrap_or_default();
        let base = get_attr(start, "base");
        // Collect text parts separated by <sep/>.
        let mut parts: Vec<String> = vec![String::new()];
        loop {
            match self.reader_read_event()? {
                Event::Text(e) => {
                    let t = e.as_ref();
                    parts.last_mut().unwrap().push_str(t);
                },
                Event::CData(e) => {
                    parts
                        .last_mut()
                        .unwrap()
                        .push_str(e.xml10_content().as_ref());
                },
                Event::GeneralRef(e) => {
                    parts
                        .last_mut()
                        .unwrap()
                        .push_str(&super::resolve_general_ref(&e));
                },
                Event::Empty(e) | Event::Start(e) if e.local_name().into_inner() == "sep" => {
                    parts.push(String::new());
                },
                Event::End(e) => {
                    let n = e.local_name().into_inner();
                    if n == "cn" {
                        break;
                    }
                },
                Event::Eof => break,
                _ => {},
            }
        }
        let parts: Vec<String> = parts.iter().map(|s| s.trim().to_owned()).collect();
        let expected_parts = match ty.as_str() {
            "rational" | "complex-cartesian" | "complex-polar" | "e-notation" | "enotation" => {
                Some(2)
            },
            "" | "integer" | "real" => Some(1),
            _ => None,
        };
        let separator_arity_ok = expected_parts.is_none_or(|expected| parts.len() == expected);
        if let Some(expected) = expected_parts
            && parts.len() != expected
        {
            self.style_warning(format!(
                "Content MathML <cn type=\"{}\"> expects {} separated part{}; got {}",
                if ty.is_empty() { "number" } else { ty.as_str() },
                expected,
                if expected == 1 { "" } else { "s" },
                parts.len()
            ));
        }
        if let Some(base) = base.as_deref() {
            let radix = base.parse::<u32>().ok();
            let valid_radix = radix.is_some_and(|value| (2..=36).contains(&value));
            let digits = parts.join("");
            let decimal_type = matches!(
                ty.as_str(),
                "" | "integer" | "real" | "rational" | "e-notation" | "enotation"
            );
            let valid_decimal_integer =
                matches!(ty.as_str(), "" | "integer") && valid_based_integer_literal(&digits, 10);
            let valid_nondecimal_integer = matches!(ty.as_str(), "" | "integer")
                && radix.is_some_and(|value| valid_based_integer_literal(&digits, value));
            if decimal_type
                && valid_radix
                && separator_arity_ok
                && ((base == "10" && (ty == "real" || valid_decimal_integer))
                    || (base != "10" && valid_nondecimal_integer))
            {
                if base == "10" {
                    return Ok(self.c_num(&digits));
                }
                // A base-qualified integer has no dedicated AST node. A
                // visible radix subscript preserves the authored base and
                // keeps the value together as one mathematical token.
                let value = self.c_text(&digits);
                let radix = self.c_num(base);
                return Ok(self.c_sub(value, radix));
            }
            if base != "10" || !decimal_type || !valid_radix {
                self.style_warning(format!(
                    "Content MathML <cn type=\"{}\" base=\"{}\"> has no faithful numeric-base representation",
                    if ty.is_empty() { "unknown" } else { ty.as_str() },
                    base
                ));
            }
        }
        match ty.as_str() {
            "rational" if parts.len() == 2 => {
                let n = self.c_num(&parts[0]);
                let d = self.c_num(&parts[1]);
                Ok(self.c_frac(n, d))
            },
            "complex-cartesian" if parts.len() == 2 => {
                let re = self.c_num(&parts[0]);
                let plus = self.c_sym("+", SymbolKind::BinaryOp);
                let im = self.c_num(&parts[1]);
                let i = self.c_ident("\u{2148}");
                let row = [re, plus, im, i];
                Ok(self.c_row(&row))
            },
            "complex-polar" if parts.len() == 2 => {
                let r = self.c_num(&parts[0]);
                let theta = self.c_num(&parts[1]);
                // Keep the modulus/argument boundary visible. Adjacent
                // numbers are ambiguous and were previously read back as
                // two unrelated operands.
                let angle = self.c_sym("\u{2220}", SymbolKind::BinaryOp);
                let row = [r, angle, theta];
                Ok(self.c_row(&row))
            },
            "e-notation" | "enotation" if parts.len() == 2 => {
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
                    let n = e.local_name().into_inner();
                    self.collect_apply_child(n, &e, false, &mut acc)?;
                },
                Event::Empty(e) => {
                    let n = e.local_name().into_inner();
                    self.collect_apply_child(n, &e, true, &mut acc)?;
                },
                Event::Text(e) => {
                    let t = e.as_ref();
                    if !t.trim().is_empty() {
                        let id = self.classify_text_compact(t);
                        acc.args.push(id);
                    }
                },
                Event::End(e) if e.local_name().into_inner() == tag => {
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
                let (v, degree) = if is_empty {
                    (self.empty_row(), None)
                } else {
                    self.parse_bvar()?
                };
                acc.bvars.push(v);
                acc.bvar_degrees.push(degree);
            },
            "lowlimit" => acc.lowlimit = Some(self.parse_qualifier(name, is_empty)?),
            "uplimit" => acc.uplimit = Some(self.parse_qualifier(name, is_empty)?),
            "degree" => acc.degree = Some(self.parse_qualifier(name, is_empty)?),
            "logbase" => acc.logbase = Some(self.parse_qualifier(name, is_empty)?),
            "condition" => acc.condition = Some(self.parse_qualifier(name, is_empty)?),
            "domainofapplication" | "momentabout" => {
                acc.domain = Some(self.parse_qualifier(name, is_empty)?);
            },
            "codomain" | "image" => {
                self.style_warning(format!(
                    "Content MathML qualifier <{name}> is not represented and was dropped"
                ));
                let _ = self.parse_qualifier(name, is_empty)?;
            },
            "interval" => {
                acc.interval = Some(if is_empty {
                    let (open, close) = self.interval_delimiters(start);
                    self.c_fenced(open, close, &[])
                } else {
                    self.parse_interval(start)?
                });
            },
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

    fn parse_qualifier(&mut self, name: &str, is_empty: bool) -> Result<NodeId> {
        if is_empty {
            Ok(self.empty_row())
        } else {
            self.parse_child_id(name)
        }
    }

    /// Read the head operator of an apply (first element child).
    fn read_content_operator(&mut self) -> Result<Option<COp>> {
        loop {
            match self.reader_read_event()? {
                Event::Start(e) => {
                    let name = e.local_name().into_inner();
                    if is_op_token(name) {
                        self.skip_element(name)?;
                        return Ok(Some(COp::Token(name.to_owned())));
                    }
                    let id = self.parse_element_by_name(name, &e)?;
                    return Ok(Some(COp::Func(id)));
                },
                Event::Empty(e) => {
                    let name = e.local_name().into_inner();
                    if is_op_token(name) {
                        return Ok(Some(COp::Token(name.to_owned())));
                    }
                    let id = self.parse_empty_element(name, &e)?;
                    return Ok(Some(COp::Func(id)));
                },
                Event::Text(_) => continue,
                Event::End(_) | Event::Eof => return Ok(None),
                _ => continue,
            }
        }
    }

    /// `<bvar>` may contain a variable and an optional `<degree>`.
    fn parse_bvar(&mut self) -> Result<(NodeId, Option<NodeId>)> {
        let mut var: Option<NodeId> = None;
        let mut degree: Option<NodeId> = None;
        loop {
            match self.reader_read_event()? {
                Event::Start(e) => {
                    let n = e.local_name().into_inner();
                    if n == "degree" {
                        degree = Some(self.parse_child_id("degree")?);
                    } else {
                        let id = self.parse_element_by_name(n, &e)?;
                        if var.is_none() {
                            var = Some(id);
                        }
                    }
                },
                Event::Empty(e) => {
                    let n = e.local_name().into_inner();
                    if n == "degree" {
                        degree = Some(self.empty_row());
                    } else {
                        let id = self.parse_empty_element(n, &e)?;
                        if var.is_none() {
                            var = Some(id);
                        }
                    }
                },
                Event::Text(e) => {
                    let t = e.as_ref();
                    if !t.trim().is_empty() && var.is_none() {
                        var = Some(self.classify_text_compact(t));
                    }
                },
                Event::CData(e) => {
                    let t = e.xml10_content();
                    if !t.trim().is_empty() && var.is_none() {
                        var = Some(self.classify_text_compact(t.as_ref()));
                    }
                },
                Event::GeneralRef(e) => {
                    let t = super::resolve_general_ref(&e);
                    if !t.trim().is_empty() && var.is_none() {
                        var = Some(self.classify_text_compact(&t));
                    }
                },
                Event::End(e) if e.local_name().into_inner() == "bvar" => {
                    break;
                },
                Event::Eof => break,
                _ => {},
            }
        }
        let var = var.unwrap_or_else(|| self.empty_row());
        // An empty `<degree/>` carries no useful order.  Keep non-empty
        // degrees alongside the variable so `diff`/`partialdiff` can use
        // both the MathML bvar form and the sibling qualifier form.
        let degree = degree.filter(|&id| !self.is_empty_node(id));
        Ok((var, degree))
    }

    // ── containers ─────────────────────────────────────────────────────
    fn interval_delimiters(
        &mut self,
        start: &quick_xml::events::BytesStart<'_>,
    ) -> (&'static str, &'static str) {
        let closure = get_attr(start, "closure").unwrap_or_else(|| "closed".to_owned());
        match closure.trim().to_ascii_lowercase().as_str() {
            "open" => ("(", ")"),
            "open-closed" => ("(", "]"),
            "closed-open" => ("[", ")"),
            "closed" | "closed-closed" => ("[", "]"),
            _ => {
                self.style_warning(format!(
                    "Content MathML interval closure {closure:?} is not representable; using a closed interval"
                ));
                ("[", "]")
            },
        }
    }

    fn parse_interval(&mut self, start: &quick_xml::events::BytesStart<'_>) -> Result<NodeId> {
        let (open, close) = self.interval_delimiters(start);
        self.parse_container("interval", open, close)
    }

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

    fn empty_matrix(&mut self) -> NodeId {
        let col_aligns = self.arena.push_children_deferred(&[]);
        let cells = self.arena.push_children_deferred(&[]);
        let inner = self.arena.push_node(CompactNode::Array {
            col_aligns,
            cells,
            num_cols: 0,
            arraystretch_pct: 100,
            add_jot: false,
            array_style: false,
            col_seps: 0,
            row_seps: 0,
        });
        self.c_fenced("(", ")", &[inner])
    }

    fn empty_piecewise(&mut self) -> NodeId {
        let col_aligns = [
            self.arena
                .push_node(CompactNode::ColAlign(crate::ast::ColumnAlign::Left)),
            self.arena
                .push_node(CompactNode::ColAlign(crate::ast::ColumnAlign::Left)),
        ];
        let col_aligns = self.arena.push_children_deferred(&col_aligns);
        let cells = self.arena.push_children_deferred(&[]);
        let inner = self.arena.push_node(CompactNode::Array {
            col_aligns,
            cells,
            num_cols: 2,
            arraystretch_pct: 100,
            add_jot: false,
            array_style: false,
            col_seps: 0,
            row_seps: 0,
        });
        let open = self.arena.push_str("{");
        let close = self.arena.push_str("");
        let content = self.arena.push_children_deferred(&[inner]);
        self.arena.push_node(CompactNode::Fenced {
            open,
            close,
            content,
        })
    }

    fn empty_lambda(&mut self) -> NodeId {
        let lambda = self.c_sym("\u{03BB}", SymbolKind::Alphabetic);
        let dot = self.c_sym(".", SymbolKind::Punctuation);
        self.c_row(&[lambda, dot])
    }

    fn parse_matrix(&mut self) -> Result<NodeId> {
        self.bump_depth("matrix")?;
        // Each child should be a matrixrow → collect rows of cells.
        let mut rows: Vec<Vec<NodeId>> = Vec::new();
        let mut max_columns = 0usize;
        loop {
            match self.reader_read_event()? {
                Event::Start(e) => {
                    let n = e.local_name().into_inner();
                    if n == "matrixrow" {
                        let row_count = rows
                            .len()
                            .checked_add(1)
                            .ok_or_else(super::table_length_error)?;
                        if row_count > super::AstArena::MAX_CHILDREN {
                            return Err(super::table_length_error());
                        }
                        let max_row_width = super::AstArena::MAX_CHILDREN / row_count;
                        if max_columns > max_row_width {
                            return Err(super::table_length_error());
                        }
                        let cells = self.parse_children_bounded("matrixrow", max_row_width)?;
                        max_columns = max_columns.max(cells.len());
                        if max_columns > max_row_width {
                            return Err(super::table_length_error());
                        }
                        rows.push(cells.to_vec());
                    } else {
                        let _ = self.parse_element_by_name(n, &e)?;
                    }
                },
                Event::Empty(e) => {
                    if e.local_name().into_inner() == "matrixrow" {
                        let row_count = rows
                            .len()
                            .checked_add(1)
                            .ok_or_else(super::table_length_error)?;
                        if row_count > super::AstArena::MAX_CHILDREN
                            || max_columns > super::AstArena::MAX_CHILDREN / row_count
                        {
                            return Err(super::table_length_error());
                        }
                        rows.push(Vec::new());
                    }
                },
                Event::End(e) if e.local_name().into_inner() == "matrix" => {
                    break;
                },
                Event::Eof => break,
                _ => {},
            }
        }
        self.unbump_depth();
        let num_cols = rows.iter().map(Vec::len).max().unwrap_or(0);
        let num_cols = u16::try_from(num_cols).map_err(|_| super::table_length_error())?;
        let mut cells: Vec<NodeId> = Vec::new();
        for r in &rows {
            for c in 0..num_cols as usize {
                let cell = r.get(c).copied().unwrap_or_else(|| self.empty_row());
                let span = self.arena.push_children_deferred(&[cell]);
                cells.push(self.arena.push_node(CompactNode::Row(span)));
            }
        }
        let col_aligns: SmallVec<[NodeId; 8]> = (0..num_cols)
            .map(|_| {
                self.arena
                    .push_node(CompactNode::ColAlign(crate::ast::ColumnAlign::Center))
            })
            .collect();
        let col_aligns = self.arena.push_children_deferred(&col_aligns);
        let cells = self.arena.push_children_deferred(&cells);
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
                    let n = e.local_name().into_inner();
                    match n {
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
                            let _ = self.parse_element_by_name(n, &e)?;
                        },
                    }
                },
                Event::Empty(e) => match e.local_name().into_inner() {
                    "piece" => rows.push((self.empty_row(), self.empty_row())),
                    "otherwise" => rows.push((self.empty_row(), self.c_text("otherwise"))),
                    _ => {},
                },
                Event::End(e) if e.local_name().into_inner() == "piecewise" => {
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
                let span = self.arena.push_children_deferred(&[cell]);
                cells.push(self.arena.push_node(CompactNode::Row(span)));
            }
        }
        let col_aligns: SmallVec<[NodeId; 8]> = (0..num_cols)
            .map(|_| {
                self.arena
                    .push_node(CompactNode::ColAlign(crate::ast::ColumnAlign::Left))
            })
            .collect();
        let col_aligns = self.arena.push_children_deferred(&col_aligns);
        let cells = self.arena.push_children_deferred(&cells);
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
        let content = self.arena.push_children_deferred(&[inner]);
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
                    let n = e.local_name().into_inner();
                    if n == "bvar" {
                        let (var, _) = self.parse_bvar()?;
                        bvars.push(var);
                    } else {
                        let id = self.parse_element_by_name(n, &e)?;
                        if body.is_none() {
                            body = Some(id);
                        }
                    }
                },
                Event::Empty(e) => {
                    let n = e.local_name().into_inner();
                    if n == "bvar" {
                        bvars.push(self.empty_row());
                    } else {
                        let id = self.parse_empty_element(n, &e)?;
                        if body.is_none() {
                            body = Some(id);
                        }
                    }
                },
                Event::End(e) if e.local_name().into_inner() == "lambda" => {
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

    /// Lower a Content MathML `<bind>` to a visible function-like form while
    /// reporting that binding semantics and declaration scope are outside the
    /// presentation AST. Keeping the operator, binders, and body visible is
    /// preferable to silently returning only the body.
    fn parse_bind(&mut self) -> Result<NodeId> {
        self.bump_depth("bind")?;
        let mut head: Option<NodeId> = None;
        let mut bvars: Vec<NodeId> = Vec::new();
        let mut body: Option<NodeId> = None;
        loop {
            match self.reader_read_event()? {
                Event::Start(e) => {
                    let name = e.local_name().into_inner();
                    if name == "bvar" {
                        let (var, _) = self.parse_bvar()?;
                        bvars.push(var);
                    } else {
                        let id = self.parse_element_by_name(name, &e)?;
                        if head.is_none() {
                            head = Some(id);
                        } else if body.is_none() {
                            body = Some(id);
                        }
                    }
                },
                Event::Empty(e) => {
                    let name = e.local_name().into_inner();
                    if name == "bvar" {
                        bvars.push(self.empty_row());
                    } else {
                        let id = self.parse_empty_element(name, &e)?;
                        if head.is_none() {
                            head = Some(id);
                        } else if body.is_none() {
                            body = Some(id);
                        }
                    }
                },
                Event::End(e) if e.local_name().into_inner() == "bind" => break,
                Event::Eof => break,
                _ => {},
            }
        }
        self.unbump_depth();
        self.style_warning(
            "Content MathML <bind> binding scope was lowered to a visible presentation form".into(),
        );
        let head = head.unwrap_or_else(|| self.empty_row());
        let binders = if bvars.is_empty() {
            self.empty_row()
        } else {
            self.c_fenced("(", ")", &bvars)
        };
        let body = body.unwrap_or_else(|| self.empty_row());
        let colon = self.c_sym(":", SymbolKind::Punctuation);
        Ok(self.c_row(&[head, binders, colon, body]))
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
        let args = &a.args;
        if matches!(
            token,
            "selector" | "domain" | "codomain" | "image" | "moment" | "momentabout"
        ) {
            self.style_warning(format!(
                "Content MathML operator <{token}> was lowered generically; its semantic metadata is not represented"
            ));
        }
        if token == "minus" && args.len() == 1 {
            let m = self.c_sym("\u{2212}", SymbolKind::BinaryOp);
            let arg = self.parenthesize(args[0]);
            let row = [m, arg];
            return self.c_row(&row);
        }
        if let Some(spec) = Self::infix_spec(token) {
            return self.join_classified_infix(args, spec);
        }
        match token {
            "divide" => {
                if args.len() >= 2 {
                    self.c_frac(args[0], args[1])
                } else {
                    self.func_arg(args)
                }
            },
            "power" if args.len() >= 2 => {
                let base_needs_group = self.row_infix(args[0]).is_some_and(|class| {
                    class == InfixClass::Unary || class.precedence() <= InfixClass::Add.precedence()
                }) || self.is_nary_node(args[0])
                    || self.is_negative_number(args[0]);
                let base = if base_needs_group {
                    self.parenthesize(args[0])
                } else {
                    args[0]
                };
                self.c_sup(base, args[1])
            },
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
        // `ℜ` and `ℑ` are letters, as `\Re` and `\Im` are; `∇` is not.
        let s = if sym.chars().all(char::is_alphabetic) {
            self.c_ident(sym)
        } else {
            self.c_sym(sym, SymbolKind::Ordinary)
        };
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

    fn combine_qualifiers(&mut self, parts: &[NodeId]) -> NodeId {
        match parts {
            [] => self.empty_row(),
            [only] => *only,
            _ => {
                let mut row: SmallVec<[NodeId; 8]> = SmallVec::new();
                for (i, &part) in parts.iter().enumerate() {
                    if i > 0 {
                        row.push(self.c_sym(",", SymbolKind::Punctuation));
                    }
                    row.push(part);
                }
                self.c_row(&row)
            },
        }
    }

    /// Lower integral-only domain qualifiers into the one lower-limit slot
    /// available in the presentation AST.  The qualifier remains visible as
    /// `[a,b]` or `x ∈ D`; when ordinary bounds are also present it is joined
    /// with a comma instead of being silently discarded.
    fn integral_qualifier(&mut self, a: &Applied) -> Option<NodeId> {
        let mut parts: SmallVec<[NodeId; 4]> = SmallVec::new();
        if let Some(interval) = a.interval {
            parts.push(interval);
        }
        if let Some(domain) = a.domain {
            let domain = if let Some(&bv) = a.bvars.first() {
                let belongs = self.c_sym("\u{2208}", SymbolKind::Relation);
                self.c_row(&[bv, belongs, domain])
            } else {
                domain
            };
            parts.push(domain);
        }
        if let Some(condition) = a.condition {
            parts.push(condition);
        }
        (!parts.is_empty()).then(|| self.combine_qualifiers(&parts))
    }

    fn integral(&mut self, a: Applied) -> NodeId {
        let qualifier = self.integral_qualifier(&a);
        let sub = match (a.lowlimit, qualifier) {
            (Some(low), Some(qualifier)) => self.combine_qualifiers(&[low, qualifier]),
            (Some(low), None) => low,
            (None, Some(qualifier)) => qualifier,
            (None, None) => self.empty_row(),
        };
        let sup = a.uplimit.unwrap_or_else(|| self.empty_row());
        let integrand = a.args.first().copied().unwrap_or_else(|| self.empty_row());
        // body = integrand d(bvar₁) d(bvar₂) …
        let mut body: SmallVec<[NodeId; 8]> = SmallVec::new();
        body.push(integrand);
        for &bv in &a.bvars {
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

    fn numeric_degree(&self, id: NodeId) -> Option<u32> {
        let CompactNode::Number(sid) = *self.arena.node(id) else {
            return None;
        };
        self.arena.str_val(sid).trim().parse().ok()
    }

    fn degree_for_bvar(&self, a: &Applied, index: usize) -> Option<NodeId> {
        let per_bvar = a.bvar_degrees.get(index).copied().flatten();
        per_bvar.or_else(|| (a.bvars.len() == 1).then_some(a.degree).flatten())
    }

    fn derivative_order(&mut self, a: &Applied) -> Option<NodeId> {
        if a.bvars.len() == 1 {
            return self.degree_for_bvar(a, 0);
        }
        if let Some(degree) = a.degree.filter(|&id| !self.is_empty_node(id)) {
            return Some(degree);
        }
        if a.bvars.len() <= 1 {
            return None;
        }
        if a.bvar_degrees.iter().all(Option::is_none) {
            // A mixed derivative has one differentiation for each bvar.
            return Some(self.c_num(a.bvars.len().to_string().as_str()));
        }

        // Sum per-bvar orders.  Numeric orders can stay a compact literal;
        // symbolic orders are kept as an explicit `n + m` superscript rather
        // than being replaced by the number of variables.
        let mut terms: SmallVec<[NodeId; 4]> = SmallVec::new();
        let mut total = 0u32;
        let mut numeric = true;
        for index in 0..a.bvars.len() {
            let degree = a
                .bvar_degrees
                .get(index)
                .copied()
                .flatten()
                .unwrap_or_else(|| self.c_num("1"));
            if let Some(value) = self.numeric_degree(degree) {
                let Some(sum) = total.checked_add(value) else {
                    // Keep the authored numeric terms and render their sum
                    // explicitly below; saturating here would silently turn
                    // a requested order into a different derivative.
                    numeric = false;
                    terms.push(degree);
                    continue;
                };
                total = sum;
            } else {
                numeric = false;
            }
            terms.push(degree);
        }
        if numeric {
            Some(self.c_num(total.to_string().as_str()))
        } else {
            let mut order: SmallVec<[NodeId; 8]> = SmallVec::new();
            for (index, term) in terms.iter().copied().enumerate() {
                if index > 0 {
                    order.push(self.c_sym("+", SymbolKind::BinaryOp));
                }
                order.push(term);
            }
            Some(self.c_row(&order))
        }
    }

    fn degree_is_one(&self, id: NodeId) -> bool {
        self.numeric_degree(id) == Some(1)
    }

    fn derivative_numerator(&mut self, marker: NodeId, order: Option<NodeId>) -> NodeId {
        match order {
            Some(order) if !self.is_empty_node(order) && !self.degree_is_one(order) => {
                self.c_sup(marker, order)
            },
            _ => marker,
        }
    }

    fn derivative_denominator(&mut self, marker: &str, a: &Applied) -> NodeId {
        let mut den: SmallVec<[NodeId; 8]> = SmallVec::new();
        for (index, &bv) in a.bvars.iter().enumerate() {
            den.push(self.c_sym(marker, SymbolKind::Ordinary));
            let variable = match self.degree_for_bvar(a, index) {
                Some(degree) if !self.is_empty_node(degree) && !self.degree_is_one(degree) => {
                    self.c_sup(bv, degree)
                },
                _ => bv,
            };
            den.push(variable);
        }
        self.c_row(&den)
    }

    fn derivative_body(&mut self, id: NodeId) -> NodeId {
        // The derivative operator applies to the complete Content operand;
        // keep products, function applications, and other compound bodies
        // together just as we already do for sums and relations.
        self.parenthesize(id)
    }

    fn derivative(&mut self, a: Applied, partial: bool) -> NodeId {
        let f = a.args.first().copied().unwrap_or_else(|| self.empty_row());
        if a.bvars.is_empty() {
            if a.degree.is_some_and(|id| !self.is_empty_node(id)) {
                self.style_warning(format!(
                    "Content MathML {} degree has no bound variable; derivative order was not represented",
                    if partial { "partialdiff" } else { "diff" }
                ));
            }
            if partial {
                return f;
            }
            // With no bound variable there is no faithful denominator.  Keep
            // the historical prime fallback rather than inventing one.
            let prime = self.c_sym("\u{2032}", SymbolKind::Ordinary);
            return self.c_sup(f, prime);
        }

        let marker = if partial {
            self.c_sym("\u{2202}", SymbolKind::Ordinary)
        } else {
            self.c_ident("d")
        };
        let order = self.derivative_order(&a);
        let numerator = self.derivative_numerator(marker, order);
        let denominator = self.derivative_denominator(if partial { "\u{2202}" } else { "d" }, &a);
        let frac = self.c_frac(numerator, denominator);
        let body = self.derivative_body(f);
        self.c_row(&[frac, body])
    }

    fn diff(&mut self, a: Applied) -> NodeId {
        self.derivative(a, false)
    }

    fn partialdiff(&mut self, a: Applied) -> NodeId {
        self.derivative(a, true)
    }
}
