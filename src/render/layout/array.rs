use super::*;

/// Detect whether columns follow an aligned `[R, L, R, L, ...]` pattern and
/// return `true` if so.  Used to switch from a uniform column gap to the
/// tighter per-pair gaps that KaTeX applies.
pub(super) fn is_aligned_rl_pattern(columns: &[ColumnAlign]) -> bool {
    columns.len() >= 2
        && columns.iter().enumerate().all(|(i, &a)| {
            if i % 2 == 0 {
                a == ColumnAlign::Right
            } else {
                a == ColumnAlign::Left
            }
        })
}

/// Return the column gap that should appear *after* column `j` (0-indexed)
/// in an array.  For aligned environments (detected via `is_aligned`) the gap
/// alternates between near-zero (within an rl-pair) and wider (between pairs).
pub(super) fn col_gap_after(j: usize, is_aligned: bool, size: f64) -> f64 {
    if is_aligned {
        // Even column (R) → next column (L): within an rl-pair → tight
        // Odd column (L) → next column (R): between rl-pairs → wide
        if j.is_multiple_of(2) {
            ALIGN_INTERNAL_GAP_EM * size
        } else {
            ALIGN_PAIR_GAP_EM * size
        }
    } else {
        MATRIX_COL_GAP_EM * size
    }
}

/// The preamble of one cell. A normal cell inherits its column's alignment
/// and the two adjacent half-gaps. A multicolumn can replace either side.
#[derive(Clone, Copy)]
struct CellTemplate {
    span: usize,
    alignment: Option<ColumnAlign>,
    omit_left: bool,
    omit_right: bool,
    left_rule: bool,
    right_rule: bool,
}

impl CellTemplate {
    const ORDINARY: Self = Self {
        span: 1,
        alignment: None,
        omit_left: false,
        omit_right: false,
        left_rule: false,
        right_rule: false,
    };

    fn apply_rules(self, col: usize, mask: &mut u32) {
        if self.alignment.is_none() {
            return;
        }
        // A vertical rule between columns belongs to the template on its
        // left. An override replaces internal rules and its trailing rule,
        // and the leading template rule only in the first logical column.
        let end = col + self.span;
        let first = if col == 0 { 0 } else { col + 1 };
        let preceding = |boundary: usize| ((1u64 << boundary.min(u32::BITS as usize)) - 1) as u32;
        *mask &= !(preceding(end + 1) & !preceding(first));
        if self.left_rule && col < u32::BITS as usize {
            *mask |= 1 << col;
        }
        if self.right_rule && end < u32::BITS as usize {
            *mask |= 1 << end;
        }
    }

    fn heap(nodes: &[MathNode]) -> Self {
        match nodes {
            [
                MathNode::SpannedCell {
                    alignment,
                    span,
                    left_padding,
                    right_padding,
                    left_rule,
                    right_rule,
                    ..
                },
            ] => Self {
                span: usize::from((*span).max(1)),
                alignment: Some(*alignment),
                omit_left: *left_padding,
                omit_right: *right_padding,
                left_rule: *left_rule,
                right_rule: *right_rule,
            },
            [MathNode::AlignedCell { alignment, .. }] => Self {
                alignment: Some(*alignment),
                ..Self::ORDINARY
            },
            _ => Self::ORDINARY,
        }
    }

    fn compact(node: &CompactNode, remaining: usize) -> Self {
        match *node {
            CompactNode::SpannedCell {
                alignment,
                span,
                left_padding,
                right_padding,
                left_rule,
                right_rule,
                ..
            } => Self {
                span: usize::from(span.max(1)).min(remaining),
                alignment: Some(alignment),
                omit_left: left_padding,
                omit_right: right_padding,
                left_rule,
                right_rule,
            },
            CompactNode::AlignedCell { alignment, .. } => Self {
                alignment: Some(alignment),
                ..Self::ORDINARY
            },
            _ => Self::ORDINARY,
        }
    }
}

/// Physical column widths include both sides of the ordinary column template.
/// Thus removing padding from one cell never changes another row's template.
struct ColumnLayout {
    widths: Vec<f64>,
    spans: Vec<(usize, usize, f64)>,
    edge: f64,
    gap: Option<f64>,
    aligned: bool,
    size: f64,
}

impl ColumnLayout {
    fn new(
        count: usize,
        small: Option<(f64, f64)>,
        array: bool,
        aligned: bool,
        size: f64,
        has_span: bool,
    ) -> Self {
        let colsep = ARRAY_COLSEP_EM * size;
        let (edge, gap) = match small {
            Some((edge, gap)) => (edge, Some(gap)),
            None if array => (colsep, Some(2.0 * colsep)),
            None => (0.0, None),
        };
        let mut layout = Self {
            widths: vec![0.0; count],
            spans: Vec::new(),
            edge,
            gap,
            aligned,
            size,
        };
        if !has_span {
            for col in 0..count {
                let (left, right) = layout.padding(col, CellTemplate::ORDINARY);
                layout.widths[col] = left + right;
            }
        }
        layout
    }

    fn padding(&self, col: usize, cell: CellTemplate) -> (f64, f64) {
        let half_gap = |before| {
            self.gap
                .unwrap_or_else(|| col_gap_after(before, self.aligned, self.size))
                * 0.5
        };
        let end = col + cell.span;
        let left = if cell.omit_left && !cell.left_rule {
            0.0
        } else if col == 0 {
            self.edge
        } else {
            half_gap(col - 1)
        };
        let right = if cell.omit_right {
            0.0
        } else if end == self.widths.len() {
            self.edge
        } else {
            half_gap(end - 1)
        };
        (left, right)
    }

    fn measure(&mut self, col: usize, cell: CellTemplate, width: f64) {
        let (left, right) = self.padding(col, cell);
        let required = width + left + right;
        if cell.span == 1 {
            self.widths[col] = self.widths[col].max(required);
        } else {
            self.spans.push((col, col + cell.span, required));
        }
    }

    fn resolve(&mut self) {
        // TeX assigns an unmet span width to its final column. Resolve earlier
        // end columns first so overlapping spans use the same column minima in
        // either input row order.
        self.spans.sort_unstable_by_key(|&(_, end, _)| end);
        for &(start, end, required) in &self.spans {
            let current: f64 = self.widths[start..end].iter().sum();
            if required > current {
                self.widths[end - 1] += required - current;
            }
        }
    }

    fn place(
        &self,
        col: usize,
        cell: CellTemplate,
        content: f64,
        columns: &[ColumnAlign],
    ) -> (f64, f64) {
        let width = if cell.span == 1 {
            self.widths[col]
        } else {
            self.widths[col..col + cell.span].iter().sum()
        };
        let (left, right) = self.padding(col, cell);
        let slack = width - left - right - content;
        let align = cell
            .alignment
            .or_else(|| columns.get(col).copied())
            .unwrap_or(ColumnAlign::Center);
        (
            width,
            left + match align {
                ColumnAlign::Left => 0.0,
                ColumnAlign::Center => slack * 0.5,
                ColumnAlign::Right => slack,
            },
        )
    }

    fn width(&self) -> f64 {
        if self.widths.is_empty() {
            2.0 * self.edge
        } else {
            self.widths.iter().sum()
        }
    }

    fn rules(&self, mask: u32) -> impl Iterator<Item = f64> + '_ {
        let ends = self.widths.iter().scan(0.0, |x, w| {
            *x += w;
            Some(*x)
        });
        std::iter::once(0.0)
            .chain(ends)
            .take(u32::BITS as usize)
            .enumerate()
            .filter_map(move |(index, x)| (mask & (1 << index) != 0).then_some(x))
    }
}

fn row_rules<'a>(
    heights: &'a [f64],
    depths: &'a [f64],
    mask: u32,
) -> impl Iterator<Item = f64> + 'a {
    let bottoms = heights.iter().zip(depths).scan(0.0, |y, (height, depth)| {
        *y += height;
        *y += depth;
        Some(*y)
    });
    std::iter::once(0.0)
        .chain(bottoms)
        .take(u32::BITS as usize)
        .enumerate()
        .filter_map(move |(index, boundary)| (mask & (1 << index) != 0).then_some(boundary))
}

/// amsmath's smallmatrix uses no minimum strut. In a regular array the
/// height and depth of the strut split the stretched baseline skip 70:30.
fn array_strut(smallmatrix: bool, size: f64, stretch: f64) -> (f64, f64) {
    if smallmatrix {
        (0.0, 0.0)
    } else {
        let skip = stretch * (1.2 * size);
        (0.7 * skip, 0.3 * skip)
    }
}

fn add_smallmatrix_row_glue(heights: &[f64], depths: &mut [f64], scale: f64) {
    // TeX uses 6pt between baselines, with at least 1.5pt of clearance
    // between the ink of adjacent rows, in a nominal ten-point document.
    let baseline_skip = 0.6 * scale;
    let line_skip = 0.15 * scale;
    for (&next_height, depth) in heights.iter().skip(1).zip(depths) {
        *depth += (baseline_skip - *depth - next_height).max(line_skip);
    }
}

fn smallmatrix_horizontal_spacing(style: MathStyle, scale: f64) -> (f64, f64) {
    // The edge thin spaces are in the surrounding math style. The thick
    // space in the horizontal alignment preamble uses its text-mode width.
    let width = |r, basis| crate::ast::style::space_width_in_em(r, Some(basis), style, scale);
    (
        width(Rational::new(1, 6), SpaceBasis::MathMu),
        width(Rational::new(2777, 10_000), SpaceBasis::TextEm),
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn layout_array(
    smallmatrix: bool,
    columns: &[ColumnAlign],
    rows: &[Vec<Vec<MathNode>>],
    arraystretch_opt: Option<f64>,
    add_jot: bool,
    array_style: bool,
    col_seps: u32,
    row_seps: u32,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let size = ctx.size();
    let aligned = is_aligned_rl_pattern(columns);

    // KaTeX strut-based row spacing (no explicit inter-row gap)
    let arraystretch = arraystretch_opt.unwrap_or(1.0);
    let (arstrut_height, arstrut_depth) = array_strut(smallmatrix, size, arraystretch);
    let jot = 0.3 * size;

    let Ok(n_cols) = crate::ast::table_width(rows, columns.len()) else {
        return LayoutBox::kern(0.0);
    };
    // Layout all cells
    let mut cell_layouts: Vec<Vec<LayoutBox>> = Vec::new();
    let cell_ctx = if smallmatrix {
        ctx.with_style(MathStyle::Script)
    } else {
        ctx.with_style(MathStyle::Text)
    };
    let mut has_span = false;
    let mut has_override = false;
    for row in rows {
        let mut row_layouts = Vec::new();
        for cell in row {
            has_span |= matches!(cell.as_slice(), [MathNode::SpannedCell { .. }]);
            has_override |= matches!(
                cell.as_slice(),
                [MathNode::SpannedCell { .. } | MathNode::AlignedCell { .. }]
            );
            row_layouts.push(layout_row(crate::ast::table_cell_parts(cell).1, &cell_ctx));
        }
        cell_layouts.push(row_layouts);
    }

    let n_rows = cell_layouts.len();

    // Compute column widths and row heights/depths with strut minimums
    let small = smallmatrix.then(|| smallmatrix_horizontal_spacing(ctx.style, ctx.size_scale));
    let mut grid = ColumnLayout::new(n_cols, small, array_style, aligned, size, has_span);
    let mut row_heights = vec![arstrut_height; n_rows];
    let mut row_depths = vec![arstrut_depth; n_rows];

    for (i, row) in cell_layouts.iter().enumerate() {
        let mut j = 0;
        for (cell_index, cell) in row.iter().enumerate() {
            let template = CellTemplate::heap(&rows[i][cell_index]);
            grid.measure(j, template, cell.width);
            row_heights[i] = row_heights[i].max(cell.height);
            row_depths[i] = row_depths[i].max(cell.depth);
            j += template.span;
        }
    }

    if smallmatrix {
        add_smallmatrix_row_glue(&row_heights, &mut row_depths, ctx.size_scale);
    }

    // addJot: extra 0.3em depth on every non-last row
    if add_jot {
        for d in row_depths.iter_mut().take(n_rows.saturating_sub(1)) {
            *d += jot;
        }
    }

    // Position cells — consume cell_layouts by ownership to avoid cloning
    let mut children = Vec::new();
    let mut cell_rules = Vec::new();
    let mut cursor_y = 0.0;
    grid.resolve();

    // Start from top
    for (i, row) in cell_layouts.into_iter().enumerate() {
        cursor_y += row_heights[i];
        let mut cursor_x = 0.0;
        let mut visible_rules = if has_override { col_seps } else { 0 };
        let mut col = 0;
        for (cell, source) in row.into_iter().zip(&rows[i]) {
            let template = CellTemplate::heap(source);
            let (width, dx) = grid.place(col, template, cell.width, columns);
            template.apply_rules(col, &mut visible_rules);
            if template.left_rule && col >= u32::BITS as usize {
                cell_rules.push((cursor_x, i, cursor_y));
            }
            if template.right_rule && col + template.span >= u32::BITS as usize {
                cell_rules.push((cursor_x + width, i, cursor_y));
            }
            children.push(PositionedBox::new(cursor_x + dx, cursor_y, cell));
            cursor_x += width;
            col += template.span;
        }
        if visible_rules != 0 {
            cell_rules.extend(grid.rules(visible_rules).map(|x| (x, i, cursor_y)));
        }
        cursor_y += row_depths[i];
    }

    let total_width = grid.width();
    let total_extent = cursor_y;
    // Center vertically around the axis
    let axis = ctx.params.axis_height * size;
    let half = total_extent / 2.0;
    let box_height = half + axis;
    let box_depth = if smallmatrix {
        (half - axis).max(0.0)
    } else {
        half - axis
    };

    // Convert top-down cursor_y positions to VBox dy coordinates
    // (positive dy = up from baseline).
    for child in &mut children {
        child.dy = box_height - child.dy;
    }

    if col_seps != 0 && !has_override {
        let rule_w = 0.04 * size;
        let rule_total_h = box_height + box_depth;
        for bx in grid.rules(col_seps) {
            let path = format!(
                "F:M0,0 L{:.4},0 L{:.4},{:.4} L0,{:.4} Z",
                rule_w, rule_w, rule_total_h, rule_total_h
            );
            children.push(PositionedBox::new(
                bx - rule_w / 2.0,
                0.0,
                LayoutBox {
                    width: rule_w,
                    height: box_height,
                    depth: box_depth,
                    kind: BoxKind::SvgPath(path),
                },
            ));
        }
    }

    if !cell_rules.is_empty() {
        let rule_w = 0.04 * size;
        for (bx, row, baseline) in cell_rules {
            let rule_total_h = row_heights[row] + row_depths[row];
            let path = format!(
                "F:M0,0 L{:.4},0 L{:.4},{:.4} L0,{:.4} Z",
                rule_w, rule_w, rule_total_h, rule_total_h
            );
            children.push(PositionedBox::new(
                bx - rule_w / 2.0,
                box_height - baseline,
                LayoutBox {
                    width: rule_w,
                    height: row_heights[row],
                    depth: row_depths[row],
                    kind: BoxKind::SvgPath(path),
                },
            ));
        }
    }

    if row_seps != 0 {
        let rule_t = 0.04 * size;
        for boundary in row_rules(&row_heights, &row_depths, row_seps) {
            let path = format!(
                "F:M0,0 L{:.4},0 L{:.4},{:.4} L0,{:.4} Z",
                total_width, total_width, rule_t, rule_t
            );
            children.push(PositionedBox::new(
                0.0,
                box_height - boundary - rule_t / 2.0,
                LayoutBox {
                    width: total_width,
                    height: rule_t,
                    depth: 0.0,
                    kind: BoxKind::SvgPath(path),
                },
            ));
        }
    }

    LayoutBox {
        width: total_width,
        height: box_height,
        depth: box_depth,
        kind: BoxKind::VBox { children },
    }
}

#[allow(clippy::needless_range_loop)]
#[allow(clippy::too_many_arguments)]
pub(super) fn layout_array_arena(
    ast: &AstArena,
    smallmatrix: bool,
    col_aligns: NodeSpan,
    cells: NodeSpan,
    num_cols: u16,
    arraystretch_pct: u8,
    add_jot: bool,
    col_seps: u32,
    row_seps: u32,
    array_style: bool,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let size = ctx.size();
    // KaTeX strut-based row spacing (no explicit inter-row gap)
    let arraystretch = arraystretch_pct as f64 / 100.0;
    let (arstrut_height, arstrut_depth) = array_strut(smallmatrix, size, arraystretch);
    let jot = 0.3 * size;

    // Extract column alignments
    let columns: Vec<ColumnAlign> = ast
        .children(col_aligns)
        .iter()
        .map(|&id| match *ast.node(id) {
            CompactNode::ColAlign(a) => a,
            _ => ColumnAlign::Center,
        })
        .collect();
    let aligned = is_aligned_rl_pattern(&columns);

    // Layout all cells (flat storage, row-major, stride = num_cols)
    let cell_ids_ast = ast.children(cells);
    let nc = num_cols as usize;
    let n_rows = cell_ids_ast.len().checked_div(nc).unwrap_or(0);

    // Layout each cell and store the layout box IDs
    let mut cell_layout_ids: Vec<LayoutBoxId> = Vec::with_capacity(cell_ids_ast.len());
    let cell_ctx = ctx.with_style(if smallmatrix {
        MathStyle::Script
    } else {
        MathStyle::Text
    });
    let mut has_span = false;
    let mut has_override = false;
    for &cell_id in cell_ids_ast {
        has_span |= matches!(ast.node(cell_id), CompactNode::SpannedCell { .. });
        has_override |= matches!(
            ast.node(cell_id),
            CompactNode::AlignedCell { .. } | CompactNode::SpannedCell { .. }
        );
        match *ast.node(cell_id) {
            CompactNode::Row(span) => {
                cell_layout_ids.push(layout_row_arena(ast, span, arena, &cell_ctx));
            },
            _ => {
                cell_layout_ids.push(layout_node_arena(ast, cell_id, arena, &cell_ctx));
            },
        }
    }

    let n_cols = columns.len().max(nc);

    // Compute column widths and row heights/depths with strut minimums
    let small = smallmatrix.then(|| smallmatrix_horizontal_spacing(ctx.style, ctx.size_scale));
    let mut grid = ColumnLayout::new(n_cols, small, array_style, aligned, size, has_span);
    let mut row_heights = vec![arstrut_height; n_rows];
    let mut row_depths = vec![arstrut_depth; n_rows];

    for i in 0..n_rows {
        let mut j = 0;
        while j < nc {
            let idx = i * nc + j;
            let template = CellTemplate::compact(ast.node(cell_ids_ast[idx]), nc - j);
            let cell = arena.get(cell_layout_ids[idx]);
            grid.measure(j, template, cell.width);
            row_heights[i] = row_heights[i].max(cell.height);
            row_depths[i] = row_depths[i].max(cell.depth);
            j += template.span;
        }
    }
    grid.resolve();

    if smallmatrix {
        add_smallmatrix_row_glue(&row_heights, &mut row_depths, ctx.size_scale);
    }

    // addJot: extra 0.3em depth on every non-last row
    if add_jot {
        for d in row_depths.iter_mut().take(n_rows.saturating_sub(1)) {
            *d += jot;
        }
    }

    // Pre-compute total extent and box_height so we can convert top-down
    // cursor_y to VBox dy (positive = up from baseline) when pushing children.
    let total_extent: f64 = row_heights.iter().sum::<f64>() + row_depths.iter().sum::<f64>();
    let axis = ctx.params.axis_height * size;
    let half = total_extent / 2.0;
    let box_height = half + axis;
    let box_depth = if smallmatrix {
        (half - axis).max(0.0)
    } else {
        half - axis
    };

    // Position cells
    let children_start = arena.begin_children();
    let mut cell_rules = Vec::new();
    let mut cursor_y = 0.0;

    for i in 0..n_rows {
        cursor_y += row_heights[i];
        let mut cursor_x = 0.0;
        let mut visible_rules = if has_override { col_seps } else { 0 };
        let mut j = 0;
        while j < nc {
            let idx = i * nc + j;
            let template = CellTemplate::compact(ast.node(cell_ids_ast[idx]), nc - j);
            let cell = cell_layout_ids[idx];
            let (width, dx) = grid.place(j, template, arena.get(cell).width, &columns);
            template.apply_rules(j, &mut visible_rules);
            if template.left_rule && j >= u32::BITS as usize {
                cell_rules.push((cursor_x, i, cursor_y));
            }
            if template.right_rule && j + template.span >= u32::BITS as usize {
                cell_rules.push((cursor_x + width, i, cursor_y));
            }
            arena.push_child(ArenaPositionedBox {
                dx: cursor_x + dx,
                dy: box_height - cursor_y,
                id: cell,
            });
            cursor_x += width;
            j += template.span;
        }
        if visible_rules != 0 {
            cell_rules.extend(grid.rules(visible_rules).map(|x| (x, i, cursor_y)));
        }
        cursor_y += row_depths[i];
    }
    let total_width = grid.width();

    // Add vertical separator rules if col_seps is non-zero
    if col_seps != 0 && !has_override {
        let rule_w = 0.04 * size;
        let rule_total_h = box_height + (half - axis);
        let box_depth = half - axis;

        for bx in grid.rules(col_seps) {
            let path = format!(
                "F:M0,0 L{:.4},0 L{:.4},{:.4} L0,{:.4} Z",
                rule_w, rule_w, rule_total_h, rule_total_h
            );
            let rule_id = arena.alloc(ArenaLayoutBox {
                width: rule_w,
                height: box_height,
                depth: box_depth,
                kind: ArenaBoxKind::SvgPath(path),
            });
            arena.push_child(ArenaPositionedBox {
                dx: bx - rule_w / 2.0,
                dy: 0.0,
                id: rule_id,
            });
        }
    }

    if !cell_rules.is_empty() {
        let rule_w = 0.04 * size;
        for (bx, row, baseline) in cell_rules {
            let rule_total_h = row_heights[row] + row_depths[row];
            let path = format!(
                "F:M0,0 L{:.4},0 L{:.4},{:.4} L0,{:.4} Z",
                rule_w, rule_w, rule_total_h, rule_total_h
            );
            let rule_id = arena.alloc(ArenaLayoutBox {
                width: rule_w,
                height: row_heights[row],
                depth: row_depths[row],
                kind: ArenaBoxKind::SvgPath(path),
            });
            arena.push_child(ArenaPositionedBox {
                dx: bx - rule_w / 2.0,
                dy: box_height - baseline,
                id: rule_id,
            });
        }
    }

    // Add horizontal rules (\hline) if row_seps is non-zero.
    if row_seps != 0 {
        let rule_t = 0.04 * size;
        for by in row_rules(&row_heights, &row_depths, row_seps) {
            let line_dy = box_height - by;
            let path = format!(
                "F:M0,0 L{:.4},0 L{:.4},{:.4} L0,{:.4} Z",
                total_width, total_width, rule_t, rule_t
            );
            let rule_id = arena.alloc(ArenaLayoutBox {
                width: total_width,
                height: rule_t,
                depth: 0.0,
                kind: ArenaBoxKind::SvgPath(path),
            });
            arena.push_child(ArenaPositionedBox {
                dx: 0.0,
                dy: line_dy - rule_t / 2.0,
                id: rule_id,
            });
        }
    }

    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: box_height,
        depth: box_depth,
        kind: ArenaBoxKind::VBox { children },
    })
}

#[cfg(test)]
mod span_tests {
    use super::*;

    #[test]
    fn padding_is_local_to_the_spanning_cell() {
        let mut grid = ColumnLayout::new(3, None, true, false, 1.0, true);
        let usual = CellTemplate::ORDINARY;
        for (col, width) in [1.0, 2.0, 1.0].into_iter().enumerate() {
            grid.measure(col, usual, width);
        }
        let left = CellTemplate {
            span: 2,
            omit_left: true,
            ..usual
        };
        let right = CellTemplate {
            span: 2,
            omit_right: true,
            ..usual
        };
        grid.measure(1, left, 0.5);
        grid.measure(1, right, 0.5);
        grid.resolve();
        let columns = [ColumnAlign::Left, ColumnAlign::Center, ColumnAlign::Right];
        let (width, center) = grid.place(1, CellTemplate { span: 2, ..usual }, 0.5, &columns);
        assert_eq!(
            grid.place(1, left, 0.5, &columns),
            (width, center - grid.edge / 2.0)
        );
        assert_eq!(
            grid.place(1, right, 0.5, &columns),
            (width, center + grid.edge / 2.0)
        );
        assert_eq!(grid.place(0, usual, 1.0, &columns).1, grid.edge);
    }

    #[test]
    fn overrides_replace_only_their_own_preamble_rules() {
        let mut mask = 0b1111;
        CellTemplate {
            span: 2,
            alignment: Some(ColumnAlign::Right),
            ..CellTemplate::ORDINARY
        }
        .apply_rules(0, &mut mask);
        assert_eq!(mask, 0b1000);
        let mut mask = 0b1111;
        CellTemplate {
            span: 2,
            alignment: Some(ColumnAlign::Right),
            right_rule: true,
            ..CellTemplate::ORDINARY
        }
        .apply_rules(1, &mut mask);
        assert_eq!(mask, 0b1011);
    }

    #[test]
    fn overlapping_spans_resolve_in_logical_column_order() {
        let usual = CellTemplate::ORDINARY;
        let span = CellTemplate { span: 2, ..usual };
        for constraints in [[(1, 9.0), (0, 8.0)], [(0, 8.0), (1, 9.0)]] {
            let mut grid = ColumnLayout::new(3, None, true, false, 1.0, true);
            for col in 0..3 {
                grid.measure(col, usual, 1.0);
            }
            for (start, width) in constraints {
                grid.measure(start, span, width);
            }
            grid.resolve();
            // Each padded ordinary cell initially needs two em. The two
            // spanning cells require nine and ten em across their slots.
            assert_eq!(grid.widths, [2.0, 7.0, 3.0]);
        }
    }
}
