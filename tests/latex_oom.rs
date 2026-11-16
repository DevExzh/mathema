use mathema::Config;
use mathema::error::Strictness;
use mathema::reader::Reader;
use mathema::reader::latex::LatexReader;

// Original 329-byte libFuzzer OOM reproducer, kept as hex so the control
// characters and CRLF bytes remain exact in the regression test.
const OLD_FONT_OOM_HEX: &str = concat!(
    "0a200a0a200a210d3b272962762c765c726d7b76765c726d7b765c726d7b76765c726d7b3b272962",
    "763b5c766227296276767b3b273b272962763b27296276767b3b2729625c726d7b3b272962767676",
    "5c726d7b767b3b3b27270d20210d3b272962762c765c726d7b76765c726d7b765c726d7b76765c72",
    "6d7b3b272962763b5c764227296276767b3b273b272962763b27296276765c726d7b765c726d7b76",
    "765c726d7b3b272962763b5c766227296276767b3b273b272962763b27296276767b3b2729625c72",
    "6d7b3b2729627676765c726d7b767b3b3b27270d20210d3b272962762c765c726d7b76765c726d7b",
    "765c726d7b76765c726d7b3b272962763b5c764227296276767b3b273b272962763b27296276767b",
    "3b2729625c726d7b3b2729627676765c726d7b767b3b2729625c726d7b3b2729627676765c726d7b",
    "767b3b3b27270d202d"
);

fn reproducer() -> String {
    let bytes: Vec<u8> = OLD_FONT_OOM_HEX
        .as_bytes()
        .chunks(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("hex is ASCII"), 16)
                .expect("reproducer hex is valid")
        })
        .collect();
    String::from_utf8(bytes).expect("reproducer is UTF-8")
}

#[test]
fn malformed_old_font_scopes_do_not_retry_until_oom() {
    let input = reproducer();
    assert_eq!(input.len(), 329);

    let strict = LatexReader::parse(&input, &Config::default());
    assert!(
        strict.is_err(),
        "strict parsing should reject the malformed input"
    );

    let lenient = LatexReader::parse(
        &input,
        &Config::default().with_strictness(Strictness::Lenient),
    );
    assert!(lenient.is_ok(), "lenient recovery should remain available");
}

#[test]
fn valid_old_font_scopes_still_parse() {
    for input in [r"\rm", r"\rm x", r"\sf{d}", r"\tt{mono}"] {
        assert!(
            LatexReader::parse(input, &Config::default()).is_ok(),
            "valid old-style font input should parse: {input:?}"
        );
    }
}
