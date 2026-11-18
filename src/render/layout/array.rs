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

/// Rule masks cover at most the first 32 boundaries. Compute these positions
/// on demand so ordinary unruled tables need no boundary storage.
fn column_rules(
    widths: &[f64],
    gap_after: impl Fn(usize) -> f64,
    edge: f64,
    total: f64,
    mask: u32,
) -> impl Iterator<Item = f64> {
    let mut next_column = edge;
    (0..=widths.len())
        .take(u32::BITS as usize)
        .filter_map(move |index| {
            let boundary = if index == 0 {
                0.0
            } else if index == widths.len() {
                total
            } else {
                next_column - gap_after(index - 1) / 2.0
            };
            if let Some(width) = widths.get(index) {
                next_column += width;
                if index + 1 < widths.len() {
                    next_column += gap_after(index);
                }
            }
            (mask & (1 << index) != 0).then_some(boundary)
        })
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

    // Layout all cells
    let mut cell_layouts: Vec<Vec<LayoutBox>> = Vec::new();
    let cell_ctx = if smallmatrix {
        ctx.with_style(MathStyle::Script)
    } else {
        ctx.with_style(MathStyle::Text)
    };
    for row in rows {
        let mut row_layouts = Vec::new();
        for cell in row {
            row_layouts.push(layout_row(crate::ast::table_cell_parts(cell).1, &cell_ctx));
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
    let mut cursor_y = 0.0;
    let colsep = ARRAY_COLSEP_EM * size;
    let (outer_pad, uniform_gap) = if smallmatrix {
        let (edge, gap) = smallmatrix_horizontal_spacing(ctx.style, ctx.size_scale);
        (edge, Some(gap))
    } else if array_style {
        (colsep, Some(2.0 * colsep))
    } else {
        (0.0, None)
    };
    let gap_after = |j| uniform_gap.unwrap_or_else(|| col_gap_after(j, aligned, size));

    // Start from top
    for (i, row) in cell_layouts.into_iter().enumerate() {
        cursor_y += row_heights[i];
        let mut cursor_x = outer_pad;
        let mut row_iter = row.into_iter();

        for (j, &col_w) in col_widths.iter().enumerate() {
            if let Some(cell) = row_iter.next() {
                let align = crate::ast::table_cell_parts(&rows[i][j])
                    .0
                    .or_else(|| columns.get(j).copied())
                    .unwrap_or(ColumnAlign::Center);
                let dx = match align {
                    ColumnAlign::Left => 0.0,
                    ColumnAlign::Center => (col_w - cell.width) / 2.0,
                    ColumnAlign::Right => col_w - cell.width,
                };
                children.push(PositionedBox::new(cursor_x + dx, cursor_y, cell));
            }
            cursor_x += col_w;
            if j < n_cols - 1 {
                cursor_x += gap_after(j);
            }
        }

        cursor_y += row_depths[i];
    }

    let total_gap: f64 = (0..n_cols.saturating_sub(1)).map(gap_after).sum();
    let total_width: f64 = outer_pad * 2.0 + col_widths.iter().sum::<f64>() + total_gap;
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

    if col_seps != 0 {
        let rule_w = 0.04 * size;
        let rule_total_h = box_height + box_depth;
        for bx in column_rules(&col_widths, gap_after, outer_pad, total_width, col_seps) {
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
    for &cell_id in cell_ids_ast {
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
    let mut cursor_y = 0.0;

    // `array`-style tables pad outer edges with
    // `\arraycolsep` and use 2·arraycolsep between columns.
    let colsep = ARRAY_COLSEP_EM * size;
    let (edge_pad, uniform_gap) = if smallmatrix {
        let (edge, gap) = smallmatrix_horizontal_spacing(ctx.style, ctx.size_scale);
        (edge, Some(gap))
    } else if array_style {
        (colsep, Some(2.0 * colsep))
    } else {
        (0.0, None)
    };
    let gap_after = |j| uniform_gap.unwrap_or_else(|| col_gap_after(j, aligned, size));

    for i in 0..n_rows {
        cursor_y += row_heights[i];
        let mut cursor_x = edge_pad;

        for j in 0..nc.min(n_cols) {
            let idx = i * nc + j;
            if idx < cell_layout_ids.len() {
                let col_w = col_widths[j];
                let cell_w = arena.get(cell_layout_ids[idx]).width;
                let align = match *ast.node(cell_ids_ast[idx]) {
                    CompactNode::AlignedCell { alignment, .. } => alignment,
                    _ => columns.get(j).copied().unwrap_or(ColumnAlign::Center),
                };
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

    let total_gap: f64 = (0..n_cols.saturating_sub(1)).map(gap_after).sum();
    let total_width: f64 = edge_pad + col_widths.iter().sum::<f64>() + total_gap + edge_pad;

    // Add vertical separator rules if col_seps is non-zero
    if col_seps != 0 {
        let rule_w = 0.04 * size;
        let rule_total_h = box_height + (half - axis);
        let box_depth = half - axis;

        for bx in column_rules(&col_widths, gap_after, edge_pad, total_width, col_seps) {
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
