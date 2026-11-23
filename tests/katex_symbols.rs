//! KaTeX command coverage.
//!
//! Every command in [`KATEX_COMMANDS`] is documented as supported by KaTeX.
//! An unknown control word falls back to an identifier spelled like the
//! command (`\lhd` → `<mi>lhd</mi>`), which silently renders the wrong thing,
//! so this pins down that each one resolves to real math and survives a
//! LaTeX round trip.

use mathema::from_latex;

/// Control words KaTeX supports, written without the backslash. Each is used
/// as `a \name b`, so commands that take an argument consume the `b`.
const KATEX_COMMANDS: &str = "
    Alpha And Bbbk Beta Box Cap Chi Colonapprox Coloneq Coloneqq Colonsim Complex Cup Dagger 
    Darr Delta Diamond Downarrow Epsilon Eqcolon Eqqcolon Eta Finv Game Gamma Harr Im Iota 
    Join Kappa Lambda Larr Leftarrow Leftrightarrow Lleftarrow Longleftarrow 
    Longleftrightarrow Longrightarrow Lrarr Lsh Mu N Nu  Omega Omicron Phi Pi Pr Psi R Rarr 
    Re Reals Rho Rightarrow Rrightarrow Rsh Sigma Subset Supset Tau Theta Uarr Uparrow 
    Updownarrow Upsilon Vdash Vert Vvdash Xi Z Zeta alef alefsym aleph alpha amalg angle 
    approxcolon approxcoloncolon arccos arcctg arcsin arctan arctg arg argmax argmin ast 
    backprime backslash barwedge because beta beth bigcap bigcirc bigcup bigodot bigoplus 
    bigotimes bigsqcup bigstar bigtriangledown bigtriangleup biguplus bigvee bigwedge 
    blacklozenge blacksquare blacktriangle blacktriangledown blacktriangleleft 
    blacktriangleright bot boxdot boxminus boxplus boxtimes bullet cap cdot cdotp cdots 
    centerdot ch checkmark chi circ circlearrowleft circlearrowright circledR circledS 
    clubsuit cnums colonapprox coloncolonequals coloneq coloneqq colonequals colonsim 
    complement coprod copyright cos cosec cosh cot cotg coth csc ctg cth cup curlyvee 
    curlywedge curvearrowleft curvearrowright dArr dag dagger daleth darr dashleftarrow 
    dashrightarrow dblcolon ddag ddagger ddots deg degree delta det diagdown diagup diamond 
    diamondsuit digamma dim div divideontimes doteqdot dotplus dots dotsb dotsc dotsi dotsm 
    dotso doublebarwedge doublecap doublecup downarrow downdownarrows downharpoonleft 
    downharpoonright ell empty emptyset epsilon eqcirc eqcolon eqqcolon eqsim eqslantgtr 
    eqslantless equalscolon equalscoloncolon eta eth exist exists exp fallingdotseq flat 
    forall frown gamma gcd ge geq geqq geqslant gg ggg gggtr gimel gnapprox gneq gneqq gnsim 
    gt gtrapprox gtrdot gtreqless gtreqqless gtrless gtrsim gvertneqq hArr harr hbar 
    heartsuit hom hookleftarrow hookrightarrow hslash iddots iff iiint iint image imageof 
    imath impliedby implies in inf infty injlim int intercal intop iota isin  jmath kappa 
    ker lArr lBrace lVert lambda land lang langle larr lbrace lbrack lceil ldotp ldots le 
    leadsto leftarrow leftarrowtail leftharpoondown leftharpoonup leftleftarrows 
    leftrightarrow leftrightarrows leftrightharpoons leftrightsquigarrow leftthreetimes leq 
    leqq leqslant lessapprox lessdot lesseqgtr lesseqqgtr lessgtr lesssim lfloor lg lgroup 
    lhd lim liminf limsup ll llbracket llcorner lll llless lmoustache ln lnapprox lneq lneqq 
    lnot lnsim log longleftarrow longleftrightarrow longmapsto longrightarrow looparrowleft 
    looparrowright lor lozenge lrArr lrarr lrcorner lt ltimes lvert lvertneqq maltese mapsto 
    mathellipsis mathsterling max measuredangle mid min minuscolon minuso models mp mu 
    multimap nLeftarrow nLeftrightarrow nRightarrow nVDash nVdash nabla natnums natural ncong 
    ne nearrow neg neq nexists ngeq ngeqq ngeqslant ngtr ni nleftarrow nleftrightarrow nleq 
    nleqq nleqslant nless nmid notin notni nparallel nprec npreceq nrightarrow nshortmid 
    nshortparallel nsim nsubseteq nsubseteqq nsucc nsucceq nsupseteq nsupseteqq ntriangleleft 
    ntrianglelefteq ntriangleright ntrianglerighteq nu nvDash nvdash nwarrow  odot oiiint 
    oiint oint omega omicron ominus oplus origof oslash otimes owns parallel partial perp phi 
    pi pitchfork plim pm pounds prec precapprox preccurlyeq preceq precnapprox precneqq 
    precnsim precsim prime prod projlim propto psi rArr rBrace rVert rang rangle rarr ratio 
    rbrace rbrack rceil real reals restriction rfloor rgroup rhd rho rightarrow 
    rightarrowtail rightharpoondown rightharpoonup rightleftarrows rightleftharpoons 
    rightrightarrows rightsquigarrow rightthreetimes risingdotseq rmoustache rrbracket rtimes 
    rvert searrow sec setminus sh sharp shortmid shortparallel sigma sim simcolon 
    simcoloncolon simeq sin sinh smallfrown smallsetminus smallsmile smile spadesuit 
    sphericalangle sqcap sqcup sqsubset sqsubseteq sqsupset sqsupseteq square star subset 
    subseteq subseteqq subsetneq subsetneqq succ succapprox succcurlyeq succeq succnapprox 
    succneqq succnsim succsim sum sup supset supseteq supseteqq supsetneq supsetneqq swarrow 
    tan tanh tau tg th therefore theta thetasym thickapprox thicksim times to top triangle 
    triangledown triangleleft trianglelefteq triangleq triangleright trianglerighteq 
    twoheadleftarrow twoheadrightarrow uArr uarr ulcorner unlhd unrhd uparrow updownarrow 
    upharpoonleft upharpoonright uplus upsilon upuparrows urcorner vDash varDelta varGamma 
    varLambda varOmega varPhi varPi varPsi varSigma varTheta varUpsilon varXi varepsilon 
    varinjlim varkappa varliminf varlimsup varnothing varphi varpi varprojlim varpropto 
    varrho varsigma varsubsetneq varsubsetneqq varsupsetneq varsupsetneqq vartheta 
    vartriangle vartriangleleft vartriangleright vcentcolon vdash vdots vee veebar vert wedge 
    weierp wp wr xi yen zeta 
";

fn commands() -> impl Iterator<Item = &'static str> {
    KATEX_COMMANDS.split_ascii_whitespace()
}

fn mathml(latex: &str) -> String {
    from_latex(latex)
        .unwrap_or_else(|e| panic!("{latex:?} should parse: {e}"))
        .to_mathml()
        .unwrap_or_else(|e| panic!("{latex:?} should write MathML: {e}"))
}

#[test]
fn katex_commands_do_not_fall_back_to_their_names() {
    let unknown: Vec<_> = commands()
        // A one-letter command (`\R`) legitimately yields that letter.
        .filter(|name| name.len() > 1)
        .filter(|name| mathml(&format!(r"a \{name} b")).contains(&format!(">{name}</mi>")))
        .collect();
    assert!(unknown.is_empty(), "rendered as literal names: {unknown:?}");
}

#[test]
fn katex_commands_round_trip_through_latex() {
    for name in commands() {
        let input = format!(r"a \{name} b");
        let first = from_latex(&input).unwrap();
        let latex = first.to_latex().unwrap();
        let second = from_latex(&latex)
            .unwrap_or_else(|e| panic!("{input:?} wrote unparsable {latex:?}: {e}"));
        // Compare the trees, not MathML: `<mo>` hides the atom class, which
        // is exactly what a lossy spelling (`\vartriangle` -> `\triangle`)
        // changes.
        assert_eq!(
            first.expr(),
            second.expr(),
            "{input:?} changed meaning via {latex:?}"
        );
        assert_eq!(latex, second.to_latex().unwrap(), "{input:?} is not stable");
    }
}

#[test]
fn katex_aliases_match_their_canonical_commands() {
    for (alias, canonical) in [
        (r"\Alpha", "\u{0391}"),
        (r"\omicron", "\u{03BF}"),
        (r"\thetasym", r"\vartheta"),
        (r"\alef", r"\aleph"),
        (r"\real", r"\Re"),
        (r"\image", r"\Im"),
        (r"\weierp", r"\wp"),
        (r"\exist", r"\exists"),
        (r"\empty", r"\emptyset"),
        (r"\isin", r"\in"),
        (r"\owns", r"\ni"),
        (r"\Dagger", r"\ddagger"),
        (r"\darr", r"\downarrow"),
        (r"\Rarr", r"\Rightarrow"),
        (r"\lrArr", r"\Leftrightarrow"),
        (r"\impliedby", r"\Longleftarrow"),
        (r"\shortmid", r"\mid"),
        (r"\thicksim", r"\sim"),
        (r"\lang x \rang", r"\langle x \rangle"),
        (r"\left\lang x \right\rang", r"\left\langle x \right\rangle"),
        (r"\dotsc", r"\ldots"),
        (r"\dotsb", r"\cdots"),
        (r"\mathellipsis", r"\ldots"),
        (r"\R", r"\mathbb{R}"),
        (r"\ratio", r"\vcentcolon"),
        (r"\colonequals", r"\coloneqq"),
    ] {
        assert_eq!(mathml(alias), mathml(canonical), "{alias} vs {canonical}");
    }
}

#[test]
fn italic_capital_greek_uses_math_italic_letters() {
    assert!(mathml(r"\varGamma").contains('\u{1D6E4}'));
    assert!(mathml(r"\varOmega").contains('\u{1D6FA}'));
}

#[test]
fn eastern_function_names_are_operators() {
    for name in [
        "tg", "ctg", "cotg", "cosec", "sh", "ch", "th", "cth", "arctg", "arcctg",
    ] {
        assert!(
            mathml(&format!(r"\{name} x")).contains(&format!("<mo>{name}</mo>")),
            "{name}"
        );
    }
}

#[test]
fn colon_compounds_keep_every_glyph() {
    for (command, glyphs) in [
        ("colonapprox", "\u{2236}\u{2248}"),
        ("Colonsim", "\u{2237}\u{223C}"),
        ("eqcolon", "\u{2212}\u{2236}"),
        ("Eqqcolon", "=\u{2237}"),
        ("approxcoloncolon", "\u{2248}\u{2237}"),
    ] {
        let input = format!(r"a \{command} b");
        assert!(
            mathml(&input).contains(&format!("<mo>{glyphs}</mo>")),
            "{command}"
        );
        assert_eq!(
            from_latex(&input).unwrap().to_latex().unwrap(),
            format!(r"a\{command} b")
        );
    }
}

#[test]
fn and_is_a_binary_ampersand() {
    let parsed = from_latex(r"a \And b").unwrap();
    assert_eq!(parsed.to_latex().unwrap(), r"a\And b");
    // The ordinary `\&` keeps its own spelling and class.
    assert_eq!(from_latex(r"a \& b").unwrap().to_latex().unwrap(), r"a\&b");
}
