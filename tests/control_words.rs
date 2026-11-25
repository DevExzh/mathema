//! A control word of the LaTeX output never takes in letters of the input.
//!
//! The leaves of the trees here are made of letters that are in no command, so
//! one inside a control word was glued to it. (Debug builds also check, in the
//! writer, that each control word of the output is one it wrote.)

use mathema::ast::compact::CompactExpr;
use mathema::ast::*;
use mathema::writer::Writer;
use mathema::writer::latex::LatexWriter;
use mathema::{Config, UndefinedCommands};

const PAYLOADS: [&str; 10] = [
    "zqj",
    "ZQ",
    "@zq",
    "\u{E9}zq",
    "\u{3A7}",
    "\u{436}zq",
    "\u{4E2D}zq",
    "\u{1D400}",
    "\u{211D}",
    "\u{301}zq",
];

/// The output's control words, as the widest lexer reads them: KaTeX's
/// `@` and the letters and marks of XeTeX and LuaTeX.
fn control_words(latex: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut chars = latex.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            continue;
        }
        let mut word = String::new();
        while let Some(&ch) = chars.peek()
            && (ch == '@' || ch == '\u{301}' || ch.is_alphabetic())
        {
            word.push(ch);
            chars.next();
        }
        if word.is_empty() {
            chars.next();
        }
        words.push(word);
    }
    words
}

#[track_caller]
fn check(latex: &str, of: &dyn std::fmt::Debug) {
    for word in control_words(latex) {
        assert!(
            word.is_ascii() && !word.contains('@') && !word.to_lowercase().contains("zq"),
            "\\{word} in {latex}\nof {of:?}"
        );
    }
}

fn check_all(nodes: &[MathNode], routes: bool) {
    let expr = MathExpr::inline(nodes.to_vec());
    for undefined in [UndefinedCommands::Escape, UndefinedCommands::Keep] {
        let config = Config::default().with_undefined_commands(undefined);
        if let Ok(latex) = LatexWriter::write(&expr, &config) {
            check(&latex, &nodes);
        }
        let Ok(compact) = CompactExpr::from_math_expr(&expr) else {
            continue;
        };
        if let Ok(latex) = LatexWriter::write_compact(&compact, &config) {
            check(&latex, &nodes);
        }
        if !routes {
            continue;
        }
        // As read back from another format.
        if let Ok(mathml) = mathema::writer::mathml::MathmlWriter::write(&expr, &config)
            && let Ok(read) = mathema::from_mathml(&mathml)
            && let Ok(latex) = read.to_latex_with(&config)
        {
            check(&latex, &nodes);
        }
        if let Ok(omml) = mathema::writer::omml::OmmlWriter::write(&expr, &config)
            && let Ok(read) = mathema::from_omml(&omml)
            && let Ok(latex) = read.to_latex_with(&config)
        {
            check(&latex, &nodes);
        }
        if let Ok(linear) = mathema::writer::unicode_math::UnicodeMathWriter::write(&expr, &config)
            && let Ok(read) = mathema::from_unicode_math(&linear)
            && let Ok(latex) = read.to_latex_with(&config)
        {
            check(&latex, &nodes);
        }
    }
}

struct Random(u64);

impl Random {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as usize % n
    }

    fn of<T: Copy>(&mut self, all: &[T]) -> T {
        all[self.below(all.len())]
    }

    fn payload(&mut self) -> String {
        self.of(&PAYLOADS).to_owned()
    }

    fn glyph(&mut self) -> String {
        self.of(&[
            "+", "\u{222B}", "\u{2211}", "\u{2264}", "\u{2A7D}", "\u{27E8}", "\u{2016}", "|", "(",
            "\u{2192}", "\u{22EF}", "\u{2026}", "\u{2020}", "\u{2062}", "", "\u{2032}", "'", ".",
            "\\", "{", "\u{211C}", "\u{2224}", "\u{B2}", "\u{338}", ":=",
        ])
        .to_owned()
    }

    fn boxed(&mut self, depth: usize) -> Box<MathNode> {
        Box::new(self.node(depth))
    }

    fn option(&mut self, depth: usize) -> Option<Box<MathNode>> {
        (self.below(2) == 0).then(|| self.boxed(depth))
    }

    fn list(&mut self, depth: usize) -> Vec<MathNode> {
        let n = self.below(4);
        (0..n).map(|_| self.node(depth)).collect()
    }

    fn rational(&mut self) -> Rational {
        Rational::new(self.of(&[0, 0, 1, -3, 5, 18]), self.of(&[18, 1, 7]))
    }

    fn leaf(&mut self) -> MathNode {
        let limits = [
            LimitStyle::Default,
            LimitStyle::AboveBelow,
            LimitStyle::SubSup,
        ];
        let kinds = [
            SymbolKind::Ordinary,
            SymbolKind::LargeOp,
            SymbolKind::BinaryOp,
            SymbolKind::Relation,
            SymbolKind::Open,
            SymbolKind::Close,
            SymbolKind::Punctuation,
            SymbolKind::Fence,
            SymbolKind::Alphabetic,
        ];
        match self.below(13) {
            0..=3 => MathNode::Identifier(self.payload()),
            4 => MathNode::Identifier(
                self.of(&["\u{3B1}", "\u{131}", "", " ", "\u{210E}", "\u{2102}"])
                    .into(),
            ),
            5 => MathNode::Number(self.of(&["1", "", "2.5", "zq"]).into()),
            6 => MathNode::Operator(OperatorAttr {
                name: self.of(&["sin", "lim", "zqj", "", "@zq"]).into(),
                limit_style: self.of(&limits),
            }),
            7 | 8 => MathNode::Symbol(self.of(&kinds), self.glyph()),
            9 => MathNode::Text(
                self.of(&[TextStyle::Normal, TextStyle::Bold, TextStyle::Monospace]),
                self.of(&["", "zq", "\\zq", " zq", "~^zq"]).into(),
            ),
            10 => MathNode::Space(self.rational()),
            11 => MathNode::ExplicitSpace {
                width: self.rational(),
                basis: self.of(&[SpaceBasis::TextEm, SpaceBasis::MathMu, SpaceBasis::Physical]),
            },
            _ => MathNode::Symbol(self.of(&kinds), self.payload()),
        }
    }

    fn node(&mut self, depth: usize) -> MathNode {
        if depth == 0 || self.below(4) == 0 {
            return self.leaf();
        }
        let d = depth - 1;
        let limits = [
            LimitStyle::Default,
            LimitStyle::AboveBelow,
            LimitStyle::SubSup,
        ];
        let styles = [
            TextStyle::Normal,
            TextStyle::Bold,
            TextStyle::Italic,
            TextStyle::Script,
            TextStyle::DoubleStruck,
        ];
        let classes = [
            MathAtomType::Ord,
            MathAtomType::Op,
            MathAtomType::Bin,
            MathAtomType::Rel,
            MathAtomType::Open,
            MathAtomType::Close,
            MathAtomType::Punct,
            MathAtomType::Inner,
        ];
        match self.below(32) {
            0 | 1 => MathNode::Row(self.list(d)),
            2 => MathNode::Fenced {
                open: self.glyph(),
                close: self.glyph(),
                content: vec![
                    FencedItem::Element(self.node(d)),
                    FencedItem::Middle(self.glyph()),
                    FencedItem::Element(self.node(d)),
                ],
            },
            3 => MathNode::Subscript {
                base: self.boxed(d),
                sub: self.boxed(d),
            },
            4 => MathNode::Superscript {
                base: self.boxed(d),
                sup: self.boxed(d),
            },
            5 => MathNode::SubSup {
                base: self.boxed(d),
                sub: self.boxed(d),
                sup: self.boxed(d),
            },
            6..=8 => MathNode::PreScripts {
                base: self.boxed(d),
                pre_sub: self.option(d),
                pre_sup: self.option(d),
            },
            9 => MathNode::Overscript {
                base: self.boxed(d),
                over: self.boxed(d),
                movable_limits: self.below(2) == 0,
            },
            10 => MathNode::Underscript {
                base: self.boxed(d),
                under: self.boxed(d),
                movable_limits: self.below(2) == 0,
            },
            11 => MathNode::UnderOver {
                base: self.boxed(d),
                under: self.boxed(d),
                over: self.boxed(d),
                movable_limits: self.below(2) == 0,
            },
            12 => MathNode::Fraction {
                kind: self.of(&[
                    FractionKind::Normal,
                    FractionKind::Inline,
                    FractionKind::NoBar,
                    FractionKind::Skewed,
                ]),
                num: self.boxed(d),
                den: self.boxed(d),
            },
            13 => MathNode::Sqrt(self.boxed(d)),
            14 => MathNode::Root {
                index: self.boxed(d),
                radicand: self.boxed(d),
            },
            15..=17 => MathNode::Nary {
                op: self.of(&['\u{222B}', '\u{2211}', '\u{2A0C}', '\u{22C3}', '\u{2A0B}']),
                sub: self.option(d),
                sup: self.option(d),
                body: self.boxed(d),
                limits: self.of(&limits),
            },
            18 => MathNode::Accent {
                base: self.boxed(d),
                accent: self.of(&['\u{302}', '\u{20D7}', '\u{30B}', '\u{23DE}']),
                position: self.of(&[AccentPosition::Above, AccentPosition::Below]),
                wide: self.below(2) == 0,
            },
            19 => MathNode::Phantom(
                self.boxed(d),
                self.of(&[PhantomKind::Full, PhantomKind::SmashBoth]),
            ),
            20 => MathNode::Boxed {
                content: self.boxed(d),
                bg_color: (self.below(2) == 0).then(|| "red".into()),
                border_color: (self.below(2) == 0).then(|| "blue".into()),
            },
            21 => MathNode::Cancel {
                stroke: self.of(&[StrokeKind::Forward, StrokeKind::Cross]),
                content: self.boxed(d),
            },
            22 => MathNode::Scaled {
                factor: self.of(&[
                    Rational::new(6, 5),
                    Rational::new(9, 5),
                    Rational::new(12, 5),
                    Rational::new(3, 1),
                ]),
                content: self.boxed(d),
            },
            23 => MathNode::Styled(self.of(&styles), self.list(d)),
            24 => MathNode::StyleSwitch(self.below(5) as u8),
            25 => MathNode::SizeSwitch(self.below(11) as u8),
            26 | 27 => MathNode::MathAtom {
                atom_type: self.of(&classes),
                content: self.boxed(d),
            },
            28 => MathNode::Color {
                color: self.of(&["red", "zq", "#f00"]).into(),
                content: self.boxed(d),
            },
            29 => MathNode::StructuredOperator {
                content: self.boxed(d),
                form: self.of(&[
                    OperatorNameForm::Unstarred,
                    OperatorNameForm::Starred(LimitStyle::AboveBelow),
                    OperatorNameForm::Starred(LimitStyle::SubSup),
                ]),
            },
            30 => MathNode::UndefinedCommand {
                name: self.of(&["\\undefinedname", "\\input", "zq"]).into(),
                arguments: (0..self.below(3)).map(|_| self.list(d)).collect(),
            },
            _ => MathNode::Array {
                columns: vec![ColumnAlign::Center],
                rows: vec![vec![self.list(d)], vec![self.list(d)]],
                arraystretch: None,
                add_jot: false,
            },
        }
    }
}

#[test]
fn random_trees_write_only_the_writers_control_words() {
    let mut random = Random(0x9E37_79B9_7F4A_7C15);
    for _ in 0..4000 {
        let n = 1 + random.below(4);
        let nodes: Vec<_> = (0..n).map(|_| random.node(3)).collect();
        check_all(&nodes, true);
    }
}

fn boxed(node: &MathNode) -> Box<MathNode> {
    Box::new(node.clone())
}

/// Every command, nothing after it, then letters, in every slot.
#[test]
fn letters_after_nothing_after_a_command_are_not_part_of_it() {
    let identifier = |s: &str| MathNode::Identifier(s.into());
    let mut commands: Vec<MathNode> = Vec::new();
    for op in "∫∬∭∮∯∰∑∏∐⋃⋂⋁⋀⨀⨁⨂⨄⨆⨌⨋".chars() {
        commands.push(MathNode::Symbol(SymbolKind::LargeOp, op.into()));
        for limits in [
            LimitStyle::Default,
            LimitStyle::AboveBelow,
            LimitStyle::SubSup,
        ] {
            commands.push(MathNode::Nary {
                op,
                sub: None,
                sup: None,
                body: Box::new(MathNode::Row(vec![])),
                limits,
            });
        }
    }
    for (kind, glyph) in [
        (SymbolKind::Relation, "≤"),
        (SymbolKind::BinaryOp, "⋅"),
        (SymbolKind::Ordinary, "…"),
        (SymbolKind::Ordinary, "‖"),
        (SymbolKind::Open, "⟨"),
        (SymbolKind::Close, "⟩"),
        (SymbolKind::Open, "|"),
        (SymbolKind::Punctuation, "."),
        (SymbolKind::BinaryOp, "&"),
        (SymbolKind::Ordinary, "\u{2032}"),
    ] {
        commands.push(MathNode::Symbol(kind, glyph.into()));
        commands.push(MathNode::Scaled {
            factor: Rational::new(6, 5),
            content: Box::new(MathNode::Symbol(kind, glyph.into())),
        });
    }
    for name in ["sin", "lim", "max", "argmax"] {
        for limit_style in [
            LimitStyle::Default,
            LimitStyle::AboveBelow,
            LimitStyle::SubSup,
        ] {
            commands.push(MathNode::Operator(OperatorAttr {
                name: name.into(),
                limit_style,
            }));
        }
    }
    commands.extend(["α", "ℜ", "ı", "ℵ"].map(identifier));
    commands.extend(
        [(1, 18), (-3, 18), (-4, 18), (3, 18), (1, 1), (2, 1), (1, 7)]
            .map(|(n, d)| MathNode::Space(Rational::new(n, d))),
    );
    commands.extend((0..4).map(MathNode::StyleSwitch));
    commands.extend((0..10).map(MathNode::SizeSwitch));
    commands.push(MathNode::UndefinedCommand {
        name: "\\undefinedname".into(),
        arguments: vec![],
    });

    let nothings = [
        MathNode::Row(vec![]),
        MathNode::Space(Rational::zero()),
        MathNode::ExplicitSpace {
            width: Rational::zero(),
            basis: SpaceBasis::TextEm,
        },
        MathNode::Text(TextStyle::Normal, String::new()),
        identifier(""),
        MathNode::Number(String::new()),
        MathNode::Symbol(SymbolKind::Ordinary, String::new()),
        MathNode::Styled(TextStyle::Normal, vec![]),
    ];
    for (i, command) in commands.iter().enumerate() {
        for (j, nothing) in nothings.iter().enumerate() {
            // One payload per pair, in turn, and other formats for some.
            for payload in [PAYLOADS[0], PAYLOADS[1 + (i + j) % 9]] {
                let routes = payload == PAYLOADS[0] && (i + j) % 4 == 0;
                let check_all = |nodes: &[MathNode]| check_all(nodes, routes);
                let letters = identifier(payload);
                let scripted = [
                    MathNode::Superscript {
                        base: boxed(command),
                        sup: boxed(nothing),
                    },
                    MathNode::Subscript {
                        base: boxed(command),
                        sub: boxed(nothing),
                    },
                    MathNode::SubSup {
                        base: boxed(command),
                        sub: boxed(nothing),
                        sup: boxed(nothing),
                    },
                    MathNode::Overscript {
                        base: boxed(command),
                        over: boxed(nothing),
                        movable_limits: true,
                    },
                    MathNode::PreScripts {
                        base: boxed(command),
                        pre_sub: Some(boxed(nothing)),
                        pre_sup: None,
                    },
                ];
                check_all(&[command.clone(), letters.clone()]);
                check_all(&[command.clone(), nothing.clone(), letters.clone()]);
                for first in scripted {
                    check_all(&[first.clone(), letters.clone()]);
                    // The base of a leading script, and everything before it.
                    for base in [
                        MathNode::Superscript {
                            base: boxed(&letters),
                            sup: boxed(nothing),
                        },
                        letters.clone(),
                    ] {
                        check_all(&[MathNode::PreScripts {
                            base: boxed(&base),
                            pre_sub: Some(boxed(command)),
                            pre_sup: Some(boxed(&first)),
                        }]);
                        check_all(&[MathNode::Row(vec![first.clone(), base])]);
                    }
                }
            }
        }
    }
}

/// `\interactionmode`, `\input`, … from an operator and the letters of the base
/// of its leading script.
#[test]
fn an_operator_as_a_leading_script_is_not_glued_to_its_base() {
    for (glyph, command, letters) in [
        ("&#x222B;", r"\int", "eractionmode"),
        ("&#x222B;", r"\int", "ertext"),
        ("&#x2211;", r"\sum", "mary"),
        ("&#x220F;", r"\prod", "uct"),
        ("&#x22C3;", r"\bigcup", "x"),
    ] {
        for width in ["<mspace width='0'/>", "<mrow/>", "<none/>"] {
            let mathml = format!(
                "<math><mmultiscripts><mi>{letters}</mi><none/>{width}<mprescripts/>\
                 <mo>{glyph}</mo><none/></mmultiscripts></math>"
            );
            let latex = mathema::from_mathml(&mathml).unwrap().to_latex().unwrap();
            let glued = format!("{command}{letters}");
            assert!(!latex.contains(&glued), "{latex}");
        }
    }
    let read = mathema::from_mathml(
        "<math><mmultiscripts><mi>eractionmode</mi><none/><mspace width='0'/><mprescripts/>\
         <mo>&#x222B;</mo><none/></mmultiscripts></math>",
    )
    .unwrap();
    assert_eq!(read.to_latex().unwrap(), r"{{}_\int eractionmode}");
}
