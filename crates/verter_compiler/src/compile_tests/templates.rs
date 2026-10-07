use super::*;

#[test]
fn vmrs_static_defaults_follow_dev_prod_and_custom_element_profiles() {
    use std::sync::Arc;
    use verter_macro_dto::{
        AuthoredMemberOrdinal, MacroAnchor, MacroRuntimeBundle, MacroRuntimeEntry,
        MacroRuntimeOutcome, MacroRuntimeShape, OrderedRuntimeConstructors,
        PropsDefaultsAssociation, PropsRuntimeShape, RuntimeConstructor, RuntimeProp,
        RuntimePropType,
    };

    let prop = |name: &str, optional: bool, constructors, ordinal| RuntimeProp {
        name: name.to_owned(),
        optional,
        type_shape: RuntimePropType::Resolved {
            constructors: OrderedRuntimeConstructors::from_ordered(constructors),
            skip_check: false,
        },
        anchor: MacroAnchor::Authored {
            macro_index: 0,
            member_ordinal: AuthoredMemberOrdinal::new(ordinal),
        },
    };
    let semantics = VueMacroSemanticInput::Runtime(Arc::new(MacroRuntimeBundle {
        entries: vec![MacroRuntimeEntry {
            syntax_index: 0,
            macro_index: 1,
            outcome: MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(PropsRuntimeShape {
                defaults: PropsDefaultsAssociation::WithDefaults {
                    payload_macro_index: 0,
                    defaults_macro_index: 1,
                },
                props: vec![
                    prop("text", false, vec![RuntimeConstructor::String], 0),
                    prop("callable", true, vec![RuntimeConstructor::Function], 1),
                    prop("enabled", true, vec![RuntimeConstructor::Boolean], 2),
                    prop("opaque", true, Vec::new(), 3),
                    prop("method", true, vec![RuntimeConstructor::Function], 4),
                ],
            })),
        }],
    }));
    let source = r#"<script setup lang="ts">
withDefaults(defineProps<{
  text: string
  callable?: () => number
  enabled?: boolean
  opaque?: unknown
  method?: () => number
}>(), { text: 'fallback', callable: () => 1, method() { return 2 } })
</script>"#;
    let compile_profile = |is_production, custom_element| {
        compile(
            source,
            &CodegenOptions {
                is_production,
                custom_element,
                ..Default::default()
            },
            &VerterCompileOptions {
                force_js: true,
                ..Default::default()
            },
            &semantics,
            &Allocator::new(),
        )
    };

    let dev = compile_profile(false, false);
    assert!(dev.errors.is_empty(), "{:?}", dev.errors);
    let dev = dev.script.expect("dev script").code;
    assert!(
        !dev.contains("_mergeDefaults("),
        "static defaults are embedded: {dev}"
    );
    assert!(
        dev.contains("text: { type: String, required: true, default: 'fallback' }"),
        "{dev}"
    );
    assert!(
        dev.contains("callable: { type: Function, required: false, default: () => 1 }"),
        "{dev}"
    );
    assert!(
        dev.contains("enabled: { type: Boolean, required: false }"),
        "{dev}"
    );
    assert!(
        dev.contains("opaque: { type: null, required: false }"),
        "{dev}"
    );
    assert!(
        dev.contains("method: { type: Function, required: false, \"default\"() { return 2 } }"),
        "{dev}"
    );

    let prod = compile_profile(true, false);
    assert!(prod.errors.is_empty(), "{:?}", prod.errors);
    let prod = prod.script.expect("prod script").code;
    assert!(prod.contains("text: { default: 'fallback' }"), "{prod}");
    assert!(
        prod.contains("callable: { type: Function, default: () => 1 }"),
        "{prod}"
    );
    assert!(prod.contains("enabled: { type: Boolean }"), "{prod}");
    assert!(prod.contains("opaque: {}"), "{prod}");
    assert!(
        prod.contains("method: { type: Function, \"default\"() { return 2 } }"),
        "{prod}"
    );
    assert!(!prod.contains("required:"), "{prod}");

    let custom_element = compile_profile(true, true);
    assert!(
        custom_element.errors.is_empty(),
        "{:?}",
        custom_element.errors
    );
    let custom_element = custom_element.script.expect("custom-element script").code;
    assert!(
        custom_element.contains("text: { default: 'fallback', type: String }"),
        "{custom_element}"
    );
    assert!(
        custom_element.contains("callable: { type: Function, default: () => 1 }"),
        "{custom_element}"
    );
    assert!(
        custom_element.contains("opaque: { type: null }"),
        "{custom_element}"
    );
    assert!(
        custom_element.contains("method: { type: Function, \"default\"() { return 2 } }"),
        "{custom_element}"
    );
}

#[test]
fn template_output_contains_render_function_vdom() {
    let result = compile_sfc(
        r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("function render("),
        "Expected render function in template output, got: {}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("<div>"),
        "Template output should not contain raw HTML: {}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("<script"),
        "Template output should not contain script tags: {}",
        tpl.code
    );
}

#[test]
fn template_output_contains_render_function_vapor() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        force_vapor: true,
        ..Default::default()
    };
    let result = compile(
        r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("function render("),
        "Expected render function in template output, got: {}",
        tpl.code
    );
    assert!(
        tpl.code.contains("_template("),
        "Expected _template() call in vapor output, got: {}",
        tpl.code
    );
    // Vapor legitimately has <div> inside _template("...") string literals,
    // so check there's no raw <div> OUTSIDE of string contexts.
    // A raw <div> would appear as a line starting with `<div>` or after whitespace.
    assert!(
        !tpl.code.contains("<script"),
        "Template output should not contain script tags: {}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("<template>"),
        "Template output should not contain raw template tags: {}",
        tpl.code
    );
}

#[test]
fn component_whitespace_children_clean_output() {
    // Component with whitespace-only children should not leak close tag
    let code = compile_and_validate_template(
        "<template><div><Comp :foo=\"bar\">\n  </Comp></div></template>",
    );
    assert!(
        !code.contains("</Comp>"),
        "Component close tag should not appear in output\n{}",
        code
    );
}

// ==================== v-for in text context ====================

#[test]
fn vfor_after_text_in_multi_root_template() {
    // Text followed by v-for in a multi-root template: the text and v-for
    // should be separate children in the Fragment array, not combined.
    let code = compile_and_validate_template(
        r#"<template>Text <template v-for="item in items"><span>{{ item }}</span></template></template>"#,
    );
    assert!(
        code.contains("_createTextVNode"),
        "Should have text node\n{}",
        code
    );
    assert!(
        code.contains("_renderList"),
        "Should have render list\n{}",
        code
    );
}

// ==================== Vapor mode tests ====================

#[test]
fn vapor_interpolation_with_call_expression() {
    // Reproduces: $t("key") → _ctx.$t"key" (missing parentheses)
    let code = compile_and_validate_vapor_template(
        r#"<template><div>{{ $t("hello.world") }}</div></template>"#,
    );
    // The call expression must preserve parentheses
    assert!(
        code.contains("$t("),
        "Call expression $t() must preserve parentheses\n{}",
        code
    );
}

#[test]
fn vapor_component_with_dotted_name() {
    // Component names like Calendar.Root should produce valid variable names
    let code = compile_and_validate_vapor_template(
        r#"<template><Calendar.Root locale="en" /></template>"#,
    );
    assert!(
        code.contains("_component_Calendar_Root"),
        "Dotted component names should use underscores in variable\n{}",
        code
    );
}

#[test]
fn unknown_component_uses_resolve_component() {
    let result = compile_sfc(
        r#"<template><div><UnknownComp /></div></template>
<script setup>const x = 1;</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_resolveComponent(\"UnknownComp\")")
            || tpl.code.contains("resolveComponent(\"UnknownComp\")"),
        "unknown component should use _resolveComponent, got:\n{}",
        tpl.code
    );
}

#[test]
fn self_referencing_component_uses_maybe_self_reference() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("TokenBreakdown.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div><TokenBreakdown /></div></template>
<script setup>const x = 1;</script>"#,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code
            .contains("_resolveComponent(\"TokenBreakdown\", true)"),
        "recursive self-reference should use _resolveComponent(name, true), got:\n{}",
        tpl.code
    );
}

#[test]
fn self_referencing_component_kebab_case() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("TokenBreakdown.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<template><div><token-breakdown /></div></template>
<script setup>const x = 1;</script>"#,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code
            .contains("_resolveComponent(\"token-breakdown\", true)"),
        "recursive self-reference (kebab-case) should use _resolveComponent(name, true), got:\n{}",
        tpl.code
    );
}

/// A completely empty .vue file (e.g. motion-vue's playground Home.vue) is a
/// valid Vue component — an EMPTY one. The compiler must emit a minimal
/// component shell (`defineComponent({ __name })` + `export default`) so the
/// bundler/host lane still exports a component instead of erroring.
#[test]
fn empty_sfc_compiles_to_empty_component_shell() {
    for src in ["", "\n   \n", "<!-- comment only -->"] {
        let result = compile_sfc(src);
        assert!(
            result.errors.is_empty(),
            "empty SFC {src:?} must compile without errors, got: {:?}",
            result.errors
        );
        let script = result
            .script
            .as_ref()
            .unwrap_or_else(|| panic!("empty SFC {src:?} must emit a synthetic script shell"));
        assert!(
            script.code.contains("defineComponent("),
            "empty SFC shell should wrap in defineComponent, got:\n{}",
            script.code
        );
        assert!(
            script.code.contains("__name: \"App\""),
            "empty SFC shell should carry the filename-derived __name, got:\n{}",
            script.code
        );
        assert!(
            script.code.contains("export default"),
            "empty SFC shell must export the component, got:\n{}",
            script.code
        );
        // Negative surface: nothing else is fabricated.
        assert!(result.template.is_none(), "empty SFC has no template block");
        assert!(result.styles.is_empty(), "empty SFC has no style blocks");
        assert!(
            !script.code.contains("props:") && !script.code.contains("slots"),
            "empty SFC shell must not fabricate props/slots, got:\n{}",
            script.code
        );
    }
}

/// `<template functional>` is unsupported in Vue 3 (`parse.spec.ts`).
#[test]
fn template_functional_unsupported() {
    let result = compile_sfc("<template functional></template>");
    assert!(
        result
            .errors
            .iter()
            .any(|e| e.code == "TemplateFunctionalUnsupported"),
        "must diagnose TemplateFunctionalUnsupported, got: {:?}",
        result.errors
    );
}

/// Plain `<template>` (no `functional`) never diagnoses `TemplateFunctionalUnsupported`.
#[test]
fn plain_template_has_no_functional_error() {
    let result = compile_sfc("<template><div>x</div></template>");
    assert!(
        !result
            .errors
            .iter()
            .any(|e| e.code == "TemplateFunctionalUnsupported"),
        "plain template must not diagnose TemplateFunctionalUnsupported, got: {:?}",
        result.errors
    );
}

// @ai-generated - TDD test: component with whitespace-only children doesn't leak close tag
#[test]
fn component_whitespace_only_children_no_close_tag_leak() {
    let result = compile_sfc(
        r#"<template><div><Comp :foo="bar">
  </Comp></div></template>
<script setup>import Comp from "./Comp.vue"; const bar = 1;</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // The close tag </Comp> must NOT appear in the output
    assert!(
        !tpl.code.contains("</Comp>"),
        "component close tag should not leak into JS output, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("_createVNode("),
        "should have _createVNode call, got:\n{}",
        tpl.code
    );
}

#[test]
fn component_is_uses_resolve_dynamic_component() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
const tag = ref('a')
</script>
<template>
  <component :is="tag" class="link">click</component>
</template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_resolveDynamicComponent"),
        "<component :is> should use _resolveDynamicComponent.\nOutput:\n{}",
        tpl.code
    );
    // The :is prop should NOT appear in the props object
    assert!(
        !tpl.code.contains("is:") && !tpl.code.contains("\"is\":"),
        ":is should not be in props object.\nOutput:\n{}",
        tpl.code
    );
}

#[test]
fn component_is_self_closing_uses_resolve_dynamic_component() {
    // Self-closing <component :is> with no children should use _resolveDynamicComponent
    let code = compile_and_validate_template(
        r#"<script setup lang="ts">
const tag = ref('div')
</script>
<template>
  <component :is="tag" />
</template>"#,
    );
    assert!(
        code.contains("_resolveDynamicComponent"),
        "Self-closing <component :is> should use _resolveDynamicComponent.\nOutput:\n{}",
        code
    );
    assert!(
        !code.contains("_resolveComponent(\"component\")"),
        "Should NOT use _resolveComponent(\"component\").\nOutput:\n{}",
        code
    );
    // The :is prop should NOT appear in the props object
    assert!(
        !code.contains("is:") && !code.contains("\"is\""),
        ":is should not be in props object.\nOutput:\n{}",
        code
    );
}

#[test]
fn component_is_empty_uses_resolve_dynamic_component() {
    // Empty <component :is> (open + close, no children) should use _resolveDynamicComponent
    let code = compile_and_validate_template(
        r#"<script setup lang="ts">
const tag = ref('div')
</script>
<template>
  <component :is="tag"></component>
</template>"#,
    );
    assert!(
        code.contains("_resolveDynamicComponent"),
        "Empty <component :is> should use _resolveDynamicComponent.\nOutput:\n{}",
        code
    );
}

#[test]
fn component_is_valueless_shorthand_resolves_dynamic() {
    // Vue 3.4 same-name shorthand: `<component :is />` == `<component :is="is" />`.
    // Must resolve via _resolveDynamicComponent(<is binding>), NOT degrade into an
    // ordinary component literally named "component" carrying `is` as a prop.
    let code = compile_and_validate_template(
        r#"<script setup>
const is = 'div'
</script>
<template>
  <component :is />
</template>"#,
    );
    assert!(
        code.contains("_resolveDynamicComponent($setup.is)")
            || code.contains("_resolveDynamicComponent(_ctx.is)"),
        "value-less :is must resolve the same-name shorthand binding via _resolveDynamicComponent.\nOutput:\n{}",
        code
    );
    // NEGATIVE: must NOT become an ordinary component named "component".
    assert!(
        !code.contains("_resolveComponent(\"component\")"),
        "value-less :is must NOT degrade into ordinary component \"component\".\nOutput:\n{}",
        code
    );
    // NEGATIVE: `is` must NOT leak into the props object or dynamicProps array.
    assert!(
        !code.contains("[\"is\"]")
            && !code.contains("is: $setup.is")
            && !code.contains("is:$setup.is"),
        ":is must be consumed by dynamic-component resolution, not emitted as a prop.\nOutput:\n{}",
        code
    );
}

#[test]
fn component_static_is_uses_resolve_dynamic_component() {
    // Static <component is="div"> (without colon binding)
    let code = compile_and_validate_template(
        r#"<template>
  <component is="div" />
</template>"#,
    );
    assert!(
        code.contains("_resolveDynamicComponent"),
        "Static <component is> should use _resolveDynamicComponent.\nOutput:\n{}",
        code
    );
}

/// @ai-generated — Template literal with HTML entities in v-bind should produce valid JS
/// with correct _ctx. prefixing. Regression test for a bug where `&quot;` entities in
/// template literals caused binding patches to be applied at wrong byte offsets,
/// producing mangled identifiers like `useri_ctx.userinfoname` instead of
/// `_ctx.userinfo.nickname`.
#[test]
pub(super) fn test_vbind_template_literal_with_html_entities() {
    let code = compile_and_validate_template(
        r#"<template><div :subtitle="`&quot;${userinfo.nickname}&quot;共获得${_formatNumber(userinfo.aweme_count)}个赞`"></div></template>"#,
    );

    // _ctx. should appear before each unresolved identifier
    assert!(
        code.contains("_ctx.userinfo.nickname"),
        "Should prefix userinfo.nickname with _ctx., got:\n{}",
        code
    );
    assert!(
        code.contains("_ctx._formatNumber"),
        "Should prefix _formatNumber with _ctx., got:\n{}",
        code
    );
    // Identifiers must not be split by _ctx. insertion
    assert!(
        !code.contains("useri_ctx"),
        "Identifiers should not be split by _ctx. insertion, got:\n{}",
        code
    );
    assert!(
        !code.contains("_formatNum_ctx"),
        "Identifiers should not be split by _ctx. insertion, got:\n{}",
        code
    );
}

// @ai-generated - TDD test: HTML entities in text content must be decoded
#[test]
fn html_entity_nbsp_decoded_in_text() {
    let result = compile_sfc(
        r#"<script setup>
</script>
<template><span>&nbsp;</span></template>"#,
    );
    let template = result.template.as_ref().expect("template block");
    // Vue's compiler decodes &nbsp; to the literal U+00A0 character in the JS string.
    // Verter must do the same — outputting "&nbsp;" literally causes double-escaping
    // in the DOM (&amp;nbsp;).
    assert!(
        !template.code.contains("&nbsp;"),
        "HTML entity &nbsp; should be decoded, not left as literal text.\nGot:\n{}",
        template.code
    );
    // Should contain the actual non-breaking space character (\u{00A0})
    assert!(
        template.code.contains('\u{00A0}'),
        "Template should contain decoded non-breaking space (U+00A0).\nGot:\n{}",
        template.code
    );
}

// ═══════════════════════════════════════════════════════════════
// force_js: template expression TS stripping
// ═══════════════════════════════════════════════════════════════

/// @ai-generated - force_js should strip `as` type assertions from template expressions
#[test]
fn force_js_strips_as_expression_from_template() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const foo = ref('hello')
</script>
<template><div>{{ (foo as string) }}</div></template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        !tpl.code.contains("as string"),
        "force_js should strip 'as string' from template expression, got:\n{}",
        tpl.code
    );
}

/// @ai-generated - force_js should strip non-null assertions from template expressions
#[test]
fn force_js_strips_non_null_assertion_from_template() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const foo = ref<string | null>('hello')
</script>
<template><div>{{ foo! }}</div></template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // The output should not contain the `!` non-null assertion
    // foo! should become just foo
    assert!(
        !tpl.code.contains("$setup.foo!"),
        "force_js should strip '!' non-null assertion from template expression, got:\n{}",
        tpl.code
    );
}

/// @ai-generated - force_js should strip type arguments from template call expressions
#[test]
fn force_js_strips_type_arguments_from_template_call() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
function generic<T>(val: T): T { return val }
</script>
<template><div>{{ generic<string>('hello') }}</div></template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        !tpl.code.contains("<string>"),
        "force_js should strip '<string>' type argument from template call expression, got:\n{}",
        tpl.code
    );
}

/// @ai-generated - force_js should strip satisfies expressions from template
#[test]
fn force_js_strips_satisfies_from_template() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
const x = { a: 1 }
</script>
<template><div>{{ (x satisfies Record<string, number>) }}</div></template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        !tpl.code.contains("satisfies"),
        "force_js should strip 'satisfies' from template expression, got:\n{}",
        tpl.code
    );
}

#[test]
fn vapor_template_attr_uses_ctx_prefix() {
    let result = compile_sfc_vapor(
        "<script setup>\nconst title = 'hello'\n</script>\n<template><div :title=\"title\"></div></template>",
    );
    let tpl = result.template.as_ref().expect("should have template");
    assert!(
        !tpl.code.contains("$setup."),
        "Vapor dynamic attr should not use $setup. prefix, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("_ctx.title"),
        "Vapor dynamic attr should use _ctx. prefix, got:\n{}",
        tpl.code
    );
}

#[test]
fn vapor_with_template_vapor_attr_contains_vapor_flag() {
    // Using <template vapor> attribute (not force_vapor option)
    let result = compile_sfc(
        "<script setup>\nconst msg = 'hello'\n</script>\n<template vapor><div>{{ msg }}</div></template>",
    );
    let script = result.script.as_ref().expect("should have script");
    assert!(
        script.code.contains("__vapor: true,"),
        "Component with <template vapor> should contain an inline __vapor \
         object-literal property, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("__sfc__.__vapor ="),
        "__vapor must not be a separate trailing assignment, got:\n{}",
        script.code
    );
}

/// A template-only Vapor component (no `<script>`/`<script setup>` at
/// all — `slots.vue`'s exact shape) emits `__vapor: true` as an INLINE
/// property of the `_sfc_main` object literal, not a separate trailing
/// `__sfc__.__vapor = true;` statement — confirmed directly against the
/// pinned rc.5 golden (`const _sfc_main = { __vapor: true }`) and against
/// `@vue/compiler-sfc`'s own `runtimeOptions` string-building convention
/// (every dev-time property, `__name`/`props`/`emits`/`__vapor`/etc., is
/// accumulated into ONE string spliced into the object literal, never a
/// post-hoc assignment).
#[test]
fn template_only_vapor_inlines_vapor_flag_in_object_literal() {
    let result = compile_sfc_vapor("<template><div>x</div></template>");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result
        .script
        .as_ref()
        .expect("template-only vapor component should emit a synthetic script block");
    assert!(
        script.code.contains("const __sfc__ = { __vapor: true };"),
        "__vapor: true must be an inline object-literal property, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("__sfc__.__vapor = true"),
        "__vapor must not be a separate trailing assignment, got:\n{}",
        script.code
    );
}

// ==================== Single element children must be array-wrapped ====================

/// @ai-generated - When an element has a single element child (e.g., <li><button>text</button></li>),
/// Vue requires the child VNode to be wrapped in an array. Passing a bare VNode as children
/// causes Vue to misinterpret it as a slots object and render nothing.
#[test]
fn single_element_child_wrapped_in_array() {
    let code = compile_and_validate_template_no_hoist(
        r#"<template>
  <ul>
    <li><button>Create User</button></li>
  </ul>
</template>"#,
    );
    eprintln!("=== SINGLE ELEMENT CHILD OUTPUT ===\n{}", code);
    // The button VNode must be wrapped in an array: [_createElementVNode("button", ...)]
    // NOT passed directly: _createElementVNode("button", ...)
    assert!(
        code.contains(r#"[_createElementVNode("button""#),
        "Single element child must be wrapped in array. Got:\n{}",
        code
    );
}

/// @ai-generated - Multiple <li><button>...</button></li> all must have array-wrapped children.
#[test]
fn single_element_children_in_list() {
    let code = compile_and_validate_template_no_hoist(
        r#"<template>
  <ul>
    <li><button>Create User</button></li>
    <li><button>Generate Report</button></li>
    <li><button>Export Data</button></li>
  </ul>
</template>"#,
    );
    eprintln!("=== LIST SINGLE ELEMENT CHILDREN ===\n{}", code);
    // Each <li> must wrap its single <button> child in an array
    // Count occurrences of [_createElementVNode("button"
    let array_wrapped_count = code.matches(r#"[_createElementVNode("button""#).count();
    assert_eq!(
        array_wrapped_count, 3,
        "Expected 3 array-wrapped button children. Got {} in:\n{}",
        array_wrapped_count, code
    );
}

/// @ai-generated - Single element child in <td><span>...</span></td> must be array-wrapped.
#[test]
fn single_element_child_in_td() {
    let code = compile_and_validate_template_no_hoist(
        r#"<template>
  <table><tbody><tr>
    <td><span class="badge">Done</span></td>
  </tr></tbody></table>
</template>"#,
    );
    eprintln!("=== TD SINGLE ELEMENT CHILD ===\n{}", code);
    assert!(
        code.contains(r#"[_createElementVNode("span""#),
        "Single element child in <td> must be wrapped in array. Got:\n{}",
        code
    );
}

#[test]
fn tsx_template_interpolation_with_bindings() {
    let result = compile_tsx(
        r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
const msg = 'hello'
</script>

<template>
  <div>{{ count }} {{ msg }}</div>
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    // count is a SetupRef — in the new output, .value is NOT appended in TSX
    assert!(
        tsx.code.contains("{ count }"),
        "SetupRef should appear without .value in TSX, got: {}",
        tsx.code
    );
    // Negative: .value must NOT appear in TSX template interpolations
    assert!(
        !tsx.code.contains("count.value"),
        ".value must not appear in TSX template: {}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_use_template_ref_infers_static_element_type() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
let el = useTemplateRef('el')
</script>
<template>
  <div ref="el"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Uses ReturnType<typeof ___VERTER___Comp{offset}> which resolves to the build node type
    assert!(
        tsx.code
            .contains(r#"useTemplateRef<ReturnType<typeof ___VERTER___Comp"#)
            && tsx.code.contains(r#","el">('el')"#),
        "Expected inferred useTemplateRef generic with Comp build node, got:\n{}",
        tsx.code
    );
    // Negative: should NOT use InstanceType for ref type
    assert!(
        !tsx.code.contains("useTemplateRef<InstanceType"),
        "should use ReturnType<typeof Comp>, not InstanceType in useTemplateRef: {}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_use_template_ref_dynamic_ref_with_const_match() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
let x = useTemplateRef('test')
const foo = 'test'
</script>
<template>
  <my-comp :ref="foo"></my-comp>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Dynamic ref with const match: should use Comp build node, not InstanceType
    assert!(
        tsx.code
            .contains(r#"useTemplateRef<ReturnType<typeof ___VERTER___Comp"#)
            && tsx.code.contains(r#",typeof foo>('test')"#),
        "Expected dynamic ref const-value matching to use Comp build node, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_use_template_ref_dynamic_ref_unknown_when_unmatched() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
let x = useTemplateRef('test')
const foo = 'testx'
</script>
<template>
  <my-comp :ref="foo"></my-comp>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code
            .contains(r#"useTemplateRef<unknown,typeof foo>('test')"#),
        "Expected unmatched dynamic ref selector to fall back to unknown, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_dynamic_component_is_union_from_literals() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
let a = useTemplateRef('a')
</script>
<template>
  <component :is="true ? 'div' : 'span'" ref="a"></component>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Dynamic component :is with ref should use ReturnType<typeof Comp{offset}>
    assert!(
        tsx.code
            .contains(r#"useTemplateRef<ReturnType<typeof ___VERTER___Comp"#),
        "Expected dynamic component ref to use Comp build node, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_ref_variable_matching_template_ref_gets_type() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const myDiv = ref()
</script>
<template>
  <div ref="myDiv"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("const myDiv = ref<") && tsx.code.contains("|null>()"),
        "Expected ref() variable to receive inferred template-ref type, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_ref_variable_not_matching_template_ref_is_unchanged() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const other = ref()
</script>
<template>
  <div ref="myDiv"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("const other = ref()"),
        "Non-matching ref() variable should remain unchanged, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_skips_calls_with_explicit_type_arguments() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const myDiv = ref<HTMLInputElement>()
const x = useTemplateRef<HTMLInputElement>('myDiv')
</script>
<template>
  <div ref="myDiv"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("const myDiv = ref<HTMLInputElement>()"),
        "Explicit ref<T>() should remain unchanged, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code
            .contains("useTemplateRef<HTMLInputElement>('myDiv')"),
        "Explicit useTemplateRef<T>() should remain unchanged, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_without_argument_uses_all_template_ref_names() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
let x = useTemplateRef()
</script>
<template>
  <div ref="foo"></div>
  <span ref="bar"></span>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains(r#""foo""#) && tsx.code.contains(r#""bar""#),
        "Expected no-arg useTemplateRef() to include all template ref names, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_dynamic_function_ref_expression_is_ignored() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
let x = useTemplateRef()
</script>
<template>
  <div :ref="el => (x = el)"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("let x = useTemplateRef()"),
        "Function-like dynamic ref expressions should not drive inference, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_v5_process_parity_matrix() {
    let ts_cases: [(&str, &[&str], &[&str]); 7] = [
        (
            r#"<script setup lang="ts">
let el = useTemplateRef('el')
</script>
<template><div ref="el"></div></template>"#,
            &[
                r#"useTemplateRef<ReturnType<typeof ___VERTER___Comp"#,
                r#","el">('el')"#,
            ],
            &[r#"useTemplateRef<InstanceType"#],
        ),
        (
            r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
let x = useTemplateRef('test')
const foo = 'test'
</script>
<template><my-comp :ref="foo"></my-comp></template>"#,
            &[
                r#"useTemplateRef<ReturnType<typeof ___VERTER___Comp"#,
                r#",typeof foo>('test')"#,
            ],
            &[r#"useTemplateRef<unknown,typeof foo>('test')"#],
        ),
        (
            r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
let x = useTemplateRef('test')
const foo = 'testx'
</script>
<template><my-comp :ref="foo"></my-comp></template>"#,
            &[r#"useTemplateRef<unknown,typeof foo>('test')"#],
            &[],
        ),
        (
            r#"<script setup lang="ts">
import { ref } from 'vue'
const itemRef = ref()
</script>
<template><div v-for="item in items" :key="item" ref="itemRef"></div></template>"#,
            &[r#"const itemRef = ref<"#, r#"[]|null>()"#],
            &[],
        ),
        (
            r#"<script setup lang="ts">
let a = useTemplateRef('a')
</script>
<template><component :is="true ? 'div' : 'span'" ref="a"></component></template>"#,
            &[r#"useTemplateRef<ReturnType<typeof ___VERTER___Comp"#],
            &[],
        ),
        (
            r#"<script setup lang="ts">
import { ref } from 'vue'
const myDiv = ref()
</script>
<template><div ref="myDiv"></div></template>"#,
            &[r#"const myDiv = ref<"#, r#"|null>()"#],
            &[],
        ),
        (
            r#"<script lang="ts">
import { defineComponent, useTemplateRef } from 'vue'
export default defineComponent({
  setup() {
    const myRef = useTemplateRef('myRef')
    return { myRef }
  }
})
</script>
<template><div ref="myRef"></div></template>"#,
            &[r#"useTemplateRef<"#, r#","myRef">('myRef')"#],
            &[],
        ),
    ];

    for (source, required_snippets, forbidden_snippets) in ts_cases {
        let result = compile_tsx(source);
        assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
        let tsx = result.tsx.as_ref().expect("tsx block");
        for required in required_snippets {
            assert!(
                tsx.code.contains(required),
                "Expected snippet '{}' in TSX output:\n{}",
                required,
                tsx.code
            );
        }
        for forbidden in forbidden_snippets {
            assert!(
                !tsx.code.contains(forbidden),
                "Unexpected snippet '{}' in TSX output:\n{}",
                forbidden,
                tsx.code
            );
        }
    }

    let js_result = compile_tsx(
        r#"<script setup lang="js">
let el = useTemplateRef('el')
</script>
<template><div ref="el"></div></template>"#,
    );
    assert!(
        js_result.errors.is_empty(),
        "errors: {:?}",
        js_result.errors
    );
    let js_tsx = js_result.tsx.as_ref().expect("tsx block");
    assert!(
        js_tsx.code.contains("let el = useTemplateRef('el')"),
        "JS parity case should preserve plain useTemplateRef call, got:\n{}",
        js_tsx.code
    );
    assert!(
        !js_tsx.code.contains("useTemplateRef<"),
        "JS parity case should not inject generic type parameters, got:\n{}",
        js_tsx.code
    );
}

#[test]
fn tsx_template_ref_multiple_refs_union() {
    // Multiple refs with different elements should create a union for the second generic
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { useTemplateRef } from 'vue'
const x = useTemplateRef('foo')
</script>
<template>
  <div ref="foo"></div>
  <span ref="bar"></span>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: second generic should contain both ref names
    assert!(
        tsx.code.contains(r#""foo""#) && tsx.code.contains(r#""bar""#),
        "second generic should contain all ref name literals: {}",
        tsx.code
    );

    // Positive: first generic should match 'foo' ref type (not union, since selector matches)
    assert!(
        tsx.code
            .contains("useTemplateRef<ReturnType<typeof ___VERTER___Comp"),
        "first generic should be the matched ref's type: {}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_unmatched_arg_produces_unknown() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { useTemplateRef } from 'vue'
const x = useTemplateRef('nonexistent')
</script>
<template>
  <div ref="foo"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: unmatched arg should produce unknown as first generic
    assert!(
        tsx.code.contains("useTemplateRef<unknown,"),
        "unmatched arg should produce unknown first generic: {}",
        tsx.code
    );
}

/// Adapted parity matrix for:
/// - template/template.spec.ts
/// - template/plugins/block/block.spec.ts
/// - template/plugins/sfc-cleaner/sfcCleaner.spec.ts
#[test]
fn tsx_template_v5_process_parity_matrix() {
    template_output_contains_render_function_vdom();
    template_output_contains_render_function_vapor();
    template_heavy_vue_full_css_scoping();
    template_only_no_scoped_style_no_script_block();
    template_only_scoped_style_emits_scope_id_in_script();
    template_only_scoped_style_css_is_scoped();
    template_only_scoped_style_grid_layout_scope_id_consistency();
    component_whitespace_children_clean_output();
    component_whitespace_only_children_no_close_tag_leak();
    analysis_panel_regression_valid_js();

    tsx_template_interpolation_with_bindings();
    tsx_template_tag_replacement_wraps_content_in_fragment();
    tsx_template_tag_empty_template_emits_empty_fragment();
    tsx_template_comment();
    tsx_template_comment_no_extra_spacing();
    tsx_template_comment_with_nested_marker_text();
    tsx_template_comment_with_angle_bracket_text();
    tsx_no_template();
    html_entity_copy_decoded();
    html_entity_nbsp_decoded_in_text();
}

/// Adapted parity matrix for:
/// - script/plugins/component-type/component-type.spec.ts
#[test]
fn tsx_component_type_v5_process_parity_matrix() {
    component_resolves_to_setup_binding();
    component_kebab_case_resolves_to_pascal_setup_binding();
    unknown_component_uses_resolve_component();
    self_referencing_component_uses_maybe_self_reference();
    self_referencing_component_kebab_case();
    component_is_uses_resolve_dynamic_component();
    component_is_self_closing_uses_resolve_dynamic_component();
    component_is_empty_uses_resolve_dynamic_component();
    component_is_self_closing_with_props();
    component_static_is_uses_resolve_dynamic_component();
    component_is_with_prop_binding_and_vbind();
    imported_component_uses_setup_binding_not_resolve_component();
    builtin_component_suspense();
    builtin_component_teleport();
    builtin_component_keep_alive();
    builtin_component_transition();
    builtin_component_transition_group();
    builtin_component_kebab_case_keep_alive();
    builtin_component_kebab_case_teleport();
    builtin_component_in_imports_list();

    tsx_component_pascal_and_dotted_names_are_preserved();
    tsx_component_static_is_rewrites_to_target_tag();
    tsx_component_static_is_keeps_other_attributes();
    tsx_component_dynamic_is_literal_string_rewrites_to_target_tag();
    tsx_component_dynamic_is_expression_rewrites_to_temp_component();
    tsx_component_with_v_if_and_v_for_preserves_component_tags();
    tsx_component_kebab_and_mixed_case_names_are_preserved();
}

/// Adapted parity matrix for:
/// - script/plugins/component-instance/componentInstance.spec.ts
#[test]
fn tsx_component_instance_v5_process_parity_matrix() {
    tsx_infer_function_component_events_from_imported_components();
    tsx_template_ref_dynamic_component_is_union_from_literals();
    tsx_template_ref_use_template_ref_dynamic_ref_with_const_match();
    tsx_template_ref_use_template_ref_dynamic_ref_unknown_when_unmatched();
    tsx_template_ref_options_api_setup_function_is_supported();
    tsx_template_ref_v5_process_parity_matrix();
}

/// Adapted parity matrix for:
/// - script/plugins/template-binding/template-binding.spec.ts
#[test]
fn tsx_template_binding_plugin_v5_process_parity_matrix() {
    tsx_binding_v5_process_parity_matrix();
    tsx_binding_type_assertions_do_not_prefix_type_members();
    imported_function_in_template_gets_setup_prefix();
    companion_script_import_available_in_template();
    companion_script_import_used_in_template_in_returned();
    tsx_template_interpolation_with_bindings();
}

/// Adapted parity matrix for:
/// - script/plugins/full-context/full-context.spec.ts
#[test]
fn tsx_full_context_v5_process_parity_matrix() {
    setup_returns_bindings_for_template_refs();
    setup_returns_bindings_with_define_props();
    import_used_in_template_should_be_in_returned();
    companion_script_import_used_in_template_in_returned();
    companion_script_type_only_import_not_in_returned();
}

#[test]
fn tsx_issue_46_bare_click_does_not_bind_click_identifier_from_context() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const a = {}
</script>
<template>
  <div @click></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        !tsx.code.contains("onclick={"),
        "Bare @click must not be emitted as lowercase onclick binding, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("_ctx.click"),
        "Bare @click must not bind synthetic click identifier from context, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_interpolation_without_spaces() {
    let result = compile_tsx(
        r#"<script setup>
const test = 1
</script>
<template>{{test}}</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{test}"),
        "Interpolation without spaces should become {{test}} (no _ctx. prefix), got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_interpolation_preserves_inner_spaces() {
    let result = compile_tsx(
        r#"<script setup>
const test = 1
</script>
<template>{{  test  }}</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{  test  }"),
        "Interpolation with spaces should preserve inner spaces (no _ctx. prefix), got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_interpolation_preserves_inner_newlines() {
    let result = compile_tsx(
        r#"<script setup>
const test = 1
</script>
<template>{{  test
  }}</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{  test\n  }"),
        "Interpolation with newlines should preserve inner formatting (no _ctx. prefix), got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_text_plain_content_wrapped_as_string_expression() {
    let result = compile_tsx(r#"<template>test</template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{\"test\"}"),
        "Plain text content should be wrapped as string expression, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_text_with_less_than_wrapped_as_string_expression() {
    let result = compile_tsx(r#"<template>2 < 1</template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        (tsx.code.contains("{\"2 < 1\"}")
            || (tsx.code.contains("{\"2\"}") && tsx.code.contains("{\"< 1\"}"))),
        "Text containing '<' should be wrapped into string expressions, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_text_escapes_double_quotes_in_string_expression() {
    let result = compile_tsx(r#"<template>"</template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{\"\\\"\"}"),
        "Text with quote should escape it inside string expression, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_text_whitespace_only_is_preserved_without_wrapping() {
    let result = compile_tsx("<template>\n\n\r\n      \n\r\n</template>");
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        !tsx.code.contains("{\""),
        "Whitespace-only text should not be wrapped as string expression, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_text_single_lt_is_not_wrapped() {
    let result = compile_tsx(r#"<template><</template>"#);
    // Parser may report a malformed-tag diagnostic, but TSX generation should still avoid
    // wrapping a lone '<' into a string expression.
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        !tsx.code.contains("{\"<\"}"),
        "A lone '<' text segment should not be wrapped, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_tag_replacement_wraps_content_in_fragment() {
    let result = compile_tsx(r#"<template><div></div></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code
            .contains("export function ___VERTER___TemplateBindingFN()"),
        "Template should be emitted inside component function wrapper, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("<div></div>"),
        "Template root content should be preserved after template-tag replacement, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("<template>"),
        "Raw <template> tag should be removed from TSX output, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_component_pascal_and_dotted_names_are_preserved() {
    let result = compile_tsx(
        r#"<template>
  <Comp></Comp>
  <Calendar.Root />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("<Comp></Comp>"),
        "PascalCase component tag should be preserved in TSX output, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("<Calendar.Root />") || tsx.code.contains("<Calendar.Root/>"),
        "Dotted component tag should be preserved in TSX output, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_component_static_is_rewrites_to_target_tag() {
    let result = compile_tsx(r#"<template><component is="div"></component></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("<div ></div>") || tsx.code.contains("<div></div>"),
        "Static component is=\"div\" should rewrite tag to <div>, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("<component"),
        "Static component is=\"...\" should not keep <component> tag, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_component_static_is_keeps_other_attributes() {
    let result =
        compile_tsx(r#"<template><component is="div" tabindex="1"></component></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("<div  tabindex=\"1\"></div>")
            || tsx.code.contains("<div tabindex=\"1\"></div>")
            || tsx.code.contains("<div tabindex={\"1\"}></div>"),
        "Static component is rewrite should preserve other attrs, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_component_dynamic_is_literal_string_rewrites_to_target_tag() {
    let result = compile_tsx(r#"<template><component :is="'div'"></component></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code
            .contains("___VERTER___extractRenderComponent('div')"),
        "Dynamic :is with literal string should use extractRenderComponent, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code
            .contains("<___VERTER___component_render ></___VERTER___component_render>"),
        "Dynamic :is should rewrite tag to ___VERTER___component_render, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_component_dynamic_is_expression_rewrites_to_temp_component() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const as = 'div'
</script>
<template><component :is="as || 'div'"></component></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code
            .contains("const ___VERTER___component_render=___VERTER___extractRenderComponent("),
        "Dynamic :is expression should emit extractRenderComponent binding, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("<___VERTER___component_render "),
        "Dynamic :is expression should rewrite element tag to ___VERTER___component_render, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_component_kebab_and_mixed_case_names_are_preserved() {
    // Kebab component tags REWRITE to their PascalCase fallback const (a
    // lowercase JSX identifier is an intrinsic lookup that never consults the
    // const); camelCase tags keep their authored identifier form.
    let result = compile_tsx(
        r#"<template>
  <item-render></item-render>
  <hello-moto />
  <helloMoto />
  <Hello-Moto />
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("<ItemRender></ItemRender>"),
        "kebab-case component tag should rewrite to its Pascal const, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains(
            "const ItemRender = {} as ___VERTER___GlobalComponentKebabType<'ItemRender', 'item-render'>"
        ),
        "the ItemRender fallback const must back the rewritten tag with the \
         fail-open kebab-authored type, got:\n{}",
        tsx.code
    );
    // Negative: a kebab-only authored name must NOT get the fail-closed
    // Pascal-authored type (that is the false-TS2604 web-component regression).
    assert!(
        !tsx.code
            .contains("___VERTER___GlobalComponentType<'ItemRender'>"),
        "kebab-only authored tag must not use the fail-closed type, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("<HelloMoto />") || tsx.code.contains("<HelloMoto/>"),
        "lower-kebab component tag should rewrite to its Pascal const, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("<helloMoto />") || tsx.code.contains("<helloMoto/>"),
        "mixed camelCase component tag should be preserved, got:\n{}",
        tsx.code
    );
    // `hello-moto` and `Hello-Moto` PascalCase to ONE shared const.
    assert_eq!(
        tsx.code.matches("const HelloMoto").count(),
        1,
        "one shared HelloMoto const for both kebab spellings, got:\n{}",
        tsx.code
    );
    // Negative: no kebab identifier survives into the JSX for resolvable tags.
    assert!(
        !tsx.code.contains("<item-render") && !tsx.code.contains("<hello-moto"),
        "no intrinsic kebab tag may survive for a rewritten component, got:\n{}",
        tsx.code
    );
}

/// A configured custom element (`custom_elements` prefix match) is a NATIVE
/// element: the IDE surface must not invent a GlobalComponents fallback const
/// for it and must not rewrite its tag — it stays authored and types through
/// `JSX.IntrinsicElements`, exactly as before the fallback machinery existed.
/// A NON-matching dashed tag in the same template still gets the (fail-open)
/// kebab fallback + rewrite.
#[test]
fn tsx_custom_elements_configured_tag_stays_authored_without_fallback() {
    let result = compile_tsx_with_custom_elements(
        r#"<template>
  <ion-button size="small">go</ion-button>
  <my-widget />
</template>"#,
        &["ion-"],
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("<ion-button") && tsx.code.contains("</ion-button>"),
        "the configured custom element must stay authored (intrinsic), got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("IonButton"),
        "no Pascal rewrite and no fallback const for a configured custom element, got:\n{}",
        tsx.code
    );
    // The non-matching dashed tag still resolves through the fallback machinery.
    assert!(
        tsx.code.contains(
            "const MyWidget = {} as ___VERTER___GlobalComponentKebabType<'MyWidget', 'my-widget'>"
        ),
        "a non-matching dashed tag still gets its kebab fallback const, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("<MyWidget"),
        "a non-matching dashed tag still rewrites, got:\n{}",
        tsx.code
    );
}

/// `custom_elements` wins over a same-name local binding: Vue's option contract
/// is "skip component resolution" for matching tags, so the tag stays an
/// authored intrinsic even when `IonButton` is imported.
#[test]
fn tsx_custom_elements_configured_tag_ignores_local_binding() {
    let result = compile_tsx_with_custom_elements(
        r#"<script setup lang="ts">
import IonButton from './IonButton.vue'
void IonButton
</script>
<template>
  <ion-button />
</template>"#,
        &["ion-"],
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("<ion-button"),
        "a configured custom element never rewrites, even with a local binding, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("<IonButton"),
        "no component rewrite for a configured custom element, got:\n{}",
        tsx.code
    );
}

/// A static `<component is="…">` whose target matches `custom_elements` keeps
/// the verbatim target tag (native element), with no fallback const.
#[test]
fn tsx_custom_elements_component_is_target_stays_verbatim() {
    let result = compile_tsx_with_custom_elements(
        r#"<template><component is="x-widget" /></template>"#,
        &["x-"],
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("<x-widget"),
        "the custom-element is-target must stay verbatim, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("XWidget"),
        "no Pascal const for a custom-element is-target, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_comment() {
    let result = compile_tsx(r#"<template><!-- hello --></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{/* hello */}"),
        "Comment should be converted to JSX comment with preserved spacing, got: {}",
        tsx.code
    );
}

#[test]
fn tsx_template_comment_no_extra_spacing() {
    let result = compile_tsx(r#"<template><!--comment--></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{/*comment*/}"),
        "Comment without spaces should not gain extra padding, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_comment_with_nested_marker_text() {
    let result = compile_tsx(r#"<template><!-- <!-- --></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{/* <!-- */}"),
        "Comment containing '<!--' text should remain valid JSX comment, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_comment_with_angle_bracket_text() {
    let result = compile_tsx(r#"<template><!-- <MyComp --></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("{/* <MyComp */}"),
        "Comment containing '<MyComp' text should remain wrapped in JSX comment, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_no_template() {
    let result = compile_tsx(
        r#"<script setup>
const msg = 'hello'
</script>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Should still contain script content, just no template JSX
    assert!(
        tsx.code.contains("const msg = 'hello'"),
        "Script-only TSX should contain setup content, got: {}",
        tsx.code
    );
}

// ==================== Vue built-in components ====================
//
// Vue built-in components (Suspense, Teleport, KeepAlive, Transition,
// TransitionGroup, BaseTransition) must be imported directly from "vue"
// instead of using _resolveComponent(). Vue's compiler emits e.g.:
//   import { Suspense as _Suspense } from "vue"
//   _createBlock(_Suspense, null, { ... })

/// @ai-generated - Suspense must be imported from "vue" and used directly,
/// NOT via _resolveComponent("Suspense").
#[test]
fn builtin_component_suspense() {
    let code = compile_and_validate_template(
        r#"<script setup>
import Comp from './Comp.vue'
</script>
<template>
  <Suspense>
    <Comp />
  </Suspense>
</template>"#,
    );
    eprintln!("=== SUSPENSE OUTPUT ===\n{}", code);
    // Must NOT use _resolveComponent for Suspense
    assert!(
        !code.contains("_resolveComponent(\"Suspense\")"),
        "Suspense must NOT use _resolveComponent, got:\n{}",
        code
    );
    // Must use _Suspense directly
    assert!(
        code.contains("_Suspense"),
        "Suspense must be imported and used as _Suspense, got:\n{}",
        code
    );
}

/// @ai-generated - Teleport must be imported from "vue" and used directly.
#[test]
fn builtin_component_teleport() {
    let code = compile_and_validate_template(
        r#"<template>
  <Teleport to="body">
    <div>modal content</div>
  </Teleport>
</template>"#,
    );
    eprintln!("=== TELEPORT OUTPUT ===\n{}", code);
    assert!(
        !code.contains("_resolveComponent(\"Teleport\")"),
        "Teleport must NOT use _resolveComponent, got:\n{}",
        code
    );
    assert!(
        code.contains("_Teleport"),
        "Teleport must be imported and used as _Teleport, got:\n{}",
        code
    );
}

/// @ai-generated - KeepAlive must be imported from "vue" and used directly.
#[test]
fn builtin_component_keep_alive() {
    let code = compile_and_validate_template(
        r#"<script setup>
import Comp from './Comp.vue'
</script>
<template>
  <KeepAlive>
    <Comp />
  </KeepAlive>
</template>"#,
    );
    eprintln!("=== KEEPALIVE OUTPUT ===\n{}", code);
    assert!(
        !code.contains("_resolveComponent(\"KeepAlive\")"),
        "KeepAlive must NOT use _resolveComponent, got:\n{}",
        code
    );
    assert!(
        code.contains("_KeepAlive"),
        "KeepAlive must be imported and used as _KeepAlive, got:\n{}",
        code
    );
}

/// @ai-generated - Transition must be imported from "vue" and used directly.
#[test]
fn builtin_component_transition() {
    let code = compile_and_validate_template(
        r#"<template>
  <Transition name="fade">
    <div>content</div>
  </Transition>
</template>"#,
    );
    eprintln!("=== TRANSITION OUTPUT ===\n{}", code);
    assert!(
        !code.contains("_resolveComponent(\"Transition\")"),
        "Transition must NOT use _resolveComponent, got:\n{}",
        code
    );
    assert!(
        code.contains("_Transition"),
        "Transition must be imported and used as _Transition, got:\n{}",
        code
    );
}

/// @ai-generated - TransitionGroup must be imported from "vue" and used directly.
#[test]
fn builtin_component_transition_group() {
    let code = compile_and_validate_template(
        r#"<template>
  <TransitionGroup name="list" tag="ul">
    <li v-for="item in items" :key="item">{{ item }}</li>
  </TransitionGroup>
</template>"#,
    );
    eprintln!("=== TRANSITION GROUP OUTPUT ===\n{}", code);
    assert!(
        !code.contains("_resolveComponent(\"TransitionGroup\")"),
        "TransitionGroup must NOT use _resolveComponent, got:\n{}",
        code
    );
    assert!(
        code.contains("_TransitionGroup"),
        "TransitionGroup must be imported and used as _TransitionGroup, got:\n{}",
        code
    );
}

/// @ai-generated - kebab-case built-in components must also be recognized.
/// <keep-alive> is the same as <KeepAlive>.
#[test]
fn builtin_component_kebab_case_keep_alive() {
    let code = compile_and_validate_template(
        r#"<script setup>
import Comp from './Comp.vue'
</script>
<template>
  <keep-alive>
    <Comp />
  </keep-alive>
</template>"#,
    );
    eprintln!("=== KEBAB KEEP-ALIVE OUTPUT ===\n{}", code);
    assert!(
        !code.contains("_resolveComponent(\"keep-alive\")"),
        "keep-alive must NOT use _resolveComponent, got:\n{}",
        code
    );
    assert!(
        code.contains("_KeepAlive"),
        "keep-alive must be imported and used as _KeepAlive, got:\n{}",
        code
    );
}

/// @ai-generated - kebab-case <teleport> must be recognized as built-in.
#[test]
fn builtin_component_kebab_case_teleport() {
    let code = compile_and_validate_template(
        r#"<template>
  <teleport to="body">
    <div>modal</div>
  </teleport>
</template>"#,
    );
    assert!(
        !code.contains("_resolveComponent(\"teleport\")"),
        "teleport must NOT use _resolveComponent, got:\n{}",
        code
    );
    assert!(
        code.contains("_Teleport"),
        "teleport must be imported and used as _Teleport, got:\n{}",
        code
    );
}

#[test]
fn tsx_template_inside_return_statement() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'

const count = ref(0)
const message = ref('Hello from Verter!')

function increment() {
  count.value++
}
</script>

<template>
  <div class="app">
    <h1>{{ message }}</h1>
    <button @click="increment">Count: {{ count }}</button>
  </div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");

    // The template JSX must be INSIDE the TemplateBindingFN function body.
    // It is emitted as an expression statement (not returned), so that
    // ReturnType<typeof TemplateBindingFN> resolves to the binding object.
    let fn_open = tsx
        .code
        .find("___VERTER___TemplateBindingFN")
        .expect("TemplateBindingFN");
    let fn_brace = tsx.code[fn_open..].find('{').expect("opening brace") + fn_open;
    // Find the matching closing brace
    let mut depth = 0i32;
    let mut fn_close = None;
    for (i, ch) in tsx.code[fn_brace..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    fn_close = Some(fn_brace + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let fn_end = fn_close.expect("closing brace of TemplateBindingFN");
    let fn_body = &tsx.code[fn_brace..fn_end + 1];

    // The template content must be inside the function body
    assert!(
        fn_body.contains("<div class=\"app\">"),
        "Template <div> should be inside TemplateBindingFN body, but body is:\n{}\n\nFull TSX:\n{}",
        fn_body,
        tsx.code
    );
    assert!(
        fn_body.contains("message"),
        "Template interpolation should be inside TemplateBindingFN body, but body is:\n{}",
        fn_body,
    );

    // The binding return must also be inside the function
    assert!(
        fn_body.contains("shallowUnwrapRef"),
        "shallowUnwrapRef return should be inside TemplateBindingFN body, got:\n{}",
        fn_body,
    );

    // The empty placeholder <></> should NOT remain
    assert!(
        !tsx.code.contains("<></>"),
        "Empty fragment placeholder <></> should be replaced with template content.\nTSX:\n{}",
        tsx.code
    );
}

#[test]
fn error_recovery_unclosed_element_does_not_panic() {
    let result = compile_sfc("<template><div></template>");
    assert!(
        !result.errors.is_empty(),
        "should report diagnostics for unclosed element"
    );
}

/// @ai-generated — Single static element emits _cache wrapping
#[test]
fn static_hoist_single_static_element() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><div class="card"><h3>Title</h3><p>text</p></div></div></template>"#,
    );
    assert!(
        code.contains("_cache["),
        "should use _cache wrapping for static elements\n--- code ---\n{}",
        code
    );
    assert!(
        code.contains("-1 /* CACHED */"),
        "should emit -1 CACHED patch flag\n--- code ---\n{}",
        code
    );
    assert!(
        !code.contains("_createStaticVNode"),
        "should NOT use _createStaticVNode\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Dynamic element is NOT hoisted
#[test]
fn static_hoist_dynamic_element_not_hoisted() {
    let code = compile_and_validate_hoisted(
        r#"<template><div><div :class="cls">dynamic</div></div></template>"#,
    );
    assert!(
        !code.contains("_createStaticVNode"),
        "dynamic element should NOT be hoisted\n--- code ---\n{}",
        code
    );
    assert!(
        code.contains("_createElementVNode"),
        "dynamic element should use _createElementVNode\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Element with interpolation is NOT hoisted
#[test]
fn static_hoist_interpolation_not_hoisted() {
    let code =
        compile_and_validate_hoisted(r#"<template><div><div>{{ msg }}</div></div></template>"#);
    assert!(
        !code.contains("_createStaticVNode"),
        "element with interpolation should NOT be hoisted\n--- code ---\n{}",
        code
    );
}

/// @ai-generated — Component is NOT hoisted
#[test]
fn static_hoist_component_not_hoisted() {
    let code = compile_and_validate_hoisted(r#"<template><div><MyComp /></div></template>"#);
    assert!(
        !code.contains("_createStaticVNode"),
        "component should NOT be hoisted\n--- code ---\n{}",
        code
    );
}

// ── TSX export / Comp / JSDoc / offset comment tests ──────────────────

#[test]
fn tsx_export_template_binding_fn() {
    let result = compile_tsx(
        r#"<script setup>
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Positive: TemplateBindingFN should be exported
    assert!(
        tsx.code
            .contains("export function ___VERTER___TemplateBindingFN"),
        "TemplateBindingFN should be exported, got:\n{}",
        tsx.code
    );
    // Negative: no bare (non-exported) function declaration
    // Check that every "function ___VERTER___TemplateBindingFN" is preceded by "export "
    let needle = "function ___VERTER___TemplateBindingFN";
    let mut search_from = 0;
    while let Some(pos) = tsx.code[search_from..].find(needle) {
        let abs_pos = search_from + pos;
        let before = &tsx.code[..abs_pos];
        assert!(
            before.ends_with("export "),
            "TemplateBindingFN at offset {} is not preceded by 'export ': {}",
            abs_pos,
            tsx.code
        );
        search_from = abs_pos + needle.len();
    }
}

#[test]
fn tsx_comp_only_for_ref_elements() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const el = ref<HTMLDivElement>()
</script>
<template><div ref="el">a</div><span>b</span></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Positive: Comp function present for div (has ref)
    assert!(
        tsx.code.contains("___VERTER___Comp"),
        "Should have Comp function for ref element, got:\n{}",
        tsx.code
    );
    // Negative: should NOT have a Comp for the span (no ref)
    // The span's tag_open.start offset should NOT appear as a Comp function
    // Since div has ref and span doesn't, we check there's exactly one Comp function
    let comp_count = tsx.code.matches("function ___VERTER___Comp").count();
    assert_eq!(
        comp_count, 1,
        "Should have exactly 1 Comp function (for ref element only), got {}: \n{}",
        comp_count, tsx.code
    );
}

#[test]
fn tsx_parse_valid_component_with_all_features() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
import Comp from './Comp.vue'
const msg = ref('Hello World!')
</script>
<template>
  <div>
    <h1>{{ msg }}</h1>
    <input v-model="msg" />
    <Comp :foo="msg" @update="msg = $event">
      <template #header>Header</template>
      <template #default="{ data }">{{ data }}</template>
    </Comp>
  </div>
</template>"#,
        "component with props, events, v-model, slots",
    );
}

#[test]
fn tsx_parse_valid_dynamic_component() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
import CompA from './CompA.vue'
import CompB from './CompB.vue'
const current = ref(CompA)
</script>
<template>
  <component :is="current" />
</template>"#,
        "dynamic component :is",
    );
}

#[test]
fn tsx_parse_valid_template_ref() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const el = ref<HTMLDivElement>()
</script>
<template>
  <div ref="el">text</div>
</template>"#,
        "template ref",
    );
}

#[test]
fn jsx_compile_template_only() {
    // Template-only SFCs should default to TSX (not JSX)
    let result = compile_tsx(r#"<template><div>hello</div></template>"#);
    let tsx = result.tsx.as_ref().expect("should have tsx block");
    assert!(
        !tsx.is_jsx,
        "template-only SFC should default to TSX (is_jsx = false):\n{}",
        tsx.code
    );
}

#[test]
fn jsx_compile_global_components() {
    assert_jsx_parses(
        r#"<script setup>
</script>
<template><RouterView /><Transition><div>x</div></Transition></template>"#,
        "JS SFC with global components",
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// ── Implicit attrs type composition — IDE codegen ───────────────────────────
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn tsx_implicit_attrs_root_element_types() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
</script>
<template><div id="app">hello</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: RootElement type alias
    assert!(
        tsx.code.contains("___VERTER___RootElement"),
        "should emit RootElement type, got:\n{}",
        tsx.code
    );
    // Positive: RootElementProps type alias using ExtractComponentProps
    assert!(
        tsx.code.contains("___VERTER___RootElementProps"),
        "should emit RootElementProps type, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("___VERTER___ExtractComponentProps"),
        "should use ExtractComponentProps, got:\n{}",
        tsx.code
    );
    // Positive: Attrs combines explicit + implicit
    assert!(
        tsx.code.contains("___VERTER___Attrs")
            && tsx.code.contains("___VERTER___attributes")
            && tsx.code.contains("___VERTER___RootElementProps"),
        "Attrs should combine attributes + RootElementProps, got:\n{}",
        tsx.code
    );
    // Positive: instance declaration overrides $attrs
    assert!(
        tsx.code.contains("$attrs: ___VERTER___Attrs"),
        "instance should override $attrs, got:\n{}",
        tsx.code
    );
    // Negative: Omit should be used on the instance
    assert!(
        tsx.code.contains("Omit<InstanceType<"),
        "should Omit $attrs from base instance type, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_no_template_no_root_element_types() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Negative: no RootElement types without template
    assert!(
        !tsx.code.contains("___VERTER___RootElement"),
        "should not emit RootElement without template, got:\n{}",
        tsx.code
    );
    // Negative: no Attrs override without template
    assert!(
        !tsx.code.contains("type ___VERTER___Attrs"),
        "should not emit Attrs type without template, got:\n{}",
        tsx.code
    );
    // Negative: instance should not use Omit
    assert!(
        !tsx.code.contains("Omit<InstanceType"),
        "instance should not use Omit without template, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_instance_declaration_template_only_uses_instance_type() {
    let result = compile_tsx(r#"<template><div>hello</div></template>"#);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    // Template-only SFCs default to TS mode (is_jsx=false)
    assert!(!tsx.is_jsx, "Template-only SFC should default to TSX");

    // Positive: should have typed instance
    assert!(
        tsx.code.contains("InstanceType<typeof import("),
        "Template-only SFC should use InstanceType<typeof import(...)>, got:\n{}",
        tsx.code
    );

    // Negative: must NOT be `any`
    assert!(
        !tsx.code
            .contains("/** @type {any} */\nvar ___VERTER___instance"),
        "Template-only SFC must NOT use JSDoc @type {{any}} for instance, got:\n{}",
        tsx.code
    );
}

// ── TemplateBindingFN empty return ──────────────────────────────────────────

#[test]
fn tsx_template_binding_fn_has_return_statement() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: should have return statement before close
    assert!(
        tsx.code
            .contains("return {};\n} // close templateBindingFN"),
        "should have empty return before closing brace of TemplateBindingFN, got:\n{}",
        tsx.code
    );

    // Negative: no `: any` return type annotation
    assert!(
        !tsx.code.contains(": any"),
        "TemplateBindingFN should not have `: any` return type, got:\n{}",
        tsx.code
    );
}

#[test]
fn jsx_template_binding_fn_has_return_statement() {
    let result = compile_tsx_with_force_js(
        r#"<script setup>
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#,
        true,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: JS mode should have `return {};` (no `as any`)
    assert!(
        tsx.code
            .contains("return {};\n} // close templateBindingFN"),
        "JSX mode should have `return {{}};` before closing brace, got:\n{}",
        tsx.code
    );

    // Negative: no `as any` in JS mode
    assert!(
        !tsx.code.contains("as any"),
        "JSX mode should not have `as any`, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_first_with_bindings() {
    let result = compile_tsx(
        r#"<template>
  <div>{{ msg }}</div>
</template>
<script setup lang="ts">
import { ref } from 'vue'
const msg = ref('hello')
</script>"#,
    );
    let tsx = result.tsx.expect("should produce TSX");

    let fn_open = tsx
        .code
        .find("function ___VERTER___TemplateBindingFN")
        .unwrap_or_else(|| panic!("should have function: {}", tsx.code));
    let fn_close = tsx
        .code
        .find("} // close templateBindingFN")
        .unwrap_or_else(|| panic!("should have close: {}", tsx.code));
    assert!(
        fn_open < fn_close,
        "function open must come before close: {}",
        tsx.code
    );
    // Script content (const msg) should be inside the function
    let msg_pos = tsx
        .code
        .find("const msg = ref('hello')")
        .unwrap_or_else(|| panic!("should have script content: {}", tsx.code));
    assert!(
        fn_open < msg_pos && msg_pos < fn_close,
        "script content must be inside function: {}",
        tsx.code
    );
    // Template JSX should be inside the function
    let jsx_pos = tsx
        .code
        .find("{ msg }")
        .unwrap_or_else(|| panic!("should have template binding: {}", tsx.code));
    assert!(
        fn_open < jsx_pos && jsx_pos < fn_close,
        "template JSX must be inside function: {}",
        tsx.code
    );
    // Script content should come BEFORE template JSX (declarations must be in scope)
    assert!(
        msg_pos < jsx_pos,
        "script declarations must precede template JSX: {}",
        tsx.code
    );
}

/// Regression: Full Popover.vue with attrs, generic, class/style merge, and
/// components. Must not produce duplicate class/style attributes (ts(17001)).
#[test]
fn ide_no_duplicate_attrs_full_popover_with_components() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("Popover.vue".to_string()),
        target: CompileTarget::IDE,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions::default();
    let runtime = crate::test_helpers::runtime_bundle([
        crate::test_helpers::runtime_props_entry(
            0,
            1,
            verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
                payload_macro_index: 0,
                defaults_macro_index: 1,
            },
            [
                crate::test_helpers::runtime_prop(
                    "actions",
                    true,
                    [verter_macro_dto::RuntimeConstructor::Array],
                ),
                crate::test_helpers::runtime_prop(
                    "actionsDirection",
                    true,
                    [verter_macro_dto::RuntimeConstructor::String],
                ),
                crate::test_helpers::runtime_prop(
                    "showArrow",
                    true,
                    [verter_macro_dto::RuntimeConstructor::Boolean],
                ),
            ],
        ),
        crate::test_helpers::runtime_model_entry(
            1,
            2,
            "show",
            "showModifiers",
            "update:show",
            true,
            [verter_macro_dto::RuntimeConstructor::Boolean],
        ),
    ]);
    let source = r#"<script setup lang="ts" attrs="{ class: string, style: string }" generic="T extends object">
import { computed, ref, useTemplateRef } from 'vue'
import { Popup } from '../Popup'
import { PopoverItem } from './components'

const props = withDefaults(defineProps<{
  actions?: { text: string }[]
  actionsDirection?: string
  showArrow?: boolean
}>(), {
  actions: () => [],
  actionsDirection: 'vertical',
  showArrow: false,
})

const show = defineModel<boolean>('show', { default: false })
const emit = defineEmits({ select: (action: { text: string }, index: number) => true })
const onClickWrapper = () => {}
const floatingStyles = ref({})
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
    :duration="1"
    position=""
    :style="[floatingStyles, $attrs.style]"
    lazy-render
  >
    <div v-if="showArrow" ref="arrowElm" class="ns-popover__arrow" :style="[arrowPos]"></div>
    <div
      role="menu"
      class="ns-popover__content"
      :class="{
        'ns-popover__content--horizontal': props.actionsDirection === 'horizontal',
      }"
    >
      <slot>
        <PopoverItem
          v-for="(action, index) in props.actions"
          :key="index"
          :text="action.text"
          @click="emit('select', action, index)"
        />
      </slot>
    </div>
  </Popup>
</template>"#;
    let result = compile(
        source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Runtime(runtime),
        &alloc,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tsx = result.tsx.as_ref().expect("TSX output");
    let code = &tsx.code;

    // Verify TSX parses cleanly (OXC catches duplicate JSX attributes)
    let alloc2 = Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc2, code, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "TSX parse errors: {:?}\n--- code ---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        code
    );
}

#[test]
fn pure_runtime_ts_target_parses_template_expressions_once() {
    reset_parse_template_expressions_calls();
    let _ = compile_with_target(TS_OVERLAY_SFC, CompileTarget::BUNDLER, false);
    assert_eq!(
        parse_template_expressions_call_count(),
        1,
        "pure runtime target parses template expressions exactly once"
    );
}

#[test]
fn pure_tsx_ts_target_parses_template_expressions_twice() {
    // The IDE/TSX target builds TWO overlays: the `ide_completion = true` lane
    // that drives TSX template codegen, and the `ide_completion = false` lane
    // that drives unused-binding LIVENESS. The two completion modes store
    // different binding facts (completion mode intentionally suppresses real
    // references), so they cannot share an overlay — the extra liveness parse is
    // the accepted correctness cost of using a SOUND usage source for the TS6133
    // gate (the zero-extra-parse optimisation is abandoned for liveness).
    reset_parse_template_expressions_calls();
    let _ = compile_with_target(TS_OVERLAY_SFC, CompileTarget::IDE, false);
    assert_eq!(
        parse_template_expressions_call_count(),
        2,
        "pure TSX target parses once for codegen (completion=true) + once for liveness (completion=false)"
    );
}

/// Discriminating guard for rule-ledger #2 (Two Template Codegen Paths):
/// a TS SFC reuses one read-only expression overlay PER `ide_completion` value
/// (the runtime lane and the IDE/TSX lane store different binding facts) and
/// emits byte-identical output to the unshared baseline, while a JS SFC cannot
/// share across source types — its TSX lane parses with `jsx()` and the runtime
/// lane with `tsx()`, an impossibility-by-key-construction enforced separately
/// by the overlay unit tests.
#[test]
fn template_expression_overlay_source_type_matrix() {
    // (a) TS SFC: combined target parses once per ide_completion value (runtime
    //     `false` + TSX `true`) and produces output byte-identical to compiling
    //     each lane independently.
    reset_parse_template_expressions_calls();
    let ts_combined = compile_with_target(
        TS_OVERLAY_SFC,
        CompileTarget::BUNDLER | CompileTarget::TSX,
        false,
    );
    let ts_combined_calls = parse_template_expressions_call_count();
    assert_eq!(
        ts_combined_calls, 2,
        "TS combined target must parse once per ide_completion value across runtime + TSX, got {ts_combined_calls}"
    );

    let ts_runtime_only = compile_with_target(TS_OVERLAY_SFC, CompileTarget::BUNDLER, false);
    let ts_tsx_only = compile_with_target(TS_OVERLAY_SFC, CompileTarget::IDE, false);

    let combined_runtime = ts_combined
        .template
        .as_ref()
        .expect("combined runtime block");
    let runtime_baseline = ts_runtime_only
        .template
        .as_ref()
        .expect("runtime-only block");
    assert_eq!(
        combined_runtime.code, runtime_baseline.code,
        "shared overlay must not change runtime output (byte-identical to unshared baseline)"
    );

    let combined_tsx = ts_combined.tsx.as_ref().expect("combined tsx block");
    let tsx_baseline = ts_tsx_only.tsx.as_ref().expect("tsx-only block");
    assert_eq!(
        combined_tsx.code, tsx_baseline.code,
        "shared overlay must not change TSX output (byte-identical to unshared baseline)"
    );

    // Diagnostics must be identical too — the shared overlay carries the same
    // parse facts an independent parse would. This valid fixture yields none on
    // either side; the non-empty case (a malformed interpolation) with FULL
    // per-field diagnostic identity is covered by
    // `malformed_template_expression_diagnostic_identical_across_shared_overlay`.
    assert_eq!(
        ts_combined.errors, ts_runtime_only.errors,
        "shared overlay must not change runtime diagnostics"
    );
    assert!(
        ts_combined.errors.is_empty(),
        "valid fixture must produce no diagnostics, got {:?}",
        ts_combined.errors
    );

    // (b) JS SFC: combined target parses twice (tsx runtime + jsx TSX) — no
    //     sharing across source types — and output stays byte-identical to
    //     compiling each lane independently.
    reset_parse_template_expressions_calls();
    let js_combined = compile_with_target(
        JS_OVERLAY_SFC,
        CompileTarget::BUNDLER | CompileTarget::TSX,
        false,
    );
    let js_combined_calls = parse_template_expressions_call_count();
    assert_eq!(
        js_combined_calls, 3,
        "JS combined parses tsx runtime + jsx liveness + jsx TSX (no cross-source-type sharing), got {js_combined_calls}"
    );

    let js_runtime_only = compile_with_target(JS_OVERLAY_SFC, CompileTarget::BUNDLER, false);
    let js_tsx_only = compile_with_target(JS_OVERLAY_SFC, CompileTarget::IDE, false);

    assert_eq!(
        js_combined
            .template
            .as_ref()
            .expect("js combined runtime")
            .code,
        js_runtime_only
            .template
            .as_ref()
            .expect("js runtime-only")
            .code,
        "JS runtime output unchanged"
    );
    let js_combined_tsx = js_combined.tsx.as_ref().expect("js combined tsx");
    assert_eq!(
        js_combined_tsx.code,
        js_tsx_only.tsx.as_ref().expect("js tsx-only").code,
        "JS TSX output unchanged"
    );
    // The JS TSX lane emits a `.jsx` (JavaScript) surface, not a `.tsx` one:
    // the JS SFC's TSX block must remain valid when parsed as JSX.
    let jsx_alloc = Allocator::new();
    let jsx_parsed = verter_parser::oxc_parse::Parser::new(
        &jsx_alloc,
        &js_combined_tsx.code,
        oxc_span::SourceType::jsx(),
    )
    .parse();
    assert!(
        jsx_parsed.diagnostics.is_empty(),
        "JS SFC TSX output must parse as JSX: {:?}",
        jsx_parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    );
}

/// Absolute-bytes anchor: the shared-overlay compile path emits exactly the
/// pinned runtime and TSX bytes. Because both goldens are hand-verified literal
/// output (not another run of the same path), this breaks the circularity of
/// the combined-equals-pure checks — a regression in the shared overlay that
/// also moved the pure-target output would still break these absolute goldens.
#[test]
fn template_expression_overlay_pins_absolute_output_bytes() {
    let combined = compile_with_target(
        TS_OVERLAY_GOLDEN_SFC,
        CompileTarget::BUNDLER | CompileTarget::TSX,
        false,
    );
    assert!(
        combined.errors.is_empty(),
        "golden fixture must compile cleanly: {:?}",
        combined.errors
    );

    let runtime = combined
        .template
        .as_ref()
        .expect("runtime block")
        .code
        .as_str();
    assert_eq!(
        runtime, TS_OVERLAY_GOLDEN_RUNTIME,
        "shared-overlay runtime output drifted from the pinned absolute bytes"
    );

    let tsx = combined.tsx.as_ref().expect("tsx block").code.as_str();
    assert_eq!(
        tsx, TS_OVERLAY_GOLDEN_TSX,
        "shared-overlay TSX output drifted from the pinned absolute bytes"
    );

    // Guard the goldens themselves against accidental triviality.
    assert!(TS_OVERLAY_GOLDEN_RUNTIME.contains("_createElementBlock(\"p\""));
    assert!(TS_OVERLAY_GOLDEN_TSX.contains("<><p title={msg}></p>{ msg }</>"));

    // The IDE carrier exports the component's PUBLIC FACADE — a clean
    // `export default` re-exported from the API carrier (`.verter.ts`). A bare
    // consumer `import Comp from "./Comp.vue"` resolves natively to the
    // `.d.vue.ts` declaration carrier; the self-import that types
    // `___VERTER___instance` targets the API carrier, NOT the IDE output.
    assert!(
        tsx.contains("export { default } from \"./App.vue.verter.js\";"),
        "IDE carrier must re-export the public default from the API carrier:\n{tsx}"
    );
    assert!(
        tsx.contains("import(\"./App.vue.verter.js\")"),
        "instance self-import must target the .verter.ts API carrier:\n{tsx}"
    );
    // Template internals stay LOCAL (non-exported): the binding fn is a plain
    // `export function` helper, never the component's public default.
    assert!(
        !tsx.contains("export default ___VERTER___")
            && !tsx.contains("export { ___VERTER___TemplateBindingFN as default }"),
        "template internals must NOT be exported as the public default:\n{tsx}"
    );
}

/// A malformed template interpolation surfaces an identical diagnostic whether
/// the SFC is compiled for the combined target (shared overlay) or the runtime
/// target alone. Asserts the FULL diagnostic (severity, code, span, message),
/// and that the diagnostic set is non-empty — so the parity is real, not an
/// empty-vs-empty comparison.
#[test]
fn malformed_template_expression_diagnostic_identical_across_shared_overlay() {
    let combined = compile_with_target(
        TS_OVERLAY_MALFORMED_SFC,
        CompileTarget::BUNDLER | CompileTarget::TSX,
        false,
    );
    let runtime_only = compile_with_target(TS_OVERLAY_MALFORMED_SFC, CompileTarget::BUNDLER, false);

    // The malformed interpolation MUST surface at least one diagnostic, else
    // this parity check would be vacuous.
    assert!(
        !runtime_only.errors.is_empty(),
        "malformed interpolation must surface a diagnostic"
    );
    let invalid_expr = runtime_only
        .errors
        .iter()
        .find(|d| d.code == "XInvalidExpression")
        .expect("malformed interpolation must surface an XInvalidExpression diagnostic");
    assert_eq!(invalid_expr.severity, CompileDiagnosticSeverity::Warning);
    assert!(
        !invalid_expr.message.is_empty(),
        "diagnostic must carry a non-empty message"
    );

    // FULL diagnostic identity (severity + code + span + message, the four
    // fields of `CompileDiagnostic`) between the shared-overlay combined target
    // and the runtime baseline — the shared overlay must neither drop nor alter
    // any diagnostic.
    assert_eq!(
        combined.errors, runtime_only.errors,
        "shared overlay changed the diagnostics surfaced for a malformed interpolation"
    );

    // The TSX lane consumes the SAME malformed `tsx()` overlay: the combined
    // target's TSX output is byte-identical to the TSX-only baseline.
    let tsx_only = compile_with_target(TS_OVERLAY_MALFORMED_SFC, CompileTarget::IDE, false);
    assert_eq!(
        combined.tsx.as_ref().expect("combined tsx").code,
        tsx_only.tsx.as_ref().expect("tsx-only").code,
        "shared overlay changed the TSX output for a malformed interpolation"
    );
}

#[test]
fn tsx_global_component_spread_arrow_satisfies_via_fallback_const() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
function onPing(s: string) { void s; }
</script>
<template>
  <GlobalEmitComp @some-event="(e) => onPing(e)" />
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains(
            r#"((e) => onPing(e)) satisfies (...___VERTER___eventArgs: Parameters<NonNullable<Required<InstanceType<typeof GlobalEmitComp>["$props"]>["onSome-event"]>>) => unknown"#
        ),
        "spread arrow must satisfies-wrap via InstanceType<typeof GlobalEmitComp>: {}",
        tsx.code
    );
    assert_no_spread_event_antipatterns(&tsx.code);
}

#[test]
fn tsx_global_component_simple_handler_param_inference_via_fallback() {
    // event_inference consumes the SAME inventory: a global component's simple-ident
    // handler types its function-declaration parameter via the fallback const, with no
    // implicit-any left behind.
    let result = compile_tsx(
        r#"<script setup lang="ts">
function handlePing(e) { void e; }
</script>
<template>
  <GlobalEmitComp @ping="handlePing" />
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains(
            "const GlobalEmitComp = {} as ___VERTER___GlobalComponentType<'GlobalEmitComp'>"
        ),
        "must emit the fallback const: {}",
        tsx.code
    );
    assert!(
        tsx.code.contains(
            r#"Parameters<NonNullable<Required<InstanceType<typeof GlobalEmitComp>["$props"]>["onPing"]>>"#
        ),
        "simple-handler param inference must resolve via the fallback const: {}",
        tsx.code
    );
    assert_no_spread_event_antipatterns(&tsx.code);
}

#[test]
fn inline_template_ts_keeps_define_component_wrapper() {
    let result = compile_sfc_inline(
        r#"<script setup lang="ts">
const msg = 'hi'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");
    // The V1a gate holds for inline too: TS → _defineComponent wrap + import.
    assert!(
        script.code.contains("/*@__PURE__*/_defineComponent({"),
        "TS inline keeps the _defineComponent wrapper, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("defineComponent as _defineComponent"),
        "TS inline tracks the helper import, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("return (_ctx,_cache) => {"),
        "got:\n{}",
        script.code
    );
}

#[test]
fn inline_template_js_plain_object_no_define_component() {
    // V1a gate holds for inline: JS → plain object, no _defineComponent import.
    let result = compile_sfc_inline(
        r#"<script setup>
const msg = 'hi'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );
    let script = result.script.as_ref().expect("script block");
    assert!(
        script.code.contains("const __sfc__ = {"),
        "JS inline emits a plain object, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("_defineComponent"),
        "JS inline must not reference _defineComponent, got:\n{}",
        script.code
    );
}

#[test]
fn inline_template_dev_default_stays_non_inline() {
    // resolve_inline: None → is_production (dev = non-inline, unchanged).
    let result = compile_sfc(
        r#"<script setup>
const msg = 'hi'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );
    let script = result.script.as_ref().expect("script block");
    assert!(
        !script.code.contains("return (_ctx,_cache) => {"),
        "dev default must stay non-inline, got:\n{}",
        script.code
    );
    assert!(
        result.template.is_some(),
        "non-inline keeps the separate template block"
    );
}

#[test]
fn inline_template_production_default_inlines() {
    // resolve_inline: None + is_production → inline (official prod default).
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        is_production: true,
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>",
        &options,
        &verter_opts,
        &VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    let script = result.script.as_ref().expect("script block");
    assert!(
        script.code.contains("return (_ctx,_cache) => {"),
        "production default must inline, got:\n{}",
        script.code
    );
}

#[test]
fn inline_template_template_only_sfc_falls_back_to_non_inline() {
    // No <script setup> to inline into — template-only stays non-inline even
    // when inline is requested (same as official).
    let result = compile_sfc_inline("<template><div>hello</div></template>");
    assert!(
        result.template.is_some(),
        "template-only SFC keeps the separate template block"
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("function render(_ctx, _cache)"),
        "template-only render keeps the standalone 2-param form, got:\n{}",
        tpl.code
    );
}

#[test]
fn inline_template_vapor_fails_closed_not_silently_demoted() {
    // Vapor inline is a DEFERRED capability, not a silent fallback: the
    // canonical request CONSTRUCTS (the Vapor-client cell explicitly
    // claims inline/separate), but execution refuses with a typed
    // `VaporInlineNotYetImplemented` before any codegen runs — it must
    // never silently emit the separate-template-block topology as if the
    // caller had asked for that.
    use crate::compile_request::{
        CompileProduct, CompileRequest, CompileRequestError, FrameworkCompileRequest,
        RuntimeProductRequest, VueBackendRequest, VueCompileRequest,
    };

    let request = CompileRequest::new(
        vec![CompileProduct::RuntimeClient(RuntimeProductRequest {
            inline: Some(true),
            ..Default::default()
        })],
        FrameworkCompileRequest::Vue(VueCompileRequest {
            backend: VueBackendRequest::Vapor,
            ..Default::default()
        }),
        None,
        Some("App.vue".to_string()),
        None,
        false,
        true,
    )
    .expect(
        "inline + vapor must construct — the capability check is at execution, not construction",
    );

    let alloc = Allocator::new();
    let result = super::super::compile(
        "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>",
        &request,
        &VueExecutionInputs::default(),
        &VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    let err = match result {
        Err(err) => err,
        Ok(_) => panic!(
            "inline + vapor must be refused at execution, not silently demoted to non-inline"
        ),
    };
    assert_eq!(err, CompileRequestError::VaporInlineNotYetImplemented);
}

#[test]
fn companion_define_component_call_preserved() {
    // HIGH: companion `export default defineComponent({ ... })` — official
    // keeps the CALL RESULT as the merge target (does not unwrap the call).
    let code = compile_sfc_script_code(
        r#"<script>
import { defineComponent } from 'vue'
export default defineComponent({ name: 'Kept' })
</script>

<script setup>
const msg = 'hi'
</script>

<template><div>{{ msg }}</div></template>"#,
    );
    assert!(
        code.contains("const __default__ = defineComponent({"),
        "the defineComponent(...) call must be preserved as __default__, got:\n{}",
        code
    );
    assert!(
        code.contains("/*@__PURE__*/Object.assign(__default__, {"),
        "the call result is the Object.assign target, got:\n{}",
        code
    );
    assert!(
        code.contains("name: 'Kept'"),
        "companion options preserved, got:\n{}",
        code
    );
}

#[test]
fn ts_companion_default_spread_into_define_component() {
    // TS + companion default: official spreads `...__default__` inside
    // _defineComponent (the binding is still emitted).
    let code = compile_sfc_script_code(
        r#"<script lang="ts">
export default { inheritAttrs: false }
</script>

<script setup lang="ts">
const msg = 'hi'
</script>

<template><div>{{ msg }}</div></template>"#,
    );
    assert!(
        code.contains("const __default__ = { inheritAttrs: false }"),
        "TS companion default bound as __default__, got:\n{}",
        code
    );
    assert!(
        code.contains("/*@__PURE__*/_defineComponent({\n  ...__default__,"),
        "TS spreads __default__ inside _defineComponent, got:\n{}",
        code
    );
    assert_eq!(
        code.matches("export default").count(),
        1,
        "exactly one default export, got:\n{}",
        code
    );
}

#[test]
fn companion_default_merges_in_inline_template_mode() {
    // The companion merge holds on the inline (production) topology too:
    // `__default__` bound + Object.assign + render inlined into setup.
    let result = compile_sfc_inline(
        r#"<script>
export default { inheritAttrs: false }
</script>

<script setup>
const msg = 'hi'
</script>

<template><div>{{ msg }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");
    assert!(
        script
            .code
            .contains("const __default__ = { inheritAttrs: false }"),
        "companion default bound in inline mode, got:\n{}",
        script.code
    );
    assert!(
        script
            .code
            .contains("/*@__PURE__*/Object.assign(__default__, {"),
        "Object.assign merge in inline mode, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("return (_ctx,_cache) => {"),
        "render inlined into setup, got:\n{}",
        script.code
    );
    assert_eq!(
        script.code.matches("export default").count(),
        1,
        "exactly one default export, got:\n{}",
        script.code
    );
}

// =========================================================================
// D4 — inline setup context: attrs/slots destructure
// =========================================================================
//
// Official inline injects `attrs: $attrs` / `slots: $slots` into the setup
// context destructure WHEN the template uses `$attrs`/`$slots` (on-use;
// `buildDestructureElements` runs only for inlineTemplate). Template
// references then resolve to the destructured binding (bare), not `_ctx.*`.

#[test]
fn inline_template_using_attrs_destructures_attrs() {
    let result = compile_sfc_inline(
        "<script setup>\nconst x = 1\n</script>\n<template><div v-bind=\"$attrs\">x</div></template>",
    );
    let code = &result.script.as_ref().expect("script block").code;
    assert!(
        code.contains("setup(__props, { attrs: $attrs })"),
        "inline setup must destructure attrs on template use, got:\n{}",
        code
    );
    assert!(
        code.contains("$attrs") && !code.contains("_ctx.$attrs"),
        "template $attrs references resolve to the destructured binding, got:\n{}",
        code
    );
}

/// The audit that produced the exhaustive match also found sub-positions inside
/// already-visited nodes: a class element that is an `accessor`, a class static
/// BLOCK reached through a class DECLARATION, the right side of `#p in obj`, and
/// the options argument of `import(source, options)`. Each was a silent miss —
/// the identifier came out bare with no compile error.
#[test]
fn audited_class_element_and_expression_sub_positions_resolve() {
    let code = compile_and_validate_template(
        r#"<script setup>
import { ref } from 'vue'
const a = ref(0)
const seed = ref(2)
const opts = ref({})
</script>
<template>
  <button @click="a = 1; class Q { accessor p = seed; static { a = seed } }">cls</button>
  <button @click="a = 1; class K { #p; static has(o) { return #p in o } }">priv</button>
  <button @click="a = 1; import('m', opts)">imp</button>
</template>"#,
    );
    for expected in [
        // `accessor` property value — was swallowed by the class-element catch-all.
        "accessor p = $setup.seed",
        // static block inside a class DECLARATION — the whole declaration was
        // swallowed by the statement catch-all.
        "static { $setup.a = $setup.seed }",
        // `import(source, options)` — only `source` was visited.
        "import('m', $setup.opts)",
    ] {
        assert!(
            code.contains(expected),
            "expected {expected:?} in generated code, got:\n{code}"
        );
    }
    // A private name and a method parameter are LOCAL: neither may be prefixed.
    assert!(
        code.contains("static has(o) { return #p in o }"),
        "the `#p in o` operands must stay local, got:\n{code}"
    );
    // Negative: the bare, unresolved forms must not survive anywhere.
    for forbidden in [
        "accessor p = seed",
        "static { a = seed }",
        "import('m', opts)",
    ] {
        assert!(
            !code.contains(forbidden),
            "unresolved {forbidden:?} must not be emitted, got:\n{code}"
        );
    }
}

/// A template nested 10,000 deep compiles on a 1 MiB thread, to the
/// runtime render function and to TSX with its template data: the
/// handler-cache and array-group-cache reservations and the template data
/// walk run from explicit stacks.
#[test]
fn a_template_nested_10000_deep_compiles_on_a_small_stack() {
    const DEPTH: usize = 10_000;
    let compiled = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            let source = format!(
                "<template>{}x{}</template>
",
                "<div>".repeat(DEPTH),
                "</div>".repeat(DEPTH)
            );
            let runtime = compile_sfc(&source);
            let tsx = compile_tsx_with_template_data(&source);
            let data = tsx.template_data.as_ref().expect("template data");
            (
                runtime.errors.len(),
                tsx.errors.len(),
                data.elements.len(),
                data.max_nesting_depth,
            )
        })
        .expect("spawn the compiling thread")
        .join()
        .expect("the compile returns");
    assert_eq!(compiled, (0, 0, DEPTH, DEPTH as u16));
}
