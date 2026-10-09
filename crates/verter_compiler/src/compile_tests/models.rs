use super::*;

#[test]
fn vmrs_ide_uses_authoritative_model_prop_for_template_bindings() {
    use verter_macro_dto::RuntimeConstructor;

    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_model_entry(
        0,
        0,
        "title",
        "titleModifiers",
        "update:title",
        false,
        [RuntimeConstructor::String],
    )]);
    let alloc = Allocator::new();
    let result = compile(
        r#"<script setup lang="ts">
defineModel<string>('title')
</script>
<template>{{ title }}</template>"#,
        &CodegenOptions {
            filename: Some("App.vue".to_string()),
            target: CompileTarget::IDE,
            ..Default::default()
        },
        &VerterCompileOptions::default(),
        &VueMacroSemanticInput::Runtime(runtime),
        &alloc,
    );

    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let code = result.tsx.expect("IDE output").code;
    assert!(
        code.contains("__props.title"),
        "authoritative model props must drive IDE binding ownership: {code}"
    );
    assert!(
        !code.contains("___VERTER___instance.title"),
        "an authoritative model prop must not degrade to instance ownership: {code}"
    );
}

#[test]
fn vmrs_production_model_uses_vue_runtime_option_policy() {
    use std::sync::Arc;
    use verter_macro_dto::{
        MacroAnchor, MacroRuntimeBundle, MacroRuntimeEntry, MacroRuntimeOutcome, MacroRuntimeShape,
        ModelRuntimeShape, OrderedRuntimeConstructors, RuntimeConstructor, RuntimeEmit,
        RuntimeProp, RuntimePropType, SynthesizedRowKind,
    };

    let model = |macro_index, name: &str| MacroRuntimeEntry {
        syntax_index: macro_index,
        macro_index,
        outcome: MacroRuntimeOutcome::Complete(MacroRuntimeShape::Model(ModelRuntimeShape {
            prop: RuntimeProp {
                name: name.to_string(),
                optional: true,
                type_shape: RuntimePropType::Resolved {
                    constructors: OrderedRuntimeConstructors::from_ordered([
                        RuntimeConstructor::String,
                    ]),
                    skip_check: false,
                },
                anchor: MacroAnchor::Synthesized {
                    macro_index,
                    row: SynthesizedRowKind::ModelProp,
                },
            },
            update_event: RuntimeEmit {
                name: format!("update:{name}"),
                anchor: MacroAnchor::Synthesized {
                    macro_index,
                    row: SynthesizedRowKind::ModelUpdateEvent,
                },
            },
            modifiers_prop: RuntimeProp {
                name: format!("{name}Modifiers"),
                optional: true,
                type_shape: RuntimePropType::Resolved {
                    constructors: OrderedRuntimeConstructors::default(),
                    skip_check: false,
                },
                anchor: MacroAnchor::Synthesized {
                    macro_index,
                    row: SynthesizedRowKind::ModelModifiersProp,
                },
            },
        })),
    };
    let semantics = VueMacroSemanticInput::Runtime(Arc::new(MacroRuntimeBundle {
        entries: vec![model(0, "title"), model(1, "subtitle")],
    }));
    let alloc = Allocator::new();
    let result = compile(
        r#"<script setup lang="ts">
defineModel<string>('title', { required: true })
defineModel<string>('subtitle')
</script>"#,
        &CodegenOptions {
            is_production: true,
            ..Default::default()
        },
        &VerterCompileOptions {
            force_js: true,
            ..Default::default()
        },
        &semantics,
        &alloc,
    );

    assert!(result.errors.is_empty(), "{:?}", result.errors);
    let code = result.script.expect("script").code;
    assert!(
        code.contains("title: { required: true }"),
        "prod keeps authored model runtime options without a phantom type merge: {code}"
    );
    assert!(
        code.contains("subtitle: {}"),
        "prod elides String-only model runtime type: {code}"
    );
    assert!(
        !code.contains("title: { ,"),
        "invalid empty option prefix: {code}"
    );
}

// ==================== v-model on components ====================

// @ai-generated - Tests v-model on components expands to prop + update handler
#[test]
pub(super) fn v_model_on_component_expands_to_props() {
    let result = compile_sfc(
        r#"<template><div><MyComp v-model="val" /></div></template>
<script setup>
import MyComp from './MyComp.vue'
const val = ref('')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("modelValue:"),
        "v-model on component should emit modelValue prop, got:
{}",
        tpl.code
    );
    assert!(
        tpl.code.contains(r#""onUpdate:modelValue""#),
        "v-model on component should emit onUpdate:modelValue handler, got:
{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("$event"),
        "v-model update handler should use $event, got:
{}",
        tpl.code
    );
}

#[test]
pub(super) fn v_model_named_on_component() {
    let result = compile_sfc(
        r#"<template><div><MyComp v-model:title="pageTitle" /></div></template>
<script setup>
import MyComp from './MyComp.vue'
const pageTitle = ref('')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("title:"),
        "v-model:title should emit title prop, got:
{}",
        tpl.code
    );
    assert!(
        tpl.code.contains(r#""onUpdate:title""#),
        "v-model:title should emit onUpdate:title handler, got:
{}",
        tpl.code
    );
}

/// `v-model="(myValue as string)"` — valid in the official Vue compiler
/// (the TS cast unwraps to a plain identifier). With `force_js` the emitted
/// render code must strip the TS cast (JS output), while still assigning to
/// the underlying reference in the onUpdate handler.
#[test]
fn v_model_with_ts_cast_compiles_to_js_without_types() {
    let result = compile_sfc(
        r#"<template><MyComp v-model="(myValue as string)" /><input v-model="obj.val as string | number" /></template>
<script setup lang="ts">
import MyComp from './MyComp.vue'
import { ref } from 'vue'
const myValue = ref<string | number>('')
const obj = ref({ val: '' })
</script>"#,
    );
    assert!(
        result
            .errors
            .iter()
            .all(|e| e.code != "XVModelMalformedExpression"),
        "TS-cast v-model must not be malformed, got: {:?}",
        result.errors.iter().map(|e| &e.code).collect::<Vec<_>>()
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("modelValue:") && tpl.code.contains(r#""onUpdate:modelValue""#),
        "cast v-model should expand to modelValue + onUpdate:modelValue, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("$event"),
        "cast v-model update handler should assign from $event, got:\n{}",
        tpl.code
    );
    // force_js output is JavaScript: the TS cast must not survive.
    assert!(
        !tpl.code.contains(" as string"),
        "force_js output must strip TS casts from v-model expressions, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains("myValue") && tpl.code.contains("obj.val"),
        "the underlying references must survive cast stripping, got:\n{}",
        tpl.code
    );
}

/// Same input without `force_js` (TS render output): the authored cast is
/// preserved verbatim — TypeScript accepts parenthesized `as` expressions as
/// assignment targets, and downstream transpilers strip them.
#[test]
fn v_model_with_ts_cast_preserves_cast_in_ts_output() {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions::default();
    let result = compile(
        r#"<template><MyComp v-model="(myValue as string)" /></template>
<script setup lang="ts">
import MyComp from './MyComp.vue'
import { ref } from 'vue'
const myValue = ref<string | number>('')
</script>"#,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    assert!(
        result
            .errors
            .iter()
            .all(|e| e.code != "XVModelMalformedExpression"),
        "TS-cast v-model must not be malformed, got: {:?}",
        result.errors.iter().map(|e| &e.code).collect::<Vec<_>>()
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("as string"),
        "TS output should preserve the authored cast, got:\n{}",
        tpl.code
    );
    assert!(
        tpl.code.contains(r#""onUpdate:modelValue""#) && tpl.code.contains("$event"),
        "cast v-model should emit the update handler, got:\n{}",
        tpl.code
    );
}

#[test]
pub(super) fn v_model_on_unresolved_component() {
    let result = compile_sfc(
        r#"<template><div><BalTabs v-model="activeTab" :tabs="tabs" /></div></template>
<script setup>
const activeTab = ref('tab1')
const tabs = []
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    assert!(
        tpl.code.contains("modelValue:"),
        "v-model on unresolved component should emit modelValue prop, got:
{}",
        tpl.code
    );
    assert!(
        tpl.code.contains(r#""onUpdate:modelValue""#),
        "v-model on unresolved component should emit onUpdate handler, got:
{}",
        tpl.code
    );
}

// @ai-generated - v-model with explicit @update:modelValue should merge into array
#[test]
pub(super) fn v_model_with_explicit_update_handler_merges_into_array() {
    let result = compile_sfc(
        r#"<template><div><MyComp v-model="val" @update:model-value="handler" /></div></template>
<script setup>
import MyComp from './MyComp.vue'
const val = ref('')
function handler(v) {}
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    // Vue merges v-model + explicit @update handler into an array:
    //   "onUpdate:modelValue": [$event => ((val) = $event), handler]
    // Verter must NOT produce duplicate "onUpdate:modelValue" keys in the props object.
    let code = &tpl.code;
    // The merged value should be an array
    assert!(
        code.contains(r#""onUpdate:modelValue": ["#),
        "merged handler should be an array, got:\n{}",
        code
    );
    // Must NOT have two separate "onUpdate:modelValue": entries in the props object
    // (one from v-model, one from @update:model-value)
    let count = code.matches(r#""onUpdate:modelValue": "#).count();
    assert_eq!(
        count, 1,
        "should have exactly one onUpdate:modelValue: assignment (merged), got {} in:\n{}",
        count, code
    );
}

// @ai-generated - v-model:title with explicit @update:title should merge into array
#[test]
pub(super) fn v_model_named_with_explicit_update_handler_merges_into_array() {
    let result = compile_sfc(
        r#"<template><div><MyComp v-model:title="pageTitle" @update:title="onTitleChange" /></div></template>
<script setup>
import MyComp from './MyComp.vue'
const pageTitle = ref('')
function onTitleChange(v) {}
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let code = &tpl.code;
    assert!(
        code.contains(r#""onUpdate:title": ["#),
        "merged handler should be an array, got:\n{}",
        code
    );
    let count = code.matches(r#""onUpdate:title": "#).count();
    assert_eq!(
        count, 1,
        "should have exactly one onUpdate:title: assignment (merged), got {} in:\n{}",
        count, code
    );
}

// ==================== v-model on native elements ====================

// @ai-generated - Tests v-model on native <input> generates withDirectives + onUpdate:modelValue
#[test]
pub(super) fn v_model_on_native_input_generates_with_directives() {
    let result = compile_sfc(
        r#"<template><div><input v-model="msg" /></div></template>
<script setup>
const msg = ref('')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let code = &tpl.code;
    assert!(
        code.contains("_withDirectives"),
        "v-model on native input should use _withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("_vModelText"),
        "v-model on native input should use _vModelText directive, got:\n{}",
        code
    );
    assert!(
        code.contains(r#""onUpdate:modelValue""#),
        "v-model on native input should emit onUpdate:modelValue handler, got:\n{}",
        code
    );
    assert!(
        code.contains("$event"),
        "v-model update handler should use $event assignment, got:\n{}",
        code
    );
}

// @ai-generated - v-model on <textarea> uses _vModelText
#[test]
pub(super) fn v_model_on_textarea_generates_with_directives() {
    let result = compile_sfc(
        r#"<template><div><textarea v-model="msg" /></div></template>
<script setup>
const msg = ref('')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let code = &tpl.code;
    assert!(
        code.contains("_withDirectives"),
        "v-model on textarea should use _withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("_vModelText"),
        "v-model on textarea should use _vModelText, got:\n{}",
        code
    );
}

// @ai-generated - v-model on <select> uses _vModelSelect
#[test]
pub(super) fn v_model_on_select_generates_with_directives() {
    let result = compile_sfc(
        r#"<template><div><select v-model="choice"><option>A</option></select></div></template>
<script setup>
const choice = ref('')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let code = &tpl.code;
    assert!(
        code.contains("_withDirectives"),
        "v-model on select should use _withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("_vModelSelect"),
        "v-model on select should use _vModelSelect, got:\n{}",
        code
    );
}

// @ai-generated - v-model on checkbox input uses _vModelCheckbox
#[test]
pub(super) fn v_model_on_checkbox_generates_with_directives() {
    let result = compile_sfc(
        r#"<template><div><input type="checkbox" v-model="checked" /></div></template>
<script setup>
const checked = ref(false)
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let code = &tpl.code;
    assert!(
        code.contains("_withDirectives"),
        "v-model on checkbox should use _withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("_vModelCheckbox"),
        "v-model on checkbox should use _vModelCheckbox, got:\n{}",
        code
    );
}

// @ai-generated - v-model on radio input uses _vModelRadio
#[test]
pub(super) fn v_model_on_radio_generates_with_directives() {
    let result = compile_sfc(
        r#"<template><div><input type="radio" v-model="picked" value="a" /></div></template>
<script setup>
const picked = ref('a')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let code = &tpl.code;
    assert!(
        code.contains("_withDirectives"),
        "v-model on radio should use _withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("_vModelRadio"),
        "v-model on radio should use _vModelRadio, got:\n{}",
        code
    );
}

// @ai-generated - v-model with .trim modifier generates modifier object in directive
#[test]
pub(super) fn v_model_on_input_with_trim_modifier() {
    let result = compile_sfc(
        r#"<template><div><input v-model.trim="msg" /></div></template>
<script setup>
const msg = ref('')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let code = &tpl.code;
    assert!(
        code.contains("_withDirectives"),
        "v-model.trim should use _withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("trim: true"),
        "v-model.trim should have modifier object with trim: true, got:\n{}",
        code
    );
}

// @ai-generated - v-model on dynamic input type uses _vModelDynamic
#[test]
pub(super) fn v_model_on_dynamic_type_input_uses_dynamic() {
    let result = compile_sfc(
        r#"<template><div><input :type="inputType" v-model="val" /></div></template>
<script setup>
const inputType = ref('text')
const val = ref('')
</script>"#,
    );
    let tpl = result.template.as_ref().expect("template block");
    let code = &tpl.code;
    assert!(
        code.contains("_withDirectives"),
        "v-model on dynamic type should use _withDirectives, got:\n{}",
        code
    );
    assert!(
        code.contains("_vModelDynamic"),
        "v-model on dynamic type input should use _vModelDynamic, got:\n{}",
        code
    );
}

// @ai-generated - TDD test: defineModel declares runtime prop and emit
#[test]
pub(super) fn define_model_declares_prop_and_emit() {
    let result = compile_sfc(
        r#"<script setup>
const modelValue = defineModel()
</script>
<template><div>{{ modelValue }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    // defineModel() should declare a `modelValue` prop in the component definition
    assert!(
        script.code.contains("modelValue"),
        "defineModel() should declare modelValue prop.\nGot:\n{}",
        script.code
    );
    // Should declare 'update:modelValue' emit
    assert!(
        script.code.contains("update:modelValue"),
        "defineModel() should declare 'update:modelValue' emit.\nGot:\n{}",
        script.code
    );
}

// @ai-generated - TDD test: defineModel with options forwards type/default to prop definition
#[test]
fn define_model_with_options_forwards_to_prop() {
    let result = compile_sfc(
        r#"<script setup>
const visible = defineModel('visible', { type: Boolean, default: false })
</script>
<template><div v-if="visible">shown</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    // defineModel options should be forwarded to the prop definition
    assert!(
        script.code.contains("type: Boolean"),
        "defineModel options should forward `type: Boolean` to prop definition.\nGot:\n{}",
        script.code
    );
    assert!(
        script.code.contains("default: false"),
        "defineModel options should forward `default: false` to prop definition.\nGot:\n{}",
        script.code
    );
    // Should NOT output an empty prop object for models with options
    assert!(
        !script.code.contains("visible: {},"),
        "Model with options should not have empty prop object.\nGot:\n{}",
        script.code
    );
}

// @ai-generated - TDD test: named defineModel declares correct prop and emit
#[test]
pub(super) fn define_model_named_declares_prop_and_emit() {
    let result = compile_sfc(
        r#"<script setup>
const count = defineModel('count')
</script>
<template><div>{{ count }}</div></template>"#,
    );
    let script = result.script.as_ref().expect("script block");
    // defineModel('count') should declare a `count` prop
    assert!(
        script.code.contains("props:") && script.code.contains("count"),
        "defineModel('count') should declare 'count' prop in props section.\nGot:\n{}",
        script.code
    );
    // Should declare 'update:count' emit
    assert!(
        script.code.contains("update:count"),
        "defineModel('count') should declare 'update:count' emit.\nGot:\n{}",
        script.code
    );
}

// ======================== defineModel + withDefaults (runtime variable) ========================

/// @ai-generated - defineModel + withDefaults with resolvable type uses _mergeModels.
/// Vue's official compiler always uses _mergeModels when both defineProps and defineModel
/// are present in the same component.
#[test]
pub(super) fn define_model_with_defaults_resolved_type() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [
            crate::test_helpers::runtime_prop_at_macro_argument(
                "placeholder",
                true,
                [verter_macro_dto::RuntimeConstructor::String],
            ),
            crate::test_helpers::runtime_prop_at_macro_argument(
                "maxLength",
                true,
                [verter_macro_dto::RuntimeConstructor::Number],
            ),
        ],
    )]);
    let result = compile_sfc_keep_ts_with_runtime(
        r#"<script setup lang="ts">
import { DEFAULT_PROPS } from './defaults'

interface ChatInputProps {
  placeholder?: string
  maxLength?: number
}

const props = withDefaults(defineProps<ChatInputProps>(), DEFAULT_PROPS)
const visible = defineModel('visible', { type: Boolean, default: false })
</script>
<template><div>{{ visible }}</div></template>"#,
        runtime,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");

    // Vue uses _mergeModels to merge typed props with model props
    assert!(
        script.code.contains("_mergeModels"),
        "Should use _mergeModels to merge model props with withDefaults props.\nGot:\n{}",
        script.code
    );
    // Model prop and modifiers must appear in the second arg to _mergeModels
    assert!(
        script.code.contains("visible"),
        "Model prop 'visible' should be declared.\nGot:\n{}",
        script.code
    );
    assert!(
        script.code.contains("visibleModifiers"),
        "Model modifiers prop should be declared.\nGot:\n{}",
        script.code
    );
}

/// defineModel + authoritative withDefaults with a runtime defaults variable
/// composes `_mergeDefaults` inside `_mergeModels` without interleaving model rows.
#[test]
pub(super) fn define_model_with_authoritative_defaults_runtime_variable() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [crate::test_helpers::runtime_prop_at_macro_argument(
            "placeholder",
            true,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let result = compile_sfc_with_runtime(
        r#"<script setup lang="ts">
import type { ChatInputProps } from './types'
import { DEFAULT_PROPS } from './defaults'

const props = withDefaults(defineProps<ChatInputProps>(), DEFAULT_PROPS)
const visible = defineModel('visible', { type: Boolean, default: false })
</script>
<template><div>{{ visible }}</div></template>"#,
        runtime,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");

    assert!(
        script.code.contains("_mergeModels"),
        "Should use _mergeModels to merge model props with authoritative defaults.\nGot:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("return p,"),
        "Model props must not be interleaved into defaults lowering.\nGot:\n{}",
        script.code
    );
}

/// @ai-generated - defineModel + defineProps with object literal merges correctly
/// When defineProps uses an object literal (not IIFE), static merge is fine,
/// but Vue still uses _mergeModels, so we should too.
#[test]
pub(super) fn define_model_with_define_props_object_uses_merge_models() {
    let result = compile_sfc(
        r#"<script setup>
const props = defineProps({ title: String })
const visible = defineModel('visible')
</script>
<template><div>{{ props.title }} {{ visible }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");

    // Vue uses _mergeModels even for object literal props + models
    assert!(
        script.code.contains("_mergeModels"),
        "Should use _mergeModels for props + model merge.\nGot:\n{}",
        script.code
    );
    assert!(
        script.code.contains("update:visible"),
        "Should declare 'update:visible' emit.\nGot:\n{}",
        script.code
    );
}

/// @ai-generated - defineModel + type-based withDefaults merges correctly
#[test]
pub(super) fn define_model_with_typed_with_defaults() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_props_entry(
        0,
        1,
        verter_macro_dto::PropsDefaultsAssociation::WithDefaults {
            payload_macro_index: 0,
            defaults_macro_index: 1,
        },
        [crate::test_helpers::runtime_prop_at_macro_argument(
            "placeholder",
            true,
            [verter_macro_dto::RuntimeConstructor::String],
        )],
    )]);
    let result = compile_sfc_keep_ts_with_runtime(
        r#"<script setup lang="ts">
interface Props {
  placeholder?: string
}

const props = withDefaults(defineProps<Props>(), { placeholder: 'Type...' })
const open = defineModel('open', { type: Boolean })
</script>
<template><div>{{ props.placeholder }} {{ open }}</div></template>"#,
        runtime,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");

    assert!(
        script.code.contains("_mergeModels"),
        "Should use _mergeModels for typed withDefaults + defineModel.\nGot:\n{}",
        script.code
    );
    assert!(
        script.code.contains("open:") || script.code.contains("open "),
        "Model prop 'open' should be declared.\nGot:\n{}",
        script.code
    );
}

/// @ai-generated - defineModel emits section also uses _mergeModels when defineEmits present
#[test]
pub(super) fn define_model_with_define_emits_uses_merge_models_for_emits() {
    let result = compile_sfc(
        r#"<script setup>
const emit = defineEmits(['click'])
const visible = defineModel('visible')
</script>
<template><div @click="emit('click')">{{ visible }}</div></template>"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let script = result.script.as_ref().expect("script block");

    // The emits section should merge ['click'] with ["update:visible"]
    assert!(
        script.code.contains("update:visible"),
        "Model emit 'update:visible' should be present.\nGot:\n{}",
        script.code
    );
}

#[test]
fn tsx_v_model_on_component_produces_valid_jsx() {
    let result = compile_tsx(
        r#"<script setup>
import { ref } from 'vue'
import MyComp from './MyComp.vue'
const val = ref('')
</script>
<template><MyComp v-model="val" /></template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // modelValue prop must be present
    assert!(
        tsx.code.contains("modelValue={"),
        "v-model on component should emit modelValue prop, got:\n{}",
        tsx.code
    );
    // Event handler must use spread syntax (quoted keys are invalid JSX attribute names)
    assert!(
        tsx.code.contains(r#""onUpdate:modelValue""#),
        "v-model should emit onUpdate:modelValue handler, got:\n{}",
        tsx.code
    );
    // Negative: quoted string must NOT appear as a standalone JSX attribute (before `=`)
    // Valid: {...{"onUpdate:modelValue": handler}} — inside spread
    // Invalid: "onUpdate:modelValue"={handler} — bare quoted attribute
    assert!(
        !tsx.code.contains(r#""onUpdate:modelValue"={"#),
        "onUpdate:modelValue must NOT be a bare JSX attribute (invalid syntax), got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_v_model_on_native_input_uses_value_prop() {
    let result = compile_tsx(
        r#"<script setup>
import { ref } from 'vue'
const msg = ref('')
</script>
<template><input v-model="msg" /></template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Native input should use `value` (not `modelValue`)
    assert!(
        tsx.code.contains("value={"),
        "v-model on native input should use value prop, got:\n{}",
        tsx.code
    );
    assert!(
        !tsx.code.contains("modelValue"),
        "v-model on native input must NOT use modelValue, got:\n{}",
        tsx.code
    );
    // Must not have quoted attribute names (invalid JSX)
    assert!(
        !tsx.code.contains(r#""onUpdate:"#),
        "native input must not have quoted onUpdate attribute, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_v_model_named_on_component_uses_spread() {
    let result = compile_tsx(
        r#"<script setup>
import { ref } from 'vue'
import MyComp from './MyComp.vue'
const title = ref('')
</script>
<template><MyComp v-model:title="title" /></template>
"#,
    );
    assert!(result.errors.is_empty(), "errors: {:?}", result.errors);
    let tsx = result.tsx.as_ref().expect("tsx block");

    // Named model: title prop + onUpdate:title handler
    assert!(
        tsx.code.contains("title={"),
        "named v-model should emit title prop, got:\n{}",
        tsx.code
    );
    // Must not have bare quoted attribute
    assert!(
        !tsx.code.contains(r#""onUpdate:title"={"#),
        "named v-model must NOT use bare quoted JSX attribute, got:\n{}",
        tsx.code
    );
}

#[test]
fn tsx_parse_valid_v_model_input() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const msg = ref('Hello')
</script>
<template>
  <input v-model="msg" />
</template>"#,
        "v-model on input",
    );
}

/// TS-cast v-model forms must produce syntactically valid TSX on the IDE
/// path — TypeScript accepts parenthesized casts and non-null assertions as
/// assignment targets (`skipOuterExpressions`).
#[test]
fn tsx_parse_valid_v_model_ts_cast_forms() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import Child from './Child.vue'
import { ref } from 'vue'
const msg = ref<string | number>('Hello')
const obj = ref({ val: '' as string | number })
</script>
<template>
  <input v-model="(msg as string)" />
  <input v-model="obj.val as string" />
  <input v-model="msg!" />
  <Child v-model="(msg satisfies string | number)" />
</template>"#,
        "v-model with TS casts",
    );
}

#[test]
fn tsx_parse_valid_v_model_textarea() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const text = ref('')
</script>
<template>
  <textarea v-model="text"></textarea>
</template>"#,
        "v-model on textarea",
    );
}

#[test]
fn tsx_parse_valid_v_model_select() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const selected = ref('a')
</script>
<template>
  <select v-model="selected">
    <option value="a">A</option>
    <option value="b">B</option>
  </select>
</template>"#,
        "v-model on select",
    );
}

#[test]
fn tsx_parse_valid_v_model_checkbox() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const checked = ref(false)
</script>
<template>
  <input type="checkbox" v-model="checked" />
</template>"#,
        "v-model on checkbox",
    );
}

#[test]
fn tsx_parse_valid_v_model_component() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
import Comp from './Comp.vue'
const count = ref(0)
</script>
<template>
  <Comp v-model="count" />
</template>"#,
        "v-model on component",
    );
}

#[test]
fn tsx_parse_valid_v_model_named() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
import Comp from './Comp.vue'
const title = ref('')
</script>
<template>
  <Comp v-model:title="title" />
</template>"#,
        "v-model:title named model",
    );
}

#[test]
fn tsx_parse_valid_v_model_with_modifiers() {
    assert_tsx_parses(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const msg = ref('')
</script>
<template>
  <input v-model.trim.lazy="msg" />
</template>"#,
        "v-model with modifiers",
    );
}

#[test]
fn tsx_parse_valid_define_model() {
    let runtime = crate::test_helpers::runtime_bundle([crate::test_helpers::runtime_model_entry(
        0,
        0,
        "firstName",
        "firstNameModifiers",
        "update:firstName",
        true,
        [verter_macro_dto::RuntimeConstructor::String],
    )]);
    assert_tsx_parses_with_runtime(
        r#"<script setup lang="ts">
const firstName = defineModel<string>('firstName')
</script>
<template>
  <input v-model="firstName" />
</template>"#,
        "defineModel",
        runtime,
    );
}

#[test]
fn jsx_compile_v_model() {
    assert_jsx_parses(
        r#"<script setup>
import { ref } from 'vue'
const text = ref('')
</script>
<template><input v-model="text" /></template>"#,
        "JS SFC with v-model",
    );
}

#[test]
fn define_model_local_ref_default_is_compile_error() {
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst dft = ref('x')\nconst m = defineModel({ default: dft })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(
                "`defineModel()` in <script setup> cannot reference locally declared variables"
            )),
        "defineModel default referencing a setup-local must be rejected (official), got: {:?}",
        result.errors
    );
}

// =========================================================================
// defineModel get/set transformers are NOT hoisted — setup-local refs valid
// =========================================================================
//
// Official `processDefineModel` (3.6.0-rc.5) emits a defineModel options
// object's `get`/`set` transformer functions back INTO setup() (they wrap the
// model ref via `useModel`), so ONLY the non-get/set option properties are
// hoisted and scope-checked (`runtimeOptionNodes`). A setup-local referenced
// inside `get`/`set` is therefore VALID; a spread element or a computed key in
// the options object defeats static analysis and the whole object is skipped.
// (defineProps/defineEmits options ARE hoisted wholesale, so a setup-local
// there stays correctly rejected — see the tests above.)

#[test]
fn define_model_get_set_arrow_setup_local_stays_valid() {
    // The headline false-positive: get/set ARROW transformers referencing a
    // setup-local ref. Official accepts (they are emitted into setup()).
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst f = ref(0)\nconst m = defineModel({ get: () => f.value, set: (v) => { f.value = v } })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "defineModel get/set arrow transformers referencing a setup-local must stay valid (official emits them into setup()), got: {:?}",
        result.errors
    );
}

#[test]
fn define_model_get_set_method_form_setup_local_stays_valid() {
    // Method-shorthand form `get() { ... }` / `set(v) { ... }` — same key name,
    // still skipped.
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst f = ref(0)\nconst m = defineModel({ get() { return f.value }, set(v) { f.value = v } })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "defineModel get/set method-form transformers referencing a setup-local must stay valid, got: {:?}",
        result.errors
    );
}

#[test]
fn define_model_get_set_string_key_setup_local_stays_valid() {
    // String-literal keys `"get"` / `"set"` — official matches by key value.
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst f = ref(0)\nconst m = defineModel({ \"get\": () => f.value, \"set\": (v) => { f.value = v } })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "defineModel string-key get/set transformers referencing a setup-local must stay valid, got: {:?}",
        result.errors
    );
}

#[test]
fn define_model_options_spread_skips_scope_check() {
    // A spread element defeats static analysis — official leaves
    // `runtimeOptionNodes` empty and checks nothing (even a `get` referencing a
    // setup-local; `...extra` is an import here, the only setup-local is `f`).
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nimport { extra } from './x'\nconst f = ref(0)\nconst m = defineModel({ ...extra, get: () => f.value })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "defineModel options with a spread element must skip the scope check (official), got: {:?}",
        result.errors
    );
}

#[test]
fn define_model_options_computed_key_skips_scope_check() {
    // A computed key defeats static analysis — official skips the whole object.
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst k = ref('get')\nconst f = ref(0)\nconst m = defineModel({ [k.value]: () => f.value })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "defineModel options with a computed key must skip the scope check (official), got: {:?}",
        result.errors
    );
}

#[test]
fn define_model_named_get_set_setup_local_stays_valid() {
    // Named model: the options object is arg1; get/set are still skipped.
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst f = ref(0)\nconst m = defineModel('count', { get: () => f.value, set: (v) => { f.value = v } })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "named defineModel get/set transformers referencing a setup-local must stay valid, got: {:?}",
        result.errors
    );
}

#[test]
fn define_model_default_arrow_setup_local_still_errors() {
    // `default` IS a hoisted runtime option — a setup-local reference (even
    // behind an arrow) stays invalid. Guards against over-skipping.
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst f = ref(0)\nconst m = defineModel({ default: () => f.value })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(
                "`defineModel()` in <script setup> cannot reference locally declared variables"
            )),
        "defineModel `default` arrow referencing a setup-local must STILL be rejected (hoisted), got: {:?}",
        result.errors
    );
}

#[test]
fn define_model_named_default_setup_local_still_errors() {
    // Named model: `default` on arg1 is still a hoisted runtime option.
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst f = ref(0)\nconst m = defineModel('count', { default: f })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(
                "`defineModel()` in <script setup> cannot reference locally declared variables"
            )),
        "named defineModel `default` referencing a setup-local must STILL be rejected, got: {:?}",
        result.errors
    );
}

#[test]
fn define_model_default_function_scope_local_stays_valid() {
    // A function-scope local inside `default` is not a setup-local — valid,
    // exactly as for defineProps.
    let result = compile_sfc(
        "<script setup>\nconst m = defineModel({ default: () => { const x = 1; return x } })\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "defineModel `default` with only a function-scope local must stay valid, got: {:?}",
        result.errors
    );
}

// =========================================================================
// R7-1 — parenthesized NAMED defineModel options must still be scope-checked
// =========================================================================
//
// OXC materialises `(expr)` as an explicit `ParenthesizedExpression` node,
// whereas Babel (the official parser) folds parentheses into an
// `extra.parenthesized` flag and exposes no wrapper node. For the NAMED form
// `defineModel("name", ({ ... }))`, official reads `node.arguments[1]` and tests
// `options.type === "ObjectExpression"` — which holds in Babel because there is
// no wrapper node. We must peel the paren node (ONLY the paren node, NOT the TS
// wrappers, matching official's un-`unwrapTSNode`'d `arguments[1]`) to reach the
// same ObjectExpression; otherwise ALL scope checks are skipped and a hoisted
// `default: <setup-local>` is wrongly accepted.

#[test]
fn define_model_named_paren_default_setup_local_still_errors() {
    // The R7-1 defect: parenthesized named options — `default: f` (a setup-local)
    // is a hoisted runtime option and must be rejected. Was ACCEPTED (the paren
    // wrapper node was not recognised as an ObjectExpression, skipping the check).
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst f = ref(0)\nconst m = defineModel('count', ({ default: f }))\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(
                "`defineModel()` in <script setup> cannot reference locally declared variables"
            )),
        "parenthesized named defineModel `default` referencing a setup-local must be rejected, got: {:?}",
        result.errors
    );
}

#[test]
fn define_model_named_paren_get_skips_but_default_errors() {
    // Mixed: a `get` transformer (setup-scoped, valid) alongside a hoisted
    // `default: f` (invalid). Peeling the paren must restore the get/set split —
    // `get` stays fine while `default: f` is still rejected.
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst f = ref(0)\nconst m = defineModel('count', ({ get: () => f.value, default: f }))\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(
                "`defineModel()` in <script setup> cannot reference locally declared variables"
            )),
        "parenthesized named defineModel with a hoisted `default: f` must be rejected, got: {:?}",
        result.errors
    );
}

#[test]
fn define_model_named_paren_get_only_stays_valid() {
    // A parenthesized named options object with ONLY a `get` transformer
    // referencing a setup-local stays valid (get is emitted back into setup()).
    // Guards against the paren-peel over-rejecting the setup-scoped transformer.
    let result = compile_sfc(
        "<script setup>\nimport { ref } from 'vue'\nconst f = ref(0)\nconst m = defineModel('count', ({ get: () => f.value }))\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        !result
            .errors
            .iter()
            .any(|d| d.severity == crate::compile::CompileDiagnosticSeverity::Error),
        "parenthesized named defineModel with only a `get` transformer must stay valid, got: {:?}",
        result.errors
    );
}

#[test]
fn ts_wrapped_define_model_init_setup_local_default_is_error() {
    // `const m = (defineModel({...}) as any)` — a TS/paren-wrapped macro in a
    // variable init. Official peels `decl.init` via unwrapTSNode before isCallOf,
    // so the get/set-aware defineModel scope check still applies to the hoisted
    // `default` option.
    let result = compile_sfc(
        "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst f = ref(0)\nconst m = (defineModel({ default: f }) as any)\n</script>\n<template><div>x</div></template>",
    );
    assert!(
        result.errors.iter().any(|d| d.severity
            == crate::compile::CompileDiagnosticSeverity::Error
            && d.message.contains(
                "`defineModel()` in <script setup> cannot reference locally declared variables"
            )),
        "a TS-wrapped defineModel init must still be scope-checked (official peels decl.init), got: {:?}",
        result.errors
    );
}
