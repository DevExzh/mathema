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

#[derive(Clone, Copy)]
enum Boundary<'a> {
    Heap(&'a [SpanBoundary]),
    Compact(&'a str),
}

#[derive(Default)]
struct BoundaryGeometry {
    first: Option<SpanBoundary>,
    last: Option<SpanBoundary>,
    advance: f64,
}

impl Boundary<'_> {
    /// Kernel LaTeX centers a rule on the cursor. Consecutive rules advance
    /// by `\doublerulesep`. An `@{}` immediately after a rule adds another
    /// half rule width; a subsequent rule is centered on that new position.
    fn measure(self, size: f64, mut stroke: impl FnMut(f64)) -> BoundaryGeometry {
        let mut edge = BoundaryGeometry::default();
        let step = |directive| {
            if edge.last == Some(SpanBoundary::Rule) {
                edge.advance += match directive {
                    SpanBoundary::Rule => 0.2 * size,
                    SpanBoundary::SuppressGlue => 0.02 * size,
                };
            }
            if directive == SpanBoundary::Rule {
                stroke(edge.advance);
            }
            edge.first.get_or_insert(directive);
            edge.last = Some(directive);
        };
        match self {
            Self::Heap(source) => source.iter().copied().for_each(step),
            Self::Compact(source) => span_boundaries(source).for_each(step),
        }
        edge
    }
}

/// Only an explicit whole-cell wrapper replaces the preamble of its columns.
#[derive(Clone, Copy)]
struct CellTemplate<'a> {
    span: usize,
    alignment: Option<ColumnAlign>,
    left: Boundary<'a>,
    right: Boundary<'a>,
}

impl<'a> CellTemplate<'a> {
    const ORDINARY: Self = Self {
        span: 1,
        alignment: None,
        left: Boundary::Heap(&[]),
        right: Boundary::Heap(&[]),
    };

    fn heap(nodes: &'a [MathNode]) -> Self {
        match nodes {
            [
                MathNode::SpannedCell {
                    alignment,
                    span,
                    left_boundaries,
                    right_boundaries,
                    ..
                },
            ] => Self {
                span: usize::from((*span).max(1)),
                alignment: Some(*alignment),
                left: Boundary::Heap(left_boundaries),
                right: Boundary::Heap(right_boundaries),
            },
            [MathNode::AlignedCell { alignment, .. }] => Self {
                alignment: Some(*alignment),
                ..Self::ORDINARY
            },
            _ => Self::ORDINARY,
        }
    }

    fn compact(arena: &'a AstArena, row: &[NodeId], index: usize, node: &CompactNode) -> Self {
        match *node {
            CompactNode::SpannedCell {
                alignment,
                span,
                left_boundaries,
                right_boundaries,
                ..
            } => Self {
                span: arena.effective_cell_span_in_row(row, index, span),
                alignment: Some(alignment),
                left: Boundary::Compact(arena.str_val(left_boundaries)),
                right: Boundary::Compact(arena.str_val(right_boundaries)),
            },
            CompactNode::AlignedCell { alignment, .. } => Self {
                alignment: Some(alignment),
                ..Self::ORDINARY
            },
            _ => Self::ORDINARY,
        }
    }

    fn apply_rules(self, col: usize, mask: &mut u32) {
        if self.alignment.is_none() {
            return;
        }
        // The internal and trailing rules belong to the replaced template.
        // The rule at the left edge of a later cell belongs to its neighbor.
        let first = if col == 0 { 0 } else { col + 1 };
        let preceding = |boundary: usize| ((1u64 << boundary.min(u32::BITS as usize)) - 1) as u32;
        *mask &= !(preceding(col + self.span + 1) & !preceding(first));
    }

    fn paint_rules(self, size: f64, x: f64, width: f64, mut stroke: impl FnMut(f64)) {
        if self.alignment.is_none() {
            return;
        }
        self.left.measure(size, |offset| stroke(x + offset));
        let right_start = x + width - self.right.measure(size, |_| {}).advance;
        self.right
            .measure(size, |offset| stroke(right_start + offset));
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

    fn padding(&self, col: usize, cell: CellTemplate<'_>) -> (f64, f64) {
        let half_gap = |before| {
            self.gap
                .unwrap_or_else(|| col_gap_after(before, self.aligned, self.size))
                * 0.5
        };
        let end = col + cell.span;
        let left_glue = if col == 0 {
            self.edge
        } else {
            half_gap(col - 1)
        };
        let right_glue = if end == self.widths.len() {
            self.edge
        } else {
            half_gap(end - 1)
        };
        if cell.alignment.is_none() {
            return (left_glue, right_glue);
        }
        let left = cell.left.measure(self.size, |_| {});
        let right = cell.right.measure(self.size, |_| {});
        (
            left.advance
                + if left.last == Some(SpanBoundary::SuppressGlue) {
                    0.0
                } else {
                    left_glue
                },
            right.advance
                + if right.first == Some(SpanBoundary::SuppressGlue) {
                    0.0
                } else {
                    right_glue
                },
        )
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

/// The filled path of a `width` × `height` rule. A dashed rule is a run of
/// dashes along its long side, `vertical` or horizontal, with arydshln's
/// default 4pt dash and 4pt gap (at 10pt).
fn rule_path(width: f64, height: f64, dashed: bool, vertical: bool, size: f64) -> String {
    let length = if vertical { height } else { width };
    let dash = 0.4 * size;
    // Very long or degenerate rules stay solid rather than emitting a huge path.
    const MAX_DASHES: f64 = 4096.0;
    if !dashed || !(dash > 0.0 && length.is_finite() && length / (2.0 * dash) < MAX_DASHES) {
        return format!(
            "F:M0,0 L{:.4},0 L{:.4},{:.4} L0,{:.4} Z",
            width, width, height, height
        );
    }
    // One buffer and the fixed-point number writer: `format!` per dash
    // dominated the layout of large dashed tables.
    let mut path = crate::render::path_writer::PathWriter::with_prefix("F:");
    let mut start = 0.0;
    while start < length {
        let end = (start + dash).min(length);
        let (x0, y0, x1, y1) = if vertical {
            (0.0, start, width, end)
        } else {
            (start, 0.0, end, height)
        };
        path.cmd(b'M');
        path.push_f64(x0);
        path.push_f64(y0);
        path.cmd(b'L');
        for (x, y) in [(x1, y0), (x1, y1), (x0, y1)] {
            path.push_f64(x);
            path.push_f64(y);
        }
        path.close();
        start += 2.0 * dash;
    }
    path.finish()
}

/// Split a rule mask into its solid and dashed rules.
fn rule_styles(mask: u32, dashed: u32) -> [(u32, bool); 2] {
    [(mask & !dashed, false), (mask & dashed, true)]
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
    dashes: RuleDashes,
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
            template.paint_rules(size, cursor_x, width, |x| {
                cell_rules.push((x, i, cursor_y, false))
            });
            children.push(PositionedBox::new(cursor_x + dx, cursor_y, cell));
            cursor_x += width;
            col += template.span;
        }
        if visible_rules != 0 {
            for (mask, dashed) in rule_styles(visible_rules, dashes.cols) {
                cell_rules.extend(grid.rules(mask).map(|x| (x, i, cursor_y, dashed)));
            }
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
        for (mask, dashed) in rule_styles(col_seps, dashes.cols) {
            for bx in grid.rules(mask) {
                let path = rule_path(rule_w, rule_total_h, dashed, true, size);
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
    }

    if !cell_rules.is_empty() {
        let rule_w = 0.04 * size;
        for (bx, row, baseline, dashed) in cell_rules {
            let rule_total_h = row_heights[row] + row_depths[row];
            let path = rule_path(rule_w, rule_total_h, dashed, true, size);
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
        for (mask, dashed) in rule_styles(row_seps, dashes.rows) {
            for boundary in row_rules(&row_heights, &row_depths, mask) {
                let path = rule_path(total_width, rule_t, dashed, false, size);
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
    dashes: RuleDashes,
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
    let nc = usize::from(num_cols.max(1));
    let n_rows = cell_ids_ast.len().div_ceil(nc);

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

    for (i, row) in cell_ids_ast.chunks(nc).enumerate() {
        let mut j = 0;
        while j < row.len() {
            let idx = i * nc + j;
            let template = CellTemplate::compact(ast, row, j, ast.node(row[j]));
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

    for (i, row) in cell_ids_ast.chunks(nc).enumerate() {
        cursor_y += row_heights[i];
        let mut cursor_x = 0.0;
        let mut visible_rules = if has_override { col_seps } else { 0 };
        let mut j = 0;
        while j < row.len() {
            let idx = i * nc + j;
            let template = CellTemplate::compact(ast, row, j, ast.node(row[j]));
            let cell = cell_layout_ids[idx];
            let (width, dx) = grid.place(j, template, arena.get(cell).width, &columns);
            template.apply_rules(j, &mut visible_rules);
            template.paint_rules(size, cursor_x, width, |x| {
                cell_rules.push((x, i, cursor_y, false))
            });
            arena.push_child(ArenaPositionedBox {
                dx: cursor_x + dx,
                dy: box_height - cursor_y,
                id: cell,
            });
            cursor_x += width;
            j += template.span;
        }
        if visible_rules != 0 {
            for (mask, dashed) in rule_styles(visible_rules, dashes.cols) {
                cell_rules.extend(grid.rules(mask).map(|x| (x, i, cursor_y, dashed)));
            }
        }
        cursor_y += row_depths[i];
    }
    let total_width = grid.width();

    // Add vertical separator rules if col_seps is non-zero
    if col_seps != 0 && !has_override {
        let rule_w = 0.04 * size;
        let rule_total_h = box_height + (half - axis);
        let box_depth = half - axis;

        for (mask, dashed) in rule_styles(col_seps, dashes.cols) {
            for bx in grid.rules(mask) {
                let path = rule_path(rule_w, rule_total_h, dashed, true, size);
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
    }

    if !cell_rules.is_empty() {
        let rule_w = 0.04 * size;
        for (bx, row, baseline, dashed) in cell_rules {
            let rule_total_h = row_heights[row] + row_depths[row];
            let path = rule_path(rule_w, rule_total_h, dashed, true, size);
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
        for (mask, dashed) in rule_styles(row_seps, dashes.rows) {
            for by in row_rules(&row_heights, &row_depths, mask) {
                let line_dy = box_height - by;
                let path = rule_path(total_width, rule_t, dashed, false, size);
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
            alignment: Some(ColumnAlign::Center),
            left: Boundary::Heap(&[SpanBoundary::SuppressGlue]),
            ..usual
        };
        let right = CellTemplate {
            span: 2,
            alignment: Some(ColumnAlign::Center),
            right: Boundary::Heap(&[SpanBoundary::SuppressGlue]),
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
            right: Boundary::Heap(&[SpanBoundary::Rule]),
            ..CellTemplate::ORDINARY
        }
        .apply_rules(1, &mut mask);
        assert_eq!(mask, 0b0011);
    }

    #[test]
    fn empty_material_after_a_rule_advances_half_a_rule_width() {
        let cell = CellTemplate {
            span: 1,
            alignment: Some(ColumnAlign::Left),
            left: Boundary::Compact("|@{}|"),
            right: Boundary::Compact("||@{}"),
        };
        let mut x = Vec::new();
        cell.paint_rules(1.0, 0.0, 2.0, |v| x.push(v));
        for (actual, expected) in x.iter().zip([0.0, 0.02, 1.78, 1.98]) {
            assert!((actual - expected).abs() < 1e-10);
        }
        assert_eq!(x.len(), 4);
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
