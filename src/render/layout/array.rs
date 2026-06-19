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

pub(super) fn layout_array(
    columns: &[ColumnAlign],
    rows: &[Vec<Vec<MathNode>>],
    arraystretch_opt: Option<f64>,
    add_jot: bool,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let size = ctx.size();
    let aligned = is_aligned_rl_pattern(columns);

    // KaTeX strut-based row spacing (no explicit inter-row gap)
    let arraystretch = arraystretch_opt.unwrap_or(1.0);
    let baselineskip = 1.2 * size;
    let arrayskip = arraystretch * baselineskip;
    let arstrut_height = 0.7 * arrayskip;
    let arstrut_depth = 0.3 * arrayskip;
    let jot = 0.3 * size;

    // Layout all cells
    let mut cell_layouts: Vec<Vec<LayoutBox>> = Vec::new();
    for row in rows {
        let mut row_layouts = Vec::new();
        for cell in row {
            row_layouts.push(layout_row(cell, ctx));
        }
        cell_layouts.push(row_layouts);
    }

    let n_cols = columns
        .len()
        .max(cell_layouts.iter().map(|r| r.len()).max().unwrap_or(0));
    let n_rows = cell_layouts.len();

    // Compute column widths and row heights/depths with strut minimums
    let mut col_widths = vec![0.0_f64; n_cols];
    let mut row_heights = vec![arstrut_height; n_rows];
    let mut row_depths = vec![arstrut_depth; n_rows];

    for (i, row) in cell_layouts.iter().enumerate() {
        for (j, cell) in row.iter().enumerate() {
            col_widths[j] = col_widths[j].max(cell.width);
            row_heights[i] = row_heights[i].max(cell.height);
            row_depths[i] = row_depths[i].max(cell.depth);
        }
    }

    // addJot: extra 0.3em depth on every non-last row
    if add_jot {
        for d in row_depths.iter_mut().take(n_rows.saturating_sub(1)) {
            *d += jot;
        }
    }

    // Position cells — consume cell_layouts by ownership to avoid cloning
    let mut children = Vec::new();
    let mut cursor_y = 0.0;

    // Start from top
    for (i, row) in cell_layouts.into_iter().enumerate() {
        cursor_y += row_heights[i];
        let mut cursor_x = 0.0;
        let mut row_iter = row.into_iter();

        for (j, &col_w) in col_widths.iter().enumerate() {
            if let Some(cell) = row_iter.next() {
                let align = columns.get(j).copied().unwrap_or(ColumnAlign::Center);
                let dx = match align {
                    ColumnAlign::Left => 0.0,
                    ColumnAlign::Center => (col_w - cell.width) / 2.0,
                    ColumnAlign::Right => col_w - cell.width,
                };
                children.push(PositionedBox::new(cursor_x + dx, cursor_y, cell));
            }
            cursor_x += col_w;
            if j < n_cols - 1 {
                cursor_x += col_gap_after(j, aligned, size);
            }
        }

        cursor_y += row_depths[i];
    }

    let total_gap: f64 = (0..n_cols.saturating_sub(1))
        .map(|j| col_gap_after(j, aligned, size))
        .sum();
    let total_width: f64 = col_widths.iter().sum::<f64>() + total_gap;
    let total_extent = cursor_y;
    // Center vertically around the axis
    let axis = ctx.params.axis_height * size;
    let half = total_extent / 2.0;
    let box_height = half + axis;

    // Convert top-down cursor_y positions to VBox dy coordinates
    // (positive dy = up from baseline).
    for child in &mut children {
        child.dy = box_height - child.dy;
    }

    LayoutBox {
        width: total_width,
        height: box_height,
        depth: half - axis,
        kind: BoxKind::VBox { children },
    }
}

#[allow(clippy::needless_range_loop)]
#[allow(clippy::too_many_arguments)]
pub(super) fn layout_array_arena(
    ast: &AstArena,
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
    let baselineskip = 1.2 * size;
    let arrayskip = arraystretch * baselineskip;
    let arstrut_height = 0.7 * arrayskip;
    let arstrut_depth = 0.3 * arrayskip;
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
    for &cell_id in cell_ids_ast {
        match *ast.node(cell_id) {
            CompactNode::Row(span) => {
                cell_layout_ids.push(layout_row_arena(ast, span, arena, ctx));
            },
            _ => {
                cell_layout_ids.push(layout_node_arena(ast, cell_id, arena, ctx));
            },
        }
    }

    let n_cols = columns.len().max(nc);

    // Compute column widths and row heights/depths with strut minimums
    let mut col_widths = vec![0.0_f64; n_cols];
    let mut row_heights = vec![arstrut_height; n_rows];
    let mut row_depths = vec![arstrut_depth; n_rows];

    for i in 0..n_rows {
        for j in 0..nc.min(n_cols) {
            let idx = i * nc + j;
            if idx < cell_layout_ids.len() {
                let b = arena.get(cell_layout_ids[idx]);
                col_widths[j] = col_widths[j].max(b.width);
                row_heights[i] = row_heights[i].max(b.height);
                row_depths[i] = row_depths[i].max(b.depth);
            }
        }
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

    // Position cells
    let children_start = arena.begin_children();
    let mut cursor_y = 0.0;
    let mut row_boundary_y: Vec<f64> = Vec::with_capacity(n_rows + 1);

    // `array`-style tables pad outer edges with
    // `\arraycolsep` and use 2·arraycolsep between columns.
    let colsep = ARRAY_COLSEP_EM * size;
    let lead_pad = if array_style { colsep } else { 0.0 };
    let trail_pad = if array_style { colsep } else { 0.0 };
    let gap_after = |j: usize| -> f64 {
        if array_style {
            2.0 * colsep
        } else {
            col_gap_after(j, aligned, size)
        }
    };

    for i in 0..n_rows {
        row_boundary_y.push(cursor_y);
        cursor_y += row_heights[i];
        let mut cursor_x = lead_pad;

        for j in 0..nc.min(n_cols) {
            let idx = i * nc + j;
            if idx < cell_layout_ids.len() {
                let col_w = col_widths[j];
                let cell_w = arena.get(cell_layout_ids[idx]).width;
                let align = columns.get(j).copied().unwrap_or(ColumnAlign::Center);
                let dx = match align {
                    ColumnAlign::Left => 0.0,
                    ColumnAlign::Center => (col_w - cell_w) / 2.0,
                    ColumnAlign::Right => col_w - cell_w,
                };
                arena.push_child(ArenaPositionedBox {
                    dx: cursor_x + dx,
                    dy: box_height - cursor_y,
                    id: cell_layout_ids[idx],
                });
                cursor_x += col_w;
            } else {
                cursor_x += col_widths.get(j).copied().unwrap_or(0.0);
            }
            if j < n_cols - 1 {
                cursor_x += gap_after(j);
            }
        }

        cursor_y += row_depths[i];
    }
    row_boundary_y.push(cursor_y); // bottom edge

    let total_gap: f64 = (0..n_cols.saturating_sub(1)).map(gap_after).sum();
    let total_width: f64 = lead_pad + col_widths.iter().sum::<f64>() + total_gap + trail_pad;

    // Column boundary x positions (left edge of each column), incl. lead pad.
    let mut boundary_x = Vec::with_capacity(n_cols + 1);
    {
        let mut x_acc = lead_pad;
        for j in 0..n_cols {
            boundary_x.push(x_acc);
            x_acc += col_widths.get(j).copied().unwrap_or(0.0);
            if j < n_cols - 1 {
                x_acc += gap_after(j);
            }
        }
        boundary_x.push(x_acc);
    }

    // Add vertical separator rules if col_seps is non-zero
    if col_seps != 0 {
        let rule_w = 0.04 * size;
        let rule_total_h = box_height + (half - axis);
        let box_depth = half - axis;

        for pos in 0..=n_cols {
            if pos < 32 && (col_seps >> pos) & 1 != 0 {
                let bx = if pos == 0 {
                    0.0
                } else if pos == n_cols {
                    total_width
                } else {
                    boundary_x[pos] - gap_after(pos - 1) / 2.0
                };
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
    }

    // Add horizontal rules (\hline) if row_seps is non-zero.
    if row_seps != 0 {
        let rule_t = 0.04 * size;
        for (pos, &by) in row_boundary_y.iter().enumerate() {
            if pos < 32 && (row_seps >> pos) & 1 != 0 {
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
    }

    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: total_width,
        height: box_height,
        depth: half - axis,
        kind: ArenaBoxKind::VBox { children },
    })
}
