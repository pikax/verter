use super::*;

#[test]
fn vmrs_runtime_bundle_is_the_only_type_based_props_authority() {
    use std::sync::Arc;
    use verter_macro_dto::{
        AuthoredMemberOrdinal, MacroAnchor, MacroRuntimeBundle, MacroRuntimeEntry,
        MacroRuntimeOutcome, MacroRuntimeShape, OrderedRuntimeConstructors,
        PropsDefaultsAssociation, PropsRuntimeShape, RuntimeConstructor, RuntimeProp,
        RuntimePropType,
    };

    let semantics = VueMacroSemanticInput::Runtime(Arc::new(MacroRuntimeBundle {
        entries: vec![MacroRuntimeEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(PropsRuntimeShape {
                defaults: PropsDefaultsAssociation::None,
                props: vec![RuntimeProp {
                    name: "authoritative".to_string(),
                    optional: false,
                    type_shape: RuntimePropType::Resolved {
                        constructors: OrderedRuntimeConstructors::from_ordered([
                            RuntimeConstructor::Boolean,
                            RuntimeConstructor::Unknown,
                        ]),
                        skip_check: true,
                    },
                    anchor: MacroAnchor::Authored {
                        macro_index: 0,
                        member_ordinal: AuthoredMemberOrdinal::new(0),
                    },
                }],
            })),
        }],
    }));
    let alloc = Allocator::new();
    let options = CodegenOptions::default();
    let verter_options = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(
        r#"<script setup lang="ts">defineProps<{ authoritative: string }>()</script>"#,
        &options,
        &verter_options,
        &semantics,
        &alloc,
    );

    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let code = result.script.expect("script").code;
    assert!(code.contains("authoritative: { type: Boolean, required: true, skipCheck: true }"));
    assert!(!code.contains("authoritative: { type: String"));
}

#[test]
fn vmrs_ide_uses_authoritative_runtime_props_for_template_bindings() {
    use std::sync::Arc;
    use verter_macro_dto::{
        AuthoredMemberOrdinal, MacroAnchor, MacroRuntimeBundle, MacroRuntimeEntry,
        MacroRuntimeOutcome, MacroRuntimeShape, OrderedRuntimeConstructors,
        PropsDefaultsAssociation, PropsRuntimeShape, RuntimeConstructor, RuntimeProp,
        RuntimePropType,
    };

    let semantics = VueMacroSemanticInput::Runtime(Arc::new(MacroRuntimeBundle {
        entries: vec![MacroRuntimeEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(PropsRuntimeShape {
                defaults: PropsDefaultsAssociation::None,
                props: vec![RuntimeProp {
                    name: "authoritative".to_string(),
                    optional: false,
                    type_shape: RuntimePropType::Resolved {
                        constructors: OrderedRuntimeConstructors::from_ordered([
                            RuntimeConstructor::String,
                        ]),
                        skip_check: false,
                    },
                    anchor: MacroAnchor::Authored {
                        macro_index: 0,
                        member_ordinal: AuthoredMemberOrdinal::new(0),
                    },
                }],
            })),
        }],
    }));
    let alloc = Allocator::new();
    let result = compile(
        r#"<script setup lang="ts">
defineProps<{ authoritative: string }>()
</script>
<template>{{ authoritative }}</template>"#,
        &CodegenOptions {
            filename: Some("App.vue".to_string()),
            target: CompileTarget::IDE,
            ..Default::default()
        },
        &VerterCompileOptions::default(),
        &semantics,
        &alloc,
    );

    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let code = result.tsx.expect("IDE output").code;
    assert!(
        code.contains("__props.authoritative"),
        "authoritative runtime props must drive IDE binding ownership: {code}"
    );
    assert!(
        !code.contains("_ctx.authoritative"),
        "an authoritative prop must not fall back to context ownership: {code}"
    );
}

#[test]
fn vmrs_invalid_props_root_reports_vue_invalid_macro_type() {
    use std::sync::Arc;
    use verter_macro_dto::{
        MacroFailure, MacroInvalidReason, MacroRuntimeBundle, MacroRuntimeEntry,
        MacroRuntimeOutcome,
    };

    let semantics = VueMacroSemanticInput::Runtime(Arc::new(MacroRuntimeBundle {
        entries: vec![MacroRuntimeEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroRuntimeOutcome::Invalid(MacroFailure::new(
                MacroInvalidReason::NonObjectRoot,
                None,
            )),
        }],
    }));
    let alloc = Allocator::new();
    let result = compile(
        r#"<script setup lang="ts">defineProps<string>()</script>"#,
        &CodegenOptions::default(),
        &VerterCompileOptions {
            force_js: true,
            ..Default::default()
        },
        &semantics,
        &alloc,
    );

    assert!(
        result
            .errors
            .iter()
            .any(|diagnostic| diagnostic.code == "XInvalidMacroType"),
        "resolved wrong-shape roots must retain Vue's invalid-macro diagnostic: {:?}",
        result.errors
    );
}

/// Mutation recipe: route typed invalid outcomes through the generic
/// unavailable renderer or ignore the syntax-owned macro role/type argument.
/// The exact role messages, cross-role negatives, and both semantic rails then
/// fail together.
#[test]
fn vmrs_invalid_macro_shapes_render_role_specific_diagnostics_on_both_rails() {
    use std::sync::Arc;
    use verter_macro_dto::{
        MacroFailure, MacroInvalidReason, MacroRuntimeBundle, MacroRuntimeEntry,
        MacroRuntimeOutcome, MacroTscBundle, MacroTscEntry, MacroTscOutcome,
    };

    let cases = [
        (
            r#"<script setup lang="ts">defineProps<Props>()</script>"#,
            "Props",
            MacroInvalidReason::NonObjectRoot,
            "defineProps() type argument 'Props' must resolve to an object-like props type.",
            "defineEmits() type argument",
        ),
        (
            r#"<script setup lang="ts">defineEmits<Emits>()</script>"#,
            "Emits",
            MacroInvalidReason::InvalidEmitsShape,
            "defineEmits() type argument 'Emits' must resolve to emit call signatures or a named-tuple emits object.",
            "defineProps() type argument",
        ),
    ];

    for (source, type_text, reason, expected, wrong_role) in cases {
        let type_start = source
            .find(&format!("<{type_text}>"))
            .expect("type argument") as u32
            + 1;
        let type_end = type_start + type_text.len() as u32;
        let runtime = VueMacroSemanticInput::Runtime(Arc::new(MacroRuntimeBundle {
            entries: vec![MacroRuntimeEntry {
                syntax_index: 0,
                macro_index: 0,
                outcome: MacroRuntimeOutcome::Invalid(MacroFailure::new(reason, None)),
            }],
        }));
        let runtime_result = compile(
            source,
            &CodegenOptions::default(),
            &VerterCompileOptions {
                force_js: true,
                ..Default::default()
            },
            &runtime,
            &Allocator::new(),
        );
        let runtime_diagnostic = runtime_result
            .errors
            .iter()
            .find(|diagnostic| diagnostic.code == "XInvalidMacroType")
            .expect("runtime invalid diagnostic");
        assert_eq!(runtime_diagnostic.message, expected);
        assert!(!runtime_diagnostic.message.contains(wrong_role));
        assert!(!runtime_diagnostic
            .message
            .contains("Authoritative runtime semantics"));
        assert_eq!(
            runtime_diagnostic.span,
            Some(crate::common::Span::new(type_start, type_end))
        );

        let tsc = VueMacroSemanticInput::Tsc(Arc::new(MacroTscBundle {
            entries: vec![MacroTscEntry {
                syntax_index: 0,
                macro_index: 0,
                outcome: MacroTscOutcome::Invalid(MacroFailure::new(reason, None)),
            }],
        }));
        let tsc_result = compile(
            source,
            &CodegenOptions {
                target: CompileTarget::TSC,
                ..Default::default()
            },
            &VerterCompileOptions::default(),
            &tsc,
            &Allocator::new(),
        );
        let tsc_diagnostic = tsc_result
            .errors
            .iter()
            .find(|diagnostic| diagnostic.code == "XInvalidMacroType")
            .expect("TSC invalid diagnostic");
        assert_eq!(tsc_diagnostic.message, expected);
        assert!(!tsc_diagnostic.message.contains(wrong_role));
        assert!(!tsc_diagnostic
            .message
            .contains("Authoritative TSC semantics"));
        assert_eq!(
            tsc_diagnostic.span,
            Some(crate::common::Span::new(type_start, type_end))
        );
    }
}

#[test]
fn vmrs_tsc_preserves_the_authorized_parser_owned_props_argument() {
    use std::sync::Arc;
    use verter_macro_dto::{
        MacroTscBundle, MacroTscEntry, MacroTscOutcome, MacroTscProjection, TscPropsProjection,
        TscPublicPropsProjection, TscScopeRequirements,
    };

    let semantics = VueMacroSemanticInput::Tsc(Arc::new(MacroTscBundle {
        entries: vec![MacroTscEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroTscOutcome::Complete(MacroTscProjection::Props(TscPropsProjection {
                public: TscPublicPropsProjection::AuthoredArgument {
                    anchor: verter_macro_dto::MacroAnchor::MacroArgument { macro_index: 0 },
                },
                testing_rows: Vec::new(),
                scope: TscScopeRequirements::default(),
            })),
        }],
    }));
    let alloc = Allocator::new();
    let result = compile(
        r#"<script setup lang="ts">defineProps<{ ignored: boolean }>()</script>"#,
        &CodegenOptions {
            target: CompileTarget::TSC,
            ..Default::default()
        },
        &VerterCompileOptions::default(),
        &semantics,
        &alloc,
    );

    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let code = result.tsc.expect("TSC output").code;
    assert!(code.contains("{ ignored: boolean }"), "{code}");
}

#[test]
fn format_import_specifier_strips_underscore_prefix() {
    assert_eq!(
        format_import_specifier("_defineComponent"),
        "defineComponent as _defineComponent"
    );
    assert_eq!(
        format_import_specifier("_useSlots"),
        "useSlots as _useSlots"
    );
    assert_eq!(
        format_import_specifier("_Fragment"),
        "Fragment as _Fragment"
    );
}

#[test]
fn format_import_specifier_preserves_non_prefixed() {
    assert_eq!(format_import_specifier("vue"), "vue");
    assert_eq!(format_import_specifier("ref"), "ref");
}

#[test]
fn script_imports_use_as_syntax() {
    // TS script setup keeps the `_defineComponent` wrapper, so its helper
    // import is emitted (JS emits a plain object with no such import).
    let result = compile_sfc(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#,
    );
    let script = result.script.as_ref().expect("script block");
    // The import should use "defineComponent as _defineComponent" syntax
    // because Vue exports "defineComponent" (no underscore prefix).
    assert!(
        script.code.contains("defineComponent as _defineComponent"),
        "Expected 'defineComponent as _defineComponent' in imports, got: {}",
        script.code
    );
    assert!(
        !script.code.contains("import { _defineComponent }"),
        "Should not import bare _defineComponent, got: {}",
        script.code
    );
}

#[test]
fn v_else_if_condition_has_setup_prefix() {
    // v-else-if should also resolve bindings
    let code = compile_and_validate_template(
        r#"<script setup>
const a = ref(true)
const b = ref(false)
</script>
<template><div><span v-if="a">A</span><span v-else-if="b">B</span><span v-else>C</span></div></template>"#,
    );
    assert!(
        code.contains("$setup.a"),
        "v-if condition should use $setup. prefix\n{}",
        code
    );
    assert!(
        code.contains("$setup.b"),
        "v-else-if condition should use $setup. prefix\n{}",
        code
    );
}

// ==================== VDOM props binding resolution ====================

#[test]
fn vdom_props_apply_ctx_prefix_to_bindings() {
    // Directive prop values like :foo="message" and @click="handler" should
    // have _ctx. prefix applied to identifiers, just like interpolation does.
    let code = compile_and_validate_template(
        r#"<template><button :click="increment" @click="increment" :foo="message"></button></template>"#,
    );
    assert!(
        code.contains("_ctx.increment") || code.contains("$setup.increment"),
        "Directive prop values should have binding prefix applied\n{}",
        code
    );
    assert!(
        code.contains("_ctx.message") || code.contains("$setup.message"),
        "Directive prop values should have binding prefix applied\n{}",
        code
    );
}

// ==================== Shorthand property expansion ====================

#[test]
fn shorthand_property_expanded_when_prefixed() {
    // When a shorthand property `{ searchTerm }` gets its identifier rewritten
    // to `$setup.searchTerm`, it must be expanded to `{ searchTerm: $setup.searchTerm }`.
    let code = compile_and_validate_template(
        r#"<template><div>{{ t('msg', { searchTerm }) }}</div></template>"#,
    );
    assert!(
        code.contains("searchTerm: "),
        "Shorthand property should be expanded to key: value form\n{}",
        code
    );
}

#[test]
fn script_attrs_contain_lang() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
const x = 1
</script>
<template><div>{{ x }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    eprintln!("attrs: {:?}", script.attrs);
    let lang = script.attrs.iter().find(|(k, _)| k == "lang");
    assert!(
        lang.is_some(),
        "Expected 'lang' in attrs, got: {:?}",
        script.attrs
    );
    assert_eq!(lang.unwrap().1, "ts");
}

#[test]
fn vapor_static_prop_with_newline() {
    // Static prop values with newlines must be escaped in the JS string
    let code = compile_and_validate_vapor_template(
        "<template><MyComp content=\"line1\nline2\" /></template>",
    );
    assert!(
        code.contains("\\n"),
        "Newlines in static prop values should be escaped\n{}",
        code
    );
}

#[test]
fn vapor_interpolation_shorthand_property_expanded() {
    // { total } with prefix → { total: _ctx.total }
    let code = compile_and_validate_vapor_template(
        r#"<template><div>{{ fn({ total }) }}</div></template>"#,
    );
    assert!(
        code.contains("total: _ctx.total"),
        "Shorthand properties should be expanded when prefixed\n{}",
        code
    );
}

#[test]
fn vapor_component_with_hyphenated_props() {
    let code = compile_and_validate_vapor_template(
        r#"<template><MyComp clear-icon="close" :void-icon="icon" /></template>"#,
    );
    // Hyphenated prop names must be quoted in object literals
    assert!(
        code.contains("\"clear-icon\""),
        "Static hyphenated prop should be quoted\n{}",
        code
    );
    assert!(
        code.contains("\"void-icon\""),
        "Dynamic hyphenated prop should be quoted\n{}",
        code
    );
}

// ======================== OXC binding resolution tests ========================
// These test that compound expressions get proper _ctx. prefixing via OXC data,
// not just simple identifiers.

#[test]
fn vapor_component_prop_compound_expr() {
    let code =
        compile_and_validate_vapor_template(r#"<template><MyComp :title="a + b" /></template>"#);
    assert!(
        code.contains("_ctx.a + _ctx.b"),
        "Compound expression in component prop should prefix both identifiers\n{}",
        code
    );
}

// ===== Component resolution tests =====

#[test]
pub(super) fn component_resolves_to_setup_binding() {
    let result = compile_sfc(
        r#"<template><div><Header :store="store" /></div></template>
<script setup>import Header from "./Header.vue"; const store = ref(1);</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$setup[\"Header\"]") || tpl.code.contains("$setup.Header"),
        "component should resolve to $setup[\"Header\"], got:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("createVNode(\"Header\""),
        "component should NOT be a string literal, got:\n{}",
        tpl.code
    );
}

#[test]
pub(super) fn component_kebab_case_resolves_to_pascal_setup_binding() {
    let result = compile_sfc(
        r#"<template><div><my-header /></div></template>
<script setup>import MyHeader from "./MyHeader.vue";</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$setup[\"MyHeader\"]") || tpl.code.contains("$setup.MyHeader"),
        "kebab-case component should resolve to PascalCase $setup binding, got:\n{}",
        tpl.code
    );
}

#[test]
fn type_based_define_props_resolves_to_props_prefix() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [crate::test_helpers::runtime_prop(
            "store",
            false,
            [verter_macro_dto::RuntimeConstructor::Object],
        )],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<template><div>{{ store.loading }}</div></template>
<script setup lang="ts">import type { Store } from "./store"; const props = defineProps<{ store: Store }>();</script>"#,
        runtime,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.store"),
        "type-based defineProps prop should resolve to $props.store, got:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("_ctx.store"),
        "type-based defineProps prop should NOT use _ctx prefix, got:\n{}",
        tpl.code
    );
}

/// Official `isEmpty()` (`parse.ts` 421-429) treats whitespace-only /
/// childless `<script>`/`<script setup>` as absent (`ignoreEmpty` 159-169),
/// unless `src` (`!hasAttr(node, 'src')`, line 165). An empty-script-only
/// carrier therefore diagnoses `MissingSfcEntryBlock` (`parse.ts` 232-238;
/// `parse.spec.ts` 220-228).
#[test]
fn missing_sfc_entry_block_for_empty_script_only_carriers() {
    for src in [
        "<script/>",
        "<script></script>",
        "<script> \n\t  </script>",
        "<script setup/>",
        "<script setup> \n\t  </script>",
    ] {
        let result = compile_sfc(src);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.code == "MissingSfcEntryBlock"),
            "{src:?} must diagnose MissingSfcEntryBlock (oracle: empty script \
             is dropped by isEmpty(), parse.ts:421-429 + :159-169), got: {:?}",
            result.errors
        );
    }
}

/// `src` exempts a script from `isEmpty` even with zero children
/// (`parse.ts:165`, `parse.spec.ts:230-235`). Must not diagnose
/// `MissingSfcEntryBlock`.
#[test]
fn script_with_src_attr_counts_as_entry_block_even_when_empty() {
    let result = compile_sfc(r#"<script src="./foo.js"/>"#);
    assert!(
        !result
            .errors
            .iter()
            .any(|e| e.code == "MissingSfcEntryBlock"),
        "<script src=.../> must NOT diagnose MissingSfcEntryBlock \
         (oracle: parse.ts:165 exempts src-attributed blocks from isEmpty), \
         got: {:?}",
        result.errors
    );
}

/// Official `hasAttr` (`parse.ts:413-415`) tests presence, not value.
/// Valueless `src` (`<script src/>`) still exempts `isEmpty`.
/// `RootNodeScript::src` is only the value span (`None` if valueless) — do
/// not use it as the presence check.
#[test]
fn script_with_valueless_src_attr_counts_as_entry_block() {
    for src in ["<script src/>", "<script src></script>"] {
        let result = compile_sfc(src);
        assert!(
            !result
                .errors
                .iter()
                .any(|e| e.code == "MissingSfcEntryBlock"),
            "{src:?} must NOT diagnose MissingSfcEntryBlock (oracle: \
             parse.ts:413-415 `hasAttr` tests attribute presence, not \
             value), got: {:?}",
            result.errors
        );
    }
}

/// `hasAttr` is case-sensitive (`parse.ts:413-415`; `onattribname` keeps
/// authored casing). `SRC`/`Src` is not `src` — empty script still
/// diagnoses `MissingSfcEntryBlock`.
#[test]
fn script_with_uppercase_src_spelling_does_not_count_as_entry_block() {
    for src in ["<script SRC/>", "<script Src></script>"] {
        let result = compile_sfc(src);
        assert!(
            result
                .errors
                .iter()
                .any(|e| e.code == "MissingSfcEntryBlock"),
            "{src:?} must still diagnose MissingSfcEntryBlock (oracle: \
             hasAttr is case-sensitive, so `SRC`/`Src` is not the `src` \
             attribute), got: {:?}",
            result.errors
        );
    }
}

/// Non-whitespace script content is never `isEmpty()` (`parse.ts:421-429`).
#[test]
fn script_with_import_only_content_counts_as_entry_block() {
    let result = compile_sfc("<script>import { ref } from 'vue'</script>");
    assert!(
        !result
            .errors
            .iter()
            .any(|e| e.code == "MissingSfcEntryBlock"),
        "<script>import ...</script> must NOT diagnose MissingSfcEntryBlock, \
         got: {:?}",
        result.errors
    );
}

#[test]
fn element_with_multiple_dynamic_props_has_all_in_array() {
    let result = compile_sfc(
        r#"<template><button @click="handler" :disabled="off">go</button></template>
<script setup>const handler = () => {}; const off = false;</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("[\"disabled\"]"),
        "dynamicProps array should contain only the genuinely dynamic \"disabled\" prop, got:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("\"onClick\""),
        "onClick must not appear in dynamicProps, got:\n{}",
        tpl.code
    );
}

#[test]
fn element_with_only_static_props_no_dynamic_props_array() {
    let result = compile_sfc(
        r#"<template><div class="foo" id="bar">text</div></template>
<script setup></script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Static-only props should NOT have a dynamicProps array
    assert!(
        !tpl.code.contains("[\"class\"]") && !tpl.code.contains("[\"id\"]"),
        "static-only props should NOT produce dynamicProps array, got:\n{}",
        tpl.code
    );
}

/// @ai-generated - Cross-file const prop is excluded from dynamicProps
#[test]
fn const_prop_excluded_from_dynamic_props() {
    // A child component template: <Comp :msg="msg" :count="count">
    // When `msg` is known to be const across all parents but `count` is not,
    // only `count` should appear in the dynamicProps array.
    let result = compile_sfc_with_const_props(
        r#"<template><div :title="msg" :id="count">text</div></template>
<script setup>const props = defineProps(['msg', 'count']);</script>"#,
        &["msg"],
    );
    let tpl = result.template.as_ref().expect("template block");
    // `msg` (const prop) should NOT be in dynamicProps
    assert!(
        !tpl.code.contains("\"title\""),
        "const prop 'msg' (bound as :title) should be excluded from dynamicProps, got:\n{}",
        tpl.code
    );
    // `count` (non-const prop) should still be in dynamicProps
    assert!(
        tpl.code.contains("\"id\""),
        "non-const prop 'count' (bound as :id) should remain in dynamicProps, got:\n{}",
        tpl.code
    );
}

/// @ai-generated - Without const_props data, all props are in dynamicProps (Vue compat)
#[test]
fn without_const_props_all_bound_props_in_dynamic_props() {
    let result = compile_sfc(
        r#"<template><div :title="msg" :id="count">text</div></template>
<script setup>const props = defineProps(['msg', 'count']);</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Both should be in dynamicProps (standard Vue behavior)
    assert!(
        tpl.code.contains("\"title\"") && tpl.code.contains("\"id\""),
        "without const_props, both bound props should be in dynamicProps, got:\n{}",
        tpl.code
    );
}

/// @ai-generated - Const prop still uses $props prefix (correct runtime access)
#[test]
fn const_prop_still_uses_props_prefix() {
    let result = compile_sfc_with_const_props(
        r#"<template><div :title="msg">text</div></template>
<script setup>const props = defineProps(['msg']);</script>"#,
        &["msg"],
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.msg"),
        "const prop should still use $props. prefix, got:\n{}",
        tpl.code
    );
}

/// @ai-generated - Vapor: const prop setter emitted as direct statement, not inside _renderEffect
#[test]
fn vapor_const_prop_skips_render_effect() {
    let result = compile_sfc_vapor_with_const_props(
        r#"<template><div :title="msg">text</div></template>
<script setup>const props = defineProps(['msg']);</script>"#,
        &["msg"],
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tpl = result.template.as_ref().expect("template block");
    // Const prop should NOT be inside _renderEffect
    assert!(
        !tpl.code.contains("_renderEffect"),
        "const prop setter should not be wrapped in _renderEffect, got:\n{}",
        tpl.code
    );
    // The setter should still be present as a direct statement
    assert!(
        tpl.code.contains("_setProp") || tpl.code.contains("_setAttr"),
        "const prop setter should still be emitted, got:\n{}",
        tpl.code
    );
}

/// @ai-generated - Vapor: non-const prop stays inside _renderEffect
#[test]
fn vapor_non_const_prop_in_render_effect() {
    let result = compile_sfc_vapor_with_const_props(
        r#"<template><div :title="msg" :id="count">text</div></template>
<script setup>const props = defineProps(['msg', 'count']);</script>"#,
        &["msg"], // only msg is const
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tpl = result.template.as_ref().expect("template block");
    // Non-const prop `count` should be inside _renderEffect
    assert!(
        tpl.code.contains("_renderEffect"),
        "non-const prop should be wrapped in _renderEffect, got:\n{}",
        tpl.code
    );
}

/// @ai-generated - Vapor: without const_props data, all dynamic props in _renderEffect
#[test]
fn vapor_without_const_props_all_in_render_effect() {
    let result = compile_sfc_vapor(
        r#"<template><div :title="msg">text</div></template>
<script setup>const props = defineProps(['msg']);</script>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("_renderEffect"),
        "without const_props, dynamic props should be in _renderEffect, got:\n{}",
        tpl.code
    );
}

#[test]
fn dual_script_preserves_named_exports() {
    // Companion <script> with runtime named exports alongside <script setup>
    let result = compile_sfc(
        r#"<script lang="ts">
export enum SwapSettingsContext {
  swap,
  invest,
}
</script>

<script setup lang="ts">
const props = defineProps({ context: String })
</script>

<template><div>{{ props.context }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    // The enum should be preserved in the output (downleveled from TS)
    assert!(
        script.code.contains("SwapSettingsContext"),
        "companion script named export should be preserved.\nOutput:\n{}",
        script.code
    );
    // Should still have the setup wrapper
    assert!(
        script.code.contains("_defineComponent"),
        "setup wrapper should be present.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("export default __sfc__"),
        "default export should be present.\nOutput:\n{}",
        script.code
    );
}

#[test]
fn with_defaults_merges_defaults_into_props() {
    // withDefaults(defineProps<{ color?: string, size?: string, label?: string }>(), { color: 'primary', size: 'md' })
    // should produce props: { color: { type: String, default: 'primary' }, size: { type: String, default: 'md' }, label: { type: String } }
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_prop(
                "color",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
            crate::test_helpers::runtime_prop(
                "size",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
            crate::test_helpers::runtime_prop(
                "label",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
        ],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
const props = withDefaults(defineProps<{
  color?: string
  size?: string
  label?: string
}>(), {
  color: 'primary',
  size: 'md'
})
</script>

<template><div>{{ props.color }}</div></template>"#,
        runtime,
    );
    let script = result.script.as_ref().expect("script block");
    // Should have props section with defaults merged
    assert!(
        script.code.contains("default: 'primary'"),
        "should merge color default.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("default: 'md'"),
        "should merge size default.\nOutput:\n{}",
        script.code
    );
    // label has no default and is optional — should NOT have required: true
    assert!(
        !script.code.contains("label") || !script.code.contains("required: true"),
        "optional label without default should not be required.\nOutput:\n{}",
        script.code
    );
    // Validate JS syntax
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let parsed = verter_parser::oxc_parse::Parser::new(&alloc, &script.code, source_type).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "output should be valid JS.\nOutput:\n{}\nErrors: {:?}",
        script.code,
        parsed.diagnostics
    );
}

#[test]
fn define_props_type_with_imported_types() {
    // defineProps<Props>() where Props has imported types — all props should
    // still appear in the runtime props section (with null type for unknown)
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [
            crate::test_helpers::runtime_degraded_prop_at_macro_argument(
                "pool",
                false,
                verter_macro_dto::MacroMemberReason::Unresolved(
                    verter_macro_dto::UnresolvedReason::MissingDeclaration,
                ),
                Some("Pool is not declared in the fixture".to_owned()),
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "loading",
                false,
                [verter_macro_dto::RuntimeConstructor::Boolean],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "items",
                true,
                [verter_macro_dto::RuntimeConstructor::Array],
            ),
        ],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
type Props = {
  pool: Pool
  loading: boolean
  items?: string[]
}

const props = defineProps<Props>()
</script>

<template><div>{{ props.loading }}</div></template>"#,
        runtime,
    );
    let script = result.script.as_ref().expect("script block");
    // All props should be present (even Pool which is unresolvable)
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("pool:"),
        "pool prop should be in props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("loading:"),
        "loading prop should be in props section.\nOutput:\n{}",
        script.code
    );
}

#[test]
fn with_defaults_imported_types_all_props_present() {
    // withDefaults with Props that has imported types — all props must be present
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_degraded_prop_at_macro_argument(
                "pool",
                false,
                verter_macro_dto::MacroMemberReason::Unresolved(
                    verter_macro_dto::UnresolvedReason::MissingDeclaration,
                ),
                Some("Pool is not declared in the fixture".to_owned()),
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "loading",
                false,
                [verter_macro_dto::RuntimeConstructor::Boolean],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "titleTokens",
                false,
                [verter_macro_dto::RuntimeConstructor::Array],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "color",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
        ],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
type Props = {
  pool: Pool
  loading: boolean
  titleTokens: PoolToken[]
  color?: string
}

const props = withDefaults(defineProps<Props>(), {
  color: 'primary',
})
</script>

<template><div>{{ props.loading }}</div></template>"#,
        runtime,
    );
    let script = result.script.as_ref().expect("script block");
    eprintln!("OUTPUT:\n{}", script.code);
    assert!(
        script.code.contains("pool:"),
        "pool prop should be in props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("loading:"),
        "loading prop should be in props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("titleTokens:"),
        "titleTokens prop should be in props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("default: 'primary'"),
        "color should have default.\nOutput:\n{}",
        script.code
    );
}

/// A non-literal defaults expression without a declarator must merge into the
/// authoritative prop declaration supplied by the semantic boundary.
#[test]
fn with_defaults_variable_expression_without_declarator_uses_authoritative_props() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        1,
        2,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 1,
            defaults_macro_index: 2,
        },
        [crate::test_helpers::runtime_prop_at_macro_argument(
            "as",
            true,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
import { DEFAULT_LABEL_PROPS } from './Label.ts'

interface LabelProps { as?: string }

defineOptions({
  name: 'RadixLabel',
  inheritAttrs: false,
})

withDefaults(defineProps<LabelProps>(), DEFAULT_LABEL_PROPS)
</script>
<template><div /></template>"#,
        runtime,
    );
    let script = result.script.as_ref().expect("script block");
    println!("OUTPUT:\n{}", script.code);
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("DEFAULT_LABEL_PROPS"),
        "should reference the defaults variable.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("_mergeDefaults("),
        "variable defaults must merge with authoritative props.\nOutput:\n{}",
        script.code
    );
}

/// The same authoritative variable-defaults path preserves the declarator.
#[test]
fn with_defaults_variable_expression_with_declarator_uses_authoritative_props() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [crate::test_helpers::runtime_prop_at_macro_argument(
            "as",
            true,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
import { DEFAULT_LABEL_PROPS } from './Label.ts'

interface LabelProps { as?: string }

const props = withDefaults(defineProps<LabelProps>(), DEFAULT_LABEL_PROPS)
</script>
<template><div>{{ props.as }}</div></template>"#,
        runtime,
    );
    let script = result.script.as_ref().expect("script block");
    println!("OUTPUT:\n{}", script.code);
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("DEFAULT_LABEL_PROPS"),
        "should reference the defaults variable.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("__props"),
        "should have __props assignment.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("_mergeDefaults("),
        "variable defaults must merge with authoritative props.\nOutput:\n{}",
        script.code
    );
}

/// Object-literal defaults merge into an explicit local prop contract.
#[test]
fn with_defaults_object_literal_defaults_use_authoritative_local_props() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_prop_at_macro_argument(
                "zIndex",
                true,
                [verter_macro_dto::RuntimeConstructor::Number],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "target",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "position",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
        ],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
interface AffixProps { zIndex?: number; target?: string; position?: string }
const props = withDefaults(defineProps<AffixProps>(), {
  zIndex: 100,
  target: '',
  position: 'top',
})
</script>
<template><div>{{ props.zIndex }}</div></template>"#,
        runtime,
    );
    let script = result.script.as_ref().expect("script block");
    println!("OUTPUT:\n{}", script.code);
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
    // Should have inline prop declarations with defaults
    assert!(
        script.code.contains("zIndex:"),
        "should declare zIndex prop.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("default:"),
        "should have default values.\nOutput:\n{}",
        script.code
    );
}

/// Function-call defaults merge into an explicit local prop contract.
#[test]
fn with_defaults_function_call_defaults_use_authoritative_local_props() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [crate::test_helpers::runtime_prop_at_macro_argument(
            "foo",
            true,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
import { getDefaults } from './defaults'

interface Props { foo?: string }

const props = withDefaults(defineProps<Props>(), getDefaults())
</script>
<template><div>{{ props.foo }}</div></template>"#,
        runtime,
    );
    let script = result.script.as_ref().expect("script block");
    println!("OUTPUT:\n{}", script.code);
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("getDefaults()"),
        "should reference the function call.\nOutput:\n{}",
        script.code
    );
}

/// Mixed object defaults use an explicit local prop contract.
#[test]
fn with_defaults_mixed_defaults_use_authoritative_local_props() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_prop_at_macro_argument(
                "method",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "action",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
        ],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
interface FormProps { method?: string; action?: string }
const props = withDefaults(defineProps<FormProps>(), {
  method: 'POST',
  action: '/api/submit',
})
</script>
<template><div>{{ props.method }}</div></template>"#,
        runtime,
    );
    let script = result.script.as_ref().expect("script block");
    println!("OUTPUT:\n{}", script.code);
    assert!(
        script.code.contains("method:"),
        "should declare method prop.\nOutput:\n{}",
        script.code
    );
    assert!(
        script.code.contains("action:"),
        "should declare action prop.\nOutput:\n{}",
        script.code
    );
}

/// Spread defaults preserve the full defaults expression while merging into an
/// explicit local prop contract.
#[test]
fn with_defaults_spread_expression_uses_authoritative_local_props() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [crate::test_helpers::runtime_prop_at_macro_argument(
            "extra",
            true,
            [verter_macro_dto::RuntimeConstructor::Boolean],
        )],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
import { baseDefaults } from './defaults'

interface Props { extra?: boolean }

withDefaults(defineProps<Props>(), { ...baseDefaults, extra: true })
</script>
<template><div /></template>"#,
        runtime,
    );
    let script = result.script.as_ref().expect("script block");
    println!("OUTPUT:\n{}", script.code);
    // Should have props section — object literal with spread still parsed as MacroObjectArg
    assert!(
        script.code.contains("props:"),
        "should have props section.\nOutput:\n{}",
        script.code
    );
}

#[test]
pub(super) fn component_is_self_closing_with_props() {
    // <component :is> with extra props but no children
    let code = compile_and_validate_template(
        r#"<script setup lang="ts">
const tag = ref('div')
const cls = ref('active')
</script>
<template>
  <component :is="tag" :class="cls" id="main" />
</template>"#,
    );
    assert!(
        code.contains("_resolveDynamicComponent"),
        "<component :is> with extra props should use _resolveDynamicComponent.\nOutput:\n{}",
        code
    );
    // :is should be excluded, but :class and id should remain
    assert!(
        !code.contains("is:") && !code.contains("\"is\""),
        ":is should not be in props object.\nOutput:\n{}",
        code
    );
}

#[test]
pub(super) fn component_is_with_prop_binding_and_vbind() {
    // Matches BalLink.vue pattern: <component :is="tag" :class="[classes]" v-bind="attrs_">
    let result = compile_sfc(
        r#"<script setup lang="ts">
const tag = withDefaults(defineProps<{ tag?: string }>(), { tag: 'a' }).tag
const attrs_ = computed(() => ({}))
const classes = computed(() => ({ link: true }))
</script>
<template>
  <component :is="tag" :class="[classes]" v-bind="attrs_">
<slot />
  </component>
</template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    eprintln!("BalLink-style output:\n{}", tpl.code);
    assert!(
        tpl.code.contains("_resolveDynamicComponent"),
        "<component :is> with prop binding should use _resolveDynamicComponent.\nOutput:\n{}",
        tpl.code
    );
}

#[test]
pub(super) fn imported_component_uses_setup_binding_not_resolve_component() {
    // When a component is imported in <script setup>, the template should
    // reference it via $setup["TokenBreakdown"] (standalone mode), NOT via
    // _resolveComponent("TokenBreakdown"). This is critical for component
    // resolution at runtime.
    let code = compile_and_validate_template(
        r#"<script setup>
import TokenBreakdown from './components/TokenBreakdown.vue'
</script>
<template><div><TokenBreakdown :token="item" /></div></template>"#,
    );
    assert!(
        !code.contains("_resolveComponent"),
        "Imported component should NOT use _resolveComponent.\nOutput:\n{}",
        code
    );
    assert!(
        code.contains("$setup[\"TokenBreakdown\"]") || code.contains("$setup.TokenBreakdown"),
        "Imported component should use $setup binding.\nOutput:\n{}",
        code
    );
}

#[test]
pub(super) fn static_and_dynamic_class_merged_into_single_prop() {
    // When an element has both a static `class` and a dynamic `:class`,
    // they must be merged into a single `class` property using _normalizeClass.
    // Having two separate `class:` keys causes the second to override the first.
    let code = compile_and_validate_template(
        r#"<template><div class="static-class" :class="[dynamic ? 'a' : 'b']">text</div></template>"#,
    );
    // Should have exactly ONE class key
    let class_count = code.matches("class:").count();
    assert_eq!(
        class_count, 1,
        "Should have exactly one `class:` key, got {}.\nOutput:\n{}",
        class_count, code
    );
    // Should include both static and dynamic in _normalizeClass
    assert!(
        code.contains("static-class"),
        "Should include static class.\nOutput:\n{}",
        code
    );
    assert!(
        code.contains("_normalizeClass"),
        "Should use _normalizeClass.\nOutput:\n{}",
        code
    );
}

#[test]
pub(super) fn dual_script_export_default_merged_as_options() {
    // Companion <script> with `export default { inheritAttrs: false }`
    // should be merged into the setup wrapper (no duplicate export default)
    let result = compile_sfc(
        r#"<script lang="ts">
export default {
  inheritAttrs: false,
};
</script>

<script lang="ts" setup>
const msg = 'hello'
</script>

<template><div>{{ msg }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    // `inheritAttrs: false` should be merged into the component definition
    assert!(
        script.code.contains("inheritAttrs: false"),
        "companion export default options should be merged.\nOutput:\n{}",
        script.code
    );
    // Should NOT have two `export default`
    let export_default_count = script.code.matches("export default").count();
    assert_eq!(
        export_default_count, 1,
        "should have exactly one export default, got {}.\nOutput:\n{}",
        export_default_count, script.code
    );
    // Validate JS syntax
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let parsed = verter_parser::oxc_parse::Parser::new(&alloc, &script.code, source_type).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "output should be valid JS.\nOutput:\n{}\nErrors: {:?}",
        script.code,
        parsed.diagnostics
    );
}

// @ai-generated - Tests that template ref works alongside other props
#[test]
fn template_ref_with_other_props() {
    let code = compile_and_validate_template(
        r#"<script setup>
import { ref } from 'vue'
const el = ref()
</script>
<template><div ref="el" class="box">content</div></template>"#,
    );
    assert!(
        code.contains("ref: \"el\""),
        "Template ref should be in props object. Got:\n{}",
        code
    );
    assert!(
        code.contains("class: \"box\""),
        "Class prop should also be present. Got:\n{}",
        code
    );
}

#[test]
pub(super) fn setup_returns_bindings_for_template_refs() {
    // Regression: setup() returned {} instead of exposing bindings,
    // causing template refs (ref="editorContainer") to not bind to
    // the ref() variable, making Vue unable to set .value.
    //
    // Vue's official compiler for this input returns:
    //   return { container, editor, msg }
    // from the setup function so that template refs can bind.
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("Editor.vue".to_string()),
        inline: Some(false),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: false,
        source_map: false,
        ..Default::default()
    };
    let result = compile(
        r#"<script setup lang="ts">
import { ref, onMounted, shallowRef } from 'vue'

const container = ref<HTMLElement>()
const editor = shallowRef()
const msg = ref('hello')

onMounted(() => {
  if (!container.value) return
  console.log('mounted', container.value)
})
</script>

<template>
  <div class="wrapper">
<div ref="container" class="editor" />
<span>{{ msg }}</span>
  </div>
</template>
"#,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.unwrap();
    let code = &script.code;

    // Extract the __returned__ assignment (matches Vue's official compiler pattern)
    let returned_idx = code.find("const __returned__ = ");
    assert!(
        returned_idx.is_some(),
        "Must have __returned__ in setup(). Got:\n{}",
        code
    );
    let returned_rest = &code[returned_idx.unwrap()..];
    let returned_end = returned_rest.find(';').unwrap_or(returned_rest.len());
    let returned_stmt = &returned_rest[..returned_end];

    // The returned object must NOT be empty
    assert!(
        !returned_stmt.contains("= {}") && !returned_stmt.contains("= { }"),
        "setup() must NOT return empty object. Returned was: '{}'. Full:\n{}",
        returned_stmt,
        code
    );

    // Must include __isScriptSetup marker (matches Vue's official compiler)
    assert!(
        code.contains("__isScriptSetup"),
        "Must have __isScriptSetup marker. Full:\n{}",
        code
    );

    // Must return container, editor, msg bindings (like Vue's official compiler)
    assert!(
        returned_stmt.contains("container"),
        "return must include 'container'. Returned was: '{}'. Full:\n{}",
        returned_stmt,
        code
    );
    assert!(
        returned_stmt.contains("editor"),
        "return must include 'editor'. Returned was: '{}'. Full:\n{}",
        returned_stmt,
        code
    );
    assert!(
        returned_stmt.contains("msg"),
        "return must include 'msg'. Returned was: '{}'. Full:\n{}",
        returned_stmt,
        code
    );
}

#[test]
pub(super) fn setup_returns_bindings_with_define_props() {
    // Test with defineProps to match the actual Editor.vue pattern
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("Editor.vue".to_string()),
        inline: Some(false),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: false,
        source_map: false,
        ..Default::default()
    };
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [crate::test_helpers::runtime_prop(
            "store",
            false,
            [verter_macro_dto::RuntimeConstructor::Unknown],
        )],
    )]);
    let result = compile(
        r#"<script setup lang="ts">
import { ref, onMounted, shallowRef } from 'vue'
import * as monaco from 'monaco-editor-core'

const props = defineProps<{
  store: any
}>()

const editorContainer = ref<HTMLElement>()
const editor = shallowRef()
const pendingCode = ref<string | null>(null)

onMounted(() => {
  if (!editorContainer.value) return
  editor.value = monaco.editor.create(editorContainer.value, {})
})
</script>

<template>
  <div class="editor-wrapper">
<div ref="editorContainer" class="editor-container" />
  </div>
</template>
"#,
        &options,
        &verter_opts,
        &VueMacroSemanticInput::Runtime(runtime),
        &alloc,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.unwrap();
    let code = &script.code;

    // Extract the __returned__ assignment
    let returned_idx = code
        .find("const __returned__ = ")
        .unwrap_or_else(|| panic!("Must have __returned__ assignment. Full output:\n{}", code));
    let returned_rest = &code[returned_idx..];
    let returned_end = returned_rest.find(';').unwrap_or(returned_rest.len());
    let returned_stmt = &returned_rest[..returned_end];

    // Must NOT return empty - Vue's official compiler returns all top-level bindings
    assert!(
        !returned_stmt.contains("= {}"),
        "setup() must NOT return empty. Returned was: '{}'. Full:\n{}",
        returned_stmt,
        code
    );

    // editorContainer must be in return for template ref binding
    assert!(
        returned_stmt.contains("editorContainer"),
        "return must include 'editorContainer' for template ref. Returned: '{}'. Full:\n{}",
        returned_stmt,
        code
    );
}

// @ai-generated - TDD test: type-only defineProps with type reference from companion <script> block
#[test]
fn cross_block_type_resolution_for_define_props() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_prop_at_macro_argument(
                "title",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "description",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "color",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
        ],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script lang="ts">
export interface AlertProps {
  title?: string
  description?: string
  color?: string
}
</script>
<script setup lang="ts">
const props = withDefaults(defineProps<AlertProps>(), {
  color: 'primary'
})
</script>
<template><div>{{ props.title }}</div></template>"#,
        runtime,
    );
    let script = result.script.as_ref().expect("script block");
    // All three props should be declared in the runtime props object
    assert!(
        script.code.contains("title:")
            && script.code.contains("description:")
            && script.code.contains("color:"),
        "All props from companion-block interface should be in runtime props, got:\n{}",
        script.code
    );
}

// ==================== Template binding resolution ====================

#[test]
pub(super) fn imported_function_in_template_gets_setup_prefix() {
    // When a function is imported in <script setup> and used in the template,
    // it should be resolved to $setup.fn or _ctx.fn (not left as bare identifier)
    let result = compile_sfc(
        r#"<script setup lang="ts">
import { isNullish } from './utils'
const val = ref(null)
</script>
<template><div v-if="isNullish(val)">empty</div><div v-else>has value</div></template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let script = result.script.as_ref().expect("script block");
    // The function should be prefixed with $setup. to be accessible in the render context
    assert!(
        tpl.code.contains("$setup.isNullish"),
        "Imported function used in template should have $setup prefix\ntemplate:\n{}\nscript:\n{}",
        tpl.code,
        script.code
    );
    // The import must also be returned from setup
    assert!(
        script.code.contains("isNullish"),
        "Imported function should be returned from setup\n{}",
        script.code
    );
}

#[test]
pub(super) fn companion_script_import_available_in_template() {
    // When a function is imported in the companion <script> block (not <script setup>),
    // it should still be available in the template via $setup prefix, and returned from setup.
    let result = compile_sfc(
        r#"<script lang="ts">
import { isNullish } from './utils'
export interface MyProps { value?: string }
</script>
<script setup lang="ts">
const props = defineProps<MyProps>()
</script>
<template><div v-if="isNullish(props.value)">empty</div></template>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let script = result.script.as_ref().expect("script block");
    // The companion import should be returned from setup so it's available at runtime
    assert!(
        script.code.contains("return") && script.code.contains("isNullish"),
        "Companion script import should be returned from setup\nscript:\n{}",
        script.code
    );
    // The companion import should be resolved in the template (not _ctx. which won't work)
    assert!(
        tpl.code.contains("$setup.isNullish"),
        "Companion script import used in template should have $setup prefix\ntemplate:\n{}",
        tpl.code
    );
}

// ==================== Vapor mode: __vapor flag and _ctx. prefix ====================

/// Official `@vue/compiler-sfc`'s non-TS `compileScript` branch builds
/// `__vapor: true` into the SAME accumulated `runtimeOptions` string as
/// `__name`/`props`/`emits` — spliced into the object literal as ONE
/// inline property, never a separate trailing `__sfc__.__vapor = true`
/// assignment (confirmed directly against the vendored rc.5 compiler
/// source, and against the pinned rc.5 golden for
/// `basic-interpolation.vue`'s vapor cell: `{ __name: '…', __vapor: true,
/// setup(…) {…} }`).
#[test]
fn vapor_script_contains_vapor_flag() {
    let result = compile_sfc_vapor(
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>",
    );
    let script = result.script.as_ref().expect("should have script");
    assert!(
        script.code.contains("__vapor: true,"),
        "Vapor script's __vapor flag must be an inline object-literal \
         property (after __name, before setup), got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("__sfc__.__vapor ="),
        "__vapor must not be a separate trailing assignment, got:\n{}",
        script.code
    );
}

#[test]
fn vapor_template_uses_ctx_prefix_not_setup() {
    let result = compile_sfc_vapor(
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>",
    );
    let tpl = result.template.as_ref().expect("should have template");
    assert!(
        !tpl.code.contains("$setup."),
        "Vapor template should not use $setup. prefix, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("_ctx.msg"),
        "Vapor template should use _ctx. prefix for bindings, got:\n{}",
        tpl.code
    );
}

#[test]
fn vapor_props_use_ctx_prefix() {
    let result = compile_sfc_vapor(
        "<script setup>\nconst props = defineProps({ msg: String })\n</script>\n<template><div>{{ props.msg }}</div></template>",
    );
    let tpl = result.template.as_ref().expect("should have template");
    assert!(
        !tpl.code.contains("$setup.")
            && !tpl.code.contains("$props.")
            && !tpl.code.contains("__props."),
        "Vapor props should not use $setup./$props./__props. prefix, got:\n{}",
        tpl.code
    );
}

// ══════════════════════════════════════════════════════════════════════
// Bug 3: Destructured props binding — template binding resolution tests
// ══════════════════════════════════════════════════════════════════════

/// @ai-generated — Destructured defineProps should resolve to $props. prefix in template
#[test]
fn destructured_define_props_resolves_to_props_prefix() {
    let result = compile_sfc(
        r#"<template><div>{{ msg }}</div></template>
<script setup lang="ts">const { msg } = defineProps<{ msg: string }>()</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.msg"),
        "destructured defineProps prop should resolve to $props.msg, got:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("_ctx.msg"),
        "destructured defineProps prop should NOT use _ctx prefix, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Aliased destructured defineProps: `const { msg: m }` → $props.m
#[test]
fn aliased_destructured_define_props_resolves_to_props_prefix() {
    let result = compile_sfc(
        r#"<template><div>{{ m }}</div></template>
<script setup lang="ts">const { msg: m } = defineProps<{ msg: string }>()</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.m"),
        "aliased destructured prop should resolve to $props.m, got:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("_ctx.m"),
        "aliased destructured prop should NOT use _ctx prefix, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Destructured withDefaults should resolve to $props. prefix
#[test]
fn destructured_with_defaults_resolves_to_props_prefix() {
    let result = compile_sfc(
        r#"<template><div>{{ msg }}</div></template>
<script setup lang="ts">const { msg } = withDefaults(defineProps<{ msg?: string }>(), { msg: 'hello' })</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.msg"),
        "destructured withDefaults prop should resolve to $props.msg, got:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("_ctx.msg"),
        "destructured withDefaults prop should NOT use _ctx prefix, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Multiple destructured props mixed with setup bindings
#[test]
fn destructured_props_mixed_with_setup_bindings() {
    let result = compile_sfc(
        r#"<template><div>{{ a }} {{ b }}</div></template>
<script setup lang="ts">
import { ref } from 'vue'
const { a } = defineProps<{ a: string }>()
const b = ref(0)
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.a"),
        "destructured prop 'a' should resolve to $props.a, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("$setup.b"),
        "setup ref 'b' should resolve to $setup.b, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Destructured props should NOT appear in setup return object
#[test]
fn destructured_props_not_in_setup_return() {
    let result = compile_sfc(
        r#"<template><div>{{ msg }}</div></template>
<script setup lang="ts">const { msg } = defineProps<{ msg: string }>()</script>"#,
    );
    let script = result.script.as_ref().expect("script block");
    // The setup return should be empty or not contain 'msg' (props use $props, not $setup)
    assert!(
        !script.code.contains("return { msg }") && !script.code.contains("return { msg,"),
        "destructured prop 'msg' should NOT be in setup return, got:\n{}",
        script.code
    );
}

/// @ai-generated — Destructured withDefaults with multiple props including rest
#[test]
fn destructured_with_defaults_multiple_props() {
    let result = compile_sfc(
        r#"<template><div>{{ a }} {{ b }}</div></template>
<script setup lang="ts">const { a, b } = withDefaults(defineProps<{ a?: string, b?: number }>(), { a: 'x', b: 1 })</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.a"),
        "destructured withDefaults prop 'a' should resolve to $props.a, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("$props.b"),
        "destructured withDefaults prop 'b' should resolve to $props.b, got:\n{}",
        tpl.code
    );
}

/// @ai-generated — Destructured withDefaults with unresolvable imported type
/// should still resolve destructured props to $props. prefix (the oku-primitives bug)
#[test]
fn destructured_with_defaults_unresolvable_type_resolves_to_props_prefix() {
    let result = compile_sfc(
        r#"<template><div>{{ label }}</div></template>
<script setup lang="ts">
import type { LabelProps } from './Label.ts'
const { label } = withDefaults(defineProps<LabelProps>(), { label: 'hello' })
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("$props.label"),
        "destructured prop with unresolvable type should resolve to $props.label, got:\n{}",
        tpl.code
    );
    assert!(
        !tpl.code.contains("_ctx.label"),
        "destructured prop with unresolvable type should NOT use _ctx prefix, got:\n{}",
        tpl.code
    );
}

#[test]
fn vdom_mode_uses_setup_prefix() {
    let result = compile_sfc(
        "<script setup>\nconst msg = 'hello'\n</script>\n<template><div>{{ msg }}</div></template>",
    );
    let tpl = result.template.as_ref().expect("should have template");
    assert!(
        tpl.code.contains("$setup.msg"),
        "VDOM mode should use $setup. prefix for setup bindings, got:\n{}",
        tpl.code
    );
}

// ==================== Async setup: _withAsyncContext ====================

#[test]
fn async_setup_wraps_await_with_async_context() {
    // Vue wraps top-level await in <script setup> with _withAsyncContext
    // to preserve component instance context across async boundaries.
    let result = compile_sfc(
        r#"<script setup>
const data = await fetch('/api').then(r => r.json());
</script>
<template>
  <div />
</template>"#,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let script = result.script.as_ref().expect("script block");

    // Should have async setup
    assert!(
        script.code.contains("async setup("),
        "setup should be async, got:\n{}",
        script.code
    );

    // Should use _withAsyncContext wrapper
    assert!(
        script.code.contains("_withAsyncContext"),
        "await should be wrapped with _withAsyncContext, got:\n{}",
        script.code
    );

    // Should declare __temp and __restore
    assert!(
        script.code.contains("__temp") && script.code.contains("__restore"),
        "should declare __temp and __restore, got:\n{}",
        script.code
    );

    // Should import withAsyncContext from vue
    assert!(
        script.code.contains("withAsyncContext"),
        "should import withAsyncContext from vue, got:\n{}",
        script.code
    );
}

#[test]
fn async_setup_wraps_dynamic_import_await() {
    // The Editor.vue pattern: const editor = await import(...)
    let result = compile_sfc(
        r#"<script setup>
const props = defineProps(['type']);
const editor = await import(`./editors/${props.type}`).then(x => x.default);
</script>
<template>
  <component :is="editor" />
</template>"#,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let script = result.script.as_ref().expect("script block");

    // The await argument should be wrapped in an arrow function for _withAsyncContext
    assert!(
        script.code.contains("_withAsyncContext("),
        "dynamic import await should use _withAsyncContext, got:\n{}",
        script.code
    );
}

// ==================== Import elision for type-only usage ====================

#[test]
fn import_specifier_used_only_as_type_should_be_elided() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import { doSomething, AuthError } from "some-lib";
import type { UserCredential } from "some-lib";

const emit = defineEmits({
  error(error: AuthError) {
    return true;
  },
  linked(credential: UserCredential) {
    return true;
  },
});

doSomething();
</script>
<template>
  <div />
</template>"#,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let script = result.script.as_ref().expect("script block");

    // Extract the __returned__ object to verify import elision
    let returned_start = script
        .code
        .find("const __returned__ = ")
        .expect("should have __returned__");
    let returned_brace = script.code[returned_start..].find('{').unwrap() + returned_start;
    let returned_end = script.code[returned_brace..].find('}').unwrap() + returned_brace + 1;
    let returned_obj = &script.code[returned_brace..returned_end];

    // AuthError should NOT appear anywhere — it's only used as a type annotation
    // in the defineEmits validator. After type stripping, it has no runtime references,
    // so it's elided from both the import statement and __returned__.
    assert!(
        !script.code.contains("AuthError"),
        "AuthError should be fully elided (only used as type), got:\n{}",
        script.code
    );
    // doSomething should be in the import (used at runtime in script body)
    assert!(
        script.code.contains("doSomething"),
        "doSomething should remain in imports (used at runtime), got:\n{}",
        script.code
    );
    // doSomething IS in __returned__: official's non-inline genSetupReturn
    // includes every setup-scope binding unconditionally (no template-usage
    // filter) (matching official's unconditional-inclusion rule).
    assert!(
        returned_obj.contains("doSomething"),
        "doSomething (a value import) should be in __returned__ per official's \
         unconditional-inclusion rule, got:\n{}",
        returned_obj
    );
    // import type should be fully stripped
    assert!(
        !script.code.contains("UserCredential"),
        "import type should be stripped, got:\n{}",
        script.code
    );
}

#[test]
pub(super) fn import_used_in_template_should_be_in_returned() {
    let result = compile_sfc(
        r#"<script setup lang="ts">
import { formatDate } from "./utils";
import MyComponent from "./MyComponent.vue";
import { helperFn } from "./helpers";
</script>
<template>
  <MyComponent>{{ formatDate(new Date()) }}</MyComponent>
</template>"#,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let script = result.script.as_ref().expect("script block");

    let returned_start = script
        .code
        .find("const __returned__ = ")
        .expect("should have __returned__");
    let returned_brace = script.code[returned_start..].find('{').unwrap() + returned_start;
    let returned_end = script.code[returned_brace..].find('}').unwrap() + returned_brace + 1;
    let returned_obj = &script.code[returned_brace..returned_end];

    // Imports used in template should be in __returned__
    assert!(
        returned_obj.contains("formatDate"),
        "formatDate (used in template) should be in __returned__, got:\n{}",
        returned_obj
    );
    assert!(
        returned_obj.contains("MyComponent"),
        "MyComponent (used in template) should be in __returned__, got:\n{}",
        returned_obj
    );
    // helperFn is used NOWHERE — not the script body, not the template — so
    // it was already dropped from the import statement itself by
    // `filter_import_specifiers`; __returned__ must not reference a name
    // that isn't actually imported. `build_returned_object`'s
    // unconditional-inclusion rule widens what counts as used (script-body
    // usage, not just template usage) — it never resurrects a genuinely
    // dead import that was already elided.
    assert!(
        !returned_obj.contains("helperFn"),
        "helperFn (unused anywhere) must not be in __returned__ — it was already \
         dropped from the import statement, got:\n{}",
        returned_obj
    );
}

// ==================== Companion script import elision ====================

#[test]
pub(super) fn companion_script_type_only_import_not_in_returned() {
    // Companion <script> imports that are only used as type assertions should NOT
    // be in __returned__. This matches Vue's official compiler behavior.
    // Regression test: a type-only companion-script import must not appear in
    // __returned__ (a build failure otherwise, matching the official compiler).
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [crate::test_helpers::runtime_prop(
            "items",
            false,
            [verter_macro_dto::RuntimeConstructor::Array],
        )],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script lang="ts">
import { computed, defineComponent } from "vue";
import { CurrencyCodes, isArray } from "vue-composable";
import { CustomField as CustomFieldType } from "@acme/shared";

function getDefaultValue(field: CustomFieldType) {
  return { currency: "EUR" as CurrencyCodes, value: 0 };
}

export default defineComponent({});
</script>
<script setup lang="ts">
import { HButton } from "@acme/ui";

const props = defineProps<{ items: string[] }>();

function doStuff() {
  if (isArray(props.items)) {
    return getDefaultValue({ type: "money" } as CustomFieldType);
  }
}
</script>
<template>
  <HButton :label="doStuff()" />
</template>"#,
        runtime,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let script = result.script.as_ref().expect("script block");

    let returned_start = script
        .code
        .find("const __returned__ = ")
        .expect("should have __returned__");
    let returned_brace = script.code[returned_start..].find('{').unwrap() + returned_start;
    let returned_end = script.code[returned_brace..].find('}').unwrap() + returned_brace + 1;
    let returned_obj = &script.code[returned_brace..returned_end];

    // CurrencyCodes is only used as a type assertion in the companion body.
    // It should NOT be in __returned__ (would cause Rollup error if it's
    // a type-only export from the source package).
    assert!(
        !returned_obj.contains("CurrencyCodes"),
        "CurrencyCodes (type-only in companion) should NOT be in __returned__, got:\n{}",
        returned_obj
    );

    // CustomFieldType is only used as a type annotation.
    // Should NOT be in __returned__.
    assert!(
        !returned_obj.contains("CustomFieldType"),
        "CustomFieldType (type-only in companion) should NOT be in __returned__, got:\n{}",
        returned_obj
    );

    // `computed` is a companion import used NOWHERE (not the companion
    // script body, not the template, not setup) — genuinely dead, so it was
    // already dropped from the import statement; __returned__ must not
    // reference it. The unconditional-inclusion rule widens what counts as
    // used (script-body usage too, not just template usage), but never
    // resurrects a genuinely dead import.
    assert!(
        !returned_obj.contains("computed"),
        "computed (unused anywhere) must not be in __returned__, got:\n{}",
        returned_obj
    );
    // `defineComponent` IS used in the companion script body
    // (`export default defineComponent({})`), so official's rule would
    // include it — but `build_returned_object`'s `runtime_text` param is the
    // SETUP block's own stripped body only (`compute_runtime_text` in
    // `process_script_setup`), so a companion-script-BODY-only usage is
    // currently invisible to that check and stays excluded. Closing this
    // fully needs the companion script's own runtime text threaded in too.
    assert!(
        !returned_obj.contains("defineComponent"),
        "defineComponent: companion-script-body-only usage is not yet detected \
         by build_returned_object (documented residual gap), got:\n{}",
        returned_obj
    );

    // isArray is used in setup body (`isArray(props.items)`) but NOT in the
    // template — still included per D6 (script-body usage counts).
    assert!(
        returned_obj.contains("isArray"),
        "isArray (used in the setup script body) should be in __returned__ per \
         official's unconditional-inclusion rule, got:\n{}",
        returned_obj
    );

    // HButton IS used in template — should be in __returned__
    assert!(
        returned_obj.contains("HButton"),
        "HButton (used in template) should be in __returned__, got:\n{}",
        returned_obj
    );

    // doStuff IS a setup declaration used in template — should be in __returned__
    assert!(
        returned_obj.contains("doStuff"),
        "doStuff (setup function used in template) should be in __returned__, got:\n{}",
        returned_obj
    );
}

#[test]
pub(super) fn companion_script_import_used_in_template_in_returned() {
    // Companion <script> value imports are in __returned__ regardless of
    // template usage (official's unconditional-inclusion rule) — including
    // ones used in the template, which this test also covers.
    let result = compile_sfc(
        r#"<script lang="ts">
import { formatCurrency } from "./utils";
import { unusedHelper } from "./helpers";

export default {};
</script>
<script setup lang="ts">
const msg = "hello";
</script>
<template>
  <div>{{ formatCurrency(42) }}</div>
</template>"#,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let script = result.script.as_ref().expect("script block");

    let returned_start = script
        .code
        .find("const __returned__ = ")
        .expect("should have __returned__");
    let returned_brace = script.code[returned_start..].find('{').unwrap() + returned_start;
    let returned_end = script.code[returned_brace..].find('}').unwrap() + returned_brace + 1;
    let returned_obj = &script.code[returned_brace..returned_end];

    // formatCurrency is used in template — should be in __returned__
    assert!(
        returned_obj.contains("formatCurrency"),
        "formatCurrency (companion import used in template) should be in __returned__, got:\n{}",
        returned_obj
    );

    // unusedHelper is used NOWHERE (not the companion script body, not the
    // template) — genuinely dead, already dropped from the import statement,
    // so it must not appear in __returned__. The unconditional-inclusion
    // rule widens inclusion to script-body-only usage, not to dead imports.
    assert!(
        !returned_obj.contains("unusedHelper"),
        "unusedHelper (unused anywhere) must not be in __returned__, got:\n{}",
        returned_obj
    );
}

// ==================== Reserved word props ====================

#[test]
fn class_prop_uses_bracket_notation_vdom() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [crate::test_helpers::runtime_prop(
            "class",
            true,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let code = compile_and_validate_template_with_runtime(
        r#"<script setup>
defineProps<{ class?: string }>()
</script>
<template>
  <div :class="class"></div>
</template>"#,
        runtime,
    );
    // Must use bracket notation for JS reserved word "class"
    assert!(
        code.contains(r#"$props["class"]"#),
        "Expected $props[\"class\"] in VDOM output, got:\n{}",
        code
    );
}

#[test]
fn class_prop_on_component_uses_bracket_notation_vdom() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [crate::test_helpers::runtime_prop(
            "class",
            true,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let code = compile_and_validate_template_with_runtime(
        r#"<script setup>
import Comp from './Comp.vue'
const props = defineProps<{ class?: string }>()
</script>
<template>
  <Comp :class="class" />
</template>"#,
        runtime,
    );
    // Must use bracket notation for JS reserved word "class"
    assert!(
        code.contains(r#"$props["class"]"#),
        "Expected $props[\"class\"] in VDOM output, got:\n{}",
        code
    );
}

#[test]
fn class_prop_uses_bracket_notation_vapor() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [crate::test_helpers::runtime_prop(
            "class",
            true,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let code = compile_and_validate_vapor_template_with_runtime(
        r#"<script setup>
defineProps<{ class?: string }>()
</script>
<template>
  <div :class="class"></div>
</template>"#,
        runtime,
    );
    // Vapor props use $props prefix (official: type === "props" ? "$props" : "_ctx");
    // must use bracket notation for the keyword "class".
    assert!(
        code.contains(r#"$props["class"]"#),
        "Expected $props[\"class\"] in Vapor output, got:\n{}",
        code
    );
}

// ==================== Top-level await ====================

#[test]
fn top_level_await_produces_async_setup() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [crate::test_helpers::runtime_prop(
            "id",
            false,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
const props = defineProps<{
  id: string
}>()

const item = (await getById(props.id))!

const name = item.name
</script>
<template>
  <div>{{ name }}</div>
</template>"#,
        runtime,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let script = result.script.as_ref().expect("script block");
    assert!(
        script.code.contains("async setup("),
        "Expected async setup() for top-level await, got:\n{}",
        script.code
    );
}

#[test]
fn vue_ide_carrier_declares_its_official_jsx_authority_without_moving_first_script_mapping() {
    let source = concat!(
        "<script setup lang=\"ts\">\n",
        "interface Props { label: string }\n",
        "const props = defineProps<Props>()\n",
        "</script>\n",
        "<template><div class=\"card\">{{ props.label }}</div></template>\n",
    );
    let result = compile_tsx(source);
    let tsx = result.tsx.expect("Vue IDE carrier");

    assert!(
        tsx.code.starts_with("/** @jsxImportSource vue */\n"),
        "the official Vue JSX authority must be compiler-owned per file:\n{}",
        tsx.code
    );

    let map = oxc_sourcemap::SourceMap::from_json_string(&tsx.source_map)
        .expect("valid Vue IDE source map");
    assert!(
        map.get_tokens()
            .filter(|token| token.get_source_id().is_some())
            .all(|token| token.get_dst_line() > 0),
        "the generated JSX-authority line must be wholly unmapped"
    );
    let interface_mapping = map
        .get_tokens()
        .find(|token| {
            token.get_source_id().is_some() && token.get_src_line() == 1 && token.get_src_col() == 0
        })
        .expect("first script declaration has a source mapping");
    assert_eq!(interface_mapping.get_dst_line(), 1);
    assert_eq!(interface_mapping.get_dst_col(), 0);
    assert!(
        tsx.code
            .lines()
            .nth(1)
            .is_some_and(|line| line.starts_with("interface Props")),
        "the first authored script declaration must retain generated column zero:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_script_with_imports() {
    let result = compile_tsx(
        r#"<script setup>
import { ref } from 'vue'
import type { Foo } from './types'
const count = ref(0)
</script>

<template>
  <div>{{ count }}</div>
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    // Imports should be hoisted above the wrapper function
    let fn_pos = tsx
        .code
        .find("export function ___VERTER___TemplateBindingFN")
        .expect("wrapper function");
    let ref_pos = tsx
        .code
        .find("import { ref } from 'vue'")
        .expect("ref import");
    let type_pos = tsx
        .code
        .find("import type { Foo } from './types'")
        .expect("type import");
    assert!(ref_pos < fn_pos, "ref import should be hoisted");
    assert!(type_pos < fn_pos, "type import should be hoisted");
}

#[test]
fn tsx_infer_function_does_not_connect_classic_script_to_template() {
    let result = compile_tsx(
        r#"<script lang="ts">
function handleClick(e) {
  return e
}

export default {}
</script>
<template>
  <button @click="handleClick">Click</button>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("function handleClick(e)"),
        "Classic-script parameter must stay authored, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("...[e]"),
        "Template-driven inference is script-setup-only, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_infer_function_skips_js_scripts() {
    let result = compile_tsx(
        r#"<script setup lang="js">
function handleClick(e) {
  return e
}
</script>
<template>
  <button @click="handleClick">Click</button>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("function handleClick(e)"),
        "JS script function should remain unchanged, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("Parameters<"),
        "JS script should not get inferred TS parameter types, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_ref_use_template_ref_skips_js_scripts() {
    let result = compile_tsx(
        r#"<script setup lang="js">
let el = useTemplateRef('el')
</script>
<template>
  <div ref="el"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("let el = useTemplateRef('el')"),
        "JS scripts should not inject template-ref generics, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("useTemplateRef<"),
        "JS scripts should not inject useTemplateRef type arguments, got:\n{}",
        tsx.code
    );
}

#[test]
pub(super) fn tsx_template_ref_options_api_setup_function_is_supported() {
    let result = compile_tsx(
        r#"<script lang="ts">
import { defineComponent, useTemplateRef } from 'vue'
export default defineComponent({
  setup() {
    const myRef = useTemplateRef('myRef')
    return { myRef }
  }
})
</script>
<template>
  <div ref="myRef"></div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.code.contains("useTemplateRef<") && tsx.code.contains(r#","myRef">('myRef')"#),
        "Expected nested setup() call to receive inferred generic, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_prop_v5_process_parity_matrix() {
    let cases: [(&str, &[&str], &[&str]); 11] = [
        (
            r#"<template><div test="test" /></template>"#,
            &[r#"<div test="test" />"#],
            &[],
        ),
        (
            r#"<template><div test /></template>"#,
            &[r#"<div test />"#],
            &[],
        ),
        (
            r#"<script setup>
const test = 1
</script>
<template><div :test="test" /></template>"#,
            &[r#"<div test={test} />"#],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const test = 1
</script>
<template><div :test /></template>"#,
            &[r#"<div test={test} />"#],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const testToFoo = 1
</script>
<template><div :test-to-foo /></template>"#,
            &[r#"<div test-to-foo={testToFoo} />"#],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const msg = ''
</script>
<template><div :[msg]="msg" /></template>"#,
            &[r#"<div {...{[msg]: msg}} />"#],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const obj = {}
</script>
<template><div v-bind="obj" /></template>"#,
            &[r#"<div {...obj} />"#],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const test = () => {}
</script>
<template><div @test="test" /></template>"#,
            &[r#"<div onTest={test} />"#],
            &["_ctx."],
        ),
        (
            r#"<script setup>
const test = () => {}
</script>
<template><div @test-camel-case="test" /></template>"#,
            &[r#""onTest-camel-case": test"#],
            &[r#"onTestCamelCase"#, "_ctx."],
        ),
        (
            r#"<template><div aria-label="test" data-test="value" /></template>"#,
            &[r#"<div aria-label="test" data-test="value" />"#],
            &[],
        ),
        (
            r#"<script setup>
const ok = true
</script>
<template><div :style="{ color: 'red' }" style="color: blue" :class="{ active: ok }" class="btn" /></template>"#,
            &[
                r#"normalizeStyle([{ color: 'red' },"color: blue"])"#,
                r#"normalizeClass([{ active: ok },"btn"])"#,
            ],
            &["_ctx.", r#"style="color: blue""#, r#"class="btn""#],
        ),
    ];

    for (source, required_snippets, forbidden_snippets) in cases {
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
}

// ==================== v5/process parity matrices (ported/adapted suites) ====================

/// Adapted parity matrix for:
/// - script/plugins/macros/macros.spec.ts
/// - script/plugins/macros/macros.fixtures.ts
#[test]
fn tsx_macros_v5_process_parity_matrix() {
    type_based_define_props_resolves_to_props_prefix();
    with_defaults_merges_defaults_into_props();
    with_defaults_type_reference();
    define_props_type_with_imported_types();
    with_defaults_imported_types_all_props_present();
    with_defaults_variable_expression_without_declarator_uses_authoritative_props();
    with_defaults_variable_expression_with_declarator_uses_authoritative_props();
    with_defaults_object_literal_defaults_use_authoritative_local_props();
    with_defaults_function_call_defaults_use_authoritative_local_props();
    with_defaults_unresolvable_type_no_defaults();
    with_defaults_resolvable_type_still_works();
    with_defaults_mixed_defaults_use_authoritative_local_props();
    with_defaults_spread_expression_uses_authoritative_local_props();
    cross_block_type_resolution_for_define_props();
    with_defaults_cross_block_type_uses_key_name();

    define_model_declares_prop_and_emit();
    define_model_named_declares_prop_and_emit();
    define_model_with_defaults_resolved_type();
    define_model_with_authoritative_defaults_runtime_variable();
    define_model_with_define_props_object_uses_merge_models();
    define_model_with_typed_with_defaults();
    define_model_with_define_emits_uses_merge_models_for_emits();

    type_based_define_emits_generates_emits_option();
    type_based_define_emits_call_signature_generates_emits_option();
    optional_tuple_element_in_define_emits();

    destructured_define_props_resolves_to_props_prefix();
    aliased_destructured_define_props_resolves_to_props_prefix();
    destructured_with_defaults_resolves_to_props_prefix();
    destructured_props_mixed_with_setup_bindings();
    destructured_prop_in_v_bind();
    destructured_prop_in_event_handler();
    destructured_props_not_in_setup_return();
    destructured_with_defaults_multiple_props();
    destructured_with_defaults_unresolvable_type_resolves_to_props_prefix();
}

/// Adapted parity matrix for:
/// - script/builders/bundle/bundle.spec.ts
#[test]
fn tsx_script_bundle_v5_process_parity_matrix() {
    basic_sfc_compiles();
    style_block_extracted();
    custom_blocks_extracted();
    export_type_hoisted_when_keep_ts();
    tsx_basic_sfc();
    tsx_source_map_script_only();
    tsx_source_map_is_generated();
    tsx_source_map_maps_script_binding();
    tsx_force_js_toggle_does_not_change_code();
    tsx_force_js_toggle_does_not_change_source_map();
}

/// Adapted parity matrix for:
/// - script/plugins/script-default/script-default.spec.ts
#[test]
fn tsx_script_default_v5_process_parity_matrix() {
    dual_script_preserves_named_exports();
    dual_script_export_default_merged_as_options();
    companion_script_import_available_in_template();
    companion_script_type_only_import_not_in_returned();
    companion_script_import_used_in_template_in_returned();
    cross_block_type_resolution_for_define_props();
    with_defaults_cross_block_type_uses_key_name();
}

/// Adapted parity matrix for:
/// - script/plugins/script-block/script-block.spec.ts
#[test]
fn tsx_script_block_v5_process_parity_matrix() {
    ts_return_type_annotation_in_computed();
    ts_return_type_no_strip_mode();
    top_level_await_produces_async_setup();
    async_setup_wraps_await_with_async_context();
    async_setup_wraps_dynamic_import_await();
    tsx_basic_sfc();
    tsx_script_with_imports();
}

/// Adapted parity matrix for:
/// - script/plugins/sfc-cleaner/sfcCleaner.spec.ts
#[test]
fn tsx_script_sfc_cleaner_v5_process_parity_matrix() {
    export_type_stripped_when_force_js();
    export_interface_stripped_when_force_js();
    bare_type_and_interface_stripped_when_force_js();
    import_specifier_used_only_as_type_should_be_elided();
    export_type_hoisted_when_keep_ts();
}

/// Adapted parity matrix for:
/// - script/plugins/attributes/attributes.spec.ts
#[test]
fn tsx_script_attributes_v5_process_parity_matrix() {
    script_attrs_contain_lang();
    export_type_hoisted_when_keep_ts();
    tsx_basic_sfc();
}

/// Adapted parity matrix for:
/// - script/plugins/imports/imports.spec.ts
#[test]
fn tsx_imports_plugin_v5_process_parity_matrix() {
    script_imports_use_as_syntax();
    tsx_script_with_imports();
    import_specifier_used_only_as_type_should_be_elided();
    imported_function_in_template_gets_setup_prefix();
}

/// @ai-generated - TSX source map should map `msg` in script back to the original position
#[test]
fn tsx_source_map_maps_script_binding() {
    let source = r#"<script setup>
const msg = 'hello'
</script>

<template>
  <div>{{ msg }}</div>
</template>
"#;
    let result = compile_tsx(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        !tsx.source_map.is_empty(),
        "TSX source map should not be empty"
    );

    // Parse with oxc_sourcemap and verify we can look up a position
    let sm = oxc_sourcemap::SourceMap::from_json_string(&tsx.source_map)
        .expect("should parse source map");

    // Find "const msg" in the TSX output and look it up
    let msg_offset = tsx
        .code
        .find("const msg")
        .expect("TSX should contain 'const msg'");
    let tsx_line = tsx.code[..msg_offset].matches('\n').count() as u32;
    let tsx_col = (msg_offset
        - tsx.code[..msg_offset]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0)) as u32;

    let lookup_table = sm.generate_lookup_table();
    let token = sm.lookup_token(&lookup_table, tsx_line, tsx_col);

    assert!(
        token.is_some(),
        "Should find a source map token for 'const msg' at TSX line {tsx_line}, col {tsx_col}"
    );

    if let Some(token) = token {
        // "const msg" is on line 1 (0-indexed) in the original source
        let original_msg_line = source[..source.find("const msg").unwrap()]
            .matches('\n')
            .count() as u32;
        assert_eq!(
            token.get_src_line(),
            original_msg_line,
            "Source line should map back to the original 'const msg' line"
        );
    }
}

/// @ai-generated - TSX source map for script-only SFC (no template)
#[test]
fn tsx_source_map_script_only() {
    let source = r#"<script setup>
const msg = 'hello'
</script>"#;
    let result = compile_tsx(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);

    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        !tsx.source_map.is_empty(),
        "TSX source map should not be empty even for script-only SFC"
    );
}

/// @ai-generated - prop_constness_overrides threads through compile pipeline without error.
/// Verifies that passing const prop overrides produces valid compiled output.
#[test]
fn const_props_override_compiles_successfully() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let mut const_set = rustc_hash::FxHashSet::default();
    const_set.insert("msg".to_string());
    let verter_opts = VerterCompileOptions {
        force_js: true,
        prop_constness_overrides: Some(const_set),
        ..Default::default()
    };
    let result = compile(
        r#"<script setup>
const props = defineProps({ msg: String, count: Number })
</script>
<template>
  <div>{{ msg }} {{ count }}</div>
</template>"#,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(!tpl.code.trim().is_empty(), "template code is empty");
    // Validate the generated JS is syntactically valid
    let alloc2 = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", tpl.code);
    let parsed = verter_parser::oxc_parse::Parser::new(&alloc2, &wrapped, source_type).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "Generated JS parse error: {:?}\n--- generated code ---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tpl.code
    );
}

/// @ai-generated - prop_constness_overrides threads through Vapor compile pipeline.
#[test]
fn const_props_override_compiles_vapor() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let mut const_set = rustc_hash::FxHashSet::default();
    const_set.insert("msg".to_string());
    let verter_opts = VerterCompileOptions {
        force_js: true,
        force_vapor: true,
        prop_constness_overrides: Some(const_set),
        ..Default::default()
    };
    let result = compile(
        r#"<script setup>
const props = defineProps({ msg: String, count: Number })
</script>
<template>
  <div>{{ msg }} {{ count }}</div>
</template>"#,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(!tpl.code.trim().is_empty(), "template code is empty");
    let alloc2 = Allocator::new();
    let source_type = oxc_span::SourceType::mjs();
    let wrapped = format!("import {{ }} from \"vue\";\n{}", tpl.code);
    let parsed = verter_parser::oxc_parse::Parser::new(&alloc2, &wrapped, source_type).parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "Vapor generated JS parse error: {:?}\n--- generated code ---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        tpl.code
    );
}

/// @ai-generated - Built-in component imports should appear in template.imports.
#[test]
pub(super) fn builtin_component_in_imports_list() {
    let result = compile_sfc(
        r#"<template>
  <Suspense>
    <div />
  </Suspense>
</template>"#,
    );
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result.template.as_ref().expect("template block");
    // The imports list should include _Suspense
    assert!(
        tpl.imports.contains(&"_Suspense"),
        "template.imports should contain _Suspense, got: {:?}",
        tpl.imports
    );
}

/// @ai-generated — TSX source map: bound prop maps back
#[test]
fn tsx_sourcemap_bound_prop() {
    let source = r#"<script setup>
const value = 42
</script>

<template>
  <input :value="value" />
</template>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);
}

/// @ai-generated — TSX source map: script-only SFC
#[test]
fn tsx_sourcemap_script_only() {
    let source = r#"<script setup>
const x = 1
const y = 2
</script>
"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");
    verify_sourcemap_tokens_in_bounds(source, tsx);
}

/// An ANCESTOR's own hoistable static props (e.g. a root `class="root"`)
/// must number BEFORE a hoistable DESCENDANT's props (e.g. a nested
/// `v-if`/`v-else` branch's synthetic key), matching official's separate
/// document-pre-order `cacheStatic` hoist-numbering pass — even though
/// Verter's codegen itself is bottom-up (child `leave` before parent
/// `leave`). Exact basic-interpolation.vue shape: the official rc.5 golden
/// hoists `{ class: "root" }` as `_hoisted_1` (the root, an ancestor), then
/// the branch keys as `_hoisted_2`/`_hoisted_3` (descendants) — NOT the
/// other way around, which is what a naive bottom-up push order would
/// produce.
#[test]
fn static_hoist_ancestor_props_number_before_descendant_branch_keys() {
    let code = compile_and_validate_hoisted(
        r#"<template><div class="root"><p v-if="count > 0">{{ count }}</p><p v-else>zero</p></div></template>"#,
    );
    assert!(
        code.contains(r#"const _hoisted_1 = { class: "root" }"#),
        "the root's own class object must be _hoisted_1 (registered before its \
         descendants, matching official's pre-order cacheStatic numbering)\n\
         --- code ---\n{}",
        code
    );
    assert!(
        code.contains("const _hoisted_2 = { key: 0 }"),
        "the v-if branch's synthetic key object must be _hoisted_2, got:\n{}",
        code
    );
    assert!(
        code.contains("const _hoisted_3 = { key: 1 }"),
        "the v-else branch's synthetic key object must be _hoisted_3, got:\n{}",
        code
    );
}

#[test]
fn tsx_no_destructured_block_meta_without_setup() {
    // No <script setup> = no destructured block
    let source = r#"<script lang="ts">
export default { setup() { return { x: 1 } } }
</script>
<template><div>{{ x }}</div></template>"#;
    let result = compile_tsx(source);
    let tsx = result.tsx.as_ref().expect("tsx block");
    assert!(
        tsx.destructured_block.is_none(),
        "destructured_block should be None without <script setup>"
    );
}

#[test]
fn tsx_parse_valid_define_props_with_defaults() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_prop(
                "msg",
                false,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
            crate::test_helpers::runtime_prop(
                "count",
                true,
                [verter_macro_dto::RuntimeConstructor::Number],
            ),
        ],
    )]);
    assert_tsx_parses_with_runtime(
        r#"<script setup lang="ts">
const props = withDefaults(defineProps<{
  msg: string
  count?: number
}>(), {
  count: 0,
})
</script>
<template>
  <div>{{ msg }} {{ count }}</div>
</template>"#,
        "defineProps + withDefaults",
        runtime,
    );
}

// =============================================================================
// JSX mode compile tests — JS SFCs produce valid JavaScript + JSDoc output
// =============================================================================

#[test]
fn jsx_compile_basic_script_setup() {
    assert_jsx_parses(
        r#"<script setup>
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
        "basic JS script setup",
    );
}

#[test]
fn jsx_compile_define_props() {
    assert_jsx_parses(
        r#"<script setup>
const props = defineProps({
  msg: String
})
</script>
<template><div>{{ msg }}</div></template>"#,
        "JS defineProps (runtime)",
    );
}

#[test]
fn tsx_attrs_with_generic() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [crate::test_helpers::runtime_prop(
            "items",
            false,
            [verter_macro_dto::RuntimeConstructor::Array],
        )],
    )]);
    let result = compile_tsx_with_runtime(
        r#"<script setup lang="ts" generic="T" attrs="{ value: T }">
defineProps<{ items: T[] }>()
</script>
<template><div /></template>"#,
        runtime,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: attrs type with generic parameter
    assert!(
        tsx.code.contains("___VERTER___attributes<T>"),
        "should emit generic attrs type, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("{ value: T }"),
        "should contain generic type value, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_default_empty_attrs_capture_outer_generic_without_redeclaration() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [crate::test_helpers::runtime_prop(
            "name",
            true,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let result = compile_tsx_with_runtime(
        r#"<script setup lang="ts" generic="T extends string">
defineProps<{ name: T }>()
</script>
<template><div>{{ name }}</div></template>"#,
        runtime,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    assert!(
        tsx.code
            .contains("export function ___VERTER___TemplateBindingFN<T extends string>()"),
        "the authored component generic should remain on the outer wrapper, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("type ___VERTER___attributes = {};"),
        "default empty attributes should remain non-generic, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains(
            "type ___VERTER___Attrs = ___VERTER___attributes & ___VERTER___RootElementProps;"
        ),
        "local attrs aliases should capture the outer generic scope without redeclaration, got:\n{}",
        tsx.code
    );
    let instance_line = tsx
        .code
        .lines()
        .find(|line| line.contains("$attrs:"))
        .expect("generic instance attrs override");
    assert!(
        instance_line.contains("$attrs: ___VERTER___Attrs"),
        "instance attrs override should reference the local non-generic alias, got:\n{instance_line}"
    );

    assert!(
        !tsx.code.contains("___VERTER___attributes<T>"),
        "default attributes alias must not be used with a type argument, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code
            .contains("type ___VERTER___Attrs<T extends string>"),
        "local Attrs alias must not redundantly redeclare the component generic, got:\n{}",
        tsx.code
    );
    assert!(
        !instance_line.contains("$attrs: ___VERTER___Attrs<T>"),
        "instance override must not apply an argument to the local Attrs alias, got:\n{instance_line}"
    );
    assert!(
        !tsx.code
            .lines()
            .any(|line| line.contains("function ___VERTER___Comp")
                && line.contains("<T extends string>")),
        "nested Comp helpers must capture rather than redeclare the outer generic, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code
            .contains("function ___VERTER___getRootComponentPassedProps<T extends string>()"),
        "root-props helper must not carry an unused copy of the outer generic, got:\n{}",
        tsx.code
    );

    let alloc = oxc_allocator::Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "generic default-attrs TSX must parse: {:?}\n---\n{}",
        parsed
            .diagnostics
            .iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>(),
        tsx.code
    );
}

// ══════════════════════════════════════════════════════════════════════════════
// ── Root element prop capture — IDE codegen ─────────────────────────────────
// ══════════════════════════════════════════════════════════════════════════════

#[test]
fn tsx_root_element_props_captured() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const handler = () => {}
</script>
<template><div id="app" :title="'hello'" @click="handler">content</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: Comp function emitted for root element
    assert!(
        tsx.code.contains("function ___VERTER___Comp"),
        "should emit Comp for root element, got:\n{}",
        tsx.code
    );
    // Positive: enhanceElementWithProps receives actual props
    assert!(
        tsx.code.contains(r#""id": "app""#),
        "should have static id prop in Comp, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains(r#""title": 'hello'"#),
        "should have dynamic title bind in Comp, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains(r#""onClick": () => {}"#),
        "should have onClick event in Comp, got:\n{}",
        tsx.code
    );
    // Positive: getRootComponentPassedProps returns actual props
    assert!(
        tsx.code.contains("getRootComponentPassedProps"),
        "should emit getRootComponentPassedProps, got:\n{}",
        tsx.code
    );
    // Negative: class/style should be excluded from props
    assert!(
        !tsx.code.contains(r#""class""#) || !tsx.code.contains("getRootComponentPassedProps"),
        "class should not appear in serialized props"
    );
}

#[test]
fn tsx_root_component_props_captured() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
import { ref } from 'vue'
const el = ref()
</script>
<template><MyComp ref="el" :title="'hello'" /></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: component Comp function has title prop
    assert!(
        tsx.code.contains(r#""title": 'hello'"#),
        "should have title prop in Comp, got:\n{}",
        tsx.code
    );
    // Positive: getRootComponentPassedProps returns actual props
    assert!(
        tsx.code.contains("getRootComponentPassedProps")
            && tsx.code.contains(r#""title": 'hello'"#),
        "getRootComponentPassedProps should return actual props, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_root_element_no_props_empty_object() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
</script>
<template><div>hello</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: Comp emitted even for root with no props
    assert!(
        tsx.code.contains("function ___VERTER___Comp"),
        "should emit Comp for root element, got:\n{}",
        tsx.code
    );
    // Positive: getRootComponentPassedProps returns empty object
    assert!(
        tsx.code
            .contains("getRootComponentPassedProps() { return {}; }"),
        "should return empty object when no props, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_inherit_attrs_false_omits_root_element_props() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
defineOptions({ inheritAttrs: false })
</script>
<template><div>hello</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: Attrs type should only be explicit attrs (no RootElementProps)
    assert!(
        tsx.code
            .contains("type ___VERTER___Attrs = ___VERTER___attributes;"),
        "inheritAttrs: false should exclude RootElementProps from Attrs, got:\n{}",
        tsx.code
    );
    // Negative: RootElementProps should NOT be in the Attrs union
    let attrs_line = tsx
        .code
        .lines()
        .find(|l| l.contains("type ___VERTER___Attrs"))
        .unwrap_or("");
    assert!(
        !attrs_line.contains("RootElementProps"),
        "Attrs should not include RootElementProps when inheritAttrs: false, got:\n{}",
        attrs_line
    );
}

#[test]
fn tsx_attrs_param_with_generics() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [crate::test_helpers::runtime_prop(
            "items",
            false,
            [verter_macro_dto::RuntimeConstructor::Array],
        )],
    )]);
    let result = compile_tsx_with_runtime(
        r#"<script setup lang="ts" generic="T extends string" attrs="{ value: T }">
defineProps<{ items: T[] }>()
</script>
<template><div /></template>"#,
        runtime,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: both generic bracket and _attrs param, generic BEFORE params
    assert!(
        tsx.code
            .contains("TemplateBindingFN<T extends string>(_attrs: { value: T })"),
        "should have generic bracket + _attrs param, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_attrs_before_generic_in_source_order() {
    // When attrs appears BEFORE generic in the SFC source,
    // the generated TSX must still have generic before params.
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        0,
        verter_macro_dto::PropsDefaultsAssociation::None,
        [crate::test_helpers::runtime_prop(
            "value",
            false,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let result = compile_tsx_with_runtime(
        r#"<script setup lang="ts" attrs="{ class: string }" generic="T extends string">
defineProps<{ value: T }>()
</script>
<template><div /></template>"#,
        runtime,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Positive: generic BEFORE params regardless of source order
    assert!(
        tsx.code
            .contains("TemplateBindingFN<T extends string>(_attrs: { class: string })"),
        "generic must come before params even when attrs is first in source, got:\n{}",
        tsx.code
    );

    // Negative: must NOT have generic after params
    assert!(
        !tsx.code.contains("})<"),
        "generic must not appear after closing paren, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_generic_content_is_sourcemapped() {
    let source = r#"<script setup lang="ts" generic="T extends string">
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#;
    let result = compile_tsx_with_source_map(source);
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    let sm =
        oxc_sourcemap::SourceMap::from_json_string(&tsx.source_map).expect("valid source map JSON");
    let lookup = sm.generate_lookup_table();

    // Find "T extends string" in the generated TSX output
    let gen_pos = tsx
        .code
        .find("T extends string")
        .expect("should find 'T extends string' in TSX output");

    // Find "T extends string" in the original SFC source
    let src_pos = source
        .find("T extends string")
        .expect("should find 'T extends string' in SFC source");

    // Look up the sourcemap token at the generated position
    let gen_line = tsx.code[..gen_pos].matches('\n').count() as u32;
    let gen_col = (gen_pos - tsx.code[..gen_pos].rfind('\n').map_or(0, |p| p + 1)) as u32;

    let token = sm
        .lookup_token(&lookup, gen_line, gen_col)
        .expect("should have sourcemap token for generic content");

    // The token should map back to the original source position
    let src_line = source[..src_pos].matches('\n').count() as u32;
    let src_col = (src_pos - source[..src_pos].rfind('\n').map_or(0, |p| p + 1)) as u32;

    assert_eq!(
        token.get_src_line(),
        src_line,
        "generic content should map back to SFC source line"
    );
    assert_eq!(
        token.get_src_col(),
        src_col,
        "generic content should map back to SFC source column"
    );
}

#[test]
fn tsx_template_first_empty_script_setup() {
    let result = compile_tsx(
        r#"<template>
	<section class="page">
		<h1>Chat</h1>
	</section>
</template>
<script setup lang="ts">
</script>"#,
    );
    let tsx = result.tsx.expect("should produce TSX");

    let fn_open = tsx.code.find("function ___VERTER___TemplateBindingFN");
    let fn_close = tsx.code.find("} // close templateBindingFN");
    assert!(fn_open.is_some(), "should have function: {}", tsx.code);
    assert!(fn_close.is_some(), "should have close: {}", tsx.code);
    assert!(
        fn_open.unwrap() < fn_close.unwrap(),
        "function open must come before close: {}",
        tsx.code
    );
    // JSX must be inside the function
    let jsx_pos = tsx.code.find("<section").expect("should have JSX");
    assert!(
        fn_open.unwrap() < jsx_pos && jsx_pos < fn_close.unwrap(),
        "template JSX must be inside the function: {}",
        tsx.code
    );
}

#[test]
fn dual_script_tsx_does_not_leak_raw_template() {
    // Vuetify pattern: <script setup> + <script> (Options API) + <template>
    // The TSX output should NOT contain raw HTML template tags.
    let result = compile_tsx(
        r#"<template>
  <div>
    <MyComp v-model="step">
      <template v-slot:header>
        <span>Title</span>
      </template>
      <template v-slot:body="{ item }">
        <p>{{ item }}</p>
      </template>
    </MyComp>
  </div>
</template>

<script setup>
const step = ref(1)
</script>

<script>
export default {
  components: {},
}
</script>"#,
    );

    let tsx = result
        .tsx
        .as_ref()
        .expect("tsx output should exist for dual-script SFC");

    // Positive: should contain JSX elements
    assert!(
        tsx.code.contains("<div"),
        "TSX should contain JSX elements. Got:\n{}",
        tsx.code
    );

    // Negative: raw <template v-slot:xxx> must NOT leak into JSX output
    assert!(
        !tsx.code.contains("<template v-slot"),
        "raw <template v-slot:...> must not appear in TSX output. Got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("</template>"),
        "raw </template> closing tag must not appear in TSX output. Got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_destructured_prop_liveness_discriminates_template_use_from_true_unused() {
    let runtime = || {
        crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
            0,
            0,
            verter_macro_dto::PropsDefaultsAssociation::None,
            [crate::test_helpers::runtime_prop_at_macro_argument(
                "count",
                false,
                [verter_macro_dto::RuntimeConstructor::Number],
            )],
        )])
    };
    let used = compiled_tsx_identifier_facts_with_runtime(
        r#"<script setup lang="ts">
interface Props { count: number }
const { count } = defineProps<Props>()
</script>
<template><div>{{ count }}</div></template>"#,
        "count",
        runtime(),
    );
    assert_eq!(
        used.bindings, 1,
        "the real destructured binding is preserved"
    );
    assert!(
        used.references >= 1,
        "a template-only use must value-read the source binding so TypeScript cannot emit a false TS6133"
    );

    let unused = compiled_tsx_identifier_facts_with_runtime(
        r#"<script setup lang="ts">
interface Props { count: number }
const { count } = defineProps<Props>()
</script>
<template><div>static</div></template>"#,
        "count",
        runtime(),
    );
    assert_eq!(
        unused.bindings, 1,
        "the real destructured binding is preserved"
    );
    assert_eq!(
        unused.references, 0,
        "a genuinely unused prop must remain eligible for TypeScript's TS6133"
    );
}

#[test]
fn tsx_unused_script_setup_local_is_omitted_from_unwrap() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const foo = 1
</script>

<template>
  <div>hello</div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Omitted: no value-read entry, no retired type-only entry, and crucially
    // NO `typeof foo` anywhere (which would keep the source decl live and
    // mis-position TS6133 to line 1).
    assert!(
        !tsx.code.contains("foo: foo as unknown as typeof foo"),
        "unused `foo` must NOT keep its value-read unwrap entry.\nTSX:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("foo: undefined as unknown as typeof foo"),
        "unused `foo` must NOT use the retired type-only unwrap entry.\nTSX:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("typeof foo"),
        "unused `foo` must not be referenced via `typeof foo` (keeps source decl live).\nTSX:\n{}",
        tsx.code
    );
    // The user's `const foo` decl survives untouched (it carries TS6133).
    assert!(
        tsx.code.contains("const foo = 1"),
        "the source `const foo` decl must remain.\nTSX:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_template_used_script_setup_local_keeps_value_read() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const foo = 1
</script>

<template>
  <div>{{ foo }}</div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    assert!(
        tsx.code.contains("foo: foo as unknown as typeof foo"),
        "template-used `foo` must keep its value-read unwrap entry.\nTSX:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("foo: undefined as unknown as typeof foo"),
        "template-used `foo` must NOT be demoted to a type-only entry.\nTSX:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_script_used_only_local_keeps_value_read() {
    let result = compile_tsx(
        r#"<script setup lang="ts">
const foo = 1
console.log(foo)
</script>

<template>
  <div>hello</div>
</template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    assert!(
        tsx.code.contains("foo: foo as unknown as typeof foo"),
        "script-used `foo` must keep its value-read unwrap entry.\nTSX:\n{}",
        tsx.code
    );
}

/// Official-Vue parity lock: an optional `boolean` prop emits
/// `{ type: Boolean, required: false }` with NO `default: undefined`, regardless
/// of the prop name. Parameterized over several names to prove the runtime prop
/// shape is NOT keyed on a specific spelling (`rounded` / `trueValue` must behave
/// identically to `foo`).
#[test]
fn optional_boolean_prop_is_official_type_boolean_no_default_for_any_name() {
    for name in ["foo", "rounded", "trueValue", "disabled", "modelValue"] {
        let src = format!(
            "<script setup lang=\"ts\">\ndefineProps<{{ {name}?: boolean }}>()\n</script>\n<template><div/></template>"
        );
        let result = compile_sfc_with_runtime(
            &src,
            crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
                0,
                0,
                verter_macro_dto::PropsDefaultsAssociation::None,
                [crate::test_helpers::runtime_prop(
                    name,
                    true,
                    [verter_macro_dto::RuntimeConstructor::Boolean],
                )],
            )]),
        );
        assert!(
            result.errors.is_empty(),
            "compile errors for {name}: {:?}",
            result.errors
        );
        let code = &result.script.as_ref().unwrap().code;
        assert!(
            code.contains(&format!("{name}: {{ type: Boolean, required: false }}")),
            "optional boolean `{name}` must emit official \
             `{{ type: Boolean, required: false }}`, got:\n{code}"
        );
        assert!(
            !code.contains("default: undefined"),
            "optional boolean `{name}` must NOT emit `default: undefined` (official Vue \
             boolean-casts absent optionals to false), got:\n{code}"
        );

        // Same parity through the withDefaults path (empty defaults object):
        // an undeclared optional boolean stays optional with no default.
        let wd_src = format!(
            "<script setup lang=\"ts\">\nwithDefaults(defineProps<{{ {name}?: boolean }}>(), {{}})\n</script>\n<template><div/></template>"
        );
        let wd = compile_sfc_with_runtime(
            &wd_src,
            crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
                0,
                1,
                verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
                    payload_macro_index: 0,
                    defaults_macro_index: 1,
                },
                [crate::test_helpers::runtime_prop(
                    name,
                    true,
                    [verter_macro_dto::RuntimeConstructor::Boolean],
                )],
            )]),
        );
        assert!(
            wd.errors.is_empty(),
            "wd compile errors {name}: {:?}",
            wd.errors
        );
        let wd_code = &wd.script.as_ref().unwrap().code;
        assert!(
            wd_code.contains(&format!("{name}: {{ type: Boolean, required: false }}")),
            "withDefaults optional boolean `{name}` must emit \
             `{{ type: Boolean, required: false }}`, got:\n{wd_code}"
        );
        assert!(
            !wd_code.contains("default: undefined"),
            "withDefaults optional boolean `{name}` must NOT emit `default: undefined`, got:\n{wd_code}"
        );
    }
}

#[test]
fn force_js_strips_inline_ts_even_with_import_type() {
    let result = compile_sfc(
        r#"
<script setup lang="ts">
import { ref } from 'vue'
import type { Ref } from 'vue'
const count = ref<number>(0)
const msg: string = 'hello'
const x = (y as string)
const z = y!
const w = ({ a: 1 } satisfies { a: number })
</script>
<template><div>{{ count }} {{ msg }}</div></template>
"#,
    );
    let script = result.script.as_ref().expect("script");
    let code = &script.code;
    assert!(!code.contains("import type"), "import type must go: {code}");
    assert!(
        !code.contains("ref<number>"),
        "call type args must go: {code}"
    );
    assert!(
        !code.contains(": string"),
        "var annotations must go: {code}"
    );
    assert!(
        !code.contains(" as string") && !code.contains("as string)"),
        "as-casts must go: {code}"
    );
    assert!(!code.contains("satisfies"), "satisfies must go: {code}");
    assert!(!code.contains("y!"), "non-null must go: {code}");
    assert!(code.contains("ref(0)"), "runtime call remains: {code}");
    assert!(
        code.contains("const msg = 'hello'"),
        "value remains: {code}"
    );
}

/// Control: same body without import type still strips (baseline).
#[test]
fn force_js_strips_inline_ts_without_import_type() {
    let result = compile_sfc(
        r#"
<script setup lang="ts">
import { ref } from 'vue'
const count = ref<number>(0)
const msg: string = 'hello'
const x = (y as string)
</script>
<template><div>{{ count }} {{ msg }}</div></template>
"#,
    );
    let script = result.script.as_ref().expect("script");
    let code = &script.code;
    assert!(!code.contains("ref<number>"), "got: {code}");
    assert!(!code.contains(": string"), "got: {code}");
    assert!(
        !code.contains(" as string") && !code.contains("as string)"),
        "got: {code}"
    );
}

/// Mixed value+type import specifiers: keep value, strip type and body TS.
#[test]
fn force_js_strips_with_mixed_import_specifiers() {
    let result = compile_sfc(
        r#"
<script setup lang="ts">
import { ref, type Ref, computed } from 'vue'
const count: Ref<number> = ref(0)
const doubled = computed(() => count.value * 2)
</script>
<template><div>{{ doubled }}</div></template>
"#,
    );
    let script = result.script.as_ref().expect("script");
    let code = &script.code;
    assert!(!code.contains("type Ref"), "type specifier must go: {code}");
    assert!(!code.contains(": Ref"), "annotation must go: {code}");
    assert!(!code.contains("<number>"), "generic must go: {code}");
    assert!(code.contains("ref(0)"), "runtime remains: {code}");
}

/// Function param/return types and generics must strip.
#[test]
fn force_js_strips_function_and_generic_syntax() {
    let result = compile_sfc(
        r#"
<script setup lang="ts">
function id<T>(x: T): T { return x }
const f = (a: string, b: number): boolean => true
const g = useFoo<Bar>()
</script>
<template><div/></template>
"#,
    );
    let script = result.script.as_ref().expect("script");
    let code = &script.code;
    assert!(!code.contains("<T>"), "type params must go: {code}");
    assert!(!code.contains(": T"), "param types must go: {code}");
    assert!(!code.contains(": string"), "param types must go: {code}");
    assert!(!code.contains(": boolean"), "return types must go: {code}");
    assert!(!code.contains("<Bar>"), "call type args must go: {code}");
    assert!(code.contains("function id"), "fn remains: {code}");
}

#[test]
fn force_js_strips_body_with_mixed_import_no_annotation_use() {
    let result = compile_sfc(
        r#"
<script setup lang="ts">
import { ref, type Ref, computed } from 'vue'
const count = ref<number>(0)
const msg: string = 'hello'
</script>
<template><div>{{ count }}</div></template>
"#,
    );
    let script = result.script.as_ref().expect("script");
    let code = &script.code;
    assert!(!code.contains("type Ref"), "got: {code}");
    assert!(!code.contains("ref<number>"), "got: {code}");
    assert!(!code.contains(": string"), "got: {code}");
}

#[test]
fn force_js_strips_ref_annotation_with_separate_type_import() {
    let result = compile_sfc(
        r#"
<script setup lang="ts">
import { ref } from 'vue'
import type { Ref } from 'vue'
const count: Ref<number> = ref(0)
</script>
<template><div>{{ count }}</div></template>
"#,
    );
    let script = result.script.as_ref().expect("script");
    let code = &script.code;
    assert!(!code.contains("import type"), "got: {code}");
    assert!(!code.contains(": Ref"), "got: {code}");
    assert!(!code.contains("<number>"), "got: {code}");
    assert!(code.contains("ref(0)"), "runtime ref must remain: {code}");
}

/// F16 guard: type-only export specifiers in `<script setup>` are stripped under
/// force_js. The setup body strip skips IMPORTS (generate_script owns them) but
/// still owns exports, so `export type { … }` / `export { type … }` are removed
/// and never leak an invalid `export` into the setup wrapper.
#[test]
fn force_js_strips_type_only_exports_in_setup() {
    let result = compile_sfc(
        r#"
<script setup lang="ts">
import { ref } from 'vue'
type Foo = { a: number }
const bar = ref(0)
export type { Foo }
export { type Foo as Baz }
</script>
<template><div>{{ bar }}</div></template>
"#,
    );
    let script = result.script.as_ref().expect("script");
    let code = &script.code;
    assert!(!code.contains("export type"), "export type must go: {code}");
    assert!(
        !code.contains("type Foo as Baz") && !code.contains("export { type"),
        "type-only export specifiers must go: {code}"
    );
    assert!(code.contains("ref(0)"), "runtime value remains: {code}");
}

#[test]
fn inline_template_merges_render_into_setup_closure() {
    let result = compile_sfc_inline(
        r#"<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>

<template>
  <div id="app" :title="msg">{{ msg }}</div>
</template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");

    // Render is inlined into setup as a returned closure (official topology).
    assert!(
        script.code.contains("return (_ctx,_cache) => {"),
        "render must be inlined as a returned closure, got:\n{}",
        script.code
    );
    // Setup bindings are referenced DIRECTLY with .value unwrap — no $setup.
    assert!(
        script.code.contains("title: msg.value"),
        "inline render must reference setup bindings directly, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("$setup."),
        "inline render must not use $setup prefixes, got:\n{}",
        script.code
    );
    // No __returned__ bindings object in inline mode.
    assert!(
        !script.code.contains("__returned__"),
        "inline mode must not emit __returned__, got:\n{}",
        script.code
    );
    // Hoisted statics live at module scope, BEFORE the component object.
    let hoisted_pos = script
        .code
        .find("const _hoisted_1")
        .expect("hoisted const present");
    let component_pos = script
        .code
        .find("const __sfc__")
        .expect("component object present");
    assert!(
        hoisted_pos < component_pos,
        "hoists must precede the component object:\n{}",
        script.code
    );
    // The render closure sits inside setup (after the component object opens).
    let render_pos = script.code.find("return (_ctx,_cache) => {").unwrap();
    assert!(
        render_pos > component_pos,
        "render closure must be inside setup:\n{}",
        script.code
    );
    // No separate template block / standalone render function.
    assert!(
        result.template.is_none(),
        "inline mode must not emit a separate template block"
    );
    // The merged module is valid JS (single deduped vue import line).
    let alloc = Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &script.code, oxc_span::SourceType::mjs())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "inline output must parse as valid JS: {:?}\n---\n{}",
        parsed.diagnostics,
        script.code
    );
}

/// The inline splice must be valid JS EVEN when the authored setup body
/// carries no trailing whitespace/newline/semicolon before `</script>` — the
/// moved-in render closure directly abuts the last authored token
/// (`const n = 1` immediately followed by `return`, which is `1return`: an
/// ECMAScript syntax error, a `NumericLiteral` may not be immediately
/// followed by an `IdentifierStart`). Every OTHER inline fixture in this
/// file happens to have a natural newline there already (the `</script>` on
/// its own line), which is why this defect survived unexercised.
#[test]
fn inline_template_splice_is_valid_js_with_no_trailing_separator_in_setup_body() {
    let result = compile_sfc_inline(
        "<script setup>const n = 1</script><template><div>{{n}}</div></template>",
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");

    let alloc = Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, &script.code, oxc_span::SourceType::mjs())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "inline output must parse as valid JS even with no separator before \
         </script>: {:?}\n---\n{}",
        parsed.diagnostics,
        script.code
    );
}

#[test]
fn self_closing_setup_with_companion_default_merges_via_default_binding() {
    // Edge case in the companion family: `<script setup />` + companion
    // `export default <expr>` must not produce duplicate default exports —
    // official rebinds to `const __default__` and merges via Object.assign.
    let code = compile_sfc_script_code(
        r#"<script>
export default { inheritAttrs: false }
</script>

<script setup />

<template><div>hi</div></template>"#,
    );
    assert!(
        code.contains("const __default__ = { inheritAttrs: false }"),
        "companion default must be bound as __default__, got:\n{}",
        code
    );
    assert!(
        code.contains("/*@__PURE__*/Object.assign(__default__, {"),
        "__default__ must merge into the minimal component, got:\n{}",
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
fn define_options_setup_local_ref_is_compile_error() {
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst someRef = ref('x')\ndefineOptions({ name: someRef })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(D1_OFFICIAL_MESSAGE)),
        "setup-local defineOptions reference must be a compile error (official), got: {:?}",
        result.errors
    );
}

#[test]
fn define_options_setup_local_call_result_is_compile_error() {
    // Any non-literal setup binding is invalid (const opts = { ... }).
    let result = compile_sfc(
        "<script setup>\nconst opts = { name: 'x' }\ndefineOptions(opts)\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(D1_OFFICIAL_MESSAGE)),
        "setup-local options object must be a compile error (official), got: {:?}",
        result.errors
    );
}

#[test]
fn define_options_imported_reference_stays_valid() {
    // Imports are module scope — `defineOptions(importedOpts)` is valid and
    // becomes the Object.assign target (official).
    let code = compile_sfc_script_code(
        "<script setup>\nimport importedOpts from './opts'\ndefineOptions(importedOpts)\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        code.contains("/*@__PURE__*/Object.assign(importedOpts, {"),
        "imported options must be the Object.assign target, got:\n{}",
        code
    );
}

#[test]
fn inline_dynamic_ref_with_setup_binding_not_hoisted() {
    // Dynamic :ref resolves in setup scope (elRef.value), never hoisted
    // (hoisting to module scope was a ReferenceError).
    let result = compile_sfc_inline(
        "<script setup>\nimport { ref } from 'vue'\nconst elRef = ref(null)\n</script>\n<template><div :ref=\"elRef\">x</div></template>",
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let code = &result.script.as_ref().expect("script block").code;
    assert!(
        code.contains("{ ref: elRef.value }"),
        "inline dynamic ref resolves in setup scope, got:\n{}",
        code
    );
    assert!(
        !code.contains("const _hoisted_"),
        "dynamic ref props object must not be hoisted to module scope, got:\n{}",
        code
    );
}

#[test]
fn noninline_static_ref_with_setup_binding_stays_string() {
    // Non-inline static ref matches official: `{ ref: "el" }` (string).
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst el = ref(null)\n</script>\n<template><div ref=\"el\">x</div></template>",
    );
    let tpl = &result.template.as_ref().expect("template block").code;
    assert!(
        tpl.contains("{ ref: \"el\" }"),
        "non-inline static ref stays a string (official), got:\n{}",
        tpl
    );
}

#[test]
fn noninline_dynamic_ref_with_setup_binding_not_hoisted() {
    // Non-inline was ALSO broken: `{ ref: $setup.elRef }` hoisted to module
    // scope ($setup is a render parameter — ReferenceError at module load).
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst elRef = ref(null)\n</script>\n<template><div :ref=\"elRef\">x</div></template>",
    );
    let tpl = &result.template.as_ref().expect("template block").code;
    assert!(
        tpl.contains("{ ref: $setup.elRef }"),
        "non-inline dynamic ref resolves via $setup in the render fn, got:\n{}",
        tpl
    );
    assert!(
        !tpl.contains("const _hoisted_1 = { ref: $setup"),
        "dynamic ref props object must not be hoisted to module scope, got:\n{}",
        tpl
    );
}

// =========================================================================
// FIX1 — inline template refs: user (maybe-ref) imports bind ref_key/ref
// =========================================================================
//
// Official binding metadata (compiler-sfc 3.6.0-rc.5):
//   imported === "*" || (imported === "default" && source.endsWith(".vue"))
//     || source === "vue"  →  "setup-const"   (string ref)
//   everything else (named imports anywhere, default imports from
//     non-vue non-.vue sources)              →  "setup-maybe-ref" (ref_key/ref)

#[test]
fn inline_static_ref_with_named_user_import_binds_ref_key() {
    let result = compile_sfc_inline(
        "<script setup>\nimport { elRef } from './refs'\n</script>\n<template><div ref=\"elRef\">x</div></template>",
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let code = &result.script.as_ref().expect("script block").code;
    assert!(
        code.contains("ref_key: \"elRef\""),
        "named user import used as a template ref must bind ref_key (official setup-maybe-ref), got:\n{}",
        code
    );
    assert!(
        code.contains("ref: elRef"),
        "must reference the import binding directly, got:\n{}",
        code
    );
    assert!(
        !code.contains("const _hoisted_"),
        "the ref pair must not be hoisted to module scope, got:\n{}",
        code
    );
}

#[test]
fn inline_static_ref_with_default_user_import_binds_ref_key() {
    // Default import from a non-.vue source → setup-maybe-ref (binds).
    let result = compile_sfc_inline(
        "<script setup>\nimport elRef from './refs'\n</script>\n<template><div ref=\"elRef\">x</div></template>",
    );
    let code = &result.script.as_ref().expect("script block").code;
    assert!(
        code.contains("ref_key: \"elRef\"") && code.contains("ref: elRef"),
        "default user import from a non-.vue source must bind ref_key/ref (official), got:\n{}",
        code
    );
}

#[test]
fn inline_static_ref_with_vue_import_stays_string() {
    // vue-source import → setup-const → string ref (official).
    let result = compile_sfc_inline(
        "<script setup>\nimport { ref } from 'vue'\nconst x = ref(1)\n</script>\n<template><div ref=\"ref\">x</div></template>",
    );
    let code = &result.script.as_ref().expect("script block").code;
    assert!(
        code.contains("{ ref: \"ref\" }") && !code.contains("ref_key"),
        "vue-source import must stay a string ref, got:\n{}",
        code
    );
}

#[test]
fn inline_static_ref_with_namespace_import_stays_string() {
    // `import * as refs` → setup-const → string ref (official).
    let result = compile_sfc_inline(
        "<script setup>\nimport * as refs from './refs'\n</script>\n<template><div ref=\"refs\">x</div></template>",
    );
    let code = &result.script.as_ref().expect("script block").code;
    assert!(
        code.contains("{ ref: \"refs\" }") && !code.contains("ref_key"),
        "namespace import must stay a string ref, got:\n{}",
        code
    );
}

#[test]
fn inline_static_ref_with_dotvue_default_import_stays_string() {
    // Default import from a .vue source → setup-const → string ref (official).
    let result = compile_sfc_inline(
        "<script setup>\nimport Comp from './Comp.vue'\n</script>\n<template><div ref=\"Comp\">x</div></template>",
    );
    let code = &result.script.as_ref().expect("script block").code;
    assert!(
        code.contains("{ ref: \"Comp\" }") && !code.contains("ref_key"),
        "default .vue import must stay a string ref, got:\n{}",
        code
    );
}

// =========================================================================
// R5-FIX3 — scope-check detects setup-locals at any name length / nesting
// =========================================================================
//
// Official `checkInvalidScopeReference` walks EVERY referenced identifier in
// the macro runtime argument (nested object-property values, array elements,
// any name length), excluding nested function-scope locals. The prior tests
// used multi-char names; a 1-char nested default slipped the check because the
// `Props` prop-key bindings carry FILE-relative spans that, sliced into the
// content-relative name map, could overwrite a same-length setup binding.

#[test]
fn r5_fix3_one_char_setup_local_nested_default_is_error() {
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst f = ref(0)\ndefineProps({ x: { default: f } })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(
                "`defineProps()` in <script setup> cannot reference locally declared variables"
            )),
        "1-char setup-local nested default must be rejected (official), got: {:?}",
        result.errors
    );
}

#[test]
fn r5_fix3_one_char_setup_local_array_element_is_error() {
    // 1-char setup-local as an array element inside defineEmits.
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst e = ref('save')\ndefineEmits([e])\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(
                "`defineEmits()` in <script setup> cannot reference locally declared variables"
            )),
        "1-char setup-local array element must be rejected (official), got: {:?}",
        result.errors
    );
}

#[test]
fn r5_fix3_setup_local_inside_nested_function_body_is_error() {
    // A setup-local referenced INSIDE a nested function body but NOT shadowed
    // (no param/local of that name) is still an invalid scope reference —
    // official walkIdentifiers records it because it is not a function-scope local.
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst g = ref(0)\ndefineProps({ x: { default: () => g.value } })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(
                "`defineProps()` in <script setup> cannot reference locally declared variables"
            )),
        "an un-shadowed setup-local used inside a nested function body must be rejected, got: {:?}",
        result.errors
    );
}

// =========================================================================
// FIX3 — scope check for defineProps / defineEmits / defineModel
// =========================================================================

#[test]
fn define_props_local_ref_default_is_compile_error() {
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst dft = ref('x')\ndefineProps({ x: { default: dft } })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(
                "`defineProps()` in <script setup> cannot reference locally declared variables"
            )),
        "defineProps default referencing a setup-local must be rejected (official), got: {:?}",
        result.errors
    );
}

#[test]
fn define_props_imported_default_stays_valid() {
    let result = compile_sfc(
        "<script setup>\nimport { dft } from './defs'\ndefineProps({ x: { default: dft } })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "imported default must stay valid, got: {:?}",
        result.errors
    );
}

// =========================================================================
// defineProps reactive-destructure default scope-check
// =========================================================================
//
// Official sets `ctx.propsDestructureDecl` (the node
// `checkInvalidScopeReference(ctx.propsDestructureDecl, DEFINE_PROPS)` walks)
// ONLY in `processDefineProps` when the call is a DIRECT `defineProps` (NOT
// `withDefaults`, i.e. `isWithDefaults === false`) AND the declaration id is an
// `ObjectPattern`. In that reactive-destructure form the default expressions are
// hoisted with the props runtime decl (the `mergeDefaults` merge), so a default
// referencing a setup-local breaks at runtime and is rejected under
// `defineProps()`. The destructured binding NAMES are NOT registered as
// setup-locals (official's `walkDeclaration` skips them), so only the default
// `right` expressions can trigger the error — never the destructure targets /
// aliases. Two forms are NOT props destructures and stay valid: under
// `withDefaults(...)` reactive destructure is DISABLED (the declId is never
// recorded, defaults are not hoisted), and a top-level ARRAY pattern is never a
// props destructure. DIAGNOSTIC ONLY: the reactive-destructure
// `_mergeDefaults`/`__props` runtime transform is a separate concern and is
// intentionally NOT implemented here.

#[test]
fn define_props_destructure_default_setup_local_is_compile_error() {
    // R7-2 defect: `const { x = dft }` where `dft` is a setup-local. The default
    // is hoisted → official rejects; Verter accepted (never walked the pattern).
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst dft = ref('x')\nconst { x = dft } = defineProps({ x: Number })\n</script>\n<template><div>{{ x }}</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(
                "`defineProps()` in <script setup> cannot reference locally declared variables"
            )),
        "defineProps destructure default referencing a setup-local must be rejected (official), got: {:?}",
        result.errors
    );
}

#[test]
fn define_props_destructure_alias_default_setup_local_is_compile_error() {
    // Aliased form `{ x: y = dft }` — the default `dft` is still hoisted; the
    // alias local `y` is a binding target (never flagged), `dft` is the
    // setup-local that must be rejected.
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst dft = ref('x')\nconst { x: y = dft } = defineProps({ x: Number })\n</script>\n<template><div>{{ y }}</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(
                "`defineProps()` in <script setup> cannot reference locally declared variables"
            )),
        "aliased defineProps destructure default referencing a setup-local must be rejected, got: {:?}",
        result.errors
    );
}

#[test]
fn with_defaults_destructure_default_setup_local_stays_valid() {
    // Under `withDefaults(...)` reactive props destructure is DISABLED
    // (`processDefineProps(..., isWithDefaults=true)` never calls
    // `processPropsDestructure`, so `propsDestructureDecl` is never set). The
    // destructure then runs as a plain in-setup destructure and its defaults are
    // NOT hoisted out of setup() — a setup-local default is therefore VALID
    // (official emits only a warning, never the scope error). Only the direct
    // `defineProps` reactive-destructure form hoists the defaults.
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [crate::test_helpers::runtime_prop(
            "x",
            true,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let result = compile_sfc_with_runtime(
        "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst dft = ref('x')\nconst { x = dft } = withDefaults(defineProps<{ x?: string }>(), {})\n</script>\n<template><div>{{ x }}</div></template>",
        runtime,
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "withDefaults destructure default referencing a setup-local must stay valid (official disables reactive destructure under withDefaults; only warns), got: {:?}",
        result.errors
    );
}

#[test]
fn define_props_destructure_default_import_stays_valid() {
    // An imported default is module-scope — valid (never hoisted out of reach).
    let result = compile_sfc(
        "<script setup>\nimport { someImport } from './defs'\nconst { x = someImport } = defineProps({ x: Number })\n</script>\n<template><div>{{ x }}</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "defineProps destructure default referencing an import must stay valid, got: {:?}",
        result.errors
    );
}

#[test]
fn define_props_destructure_default_literal_stays_valid() {
    // A literal default carries no free reference — valid.
    let result = compile_sfc(
        "<script setup>\nconst { x = 1 } = defineProps({ x: Number })\n</script>\n<template><div>{{ x }}</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "defineProps destructure literal default must stay valid, got: {:?}",
        result.errors
    );
}

#[test]
fn array_pattern_destructure_default_setup_local_stays_valid() {
    // Only an ObjectPattern declId is a props destructure (official
    // `processDefineProps` gates on `declId.type === "ObjectPattern"`). A
    // top-level ARRAY pattern is never recorded as `propsDestructureDecl`, so its
    // defaults are not hoisted and a setup-local default stays VALID.
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst d = ref(0)\nconst [a = d] = defineProps({ x: Number })\n</script>\n<template><div>{{ a }}</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "a top-level array-pattern destructure of defineProps is not a props destructure — a setup-local default must stay valid (official), got: {:?}",
        result.errors
    );
}

// =========================================================================
// Wrapped macro CALL SITES — peel Parenthesized + TS wrappers before detection
// =========================================================================
//
// Official runs `unwrapTSNode(node.expression)` / `unwrapTSNode(decl.init)`
// BEFORE `isCallOf(...)` dispatches to processDefineProps / processDefineEmits /
// processDefineModel / processDefineOptions / withDefaults. Babel folds
// parentheses into a flag (no wrapper node), so a merely-parenthesized macro
// call still matches; OXC materializes an explicit `ParenthesizedExpression`, so
// Verter peels parens plus the 5 TS wrapper nodes (`as` / satisfies / non-null /
// type-assertion / instantiation) to reach the same CallExpression. Without the
// peel the whole scope walk is skipped and a setup-local reference goes uncaught.

#[test]
fn paren_wrapped_define_props_call_setup_local_default_is_error() {
    // `(defineProps({...}))` — a parenthesized bare macro call. Official detects
    // it (Babel drops the paren); Verter must peel the OXC paren node.
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst f = ref(0)\n;(defineProps({ x: { default: f } }))\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(
                "`defineProps()` in <script setup> cannot reference locally declared variables"
            )),
        "a parenthesized defineProps call must still be scope-checked (official peels wrappers), got: {:?}",
        result.errors
    );
}

#[test]
fn ts_as_wrapped_define_props_call_setup_local_default_is_error() {
    // `(defineProps({...}) as any)` — a TS `as`-wrapped bare macro call. Official
    // `unwrapTSNode` peels TSAsExpression before `isCallOf`.
    let result = compile_sfc(
        "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst f = ref(0)\n;(defineProps({ x: { default: f } }) as any)\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(
                "`defineProps()` in <script setup> cannot reference locally declared variables"
            )),
        "a TS-as-wrapped defineProps call must still be scope-checked (official unwrapTSNode), got: {:?}",
        result.errors
    );
}

#[test]
fn paren_wrapped_define_props_call_imported_default_stays_valid() {
    // Discrimination: the peel must not over-reject — a wrapped call whose default
    // is an import (module scope) stays valid.
    let result = compile_sfc(
        "<script setup>\nimport { dft } from './defs'\n;(defineProps({ x: { default: dft } }))\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "a parenthesized defineProps call with an imported default must stay valid, got: {:?}",
        result.errors
    );
}

// =========================================================================
// FIX4 — inline setup destructure order: expose, emit, attrs, slots
// =========================================================================

#[test]
fn inline_setup_destructure_official_order() {
    // Official builds emit (__emit) BEFORE buildDestructureElements pushes
    // attrs/slots — so emit precedes attrs/slots.
    let result = compile_sfc_inline(
        "<script setup>\nconst emit = defineEmits(['save'])\ndefineExpose({ x: 1 })\n</script>\n<template><div :data-a=\"$attrs\" :data-s=\"$slots.default\">x</div></template>",
    );
    let code = &result.script.as_ref().expect("script block").code;
    assert!(
        code.contains(
            "setup(__props, { expose: __expose, emit: __emit, attrs: $attrs, slots: $slots })"
        ),
        "official destructure order is expose, emit, attrs, slots, got:\n{}",
        code
    );
}
