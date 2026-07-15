use super::*;
use oxc_allocator::Allocator;

#[test]
fn code_gen_output_overwrite_pushes_to_vec() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    out.overwrite(0, 5, "hello");
    assert_eq!(out.overwrites.len(), 1);
    assert_eq!(out.overwrites[0].0, 0);
    assert_eq!(out.overwrites[0].1, 5);
    assert_eq!(out.overwrites[0].2, "hello");
}

#[test]
fn code_gen_output_prepend_static_pushes_to_vec() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    out.prepend_static(10, "_ctx.");
    assert_eq!(out.prepends.len(), 1);
    assert_eq!(out.prepends[0].0, 10);
    assert_eq!(out.prepends[0].1, "_ctx.");
}

#[test]
fn code_gen_output_prepend_alloc_pushes_to_vec() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    out.prepend_alloc(5, "dynamic");
    assert_eq!(out.prepends.len(), 1);
    assert_eq!(out.prepends[0].0, 5);
    assert_eq!(out.prepends[0].1, "dynamic");
}

#[test]
fn apply_to_sorts_overwrites_by_start() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    // Push in reverse order
    out.overwrite(10, 15, "b");
    out.overwrite(0, 5, "a");

    let mut ct = crate::code_transform::CodeTransform::new("0123456789ABCDE", &alloc);
    out.apply_to(&mut ct);
    let result = ct.build_string();
    assert_eq!(result, "a56789b");
}

#[test]
fn apply_to_sorts_prepends_by_position() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    // Push in reverse order
    out.prepend_static(5, "Y");
    out.prepend_static(2, "X");

    let mut ct = crate::code_transform::CodeTransform::new("ABCDEFGH", &alloc);
    out.apply_to(&mut ct);
    let result = ct.build_string();
    assert_eq!(result, "ABXCDEYFGH");
}

/// @ai-generated - Regression test: prepends at the same position must
/// preserve insertion order (stable sort). This matters when scope_close
/// suffixes and sibling comma separators are both prepended at an
/// element's end position. Without stable sort, the comma can appear
/// before the scope_close, producing invalid JS like `, : _createCommentVNode`.
#[test]
fn apply_to_preserves_same_position_prepend_order() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);

    // Simulate the real compilation pattern: scope_close is pushed early
    // (during child's leave_element), then many other prepends are added
    // for other parts of the template, then the sibling comma is pushed
    // (during parent's add_children_separators).
    // The target position where both scope_close and comma land:
    let target = 50u32;

    // First batch: prepends BEFORE the scope_close (from earlier template processing)
    for i in 0..40u32 {
        out.prepend_static(i, "x");
    }
    // scope_close is pushed at target position
    out.prepend_static(target, "SCOPE_CLOSE");
    // Second batch: many more prepends from other template elements
    // (these go to positions AFTER target, interleaved)
    for i in 0..60u32 {
        out.prepend_static(target + 1 + i, "y");
    }
    // Sibling comma is pushed much later at the SAME target position
    out.prepend_static(target, "COMMA");

    let source = &"_".repeat(200);
    let mut ct = crate::code_transform::CodeTransform::new(source, &alloc);
    out.apply_to(&mut ct);
    let result = ct.build_string();

    // The two same-position prepends must appear in insertion order
    assert!(
        result.contains("SCOPE_CLOSECOMMA"),
        "Same-position prepends must preserve insertion order.\n\
             Expected 'SCOPE_CLOSECOMMA' but got:\n{}",
        result
    );
}

// ==================== Imports ====================

#[test]
fn add_vdom_import_sets_flag() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    out.add_vdom_import(VdomHelper::CreateElementVNode);
    assert!(out.vdom_imports().has(VdomHelper::CreateElementVNode));
}

#[test]
fn add_vdom_import_deduplicates() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    out.add_vdom_import(VdomHelper::ToDisplayString);
    out.add_vdom_import(VdomHelper::CreateElementVNode);
    out.add_vdom_import(VdomHelper::ToDisplayString); // duplicate
    let imports = out.vdom_imports().to_imports();
    assert_eq!(imports.len(), 2);
}

#[test]
fn apply_to_returns_vdom_imports() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    out.add_vdom_import(VdomHelper::CreateCommentVNode);
    out.add_vdom_import(VdomHelper::ToDisplayString);

    let mut ct = crate::code_transform::CodeTransform::new("hello", &alloc);
    let imports = out.apply_to(&mut ct);
    assert_eq!(imports.vue.len(), 2);
    assert!(imports.vue.contains(&"_createCommentVNode"));
    assert!(imports.vue.contains(&"_toDisplayString"));
    assert!(imports.ssr.is_empty());
}

#[test]
fn apply_to_returns_vapor_imports() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    out.add_vapor_import(VaporHelper::Template);
    out.add_vapor_import(VaporHelper::RenderEffect);

    let mut ct = crate::code_transform::CodeTransform::new("hello", &alloc);
    let imports = out.apply_to(&mut ct);
    assert_eq!(imports.vue.len(), 2);
    assert!(imports.vue.contains(&"_template"));
    assert!(imports.vue.contains(&"_renderEffect"));
    assert!(imports.ssr.is_empty());
}

#[test]
fn empty_output_returns_empty_imports() {
    let alloc = Allocator::default();
    let out = CodeGenOutput::new(&alloc);

    let mut ct = crate::code_transform::CodeTransform::new("hello", &alloc);
    let imports = out.apply_to(&mut ct);
    assert!(imports.is_empty());
}

// ==================== VaporCounters ====================

#[test]
fn vapor_counters_increment() {
    let mut c = VaporCounters::default();
    assert_eq!(c.next_node(), 0);
    assert_eq!(c.next_node(), 1);
    assert_eq!(c.next_text(), 0);
    assert_eq!(c.next_text(), 1);
    assert_eq!(c.next_path(), 0);
    assert_eq!(c.next_template(), 0);
    assert_eq!(c.next_template(), 1);
}

// ==================== VaporTextPart ====================

#[test]
fn vapor_text_part_static() {
    let part = VaporTextPart::Static("\"hello\"");
    assert_eq!(part.to_js(), "\"hello\"");
    assert!(!part.is_dynamic());
}

#[test]
fn vapor_text_part_dynamic() {
    let part = VaporTextPart::Dynamic("_toDisplayString(_ctx.msg)");
    assert_eq!(part.to_js(), "_toDisplayString(_ctx.msg)");
    assert!(part.is_dynamic());
}

// ==================== VaporEffect ====================

#[test]
fn vapor_effect_set_text() {
    let effect = VaporEffect::SetText {
        text_ref: 0,
        parts: vec![
            VaporTextPart::Static("\"hello \""),
            VaporTextPart::Dynamic("_toDisplayString(_ctx.msg)"),
        ],
    };
    assert_eq!(
        effect.to_code(),
        "_setText(x0, \"hello \" + _toDisplayString(_ctx.msg))"
    );
}

#[test]
fn vapor_effect_set_text_single_part() {
    let effect = VaporEffect::SetText {
        text_ref: 1,
        parts: vec![VaporTextPart::Dynamic("_toDisplayString(_ctx.count)")],
    };
    assert_eq!(
        effect.to_code(),
        "_setText(x1, _toDisplayString(_ctx.count))"
    );
}

#[test]
fn vapor_effect_set_class() {
    let effect = VaporEffect::SetClass {
        node_ref: 0,
        expr: "_ctx.cls",
    };
    assert_eq!(effect.to_code(), "_setClass(n0, _ctx.cls)");
}

#[test]
fn vapor_effect_set_style() {
    let effect = VaporEffect::SetStyle {
        node_ref: 2,
        expr: "_ctx.sty",
    };
    assert_eq!(effect.to_code(), "_setStyle(n2, _ctx.sty)");
}

#[test]
fn vapor_effect_set_prop() {
    let effect = VaporEffect::SetProp {
        node_ref: 0,
        attr: "title",
        expr: "_ctx.title",
    };
    assert_eq!(effect.to_code(), "_setProp(n0, \"title\", _ctx.title)");
}

#[test]
fn vapor_effect_set_attr() {
    let effect = VaporEffect::SetAttr {
        node_ref: 1,
        attr: "data-id",
        expr: "_ctx.id",
    };
    assert_eq!(effect.to_code(), "_setAttr(n1, \"data-id\", _ctx.id)");
}

#[test]
fn vapor_effect_set_html() {
    let effect = VaporEffect::SetHtml {
        node_ref: 0,
        expr: "_ctx.rawHtml",
    };
    assert_eq!(effect.to_code(), "_setHtml(n0, _ctx.rawHtml)");
}

#[test]
fn vapor_effect_set_html_with_resolved_ref() {
    let effect = VaporEffect::SetHtml {
        node_ref: 1,
        expr: "rawHtml.value",
    };
    assert_eq!(effect.to_code(), "_setHtml(n1, rawHtml.value)");
}

#[test]
fn vapor_effect_set_dynamic_props() {
    let effect = VaporEffect::SetDynamicProps {
        node_ref: 0,
        expr: "_ctx.obj",
    };
    assert_eq!(effect.to_code(), "_setDynamicProps(n0, [_ctx.obj])");
}

#[test]
fn vapor_effect_set_dynamic_props_with_resolved_ref() {
    let effect = VaporEffect::SetDynamicProps {
        node_ref: 2,
        expr: "obj.value",
    };
    assert_eq!(effect.to_code(), "_setDynamicProps(n2, [obj.value])");
}

// ==================== VaporElementState ====================

#[test]
fn vapor_element_state_new() {
    let state = VaporElementState::new();
    assert!(state.node_ref.is_none());
    assert!(state.text_node_ref.is_none());
    assert!(state.html.is_empty());
    assert!(state.text_parts.is_empty());
    assert!(state.own_effects.is_empty());
    assert!(state.child_nav.is_empty());
}

#[test]
fn vapor_element_state_ensure_node_ref() {
    let mut counters = VaporCounters::default();
    let mut state = VaporElementState::new();
    let r1 = state.ensure_node_ref(&mut counters);
    assert_eq!(r1, 0);
    // Second call returns same ref
    let r2 = state.ensure_node_ref(&mut counters);
    assert_eq!(r2, 0);
    // Counter only incremented once
    assert_eq!(counters.n, 1);
}

#[test]
fn vapor_element_state_ensure_text_ref() {
    let mut counters = VaporCounters::default();
    let mut state = VaporElementState::new();
    let r1 = state.ensure_text_ref(&mut counters);
    assert_eq!(r1, 0);
    let r2 = state.ensure_text_ref(&mut counters);
    assert_eq!(r2, 0);
    assert_eq!(counters.x, 1);
}

// ==================== Mapped Prepends ====================

/// @ai-generated — prepend_alloc_mapped pushes to mapped_prepends vec
#[test]
fn prepend_alloc_mapped_pushes_to_vec() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    out.prepend_alloc_mapped(10, 20, "(show) ? ");
    assert_eq!(out.mapped_prepends.len(), 1);
    assert_eq!(out.mapped_prepends[0].0, 10); // insertion pos
    assert_eq!(out.mapped_prepends[0].1, 20); // source pos
    assert_eq!(out.mapped_prepends[0].2, 0); // content_offset
    assert_eq!(out.mapped_prepends[0].3, "(show) ? ");
}

/// @ai-generated — apply_to merges mapped and regular prepends correctly
#[test]
fn apply_to_merges_mapped_and_regular_prepends() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    // Regular prepend at position 5
    out.prepend_static(5, "_ctx.");
    // Mapped prepend at position 3
    out.prepend_alloc_mapped(3, 100, "(show) ? ");

    let source = "ABCDEFGHIJ";
    let mut ct = crate::code_transform::CodeTransform::new(source, &alloc);
    out.apply_to(&mut ct);
    let result = ct.build_string();
    // Position 3: "(show) ? " inserted, position 5: "_ctx." inserted
    assert_eq!(result, "ABC(show) ? DE_ctx.FGHIJ");
}

/// @ai-generated — apply_to with mapped prepends produces source-mapped tokens
#[test]
fn apply_to_mapped_prepend_produces_source_map_token() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    // Insert "(show) ? " at position 5, mapped to source position 20
    out.prepend_alloc_mapped(5, 20, "(show) ? ");

    let source = "0123456789ABCDEFGHIJKLMNOP";
    let mut ct = crate::code_transform::CodeTransform::new(source, &alloc);
    out.apply_to(&mut ct);

    let map =
        ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("test.vue"));
    let tokens: Vec<_> = map.get_tokens().collect();

    // Find token mapping to source col 20
    let mapped = tokens
        .iter()
        .find(|t| t.get_src_col() == 20 && t.get_source_id().is_some());
    assert!(
        mapped.is_some(),
        "should have source-mapped token at src col 20"
    );
}

// ==================== Format-sink emission APIs ====================

/// `overwrite_fmt` must produce a byte-identical operation to
/// `overwrite(start, end, &format!(...))`: same content string, same
/// range, same built output. The format sink only changes how the
/// operation's string is PRODUCED (reusable scratch + one bump copy),
/// never the edit semantics.
#[test]
fn overwrite_fmt_matches_overwrite_with_format() {
    let alloc = Allocator::default();
    let source = "0123456789ABCDE";

    let mut via_format = CodeGenOutput::new(&alloc);
    via_format.overwrite(0, 5, &format!("x{}y{}", 1, "z"));

    let mut via_fmt = CodeGenOutput::new(&alloc);
    via_fmt.overwrite_fmt(0, 5, format_args!("x{}y{}", 1, "z"));

    // Same recorded operation (range + content bytes).
    assert_eq!(via_format.overwrites, via_fmt.overwrites);

    // Same built output.
    let mut ct_a = crate::code_transform::CodeTransform::new(source, &alloc);
    via_format.apply_to(&mut ct_a);
    let mut ct_b = crate::code_transform::CodeTransform::new(source, &alloc);
    via_fmt.apply_to(&mut ct_b);
    assert_eq!(ct_a.build_string(), ct_b.build_string());
    assert_eq!(ct_b.build_string(), "x1yz56789ABCDE");
}

/// `prepend_fmt` must produce a byte-identical operation to
/// `prepend_alloc(pos, &format!(...))`.
#[test]
fn prepend_fmt_matches_prepend_alloc_with_format() {
    let alloc = Allocator::default();
    let source = "ABCDEFGHIJ";

    let mut via_format = CodeGenOutput::new(&alloc);
    via_format.prepend_alloc(5, &format!("{}: ", "foo"));

    let mut via_fmt = CodeGenOutput::new(&alloc);
    via_fmt.prepend_fmt(5, format_args!("{}: ", "foo"));

    assert_eq!(via_format.prepends, via_fmt.prepends);

    let mut ct_a = crate::code_transform::CodeTransform::new(source, &alloc);
    via_format.apply_to(&mut ct_a);
    let mut ct_b = crate::code_transform::CodeTransform::new(source, &alloc);
    via_fmt.apply_to(&mut ct_b);
    assert_eq!(ct_a.build_string(), ct_b.build_string());
    assert_eq!(ct_b.build_string(), "ABCDEfoo: FGHIJ");
}

/// `prepend_fmt_mapped` records the same mapped operation as
/// `prepend_alloc_mapped` (content_offset 0), and both paths build
/// byte-identical output.
#[test]
fn prepend_fmt_mapped_matches_prepend_alloc_mapped() {
    let alloc = Allocator::default();
    let source = "0123456789ABCDEFGHIJ";

    let mut via_alloc = CodeGenOutput::new(&alloc);
    via_alloc.prepend_alloc_mapped(10, 20, &format!("({}) ? ", "show"));

    let mut via_fmt = CodeGenOutput::new(&alloc);
    via_fmt.prepend_fmt_mapped(10, 20, format_args!("({}) ? ", "show"));

    // Same recorded operation (mapped-prepend vec, content_offset 0).
    assert_eq!(via_alloc.mapped_prepends, via_fmt.mapped_prepends);
    assert_eq!(via_fmt.mapped_prepends[0].2, 0); // content_offset

    // Both paths build byte-identical output.
    let mut ct_alloc = crate::code_transform::CodeTransform::new(source, &alloc);
    via_alloc.apply_to(&mut ct_alloc);
    let mut ct_fmt = crate::code_transform::CodeTransform::new(source, &alloc);
    via_fmt.apply_to(&mut ct_fmt);
    assert_eq!(ct_alloc.build_string(), ct_fmt.build_string());
    assert_eq!(ct_fmt.build_string(), "0123456789(show) ? ABCDEFGHIJ");
}

/// `prepend_fmt_mapped_with_offset` records the same mapped operation
/// (explicit source offset) as `prepend_alloc_mapped_with_offset`, both
/// paths build byte-identical output, and the resulting source map emits a
/// token at the requested source position offset within the formatted
/// content.
#[test]
fn prepend_fmt_mapped_with_offset_matches_and_maps() {
    let alloc = Allocator::default();
    let source = "0123456789ABCDEFGHIJKLMNOP";

    let mut via_alloc = CodeGenOutput::new(&alloc);
    via_alloc.prepend_alloc_mapped_with_offset(5, 20, 1, &format!("({}", "show"));

    let mut via_fmt = CodeGenOutput::new(&alloc);
    via_fmt.prepend_fmt_mapped_with_offset(5, 20, 1, format_args!("({}", "show"));

    // Same recorded operation.
    assert_eq!(via_alloc.mapped_prepends, via_fmt.mapped_prepends);

    // Both paths build byte-identical output.
    let mut ct_alloc = crate::code_transform::CodeTransform::new(source, &alloc);
    via_alloc.apply_to(&mut ct_alloc);
    let mut ct_fmt = crate::code_transform::CodeTransform::new(source, &alloc);
    via_fmt.apply_to(&mut ct_fmt);
    assert_eq!(ct_alloc.build_string(), ct_fmt.build_string());
    assert_eq!(ct_fmt.build_string(), "01234(show56789ABCDEFGHIJKLMNOP");

    // The format-sink path emits a source-mapped token at src col 20.
    let map =
        ct_fmt.generate_map(crate::code_transform::SourceMapOptions::new().with_source("test.vue"));
    let tokens: Vec<_> = map.get_tokens().collect();
    let mapped = tokens
        .iter()
        .find(|t| t.get_src_col() == 20 && t.get_source_id().is_some());
    assert!(
        mapped.is_some(),
        "format-sink mapped prepend should emit a source-mapped token at src col 20"
    );
}

/// Allocation-reduction invariant: the format sinks share ONE reusable
/// scratch buffer that is cleared and reused per emission. A large emission
/// grows the buffer; every subsequent smaller emission reuses the retained
/// capacity instead of allocating a fresh `String`. Pinning the retained
/// capacity across many emissions discriminates the one-reused-buffer sink
/// from a per-call fresh-`String` sink (which carries no capacity between
/// calls).
#[test]
fn format_sinks_reuse_one_scratch_buffer() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);

    // A long emission forces the scratch buffer to grow.
    let long = "y".repeat(256);
    out.overwrite_fmt(0, 1, format_args!("{}", long));
    let cap_after_long = out.scratch_capacity();
    assert!(
        cap_after_long >= 256,
        "scratch buffer should hold the long emission (cap {cap_after_long})"
    );

    // Many subsequent short emissions must reuse the retained capacity —
    // the buffer is never reallocated smaller, and no fresh String is
    // created per call.
    for i in 0..1000u32 {
        out.prepend_fmt(i, format_args!("{}", i % 10));
    }
    assert_eq!(
        out.scratch_capacity(),
        cap_after_long,
        "scratch capacity must be retained across emissions (one reused buffer, \
             not a fresh String per call)"
    );

    // The operations were still recorded correctly (1 overwrite + 1000 prepends).
    assert_eq!(out.overwrites.len(), 1);
    assert_eq!(out.prepends.len(), 1000);
}

// ==================== merged prepend channels ====================

/// When both the unmapped (`prepends`) and source-mapped (`mapped_prepends`)
/// channels collide at ONE insertion position, the in-place channel merge
/// must preserve the exact emission order: every unmapped prepend (in
/// insertion order) precedes every mapped prepend (in insertion order) at
/// that position. The prepends are CALLED in interleaved order (mapped, plain,
/// mapped, plain) at the same anchor to prove the ordering is channel-based
/// (plain-first), not call-order — so a tie-break flip or a call-order
/// interleave is caught. Bytes and token placement are pinned exactly.
#[test]
fn apply_to_merges_colliding_plain_and_mapped_prepends_in_channel_order() {
    let alloc = Allocator::default();
    let source = "abcdef";

    let mut out = CodeGenOutput::new(&alloc);
    out.prepend_alloc_mapped(3, 0, "M1"); // mapped → src col 0
    out.prepend_static(3, "P1"); // unmapped
    out.prepend_alloc_mapped(3, 1, "M2"); // mapped → src col 1
    out.prepend_static(3, "P2"); // unmapped

    let mut ct = crate::code_transform::CodeTransform::new(source, &alloc);
    out.apply_to(&mut ct);

    // Exact bytes: plain (P1,P2 in insertion order) THEN mapped (M1,M2).
    assert_eq!(ct.build_string(), "abcP1P2M1M2def");

    let map = ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("t.vue"));
    let tokens: Vec<_> = map.get_tokens().collect();
    let dump: Vec<_> = tokens
        .iter()
        .map(|t| {
            (
                t.get_dst_col(),
                t.get_src_col(),
                t.get_source_id().is_some(),
            )
        })
        .collect();

    // M1 maps to src col 0 at gen col 7 (after "abc" + "P1P2").
    assert!(
        tokens
            .iter()
            .any(|t| t.get_dst_col() == 7 && t.get_source_id().is_some() && t.get_src_col() == 0),
        "M1 must map to src col 0 at gen col 7; tokens: {dump:?}"
    );
    // M2 maps to src col 1 at gen col 9.
    assert!(
        tokens
            .iter()
            .any(|t| t.get_dst_col() == 9 && t.get_source_id().is_some() && t.get_src_col() == 1),
        "M2 must map to src col 1 at gen col 9; tokens: {dump:?}"
    );
    // Negative: the unmapped plain region [3,7) carries NO source-mapped token.
    assert!(
        !tokens
            .iter()
            .any(|t| t.get_dst_col() >= 3 && t.get_dst_col() < 7 && t.get_source_id().is_some()),
        "plain prepend region must be unmapped; tokens: {dump:?}"
    );
}

// ==================== mapped generated-text insertions ====================

/// Build a [`MappedGeneratedText`] from ordered `(text, source)` segments,
/// mirroring how `build_prefixed_expr_segments` records a plan.
fn mgt_from(segments: &[(&str, Option<u32>)]) -> MappedGeneratedText {
    let mut mgt = MappedGeneratedText::default();
    for &(text, source) in segments {
        mgt.push(text, source);
    }
    mgt
}

/// Structural — `prepend_mapped_generated_text` records one ordered
/// `mapped_prepends` entry per segment at the SAME anchor: a source segment
/// as `(pos, source_pos, 0, content)` (token at its start) and a synthetic
/// segment as `(pos, 0, len, content)` (content_offset == len → the emitter
/// places it as an unmapped run and emits no source token). Nothing leaks
/// into the unmapped fast-path `prepends` channel.
#[test]
fn prepend_mapped_generated_text_records_one_entry_per_segment() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    let mgt = mgt_from(&[("(__props.", None), ("count", Some(4)), (" + 1)", None)]);
    out.prepend_mapped_generated_text(10, &mgt);

    assert_eq!(out.mapped_prepends.len(), 3);
    // Synthetic prefix: source_pos 0, content_offset == len (9).
    assert_eq!(out.mapped_prepends[0], (10, 0, 9, "(__props."));
    // Source segment: source_pos 4, content_offset 0.
    assert_eq!(out.mapped_prepends[1], (10, 4, 0, "count"));
    // Synthetic suffix: source_pos 0, content_offset == len (5).
    assert_eq!(out.mapped_prepends[2], (10, 0, 5, " + 1)"));
    // Nothing leaks into the unmapped fast-path channel.
    assert!(out.prepends.is_empty());
    // The segmented insertion touches ONLY the mapped-prepend channel and
    // queues NO overwrite, so it never interacts with the overwrite
    // containment filter — a segmented insertion can never leave ghost
    // content the way a range-removal-plus-independent-prepend composition
    // would when the filter drops the contained removal.
    assert!(out.overwrites.is_empty());
}

/// Empty segments are never carried by the plan, so none are recorded.
#[test]
fn mapped_generated_text_skips_empty_segments() {
    let alloc = Allocator::default();
    let mut out = CodeGenOutput::new(&alloc);
    let mgt = mgt_from(&[("", None), ("x", Some(0)), ("", Some(2))]);
    // The carrier itself drops empty pushes.
    assert_eq!(mgt.segments.len(), 1);
    out.prepend_mapped_generated_text(5, &mgt);
    assert_eq!(out.mapped_prepends.len(), 1);
    assert_eq!(out.mapped_prepends[0], (5, 0, 0, "x"));
}

/// Discriminating — a mapped-text insertion mixing a synthetic
/// (unmapped) prefix, a source-derived identifier, and a synthetic suffix
/// places EXACTLY one source token, at the identifier, mapping to its
/// original offset; the synthetic runs emit NO source token. Emitting one
/// concatenated token for the whole insertion would bleed the identifier
/// mapping across the synthetic suffix — the [24,29) negative assertion
/// catches that.
#[test]
fn prepend_mapped_generated_text_places_one_token_per_source_segment() {
    let alloc = Allocator::default();
    // Source: "abc count def" — "count" starts at byte 4.
    let source = "abc count def";

    let mut out = CodeGenOutput::new(&alloc);
    let mgt = mgt_from(&[("(__props.", None), ("count", Some(4)), (" + 1)", None)]);
    out.prepend_mapped_generated_text(10, &mgt);

    let mut ct = crate::code_transform::CodeTransform::new(source, &alloc);
    out.apply_to(&mut ct);

    assert_eq!(ct.build_string(), "abc count (__props.count + 1)def");

    let map = ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("t.vue"));
    let tokens: Vec<_> = map.get_tokens().collect();
    let dump: Vec<_> = tokens
        .iter()
        .map(|t| {
            (
                t.get_dst_col(),
                t.get_src_col(),
                t.get_source_id().is_some(),
            )
        })
        .collect();

    // The "count" segment maps to src col 4 at gen col 19 (10 + len "(__props.").
    let count = tokens
        .iter()
        .find(|t| t.get_dst_col() == 19 && t.get_source_id().is_some());
    assert!(count.is_some(), "count segment must map; tokens: {dump:?}");
    assert_eq!(
        count.unwrap().get_src_col(),
        4,
        "count must map to src col 4"
    );
    assert_eq!(count.unwrap().get_src_line(), 0);

    // Negative: synthetic prefix "(__props." region [10,19) has NO source token.
    assert!(
        !tokens
            .iter()
            .any(|t| t.get_dst_col() >= 10 && t.get_dst_col() < 19 && t.get_source_id().is_some()),
        "synthetic prefix must emit no source token; tokens: {dump:?}"
    );
    // Discriminating: synthetic suffix " + 1)" must START its OWN unmapped
    // segment at gen col 24 (its own token, source_id none). A single
    // concatenated mapped op emits the token at the content offset and then
    // advances through the rest, so NO token starts at col 24 and the
    // `count` mapping silently covers the suffix — this positive assertion
    // fails on that bleed where a "no token starts here" check would not.
    assert!(
        tokens
            .iter()
            .any(|t| t.get_dst_col() == 24 && t.get_source_id().is_none()),
        "synthetic suffix must start an unmapped segment at col 24; tokens: {dump:?}"
    );
    // Negative: synthetic suffix " + 1)" region [24,29) has NO source token.
    assert!(
        !tokens
            .iter()
            .any(|t| t.get_dst_col() >= 24 && t.get_dst_col() < 29 && t.get_source_id().is_some()),
        "synthetic suffix must emit no source token (no mapping bleed); tokens: {dump:?}"
    );
}

/// `wrapped` adds unmapped `prefix`/`suffix` segments and shifts the inner
/// segment offsets, preserving each inner segment's source mapping. Used to add
/// the `(` … `) ? ` ternary head around a resolved condition expression.
#[test]
fn mapped_generated_text_wrapped_shifts_inner_and_keeps_wrapper_unmapped() {
    // Inner plan: `count` (mapped to src 4) + `.value` (synthetic).
    let inner = mgt_from(&[("count", Some(4)), (".value", None)]);
    let wrapped = inner.wrapped("(", ") ? ");

    assert_eq!(wrapped.text, "(count.value) ? ");
    let pairs: Vec<_> = wrapped
        .segments
        .iter()
        .map(|s| {
            (
                &wrapped.text[s.generated_start as usize..s.generated_end as usize],
                s.source_start,
            )
        })
        .collect();
    assert_eq!(
        pairs,
        vec![
            ("(", None),
            ("count", Some(4)),
            (".value", None),
            (") ? ", None),
        ],
        "wrapper punctuation stays unmapped; inner mappings are preserved (shifted by `(`)"
    );
    // The inner `count` segment shifted right by one byte (the `(`).
    assert_eq!(wrapped.segments[1].generated_start, 1);
}
