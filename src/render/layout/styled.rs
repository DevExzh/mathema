use super::*;

pub(super) fn layout_phantom(content: &MathNode, kind: PhantomKind, ctx: &LayoutCtx) -> LayoutBox {
    let inner = layout_node(content, ctx);
    match kind {
        // Phantom: invisible but occupies space
        PhantomKind::Full | PhantomKind::Vertical | PhantomKind::Horizontal => LayoutBox {
            width: if kind == PhantomKind::Vertical {
                0.0
            } else {
                inner.width
            },
            height: if kind == PhantomKind::Horizontal {
                0.0
            } else {
                inner.height
            },
            depth: if kind == PhantomKind::Horizontal {
                0.0
            } else {
                inner.depth
            },
            kind: BoxKind::Phantom,
        },
        // Smash: visible but with zeroed height/depth
        PhantomKind::SmashBoth | PhantomKind::SmashBottom | PhantomKind::SmashTop => {
            let h = if kind == PhantomKind::SmashTop || kind == PhantomKind::SmashBoth {
                0.0
            } else {
                inner.height
            };
            let d = if kind == PhantomKind::SmashBottom || kind == PhantomKind::SmashBoth {
                0.0
            } else {
                inner.depth
            };
            LayoutBox {
                width: inner.width,
                height: h,
                depth: d,
                kind: BoxKind::HBox {
                    children: vec![PositionedBox::new(0.0, 0.0, inner)],
                },
            }
        },
    }
}

pub(super) fn layout_boxed(
    content: &MathNode,
    bg_color: Option<&str>,
    border_color: Option<&str>,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let size = ctx.size();
    let padding = 0.1 * size;
    let inner = layout_node(content, ctx);

    LayoutBox {
        width: inner.width + 2.0 * padding,
        height: inner.height + padding,
        depth: inner.depth + padding,
        kind: BoxKind::Boxed {
            child: Box::new(PositionedBox::new(padding, 0.0, inner)),
            padding,
            bg_color: bg_color.filter(|s| !s.is_empty()).map(|s| s.to_owned()),
            border_color: border_color.filter(|s| !s.is_empty()).map(|s| s.to_owned()),
        },
    }
}

/// Build the stroke `<path>` data for a `\cancel`/`\bcancel`/`\xcancel`/`\sout`
/// strike, rounded to the minimal coordinate precision via [`PathWriter`].
/// Shared by the heap and arena cancel layouts.
fn cancel_stroke_path(stroke: StrokeKind, w: f64, h: f64) -> String {
    use crate::render::path_writer::PathWriter;

    fn seg(pw: &mut PathWriter, x0: f64, y0: f64, x1: f64, y1: f64) {
        pw.cmd(b'M');
        pw.push_f64(x0);
        pw.push_f64(y0);
        pw.cmd(b'L');
        pw.push_f64(x1);
        pw.push_f64(y1);
    }

    let mut pw = PathWriter::with_prefix("");
    match stroke {
        StrokeKind::Forward => seg(&mut pw, 0.0, h, w, 0.0),
        StrokeKind::Backward => seg(&mut pw, 0.0, 0.0, w, h),
        StrokeKind::Cross => {
            seg(&mut pw, 0.0, h, w, 0.0);
            seg(&mut pw, 0.0, 0.0, w, h);
        },
        StrokeKind::Horizontal => {
            let mid = h / 2.0;
            seg(&mut pw, 0.0, mid, w, mid);
        },
        StrokeKind::Vertical => {
            let mid = w / 2.0;
            seg(&mut pw, mid, 0.0, mid, h);
        },
    }
    pw.finish()
}

pub(super) fn layout_cancel(stroke: StrokeKind, content: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let inner = layout_node(content, ctx);
    let w = inner.width;
    let h = inner.height + inner.depth;

    let path = cancel_stroke_path(stroke, w, h);

    // Capture dimensions before moving inner into children vec
    let inner_width = inner.width;
    let inner_height = inner.height;
    let inner_depth = inner.depth;

    let children = vec![
        PositionedBox::at_origin(inner),
        PositionedBox::at_origin(LayoutBox {
            width: w,
            height: inner_height,
            depth: inner_depth,
            kind: BoxKind::SvgPath(path),
        }),
    ];

    LayoutBox {
        width: inner_width,
        height: inner_height,
        depth: inner_depth,
        kind: BoxKind::HBox { children },
    }
}

pub(super) fn layout_scaled(factor: Rational, content: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    let inner = layout_node(content, ctx);
    let f = factor.as_f64();
    LayoutBox {
        width: inner.width * f,
        height: inner.height * f,
        depth: inner.depth * f,
        kind: scale_box_kind(inner.kind, f),
    }
}

pub(super) fn layout_rule(
    width: Rational,
    height: Rational,
    lift: Rational,
    ctx: &LayoutCtx,
) -> LayoutBox {
    let w = width.as_f64() * ctx.size();
    let h = height.as_f64() * ctx.size();
    let lift = lift.as_f64() * ctx.size();
    let rule = LayoutBox {
        width: w,
        height: h,
        depth: 0.0,
        kind: BoxKind::Rule { thickness: h },
    };
    LayoutBox {
        width: w,
        height: (lift + h).max(0.0),
        depth: (-lift).max(0.0),
        kind: BoxKind::HBox {
            children: vec![PositionedBox::new(0.0, lift, rule)],
        },
    }
}

pub(super) fn layout_styled(style: TextStyle, children: &[MathNode], ctx: &LayoutCtx) -> LayoutBox {
    use crate::unicode::fonts::styled_char;

    if children.is_empty() {
        return LayoutBox::kern(0.0);
    }

    // Inline the row layout logic to avoid cloning non-leaf nodes.
    // Only Identifier/Number leaves need remapping; everything else is
    // laid out directly from the original reference.
    let mut boxes = Vec::with_capacity(children.len() * 2);
    let mut cursor_x = 0.0;
    let mut prev_atom: Option<AtomType> = None;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;

    for node in children {
        let raw_atom = spacing::classify_node(node);
        let (adj_prev, adj_cur) = spacing::adjust_bin_context(prev_atom, Some(raw_atom));
        let cur_atom = adj_cur.expect("passed Some");
        if let Some(prev) = adj_prev {
            let space = spacing::inter_atom_space(prev, cur_atom, ctx.effective_spacing_style());
            if space > 0.0 {
                cursor_x += space * ctx.size();
            }
        }

        let lm_font = fonts_registry::text_font_for_style(style);
        let layout = match node {
            MathNode::Identifier(s) => {
                if let Some(font) = lm_font {
                    // Mappable style: draw the base codepoints from the matching
                    // Latin Modern text face (slant/weight baked into the face).
                    if s.chars().count() == 1 {
                        let ch = s.chars().next().expect("non-empty checked");
                        LayoutBox::glyph_font(ch, ctx.size(), false, font)
                    } else {
                        layout_text_run(s, ctx, false, Some(style))
                    }
                } else {
                    // Non-mappable style (italic letters, double-struck, script,
                    // fraktur): keep the Math-font Unicode-remap path.
                    let mapped: String = s
                        .chars()
                        .map(|c| styled_char(c, style).unwrap_or(c))
                        .collect();
                    layout_node(&MathNode::Identifier(mapped), ctx)
                }
            },
            MathNode::Number(s) => {
                if let Some(font) = lm_font {
                    let _ = font;
                    layout_text_run(s, ctx, false, Some(style))
                } else {
                    let mapped: String = s
                        .chars()
                        .map(|c| styled_char(c, style).unwrap_or(c))
                        .collect();
                    layout_node(&MathNode::Number(mapped), ctx)
                }
            },
            other => layout_node(other, ctx),
        };
        max_height = max_height.max(layout.height);
        max_depth = max_depth.max(layout.depth);
        let w = layout.width;
        boxes.push(PositionedBox::new(cursor_x, 0.0, layout));
        cursor_x += w;
        prev_atom = Some(cur_atom);
    }

    LayoutBox {
        width: cursor_x,
        height: max_height,
        depth: max_depth,
        kind: BoxKind::HBox { children: boxes },
    }
}

pub(super) fn layout_phantom_arena(
    ast: &AstArena,
    content: NodeId,
    kind: PhantomKind,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let inner_id = layout_node_arena(ast, content, arena, ctx);
    let inner_w = arena.get(inner_id).width;
    let inner_h = arena.get(inner_id).height;
    let inner_d = arena.get(inner_id).depth;

    match kind {
        PhantomKind::Full | PhantomKind::Vertical | PhantomKind::Horizontal => {
            arena.alloc(ArenaLayoutBox {
                width: if kind == PhantomKind::Vertical {
                    0.0
                } else {
                    inner_w
                },
                height: if kind == PhantomKind::Horizontal {
                    0.0
                } else {
                    inner_h
                },
                depth: if kind == PhantomKind::Horizontal {
                    0.0
                } else {
                    inner_d
                },
                kind: ArenaBoxKind::Phantom,
            })
        },
        PhantomKind::SmashBoth | PhantomKind::SmashBottom | PhantomKind::SmashTop => {
            let h = if kind == PhantomKind::SmashTop || kind == PhantomKind::SmashBoth {
                0.0
            } else {
                inner_h
            };
            let d = if kind == PhantomKind::SmashBottom || kind == PhantomKind::SmashBoth {
                0.0
            } else {
                inner_d
            };
            let children_start = arena.begin_children();
            arena.push_child(ArenaPositionedBox {
                dx: 0.0,
                dy: 0.0,
                id: inner_id,
            });
            let children = arena.finish_children(children_start);
            arena.alloc(ArenaLayoutBox {
                width: inner_w,
                height: h,
                depth: d,
                kind: ArenaBoxKind::HBox { children },
            })
        },
    }
}

pub(super) fn layout_boxed_arena(
    ast: &AstArena,
    content: NodeId,
    bg_color: Option<String>,
    border_color: Option<String>,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let size = ctx.size();
    let padding = 0.1 * size;
    let inner_id = layout_node_arena(ast, content, arena, ctx);
    let inner_w = arena.get(inner_id).width;
    let inner_h = arena.get(inner_id).height;
    let inner_d = arena.get(inner_id).depth;

    arena.alloc(ArenaLayoutBox {
        width: inner_w + 2.0 * padding,
        height: inner_h + padding,
        depth: inner_d + padding,
        kind: ArenaBoxKind::Boxed {
            child: inner_id,
            padding,
            bg_color,
            border_color,
        },
    })
}

pub(super) fn layout_color_arena(
    ast: &AstArena,
    color: StrId,
    content: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let color_str = ast.str_val(color).to_owned();
    let child_id = layout_node_arena(ast, content, arena, ctx);
    let w = arena.get(child_id).width;
    let h = arena.get(child_id).height;
    let d = arena.get(child_id).depth;

    arena.alloc(ArenaLayoutBox {
        width: w,
        height: h,
        depth: d,
        kind: ArenaBoxKind::Color {
            color: color_str,
            child: child_id,
        },
    })
}

pub(super) fn layout_cancel_arena(
    ast: &AstArena,
    stroke: StrokeKind,
    content: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let inner_id = layout_node_arena(ast, content, arena, ctx);
    let inner_w = arena.get(inner_id).width;
    let inner_h = arena.get(inner_id).height;
    let inner_d = arena.get(inner_id).depth;

    let w = inner_w;
    let h = inner_h + inner_d;

    let path = cancel_stroke_path(stroke, w, h);

    let path_id = arena.alloc(ArenaLayoutBox {
        width: w,
        height: inner_h,
        depth: inner_d,
        kind: ArenaBoxKind::SvgPath(path),
    });

    let children_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: 0.0,
        dy: 0.0,
        id: inner_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: 0.0,
        dy: 0.0,
        id: path_id,
    });
    let children = arena.finish_children(children_start);

    arena.alloc(ArenaLayoutBox {
        width: inner_w,
        height: inner_h,
        depth: inner_d,
        kind: ArenaBoxKind::HBox { children },
    })
}

pub(super) fn layout_scaled_arena(
    ast: &AstArena,
    factor: Rational,
    content: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let inner_id = layout_node_arena(ast, content, arena, ctx);
    let f = factor.as_f64();
    // Recursively scale all positions and dimensions in the subtree
    arena.scale_subtree(inner_id, f);
    inner_id
}

pub(super) fn layout_styled_arena(
    ast: &AstArena,
    style: TextStyle,
    span: NodeSpan,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    use crate::unicode::fonts::styled_char;

    let ids = ast.children(span);
    if ids.is_empty() {
        return arena.alloc(ArenaLayoutBox::kern(0.0));
    }

    // Phase 1: lay out every child first. We must NOT hold `begin_children`
    // open here: a composite child (e.g. an `Accent` taking the `_` arm below)
    // lays itself out via `layout_node_arena`, which pushes ITS OWN children
    // onto the shared, append-only children buffer. If our span were already
    // open, `finish_children` would wrongly capture those grandchildren too and
    // the wrapper would render its content twice. Mirror the normal HBox builder
    // (two-phase: lay out, then push).
    let mut laid_out: Vec<(LayoutBoxId, f64)> = Vec::with_capacity(ids.len());
    let mut cursor_x = 0.0;
    let mut max_height: f64 = 0.0;
    let mut max_depth: f64 = 0.0;
    let mut prev_atom: Option<AtomType> = None;

    for &id in ids {
        // Inter-atom spacing (e.g. medium space around a binary `+`), matching
        // the legacy styled-row layout and the normal arena row builder.
        let raw_atom = spacing::classify_node_c(ast, id);
        let (adj_prev, adj_cur) = spacing::adjust_bin_context(prev_atom, Some(raw_atom));
        let cur_atom = adj_cur.expect("passed Some");
        if let Some(prev) = adj_prev {
            let space = spacing::inter_atom_space(prev, cur_atom, ctx.effective_spacing_style());
            if space > 0.0 {
                cursor_x += space * ctx.size();
            }
        }

        let lm_font = fonts_registry::text_font_for_style(style);
        let layout_id = match *ast.node(id) {
            CompactNode::Identifier(sid) => {
                let s = ast.str_val(sid);
                if let Some(font) = lm_font {
                    if s.chars().count() == 1 {
                        let ch = s.chars().next().expect("non-empty checked");
                        alloc_glyph_font(arena, ch, ctx.size(), false, font)
                    } else {
                        layout_text_run_arena(s, arena, ctx, false, Some(style))
                    }
                } else {
                    let mapped: String = s
                        .chars()
                        .map(|c| styled_char(c, style).unwrap_or(c))
                        .collect();
                    // The style is already baked into the remapped Unicode
                    // codepoint (e.g. `\mathit{x}` → U+1D465 𝑥), so decide
                    // faux-italic exactly as a bare identifier would rather than
                    // forcing it on — otherwise an already-italic glyph would be
                    // double-italicized (a spurious `font-style="italic"`). This
                    // mirrors the normal identifier path and the legacy engine.
                    if mapped.chars().count() == 1 {
                        let ch = mapped.chars().next().expect("non-empty checked");
                        alloc_glyph(arena, ch, ctx.size(), is_math_italic_char(ch))
                    } else {
                        layout_text_run_arena(&mapped, arena, ctx, false, None)
                    }
                }
            },
            CompactNode::Number(sid) => {
                let s = ast.str_val(sid);
                if lm_font.is_some() {
                    layout_text_run_arena(s, arena, ctx, false, Some(style))
                } else {
                    let mapped: String = s
                        .chars()
                        .map(|c| styled_char(c, style).unwrap_or(c))
                        .collect();
                    layout_text_run_arena(&mapped, arena, ctx, false, None)
                }
            },
            _ => layout_node_arena(ast, id, arena, ctx),
        };
        let b = arena.get(layout_id);
        max_height = max_height.max(b.height);
        max_depth = max_depth.max(b.depth);
        let w = b.width;
        laid_out.push((layout_id, cursor_x));
        cursor_x += w;
        prev_atom = Some(cur_atom);
    }

    // Phase 2: now that all sub-layouts are done, capture exactly this box's
    // direct children.
    let children_start = arena.begin_children();
    for &(layout_id, dx) in &laid_out {
        arena.push_child(ArenaPositionedBox {
            dx,
            dy: 0.0,
            id: layout_id,
        });
    }
    let children = arena.finish_children(children_start);
    arena.alloc(ArenaLayoutBox {
        width: cursor_x,
        height: max_height,
        depth: max_depth,
        kind: ArenaBoxKind::HBox { children },
    })
}
