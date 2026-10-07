use super::*;

// ── Basic nodes ────────────────────────────────────────────

#[test]
fn basic_div() {
    let result = gen_tsx_template("<template><div></div></template>");
    assert!(result.contains("<div></div>"), "got: {}", result);
}

#[test]
fn comment_preserved() {
    let result = gen_tsx_template("<template><!-- hello --></template>");
    assert!(
        result.contains("{/* hello */}"),
        "Comment should be converted to JSX, got: {}",
        result
    );
}

// ── ref attribute tests ──────────────────────────────────────

#[test]
fn ref_static_converts_to_jsx_expression() {
    let result = gen_tsx_template(r#"<template><div ref="myRef">content</div></template>"#);
    // Should convert to ref={"myRef"} (JSX expression with string literal)
    assert!(
        result.contains(r#"ref={"myRef"}"#),
        "static ref should become ref={{\"myRef\"}}, got: {}",
        result
    );
    // Must NOT have bare ref="myRef" (Vue syntax, not valid JSX expression)
    assert!(
        !result.contains(r#"ref="myRef""#),
        "bare ref=\"myRef\" must not appear in JSX output, got: {}",
        result
    );
}

#[test]
fn ref_with_other_attrs_preserved() {
    let result = gen_tsx_template(
        r#"<template><input ref="inputRef" type="text" class="field" /></template>"#,
    );
    assert!(
        result.contains(r#"ref={"inputRef"}"#),
        "ref should be converted, got: {}",
        result
    );
    assert!(
        result.contains(r#"type="text""#),
        "type attribute should be preserved, got: {}",
        result
    );
    assert!(
        result.contains(r#"class="field""#),
        "class attribute should be preserved, got: {}",
        result
    );
}

/// MULTIPLE `v-model` on one COMPONENT — each generated prop NAME must map
/// INDEPENDENTLY back to its OWN source arg span. The `v_model_hover` loop walks
/// every directive and the per-directive mapped prop-name piece must anchor at
/// the matching arg, so `v-model:a` → source `a` and `v-model:b` → source `b`
/// (never both onto one arg, never a cross-wired mapping).
#[test]
fn vmodel_multiple_named_args_map_independently() {
    let source = r#"<template><Comp v-model:a="x" v-model:b="y" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("x", BindingType::SetupConst),
            ("y", BindingType::SetupConst),
        ],
    );

    assert!(
        output.contains("a={") && output.contains("b={"),
        "both named v-models should emit `a={{...}}` and `b={{...}}`: {output}"
    );

    // Source cols of each arg (`v-model:a` / `v-model:b` → the char after the colon).
    let a_src_col = source.find("v-model:a").unwrap() as u32 + "v-model:".len() as u32;
    let b_src_col = source.find("v-model:b").unwrap() as u32 + "v-model:".len() as u32;
    assert_ne!(
        a_src_col, b_src_col,
        "the two args must be distinct source spans"
    );

    // Generated prop-name positions: the `a={` / `b={` prop-name tokens.
    let a_gen = output.find("a={").unwrap();
    let b_gen = output.find("b={").unwrap();
    let (a_gl, a_gc) = gen_offset_to_line_col(&output, a_gen);
    let (b_gl, b_gc) = gen_offset_to_line_col(&output, b_gen);

    // `a` prop name maps to the `a` arg span (and ONLY that one).
    assert!(
        tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == a_gl && dc == a_gc && sc == a_src_col),
        "generated `a` prop-name (gen {a_gl}:{a_gc}) must map to source arg col {a_src_col}. \
         Tokens: {tokens:?}, output: {output}"
    );
    // `b` prop name maps to the `b` arg span (and ONLY that one).
    assert!(
        tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == b_gl && dc == b_gc && sc == b_src_col),
        "generated `b` prop-name (gen {b_gl}:{b_gc}) must map to source arg col {b_src_col}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // Each arg span is mapped EXACTLY ONCE (no cross-wiring onto the other prop name
    // and no duplicate onto an onUpdate/modifier key).
    let a_mappings = tokens.iter().filter(|&&(_, _, sc)| sc == a_src_col).count();
    let b_mappings = tokens.iter().filter(|&&(_, _, sc)| sc == b_src_col).count();
    assert_eq!(
        a_mappings, 1,
        "arg `a` (col {a_src_col}) must be mapped exactly once. Tokens: {tokens:?}"
    );
    assert_eq!(
        b_mappings, 1,
        "arg `b` (col {b_src_col}) must be mapped exactly once. Tokens: {tokens:?}"
    );
}

#[test]
fn multiline_static_style_merged_with_dynamic_no_unterminated_string() {
    // When static style has newlines and is merged with :style, the static value
    // must not produce an unterminated JS string literal inside normalizeStyle.
    let result = gen_tsx_template_with_bindings(
        "<template><div style=\"\n  position: absolute;\n  top: 0;\n\" :style=\"{ height: h + 'px' }\">hi</div></template>",
        &[("h", BindingType::SetupConst)],
    );
    // Positive: should have normalizeStyle call
    assert!(
        result.contains("normalizeStyle"),
        "merged style should use normalizeStyle. Got: {}",
        result
    );
    // Negative: the static string inside normalizeStyle must NOT have literal newlines
    // (which would be unterminated string literal TS1002)
    let norm_idx = result.find("normalizeStyle").unwrap();
    let after_norm = &result[norm_idx..];
    // Find the string literal inside the normalizeStyle call
    if let Some(quote_idx) = after_norm.find(",\"") {
        let after_quote = &after_norm[quote_idx + 2..];
        let end_quote = after_quote.find('"').unwrap_or(after_quote.len());
        let static_str = &after_quote[..end_quote];
        assert!(
            !static_str.contains('\n'),
            "static style string must not contain newlines. Got: {}",
            static_str
        );
    }
}

#[test]
fn tsx_unresolved_dollar_attrs_gets_instance_prefix() {
    let result =
        gen_tsx_template_with_bindings(r#"<template><div>{{ $attrs }}</div></template>"#, &[]);
    assert!(
        result.contains("___VERTER___instance.$attrs"),
        "Unresolved $attrs should get instance prefix. Got: {}",
        result
    );
}

// ── Bug 2: Class/Style merge ──

#[test]
fn class_merge_static_and_dynamic() {
    let source = r#"<template><div class="foo" :class="{bar: true}"/></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass: {output}"
    );
    assert!(
        output.contains("{bar: true}") && output.contains("\"foo\""),
        "should contain both class expressions: {output}"
    );
    // Count class= occurrences — should be exactly 1
    let class_count = output.matches("class=").count();
    assert_eq!(
        class_count, 1,
        "should have exactly 1 class= attribute, got {class_count}: {output}"
    );
}

#[test]
fn style_merge_static_and_dynamic() {
    let source = r#"<template><div style="color:red" :style="{ bg: 'blue' }"/></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("normalizeStyle"),
        "should use normalizeStyle: {output}"
    );
    let style_count = output.matches("style=").count();
    assert_eq!(
        style_count, 1,
        "should have exactly 1 style= attribute, got {style_count}: {output}"
    );
}

#[test]
fn class_and_style_merge_combined() {
    let source = r#"<template><div class="a" :class="b" style="c" :style="d"/></template>"#;
    let output = gen_tsx_template_with_bindings(
        source,
        &[("b", BindingType::SetupRef), ("d", BindingType::SetupRef)],
    );

    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass: {output}"
    );
    assert!(
        output.contains("normalizeStyle"),
        "should use normalizeStyle: {output}"
    );
    let class_count = output.matches("class=").count();
    assert_eq!(
        class_count, 1,
        "should have exactly 1 class= attribute: {output}"
    );
    let style_count = output.matches("style=").count();
    assert_eq!(
        style_count, 1,
        "should have exactly 1 style= attribute: {output}"
    );
}

#[test]
fn class_only_static_no_merge() {
    let source = r#"<template><div class="foo"/></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("class=\"foo\""),
        "static class should be unchanged: {output}"
    );
    assert!(
        !output.contains("normalizeClass"),
        "should not use normalizeClass for static-only: {output}"
    );
}

#[test]
fn class_only_dynamic_no_merge() {
    let source = r#"<template><div :class="{bar: true}"/></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("class={{bar: true}}"),
        "dynamic-only class should be simple binding: {output}"
    );
    assert!(
        !output.contains("normalizeClass"),
        "should not use normalizeClass for dynamic-only: {output}"
    );
}

#[test]
fn class_merge_no_extra_closing_brace() {
    // Bug: `<span :class="$attrs.class" class="ns-popover--wrapper">` generated `])}}`
    // (double closing brace) instead of `])}`
    let source =
        r#"<template><span :class="$attrs.class" class="ns-popover--wrapper">hi</span></template>"#;
    let output = gen_tsx_template(source);

    // Positive: should contain merged normalizeClass with static value
    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass for merged class: {output}"
    );
    assert!(
        output.contains("\"ns-popover--wrapper\""),
        "should contain static class value: {output}"
    );

    // Negative: must NOT have double closing brace `])}}` — only `])}`
    let double_brace = "])}}";
    assert!(
        !output.contains(double_brace),
        "must not have extra closing brace: {output}"
    );
    // Positive: should have exactly `])}`
    let single_brace = "])}";
    assert!(
        output.contains(single_brace),
        "should have correct single closing brace: {output}"
    );
}

#[test]
fn class_merge_static_before_dynamic_no_extra_brace() {
    // Popover.vue pattern: static `class` BEFORE dynamic `:class`
    let source =
        r#"<template><span class="ns-popover--wrapper" :class="$attrs.class">hi</span></template>"#;
    let output = gen_tsx_template(source);

    eprintln!("=== OUTPUT ===\n{}\n=== END ===", output);

    // Positive: should contain normalizeClass
    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass: {output}"
    );

    // Negative: must NOT have double closing brace
    let double_brace = "])}}";
    assert!(
        !output.contains(double_brace),
        "must not have extra closing brace: {output}"
    );
}

#[test]
fn class_merge_dynamic_before_static_no_extra_brace() {
    // Original Bug 2 pattern: dynamic `:class` BEFORE static `class`
    let source =
        r#"<template><span :class="$attrs.class" class="ns-popover--wrapper">hi</span></template>"#;
    let output = gen_tsx_template(source);

    eprintln!("=== OUTPUT ===\n{}\n=== END ===", output);

    // Positive: should contain normalizeClass
    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass: {output}"
    );

    // Negative: must NOT have double closing brace
    let double_brace = "])}}";
    assert!(
        !output.contains(double_brace),
        "must not have extra closing brace: {output}"
    );
}

#[test]
fn popover_vue_template_generates_valid_tsx() {
    // Full Popover.vue template pattern that user reports as broken
    let source = r#"<script setup lang="ts">
import { computed, ref, useTemplateRef, watch } from 'vue'
const show = ref(false)
const onClickWrapper = () => {}
const floatingStyles = ref({})
const showArrow = ref(false)
const arrowPos = ref({})
</script>
<template>
  <span
    ref="wrapperElm"
    class="ns-popover--wrapper"
    :class="$attrs.class"
    :style="$attrs.style as any"
    @click="onClickWrapper"
  >
    <slot name="reference" />
  </span>
  <Popup
    ref="popupElm"
    v-model:show="show"
    class="ns-popover"
    :style="[floatingStyles, $attrs.style]"
    position=""
  >
    <div v-if="showArrow" ref="arrowElm" class="ns-popover__arrow" :style="[arrowPos]"></div>
    <div
      role="menu"
      class="ns-popover__content"
      :class="{
        'ns-popover__content--horizontal': true,
      }"
    >
      <slot />
    </div>
  </Popup>
</template>"#;
    let output = gen_tsx_template(source);

    eprintln!("=== POPOVER OUTPUT ===\n{}\n=== END ===", output);

    // Must not have any double closing braces from normalizeClass
    let double_brace = "])}}";
    assert!(
        !output.contains(double_brace),
        "must not have extra closing brace: {output}"
    );

    // normalizeClass should be present for merged class attrs
    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass for merged class: {output}"
    );

    // v-if should NOT appear in JSX
    assert!(
        !output.contains("v-if"),
        "v-if attribute must be removed from JSX: {output}"
    );
}

#[test]
fn balcard_vue_full_sfc_produces_valid_tsx() {
    let Some(source) = read_external_corpus_vue(
        "VERTER_TEST_REPOS_ROOT",
        "balancer-frontend-v2/src/components/_global/BalCard/BalCard.vue",
    ) else {
        return;
    };
    let alloc = Allocator::new();
    let options = crate::compile::legacy_test_support::CodegenOptions {
        filename: Some("BalCard.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::legacy_test_support::VerterCompileOptions {
        source_map: true,
        ..Default::default()
    };
    let result = crate::compile::legacy_test_support::compile(
        &source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== BALCARD FULL TSX ===\n{}\n=== END ===", tsx.code);

    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx())
            .parse();
    for err in &parsed.diagnostics {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.diagnostics.is_empty(),
        "BalCard TSX should have no parse errors. Got {} errors",
        parsed.diagnostics.len(),
    );
}

#[test]
fn custom_docs_block_before_template_produces_valid_tsx() {
    let source = r#"<docs>
---
order: 0
title:
  zh-CN: 基本用法
---
## Notes
</docs>

<template>
  <div>hello</div>
</template>
<script lang="ts" setup>
import { ref } from 'vue';
const checked = ref<boolean>(false);
</script>"#;
    let alloc = Allocator::new();
    let options = crate::compile::legacy_test_support::CodegenOptions {
        filename: Some("Basic.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::legacy_test_support::VerterCompileOptions::default();
    let result = crate::compile::legacy_test_support::compile(
        source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== DOCS BLOCK TSX ===\n{}\n=== END ===", tsx.code);

    // Custom block content should not appear in TSX
    assert!(
        !tsx.code.contains("order: 0"),
        "Custom block content should not leak into TSX"
    );

    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx())
            .parse();
    for err in &parsed.diagnostics {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.diagnostics.is_empty(),
        "TSX with custom block should have no parse errors. Got {} errors.\nOutput:\n{}",
        parsed.diagnostics.len(),
        tsx.code
    );
}

#[test]
fn ant_design_switch_basic_produces_valid_tsx() {
    let Some(source) = read_external_corpus_vue(
        "VERTER_TEST_REPOS_ROOT",
        "ant-design-vue/components/switch/demo/basic.vue",
    ) else {
        return;
    };
    let alloc = Allocator::new();
    let options = crate::compile::legacy_test_support::CodegenOptions {
        filename: Some("basic.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::legacy_test_support::VerterCompileOptions::default();
    let result = crate::compile::legacy_test_support::compile(
        &source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== ANT BASIC TSX ===\n{}\n=== END ===", tsx.code);

    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx())
            .parse();
    for err in &parsed.diagnostics {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.diagnostics.is_empty(),
        "Got {} errors",
        parsed.diagnostics.len(),
    );
}

#[test]
fn activist_machine_steps_produces_valid_tsx() {
    let Some(source) = read_external_corpus_vue(
        "VERTER_TEST_REPOS_ROOT",
        "activist-org-activist/frontend/app/components/MachineStepsCreateEventTime.vue",
    ) else {
        return;
    };
    let alloc = Allocator::new();
    let options = crate::compile::legacy_test_support::CodegenOptions {
        filename: Some("MachineStepsCreateEventTime.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::legacy_test_support::VerterCompileOptions::default();
    let result = crate::compile::legacy_test_support::compile(
        &source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== MACHINE STEPS TSX ===\n{}\n=== END ===", tsx.code);
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx())
            .parse();
    for err in &parsed.diagnostics {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.diagnostics.is_empty(),
        "Got {} errors",
        parsed.diagnostics.len()
    );
}

#[test]
fn ts_ignore_same_behavior() {
    let result = gen_tsx_template(r#"<template><!-- @ts-ignore --><MyComp/></template>"#);
    assert!(
        result.contains("{/* @ts-ignore */}"),
        "should have @ts-ignore comment, got:\n{}",
        result
    );
    let comment_pos = result.find("{/* @ts-ignore */}").unwrap();
    let comp_pos = result.find("<MyComp").unwrap();
    assert!(
        comment_pos < comp_pos,
        "@ts-ignore should appear before component, got:\n{}",
        result
    );
    assert!(
        !result.contains("<!--"),
        "no raw HTML markers, got:\n{}",
        result
    );
}

#[test]
fn dynamic_key_bind_both_identifiers_map() {
    // <div :[key]="val"/> → {...{[key]: val}}. Both `key` and `val` map back;
    // `{...{[`, `]: `, `}}}` map to None.
    let source = r#"<template><div :[key]="val"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("key", BindingType::SetupConst),
            ("val", BindingType::SetupConst),
        ],
    );

    assert!(
        output.contains("{...{[key]: val}}"),
        ":[key]=\"val\" should emit {{...{{[key]: val}}}}: {output}"
    );
    // Exact closing — no extra brace (regression: the spread+object closes with
    // exactly `}}`, not `}}}`).
    assert!(
        !output.contains("{...{[key]: val}}}"),
        ":[key] emission must close with exactly `}}}}` (no extra brace): {output}"
    );

    // Positive: both identifiers map back.
    let key_src = source.find("[key]").unwrap() as u32 + 1; // inside the [ ]
    let val_src = source.find("\"val\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, key_src),
        "key must map to source col {key_src}. Tokens: {tokens:?}"
    );
    assert!(
        has_token_for_src(&tokens, val_src),
        "val must map to source col {val_src}. Tokens: {tokens:?}"
    );

    // Negative: the `{...{[` boundary start maps to None.
    let boundary_gen = output.find("{...{[").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, boundary_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "{{...{{[ start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}"
    );
    // The `]: ` separator between key and val maps to None.
    let sep_gen = output.find("]: ").unwrap();
    let (sl, sc) = gen_offset_to_line_col(&output, sep_gen);
    assert!(
        !has_token_at_gen(&tokens, sl, sc),
        "]: separator (gen {sl}:{sc}) must map to None. Tokens: {tokens:?}"
    );
}

#[test]
fn native_vmodel_every_occurrence_maps_back() {
    // <input v-model="count"/> on a native element emits `count` 2-3 times:
    //   value={count} onInput={($event:any) => ((count) = $event)}
    // Every generated occurrence of `count` must map back to the single source
    // span; the assignment punctuation (=>, ($event, =) maps to None.
    let source = r#"<template><input v-model="count"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("count", BindingType::SetupRef)]);

    assert!(
        output.contains("value={"),
        "native v-model should emit value={{...}}: {output}"
    );
    assert!(
        output.contains("onInput={"),
        "native v-model should emit onInput handler: {output}"
    );

    let count_src = source.find("\"count\"").unwrap() as u32 + 1;

    // SetupRef bindings are emitted bare (no prefix) but with a `.value` suffix
    // appended; the identifier text `count` therefore appears at each occurrence.
    // Enumerate ALL generated occurrences of the identifier `count` and assert
    // each one is covered by a token mapping back to the source span.
    let mut occurrence_starts = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = output[search_from..].find("count") {
        let at = search_from + rel;
        occurrence_starts.push(at);
        search_from = at + "count".len();
    }
    assert!(
        occurrence_starts.len() >= 2,
        "expected >=2 generated `count` occurrences (read + write), found {}: {output}",
        occurrence_starts.len()
    );

    for at in &occurrence_starts {
        let (gl, gc) = gen_offset_to_line_col(&output, *at);
        let covered = tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == gl && dc == gc && sc == count_src);
        assert!(
            covered,
            "generated `count` occurrence at gen {gl}:{gc} must map back to source col {count_src}. Tokens: {tokens:?}, output: {output}"
        );
    }

    // Negative: the arrow `=>` of the handler maps to None.
    let arrow_gen = output.find("=>").unwrap();
    let (al, ac) = gen_offset_to_line_col(&output, arrow_gen);
    assert!(
        !has_token_at_gen(&tokens, al, ac),
        "arrow => (gen {al}:{ac}) must map to None. Tokens: {tokens:?}"
    );
    // The `($event` parameter list maps to None.
    let ev_gen = output.find("($event").unwrap();
    let (el, ec) = gen_offset_to_line_col(&output, ev_gen);
    assert!(
        !has_token_at_gen(&tokens, el, ec),
        "($event param (gen {el}:{ec}) must map to None. Tokens: {tokens:?}"
    );
}

#[test]
fn vmodel_source_to_generated_selects_read_occurrence() {
    // P2-A: one source span → multiple generated occurrences. The FIRST covering
    // mapped run in generated byte order is the value-binding (read) occurrence,
    // emitted before the assignment LHS. Discriminating: an LHS-first or
    // non-deterministic selection picks the occurrence inside `((count) = $event)`.
    let source = r#"<template><input v-model="count"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("count", BindingType::SetupRef)]);

    let count_src = source.find("\"count\"").unwrap() as u32 + 1;

    // The value-binding occurrence is the one inside `value={...count...}`.
    let value_eq = output.find("value={").expect("value={ in output");
    let assign_lhs = output.find("((").expect("(( assignment LHS in output");
    assert!(
        value_eq < assign_lhs,
        "value binding must be emitted before the assignment LHS: {output}"
    );

    // Collect all tokens that map to count_src, sorted by generated position.
    let mut covering: Vec<(u32, u32)> = tokens
        .iter()
        .filter(|&&(_, _, sc)| sc == count_src)
        .map(|&(dl, dc, _)| (dl, dc))
        .collect();
    covering.sort_unstable();
    assert!(
        !covering.is_empty(),
        "count must have at least one mapped token. Tokens: {tokens:?}"
    );

    // The first covering run (deterministic strict first-covering lookup in
    // generated order) must fall within the value-binding occurrence, NOT the
    // assignment LHS inside `((count) = $event)`.
    let (fl, fc) = covering[0];
    let first_byte = {
        // recover byte offset of (fl, fc) — single fixture, find nth line break
        let mut idx = 0usize;
        let mut line = 0u32;
        let mut col = 0u32;
        for (i, ch) in output.char_indices() {
            if line == fl && col == fc {
                idx = i;
                break;
            }
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += ch.len_utf16() as u32;
            }
            idx = i + ch.len_utf8();
        }
        idx
    };
    assert!(
        first_byte >= value_eq && first_byte < assign_lhs,
        "first covering run (gen {fl}:{fc}, byte {first_byte}) must be the value-binding occurrence \
         in [{value_eq}, {assign_lhs}), not the assignment LHS. Output: {output}"
    );
}

#[test]
fn vmodel_modifier_maps_to_source() {
    // <MyComp v-model.trim="x"/> → the `trim` modifier token maps to its source span.
    // The host is a COMPONENT because `modelModifiers` is a component prop; a native
    // element publishes no modifiers prop at all (see
    // `intrinsic_vmodel_does_not_emit_model_modifiers_prop`).
    let source = r#"<template><MyComp v-model.trim="x"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("x", BindingType::SetupRef),
            ("MyComp", BindingType::SetupConst),
        ],
    );

    assert!(
        output.contains("Modifiers={{"),
        "v-model.trim should emit a modifiers prop: {output}"
    );
    assert!(
        output.contains("trim"),
        "modifiers prop should contain `trim`: {output}"
    );

    let trim_src = source.find(".trim").unwrap() as u32 + 1; // the `trim` after the dot
    assert!(
        has_token_for_src(&tokens, trim_src),
        "modifier `trim` must map to source col {trim_src}. Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn vmodel_prefix_not_double_shifted() {
    // P1-B: with a Data binding the identifier is prefixed by `___VERTER___instance.`.
    // The identifier token must map to the FIRST byte of the identifier in
    // generated output (the byte right after the prefix), not shifted into the
    // prefix and not leaving the identifier interior unmapped.
    let source = r#"<template><MyComp v-model="d_val"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("d_val", BindingType::Data)]);

    let needle = "___VERTER___instance.d_val";
    assert!(
        output.contains(needle),
        "Data v-model should emit the instance prefix: {output}"
    );

    let src_col = source.find("\"d_val\"").unwrap() as u32 + 1;

    // The generated `d_val` (after the FIRST prefix) must carry a token that maps
    // to the source identifier, anchored exactly at the identifier start.
    let prefix_pos = output.find(needle).unwrap();
    let ident_gen = prefix_pos + "___VERTER___instance.".len();
    let (il, ic) = gen_offset_to_line_col(&output, ident_gen);
    let anchored = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == il && dc == ic && sc == src_col);
    assert!(
        anchored,
        "d_val must map to source col {src_col} anchored at the identifier start (gen {il}:{ic}), \
         no double shift. Tokens: {tokens:?}, output: {output}"
    );

    // Negative: the prefix start must NOT carry the identifier's mapping.
    let (pl, pc) = gen_offset_to_line_col(&output, prefix_pos);
    let prefix_carries = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == pl && dc == pc && sc == src_col);
    assert!(
        !prefix_carries,
        "the ___VERTER___instance. prefix start (gen {pl}:{pc}) must NOT carry d_val's mapping. \
         Tokens: {tokens:?}"
    );
}

#[test]
fn synthetic_boundary_start_maps_to_none() {
    // P1-C: the generated column at the start of an OverwriteSyntheticBoundary
    // (`innerHTML=` for v-html) maps to None. Discriminating: a Chunk::Overwritten
    // lowering would map that column back to the prop start.
    let source = r#"<template><div v-html="msg"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("msg", BindingType::SetupConst)]);

    let boundary_gen = output.find("innerHTML=").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, boundary_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "innerHTML= boundary start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}, output: {output}"
    );

    // And specifically: no token at that generated column maps to the prop start
    // (the old Chunk::Overwritten bug).
    let prop_start = source.find("v-html").unwrap() as u32;
    let maps_to_prop_start = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == bl && dc == bc && sc == prop_start);
    assert!(
        !maps_to_prop_start,
        "innerHTML= start must NOT map to the prop start (col {prop_start}) — the desync bug. \
         Tokens: {tokens:?}"
    );
}

#[test]
fn vmodel_dynamic_arg_modifier_maps_and_is_valid() {
    // <MyComp v-model:[eventName].trim="val"/> — dynamic arg + modifier.
    // The modifiers prop name must be the COMPUTED `[`${...}Modifiers`]` name with
    // the arg expression embedded, NOT an empty JSX attribute name (` ={{`), which
    // is invalid TSX. The embedded arg `eventName` must map back to its source span.
    // The host is a COMPONENT: an argument-bearing v-model is a component-only Vue
    // form, and `modelModifiers` is a component prop.
    let source = r#"<template><MyComp v-model:[eventName].trim="val"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("eventName", BindingType::SetupConst),
            ("val", BindingType::SetupRef),
            ("MyComp", BindingType::SetupConst),
        ],
    );

    // Positive: a computed `[`${...}Modifiers`]` prop name is present.
    assert!(
        output.contains("Modifiers`]"),
        "dynamic-arg v-model with a modifier must emit a computed `[`${{...}}Modifiers`]` \
         prop name: {output}"
    );
    // Negative: the empty-attribute-name shape ` ={{` (the regression) must NOT appear.
    assert!(
        !output.contains(" ={{"),
        "dynamic-arg v-model must NOT emit an empty JSX attribute name ` ={{` (invalid TSX). \
         The computed modifiers name was dropped: {output}"
    );

    // The arg identifier `eventName` must map back to its source span. The arg
    // appears multiple times (computed prop name, event key, modifiers name); at
    // least one occurrence maps back.
    let arg_src = source.find("[eventName]").unwrap() as u32 + 1; // inside the [ ]
    assert!(
        has_token_for_src(&tokens, arg_src),
        "v-model dynamic arg `eventName` must map to source col {arg_src}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // The whole emission must be valid TSX (no empty attribute name, balanced
    // braces). Wrap as a JSX element attribute list and parse.
    let wrapper = format!("const x = <input {} />", output_attrs(&output));
    let val_alloc = oxc_allocator::Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&val_alloc, &wrapper, oxc_span::SourceType::tsx())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "dynamic-arg v-model + modifier must produce valid TSX. Errors: {:?}\n--- output ---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        output
    );
}

/// Q3 — dynamic `:ref="expr"` → `ref={expr}` IN PLACE. The parser routes a dynamic
/// `:ref` through `el.props` → `process_v_bind` (static-key path), which preserves
/// the value expression in place: the VALUE identifier `myRef` maps to its source
/// span, and (the desync check) it must NOT collapse to the foreign prop start.
/// The `ref` JSX attribute NAME is the preserved `ref` arg token and legitimately
/// maps to source — that is NOT the desync (the desync was a baked VALUE).
#[test]
fn ref_expr_value_maps_to_source() {
    let source = r#"<template><div :ref="myRef"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("myRef", BindingType::SetupRef)]);

    assert!(
        output.contains("ref={myRef}"),
        ":ref should emit ref={{myRef}} in place: {output}"
    );
    assert!(
        !output.contains(":ref"),
        ":ref directive must be removed: {output}"
    );

    // Positive: the VALUE identifier `myRef` maps to its source byte offset.
    let myref_src = source.find("\"myRef\"").unwrap() as u32 + 1;
    let value_gen_col = output.find("ref={myRef}").unwrap() as u32 + "ref={".len() as u32;
    let value_maps_to_source = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == value_gen_col && sc == myref_src);
    assert!(
        value_maps_to_source,
        ":ref VALUE `myRef` (gen col {value_gen_col}) must map to source col {myref_src}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // Negative (the desync): the value identifier must NOT collapse to the `:` prop
    // start (a baked `out.overwrite(prop.start, .., &format!(\"ref={{{}}}\", value))`
    // would map the value back to the prop start).
    let prop_start = source.find(":ref").unwrap() as u32;
    let value_maps_to_prop_start = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == value_gen_col && sc == prop_start);
    assert!(
        !value_maps_to_prop_start,
        ":ref VALUE must not map to the prop start (col {prop_start}). Tokens: {tokens:?}, output: {output}"
    );
}

/// `emit_synthesized_shorthand_value`'s no-core fallback. When the
/// derived value `core` is NOT a substring of the resolver output `resolved` (the
/// resolver rewrote the expression so the core token is absent), the value is not
/// precisely mappable. Pre-fix the fallback mapped the ENTIRE synthetic `resolved`
/// string to the user source token — violating "synthetic text maps to None". Per
/// the prove-or-drop principle a feature drop (None mapping) is acceptable, a mismap
/// is not. Post-fix the no-core fallback emits the synthetic text UNMAPPED.
#[test]
fn synthesized_core_not_found_falls_back_unmapped() {
    use super::super::emit::emit_synthesized_shorthand_value;
    use verter_span::SourceByteOffset;

    let alloc = Allocator::new();
    // The CodeTransform source is a single original char `x` so the inserted synthetic
    // text is the only thing that could carry a (wrong) mapping.
    let mut ct = CodeTransform::new("x", &alloc);
    let mut out = CodeGenOutput::new(&alloc);

    // `resolved` = `$setup.bar` (a resolver rewrite), `core` = `zzz` is NOT a
    // substring of it → the no-core fallback path. `core_source_start` points at the
    // user token (offset 0). Pre-fix the WHOLE `$setup.bar` mapped to source col 0.
    emit_synthesized_shorthand_value(
        &mut out,
        SourceByteOffset(0),
        "$setup.bar",
        "zzz",
        SourceByteOffset(0),
    );
    out.apply_to(&mut ct);

    let built = ct.build_string();
    // The synthetic text is still emitted verbatim (semantics preserved).
    assert!(
        built.starts_with("$setup.bar"),
        "the synthetic value text must still be emitted: {built:?}"
    );

    let map = ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("t.vue"));
    let tokens: Vec<(u32, u32, u32)> = map
        .get_tokens()
        .filter(|t| t.get_source_id().is_some())
        .map(|t| (t.get_dst_line(), t.get_dst_col(), t.get_src_col()))
        .collect();

    // The inserted synthetic `$setup.bar` starts at generated (0, 0). It must map to
    // None — no token may anchor the synthetic insert to the user source token.
    assert!(
        !has_token_at_gen(&tokens, 0, 0),
        "the no-core synthetic fallback `$setup.bar` (gen 0:0) must map to None, not to the \
         user source token. Tokens: {tokens:?}, built: {built:?}"
    );
    // Discriminating: NO token at all may map back to source col 0 from the synthetic
    // insert (the pre-fix bug mapped the whole run to src col 0).
    assert!(
        !tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == 0 && dc == 0 && sc == 0),
        "the no-core fallback must not map the synthetic string to source col 0. \
         Tokens: {tokens:?}, built: {built:?}"
    );
}
