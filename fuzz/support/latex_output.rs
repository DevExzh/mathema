//! LaTeX of what was read from another format.

use mathema::Config;
use mathema::ast::compact::CompactExpr;
use mathema::ast::normalize::normalize_compact_expr;
use mathema::writer::Writer;
use mathema::writer::latex::LatexWriter;

/// Write LaTeX of the tree as it was read and normalized: in builds with debug
/// assertions the writer checks that every control word in its output is one
/// it wrote.
pub fn write(mut expr: CompactExpr, config: &Config) {
    let _ = LatexWriter::write_compact(&expr, config);
    if normalize_compact_expr(&mut expr).is_ok() {
        let _ = LatexWriter::write_compact(&expr, config);
    }
}
