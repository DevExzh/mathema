use crate::ast::arena::AstArena;
use crate::ast::compact::{CompactExpr, CompactNode, NodeId};
use crate::ast::*;
use crate::error::{Config, Result};
use crate::writer::Writer as MathWriter;

pub struct MtefWriter;

impl MathWriter for MtefWriter {
    /// Write AST to MTEF binary, returned as hex string.
    fn write(expr: &MathExpr, _config: &Config) -> Result<String> {
        let mut buf = Vec::with_capacity(64);
        // MTEF v5 header
        buf.push(5); // version
        buf.push(1); // platform (Win)
        buf.push(1); // product (MathType)
        buf.push(5); // product version major
        buf.push(0); // product version minor
        buf.push(0); // null-terminated app key (empty)
        buf.push(if expr.display == DisplayStyle::Inline {
            1
        } else {
            0
        });

        // SIZE record (required before content)
        write_tag_options(&mut buf, SIZE, 0);
        buf.push(12); // 12pt default

        // Wrap in a LINE
        write_tag_options(&mut buf, LINE, 0);
        for node in &expr.nodes {
            write_node(&mut buf, node)?;
        }
        buf.push(END);

        // Hex encode — lookup table, no formatting overhead
        let hex = hex_encode(&buf);
        Ok(hex)
    }

    /// Write from compact arena representation, reading directly from the arena
    /// without reconstructing `MathExpr`.
    fn write_compact(expr: &CompactExpr, _config: &Config) -> Result<String> {
        let mut buf = Vec::with_capacity(64);
        let arena = &expr.arena;

        // MTEF v5 header
        buf.push(5); // version
        buf.push(1); // platform (Win)
        buf.push(1); // product (MathType)
        buf.push(5); // product version major
        buf.push(0); // product version minor
        buf.push(0); // null-terminated app key (empty)
        buf.push(if expr.display == DisplayStyle::Inline {
            1
        } else {
            0
        });

        // SIZE record (required before content)
        write_tag_options(&mut buf, SIZE, 0);
        buf.push(12); // 12pt default

        // Wrap in a LINE
        write_tag_options(&mut buf, LINE, 0);
        for &root_id in arena.children(expr.roots) {
            write_node_c(&mut buf, arena, root_id)?;
        }
        buf.push(END);

        // Hex encode — lookup table, no formatting overhead
        let hex = hex_encode(&buf);
        Ok(hex)
    }
}

/// Hex lookup table for fast byte-to-hex conversion.
const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";

/// Encode bytes as lowercase hex string using a lookup table.
#[inline]
fn hex_encode(data: &[u8]) -> String {
    let mut hex = String::with_capacity(data.len() * 2);
    for &b in data {
        hex.push(HEX_CHARS[(b >> 4) as usize] as char);
        hex.push(HEX_CHARS[(b & 0x0f) as usize] as char);
    }
    hex
}

fn write_tag_options(buf: &mut Vec<u8>, tag: u8, options: u8) {
    buf.push(tag);
    buf.push(options);
}

const END: u8 = 0;
const LINE: u8 = 1;
const CHAR: u8 = 2;
const TMPL: u8 = 3;
const MATRIX: u8 = 5;
const SIZE: u8 = 9;

const TF_TEXT: u8 = 1;
const TF_FUNCTION: u8 = 2;
const TF_VARIABLE: u8 = 3;
const TF_LCGREEK: u8 = 4;
const TF_UCGREEK: u8 = 5;
const TF_SYMBOL: u8 = 6;
const TF_NUMBER: u8 = 8;

fn write_node(buf: &mut Vec<u8>, node: &MathNode) -> Result<()> {
    match node {
        MathNode::Number(s) => {
            for ch in s.chars() {
                write_char(buf, TF_NUMBER, ch)?;
            }
        },
        MathNode::Identifier(s) => {
            for ch in s.chars() {
                let (tf, code) = resolve_to_mtef(ch);
                write_char(buf, tf, code)?;
            }
        },
        MathNode::Operator(attr) => {
            for ch in attr.name.chars() {
                write_char(buf, TF_FUNCTION, ch)?;
            }
        },
        MathNode::Symbol(_, s) => {
            for ch in s.chars() {
                write_char(buf, TF_SYMBOL, ch)?;
            }
        },
        MathNode::Text(_, s) => {
            for ch in s.chars() {
                write_char(buf, TF_TEXT, ch)?;
            }
        },
        MathNode::Space(_) => {
            write_char(buf, TF_TEXT, ' ')?;
        },
        MathNode::Row(children) => {
            for child in children {
                write_node(buf, child)?;
            }
        },
        MathNode::Fenced {
            open,
            close,
            content,
        } => {
            let (selector, variation) = fence_to_template(open, close);
            write_tag_options(buf, TMPL, 0);
            buf.push(selector);
            buf.push(variation);
            buf.push(0); // options
            // Content slot
            write_tag_options(buf, LINE, 0);
            for item in content {
                match item {
                    FencedItem::Element(n) => write_node(buf, n)?,
                    FencedItem::Middle(_) => {},
                }
            }
            buf.push(END);
            buf.push(END); // end template
        },
        MathNode::Subscript { base, sub } => {
            write_tag_options(buf, TMPL, 0);
            buf.push(15); // scripts
            buf.push(1); // subscript variation
            buf.push(0);
            write_line_slot(buf, base)?;
            write_line_slot(buf, sub)?;
            buf.push(END);
        },
        MathNode::Superscript { base, sup } => {
            write_tag_options(buf, TMPL, 0);
            buf.push(15); // scripts
            buf.push(0); // superscript variation
            buf.push(0);
            write_line_slot(buf, base)?;
            write_line_slot(buf, sup)?;
            buf.push(END);
        },
        MathNode::SubSup { base, sub, sup } => {
            write_tag_options(buf, TMPL, 0);
            buf.push(15); // scripts
            buf.push(2); // subsup variation
            buf.push(0);
            write_line_slot(buf, base)?;
            write_line_slot(buf, sub)?;
            write_line_slot(buf, sup)?;
            buf.push(END);
        },
        MathNode::Fraction { num, den, kind } => {
            let variation = match kind {
                FractionKind::Skewed => 0, // map to slash fraction selector 40 later
                _ => 0,
            };
            let selector = match kind {
                FractionKind::Skewed => 40,
                _ => 14,
            };
            write_tag_options(buf, TMPL, 0);
            buf.push(selector);
            buf.push(variation);
            buf.push(0);
            write_line_slot(buf, num)?;
            write_line_slot(buf, den)?;
            buf.push(END);
        },
        MathNode::Sqrt(inner) => {
            write_tag_options(buf, TMPL, 0);
            buf.push(13); // root
            buf.push(0); // square root
            buf.push(0);
            write_line_slot(buf, inner)?;
            buf.push(END);
        },
        MathNode::Root { index, radicand } => {
            write_tag_options(buf, TMPL, 0);
            buf.push(13); // root
            buf.push(1); // nth root
            buf.push(0);
            write_line_slot(buf, index)?;
            write_line_slot(buf, radicand)?;
            buf.push(END);
        },
        MathNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => {
            let (selector_base, _) = nary_op_to_selector(*op);
            let selector = if *limits == LimitStyle::AboveBelow {
                selector_base
            } else {
                selector_base + 1
            };
            let variation = match (sub, sup) {
                (Some(_), Some(_)) => 1,
                (Some(_), None) => 0,
                (None, Some(_)) => 0,
                (None, None) => 2,
            };
            write_tag_options(buf, TMPL, 0);
            buf.push(selector);
            buf.push(variation);
            buf.push(0);
            if let Some(s) = sub {
                write_line_slot(buf, s)?;
            }
            if let Some(s) = sup {
                write_line_slot(buf, s)?;
            }
            write_line_slot(buf, body)?;
            buf.push(END);
        },
        MathNode::Accent { base, accent, .. } => {
            // Write base char with embellishment
            let emb = accent_to_embell(*accent);
            write_tag_options(buf, LINE, 0);
            // Write the base
            write_node(buf, base)?;
            // Write embellishment record
            if emb != 0 {
                write_tag_options(buf, 6, 0); // EMBELL
                buf.push(emb);
            }
            buf.push(END);
        },
        MathNode::Overscript {
            base,
            over,
            movable_limits,
        } => {
            if *movable_limits {
                // Use sum-like template
                write_node(buf, base)?;
            } else {
                write_tag_options(buf, TMPL, 0);
                buf.push(17); // overbar
                buf.push(0);
                buf.push(0);
                write_line_slot(buf, base)?;
                buf.push(END);
            }
            // MTEF limitation: overscript content not representable in template structure
            let _ = over;
        },
        MathNode::Underscript {
            base,
            under,
            movable_limits,
        } => {
            if *movable_limits {
                write_node(buf, base)?;
            } else {
                write_tag_options(buf, TMPL, 0);
                buf.push(16); // underbar
                buf.push(0);
                buf.push(0);
                write_line_slot(buf, base)?;
                buf.push(END);
            }
            // MTEF limitation: underscript content not representable in template structure
            let _ = under;
        },
        MathNode::UnderOver {
            base, under, over, ..
        } => {
            // Nest as overscript containing underscript
            write_node(buf, base)?;
            // MTEF limitation: under/over content not representable in template structure
            let _ = (under, over);
        },
        MathNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            // MTEF doesn't have a direct prescripts template.
            // Write as subsup with the prescripts before the base.
            if let Some(sub) = pre_sub {
                write_node(buf, sub)?;
            }
            if let Some(sup) = pre_sup {
                write_node(buf, sup)?;
            }
            write_node(buf, base)?;
        },
        MathNode::Array { columns, rows } => {
            let num_rows = u8::try_from(rows.len()).map_err(|_| {
                crate::error::Error::Unsupported(format!(
                    "MTEF matrix rows ({}) exceeds maximum of 255",
                    rows.len()
                ))
            })?;
            let num_cols = u8::try_from(columns.len()).map_err(|_| {
                crate::error::Error::Unsupported(format!(
                    "MTEF matrix columns ({}) exceeds maximum of 255",
                    columns.len()
                ))
            })?;
            write_tag_options(buf, MATRIX, 0);
            buf.push(0); // valign
            buf.push(0); // halign
            buf.push(num_rows);
            buf.push(num_cols);
            for _ in 0..num_cols {
                buf.push(1); // center alignment
            }
            for row in rows {
                for cell in row {
                    write_tag_options(buf, LINE, 0);
                    for node in cell {
                        write_node(buf, node)?;
                    }
                    buf.push(END);
                }
            }
            buf.push(END);
        },
        MathNode::Phantom(inner) => {
            // No MTEF phantom; just write content
            write_node(buf, inner)?;
        },
        MathNode::Boxed(inner) => {
            write_node(buf, inner)?;
        },
        MathNode::Cancel { content, .. } => {
            write_node(buf, content)?;
        },
        MathNode::Scaled { content, .. } => {
            write_node(buf, content)?;
        },
        MathNode::Styled(_, children) => {
            for child in children {
                write_node(buf, child)?;
            }
        },
    }
    Ok(())
}

fn write_line_slot(buf: &mut Vec<u8>, node: &MathNode) -> Result<()> {
    write_tag_options(buf, LINE, 0);
    write_node(buf, node)?;
    buf.push(END);
    Ok(())
}

fn write_char(buf: &mut Vec<u8>, typeface: u8, ch: char) -> Result<()> {
    let code = u16::try_from(u32::from(ch)).map_err(|_| {
        crate::error::Error::Unsupported(format!(
            "character U+{:04X} exceeds MTEF 16-bit encoding limit",
            u32::from(ch)
        ))
    })?;
    write_tag_options(buf, CHAR, 0);
    buf.push(128 + typeface);
    buf.push((code & 0xFF) as u8);
    buf.push(((code >> 8) & 0xFF) as u8);
    Ok(())
}

fn resolve_to_mtef(ch: char) -> (u8, char) {
    // Check if it's a Greek letter
    match ch {
        '\u{03B1}'..='\u{03C9}' => (TF_LCGREEK, ch),
        '\u{0391}'..='\u{03A9}' => (TF_UCGREEK, ch),
        _ if ch.is_ascii_alphabetic() => (TF_VARIABLE, ch),
        _ if ch.is_ascii_digit() => (TF_NUMBER, ch),
        _ => (TF_VARIABLE, ch),
    }
}

fn fence_to_template(open: &str, close: &str) -> (u8, u8) {
    let selector = match (open, close) {
        ("(", ")") => 1,
        ("{", "}") => 2,
        ("[", "]") => 3,
        ("|", "|") => 4,
        _ if open.contains('\u{27E8}') || close.contains('\u{27E9}') => 0,
        _ if open.contains('\u{2016}') || close.contains('\u{2016}') => 5,
        _ if open.contains('\u{230A}') || close.contains('\u{230B}') => 6,
        _ if open.contains('\u{2308}') || close.contains('\u{2309}') => 7,
        _ => 1, // default to parentheses
    };
    let variation = match (open.is_empty(), close.is_empty()) {
        (false, false) => 0,
        (false, true) => 1,
        (true, false) => 2,
        (true, true) => 0,
    };
    (selector, variation)
}

fn nary_op_to_selector(op: char) -> (u8, u8) {
    match op {
        '\u{2211}' => (29, 0), // sum: display=29, inline=30
        '\u{220F}' => (31, 0), // product
        '\u{2210}' => (33, 0), // coproduct
        '\u{222B}' => (21, 0), // integral
        '\u{222C}' => (22, 0), // double integral
        '\u{222D}' => (23, 0), // triple integral
        '\u{22C3}' => (35, 0), // union
        '\u{22C2}' => (37, 0), // intersection
        _ => (29, 0),          // default to sum
    }
}

fn accent_to_embell(accent: char) -> u8 {
    match accent {
        '\u{0307}' => 2,  // dot
        '\u{0308}' => 3,  // ddot
        '\u{20DB}' => 4,  // tdot
        '\u{0303}' => 8,  // tilde
        '\u{0302}' => 9,  // hat
        '\u{0338}' => 10, // not
        '\u{20D7}' => 11, // vec (right arrow)
        '\u{0332}' => 16, // underbar
        '\u{0305}' => 17, // overbar
        '\u{0306}' => 20, // breve
        _ => 0,
    }
}

// ── Compact arena helpers ────────────────────────────────────────────────

fn write_node_c(buf: &mut Vec<u8>, arena: &AstArena, id: NodeId) -> Result<()> {
    match *arena.node(id) {
        CompactNode::Number(sid) => {
            for ch in arena.str_val(sid).chars() {
                write_char(buf, TF_NUMBER, ch)?;
            }
        },
        CompactNode::Identifier(sid) => {
            for ch in arena.str_val(sid).chars() {
                let (tf, code) = resolve_to_mtef(ch);
                write_char(buf, tf, code)?;
            }
        },
        CompactNode::Operator { name, .. } => {
            for ch in arena.str_val(name).chars() {
                write_char(buf, TF_FUNCTION, ch)?;
            }
        },
        CompactNode::Symbol(_, sid) => {
            for ch in arena.str_val(sid).chars() {
                write_char(buf, TF_SYMBOL, ch)?;
            }
        },
        CompactNode::Text(_, sid) => {
            for ch in arena.str_val(sid).chars() {
                write_char(buf, TF_TEXT, ch)?;
            }
        },
        CompactNode::Space(_) => {
            write_char(buf, TF_TEXT, ' ')?;
        },
        CompactNode::Row(span) => {
            for &child_id in arena.children(span) {
                write_node_c(buf, arena, child_id)?;
            }
        },
        CompactNode::Fenced {
            open,
            close,
            content,
        } => {
            let (selector, variation) =
                fence_to_template(arena.str_val(open), arena.str_val(close));
            write_tag_options(buf, TMPL, 0);
            buf.push(selector);
            buf.push(variation);
            buf.push(0); // options
            // Content slot
            write_tag_options(buf, LINE, 0);
            for &child_id in arena.children(content) {
                // Skip Middle nodes (like the original FencedItem::Middle)
                if matches!(*arena.node(child_id), CompactNode::Middle(_)) {
                    continue;
                }
                write_node_c(buf, arena, child_id)?;
            }
            buf.push(END);
            buf.push(END); // end template
        },
        CompactNode::Subscript { base, sub } => {
            write_tag_options(buf, TMPL, 0);
            buf.push(15); // scripts
            buf.push(1); // subscript variation
            buf.push(0);
            write_line_slot_c(buf, arena, base)?;
            write_line_slot_c(buf, arena, sub)?;
            buf.push(END);
        },
        CompactNode::Superscript { base, sup } => {
            write_tag_options(buf, TMPL, 0);
            buf.push(15); // scripts
            buf.push(0); // superscript variation
            buf.push(0);
            write_line_slot_c(buf, arena, base)?;
            write_line_slot_c(buf, arena, sup)?;
            buf.push(END);
        },
        CompactNode::SubSup { base, sub, sup } => {
            write_tag_options(buf, TMPL, 0);
            buf.push(15); // scripts
            buf.push(2); // subsup variation
            buf.push(0);
            write_line_slot_c(buf, arena, base)?;
            write_line_slot_c(buf, arena, sub)?;
            write_line_slot_c(buf, arena, sup)?;
            buf.push(END);
        },
        CompactNode::Fraction { kind, num, den } => {
            let variation = match kind {
                FractionKind::Skewed => 0,
                _ => 0,
            };
            let selector = match kind {
                FractionKind::Skewed => 40,
                _ => 14,
            };
            let _ = variation; // suppress unused warning; variation is always 0
            write_tag_options(buf, TMPL, 0);
            buf.push(selector);
            buf.push(0); // variation
            buf.push(0);
            write_line_slot_c(buf, arena, num)?;
            write_line_slot_c(buf, arena, den)?;
            buf.push(END);
        },
        CompactNode::Sqrt(inner) => {
            write_tag_options(buf, TMPL, 0);
            buf.push(13); // root
            buf.push(0); // square root
            buf.push(0);
            write_line_slot_c(buf, arena, inner)?;
            buf.push(END);
        },
        CompactNode::Root { index, radicand } => {
            write_tag_options(buf, TMPL, 0);
            buf.push(13); // root
            buf.push(1); // nth root
            buf.push(0);
            write_line_slot_c(buf, arena, index)?;
            write_line_slot_c(buf, arena, radicand)?;
            buf.push(END);
        },
        CompactNode::Nary {
            op,
            sub,
            sup,
            body,
            limits,
        } => {
            let (selector_base, _) = nary_op_to_selector(op);
            let selector = if limits == LimitStyle::AboveBelow {
                selector_base
            } else {
                selector_base + 1
            };
            let variation = match (sub.is_some(), sup.is_some()) {
                (true, true) => 1,
                (true, false) => 0,
                (false, true) => 0,
                (false, false) => 2,
            };
            write_tag_options(buf, TMPL, 0);
            buf.push(selector);
            buf.push(variation);
            buf.push(0);
            if sub.is_some() {
                write_line_slot_c(buf, arena, sub)?;
            }
            if sup.is_some() {
                write_line_slot_c(buf, arena, sup)?;
            }
            write_line_slot_c(buf, arena, body)?;
            buf.push(END);
        },
        CompactNode::Accent { base, accent, .. } => {
            let emb = accent_to_embell(accent);
            write_tag_options(buf, LINE, 0);
            write_node_c(buf, arena, base)?;
            if emb != 0 {
                write_tag_options(buf, 6, 0); // EMBELL
                buf.push(emb);
            }
            buf.push(END);
        },
        CompactNode::Overscript {
            base,
            over,
            movable_limits,
        } => {
            if movable_limits {
                write_node_c(buf, arena, base)?;
            } else {
                write_tag_options(buf, TMPL, 0);
                buf.push(17); // overbar
                buf.push(0);
                buf.push(0);
                write_line_slot_c(buf, arena, base)?;
                buf.push(END);
            }
            let _ = over;
        },
        CompactNode::Underscript {
            base,
            under,
            movable_limits,
        } => {
            if movable_limits {
                write_node_c(buf, arena, base)?;
            } else {
                write_tag_options(buf, TMPL, 0);
                buf.push(16); // underbar
                buf.push(0);
                buf.push(0);
                write_line_slot_c(buf, arena, base)?;
                buf.push(END);
            }
            let _ = under;
        },
        CompactNode::UnderOver {
            base, under, over, ..
        } => {
            write_node_c(buf, arena, base)?;
            let _ = (under, over);
        },
        CompactNode::PreScripts {
            base,
            pre_sub,
            pre_sup,
        } => {
            if pre_sub.is_some() {
                write_node_c(buf, arena, pre_sub)?;
            }
            if pre_sup.is_some() {
                write_node_c(buf, arena, pre_sup)?;
            }
            write_node_c(buf, arena, base)?;
        },
        CompactNode::Array {
            cells, num_cols, ..
        } => {
            let nc = num_cols as usize;
            let cell_ids = arena.children(cells);
            let num_rows = if nc > 0 { cell_ids.len() / nc } else { 0 };
            let num_rows_u8 = u8::try_from(num_rows).map_err(|_| {
                crate::error::Error::Unsupported(format!(
                    "MTEF matrix rows ({num_rows}) exceeds maximum of 255"
                ))
            })?;
            let num_cols_u8 = u8::try_from(nc).map_err(|_| {
                crate::error::Error::Unsupported(format!(
                    "MTEF matrix columns ({nc}) exceeds maximum of 255"
                ))
            })?;
            write_tag_options(buf, MATRIX, 0);
            buf.push(0); // valign
            buf.push(0); // halign
            buf.push(num_rows_u8);
            buf.push(num_cols_u8);
            for _ in 0..nc {
                buf.push(1); // center alignment
            }
            for row in 0..num_rows {
                for col in 0..nc {
                    let cell_id = cell_ids[row * nc + col];
                    write_tag_options(buf, LINE, 0);
                    // Each cell is a Row node wrapping the cell contents
                    match *arena.node(cell_id) {
                        CompactNode::Row(span) => {
                            for &child_id in arena.children(span) {
                                write_node_c(buf, arena, child_id)?;
                            }
                        },
                        _ => {
                            write_node_c(buf, arena, cell_id)?;
                        },
                    }
                    buf.push(END);
                }
            }
            buf.push(END);
        },
        CompactNode::Phantom(inner) => {
            write_node_c(buf, arena, inner)?;
        },
        CompactNode::Boxed(inner) => {
            write_node_c(buf, arena, inner)?;
        },
        CompactNode::Cancel { content, .. } => {
            write_node_c(buf, arena, content)?;
        },
        CompactNode::Scaled { content, .. } => {
            write_node_c(buf, arena, content)?;
        },
        CompactNode::Styled(_, span) => {
            for &child_id in arena.children(span) {
                write_node_c(buf, arena, child_id)?;
            }
        },
        CompactNode::Color { content, .. } => {
            write_node_c(buf, arena, content)?;
        },
        // Helper nodes that shouldn't appear as standalone roots
        CompactNode::Middle(_) | CompactNode::ColAlign(_) | CompactNode::Rule { .. } | CompactNode::StyleSwitch(_) | CompactNode::SizeScale(_) => {},
    }
    Ok(())
}

fn write_line_slot_c(buf: &mut Vec<u8>, arena: &AstArena, id: NodeId) -> Result<()> {
    write_tag_options(buf, LINE, 0);
    write_node_c(buf, arena, id)?;
    buf.push(END);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::Reader;
    use crate::reader::mtef::MtefReader;

    fn roundtrip(node: MathNode) -> MathExpr {
        let expr = MathExpr::inline(vec![node]);
        let config = Config::default();
        let hex = MtefWriter::write(&expr, &config).unwrap();
        MtefReader::parse(&hex, &config).unwrap().expr
    }

    #[test]
    fn number_roundtrip() {
        let expr = roundtrip(MathNode::Number("2".into()));
        assert!(!expr.nodes.is_empty());
    }

    #[test]
    fn identifier_roundtrip() {
        let expr = roundtrip(MathNode::Identifier("x".into()));
        assert!(!expr.nodes.is_empty());
    }

    #[test]
    fn fraction_roundtrip() {
        let expr = roundtrip(MathNode::Fraction {
            kind: FractionKind::Normal,
            num: Box::new(MathNode::Number("1".into())),
            den: Box::new(MathNode::Number("2".into())),
        });
        assert!(!expr.nodes.is_empty());
        assert!(matches!(&expr.nodes[0], MathNode::Fraction { .. }));
    }

    #[test]
    fn sqrt_roundtrip() {
        let expr = roundtrip(MathNode::Sqrt(Box::new(MathNode::Identifier("x".into()))));
        assert!(matches!(&expr.nodes[0], MathNode::Sqrt(_)));
    }

    #[test]
    fn parentheses_roundtrip() {
        let expr = roundtrip(MathNode::Fenced {
            open: "(".into(),
            close: ")".into(),
            content: vec![FencedItem::Element(MathNode::Identifier("x".into()))],
        });
        assert!(matches!(&expr.nodes[0], MathNode::Fenced { .. }));
    }
}
