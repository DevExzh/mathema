#![no_main]
//! OMML → OMML must be a fixed point after one pass, on two routes.
//!
//! [`mathema::convert`] drops what pure OMML does not write (font sizes) before
//! it normalizes, so its first output must be stable for every input.
//!
//! The other route normalizes what was read and gives it to the public writer,
//! which writes the tree it is given. A font size or a color that it does not
//! write keeps runs of that tree apart that normalization joins once they have
//! been written: for an input with sizes or colors the second output must be
//! stable there, for any other the first.
use libfuzzer_sys::fuzz_target;
use mathema::ast::MathNode;
use mathema::ast::compact::CompactExpr;
use mathema::ast::normalize::normalize_compact_expr;
use mathema::ast::visit::{Visitor, walk_node};
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::omml::OmmlReader;
use mathema::writer::Writer;
use mathema::writer::omml::OmmlWriter;
use mathema::{Format, convert};

/// Whether the tree has sizes or colors, which pure OMML does not write.
fn has_unwritten(expr: &CompactExpr) -> bool {
    struct Finder(bool);
    impl Visitor for Finder {
        fn visit_node(&mut self, node: &MathNode) {
            self.0 |= matches!(node, MathNode::Color { .. });
            if !self.0 {
                walk_node(self, node);
            }
        }
    }
    let mut finder = Finder(expr.arena.has_attrs());
    if !finder.0 {
        finder.visit_expr(&expr.to_math_expr());
    }
    finder.0
}

/// Parse → normalize → write. `None` when the input is refused.
fn pass(input: &str) -> Option<(String, bool)> {
    let config = Config::default();
    let mut read = OmmlReader::parse_compact(input, &config).ok()?;
    let unwritten = has_unwritten(&read.expr);
    normalize_compact_expr(&mut read.expr).ok()?;
    let written = OmmlWriter::write_compact(&read.expr, &config).ok()?;
    Some((written, unwritten))
}

/// [`pass`] over an output. `None` when it exceeds a limit in normalization.
fn again(input: &str, written: &str) -> Option<String> {
    let config = Config::default();
    let Ok(mut read) = OmmlReader::parse_compact(written, &config) else {
        panic!(
            "round-trip failed: the output could not be re-parsed.\n\
             input:  {input:?}\n\
             write:  {written:?}"
        );
    };
    normalize_compact_expr(&mut read.expr).ok()?;
    let Ok(next) = OmmlWriter::write_compact(&read.expr, &config) else {
        panic!(
            "round-trip failed: the output could not be written again.\n\
             input:  {input:?}\n\
             write:  {written:?}"
        );
    };
    Some(next)
}

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };

    // The route that writes the normalized tree.
    if let Some((first, unwritten)) = pass(input)
        && let Some(second) = again(input, &first)
    {
        if unwritten {
            if let Some(third) = again(input, &second) {
                assert_eq!(
                    second, third,
                    "OMML round-trip does not settle.\n\
                     input:  {input:?}\n\
                     write1: {first:?}\n\
                     write2: {second:?}\n\
                     write3: {third:?}"
                );
            }
        } else {
            assert_eq!(
                first, second,
                "OMML round-trip not idempotent.\n\
                 input:  {input:?}\n\
                 write1: {first:?}\n\
                 write2: {second:?}"
            );
        }
    }

    // The route that drops first.
    if let Ok(first) = convert(input, Format::Omml, Format::Omml) {
        let second = convert(&first, Format::Omml, Format::Omml).unwrap_or_else(|e| {
            panic!(
                "conversion failed: the output could not be converted again: {e}\n\
                 input:  {input:?}\n\
                 write1: {first:?}"
            )
        });
        assert_eq!(
            first, second,
            "OMML conversion not idempotent.\n\
             input:  {input:?}\n\
             write1: {first:?}\n\
             write2: {second:?}"
        );
    }
});
