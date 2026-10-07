use super::*;

#[test]
fn isolated_empty_pair_preserves_both_component_tag_mappings() {
    let source = r#"<template><Panel><span>{{ templateOnly }}</span></Panel></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("Panel", BindingType::SetupConst),
            ("templateOnly", BindingType::SetupConst),
        ],
    );
    let facts = jsx_element_body_facts(&output);
    assert!(
        facts.non_empty_elements.is_empty(),
        "mapped empty-pair carrier must also isolate JSX children: {facts:?}\n{output}"
    );

    assert_eq!(
        facts.panel_open_name_offsets.len(),
        1,
        "the typed empty pair must retain one opening Panel tag: {facts:?}\n{output}"
    );
    assert_eq!(
        facts.panel_close_name_offsets.len(),
        1,
        "the typed empty pair must retain one closing Panel tag: {facts:?}\n{output}"
    );

    let source_open = source.find("Panel").unwrap() as u32;
    let source_close = source.rfind("Panel").unwrap() as u32;
    assert!(
        tokens.iter().any(|&(line, generated, original)| line == 0
            && generated == facts.panel_open_name_offsets[0]
            && original == source_open),
        "opening Panel must map to the authored opening name; tokens={tokens:?}\n{output}"
    );
    assert!(
        tokens
            .iter()
                .any(|&(line, generated, original)| line == 0
                && generated == facts.panel_close_name_offsets[0]
                && original == source_close),
        "generated empty-pair closing Panel must map to the authored closing name; tokens={tokens:?}\n{output}"
    );
}

/// @ai-generated - Guards Vue component prop normalization without weakening native JSX attrs.
///
/// Mutation recipe: at the component-only normalization call site, replace the normalized
/// name with the authored name. This test must fail while `basic_div` remains green; restore
/// the call site, verify a clean worktree, then rerun both tests green.
#[test]
fn component_kebab_props_are_camelized_but_native_attributes_are_not() {
    let source = r#"<template><DirectChild contract-prop="literal" :optional-flag="enabled"/><div aria-label="label" :data-test="enabled"/></template>"#;
    let output = gen_tsx_template_with_bindings(
        source,
        &[
            ("DirectChild", BindingType::SetupConst),
            ("enabled", BindingType::SetupConst),
        ],
    );

    assert_eq!(
        jsx_attributes_for_element(&output, "DirectChild")
            .into_iter()
            .map(|attribute| attribute.name)
            .collect::<Vec<_>>(),
        vec!["contractProp", "optionalFlag"],
        "Vue component props must use the public camel-case JSX contract: {output}"
    );
    assert_eq!(
        jsx_attributes_for_element(&output, "div")
            .into_iter()
            .map(|attribute| attribute.name)
            .collect::<Vec<_>>(),
        vec!["aria-label", "data-test"],
        "native DOM attributes retain their authored JSX spelling: {output}"
    );

    let mapped_source = r#"<template><DirectChild :contract-prop="enabled"/></template>"#;
    let (mapped_output, tokens) = gen_tsx_template_with_map(
        mapped_source,
        &[
            ("DirectChild", BindingType::SetupConst),
            ("enabled", BindingType::SetupConst),
        ],
    );
    let generated_prop = jsx_attributes_for_element(&mapped_output, "DirectChild")
        .into_iter()
        .find(|attribute| attribute.name == "contractProp")
        .expect("normalized component prop must remain a parsed JSX attribute");
    let authored_prop = mapped_source
        .find("contract-prop")
        .expect("fixture contains authored component prop") as u32;
    assert!(
        tokens.iter().any(|&(line, generated, original)| {
            line == 0 && generated == generated_prop.start && original == authored_prop
        }),
        "normalized component prop must map to the authored kebab-name start; attr={generated_prop:?}, tokens={tokens:?}, output={mapped_output}"
    );
}

/// `v-model:show` on a COMPONENT — the generated `show=` prop NAME token must map
/// back to the source `show` arg span so a TypeProvider can resolve the child
/// component's `$props['show']` and hover lands on the directive arg. Pre-change
/// the static-arg prop name was emitted as unmapped synthetic text
/// (`Piece::Syn("show={")`) and the whole `v-model:show="x"` span was overwritten,
/// so the arg token had ZERO source→TSX mapping. Baseline against the working
/// `:show` bind name mapping (`v_bind_shorthand_title_source_map_accuracy`).
#[test]
fn vmodel_named_component_prop_name_maps_to_arg() {
    let source = r#"<template><Comp v-model:show="x" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("x", BindingType::SetupConst)]);

    assert!(
        output.contains("show={"),
        "named v-model should emit `show={{...}}`: {output}"
    );

    // Source col of the `show` arg token (`v-model:show` → after the colon).
    let arg_src_col = source.find("v-model:show").unwrap() as u32 + "v-model:".len() as u32;
    // The generated prop NAME `show=` — locate the prop-name occurrence (the one
    // immediately followed by `={`).
    let name_gen_col = output.find("show={").unwrap() as u32;
    let (name_gl, name_gc) = gen_offset_to_line_col(&output, name_gen_col as usize);

    let has_correct = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == name_gl && dc == name_gc && sc == arg_src_col);
    assert!(
        has_correct,
        "generated `show` prop-name (gen {name_gl}:{name_gc}) must map back to the \
         source arg col {arg_src_col} (the `s` in `show`). Tokens: {tokens:?}, output: {output}"
    );
}

/// `v-model:my-arg` on a COMPONENT — the generated prop name is the camelCased
/// `myArg` (≠ the source `my-arg`), so preserve-in-place is impossible. The mapped
/// piece lets the generated token be `myArg` while owning the source `arg_span`.
/// The mapped token (at the prop-name start) must point back into the `my-arg`
/// source token. Pre-change there was no mapping at all.
#[test]
fn vmodel_named_component_kebab_prop_name_maps_to_arg() {
    let source = r#"<template><Comp v-model:my-arg="x" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("x", BindingType::SetupConst)]);

    assert!(
        output.contains("myArg={"),
        "named v-model:my-arg should camelCase to `myArg={{...}}`: {output}"
    );

    let arg_src_col = source.find("v-model:my-arg").unwrap() as u32 + "v-model:".len() as u32;
    let name_gen_col = output.find("myArg={").unwrap() as u32;
    let (name_gl, name_gc) = gen_offset_to_line_col(&output, name_gen_col as usize);

    // The mapped prop-name token starts at the generated `myArg` and points back
    // into the `my-arg` source token (InsertMapped is linear-run, so it anchors at
    // the arg start; char-perfect kebab→camel is not required — the whole source
    // token is covered by the source-owned hover).
    let has_correct = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == name_gl && dc == name_gc && sc == arg_src_col);
    assert!(
        has_correct,
        "generated `myArg` prop-name (gen {name_gl}:{name_gc}) must map back to the \
         source arg col {arg_src_col} (the `m` in `my-arg`). Tokens: {tokens:?}, output: {output}"
    );
}

/// REGRESSION: the bound VALUE (`x`) and the `onUpdate:show` handler emissions
/// must be UNCHANGED by the mapped prop-name piece. The value still maps back to
/// its source span, and the synthetic `onUpdate:show` event key must NOT carry a
/// duplicate mapping to the arg span (which would make hover land on the event
/// key instead of the prop).
#[test]
fn vmodel_named_component_value_and_onupdate_unchanged() {
    let source = r#"<template><Comp v-model:show="x" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("x", BindingType::SetupConst)]);

    // The bound value `x` still maps back to source.
    let value_src_col = source.find("\"x\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, value_src_col),
        "bound value `x` must still map back to source col {value_src_col}. Tokens: {tokens:?}"
    );

    // The onUpdate handler key is still synthetic/unmapped.
    assert!(
        output.contains("\"onUpdate:show\""),
        "named v-model should still produce onUpdate:show handler: {output}"
    );
    let onupdate_gen = output.find("onUpdate:show").unwrap();
    let (ul, uc) = gen_offset_to_line_col(&output, onupdate_gen);
    assert!(
        !has_token_at_gen(&tokens, ul, uc),
        "onUpdate:show event key (gen {ul}:{uc}) must NOT carry a mapping (no duplicate \
         arg provenance). Tokens: {tokens:?}"
    );

    // The arg span must be mapped EXACTLY ONCE (only the prop-name piece) — never a
    // second time onto the onUpdate key. Count distinct generated positions that map
    // to the arg source col.
    let arg_src_col = source.find("v-model:show").unwrap() as u32 + "v-model:".len() as u32;
    let arg_mappings = tokens
        .iter()
        .filter(|&&(_, _, sc)| sc == arg_src_col)
        .count();
    assert_eq!(
        arg_mappings, 1,
        "the source arg col {arg_src_col} must be mapped exactly once (the prop name), \
         not duplicated onto onUpdate/modifier keys. Tokens: {tokens:?}"
    );
}

/// REGRESSION: native `<input v-model:foo="x">` must NOT map the static arg to the
/// generated DOM prop (`value`/`checked`) — native v-model DOM props are
/// compiler-synthesized, not a `$props` surface, so mapping would be false
/// provenance. The DOM-prop name must carry NO mapping to the arg span.
#[test]
fn vmodel_native_arg_not_mapped_to_dom_prop() {
    // Native input with a NAMED arg (`v-model:foo`): the named-arg-on-native branch
    // is the actual risk the `!is_native` guard defends — a default `v-model` (no
    // arg) never had a name to map, so it passes trivially. With a named arg, the
    // arg `foo` is meaningless on a native element (the DOM prop is the synthesized
    // `value`/`checked`, NOT a `$props` surface), so `static_arg_span` stays `None`
    // and the codegen must NOT fabricate a mapping onto the synthesized DOM prop.
    let source = r#"<template><input v-model:foo="x"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("x", BindingType::SetupConst)]);

    // A native element ignores the named arg: the DOM prop is still `value`, NOT a
    // camelCased `foo` prop. The arg name must not leak into a `$props`-style prop.
    assert!(
        output.contains("value={"),
        "native v-model:foo should still emit the synthesized DOM prop value={{...}}: {output}"
    );
    assert!(
        !output.contains("foo={"),
        "native v-model:foo must NOT synthesize a `foo` component prop: {output}"
    );

    // The generated DOM prop name (`value`) must NOT carry a mapped token — it is
    // compiler-synthesized, not a source token, so the `!is_native` guard must keep
    // `static_arg_span = None` and emit no `MappedStaticModelPropName` piece.
    let value_gen = output.find("value={").unwrap();
    let (vl, vc) = gen_offset_to_line_col(&output, value_gen);
    assert!(
        !has_token_at_gen(&tokens, vl, vc),
        "native DOM prop `value` (gen {vl}:{vc}) must NOT carry a mapped token \
         (false provenance). Tokens: {tokens:?}, output: {output}"
    );

    // Strongest: the source `foo` arg span must NOT be mapped anywhere in the
    // generated TSX (no `MappedStaticModelPropName` emission for a native element).
    let arg_src_col = source.find("v-model:foo").unwrap() as u32 + "v-model:".len() as u32;
    assert!(
        !has_token_for_src(&tokens, arg_src_col),
        "native v-model:foo arg (source col {arg_src_col}) must carry NO mapped token \
         — `!is_native` keeps `static_arg_span = None`. Tokens: {tokens:?}, output: {output}"
    );
}

/// MODIFIER on a named component `v-model` — the prop NAME still maps to the arg
/// span, and the modifier name (`trim`) falls OUTSIDE the source-owned prop-name
/// hover range and emits through the UNCHANGED `Piece::Modifier` path (its own
/// mapped token in the `showModifiers={{ ... }}` prop).
#[test]
fn vmodel_named_component_with_modifier_maps_prop_name_and_modifier() {
    let source = r#"<template><Comp v-model:show.trim="x" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("x", BindingType::SetupConst)]);

    assert!(
        output.contains("show={"),
        "named v-model with modifier should still emit `show={{...}}`: {output}"
    );
    assert!(
        output.contains("showModifiers={{"),
        "the `.trim` modifier should emit a `showModifiers` prop: {output}"
    );
    assert!(
        output.contains("trim"),
        "the modifiers prop should contain `trim`: {output}"
    );

    // The prop NAME still maps to the source `show` arg span.
    let arg_src_col = source.find("v-model:show").unwrap() as u32 + "v-model:".len() as u32;
    let name_gen = output.find("show={").unwrap();
    let (name_gl, name_gc) = gen_offset_to_line_col(&output, name_gen);
    assert!(
        tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == name_gl && dc == name_gc && sc == arg_src_col),
        "generated `show` prop-name (gen {name_gl}:{name_gc}) must map to source arg col \
         {arg_src_col}. Tokens: {tokens:?}, output: {output}"
    );

    // The modifier name `trim` maps through the UNCHANGED `Piece::Modifier` path to
    // its OWN source span (the `trim` after the dot) — outside the prop-name token.
    let trim_src = source.find(".trim").unwrap() as u32 + 1;
    assert_ne!(
        trim_src, arg_src_col,
        "the modifier span must be distinct from the arg span"
    );
    assert!(
        has_token_for_src(&tokens, trim_src),
        "modifier `trim` must map to its own source col {trim_src} (unchanged \
         Piece::Modifier path). Tokens: {tokens:?}, output: {output}"
    );
}

/// KEBAB component tag + KEBAB arg — `<my-comp v-model:my-arg="x"/>`. The generated
/// prop name camelCases to `myArg`, and that token must map back to the SOURCE
/// `my-arg` arg span (a PascalCase/kebab component tag is still a component, so the
/// `!is_native` branch emits the mapped prop-name piece).
#[test]
fn vmodel_kebab_component_named_arg_maps_to_arg() {
    let source = r#"<template><my-comp v-model:my-arg="x"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("x", BindingType::SetupConst)]);

    assert!(
        output.contains("myArg={"),
        "kebab component v-model:my-arg should camelCase to `myArg={{...}}`: {output}"
    );

    let arg_src_col = source.find("v-model:my-arg").unwrap() as u32 + "v-model:".len() as u32;
    let name_gen = output.find("myArg={").unwrap();
    let (name_gl, name_gc) = gen_offset_to_line_col(&output, name_gen);
    assert!(
        tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == name_gl && dc == name_gc && sc == arg_src_col),
        "generated `myArg` prop-name (gen {name_gl}:{name_gc}) must map back to source arg \
         col {arg_src_col} (the `m` in `my-arg`). Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn tsx_props_binding_stays_dunder_props() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ msg }}</div></template>"#,
        &[("msg", BindingType::Props)],
    );
    assert!(
        result.contains("__props.msg"),
        "Props binding should use __props. Got: {}",
        result
    );
    assert!(
        !result.contains("___VERTER___instance.msg"),
        "Props binding should NOT get instance prefix. Got: {}",
        result
    );
}

#[test]
fn component_is_dynamic_expr_is_source_mapped() {
    // <component :is="currentView"> should emit a source-mapped temp variable
    // so TSGO can provide hover info on `currentView`.
    let source = r#"<template><component :is="currentView">hello</component></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("currentView", BindingType::SetupRef)]);

    // The output should contain the temp variable with the expression
    assert!(
        output.contains("currentView"),
        "output should contain `currentView`: {output}"
    );

    // Find the byte offset of "currentView" in the :is attribute value
    let expr_src_offset = source.find("currentView").unwrap();

    // There should be a source map token pointing back to the expression
    let has_expr_token = tokens
        .iter()
        .any(|&(_, _, src_col)| src_col == expr_src_offset as u32);
    assert!(
        has_expr_token,
        "component :is expression should have source map token at src col {}. Tokens: {:?}",
        expr_src_offset, tokens
    );
}

#[test]
fn component_is_dynamic_resolves_bindings() {
    // <component :is="currentView"> with SetupRef binding should resolve
    // the expression through the BindingResolver (e.g., `currentView.value`
    // for refs in non-inline mode, or just `currentView` for inline).
    let source = r#"<template><component :is="currentView">hello</component></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("currentView", BindingType::SetupRef)]);

    // With inline mode (default for TSX), SetupRef bindings are used directly.
    // The expression should be present in the output (not _ctx. prefixed since inline).
    assert!(
        output.contains("currentView"),
        "output should contain resolved `currentView`: {output}"
    );
    // The `:is` attribute itself should be removed
    assert!(
        !output.contains(":is="),
        "`:is` attribute should be removed from output: {output}"
    );
    // The `component` tag should be rewritten
    assert!(
        !output.contains("<component"),
        "`<component` tag should be rewritten: {output}"
    );
}

#[test]
fn component_is_dynamic_resolves_data_binding() {
    // In TSX mode, Data bindings use ___VERTER___instance. prefix (no _ctx. prefix).
    let source = r#"<template><component :is="currentView">hello</component></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("currentView", BindingType::Data)]);

    assert!(
        output.contains("___VERTER___instance.currentView") && !output.contains("_ctx.currentView"),
        "Data binding should use instance prefix in TSX mode: {output}"
    );
    assert!(
        !output.contains(":is="),
        "`:is` attribute should be removed from output: {output}"
    );
}

// ── Bug 1: Dynamic <component :is> uses extractRenderComponent ──

#[test]
fn component_dynamic_is_uses_extract_render_component() {
    let source = r#"<template><component :is="'div'"></component></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("___VERTER___extractRenderComponent"),
        "should use extractRenderComponent wrapper: {output}"
    );
    assert!(
        output.contains("___VERTER___component_render"),
        "should use ___VERTER___component_render temp name: {output}"
    );
    assert!(
        output.contains("const ___VERTER___component_render=___VERTER___extractRenderComponent("),
        "should declare const with extractRenderComponent wrapper: {output}"
    );
    // Negative: old format should not appear
    assert!(
        !output.contains("__verter_component_render"),
        "old format __verter_component_render should not appear: {output}"
    );
    assert!(
        !output.contains("<component"),
        "<component tag should be rewritten: {output}"
    );
}

#[test]
fn component_dynamic_is_expression() {
    let source = r#"<template><component :is="as || 'div'"></component></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("as", BindingType::SetupRef)]);

    assert!(
        output.contains("___VERTER___extractRenderComponent("),
        "should use extractRenderComponent: {output}"
    );
    assert!(
        output.contains("<___VERTER___component_render"),
        "should rewrite opening tag: {output}"
    );
    assert!(
        output.contains("</___VERTER___component_render>"),
        "should rewrite closing tag: {output}"
    );
}

#[test]
fn component_static_is_unchanged() {
    let source = r#"<template><component is="div" tabindex="1"></component></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("<div"),
        "static is should rewrite to target tag: {output}"
    );
    assert!(
        !output.contains("extractRenderComponent"),
        "static is should not use extractRenderComponent: {output}"
    );
    assert!(
        !output.contains("<component"),
        "<component tag should be rewritten: {output}"
    );
}

// ── Kebab component tags rewrite to their PascalCase binding ──────
//
// A resolvable kebab tag must reference the in-scope PascalCase const (local
// binding or GlobalComponents fallback) — a lowercase JSX identifier is an
// INTRINSIC lookup that never consults the emitted const.

#[test]
fn kebab_component_tag_rewrites_to_fallback_const() {
    let source = r#"<template><el-button size="small">go</el-button></template>"#;
    let output = gen_tsx_template_with_components(source, &[], &["ElButton"]);

    assert!(
        output.contains("<ElButton"),
        "kebab open tag must rewrite to the PascalCase const: {output}"
    );
    assert!(
        output.contains("</ElButton>"),
        "kebab close tag must rewrite to the PascalCase const: {output}"
    );
    assert!(
        !output.contains("<el-button"),
        "the intrinsic kebab tag must not survive: {output}"
    );
}

#[test]
fn kebab_component_tag_rewrites_to_local_binding() {
    let source = r#"<template><my-comp /></template>"#;
    let output =
        gen_tsx_template_with_components(source, &[("MyComp", BindingType::SetupImport)], &[]);

    assert!(
        output.contains("<MyComp"),
        "kebab tag must rewrite to the local import binding: {output}"
    );
    assert!(
        !output.contains("<my-comp"),
        "the intrinsic kebab tag must not survive: {output}"
    );
}

#[test]
fn kebab_component_tag_without_binding_stays_authored() {
    let source = r#"<template><never-registered /></template>"#;
    let output = gen_tsx_template_with_components(source, &[], &[]);

    assert!(
        output.contains("<never-registered"),
        "an unresolvable kebab tag stays as-authored (fail-closed intrinsic): {output}"
    );
    assert!(
        !output.contains("<NeverRegistered"),
        "no invented Pascal rewrite without a binding: {output}"
    );
}

#[test]
fn kebab_component_tag_with_body_rewrites_isolated_close() {
    // Body content routes through the isolated Vue slot-body fragment: the
    // typed empty pair must close with the REWRITTEN Pascal name.
    let source = r#"<template><el-button><span>x</span></el-button></template>"#;
    let output = gen_tsx_template_with_components(source, &[], &["ElButton"]);

    assert!(
        output.contains("<ElButton"),
        "kebab open tag must rewrite: {output}"
    );
    assert!(
        output.contains("></ElButton>"),
        "the isolated empty pair must close with the Pascal name: {output}"
    );
    assert!(
        !output.contains("el-button"),
        "no kebab remnant anywhere in the JSX: {output}"
    );
}

#[test]
fn component_static_is_kebab_resolves_through_inventory() {
    let source = r#"<template><component is="el-button" /></template>"#;
    let output = gen_tsx_template_with_components(source, &[], &["ElButton"]);

    assert!(
        output.contains("<ElButton"),
        "static is=\"el-button\" must rewrite to the resolvable Pascal const: {output}"
    );
    assert!(
        !output.contains("<el-button"),
        "the intrinsic kebab rewrite must not survive: {output}"
    );
}

#[test]
fn class_merge_with_prop_in_between() {
    let source =
        r#"<template><div class="foo" my-random-prop="true" :class="{bar: true}"/></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass: {output}"
    );
    assert!(
        output.contains("my-random-prop"),
        "should preserve other props: {output}"
    );
    let class_count = output.matches("class=").count();
    assert_eq!(
        class_count, 1,
        "should have exactly 1 class= attribute, got {class_count}: {output}"
    );
}

#[test]
fn style_object_literal_gets_css_properties_satisfies() {
    let source = r#"<template><div :style="{ color: 'red' }"/></template>"#;
    let output = gen_tsx_template(source);
    // Positive: object literal style should get CSSProperties satisfies annotation
    assert!(
        output.contains("satisfies") && output.contains("CSSProperties"),
        "object literal :style should have satisfies CSSProperties: {output}"
    );
    // Negative: non-object-literal style should NOT get satisfies
    let source2 = r#"<template><div :style="myVar"/></template>"#;
    let output2 = gen_tsx_template(source2);
    assert!(
        !output2.contains("satisfies"),
        "non-object-literal :style should NOT have satisfies: {output2}"
    );
}

#[test]
fn class_merge_with_script_attrs_and_generic() {
    // Regression: Popover.vue with attrs="{ class: string, style: string }" on
    // <script setup> produces duplicate class/style attributes in JSX.
    let source = r#"<script setup lang="ts" attrs="{ class: string, style: string }" generic="T extends object">
import { ref } from 'vue'
const show = ref(false)
const onClickWrapper = () => {}
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
</template>"#;
    let output = gen_tsx_template(source);

    eprintln!("=== ATTRS+GENERIC OUTPUT ===\n{}\n=== END ===", output);

    // Positive: should use normalizeClass for merged class
    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass for merged class: {output}"
    );

    // Critical: must have exactly 1 class= attribute (no duplicates → ts(17001))
    let class_count = output.matches("class=").count();
    assert_eq!(
        class_count, 1,
        "should have exactly 1 class= attribute, got {class_count}: {output}"
    );

    // Critical: must have exactly 1 style= attribute (no duplicates)
    let style_count = output.matches("style=").count();
    assert_eq!(
        style_count, 1,
        "should have exactly 1 style= attribute, got {style_count}: {output}"
    );

    // Negative: must not have double closing brace from normalizeClass
    assert!(
        !output.contains("])}}"),
        "must not have extra closing brace: {output}"
    );
}

/// `:data="$attrs"` (static key with instance prefix) must use split overwrite
/// so `$attrs` retains its source map position.
#[test]
fn static_prop_with_prefix_source_map_accuracy() {
    let source = r#"<template><div :data="$attrs"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Positive: the prop should be converted to JSX binding
    assert!(
        output.contains("data={___VERTER___instance.$attrs}"),
        ":data=\"$attrs\" should produce data={{instance.$attrs}}: {output}"
    );
    // Negative: no raw `:data` or `v-bind`
    assert!(
        !output.contains(":data"),
        ":data directive must be removed from JSX: {output}"
    );

    // Source map: verify $attrs maps to its original source position
    let source_attrs_col = source.find("$attrs").expect("$attrs in source") as u32;
    let has_attrs_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == source_attrs_col);
    assert!(
        has_attrs_token,
        "source map must have a token mapping to the original $attrs position (col {}), \
         but only found source columns: {:?}",
        source_attrs_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );
}

/// `:rows="d_rows"` with Data binding (PrimeVue-shaped case) — the prefix-only
/// rewrite must use split overwrite so `d_rows` retains its source map position.
/// Without the split, TSGO hover lands on the synthetic `___VERTER___instance` prefix.
#[test]
fn data_prop_binding_source_map_accuracy() {
    let source = r#"<template><DataTable :rows="d_rows"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("d_rows", BindingType::Data)]);

    // Positive: prop should use instance prefix
    assert!(
        output.contains("rows={___VERTER___instance.d_rows}"),
        ":rows=\"d_rows\" should produce rows={{___VERTER___instance.d_rows}}: {output}"
    );
    // Negative: no raw :rows
    assert!(
        !output.contains(":rows"),
        ":rows directive must be removed from JSX: {output}"
    );

    // Source map: d_rows should map to its original source position
    let source_col = source.find("d_rows").expect("d_rows in source") as u32;
    let has_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == source_col);
    assert!(
        has_token,
        "source map must have a token mapping to the original d_rows position (col {}), \
         but only found source columns: {:?}",
        source_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );
}

/// `:class="{ 'active': visible }"` with Props binding — patch-based approach must
/// preserve source map tokens for identifiers so TSGO hover works on sub-expressions.
/// With Props binding, `visible` gets `__props.` prefix, which previously used a single
/// overwrite destroying source map tokens.
#[test]
fn class_binding_with_props_source_map_accuracy() {
    let source = r#"<template><div :class="{ 'active': visible }"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("visible", BindingType::Props)]);

    // Positive: should produce JSX class binding with __props prefix
    assert!(
        output.contains("class={{ 'active': __props.visible }}"),
        "should convert :class to JSX class binding with props prefix: {output}"
    );
    // Negative: no raw :class
    assert!(
        !output.contains(":class"),
        ":class directive must be removed from JSX: {output}"
    );

    // Source map: `visible` identifier should have a token at its original source position
    // (patch-based approach preserves it via collect_binding_patches)
    let visible_src_col = source.find("visible").expect("visible in source") as u32;
    let has_visible_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == visible_src_col);
    assert!(
        has_visible_token,
        "source map must have a token mapping to the original visible position (col {}), \
         but only found source columns: {:?}",
        visible_src_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );
}

/// `$props` member access: `{{ $props.msg }}` — verify sourcemap token for `$props`.
///
/// The PositionMapper interpolates from `$props` token to `.msg`. If the expression is
/// rewritten (e.g., `$props` → `__props`), the original source token should still map
/// correctly. The `.msg` part needs the linear offset from the `$props` token to be intact.
#[test]
fn dollar_props_member_access_source_map() {
    let source = r#"<template><div>{{ $props.msg }}</div></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Positive: should contain $props.msg or a prefixed version
    assert!(
        output.contains("$props") || output.contains("__props"),
        "should contain $props reference: {output}"
    );

    // Sourcemap: verify `$props` has a token
    let props_src_col = source.find("$props").unwrap() as u32;
    let has_props_token = tokens.iter().any(|&(_, _, sc)| sc == props_src_col);
    assert!(
        has_props_token,
        "source map must have token for `$props` at col {}, tokens: {:?}",
        props_src_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );

    // Check: if $props is rewritten to something longer (e.g., __props or ___VERTER___.instance.$props),
    // the interpolation from the $props token to .msg won't work because the generated text
    // is longer than the source. Log the output for diagnosis.
    let msg_src_col = source.find("msg").unwrap() as u32;
    let props_to_msg_src_offset = msg_src_col - props_src_col; // 7 chars ("$props.")

    // Find generated position of $props token
    let props_gen = tokens
        .iter()
        .find(|&&(_, _, sc)| sc == props_src_col)
        .map(|&(dl, dc, _)| (dl, dc));

    if let Some((_gen_line, gen_col)) = props_gen {
        // In the generated output, check what's at gen_col + 7 (the interpolated .msg position)
        let gen_msg_expected = gen_col + props_to_msg_src_offset;
        let lines: Vec<&str> = output.lines().collect();
        if let Some(line_str) = lines.first() {
            if (gen_msg_expected as usize) < line_str.len() {
                let at_expected = &line_str[gen_msg_expected as usize..];
                if !at_expected.starts_with("msg") {
                    // Interpolation broken — $props was rewritten to something longer.
                    // This is the root cause: the generated text between $props and .msg
                    // has different length than the source, breaking linear interpolation.
                    eprintln!(
                        "DIAGNOSIS: $props interpolation broken. At gen col {}: '{}'. \
                         Output: '{}'",
                        gen_msg_expected,
                        &at_expected[..at_expected.len().min(20)],
                        output,
                    );
                }
            }
        }
    }
}

/// Props binding prefix sourcemap accuracy: `:title="myProp"` with Props binding.
/// The generated output has `__props.myProp`. The source map token for `myProp`
/// should point to the generated position of `myProp` (AFTER `__props.`), not to
/// `__props.` itself. This ensures hover at `myProp` in the Vue SFC resolves to the
/// correct prop type rather than the full `__props` object type.
#[test]
fn prop_binding_prefix_source_map_accuracy() {
    let source = r#"<template><div :title="myProp"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("myProp", BindingType::Props)]);

    // Positive: output should contain __props.myProp
    assert!(
        output.contains("__props.myProp"),
        "should apply __props prefix: {output}"
    );

    // Find source column of `myProp` in the :title attribute value
    let src_col = source.find("myProp").unwrap() as u32;

    // There should be a source map token whose source column points to `myProp`
    let token = tokens.iter().find(|&&(_, _, sc)| sc == src_col);
    assert!(
        token.is_some(),
        "source map must have a token for myProp at src col {src_col}. Tokens: {:?}",
        tokens
    );

    // The generated column of that token should point to `myProp` (after `__props.`),
    // not to `__props.` itself.
    let &(gen_line, gen_col, _) = token.unwrap();
    let lines: Vec<&str> = output.lines().collect();
    if let Some(line_str) = lines.get(gen_line as usize) {
        let at_gen = &line_str[gen_col as usize..];
        assert!(
            at_gen.starts_with("myProp"),
            "generated column {gen_col} should point to 'myProp', not '__props.'. \
             At gen col {gen_col}: '{}'. Full output: {output}",
            &at_gen[..at_gen.len().min(20)]
        );
    }
}

/// Props binding in template literal: `:class="\`prefix--${closeIconPosition}\`"`.
/// Same issue as above but within a template literal expression.
#[test]
fn prop_in_template_literal_source_map_accuracy() {
    let source = r#"<template><div :class="`prefix--${closeIconPosition}`"></div></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("closeIconPosition", BindingType::Props)]);

    // Positive: should apply __props prefix
    assert!(
        output.contains("__props.closeIconPosition"),
        "should apply __props prefix: {output}"
    );

    // Find source column of `closeIconPosition` in the template literal
    let src_col = source.find("closeIconPosition").unwrap() as u32;

    // There should be a source map token for closeIconPosition
    let token = tokens.iter().find(|&&(_, _, sc)| sc == src_col);
    assert!(
        token.is_some(),
        "source map must have a token for closeIconPosition at src col {src_col}. Tokens: {:?}",
        tokens
    );

    // The generated column should point to 'closeIconPosition', not '__props.'
    let &(gen_line, gen_col, _) = token.unwrap();
    let lines: Vec<&str> = output.lines().collect();
    if let Some(line_str) = lines.get(gen_line as usize) {
        let at_gen = &line_str[gen_col as usize..];
        assert!(
            at_gen.starts_with("closeIconPosition"),
            "generated column should point to 'closeIconPosition', not '__props.'. \
             At gen col {gen_col}: '{}'. Full output: {output}",
            &at_gen[..at_gen.len().min(30)]
        );
    }
}

/// `.foo` v-bind prop-modifier shorthand without value: `.foo` ≡ `.foo="foo"`.
/// The generated VALUE identifier (inside `foo={…}`) must map back to the source
/// `foo` key token (after the `.`). Pre-fix the WHOLE prop span was overwritten
/// with `format!("{}={{{}}}", key, resolved)`, baking both name and value into one
/// `Overwritten` chunk anchored at `prop.start` (the `.`), so the value
/// identifier had NO token at the source `foo` start.
#[test]
fn dot_prop_shorthand_no_value_value_maps_to_source() {
    let source = r#"<template><Comp .foo/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("foo", BindingType::SetupConst)]);

    assert!(
        output.contains("foo={foo}"),
        "should convert .foo to foo={{foo}}: {output}"
    );

    let dot_src_col = source.find(".foo").unwrap() as u32;
    let key_src_col = dot_src_col + 1; // the `f` of the key token (after `.`)

    let pair_gen_col = output.find("foo={foo}").unwrap() as u32;
    let value_gen_col = pair_gen_col + "foo={".len() as u32;

    let value_maps_to_source = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == value_gen_col && sc == key_src_col);
    assert!(
        value_maps_to_source,
        "the generated VALUE identifier `foo` (gen col {value_gen_col}) must map to source col \
         {key_src_col} (the `f` in the `.foo` key). Pre-fix the whole `.foo` span was baked into a \
         mapped overwrite anchored at the `.` and had no such token. Tokens: {:?}",
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );

    // Negative: the value identifier must NOT collapse to the prop start (`.`).
    let value_maps_to_dot = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == value_gen_col && sc == dot_src_col);
    assert!(
        !value_maps_to_dot,
        "the generated VALUE identifier must not map to the `.` (col {dot_src_col}). \
         Tokens: {:?}",
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );
}

#[test]
fn dynamic_component_closing_tag_no_attributes() {
    // Bug: </component :is="as"> leaks attributes onto JSX closing tag
    let result = gen_tsx_template(
        r#"<template><component :is="tag">child</component :is="tag"></template>"#,
    );
    eprintln!("TSX output:\n{}", result);

    // POSITIVE: should have the component render variable
    assert!(
        result.contains("___VERTER___component_render"),
        "should use component_render for dynamic :is, got: {}",
        result
    );

    // NEGATIVE: closing tag must NOT contain attributes
    assert!(
        !result.contains("</___VERTER___component_render :is"),
        "closing tag must not have :is attribute, got: {}",
        result
    );
    assert!(
        !result.contains(r#"</___VERTER___component_render "#),
        "closing tag must not have trailing content after tag name, got: {}",
        result
    );
}

#[test]
fn dynamic_component_inside_jsx_children_valid_tsx() {
    // Bug: <component :is> inside another element puts const in JSX children
    let result = gen_tsx_template_with_bindings(
        r#"<template><div><component :is="tag" /></div></template>"#,
        &[("tag", BindingType::SetupConst)],
    );
    eprintln!("TSX output:\n{}", result);

    // The const statement for extractRenderComponent must be in valid JS context,
    // not inside JSX element children where it would be treated as text
    // Valid patterns:
    //   {(() => { const comp = ...; return <comp />; })()}
    //   Block scope before JSX
    // Invalid: <div>const comp = ...; <comp /></div>
    assert!(
        !result.contains(">const ___VERTER___component_render"),
        "const statement must not appear as JSX text children, got: {}",
        result
    );
}

/// <component :is="..."> should not generate a ___VERTER___Comp function with
/// `instantiateComponent(component, {})` — `component` is not a valid variable.
#[test]
fn component_is_dynamic_no_comp_function() {
    let source = r#"<template>
  <component :is="tag" />
</template>
<script setup lang="ts">
const tag = 'div';
</script>"#;
    let alloc = Allocator::new();
    let options = crate::compile::legacy_test_support::CodegenOptions {
        filename: Some("App.vue".to_string()),
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

    // Must NOT contain instantiateComponent(component, ...)
    assert!(
        !tsx.code.contains("instantiateComponent(component"),
        "Should not emit Comp function for <component :is>. Got:\n{}",
        tsx.code
    );

    // Parse to ensure valid TSX
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "Got {} errors",
        parsed.diagnostics.len()
    );
}

/// Destructured defineProps (`const { foo } = defineProps<{...}>()`) should
/// declare ___VERTER___props so that `const __props = ___VERTER___props` resolves.
#[test]
fn destructured_define_props_declares_verter_props() {
    let source = r#"<script setup lang="ts">
const { msg, count } = defineProps<{
  msg: string
  count: number
}>()
</script>
<template>
  <div>{{ msg }} {{ count }}</div>
</template>"#;
    let alloc = Allocator::new();
    let options = crate::compile::legacy_test_support::CodegenOptions {
        filename: Some("App.vue".to_string()),
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

    // ___VERTER___props must be declared, not just referenced
    assert!(
        tsx.code.contains("const ___VERTER___props"),
        "Should declare ___VERTER___props for destructured defineProps. Got:\n{}",
        tsx.code
    );

    // Original destructured pattern should NOT remain
    assert!(
        !tsx.code.contains("const { msg, count }"),
        "Destructuring pattern should be rewritten. Got:\n{}",
        tsx.code
    );

    // Parse to ensure valid TSX
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
fn external_bloc_component_produces_valid_tsx() {
    let Some(source) = read_external_corpus_vue(
        "VERTER_PRIVATE_CORPUS_ROOT",
        "packages/ui/src/components/atom/Bloc/Bloc.vue",
    ) else {
        return;
    };
    let alloc = Allocator::new();
    let options = crate::compile::legacy_test_support::CodegenOptions {
        filename: Some("Bloc.vue".to_string()),
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
    eprintln!("=== BLOC TSX ===\n{}\n=== END ===", tsx.code);
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
fn runtime_define_props_in_template_scope() {
    // Runtime defineProps({...}) without assignment should expose prop names
    // in the template scope. TS2304 "Cannot find name" if they're not.
    let source = r#"<template>
  <div v-if="showBoard">
    <router-link :to="`/boards/${url}`">{{ name }}</router-link>
  </div>
</template>

<script setup lang="ts">
defineProps({
  name: { type: String, required: true },
  url: { type: String, required: true },
  showBoard: { type: Boolean, required: true },
});
</script>"#;
    let alloc = Allocator::new();
    let options = crate::compile::legacy_test_support::CodegenOptions {
        filename: Some("BoardBadge.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
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
    eprintln!("=== RUNTIME PROPS TSX ===\n{}\n=== END ===", tsx.code);

    // Positive: props should be accessible via __props in template
    assert!(
        tsx.code.contains("__props.showBoard"),
        "showBoard should be accessed via __props in template, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("__props.url") || tsx.code.contains("__props.name"),
        "url/name should be accessed via __props in template, got:\n{}",
        tsx.code
    );

    // Negative: Comp function condition guards must also use __props
    // (TS2304 "Cannot find name 'showBoard'" if bare)
    assert!(
        !tsx.code.contains("if(!((showBoard)))"),
        "Comp function guard must NOT use bare 'showBoard' — should be __props.showBoard, got:\n{}",
        tsx.code
    );

    // OXC validation
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "Full TSX should parse without errors. Got {} errors:\n{}",
        parsed.diagnostics.len(),
        tsx.code
    );
}

#[test]
fn closing_tag_case_mismatch_component() {
    // Vue is case-insensitive for closing tags: <Button>...</button> is valid.
    // JSX is case-sensitive: the closing tag must match the opening tag.
    // Verter must rewrite the closing tag to match the opening tag.
    let result = gen_tsx_template_with_bindings(
        r#"<template>
  <Button class="btn">Click</Button>
  <Button class="btn2">Click2</button>
</template>"#,
        &[("Button", BindingType::SetupConst)],
    );
    eprintln!("=== CASE MISMATCH ===\n{}\n=== END ===", result);

    // Positive: both buttons should have matching closing tags
    let close_count = result.matches("</Button>").count();
    assert!(
        close_count == 2,
        "should have 2 </Button> closing tags (case-corrected), got {} in:\n{}",
        close_count,
        result
    );

    // Negative: lowercase </button> should not appear
    assert!(
        !result.contains("</button>"),
        "lowercase </button> should be rewritten to </Button>, got:\n{}",
        result
    );
}

#[test]
fn object_literal_binding_prop_key_not_rewritten() {
    // Bug: `:overlay-style="{ zIndex: zIndex - 2 }"` where `zIndex` is a prop
    // causes `resolve_all_prop_refs_in_expr` to produce `__props.zIndex: __props.zIndex - 2`
    // which is invalid JS (can't have dots in object keys without quotes).
    let source = r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
const props = defineProps<{ zIndex: number }>()
</script>
<template>
  <MyComp :overlay-style="{ zIndex: zIndex - 2 }" />
</template>"#;
    let code = compile_full_sfc_tsx(source, "Test.vue");
    eprintln!("Object key test TSX:\n{code}");

    // Should parse without errors (the core assertion)
    assert_valid_tsx(&code, "object-key-prop");

    // Negative: should NOT have __props.zIndex: (invalid object key)
    assert!(
        !code.contains("__props.zIndex:"),
        "object key must NOT be prefixed with __props.: {code}"
    );
}

// ── @ts-expect-error / @ts-ignore in template comments ──────────────────────

#[test]
fn ts_expect_error_before_component() {
    let result = gen_tsx_template(r#"<template><!-- @ts-expect-error --><MyComp/></template>"#);
    // Comment should appear as JSX comment before the component
    assert!(
        result.contains("{/* @ts-expect-error */}"),
        "should have TS directive comment, got:\n{}",
        result
    );
    // Comment must appear before the component tag
    let comment_pos = result.find("{/* @ts-expect-error */}").unwrap();
    let comp_pos = result.find("<MyComp").unwrap();
    assert!(
        comment_pos < comp_pos,
        "comment should appear before component, got:\n{}",
        result
    );
    // No raw HTML comment markers in output
    assert!(
        !result.contains("<!--"),
        "should not have raw HTML comment markers, got:\n{}",
        result
    );
    assert!(
        !result.contains("-->"),
        "should not have raw HTML comment close, got:\n{}",
        result
    );
}

#[test]
fn ts_expect_error_before_component_is() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><!-- @ts-expect-error --><component :is="comp"/></template>"#,
        &[("comp", BindingType::SetupRef)],
    );
    // <component :is> wraps in IIFE — comment must be inside the IIFE
    assert!(
        result.contains("extractRenderComponent"),
        "should have extractRenderComponent IIFE, got:\n{}",
        result
    );
    // Comment should be inside the IIFE (after the IIFE open)
    let iife_pos = result.find("(() =>").unwrap();
    // Check that a TS directive comment appears somewhere after the IIFE open
    let after_iife = &result[iife_pos..];
    assert!(
        after_iife.contains("@ts-expect-error"),
        "TS directive comment should be inside component :is IIFE, got:\n{}",
        result
    );
    // No raw HTML comment markers
    assert!(
        !result.contains("<!--"),
        "no raw HTML markers, got:\n{}",
        result
    );
}

#[test]
fn options_api_component_shorthand_no_alias_needed() {
    // Shorthand: components: { SomeComp } — SomeComp is already imported, no alias needed
    let source = r#"<script lang="ts">
import { defineComponent } from 'vue'
import SomeComp from './SomeComp.vue'

export default defineComponent({
  components: { SomeComp },
  setup() { return {} }
})
</script>
<template>
  <SomeComp />
</template>"#;
    let alloc = Allocator::new();
    let options = crate::compile::legacy_test_support::CodegenOptions {
        filename: Some("Test.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
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

    // SomeComp is already imported — no extra alias declaration needed
    // (it shouldn't break if one IS emitted, but it's unnecessary)
    assert!(
        tsx.code.contains("<SomeComp"),
        "template should contain <SomeComp> JSX tag:\n{}",
        tsx.code
    );
    // Must be valid TSX
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "TSX should have no parse errors. Got {} errors:\n{}",
        parsed.diagnostics.len(),
        tsx.code
    );
}

// ── Standalone mapped resolver-prefixed expression heads ─────

/// Discriminating — the dynamic `<component :is="expr">` emitter routes its
/// resolved expression through the shared segmented producer. With an INLINE
/// resolver, a setup ref `currentView` resolves to `currentView.value`; the
/// `currentView` identifier maps to its source while the injected `.value` stays
/// UNMAPPED. The old single-chunk fold mapped the whole `currentView.value` run
/// at `iife_prefix.len()`, leaving no unmapped token at the `.value` boundary —
/// the `value_gen` assertion fails on that fold.
#[test]
fn dynamic_component_is_setup_ref_keeps_value_unmapped() {
    let alloc = Allocator::new();
    let source = r#"<template><component :is="currentView" /></template>"#;
    let bytes = source.as_bytes();

    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });
    let template_ast = syntax.take_template_ast().expect("template ast");
    let oxc_ast = crate::template::oxc::parse_template_expressions(
        &template_ast,
        source,
        &alloc,
        oxc_span::SourceType::tsx(),
        true,
    );

    // Locate the dynamic `<component>` element and its OXC data.
    let (el, oxc_el) = template_ast
        .nodes
        .iter()
        .enumerate()
        .find_map(|(i, node)| match &node.kind {
            AstNodeKind::Element(el) if el.tag_type == TagType::Component => {
                let oxc_el = match &oxc_ast.data[i] {
                    OxcNodeData::Element(b) => Some(b.as_ref()),
                    _ => None,
                };
                Some((el.as_ref(), oxc_el))
            }
            _ => None,
        })
        .expect("dynamic <component> element");

    // Inline (non-TSX) resolver so the setup ref takes the `.value` suffix.
    let mut binding_map: FxHashMap<&str, BindingType> = FxHashMap::default();
    binding_map.insert("currentView", BindingType::SetupRef);
    let resolver = BindingResolver::new(binding_map, true);

    let mut out = CodeGenOutput::new(&alloc);
    let rewrite = rewrite_component_is(
        el,
        oxc_el,
        source,
        &mut out,
        &resolver,
        &TemplateComponentBindings::default(),
        &[],
        EmitContext::JsxChildren,
    );
    let rewrite = rewrite.expect("dynamic :is must be handled");
    assert_eq!(rewrite.tag_name, "___VERTER___component_render");
    assert!(rewrite.needs_iife_close);

    let mut ct = CodeTransform::new(source, &alloc);
    out.apply_to(&mut ct);
    let built = ct.build_string();

    // The resolved expression keeps its `.value` (inline setup ref); bytes unchanged.
    assert!(
        built.contains("___VERTER___extractRenderComponent(currentView.value)"),
        "got: {built}"
    );

    let iife_prefix =
        "{(() => { const ___VERTER___component_render=___VERTER___extractRenderComponent(";
    let cv_gen = el.tag_open.start + iife_prefix.len() as u32;
    let cv_src = source.find("currentView").unwrap() as u32;

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

    // `currentView` maps at `iife_prefix.len()` (from the prepend anchor) → its source col.
    let cv = tokens
        .iter()
        .find(|t| t.get_dst_col() == cv_gen && t.get_source_id().is_some());
    assert!(cv.is_some(), "`currentView` must map; tokens: {dump:?}");
    assert_eq!(
        cv.unwrap().get_src_col(),
        cv_src,
        "`currentView` must map to its source col, not the synthetic `.value`"
    );

    // The synthetic `.value` begins its own UNMAPPED segment at iife_prefix.len() + 11.
    let value_gen = cv_gen + "currentView".len() as u32;
    assert!(
        tokens
            .iter()
            .any(|t| t.get_dst_col() == value_gen && t.get_source_id().is_none()),
        "synthetic `.value` must start an unmapped segment at col {value_gen}; tokens: {dump:?}"
    );
    // No source token inside the `.value` region.
    assert!(
        !tokens.iter().any(|t| t.get_dst_col() >= value_gen
            && t.get_dst_col() < value_gen + ".value".len() as u32
            && t.get_source_id().is_some()),
        "`.value` region must carry no source token; tokens: {dump:?}"
    );
}
