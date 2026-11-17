use std::borrow::Cow;

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
/// macros are present.  Expansion errors carry the byte limit that stopped the
/// traversal so callers can report a controlled resource-limit diagnostic.
pub fn preprocess_cow(input: &str) -> Result<Cow<'_, str>, usize> {
    if !needs_preprocessing(input) {
        return Ok(Cow::Borrowed(input));
    }
    let stripped = strip_comments(input);
    // A comments-only change still needs an owned result, but does not need to
    // seed the macro table or walk a token queue.
    if !needs_expansion_processing(&stripped) {
        return Ok(Cow::Owned(stripped));
    }
    // Definitions are processed while walking the token stream.  In
    // particular, a definition inside a group must not leak into text before
    // it or after the group closes.  The ordered stream also owns expansion
    // caps and verbatim opacity guarantees.
    Ok(Cow::Owned(ordered_stream::expand(&stripped)?))
}

/// Fast-path check: does the input contain any syntax we need to process?
fn needs_preprocessing(input: &str) -> bool {
    input.contains('%') || needs_expansion_processing(input)
}

fn needs_expansion_processing(input: &str) -> bool {
    input.contains("\\newcommand")
        || input.contains("\\renewcommand")
        || input.contains("\\providecommand")
        || input.contains("\\def")
        || input.contains("\\gdef")
        || input.contains("\\edef")
        || input.contains("\\xdef")
        || input.contains("\\let")
        || input.contains("\\futurelet")
        || input.contains("\\expandafter")
        || input.contains("\\@firstoftwo")
        || input.contains("\\@secondoftwo")
        || input.contains("\\@ifstar")
        || input.contains("\\@ifnextchar")
        || input.contains("\\TextOrMath")
        || input.contains("\\noexpand")
        || input.contains("\\global")
        || input.contains("\\long")
        || input.contains("\\protected")
        || input.contains("\\newenvironment")
        || uses_builtin_macro(input)
}

/// Scan `input` for any `\command` that is a registered builtin (package)
/// macro. Returns on the first hit. Plain formulas — even those that use core
/// commands like `\frac` or `\alpha` — return `false` and stay zero-copy.
fn uses_builtin_macro(input: &str) -> bool {
    let mut s = input;
    while let Some(pos) = s.find('\\') {
        // A verbatim block owns everything through its closing delimiter.
        // Commands-looking text in that block must not trigger the
        // preprocessor's slow path or be considered for macro expansion.
        let command_start = &s[pos..];
        if let Some(end) = find_verb_end(command_start, 0) {
            s = &command_start[end..];
            continue;
        }
        let after = &s[pos + 1..];
        let name_len = super::control_word_len(after);
        if name_len > 0 && packages::is_builtin_macro_name(&after[..name_len]) {
            return true;
        }
        // Advance past this backslash (and any command/escaped char) to avoid
        // re-finding the same `\`.
        let skip = if name_len > 0 {
            name_len
        } else {
            after.chars().next().map_or(0, char::len_utf8)
        };
        if skip == 0 {
            break;
        }
        s = &after[skip..];
    }
    false
}

// ---------------------------------------------------------------------------
// Comment stripping and verbatim boundaries
// ---------------------------------------------------------------------------

fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let mut prev_was_backslash = false;

    while i < input.len() {
        // Verbatim content must be copied unchanged, including `%` characters.
        if input.as_bytes().get(i) == Some(&b'\\')
            && let Some(end) = find_verb_end(input, i)
        {
            out.push_str(&input[i..end]);
            i = end;
            prev_was_backslash = false;
            continue;
        }

        let ch = input[i..].chars().next().unwrap();
        let ch_len = ch.len_utf8();
        if ch == '%' && !prev_was_backslash {
            // Skip until end of line.
            i += ch_len;
            while i < input.len() {
                let c = input[i..].chars().next().unwrap();
                i += c.len_utf8();
                if c == '\n' {
                    out.push(c);
                    break;
                }
            }
            prev_was_backslash = false;
            continue;
        }
        out.push(ch);
        prev_was_backslash = ch == '\\' && !prev_was_backslash;
        i += ch_len;
    }
    out
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
    fn primitive_definitions_expand() {
        assert_eq!(expanded(r"\def\hi{hello}\hi"), "hello");
        assert_eq!(expanded(r"\def\bold#1{\mathbf{#1}}\bold{x}"), r"\mathbf{x}");
        assert_eq!(expanded(r"\def\foo|#1||{#1}\foo| x {y} ||"), " x {y} ");
        assert_eq!(expanded(r"\def\foo#1|#2{#1+#2}\foo 1 2 |34"), " 1 2 +34");
        assert_eq!(expanded(r"\def\foo#1#{#1}\foo1^{23}"), "{1^23}");
        assert_eq!(expanded(r"\def\foo#1{\{#1\}}\foo{x}"), r"\{x\}");
        assert_eq!(expanded(r"\def\foo\bar#1{#1}\foo\barX"), r"\foo\barX");
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
            "X Y Z"
        );
        assert!(expanded(r"\newcommand{\r}{\r}\r").contains(r"\r"));
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
            r"\alpha + bar"
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
        assert_eq!(expanded(r"\let\equals==a\equals b"), "a= b");
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
        assert_eq!(expanded(r"\def\foo{x}\noexpand\foo y"), " y");
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
