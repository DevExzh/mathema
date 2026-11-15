//! OMML → AST → OMML round trips for Word and PowerPoint equations, plus a
//! structural check of the writer output against the content models of the
//! OMML schema (ECMA-376 Part 1, Annex A, `shared-math.xsd`).

use std::collections::HashMap;

use mathema::ast::{ColumnAlign, MathAtomType, MathNode, TextStyle};
use mathema::reader::Reader;
use mathema::reader::omml::OmmlReader;
use mathema::writer::omml::OmmlWriter;
use mathema::{
    Config, DiagnosticKind, OmmlJustification, OmmlOptions, OmmlParagraph, OmmlRunProperties,
};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/omml");

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{FIXTURES}/{name}")).unwrap()
}

fn dialects() -> [OmmlOptions; 3] {
    [
        OmmlOptions::default(),
        OmmlOptions::default().with_run_properties(OmmlRunProperties::WordprocessingMl),
        OmmlOptions::default().with_run_properties(OmmlRunProperties::DrawingMl),
    ]
}

fn nodes(para: &OmmlParagraph) -> Vec<Vec<MathNode>> {
    para.equations
        .iter()
        .map(|e| e.to_math_expr().nodes)
        .collect()
}

/// Read, write, read again and write again: the AST and the markup must be
/// stable after the first pass, and every output must be schema-shaped.
fn round_trip(name: &str, options: &OmmlOptions) -> (OmmlParagraph, String) {
    let config = Config::default();
    let source = fixture(name);
    let first = OmmlReader::parse_paragraph(&source, &config).unwrap();
    let (out1, _) = OmmlWriter::write_paragraph(&first, &config, options).unwrap();
    check_schema(&out1);
    let second = OmmlReader::parse_paragraph(&out1, &config).unwrap();
    assert_eq!(nodes(&first), nodes(&second), "{name}: AST changed\n{out1}");
    assert_eq!(first.justification, second.justification);
    let (out2, _) = OmmlWriter::write_paragraph(&second, &config, options).unwrap();
    assert_eq!(out1, out2, "{name}: OMML not stable");
    (first, out1)
}

fn find<'a>(node: &'a MathNode, pred: &dyn Fn(&MathNode) -> bool) -> Option<&'a MathNode> {
    if pred(node) {
        return Some(node);
    }
    let children: Vec<&MathNode> = match node {
        MathNode::Row(c) | MathNode::Styled(_, c) => c.iter().collect(),
        MathNode::Fenced { content, .. } => content
            .iter()
            .filter_map(|i| match i {
                mathema::ast::FencedItem::Element(n) => Some(n),
                _ => None,
            })
            .collect(),
        MathNode::Subscript { base, sub } => vec![base, sub],
        MathNode::Superscript { base, sup } => vec![base, sup],
        MathNode::SubSup { base, sub, sup } => vec![base, sub, sup],
        MathNode::Underscript { base, under, .. } => vec![base, under],
        MathNode::Overscript { base, over, .. } => vec![base, over],
        MathNode::Fraction { num, den, .. } => vec![num, den],
        MathNode::Nary { sub, sup, body, .. } => {
            let mut v: Vec<&MathNode> = vec![body];
            v.extend(sub.as_deref());
            v.extend(sup.as_deref());
            v
        },
        MathNode::Array { rows, .. } => rows.iter().flatten().flatten().collect(),
        MathNode::MathAtom { content, .. } => vec![content],
        _ => vec![],
    };
    children.into_iter().find_map(|c| find(c, pred))
}

fn contains(eq: &[MathNode], pred: &dyn Fn(&MathNode) -> bool) -> bool {
    eq.iter().any(|n| find(n, pred).is_some())
}

#[test]
fn word_paragraph_round_trips() {
    for options in dialects() {
        let (para, out) = round_trip("word_paragraph.omml", &options);
        assert_eq!(para.justification, Some(OmmlJustification::Left));
        assert!(out.contains(r#"<m:oMathParaPr><m:jc m:val="left"/></m:oMathParaPr>"#));
        let eqs = nodes(&para);
        assert_eq!(eqs.len(), 4);

        // Cases: an equation array whose normal text keeps its spaces.
        assert!(contains(&eqs[0], &|n| matches!(
            n,
            MathNode::Array { add_jot: true, .. }
        )));
        assert!(contains(&eqs[0], &|n| *n
            == MathNode::Text(TextStyle::Normal, "  if ".into())));
        assert!(out.contains(r#"<m:t xml:space="preserve">  otherwise</m:t>"#));
        assert_eq!(out.matches("<m:eqArr>").count(), 1);

        // lim under m:func, and nothing else becomes a function.
        assert!(contains(
            &eqs[1],
            &|n| matches!(n, MathNode::Symbol(_, s) if s == "\u{2061}")
        ));
        assert_eq!(out.matches("<m:func>").count(), 2, "{out}");

        // Integral with a differential box, radical degree, vector accent.
        assert!(contains(&eqs[2], &|n| matches!(
            n,
            MathNode::MathAtom {
                atom_type: MathAtomType::Ord,
                ..
            }
        )));
        assert!(contains(&eqs[2], &|n| matches!(n, MathNode::Root { .. })));
        assert!(contains(&eqs[2], &|n| matches!(
            n,
            MathNode::Accent {
                accent: '\u{20D7}',
                ..
            }
        )));
        assert!(contains(&eqs[2], &|n| matches!(
            n,
            MathNode::Styled(TextStyle::DoubleStruck, _)
        )));
        assert!(out.contains(r#"<m:limLoc m:val="subSup"/>"#));

        // Bracketed 2 × 2 matrix.
        assert!(contains(
            &eqs[3],
            &|n| matches!(n, MathNode::Array { add_jot: false, columns, .. } if *columns == [ColumnAlign::Center, ColumnAlign::Center])
        ));
    }
}

#[test]
fn word_run_color_survives_with_wordprocessing_properties() {
    let options = OmmlOptions::default().with_run_properties(OmmlRunProperties::WordprocessingMl);
    let (_, out) = round_trip("word_paragraph.omml", &options);
    assert!(
        out.contains(r#"<m:r><w:rPr><w:color w:val="C00000"/></w:rPr><m:t>e</m:t></m:r>"#),
        "{out}"
    );

    // Without host properties the color is reported, never put in m:rPr.
    let config = Config::default();
    let para = OmmlReader::parse_paragraph(&fixture("word_paragraph.omml"), &config).unwrap();
    let (out, diags) =
        OmmlWriter::write_paragraph(&para, &config, &OmmlOptions::default()).unwrap();
    assert!(!out.contains("C00000"));
    assert!(
        diags.iter().any(|d| d.message.contains("color")),
        "{diags:?}"
    );
}

#[test]
fn powerpoint_paragraph_round_trips() {
    for options in dialects() {
        let (para, out) = round_trip("powerpoint_paragraph.omml", &options);
        assert_eq!(para.justification, Some(OmmlJustification::CenterGroup));
        let eqs = nodes(&para);
        assert_eq!(eqs.len(), 2);
        // Math alphanumerics are read as base letters with a style.
        assert!(contains(&eqs[0], &|n| *n == MathNode::Identifier("a".into())));
        assert!(contains(&eqs[0], &|n| *n == MathNode::Identifier("\u{03B8}".into())));
        assert!(contains(
            &eqs[1],
            &|n| matches!(n, MathNode::Styled(TextStyle::BoldItalic, c) if c == &[MathNode::Identifier("v".into())])
        ));
        assert!(contains(
            &eqs[1],
            &|n| matches!(n, MathNode::Styled(TextStyle::DoubleStruck, c) if c == &[MathNode::Identifier("R".into())])
        ));
        assert!(contains(&eqs[1], &|n| *n
            == MathNode::Text(TextStyle::Normal, " for all ".into())));
        assert_eq!(out.matches("<m:func>").count(), 1);
    }
    let options = OmmlOptions::default().with_run_properties(OmmlRunProperties::DrawingMl);
    let (_, out) = round_trip("powerpoint_paragraph.omml", &options);
    assert!(
        out.contains(
            r#"<a:rPr><a:solidFill><a:srgbClr val="0070C0"/></a:solidFill></a:rPr><m:t>=</m:t>"#
        ),
        "{out}"
    );
}

#[test]
fn plurimath_word_equations_round_trip() {
    for options in dialects() {
        let (para, out) = round_trip("plurimath_047_nary_eqarr.omml", &options);
        // The equation array under the sum stays an equation array; the
        // hidden upper limit (a zero-width space) is dropped with a note.
        assert!(out.contains("<m:sub><m:eqArr>"), "{out}");
        assert!(out.contains(r#"<m:supHide m:val="1"/>"#));
        assert!(
            para.diagnostics
                .iter()
                .any(|d| d.kind == DiagnosticKind::Info && d.message.contains("hidden"))
        );

        let (_, out) = round_trip("plurimath_066_delimited_eqarr.omml", &options);
        // Only begChr is given, so endChr keeps its default ")".
        assert!(
            out.contains(r#"<m:d><m:dPr><m:begChr m:val="{"/></m:dPr><m:e><m:eqArr>"#),
            "{out}"
        );

        let (_, out) = round_trip("plurimath_075_func.omml", &options);
        assert!(out.contains("<m:func><m:fName>"), "{out}");
    }
}

#[test]
fn single_expression_api_round_trips_fixtures() {
    let config = Config::default();
    for name in [
        "word_paragraph.omml",
        "powerpoint_paragraph.omml",
        "plurimath_047_nary_eqarr.omml",
        "plurimath_066_delimited_eqarr.omml",
        "plurimath_075_func.omml",
    ] {
        let first = OmmlReader::parse(&fixture(name), &config).unwrap();
        let out1 = <OmmlWriter as mathema::writer::Writer>::write(&first.expr, &config).unwrap();
        check_schema(&out1);
        let second = OmmlReader::parse(&out1, &config).unwrap();
        assert_eq!(first.expr.nodes, second.expr.nodes, "{name}\n{out1}");
    }
}

#[test]
fn latex_corpus_output_is_schema_shaped() {
    let corpus = [
        r"x^2 + y^2 = z^2",
        r"\frac{a}{b} + \dfrac{1}{2} + \tfrac{3}{4} + \binom{n}{k}",
        r"\sqrt{x} + \sqrt[3]{y}",
        r"\sum_{i=0}^{n} i^2 = \int_0^1 f(x)\,dx",
        r"\lim_{n\to\infty} \frac{1}{n} + \sin x + \log_2 n",
        r"\left( a \middle| b \middle\| c \right) \left\{ z \right.",
        r"\begin{pmatrix} a & b \\ c & d \end{pmatrix}",
        r"\begin{cases} x & x>0 \\ -x & \text{otherwise} \end{cases}",
        r"\begin{gathered} a = b \\ c = d \end{gathered}",
        r"\hat{x} \widehat{xyz} \overline{AB} \overbrace{a+b}^{n} \underbrace{c+d}_{m}",
        r"\overset{!}{=} \underset{x}{\arg\min} \stackrel{def}{=}",
        r"\mathbb{R} \mathcal{L} \mathfrak{g} \mathbf{v} \mathrm{d}x \mathsf{A} \mathtt{c}",
        r"\text{if } x > 0 \textbf{ and } y",
        r"\phantom{x} \vphantom{y} \hphantom{z} \smash{w}",
        r"\boxed{E = mc^2} \cancel{x} \xcancel{z} \fcolorbox{red}{blue}{q}",
        r"\textcolor{red}{x} + \color{blue} y",
        r"{}_{6}^{14}C \quad a \, b \; c \! d",
        r"\mathop{lim} \mathord{x} \mathrel{R} \mathchoice{D}{T}{S}{SS}",
        r"\scalebox{2}{x} \raisebox{1pt}{y} \rule{1em}{2pt}",
        r"\begin{array}{|l|r|} \hline 1 & 2 \\ \end{array}",
    ];
    let config = Config::default();
    for tex in corpus {
        let expr = mathema::reader::latex::LatexReader::parse_compact(tex, &config)
            .unwrap()
            .expr;
        for options in dialects()
            .into_iter()
            .chain([OmmlOptions::default().with_display_block(Some(OmmlJustification::Right))])
        {
            let (out, _) = OmmlWriter::write_with_options(&expr, &config, &options).unwrap();
            check_schema(&out);
            let back = OmmlReader::parse_compact(&out, &config)
                .unwrap_or_else(|e| panic!("{tex}: {e}\n{out}"));
            let (again, _) = OmmlWriter::write_with_options(&back.expr, &config, &options).unwrap();
            let back2 = OmmlReader::parse_compact(&again, &config).unwrap();
            let (third, _) =
                OmmlWriter::write_with_options(&back2.expr, &config, &options).unwrap();
            assert_eq!(again, third, "{tex}: not stable");
        }
    }
}

// ── Structural schema check ─────────────────────────────────────────────

/// One particle of a sequence content model: child name, min, max.
type Particle = (&'static str, usize, usize);

const MANY: usize = usize::MAX;
/// Any element of `EG_OMathMathElements`.
const MATH: &str = "#math";
const MATH_ELEMENTS: [&str; 20] = [
    "acc",
    "bar",
    "box",
    "borderBox",
    "d",
    "eqArr",
    "f",
    "func",
    "groupChr",
    "limLow",
    "limUpp",
    "m",
    "nary",
    "phant",
    "rad",
    "sPre",
    "sSub",
    "sSubSup",
    "sSup",
    "r",
];

/// Content models of the math elements the writer emits, transcribed from
/// `shared-math.xsd`. Every model there is a plain sequence.
fn content_models() -> HashMap<&'static str, Vec<Particle>> {
    let arg = vec![("argPr", 0, 1), (MATH, 0, MANY), ("ctrlPr", 0, 1)];
    let mut m: HashMap<&'static str, Vec<Particle>> = HashMap::new();
    m.insert("oMathPara", vec![("oMathParaPr", 0, 1), ("oMath", 1, MANY)]);
    m.insert("oMathParaPr", vec![("jc", 0, 1)]);
    m.insert("oMath", vec![(MATH, 0, MANY)]);
    for a in ["e", "num", "den", "sub", "sup", "deg", "lim", "fName"] {
        m.insert(a, arg.clone());
    }
    m.insert("r", vec![("rPr", 0, 1), ("#host", 0, 1), ("t", 0, MANY)]);
    m.insert(
        "rPr",
        vec![
            ("lit", 0, 1),
            ("nor", 0, 1),
            ("scr", 0, 1),
            ("sty", 0, 1),
            ("brk", 0, 1),
            ("aln", 0, 1),
        ],
    );
    m.insert("acc", vec![("accPr", 0, 1), ("e", 1, 1)]);
    m.insert("accPr", vec![("chr", 0, 1), ("ctrlPr", 0, 1)]);
    m.insert("bar", vec![("barPr", 0, 1), ("e", 1, 1)]);
    m.insert("barPr", vec![("pos", 0, 1), ("ctrlPr", 0, 1)]);
    m.insert("box", vec![("boxPr", 0, 1), ("e", 1, 1)]);
    m.insert(
        "boxPr",
        vec![
            ("opEmu", 0, 1),
            ("noBreak", 0, 1),
            ("diff", 0, 1),
            ("brk", 0, 1),
            ("aln", 0, 1),
            ("ctrlPr", 0, 1),
        ],
    );
    m.insert("borderBox", vec![("borderBoxPr", 0, 1), ("e", 1, 1)]);
    m.insert(
        "borderBoxPr",
        vec![
            ("hideTop", 0, 1),
            ("hideBot", 0, 1),
            ("hideLeft", 0, 1),
            ("hideRight", 0, 1),
            ("strikeH", 0, 1),
            ("strikeV", 0, 1),
            ("strikeBLTR", 0, 1),
            ("strikeTLBR", 0, 1),
            ("ctrlPr", 0, 1),
        ],
    );
    m.insert("d", vec![("dPr", 0, 1), ("e", 1, MANY)]);
    m.insert(
        "dPr",
        vec![
            ("begChr", 0, 1),
            ("sepChr", 0, 1),
            ("endChr", 0, 1),
            ("grow", 0, 1),
            ("shp", 0, 1),
            ("ctrlPr", 0, 1),
        ],
    );
    m.insert("eqArr", vec![("eqArrPr", 0, 1), ("e", 1, MANY)]);
    m.insert("f", vec![("fPr", 0, 1), ("num", 1, 1), ("den", 1, 1)]);
    m.insert("fPr", vec![("type", 0, 1), ("ctrlPr", 0, 1)]);
    m.insert("func", vec![("funcPr", 0, 1), ("fName", 1, 1), ("e", 1, 1)]);
    m.insert("groupChr", vec![("groupChrPr", 0, 1), ("e", 1, 1)]);
    m.insert(
        "groupChrPr",
        vec![
            ("chr", 0, 1),
            ("pos", 0, 1),
            ("vertJc", 0, 1),
            ("ctrlPr", 0, 1),
        ],
    );
    m.insert(
        "limLow",
        vec![("limLowPr", 0, 1), ("e", 1, 1), ("lim", 1, 1)],
    );
    m.insert(
        "limUpp",
        vec![("limUppPr", 0, 1), ("e", 1, 1), ("lim", 1, 1)],
    );
    m.insert("m", vec![("mPr", 0, 1), ("mr", 1, MANY)]);
    m.insert(
        "mPr",
        vec![
            ("baseJc", 0, 1),
            ("plcHide", 0, 1),
            ("mcs", 0, 1),
            ("ctrlPr", 0, 1),
        ],
    );
    m.insert("mcs", vec![("mc", 1, MANY)]);
    m.insert("mc", vec![("mcPr", 0, 1)]);
    m.insert("mcPr", vec![("count", 0, 1), ("mcJc", 0, 1)]);
    m.insert("mr", vec![("e", 1, MANY)]);
    m.insert(
        "nary",
        vec![("naryPr", 0, 1), ("sub", 1, 1), ("sup", 1, 1), ("e", 1, 1)],
    );
    m.insert(
        "naryPr",
        vec![
            ("chr", 0, 1),
            ("limLoc", 0, 1),
            ("grow", 0, 1),
            ("subHide", 0, 1),
            ("supHide", 0, 1),
            ("ctrlPr", 0, 1),
        ],
    );
    m.insert("phant", vec![("phantPr", 0, 1), ("e", 1, 1)]);
    m.insert(
        "phantPr",
        vec![
            ("show", 0, 1),
            ("zeroWid", 0, 1),
            ("zeroAsc", 0, 1),
            ("zeroDesc", 0, 1),
            ("transp", 0, 1),
            ("ctrlPr", 0, 1),
        ],
    );
    m.insert("rad", vec![("radPr", 0, 1), ("deg", 1, 1), ("e", 1, 1)]);
    m.insert("radPr", vec![("degHide", 0, 1), ("ctrlPr", 0, 1)]);
    m.insert(
        "sPre",
        vec![("sPrePr", 0, 1), ("sub", 1, 1), ("sup", 1, 1), ("e", 1, 1)],
    );
    m.insert("sSub", vec![("sSubPr", 0, 1), ("e", 1, 1), ("sub", 1, 1)]);
    m.insert("sSup", vec![("sSupPr", 0, 1), ("e", 1, 1), ("sup", 1, 1)]);
    m.insert(
        "sSubSup",
        vec![
            ("sSubSupPr", 0, 1),
            ("e", 1, 1),
            ("sub", 1, 1),
            ("sup", 1, 1),
        ],
    );
    m
}

/// Allowed `m:val` values of the simple-typed leaf properties.
fn value_ok(element: &str, value: &str) -> bool {
    let on_off = ["0", "1", "on", "off", "true", "false"];
    match element {
        "sty" => ["p", "b", "i", "bi"].contains(&value),
        "scr" => [
            "roman",
            "script",
            "fraktur",
            "double-struck",
            "sans-serif",
            "monospace",
        ]
        .contains(&value),
        "jc" => ["left", "right", "center", "centerGroup"].contains(&value),
        "limLoc" => ["undOvr", "subSup"].contains(&value),
        "type" => ["bar", "skw", "lin", "noBar"].contains(&value),
        "pos" | "vertJc" => ["top", "bot"].contains(&value),
        "mcJc" => ["left", "center", "right", "inside", "outside"].contains(&value),
        "count" => value.parse::<u32>().is_ok_and(|n| (1..=255).contains(&n)),
        // CT_Char: at most one character.
        "chr" | "begChr" | "sepChr" | "endChr" => value.chars().count() <= 1,
        _ => on_off.contains(&value),
    }
}

struct Frame {
    local: String,
    children: Vec<String>,
}

fn check_sequence(parent: &str, children: &[String], model: &[Particle], xml: &str) {
    let mut idx = 0;
    let mut used = 0;
    for child in children {
        loop {
            assert!(
                idx < model.len(),
                "<m:{child}> not allowed here in <m:{parent}> (children {children:?})\n{xml}"
            );
            let (name, _, max) = model[idx];
            let matches = name == child.as_str()
                || (name == MATH && MATH_ELEMENTS.contains(&child.as_str()))
                || (name == "#host" && (child == "w:rPr" || child == "a:rPr"));
            if matches && used < max {
                used += 1;
                break;
            }
            assert!(
                used >= model[idx].1,
                "<m:{parent}> misses <m:{name}>\n{xml}"
            );
            idx += 1;
            used = 0;
        }
    }
    let first_missing = model[idx.min(model.len())..]
        .iter()
        .enumerate()
        .find(|&(i, &(_, min, _))| if i == 0 { used < min } else { min > 0 });
    assert!(
        first_missing.is_none(),
        "<m:{parent}> misses a required child {first_missing:?} (children {children:?})\n{xml}"
    );
    if parent == "rPr" {
        let nor = children.iter().any(|c| c == "nor");
        let style = children.iter().any(|c| c == "scr" || c == "sty");
        assert!(!(nor && style), "m:nor excludes m:scr/m:sty\n{xml}");
    }
}

/// Check well-formedness, declared prefixes and the math content models.
fn check_schema(xml: &str) {
    use quick_xml::events::Event;
    let models = content_models();
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().expand_empty_elements = true;
    let mut declared: Vec<String> = vec!["xml".into()];
    let mut stack: Vec<Frame> = Vec::new();
    loop {
        match reader
            .read_event()
            .unwrap_or_else(|e| panic!("not well-formed: {e}\n{xml}"))
        {
            Event::Start(e) => {
                let qname = e.name().into_inner().to_owned();
                for attr in e.attributes() {
                    let attr = attr.unwrap();
                    let key = attr.key.into_inner();
                    if let Some(prefix) = key.strip_prefix("xmlns:") {
                        declared.push(prefix.to_owned());
                    } else if let Some((prefix, _)) = key.split_once(':') {
                        assert!(
                            declared.iter().any(|d| d == prefix),
                            "undeclared attribute prefix {key}\n{xml}"
                        );
                    }
                }
                let (prefix, local) = qname
                    .split_once(':')
                    .expect("writer output is always prefixed");
                assert!(
                    declared.iter().any(|d| d == prefix),
                    "undeclared prefix {prefix}\n{xml}"
                );
                if prefix == "m"
                    && let Some(val) = e.try_get_attribute("m:val").unwrap()
                {
                    let value = val
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .unwrap();
                    assert!(
                        value_ok(local, &value),
                        "invalid m:val {value:?} on <m:{local}>\n{xml}"
                    );
                }
                let recorded = if prefix == "m" {
                    local.to_owned()
                } else {
                    qname.clone()
                };
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(recorded);
                }
                stack.push(Frame {
                    local: if prefix == "m" {
                        local.to_owned()
                    } else {
                        qname
                    },
                    children: Vec::new(),
                });
            },
            Event::End(_) => {
                let frame = stack.pop().unwrap();
                if let Some(model) = models.get(frame.local.as_str()) {
                    check_sequence(&frame.local, &frame.children, model, xml);
                }
            },
            Event::Eof => break,
            _ => {},
        }
    }
    assert!(stack.is_empty());
}

#[test]
fn structural_check_rejects_known_schema_violations() {
    let ns = r#"xmlns:m="http://schemas.openxmlformats.org/officeDocument/2006/math""#;
    for bad in [
        // m:sty before m:scr.
        format!(
            r#"<m:oMath {ns}><m:r><m:rPr><m:sty m:val="p"/><m:scr m:val="script"/></m:rPr><m:t>x</m:t></m:r></m:oMath>"#
        ),
        // Undeclared w: prefix.
        format!(
            r#"<m:oMath {ns}><m:r><m:rPr><w:color w:val="FF0000"/></m:rPr><m:t>x</m:t></m:r></m:oMath>"#
        ),
        // endChr before sepChr.
        format!(
            r#"<m:oMath {ns}><m:d><m:dPr><m:endChr m:val=")"/><m:sepChr m:val=";"/></m:dPr><m:e/></m:d></m:oMath>"#
        ),
        // Missing m:den.
        format!(r#"<m:oMath {ns}><m:f><m:num/></m:f></m:oMath>"#),
    ] {
        assert!(
            std::panic::catch_unwind(|| check_schema(&bad)).is_err(),
            "accepted: {bad}"
        );
    }
}

/// Remove DrawingML `a:rPr` elements (they never nest).
fn strip_drawing_run_properties(xml: &str) -> String {
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(start) = rest.find("<a:rPr") {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let open_end = tail.find('>').expect("tag is closed");
        rest = if tail[..open_end].ends_with('/') {
            &tail[open_end + 1..]
        } else {
            let close = tail.find("</a:rPr>").expect("a:rPr is closed");
            &tail[close + "</a:rPr>".len()..]
        };
    }
    out.push_str(rest);
    out
}

#[test]
fn drawing_dialect_is_schema_shaped_without_its_run_properties() {
    let config = Config::default();
    let options = OmmlOptions::default().with_run_properties(OmmlRunProperties::DrawingMl);
    for name in ["word_paragraph.omml", "powerpoint_paragraph.omml"] {
        let para = OmmlReader::parse_paragraph(&fixture(name), &config).unwrap();
        let (out, _) = OmmlWriter::write_paragraph(&para, &config, &options).unwrap();
        assert!(out.contains("<a:rPr"), "{name}: {out}");
        let stripped = strip_drawing_run_properties(&out);
        assert!(!stripped.contains("a:rPr"));
        // With a:rPr gone, only the schema's own content models remain.
        check_schema(&stripped);
    }
}

// ── Optional XSD validation ─────────────────────────────────────────────

/// Validate writer output with `xmllint --schema` against the OMML schema
/// (ECMA-376 Part 1, Annex A, `shared-math.xsd`), which is not distributed
/// with this crate. Run it with:
///
/// ```text
/// MATHEMA_OMML_XSD=/path/to/OMML.xsd cargo test --test omml_round_trip -- --ignored
/// ```
///
/// The schema imports two documents that are not part of it; minimal
/// stand-ins declaring only what it references are generated. DrawingML
/// output is validated with its `a:rPr` elements removed, since the schema
/// only allows WordprocessingML run properties there.
#[test]
#[ignore = "needs xmllint and MATHEMA_OMML_XSD pointing at the OMML schema"]
fn writer_output_validates_against_omml_xsd() {
    let xsd = std::env::var_os("MATHEMA_OMML_XSD")
        .map(std::path::PathBuf::from)
        .expect("set MATHEMA_OMML_XSD to the path of the OMML schema");
    let have_xmllint = std::process::Command::new("xmllint")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success());
    assert!(have_xmllint, "xmllint is not installed");
    let dir = std::env::temp_dir().join(format!("mathema-omml-xsd-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let schema = std::fs::read_to_string(&xsd).unwrap().replace(
        r#"<xsd:import namespace="http://www.w3.org/XML/1998/namespace"/>"#,
        r#"<xsd:import namespace="http://www.w3.org/XML/1998/namespace" schemaLocation="xml.xsd"/>"#,
    );
    std::fs::write(dir.join("OMML.xsd"), schema).unwrap();
    std::fs::write(dir.join("xml.xsd"), XML_XSD).unwrap();
    std::fs::write(dir.join("wml.xsd"), WML_STUB_XSD).unwrap();
    std::fs::write(dir.join("shared-commonSimpleTypes.xsd"), SHARED_STUB_XSD).unwrap();

    let config = Config::default();
    let mut outputs = Vec::new();
    for name in [
        "word_paragraph.omml",
        "powerpoint_paragraph.omml",
        "plurimath_047_nary_eqarr.omml",
    ] {
        let para = OmmlReader::parse_paragraph(&fixture(name), &config).unwrap();
        for options in &dialects() {
            let out = OmmlWriter::write_paragraph(&para, &config, options)
                .unwrap()
                .0;
            outputs.push(strip_drawing_run_properties(&out));
        }
    }
    for (i, xml) in outputs.iter().enumerate() {
        let path = dir.join(format!("out{i}.xml"));
        std::fs::write(&path, xml).unwrap();
        let status = std::process::Command::new("xmllint")
            .arg("--noout")
            .arg("--schema")
            .arg(dir.join("OMML.xsd"))
            .arg(&path)
            .status()
            .unwrap();
        assert!(status.success(), "schema validation failed:\n{xml}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

const XML_XSD: &str = r#"<?xml version="1.0"?>
<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema" targetNamespace="http://www.w3.org/XML/1998/namespace">
  <xsd:attribute name="space"><xsd:simpleType><xsd:restriction base="xsd:NCName">
    <xsd:enumeration value="default"/><xsd:enumeration value="preserve"/></xsd:restriction></xsd:simpleType></xsd:attribute>
</xsd:schema>"#;

const SHARED_STUB_XSD: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema"
  targetNamespace="http://schemas.openxmlformats.org/officeDocument/2006/sharedTypes" elementFormDefault="qualified">
  <xsd:simpleType name="ST_OnOff"><xsd:restriction base="xsd:string">
    <xsd:enumeration value="true"/><xsd:enumeration value="false"/><xsd:enumeration value="on"/>
    <xsd:enumeration value="off"/><xsd:enumeration value="0"/><xsd:enumeration value="1"/>
  </xsd:restriction></xsd:simpleType>
  <xsd:simpleType name="ST_String"><xsd:restriction base="xsd:string"/></xsd:simpleType>
  <xsd:simpleType name="ST_XAlign"><xsd:restriction base="xsd:string">
    <xsd:enumeration value="left"/><xsd:enumeration value="center"/><xsd:enumeration value="right"/>
    <xsd:enumeration value="inside"/><xsd:enumeration value="outside"/>
  </xsd:restriction></xsd:simpleType>
  <xsd:simpleType name="ST_YAlign"><xsd:restriction base="xsd:string">
    <xsd:enumeration value="inline"/><xsd:enumeration value="top"/><xsd:enumeration value="center"/>
    <xsd:enumeration value="bottom"/><xsd:enumeration value="inside"/><xsd:enumeration value="outside"/>
  </xsd:restriction></xsd:simpleType>
  <xsd:simpleType name="ST_TwipsMeasure"><xsd:restriction base="xsd:unsignedLong"/></xsd:simpleType>
</xsd:schema>"#;

const WML_STUB_XSD: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<xsd:schema xmlns:xsd="http://www.w3.org/2001/XMLSchema"
  xmlns="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
  targetNamespace="http://schemas.openxmlformats.org/wordprocessingml/2006/main"
  elementFormDefault="qualified" attributeFormDefault="qualified">
  <xsd:complexType name="CT_OnOff"><xsd:attribute name="val" type="xsd:string"/></xsd:complexType>
  <xsd:complexType name="CT_Color"><xsd:attribute name="val" type="xsd:hexBinary" use="required"/></xsd:complexType>
  <xsd:complexType name="CT_RPr"><xsd:sequence>
    <xsd:element name="b" type="CT_OnOff" minOccurs="0"/>
    <xsd:element name="i" type="CT_OnOff" minOccurs="0"/>
    <xsd:element name="color" type="CT_Color" minOccurs="0"/>
  </xsd:sequence></xsd:complexType>
  <xsd:group name="EG_RPr"><xsd:sequence><xsd:element name="rPr" type="CT_RPr"/></xsd:sequence></xsd:group>
  <xsd:group name="EG_RPrMath"><xsd:sequence><xsd:element name="rPr" type="CT_RPr"/></xsd:sequence></xsd:group>
  <xsd:group name="EG_RunInnerContent"><xsd:choice><xsd:element name="br"/></xsd:choice></xsd:group>
  <xsd:group name="EG_PContentMath"><xsd:choice><xsd:element name="bookmarkStart"/></xsd:choice></xsd:group>
</xsd:schema>"#;
