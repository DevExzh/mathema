#![no_main]
//! OMML → OMML must be a fixed point after one pass, on two routes.
//!
//! [`mathema::convert`] drops what pure OMML does not write (font sizes and
//! colors) before it normalizes, so its first output must be stable for every input.
//!
//! The other route normalizes what was read and gives it to the public writer,
//! which writes the tree it is given. A font size or a color that it does not
//! write keeps runs of that tree apart that normalization joins once they have
//! been written: for an input with sizes or colors the second output must be
//! stable there, for any other the first.
//!
//! What is written draws the characters of what was read.
use libfuzzer_sys::fuzz_target;
use mathema::ast::MathNode;
use mathema::ast::compact::CompactExpr;
use mathema::ast::normalize::normalize_compact_expr;
use mathema::ast::visit::{MutVisitor, Visitor, walk_node, walk_node_mut};
use mathema::error::Config;
use mathema::reader::Reader;
use mathema::reader::omml::OmmlReader;
use mathema::writer::Writer;
use mathema::writer::mathml::MathmlWriter;
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

/// [`pass`] over an output, which must be possible.
fn again(input: &str, written: &str) -> String {
    let config = Config::default();
    let Ok(mut read) = OmmlReader::parse_compact(written, &config) else {
        panic!(
            "round-trip failed: the output could not be re-parsed.\n\
             input:  {input:?}\n\
             write:  {written:?}"
        );
    };
    if let Err(e) = normalize_compact_expr(&mut read.expr) {
        panic!(
            "round-trip failed: the output could not be normalized: {e}\n\
             input:  {input:?}\n\
             write:  {written:?}"
        );
    }
    let Ok(next) = OmmlWriter::write_compact(&read.expr, &config) else {
        panic!(
            "round-trip failed: the output could not be written again.\n\
             input:  {input:?}\n\
             write:  {written:?}"
        );
    };
    next
}

/// The characters a tree draws, in no order: scripts and limits are written
/// in an order of their own.
fn drawn(expr: &CompactExpr) -> Option<Vec<char>> {
    let mathml = MathmlWriter::write_compact(expr, &Config::default()).ok()?;
    let mut inside_tag = false;
    let mut characters: Vec<char> = mathml
        .chars()
        .filter(|&ch| {
            let text = !inside_tag && ch != '<';
            inside_tag = ch == '<' || inside_tag && ch != '>';
            // (Invisible operators draw nothing.)
            text && !ch.is_whitespace() && !('\u{2061}'..='\u{2064}').contains(&ch)
        })
        .collect();
    characters.sort_unstable();
    Some(characters)
}

/// Whether the tree has a function application. The writer knows one by its
/// place in a list, which a color around it changes.
fn applies_function(expr: &CompactExpr) -> bool {
    struct Finder(bool);
    impl Visitor for Finder {
        fn visit_node(&mut self, node: &MathNode) {
            self.0 |= matches!(node, MathNode::Symbol(_, text) if text.contains('\u{2061}'));
            if !self.0 {
                walk_node(self, node);
            }
        }
    }
    let mut finder = Finder(false);
    finder.visit_expr(&expr.to_math_expr());
    finder.0
}

/// A color changes nothing of what is written: a tree is written as it is with
/// each color replaced by what it colors.
fn colors_are_content(read: &CompactExpr, written: &str) {
    struct Uncolor;
    impl MutVisitor for Uncolor {
        fn visit_node_mut(&mut self, node: &mut MathNode) {
            while let MathNode::Color { content, .. } = node {
                let content = std::mem::replace(&mut **content, MathNode::Row(Vec::new()));
                *node = content;
            }
            walk_node_mut(self, node);
        }
    }
    let mut plain = read.to_math_expr();
    Uncolor.visit_expr_mut(&mut plain);
    assert_eq!(
        OmmlWriter::write(&plain, &Config::default())
            .ok()
            .as_deref(),
        Some(written),
        "colors change what is written: {read:?}"
    );
}

/// What is written without a loss (but of sizes, colors and fonts of text)
/// draws what was read.
fn keeps_text(input: &str) {
    let config = Config::default();
    let Ok(read) = OmmlReader::parse_compact(input, &config) else {
        return;
    };
    let (Ok((written, lost)), Ok(converted)) = (
        OmmlWriter::write_compact_with_diagnostics(&read.expr, &config),
        convert(input, Format::Omml, Format::Omml),
    ) else {
        return;
    };
    if has_unwritten(&read.expr) && !applies_function(&read.expr) {
        colors_are_content(&read.expr, &written);
    }
    if !lost
        .iter()
        .all(|d| d.message.contains("host run properties") || d.message.contains("font sizes"))
    {
        return;
    }
    for written in [written, converted] {
        let Ok(again) = OmmlReader::parse_compact(&written, &config) else {
            panic!("the output could not be re-parsed.\ninput: {input:?}\nwrite: {written:?}");
        };
        assert_eq!(
            drawn(&read.expr),
            drawn(&again.expr),
            "the output draws other characters.\ninput: {input:?}\nwrite: {written:?}"
        );
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };

    keeps_text(input);

    // The route that writes the normalized tree.
    if let Some((first, unwritten)) = pass(input) {
        let second = again(input, &first);
        if unwritten {
            let third = again(input, &second);
            assert_eq!(
                second, third,
                "OMML round-trip does not settle.\n\
                     input:  {input:?}\n\
                     write1: {first:?}\n\
                     write2: {second:?}\n\
                     write3: {third:?}"
            );
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
