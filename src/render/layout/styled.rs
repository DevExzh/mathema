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

pub(super) fn layout_actuarial(content: &MathNode, force_text: bool, ctx: &LayoutCtx) -> LayoutBox {
    let size = ctx.size();
    let rule = (ctx.params.rule_thickness * size).max(0.04 * size);
    let top_pad = 4.0 * rule;
    let side_pad = size / 18.0;
    let trailing_margin = side_pad;
    let inner = if force_text {
        layout_node(content, &ctx.with_style(MathStyle::Text))
    } else {
        layout_node(content, ctx)
    };
    let bottom_pad = (0.25 * size - inner.depth).max(0.0);
    let body_height = inner.height;
    let body_depth = inner.depth + bottom_pad;
    let total = body_height + top_pad + body_depth;
    let right = LayoutBox {
        width: rule,
        height: body_height + top_pad,
        depth: body_depth,
        kind: BoxKind::Rule { thickness: total },
    };
    let right_x = 2.0 * side_pad + inner.width;
    let body = LayoutBox {
        width: inner.width + 2.0 * side_pad + rule,
        height: body_height,
        depth: body_depth,
        kind: BoxKind::HBox {
            children: vec![
                PositionedBox::new(side_pad, 0.0, inner),
                PositionedBox::new(right_x, 0.0, right),
            ],
        },
    };
    let top = LayoutBox::rule(body.width, rule);
    LayoutBox {
        width: body.width + trailing_margin,
        height: body.height + top_pad,
        depth: body.depth,
        kind: BoxKind::VBox {
            children: vec![
                PositionedBox::new(0.0, body.height + top_pad - top.height, top),
                PositionedBox::at_origin(body),
            ],
        },
    }
}

pub(super) fn layout_reflect(content: &MathNode, text_mode: bool, ctx: &LayoutCtx) -> LayoutBox {
    let inner = if text_mode {
        layout_node(content, &ctx.with_style(MathStyle::Text))
    } else {
        layout_node(content, ctx)
    };
    LayoutBox {
        width: inner.width,
        height: inner.height,
        depth: inner.depth,
        kind: BoxKind::Reflected {
            child: Box::new(PositionedBox::at_origin(inner)),
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
    if let MathNode::Symbol(_, s) = content
        && let Some(b) = layout_big_delimiter(factor, s, ctx.size_scale, &ctx.params)
    {
        return b.into_heap();
    }
    let inner = layout_node(content, ctx);
    let f = factor.as_f64();
    LayoutBox {
        width: inner.width * f,
        height: inner.height * f,
        depth: inner.depth * f,
        kind: scale_box_kind(inner.kind, f),
    }
}

/// A `\big`-family delimiter laid out through the font's size variants.
enum BigDelimLayout {
    /// `\big.`: KaTeX's null delimiter space.
    Null(f64),
    Glyph(BigDelimiter),
}

impl BigDelimLayout {
    #[inline(never)]
    fn into_heap(self) -> LayoutBox {
        match self {
            Self::Null(w) => LayoutBox::kern(w),
            Self::Glyph(d) => big_delimiter_box(d),
        }
    }

    #[inline(never)]
    fn into_arena(self, arena: &mut LayoutArena) -> LayoutBoxId {
        match self {
            Self::Null(w) => arena.alloc(ArenaLayoutBox::kern(w)),
            Self::Glyph(d) => alloc_big_delimiter(arena, d),
        }
    }
}

/// The `\big`-family layout of a `Scaled` symbol `s` at text-style font size
/// `size` (TeX sets `\big` delimiters at text size even in scripts), or
/// `None` when the node is not a `\big` delimiter the font can draw (it is
/// then scaled).
fn layout_big_delimiter(
    factor: Rational,
    s: &str,
    size: f64,
    params: &TexParams,
) -> Option<BigDelimLayout> {
    let mut chars = s.chars();
    match (chars.next(), chars.next()) {
        (None, _) if is_big_delim_factor(factor) => {
            Some(BigDelimLayout::Null(NULL_DELIMITER_SPACE_EM * size))
        },
        (Some(ch), None) => {
            big_delimiter(factor, ch, size, params.axis_height * size).map(BigDelimLayout::Glyph)
        },
        _ => None,
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

pub(super) fn layout_styled_node(style: TextStyle, node: &MathNode, ctx: &LayoutCtx) -> LayoutBox {
    use crate::unicode::fonts::styled_char;

    match node {
        MathNode::Identifier(s) => {
            if let Some(font) = fonts_registry::text_font_for_style(style) {
                if s.chars().count() == 1 {
                    let ch = s.chars().next().expect("non-empty checked");
                    LayoutBox::glyph_font(ch, ctx.size(), false, font)
                } else {
                    layout_text_run(s, ctx, false, Some(style))
                }
            } else {
                let mapped: String = s
                    .chars()
                    .map(|c| styled_char(c, style).unwrap_or(c))
                    .collect();
                layout_node(&MathNode::Identifier(mapped), ctx)
            }
        },
        MathNode::Number(s) => {
            if fonts_registry::text_font_for_style(style).is_some() {
                layout_text_run(s, ctx, false, Some(style))
            } else {
                let mapped: String = s
                    .chars()
                    .map(|c| styled_char(c, style).unwrap_or(c))
                    .collect();
                layout_node(&MathNode::Number(mapped), ctx)
            }
        },
        MathNode::Color { color, content } => {
            layout_styled_node(style, content, ctx).with_color(color)
        },
        MathNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => layout_styled_node(
            style,
            ctx.branch_style()
                .choose(display, text, script, script_script),
            ctx,
        ),
        MathNode::Row(children) => layout_styled(style, children, ctx),
        MathNode::Styled(inner_style, children) => layout_styled(*inner_style, children, ctx),
        MathNode::StyleSwitch(_) => LayoutBox::kern(0.0),
        other => layout_node(other, &ctx.with_styled_face(style)),
    }
}

pub(super) fn layout_styled(style: TextStyle, children: &[MathNode], ctx: &LayoutCtx) -> LayoutBox {
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
    let mut current_ctx = ctx.clone();

    for node in children {
        if let MathNode::StyleSwitch(value) = node {
            current_ctx = current_ctx.with_switch(*value);
            continue;
        }

        let raw_atom = spacing::classify_node_in(node, current_ctx.branch_style());
        let (adj_prev, adj_cur) = spacing::adjust_bin_context(prev_atom, Some(raw_atom));
        let cur_atom = adj_cur.expect("passed Some");
        if let Some(prev) = adj_prev {
            let space =
                spacing::inter_atom_space(prev, cur_atom, current_ctx.effective_spacing_style());
            if space > 0.0 {
                cursor_x += space * current_ctx.size();
            }
        }

        let layout = layout_styled_node(style, node, &current_ctx);
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

pub(super) fn layout_actuarial_arena(
    ast: &AstArena,
    content: NodeId,
    force_text: bool,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let size = ctx.size();
    let rule = (ctx.params.rule_thickness * size).max(0.04 * size);
    let side_pad = size / 18.0;
    let trailing_margin = side_pad;
    let inner_id = if force_text {
        layout_node_arena(ast, content, arena, &ctx.with_style(MathStyle::Text))
    } else {
        layout_node_arena(ast, content, arena, ctx)
    };
    let inner_depth = arena.get(inner_id).depth;
    let top_pad = 4.0 * rule;
    let bottom_pad = (0.25 * size - inner_depth).max(0.0);
    let inner_h = arena.get(inner_id).height;
    let inner_w = arena.get(inner_id).width;
    let body_height = inner_h;
    let body_depth = inner_depth + bottom_pad;
    let total = body_height + top_pad + body_depth;
    let right_id = arena.alloc(ArenaLayoutBox {
        width: rule,
        height: body_height + top_pad,
        depth: body_depth,
        kind: ArenaBoxKind::Rule { thickness: total },
    });
    let body_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: side_pad,
        dy: 0.0,
        id: inner_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: side_pad + inner_w + side_pad,
        dy: 0.0,
        id: right_id,
    });
    let body_children = arena.finish_children(body_start);
    let body_id = arena.alloc(ArenaLayoutBox {
        width: inner_w + 2.0 * side_pad + rule,
        height: body_height,
        depth: body_depth,
        kind: ArenaBoxKind::HBox {
            children: body_children,
        },
    });
    let top_id = arena.alloc(ArenaLayoutBox::rule(inner_w + 2.0 * side_pad + rule, rule));
    let outer_start = arena.begin_children();
    arena.push_child(ArenaPositionedBox {
        dx: 0.0,
        dy: arena.get(body_id).height + top_pad - arena.get(top_id).height,
        id: top_id,
    });
    arena.push_child(ArenaPositionedBox {
        dx: 0.0,
        dy: 0.0,
        id: body_id,
    });
    let outer_children = arena.finish_children(outer_start);
    arena.alloc(ArenaLayoutBox {
        width: inner_w + 2.0 * side_pad + rule + trailing_margin,
        height: arena.get(body_id).height + top_pad,
        depth: arena.get(body_id).depth,
        kind: ArenaBoxKind::VBox {
            children: outer_children,
        },
    })
}

pub(super) fn layout_color_arena(
    color: &str,
    child: LayoutBoxId,
    arena: &mut LayoutArena,
) -> LayoutBoxId {
    let inner = arena.get(child);
    arena.alloc(ArenaLayoutBox {
        width: inner.width,
        height: inner.height,
        depth: inner.depth,
        kind: ArenaBoxKind::Color {
            color: color.to_owned(),
            child,
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
    if let CompactNode::Symbol(_, sid) = *ast.node(content)
        && let Some(b) = layout_big_delimiter(factor, ast.str_val(sid), ctx.size_scale, &ctx.params)
    {
        return b.into_arena(arena);
    }
    let inner_id = layout_node_arena(ast, content, arena, ctx);
    let f = factor.as_f64();
    // Recursively scale all positions and dimensions in the subtree
    arena.scale_subtree(inner_id, f);
    inner_id
}

pub(super) fn layout_reflect_arena(
    ast: &AstArena,
    content: NodeId,
    text_mode: bool,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    let inner = if text_mode {
        layout_node_arena(ast, content, arena, &ctx.with_style(MathStyle::Text))
    } else {
        layout_node_arena(ast, content, arena, ctx)
    };
    let (width, height, depth) = {
        let node = arena.get(inner);
        (node.width, node.height, node.depth)
    };
    arena.alloc(ArenaLayoutBox {
        width,
        height,
        depth,
        kind: ArenaBoxKind::Reflected { child: inner },
    })
}

pub(super) fn layout_styled_node_arena(
    style: TextStyle,
    ast: &AstArena,
    id: NodeId,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
    use crate::unicode::fonts::styled_char;

    match *ast.node(id) {
        CompactNode::Identifier(sid) => {
            let s = ast.str_val(sid);
            if let Some(font) = fonts_registry::text_font_for_style(style) {
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
            if fonts_registry::text_font_for_style(style).is_some() {
                layout_text_run_arena(s, arena, ctx, false, Some(style))
            } else {
                let mapped: String = s
                    .chars()
                    .map(|c| styled_char(c, style).unwrap_or(c))
                    .collect();
                layout_text_run_arena(&mapped, arena, ctx, false, None)
            }
        },
        CompactNode::Color { color, content } => {
            let child = layout_styled_node_arena(style, ast, content, arena, ctx);
            layout_color_arena(ast.str_val(color), child, arena)
        },
        CompactNode::MathChoice {
            display,
            text,
            script,
            script_script,
        } => layout_styled_node_arena(
            style,
            ast,
            ctx.branch_style()
                .choose(display, text, script, script_script),
            arena,
            ctx,
        ),
        CompactNode::Row(span) => layout_styled_arena(ast, style, span, arena, ctx),
        CompactNode::Styled(inner_style, span) => {
            layout_styled_arena(ast, inner_style, span, arena, ctx)
        },
        CompactNode::StyleSwitch(_) => arena.alloc(ArenaLayoutBox::kern(0.0)),
        _ => layout_node_arena(ast, id, arena, &ctx.with_styled_face(style)),
    }
}

pub(super) fn layout_styled_arena(
    ast: &AstArena,
    style: TextStyle,
    span: NodeSpan,
    arena: &mut LayoutArena,
    ctx: &ArenaLayoutCtx,
) -> LayoutBoxId {
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
    let mut current_ctx = ctx.clone();

    for &id in ids {
        if let CompactNode::StyleSwitch(value) = *ast.node(id) {
            current_ctx = current_ctx.with_switch(value);
            continue;
        }

        // Inter-atom spacing (e.g. medium space around a binary `+`), matching
        // the legacy styled-row layout and the normal arena row builder.
        let raw_atom = spacing::classify_node_c_in(ast, id, current_ctx.branch_style());
        let (adj_prev, adj_cur) = spacing::adjust_bin_context(prev_atom, Some(raw_atom));
        let cur_atom = adj_cur.expect("passed Some");
        if let Some(prev) = adj_prev {
            let space =
                spacing::inter_atom_space(prev, cur_atom, current_ctx.effective_spacing_style());
            if space > 0.0 {
                cursor_x += space * current_ctx.size();
            }
        }

        let layout_id = layout_styled_node_arena(style, ast, id, arena, &current_ctx);
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
