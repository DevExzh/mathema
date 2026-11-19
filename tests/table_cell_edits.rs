use mathema::writer::Writer;
use mathema::writer::latex::LatexWriter;
use mathema::writer::mathml::MathmlWriter;
use mathema::writer::mtef::MtefWriter;
use mathema::writer::omml::OmmlWriter;
use mathema::writer::unicode_math::UnicodeMathWriter;
use mathema::{CompactExpr, CompactNode, Config, MathExpr};

fn check_writer<W: Writer>(compact: &CompactExpr, heap: &MathExpr) {
    let config = Config::default();
    let actual = W::write_compact(compact, &config).unwrap();
    assert_eq!(
        W::write(heap, &config).unwrap(),
        actual,
        "{}",
        std::any::type_name::<W>()
    );
}

#[test]
fn shortening_the_final_physical_row_keeps_its_last_operand() {
    let result =
        mathema::from_latex(r"\begin{array}{ccc}1&2&3\\4&\multicolumn{2}{r}{7}\end{array}")
            .unwrap();
    let mut compact = result.compact_expr().clone();
    let root = compact.arena.children(compact.roots)[0];
    let CompactNode::Array { cells, .. } = *compact.arena.node(root) else {
        panic!("expected array")
    };
    let mut actual_cells = compact.arena.children(cells).to_vec();
    actual_cells.pop();
    let shorter = compact.arena.push_children(&actual_cells).unwrap();
    let CompactNode::Array { cells, .. } = compact.arena.node_mut(root) else {
        unreachable!()
    };
    *cells = shorter;
    let heap = compact.to_math_expr();
    let xml = MathmlWriter::write(&heap, &Config::default()).unwrap();
    assert!(
        xml.contains(">7</mn>") && !xml.contains("columnspan"),
        "{xml}"
    );
    check_writer::<MathmlWriter>(&compact, &heap);
    check_writer::<LatexWriter>(&compact, &heap);
    check_writer::<UnicodeMathWriter>(&compact, &heap);
    check_writer::<OmmlWriter>(&compact, &heap);
    check_writer::<MtefWriter>(&compact, &heap);
    #[cfg(feature = "render")]
    assert_eq!(
        mathema::render::render_to_svg(&heap, &Default::default()).unwrap(),
        mathema::render::render_to_svg_compact(&compact, &Default::default()).unwrap()
    );
}
