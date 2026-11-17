use mathema::Config;
use mathema::error::Strictness;
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;

#[test]
fn valid_macro_declarations_and_user_overrides_still_expand() {
    let config = Config::default();
    for (input, expected) in [
        (r"\newcommand{\foo}[1]{(#1)}\foo{x}", "(x)"),
        (r"\gdef\foo#1{#1}\foo{x}", "x"),
        (r"\newcommand{\sin}{S}\sin", "S"),
    ] {
        let result = LatexReader::parse(input, &config).expect(input);
        assert_eq!(result.expr.nodes.len(), expected.chars().count(), "{input}");
    }
}

#[test]
fn strict_parsing_reports_malformed_macro_syntax() {
    for input in [
        r"\gdef\foo1",
        r"\gdef\foo",
        r"\gdef{\foo}{}",
        r"\gdef{}{}",
        r"\let\foo",
        r"\futurelet\foo",
        r"\newcommand\foo[x]{}",
        r"\newcommand{}{}",
        r"\newcommand{\foo x}{}",
        r"\newcommand{\foo}[1.5]{}",
        r"\newcommand{\foo}[0][x]{}",
        r"\newcommand{\foo}[1]",
    ] {
        let error = LatexReader::parse(input, &Config::default())
            .expect_err("malformed declaration should fail in strict mode");
        let message = error.to_string();
        assert!(message.contains("malformed"), "{input}: {message}");
        assert!(
            !message.contains("expansion exceeded"),
            "syntax must not be reported as a resource limit: {input}: {message}"
        );
    }
}

#[test]
fn lenient_parsing_recovers_malformed_macro_syntax() {
    let config = Config::default().with_strictness(Strictness::Lenient);
    for input in [
        r"\gdef\foo1",
        r"\gdef\foo",
        r"\gdef{\foo}{}",
        r"\newcommand\foo[x]{}",
        r"\newcommand{\foo}[1.5]{}",
    ] {
        assert!(
            LatexReader::parse(input, &config).is_ok(),
            "lenient parsing should recover: {input}"
        );
    }
}

#[test]
fn unknown_control_words_keep_their_existing_fallback() {
    assert!(LatexReader::parse(r"\ordinaryunknown", &Config::default()).is_ok());
}
