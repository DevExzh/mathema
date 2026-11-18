use std::borrow::Cow;

use smallvec::SmallVec;

use super::packages;

#[path = "ordered_stream.rs"]
mod ordered_stream;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Lower bound on the macro-expansion output cap. Inputs smaller than
/// `MIN_EXPANSION_CAP / 64` bytes are still permitted to expand up to this
/// many bytes, so ordinary macro use is never penalized.
const MIN_EXPANSION_CAP: usize = 1 << 20; // 1 MiB

/// Work budget multiplier for token expansion. It accounts for queue scans,
/// replacement construction and discarded branches in addition to rendered
/// output bytes.
const EXPANSION_WORK_MULTIPLIER: usize = 128;
const MIN_EXPANSION_WORK: usize = 1 << 20;

/// Resource failure raised while expanding the ordered macro stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpansionFailure {
    /// Rendered or intermediate expansion bytes exceeded the output cap.
    OutputBytes { limit: usize },
    /// Token processing, queue traversal or clone work exceeded its budget.
    Work { limit: usize },
    /// A replacement chain exceeded the recursive expansion depth.
    Depth { limit: usize },
    /// A defined macro or selector was invoked without matching arguments.
    Arguments { command: Box<str> },
    /// A recognized declaration or assignment did not satisfy its grammar.
    Syntax {
        command: SyntaxCommand,
        kind: SyntaxKind,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxCommand {
    Def,
    Gdef,
    Edef,
    Xdef,
    Let,
    Futurelet,
    Newcommand,
    Renewcommand,
    Providecommand,
    Newenvironment,
    Begin,
    Begingroup,
    Endgroup,
    Global,
    Long,
    Protected,
    Other,
}

impl SyntaxCommand {
    pub(crate) fn from_name(name: &str) -> Self {
        match name {
            "def" => Self::Def,
            "gdef" => Self::Gdef,
            "edef" => Self::Edef,
            "xdef" => Self::Xdef,
            "let" => Self::Let,
            "futurelet" => Self::Futurelet,
            "newcommand" => Self::Newcommand,
            "renewcommand" => Self::Renewcommand,
            "providecommand" => Self::Providecommand,
            "newenvironment" => Self::Newenvironment,
            "begin" => Self::Begin,
            "begingroup" => Self::Begingroup,
            "endgroup" => Self::Endgroup,
            "global" => Self::Global,
            "long" => Self::Long,
            "protected" => Self::Protected,
            _ => Self::Other,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Def => "def",
            Self::Gdef => "gdef",
            Self::Edef => "edef",
            Self::Xdef => "xdef",
            Self::Let => "let",
            Self::Futurelet => "futurelet",
            Self::Newcommand => "newcommand",
            Self::Renewcommand => "renewcommand",
            Self::Providecommand => "providecommand",
            Self::Newenvironment => "newenvironment",
            Self::Begin => "begin",
            Self::Begingroup => "begingroup",
            Self::Endgroup => "endgroup",
            Self::Global => "global",
            Self::Long => "long",
            Self::Protected => "protected",
            Self::Other => "macro declaration",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxKind {
    ControlSequenceTarget,
    ReplacementBody,
    ParameterText,
    ParameterCount,
    OptionalDefault,
    OptionalDefaultWithoutParameter,
    TargetGroup,
    SourceToken,
    MiddleToken,
    LookaheadToken,
    PrefixAssignment,
    UnclosedParameterCount,
    UnclosedTarget,
    EnvironmentName,
    EnvironmentBegin,
    EnvironmentEnd,
    MissingArguments,
    GroupEnd,
    ProtectedAssignment,
}

impl SyntaxKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ControlSequenceTarget => "expected a control-sequence target",
            Self::ReplacementBody => "expected a braced replacement body",
            Self::ParameterText => "invalid parameter text",
            Self::ParameterCount => "parameter count must be a nonnegative integer from 0 to 255",
            Self::OptionalDefault => "unterminated optional default",
            Self::OptionalDefaultWithoutParameter => {
                "optional default requires at least one parameter"
            },
            Self::TargetGroup => "target group must contain one control sequence",
            Self::SourceToken => "expected a source token",
            Self::MiddleToken => "expected a middle token",
            Self::LookaheadToken => "expected a lookahead token",
            Self::PrefixAssignment => {
                "expected a definition, let, or futurelet assignment after prefix"
            },
            Self::UnclosedParameterCount => "unterminated parameter count",
            Self::UnclosedTarget => "unterminated target group",
            Self::EnvironmentName => "expected a nonempty environment name group",
            Self::EnvironmentBegin => "expected a braced begin body",
            Self::EnvironmentEnd => "expected a braced end body",
            Self::GroupEnd => "unmatched explicit group delimiter",
            Self::ProtectedAssignment => "protected applies only to a macro definition",
            Self::MissingArguments => "missing a required argument",
        }
    }
}

/// Maximum number of bytes macro expansion may produce for a given input.
pub(super) fn expansion_cap(input_len: usize) -> usize {
    input_len.saturating_mul(64).max(MIN_EXPANSION_CAP)
}

pub(crate) fn expansion_work_cap(input_len: usize) -> usize {
    input_len
        .saturating_mul(EXPANSION_WORK_MULTIPLIER)
        .max(MIN_EXPANSION_WORK)
}

/// Strip comments and expand definitions in the active ordered token stream.
///
/// The common path borrows the input when no comments or expandable package
/// macros are present. Expansion errors identify whether output bytes, token
/// work, or recursive depth stopped the traversal.
pub fn preprocess_cow(input: &str) -> Result<Cow<'_, str>, ExpansionFailure> {
    expand_source(input, false)
}

/// Prepare parser input, allowing direct consumption of a leading `\plim`
/// and a bounded literal rewrite of mathtools fences.
pub(super) fn prepare_math_input(input: &str) -> Result<Cow<'_, str>, ExpansionFailure> {
    expand_source(input, true)
}

fn expand_source(input: &str, for_parser: bool) -> Result<Cow<'_, str>, ExpansionFailure> {
    let (source, needs_expansion, fenced_matrix) = prepare_source(input, for_parser);
    if !needs_expansion {
        if !fenced_matrix {
            return Ok(source);
        }
        // With at least one input byte per potential group, a short literal input
        // cannot exhaust the ordered stream's group or token budgets. Long or
        // dynamic bodies retain the full expansion and its resource checks.
        if for_parser
            && source.len() <= crate::error::DEFAULT_MAX_DEPTH
            && let Some(expanded) = rewrite_literal_smallmatrix(&source)
        {
            return Ok(Cow::Owned(expanded));
        }
    }
    Ok(Cow::Owned(ordered_stream::expand(&source)?))
}

/// Locate expandable words and strip comments in one traversal. Only allocate
/// a replacement string if a real comment, outside a verbatim block, is found.
fn prepare_source(input: &str, direct_leading_plim: bool) -> (Cow<'_, str>, bool, bool) {
    let mut offset = 0;
    let mut copy_from = 0;
    let mut stripped: Option<String> = None;
    let mut needs_expansion = false;
    let mut fenced_matrix = false;
    while let Some(pos) = input[offset..].find(['\\', '%']) {
        let start = offset + pos;
        let tail = &input[start..];
        if tail.starts_with('%') {
            stripped
                .get_or_insert_with(|| String::with_capacity(input.len()))
                .push_str(&input[copy_from..start]);
            // Preserve the line ending for the parser's line-break handling.
            let end = tail.find(['\r', '\n']).unwrap_or(tail.len());
            copy_from = start + end;
            offset = copy_from;
        } else if let Some(end) = find_verb_end(input, start) {
            offset = end;
        } else if let Some((rest, token)) = super::split_token(tail) {
            let name = &token[1..];
            if !needs_expansion {
                fenced_matrix |= name == "begin" && starts_fenced_smallmatrix(rest);
                needs_expansion = matches!(
                    name,
                    "newcommand"
                        | "renewcommand"
                        | "providecommand"
                        | "def"
                        | "gdef"
                        | "edef"
                        | "xdef"
                        | "let"
                        | "futurelet"
                        | "begingroup"
                        | "endgroup"
                        | "expandafter"
                        | "noexpand"
                        | "global"
                        | "long"
                        | "protected"
                        | "@firstoftwo"
                        | "@secondoftwo"
                        | "@ifstar"
                        | "@ifnextchar"
                        | "TextOrMath"
                        | "newenvironment"
                ) || (!(direct_leading_plim && start == 0 && name == "plim")
                    && packages::is_builtin_macro_name(name));
            }
            offset = input.len() - rest.len();
        } else {
            break; // A final backslash is left for the parser to diagnose.
        }
    }
    let source = match stripped {
        Some(mut out) => {
            out.push_str(&input[copy_from..]);
            Cow::Owned(out)
        },
        None => Cow::Borrowed(input),
    };
    (source, needs_expansion, fenced_matrix)
}

/// Inspect only the environment name. Comments may separate the command
/// from its opening brace before the ordered stream strips them.
fn starts_fenced_smallmatrix(mut rest: &str) -> bool {
    loop {
        rest = rest.trim_start_matches(|ch: char| ch.is_ascii_whitespace());
        if !rest.starts_with('%') {
            break;
        }
        let Some(end) = rest.find(['\r', '\n']) else {
            return false;
        };
        rest = &rest[end..];
    }
    let Some(name) = rest.strip_prefix('{') else {
        return false;
    };
    let Some(end) = name
        .bytes()
        .take("psmallmatrix*}".len())
        .position(|b| b == b'}')
    else {
        return false;
    };
    packages::smallmatrix_fences(&name[..end]).is_some()
}

/// Apply the same literal replacements as the ordered processor. A mismatched
/// or nonliteral environment name hands the entire input back to that processor.
fn rewrite_literal_smallmatrix(input: &str) -> Option<String> {
    let mut stack: SmallVec<[&str; 4]> = SmallVec::new();
    let mut output = String::with_capacity(input.len());
    let mut copy_from = 0;
    let mut cursor = 0;
    while let Some(pos) = input[cursor..].find('\\') {
        let start = cursor + pos;
        if let Some(end) = find_verb_end(input, start) {
            cursor = end;
            continue;
        }
        let (rest, token) = super::split_token(&input[start..])?;
        cursor = input.len() - rest.len();
        let begin = match token {
            r"\begin" => true,
            r"\end" => false,
            _ => continue,
        };
        let (consumed, name) = literal_environment_group(rest)?;
        cursor += consumed;
        if begin {
            stack.push(name);
        } else if stack.pop() != Some(name) {
            return None;
        }
        if let Some(replacement) = ordered_stream::smallmatrix_environment_replacement(name, begin)
        {
            output.push_str(&input[copy_from..start]);
            output.push_str(&replacement);
            copy_from = cursor;
        }
    }
    if !stack.is_empty() || copy_from == 0 {
        return None;
    }
    output.push_str(&input[copy_from..]);
    Some(output)
}

fn literal_environment_group(input: &str) -> Option<(usize, &str)> {
    let skipped = input
        .bytes()
        .position(|byte| !byte.is_ascii_whitespace())
        .unwrap_or(input.len());
    let rest = &input[skipped..];
    let name = rest.strip_prefix('{')?;
    let end = name.find('}')?;
    let value = &name[..end];
    if value.is_empty()
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '*')
    {
        return None;
    }
    Some((skipped + end + 2, value))
}

/// If `input[start..]` begins `\verb` or `\verb*` followed by a delimiter,
/// return the byte index just past the matching closing delimiter. Verbatim
/// segments must not be altered by comment stripping or macro expansion.
pub(super) fn find_verb_end(input: &str, start: usize) -> Option<usize> {
    if !input[start..].starts_with("\\verb") {
        return None;
    }
    let mut i = start + "\\verb".len();
    // `\verbfoo` is an ordinary command named `verbfoo`, not a verbatim
    // command followed by `f` as its delimiter.
    if input[i..]
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic())
    {
        return None;
    }
    if input[i..].starts_with('*') {
        i += 1;
    }
    let delim = input[i..].chars().next()?;
    let after_delim = i + delim.len_utf8();
    // The parser consumes an unterminated `\verb` through the end of input,
    // so preserve that same literal region during preprocessing.
    match input[after_delim..].find(delim) {
        Some(end) => Some(after_delim + end + delim.len_utf8()),
        None => Some(input.len()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expanded(input: &str) -> String {
        preprocess_cow(input).unwrap().into_owned()
    }

    // --- Comment stripping ---

    #[test]
    fn processing_only_runs_for_actual_control_words_and_comments() {
        for untouched in [r"\definite x", r"\verb|\def\x{%}|", r"\%x"] {
            assert!(
                matches!(preprocess_cow(untouched), Ok(Cow::Borrowed(_))),
                "{untouched}"
            );
        }
        assert_eq!(expanded("a% skipped\rb"), "a\rb");
        assert_eq!(expanded("a% skipped\r\nb"), "a\r\nb");
        assert_eq!(expanded("a% \\gdef\\x{discarded}\nb"), "a\nb");
    }

    #[test]
    fn strip_simple_comment() {
        assert_eq!(expanded("a % comment\nb"), "a \nb");
    }

    #[test]
    fn strip_comment_at_start() {
        assert_eq!(expanded("% full line\nb"), "\nb");
    }

    #[test]
    fn strip_comment_at_end_no_newline() {
        assert_eq!(expanded("a % trailing"), "a ");
    }

    #[test]
    fn preserve_escaped_percent() {
        assert_eq!(expanded("a \\% b"), "a \\% b");
    }

    #[test]
    fn strip_multiple_comments() {
        assert_eq!(expanded("a % one\nb % two\nc"), "a \nb \nc");
    }

    #[test]
    fn no_comments_unchanged_and_borrows() {
        assert!(matches!(
            preprocess_cow("hello world"),
            Ok(Cow::Borrowed("hello world"))
        ));
    }

    #[test]
    fn public_and_parser_preprocessing_agree_on_literal_package_fences() {
        let source = r"\begin{psmallmatrix}a&b\\c&d\end{psmallmatrix}";
        let public = preprocess_cow(source).unwrap();
        assert!(
            public.contains(r"\mathopen{}\mathclose{\left("),
            "public expansion changed: {public}"
        );
        assert_eq!(prepare_math_input(source).unwrap(), public.as_ref());
    }

    #[test]
    fn dynamic_environment_names_and_shadowing_keep_ordered_expansion() {
        let alias = r"\def\E{psmallmatrix}\begin{\E}a\end{\E}";
        assert!(
            prepare_math_input(alias)
                .unwrap()
                .contains(r"\mathopen{}\mathclose{\left(")
        );
        let shadow = r"\newenvironment{psmallmatrix}{P}{Q}\begin{psmallmatrix}x\end{psmallmatrix}";
        assert_eq!(prepare_math_input(shadow).unwrap(), "PxQ");
        assert!(crate::from_latex(r"\def\E{\E}\begin{\E}x\end{\E}").is_err());
    }

    // --- Active definitions and expansion ---

    #[test]
    fn newcommand_forms_expand() {
        assert_eq!(expanded(r"\newcommand{\foo}{bar}\foo"), "bar");
        assert_eq!(expanded(r"\newcommand{\my}[1]{(#1)}\my{x}"), "(x)");
        assert_eq!(expanded(r"\newcommand{\my}[1]{(#1)}\my\phi"), r"(\phi)");
        assert_eq!(expanded(r"\newcommand{\my}[1]{(#1)}\my 3"), "(3)");
        assert_eq!(expanded(r"\newcommand{\f}[2][x]{#1+#2}\f{3}"), "x+3");
        assert_eq!(expanded(r"\newcommand{\f}[2][x]{#1+#2}\f[y]{3}"), "y+3");
        assert_eq!(expanded(r"\newcommand*{\star}{S}\star"), "S");
        assert_eq!(
            expanded(r"\newcommand{\foo}{old}\renewcommand{\foo}{new}\foo"),
            "new"
        );
    }

    #[test]
    fn long_literal_fences_keep_ordered_group_limits_even_in_text() {
        let n = crate::error::DEFAULT_MAX_DEPTH + 8;
        let source = format!(
            r"\begin{{psmallmatrix}}\text{{{}x{}}}\end{{psmallmatrix}}",
            "{".repeat(n),
            "}".repeat(n)
        );
        let public = preprocess_cow(&source);
        assert!(matches!(public, Err(ExpansionFailure::Depth { .. })));
        assert_eq!(prepare_math_input(&source), public);
    }

    #[test]
    fn every_literal_smallmatrix_fence_uses_the_bounded_rewrite() {
        for name in [
            "psmallmatrix",
            "bsmallmatrix",
            "Bsmallmatrix",
            "vsmallmatrix",
            "Vsmallmatrix",
            "psmallmatrix*",
            "bsmallmatrix*",
            "Bsmallmatrix*",
            "vsmallmatrix*",
            "Vsmallmatrix*",
        ] {
            let source = format!(r"\begin{{{name}}}a\end{{{name}}}");
            let public = preprocess_cow(&source).unwrap();
            assert_eq!(prepare_math_input(&source).unwrap(), public, "{name}");
        }
    }

    #[test]
    fn primitive_definitions_expand() {
        assert_eq!(expanded(r"\def\hi{hello}\hi"), "hello");
        assert_eq!(expanded(r"\def\bold#1{\mathbf{#1}}\bold{x}"), r"\mathbf{x}");
        assert_eq!(expanded(r"\def\foo|#1||{#1}\foo| x {y} ||"), " x {y} ");
        assert_eq!(expanded(r"\def\foo#1|#2{#1+#2}\foo 1 2 |34"), "1 2 +34");
        assert_eq!(expanded(r"\def\foo#1#{#1}\foo1^{23}"), "{1^23}");
        assert_eq!(expanded(r"\def\foo#1{\{#1\}}\foo{x}"), r"\{x\}");
        assert!(matches!(
            preprocess_cow(r"\def\foo\bar#1{#1}\foo\barX"),
            Err(ExpansionFailure::Arguments { .. })
        ));
        assert_eq!(expanded(r"\def\foo\bar#1{#1}\foo\bar X"), "X");
        assert_eq!(expanded(r"\gdef\foo#1#2{(#1,#2)}\foo 1 2"), "(1,2)");
    }

    #[test]
    fn malformed_primitive_definitions_reach_normal_parse_errors() {
        for input in [
            r"\gdef\foo#2{}",
            r"\gdef\foo#a{}",
            r"\gdef\foo#1#3{}",
            r"\gdef\foo#1#2#3#4#5#6#7#8#9#10{}",
            r"\gdef\foo\bar",
            r"\gdef{\foo}{}}",
        ] {
            assert!(crate::from_latex(input).is_err(), "{input}");
        }
    }

    #[test]
    fn environment_and_multiple_definitions_expand() {
        assert_eq!(
            expanded(r"\newenvironment{ary}{BEGIN}{END}\begin{ary}body\end{ary}"),
            "BEGINbodyEND"
        );
        assert_eq!(
            expanded(r"\newcommand{\a}{A}\def\b{B}\newenvironment{c}{C1}{C2}\a\b\begin{c}x\end{c}"),
            "ABC1xC2"
        );
    }

    #[test]
    fn recursive_and_chained_expansion() {
        assert_eq!(
            expanded(r"\newcommand{\inner}{I}\newcommand{\outer}{[\inner]}\outer"),
            "[I]"
        );
        assert_eq!(
            expanded(r"\newcommand{\a}{X}\newcommand{\b}{\a Y}\newcommand{\c}{\b Z}\c"),
            "XYZ"
        );
        let cycle = r"\newcommand{\r}{\r}\r";
        assert_eq!(
            preprocess_cow(cycle),
            Err(ExpansionFailure::Depth { limit: 64 })
        );
    }

    #[test]
    fn expansion_cap_is_reported() {
        let start = std::time::Instant::now();
        assert!(preprocess_cow(r"\newcommand{\a}{\a\a}\a").is_err());
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
        assert!(preprocess_cow(r"\newcommand{\a}{\a\a\a}\a").is_err());
    }

    #[test]
    fn non_macro_and_native_content_is_preserved() {
        assert_eq!(
            expanded(r"\newcommand{\foo}{bar}\alpha + \foo"),
            r"\alpha+ bar"
        );
        assert_eq!(expanded(r"\verb@\ip!@"), r"\verb@\ip!@");
        assert_eq!(expanded(r"\verb*!\ip{!}!"), r"\verb*!\ip{!}!");
        assert_eq!(
            expanded(r"\verb!\def\inside{literal}!"),
            r"\verb!\def\inside{literal}!"
        );
        assert_eq!(
            expanded(r"\def\outside{expanded}\verb!\def\inside{literal}!\outside"),
            r"\verb!\def\inside{literal}!expanded"
        );
        assert_eq!(
            expanded(r"\verb!\def\inside{literal}"),
            r"\verb!\def\inside{literal}"
        );
        assert_eq!(expanded(r"\def\x{expanded}\verbfoo\x"), r"\verbfooexpanded");
        assert_eq!(expanded(r"\verb@% still here@"), r"\verb@% still here@");
    }

    // --- Ordered/scoped TeX definitions -----------------------------------

    #[test]
    fn definitions_take_effect_at_their_location() {
        assert_eq!(expanded(r"\foo\def\foo{a}\foo\def\foo{b}\foo"), r"\foo ab");
    }

    #[test]
    fn text_distinguishes_source_delimiters_from_actual_replacement_spaces() {
        for (source, expected) in [
            (r"\def\word{abc}\text{\word def}", r"\text{abcdef}"),
            (r"\def\word{abc}\text{\word{} def}", r"\text{abc def}"),
            (r"\def\word{abc}\text{\word\ def}", r"\text{abc def}"),
            (r"\def\f#1{#1 x}\text{\f\alpha}", r"\text{α x}"),
            (r"\def\f#1{#1 x}\f\alpha", r"\alpha x"),
            (r"\def\f{\alpha x}\text{\f}", r"\text{αx}"),
            (r"\def\f{\alpha{} x}\text{\f}", r"\text{α x}"),
            (
                r"\def\q{x}\text{\verb|abc % Def |}",
                r"\texttt{abc \% Def }",
            ),
        ] {
            let xml = |s| crate::from_latex(s).unwrap().to_mathml().unwrap();
            assert_eq!(xml(source), xml(expected), "{source}");
        }
    }

    #[test]
    fn local_definitions_follow_groups() {
        assert_eq!(expanded(r"\def\x{1}\x{\def\x{2}\x}\x"), "1{2}1");
        assert_eq!(
            expanded(r"\def\x{1}\x{\def\x{2}\x{\gdef\x{3}\x}\x}\x"),
            "1{2{3}3}3"
        );
    }

    #[test]
    fn edef_and_xdef_capture_expansion_at_definition_time() {
        assert_eq!(expanded(r"\def\foo{a}\xdef\bar{\foo}\def\foo{}\bar"), "a");
        assert_eq!(
            expanded(r"\def\foo{a}\xdef\bar{\foo\noexpand\foo}\def\foo{b}\bar"),
            "ab"
        );
        assert_eq!(
            expanded(r"\def\foo{a}\xdef\bar{\def\noexpand\foo{}}\foo\bar\foo"),
            "a"
        );
    }

    #[test]
    fn let_copies_meaning_or_token_identity() {
        assert_eq!(expanded(r"\let\foo=\frac\def\frac{}\foo12"), r"\frac12");
        assert_eq!(expanded(r"\def\foo{1}\let\bar\foo\def\foo{2}\bar"), "1");
        assert_eq!(expanded(r"\let\foo=x\noexpand\foo"), "x");
        assert_eq!(expanded(r"\let\foo=x\edef\bar{\foo}\def\foo{y}\bar"), "y");
        assert_eq!(expanded(r"\let\foo=\alpha\foo x"), r"\alpha x");
        assert_eq!(expanded(r"\let\foo{\sqrt\foo1}"), r"\sqrt{1}");
        assert_eq!(expanded(r"\let\equals==a\equals b"), "a=b");
        assert_eq!(
            expanded(r"\def\:{\let\space= }\: \text{\space}"),
            r"\text{ }"
        );
        assert_eq!(expanded(r"\def\:{a}\let\x\:\x"), "a");
    }

    #[test]
    fn macro_replacement_consumes_outer_arguments() {
        assert_eq!(
            expanded(r"\def\pick#1{[#1]}\def\alias{\pick}\alias{x}"),
            "[x]"
        );
        assert_eq!(expanded(r"\def\f#1{#1x}\f{\alpha}"), r"\alpha x");
    }

    #[test]
    fn expandafter_expands_only_the_second_token_once() {
        assert_eq!(expanded(r"\expandafter\foo\bar"), r"\foo\bar");
    }

    #[test]
    fn expandafter_and_noexpand_match_tex_ordering() {
        assert_eq!(
            expanded(r"\def\foo#1#2{#1+#2}\def\bar{xy}\expandafter\foo\bar"),
            "x+y"
        );
        assert_eq!(
            expanded(r"\def\foo#1#2{#1+#2}\expandafter\foo\@firstoftwo{ab}{z}"),
            "a+b"
        );
        assert_eq!(
            expanded(r"\def\foo{x}\def\bar{\def\foo{y}}\expandafter\bar\foo"),
            "x"
        );
        assert_eq!(expanded(r"\noexpand\foo y\def\foo{x}"), r"\foo y");
        assert_eq!(expanded(r"\def\foo{x}\noexpand\foo y"), "y");
        assert_eq!(expanded(r"\noexpand\frac xy"), r"\frac xy");
        assert_eq!(expanded(r"\def\foo{x}\expandafter\foo\noexpand\foo"), "x");
        assert_eq!(expanded(r"\noexpand\def\foo{xy}\foo"), "xy");
        assert!(crate::from_latex(r"\def\foo{x}\noexpand\foo y").is_ok());
        assert!(crate::from_latex(r"\noexpand\def\foo{xy}\foo").is_ok());
        assert_eq!(expanded(r"\expandafter\foo\def\foo{x}"), r"\foo");
    }

    #[test]
    fn futurelet_and_protected_macros_work() {
        assert_eq!(expanded(r"\futurelet\foo\frac1{2+\foo}"), r"\frac1{2+1}");
        assert_eq!(
            expanded(r"\global\futurelet\foo\frac1{2+\foo}"),
            r"\frac1{2+1}"
        );
        assert_eq!(expanded(r"\protected\def\foo{a}\edef\bar{\foo}\bar"), "a");
        assert_eq!(
            expanded(r"\protected\def\foo{a}\edef\bar{\foo}\def\foo{b}\bar"),
            "b"
        );
    }

    #[test]
    fn selector_builtins_discard_unselected_branches() {
        assert_eq!(expanded(r"\@firstoftwo{a}{b}"), "a");
        assert_eq!(expanded(r"\@secondoftwo{a}{b}"), "b");
        assert_eq!(expanded(r"\@ifstar{a}{b}*"), "a");
        assert_eq!(expanded(r"\@ifstar{a}{b}"), "b");
        assert_eq!(expanded(r"\@ifnextchar x{a}{b}x"), "ax");
        assert_eq!(expanded(r"\@ifnextchar x{a}{b}y"), "by");
        assert_eq!(expanded(r"\def\@firstoftwo#1#2{#2}\@firstoftwo{a}{b}"), "b");
        assert_eq!(expanded(r"\let\pick\@secondoftwo\pick{a}{b}"), "b");
    }

    #[test]
    fn macro_argument_braces_do_not_create_a_scope() {
        assert_eq!(
            expanded(r"\def\x{1}\x\def\foo#1{#1}\foo{\x\def\x{2}\x}\x"),
            "1122"
        );
    }

    #[test]
    fn only_the_current_alignment_separator_ends_a_macro_scope() {
        assert_eq!(
            expanded(r"\def\x{a}\begin{matrix}\def\x{b}\x\cr\x\end{matrix}"),
            r"\begin{matrix}b\cr a\end{matrix}"
        );
        assert_eq!(
            expanded(r"\def\x{a}\begin{matrix}{\def\x{b}\text{u\\v}\x}&\x\end{matrix}"),
            r"\begin{matrix}{\text{u\\v}b}&a\end{matrix}"
        );
    }

    #[test]
    fn array_cells_are_group_scoped() {
        assert_eq!(
            expanded(r"\def\x{1}\begin{matrix}\x&\def\x{2}\x&\x\end{matrix}\x"),
            r"\begin{matrix}1&2&1\end{matrix}1"
        );
        assert_eq!(
            expanded(r"\def\x{1}\begin{matrix}\def\x{2}\x&\x\end{matrix}\x"),
            r"\begin{matrix}2&1\end{matrix}1"
        );
    }
}
