//! Integration / end-to-end / IDE-feature tests (cross-domain cohort).

use super::*;

// ── Helper-import-preamble boundary (auto-import re-anchoring) ──

/// IDE codegen publishes the typed helper-import-preamble end boundary on the source map for an
/// EMPTY `<script setup>` — the case with NO mapped runs, where the boundary is the LSP auto-import
/// classifier's only signal separating the leading preamble from trailing synthetic component code.
/// The boundary points at the first non-import (synthetic) line; everything before it is a helper
/// import (or blank).
#[test]
fn ide_source_map_publishes_helper_preamble_boundary_for_empty_setup() {
    let (code, json) = gen_tsx_script_with_sourcemap("<script setup lang=\"ts\">\n</script>\n");

    let map: serde_json::Value = serde_json::from_str(&json).expect("valid source map JSON");
    let boundary = &map["x_verter_helper_preamble_end"];
    assert!(
        boundary.is_object(),
        "IDE source map must publish x_verter_helper_preamble_end: {json}"
    );
    let line = boundary["line"].as_u64().expect("boundary line") as usize;
    let character = boundary["character"].as_u64().expect("boundary character");
    assert_eq!(
        character, 0,
        "the boundary is the start of the line immediately after the helper imports"
    );
    assert!(
        line >= 1,
        "at least one helper-import line precedes the boundary"
    );

    let lines: Vec<&str> = code.lines().collect();
    for (i, l) in lines.iter().enumerate().take(line) {
        assert!(
            l.is_empty() || l.starts_with("import "),
            "line {i} before the boundary must be a helper import (or blank), got: {l:?}"
        );
    }
    let boundary_line = lines.get(line).copied().unwrap_or("");
    assert!(
        !boundary_line.starts_with("import "),
        "the boundary line must be the first non-import (synthetic) line, got: {boundary_line:?}"
    );
}

/// The boundary is correct for a `<script setup>` WITH user code too: it lands after the helper
/// imports and before the user/synthetic body, never inside the trailing component wrapper.
#[test]
fn ide_source_map_helper_preamble_boundary_precedes_user_body() {
    let (code, json) = gen_tsx_script_with_sourcemap(
        "<script setup lang=\"ts\">\nconst count = 0\n</script>\n<template><div>{{ count }}</div></template>\n",
    );

    let map: serde_json::Value = serde_json::from_str(&json).expect("valid source map JSON");
    let line = map["x_verter_helper_preamble_end"]["line"]
        .as_u64()
        .expect("boundary line") as usize;

    let lines: Vec<&str> = code.lines().collect();
    for (i, l) in lines.iter().enumerate().take(line) {
        assert!(
            l.is_empty() || l.starts_with("import "),
            "line {i} before the boundary must be a helper import (or blank), got: {l:?}"
        );
    }
    assert!(
        !lines
            .get(line)
            .copied()
            .unwrap_or("")
            .starts_with("import "),
        "the boundary line is the first non-import line"
    );
}

// ── End-to-end tests ─────────────────────────────────────────

#[test]
fn end_to_end_generic_component() {
    let (code, _, tc) = gen_tsx_script_full(
        r#"<script setup lang="ts" generic="T extends { id: number }">
import { ref } from 'vue'
const item = {} as T
const count = ref(0)
</script>
<template><div>{{ item.id }}</div></template>"#,
    );

    // Wrapper function has generic
    assert!(code.contains("function ___VERTER___TemplateBindingFN<T extends { id: number }>()"));

    // Instance type should no longer be emitted
    assert!(!tc.contains("type ___VERTER___Instance"));
}

#[test]
fn end_to_end_non_generic_component() {
    let (code, _, tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );

    // Wrapper function — no generic
    assert!(code.contains("function ___VERTER___TemplateBindingFN()"));

    // Type constructs — Instance type should no longer be emitted
    assert!(!tc.contains("type ___VERTER___Instance"));
    // No component-level <T> generic in the verter type constructs (before ambient module)
    let ambient_start = tc
        .find(r#"declare module "@verter/types""#)
        .unwrap_or(tc.len());
    let tc_before_ambient = &tc[..ambient_start];
    assert!(
        !tc_before_ambient.contains("<T>"),
        "non-generic component type constructs should not contain <T>"
    );
}

// ── Vue built-in component auto-imports in TSX (#15) ────────────

#[test]
fn builtin_suspense_auto_imported() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><Suspense><div/></Suspense></template>"#,
    );
    assert!(
        code.contains("import { Suspense") || code.contains(", Suspense"),
        "Suspense should be auto-imported from vue: {code}"
    );
    assert!(
        !code.contains("_resolveComponent"),
        "built-in components should not use _resolveComponent: {code}"
    );
}

#[test]
fn builtin_transition_auto_imported() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const show = ref(true)
</script>
<template><Transition><div v-if="show"/></Transition></template>"#,
    );
    assert!(
        code.contains("import { Transition") || code.contains(", Transition"),
        "Transition should be auto-imported from vue: {code}"
    );
}

#[test]
fn builtin_multiple_auto_imported() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const x = 1
</script>
<template><Suspense><Teleport to="body"><div/></Teleport></Suspense></template>"#,
    );
    assert!(
        code.contains("Suspense"),
        "Suspense should be imported: {code}"
    );
    assert!(
        code.contains("Teleport"),
        "Teleport should be imported: {code}"
    );
}

#[test]
fn no_builtin_import_when_not_used() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const x = 1
</script>
<template><div>hello</div></template>"#,
    );
    // Should NOT import any built-in components when none are used
    assert!(
        !code.contains("Suspense"),
        "should not import Suspense when unused: {code}"
    );
    assert!(
        !code.contains("Teleport"),
        "should not import Teleport when unused: {code}"
    );
    assert!(
        !code.contains("KeepAlive"),
        "should not import KeepAlive when unused: {code}"
    );
}

#[test]
fn builtin_keep_alive_auto_imported() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const x = 1
</script>
<template><KeepAlive><div/></KeepAlive></template>"#,
    );
    assert!(
        code.contains("KeepAlive"),
        "KeepAlive should be auto-imported from vue: {code}"
    );
}

#[test]
fn builtin_kebab_case_auto_imported() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const x = 1
</script>
<template><keep-alive><div/></keep-alive></template>"#,
    );
    assert!(
        code.contains("KeepAlive"),
        "kebab-case keep-alive should auto-import KeepAlive: {code}"
    );
}

#[test]
fn tsx_contains_ambient_module_declaration() {
    let (_, _, type_constructs) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const props = defineProps<{ msg: string }>()
</script>
<template><div>{{ props.msg }}</div></template>"#,
    );

    // Positive: ambient module declaration is present with key exports
    assert!(
        type_constructs.contains(r#"declare module "@verter/types""#),
        "type_constructs must contain ambient module declaration"
    );
    assert!(
        type_constructs.contains("export type Prettify<T>"),
        "ambient module must export Prettify"
    );
    assert!(
        type_constructs.contains("export declare function shallowUnwrapRef"),
        "ambient module must export shallowUnwrapRef"
    );
    assert!(
        type_constructs.contains("export declare function enhanceElementWithProps"),
        "ambient module must export enhanceElementWithProps"
    );

    // Negative: removed features must NOT be present
    assert!(
        !type_constructs.contains("createMacroReturn"),
        "ambient module must NOT export createMacroReturn (removed)"
    );
    assert!(
        !type_constructs.contains("PublicInstanceFromMacro"),
        "ambient module must NOT export PublicInstanceFromMacro (removed)"
    );
    assert!(
        !type_constructs.contains("defineProps_Box"),
        "ambient module must NOT export defineProps_Box (removed)"
    );
    assert!(
        !type_constructs.contains("defineEmits_Box"),
        "ambient module must NOT export defineEmits_Box (removed)"
    );
    assert!(
        !type_constructs.contains("defineModel_Box"),
        "ambient module must NOT export defineModel_Box (removed)"
    );
    assert!(
        !type_constructs.contains("defineSlots_Box"),
        "ambient module must NOT export defineSlots_Box (removed)"
    );
    assert!(
        !type_constructs.contains("defineExpose_Box"),
        "ambient module must NOT export defineExpose_Box (removed)"
    );
    assert!(
        !type_constructs.contains("withDefaults_Box"),
        "ambient module must NOT export withDefaults_Box (removed)"
    );
    assert!(
        !type_constructs.contains("defineOptions_Box"),
        "ambient module must NOT export defineOptions_Box (removed)"
    );

    // Negative: no top-level `import ... from "vue"` inside declare module
    // (must use import("vue").X syntax instead)
    assert!(
        !type_constructs.contains(r#"import type { ShallowUnwrapRef"#),
        "ambient module must not use top-level import from vue"
    );
    // Verify it uses import("vue") syntax
    assert!(
        type_constructs.contains(r#"import("vue").ShallowUnwrapRef"#),
        "ambient module must use import(\"vue\").ShallowUnwrapRef syntax"
    );
}

#[test]
fn ambient_module_present_for_template_only() {
    let (_, _, type_constructs) = gen_tsx_script_full(r#"<template><div>hello</div></template>"#);

    assert!(
        type_constructs.contains(r#"declare module "@verter/types""#),
        "template-only SFC must also get ambient module declaration"
    );
}

#[test]
fn ambient_module_present_for_options_api() {
    let (_, _, type_constructs) = gen_tsx_script_full(
        r#"<script lang="ts">
export default { props: ['msg'] }
</script>
<template><div>{{ msg }}</div></template>"#,
    );

    assert!(
        type_constructs.contains(r#"declare module "@verter/types""#),
        "Options API SFC must also get ambient module declaration"
    );
}

#[test]
fn ambient_module_omitted_when_embed_false() {
    let (_, _, type_constructs) = gen_tsx_script_full_with_options(
        r#"<script setup lang="ts">
const props = defineProps<{ msg: string }>()
</script>
<template><div>{{ props.msg }}</div></template>"#,
        IdeScriptOptions {
            component_name: "App",
            js_component_name: "App",
            filename: "App.vue",
            scope_id: "data-v-abc123",
            has_scoped_style: false,
            runtime_module_name: "vue",
            macro_runtime: None,
            types_module_name: "@verter/types",
            is_vapor: false,
            embed_ambient_types: false,
            is_jsx: false,
            conditional_root_narrowing: false,
            style_v_bind_vars: vec![],
            style_usage_complete: true,
            css_modules: vec![],
            template_used_vars: None,
        },
    );

    assert!(
        !type_constructs.contains(r#"declare module "@verter/types""#),
        "ambient module should NOT be emitted when embed_ambient_types=false"
    );
}

// ── E2E Macro Type Checking ───────────────────────────────────────

/// @ai-generated — defineProps with runtime args stays as-is (no boxing).
#[test]
fn define_props_runtime_args_type_not_any() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const props = defineProps({ msg: String })
</script>"#,
    );
    // defineProps call preserved as-is
    assert!(
        code.contains("const props = defineProps({ msg: String })"),
        "defineProps call must be preserved: {}",
        code
    );
    // No boxing
    assert!(
        !code.contains("_Box"),
        "no boxing should be present: {}",
        code
    );
}

/// @ai-generated — defineEmits with runtime args stays as-is (no boxing).
#[test]
fn define_emits_runtime_args_type_not_any() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const emit = defineEmits(['change', 'update'])
</script>"#,
    );
    // defineEmits call preserved as-is
    assert!(
        code.contains("const emit = defineEmits(['change', 'update'])"),
        "defineEmits call must be preserved: {}",
        code
    );
    // No boxing
    assert!(
        !code.contains("_Box"),
        "no boxing should be present: {}",
        code
    );
}

/// @ai-generated — TS v5 parity: defineExpose must preserve call as-is (no boxing).
#[test]
fn define_expose_args_type_not_any() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
defineExpose({ focus: () => {} })
</script>"#,
    );
    // defineExpose stays as-is, no boxing
    assert!(
        code.contains("defineExpose({ focus: () => {} })"),
        "defineExpose call must be preserved: {}",
        code
    );
    // No boxing
    assert!(
        !code.contains("_Box"),
        "no boxing should be present: {}",
        code
    );
}

// ── IDE: IntelliSense — Correct Types in Interpolation (A) ──

/// @ai-generated — A1: ref binding appears in shallowUnwrapRef destructuring with correct cast
#[test]
fn ref_unwrap_in_interpolation() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );
    // Positive: ref binding should appear in the destructuring with the unwrap cast
    assert!(
        code.contains("count: count as unknown as typeof count"),
        "count should be in shallowUnwrapRef destructuring: {}",
        code
    );
    // Negative: no .value suffix — block scope handles unwrapping
    assert!(
        !code.contains("count.value"),
        "count.value must not appear — block scope unwraps: {}",
        code
    );
}

/// @ai-generated — A2: computed binding appears in shallowUnwrapRef destructuring
#[test]
fn computed_unwrap_in_interpolation() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
import { computed } from 'vue'
const doubled = computed(() => 2)
</script>
<template><div>{{ doubled }}</div></template>"#,
    );
    assert!(
        code.contains("doubled: doubled as unknown as typeof doubled"),
        "computed should be in shallowUnwrapRef destructuring: {}",
        code
    );
    assert!(
        !code.contains("doubled.value"),
        "doubled.value must not appear — block scope unwraps: {}",
        code
    );
}

/// @ai-generated — A3: reactive binding appears in destructuring (no unwrap needed but still present)
#[test]
fn reactive_passthrough_in_destructuring() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
import { reactive } from 'vue'
const state = reactive({x: 1})
</script>
<template><div>{{ state.x }}</div></template>"#,
    );
    assert!(
        code.contains("state: state as unknown as typeof state"),
        "reactive binding should be in shallowUnwrapRef destructuring: {}",
        code
    );
    // Negative: no _ctx prefix
    assert!(
        !code.contains("_ctx.state"),
        "_ctx.state must not appear — bare identifiers in block scope: {}",
        code
    );
}

/// @ai-generated — A4: multiple refs both appear in destructuring
#[test]
fn multiple_refs_in_interpolation() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
const msg = ref('')
</script>
<template><div>{{ count + msg }}</div></template>"#,
    );
    assert!(
        code.contains("count: count as unknown as typeof count"),
        "count should be in destructuring: {}",
        code
    );
    assert!(
        code.contains("msg: msg as unknown as typeof msg"),
        "msg should be in destructuring: {}",
        code
    );
    // Both in the same shallowUnwrapRef call
    assert!(
        code.contains("___VERTER___shallowUnwrapRef("),
        "should use shallowUnwrapRef: {}",
        code
    );
    // No .value on either
    assert!(
        !code.contains("count.value"),
        "count.value must not appear: {}",
        code
    );
    assert!(
        !code.contains("msg.value"),
        "msg.value must not appear: {}",
        code
    );
}

/// @ai-generated — A5: ref with nested property access (items.length) — no .value in output
#[test]
fn nested_property_on_ref() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const items = ref([])
</script>
<template><div>{{ items.length }}</div></template>"#,
    );
    assert!(
        code.contains("items: items as unknown as typeof items"),
        "items should be in shallowUnwrapRef destructuring: {}",
        code
    );
    assert!(
        !code.contains("items.value.length"),
        "items.value.length must not appear — block scope unwraps: {}",
        code
    );
}

// ── IDE: Component Resolution (B) ─────────────────────────────

/// @ai-generated — B6: imported component is in bindings and gets no global fallback
#[test]
fn imported_component_no_fallback() {
    let (code, bindings) = gen_tsx_script(
        r#"<script setup lang="ts">
import Foo from './Foo.vue'
</script>
<template><Foo /></template>"#,
    );
    // Positive: Foo should be in bindings
    assert!(
        bindings.contains_key("Foo"),
        "Foo should be in bindings: {:?}",
        bindings
    );
    // Negative: no global fallback for imported component
    assert!(
        !code.contains("as import('vue').GlobalComponents"),
        "imported component should not get GlobalComponents fallback: {}",
        code
    );
}

/// @ai-generated — B7: unresolved component gets GlobalComponents fallback
#[test]
fn unresolved_component_gets_global_fallback() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const x = 1
</script>
<template><RouterLink to="/" /></template>"#,
    );
    // Positive: global fallback for unresolved component
    assert!(
        code.contains("const RouterLink = {} as import('vue').GlobalComponents extends { RouterLink: infer C } ? C : unknown"),
        "unresolved component should get GlobalComponents fallback: {}",
        code
    );
    // Negative: RouterLink should NOT be in the destructuring
    assert!(
        !code.contains("RouterLink: RouterLink as unknown as typeof RouterLink"),
        "unresolved component should not be in destructuring: {}",
        code
    );
}

/// @ai-generated — B8: multiple unresolved components each get their own fallback
#[test]
fn multiple_unresolved_components() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const x = 1
</script>
<template><RouterLink to="/" /><RouterView /></template>"#,
    );
    assert!(
        code.contains("const RouterLink"),
        "RouterLink should get fallback const: {}",
        code
    );
    assert!(
        code.contains("const RouterView"),
        "RouterView should get fallback const: {}",
        code
    );
    // Both should use GlobalComponents
    assert!(
        code.contains("RouterLink: infer C"),
        "RouterLink fallback should use GlobalComponents infer: {}",
        code
    );
    assert!(
        code.contains("RouterView: infer C"),
        "RouterView fallback should use GlobalComponents infer: {}",
        code
    );
}

/// @ai-generated — B9: builtin component (Transition) is auto-imported, no GlobalComponents fallback
#[test]
fn builtin_component_no_fallback() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const x = 1
</script>
<template><Transition><div /></Transition></template>"#,
    );
    // Positive: Transition should be auto-imported from vue
    assert!(
        code.contains("import { Transition") || code.contains(", Transition"),
        "Transition should be auto-imported from vue: {}",
        code
    );
    // Negative: no GlobalComponents fallback for builtins
    assert!(
        !code.contains("as import('vue').GlobalComponents extends { Transition"),
        "builtin component should not get GlobalComponents fallback: {}",
        code
    );
}

/// @ai-generated — B10: component with ref attribute triggers Comp function emission
#[test]
fn component_with_ref_has_comp_function() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import Foo from './Foo.vue'
</script>
<template><Foo ref="myFoo" /></template>"#,
    );
    // Positive: Comp function emitted
    assert!(
        code.contains("function ___VERTER___Comp"),
        "Comp function should be emitted for component with ref: {}",
        code
    );
    // Positive: Comp function references Foo via instantiateComponent
    assert!(
        code.contains("instantiateComponent(Foo,"),
        "Comp function should instantiate Foo: {}",
        code
    );
    // Negative: no GlobalComponents fallback since Foo is imported
    assert!(
        !code.contains("as import('vue').GlobalComponents extends { Foo"),
        "imported Foo should not get GlobalComponents fallback: {}",
        code
    );
}

// ── IDE: Unused Binding Detection (C) ─────────────────────────

/// @ai-generated — C11: unused ref still appears in destructuring for TS to flag as unused
#[test]
fn unused_ref_in_destructuring() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const unused = ref(0)
</script>"#,
    );
    // Positive: unused ref is in destructuring so TS can flag it
    assert!(
        code.contains("unused: unused as unknown as typeof unused"),
        "unused ref should be in destructuring for TS unused detection: {}",
        code
    );
    // Negative: no _ctx prefix
    assert!(
        !code.contains("_ctx.unused"),
        "should not have _ctx prefix: {}",
        code
    );
}

/// @ai-generated — C12: used ref also appears in destructuring
#[test]
fn used_ref_in_destructuring() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(
        code.contains("count: count as unknown as typeof count"),
        "used ref should be in destructuring: {}",
        code
    );
    // Negative: no .value
    assert!(
        !code.contains("count.value"),
        "count.value must not appear: {}",
        code
    );
}

/// @ai-generated — C13: unused plain const also appears in destructuring
#[test]
fn unused_const_in_destructuring() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
    );
    assert!(
        code.contains("msg: msg as unknown as typeof msg"),
        "unused const should be in destructuring: {}",
        code
    );
    // Negative: no _ctx
    assert!(
        !code.contains("_ctx.msg"),
        "_ctx.msg should not appear: {}",
        code
    );
}

/// @ai-generated — C14: props are accessed via __props, not in destructuring
#[test]
fn props_not_in_destructuring() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const props = defineProps<{title: string}>()
</script>
<template><div>{{ title }}</div></template>"#,
    );
    // Positive: __props alias is created
    assert!(
        code.contains("const __props = props"),
        "__props alias should be created: {}",
        code
    );
    // Negative: individual prop fields are NOT in destructuring — they use __props.xxx
    assert!(
        !code.contains("title: title as unknown as typeof title"),
        "prop 'title' should NOT be in destructuring — accessed via __props: {}",
        code
    );
}

/// @ai-generated — C15: import bindings are not in destructuring (already hoisted)
#[test]
fn import_not_in_destructuring() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
import { helper } from './utils'
const x = helper()
</script>"#,
    );
    // Negative: imports are already hoisted, not in destructuring
    assert!(
        !code.contains("helper: helper as unknown as typeof helper"),
        "import 'helper' should NOT be in destructuring — already hoisted: {}",
        code
    );
    // Positive: setup binding IS in destructuring
    assert!(
        code.contains("x: x as unknown as typeof x"),
        "setup binding 'x' should be in destructuring: {}",
        code
    );
}

// ── IDE: v-if Type Narrowing in Block Scope (D) ───────────────

/// @ai-generated — D16: v-if uses bare identifiers, no _ctx prefix, no .value
#[test]
fn v_if_bare_identifiers_in_block_scope() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const value = ref<string | null>(null)
</script>
<template><div v-if="value">{{ value }}</div></template>"#,
    );
    // Positive: value in destructuring
    assert!(
        code.contains("value: value as unknown as typeof value"),
        "value should be in destructuring: {}",
        code
    );
    // Negative: no _ctx prefix anywhere in the output
    assert!(
        !code.contains("_ctx.value"),
        "_ctx.value must not appear — bare identifiers in block scope: {}",
        code
    );
    // Negative: no .value suffix
    assert!(
        !code.contains("value.value"),
        "value.value must not appear — block scope handles unwrapping: {}",
        code
    );
}

/// @ai-generated — D17: v-if/v-else-if/v-else chain uses bare identifiers
#[test]
fn v_if_v_else_chain_bare() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const isA = ref(true)
const isB = ref(false)
</script>
<template><div v-if="isA">A</div><div v-else-if="isB">B</div><div v-else>C</div></template>"#,
    );
    // Positive: both refs in destructuring
    assert!(
        code.contains("isA: isA as unknown as typeof isA"),
        "isA should be in destructuring: {}",
        code
    );
    assert!(
        code.contains("isB: isB as unknown as typeof isB"),
        "isB should be in destructuring: {}",
        code
    );
    // Negative: no _ctx anywhere in the output
    assert!(
        !code.contains("_ctx."),
        "_ctx. must not appear anywhere in TSX output: {}",
        code
    );
}

/// @ai-generated — D19: nested v-if uses bare identifiers in Comp guards
#[test]
fn nested_v_if_bare() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const a = ref(true)
const b = ref(true)
const el = ref<HTMLSpanElement>()
</script>
<template><div v-if="a"><span v-if="b" ref="el">nested</span></div></template>"#,
    );
    // Positive: condition uses bare identifier for outer
    assert!(
        code.contains("(a)"),
        "v-if condition should use bare 'a': {}",
        code
    );
    // Positive: inner condition uses bare identifier
    assert!(
        code.contains("(b)"),
        "nested v-if condition should use bare 'b': {}",
        code
    );
    // Negative: no _ctx
    assert!(
        !code.contains("_ctx."),
        "_ctx. must not appear in block scope output: {}",
        code
    );
}

/// @ai-generated — D20: v-for with v-if uses bare identifiers, no .value on ref
#[test]
fn v_for_with_v_if() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const items = ref([{active: true, name: 'a'}])
const el = ref<HTMLDivElement>()
</script>
<template><div v-for="item in items" v-if="item.active" ref="el">{{ item.name }}</div></template>"#,
    );
    // Positive: items in destructuring
    assert!(
        code.contains("items: items as unknown as typeof items"),
        "items should be in destructuring: {}",
        code
    );
    // Negative: no items.value — block scope unwraps
    assert!(
        !code.contains("items.value"),
        "items.value must not appear — block scope unwraps: {}",
        code
    );
    // Positive: v-for scoped variable used bare in Comp guard
    assert!(
        code.contains("item.active"),
        "v-for scoped variable 'item' should be used bare: {}",
        code
    );
}

// ── IDE: Event Handler Typing (E) ─────────────────────────────

/// @ai-generated — E21: inline event handler uses bare ref, no .value
#[test]
fn inline_handler_ref_bare() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>click</div></template>"#,
    );
    // Positive: count in destructuring for ref unwrap
    assert!(
        code.contains("count: count as unknown as typeof count"),
        "count should be in destructuring: {}",
        code
    );
    // Negative: no count.value
    assert!(
        !code.contains("count.value"),
        "count.value must not appear in block scope: {}",
        code
    );
    // Negative: no _ctx
    assert!(
        !code.contains("_ctx.count"),
        "_ctx.count must not appear: {}",
        code
    );
}

/// @ai-generated — E22: method reference in event handler uses bare identifier
#[test]
fn method_reference_bare() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
function handleClick() {}
</script>
<template><div>click</div></template>"#,
    );
    // Positive: handleClick in destructuring
    assert!(
        code.contains("handleClick: handleClick as unknown as typeof handleClick"),
        "handleClick should be in destructuring: {}",
        code
    );
    // Negative: no _ctx prefix
    assert!(
        !code.contains("_ctx.handleClick"),
        "_ctx.handleClick must not appear: {}",
        code
    );
}

// ── IDE: Slot + v-for Scoped Variables (F) ────────────────────

/// @ai-generated — F23: v-for scoped variable is NOT in destructuring, but the iterable is
#[test]
fn v_for_scoped_variable_not_in_destructuring() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const items = ref([{name: 'a'}])
const el = ref<HTMLDivElement>()
</script>
<template><div v-for="item in items" v-if="item.active" ref="el">{{ item.name }}</div></template>"#,
    );
    // Positive: iterable 'items' IS in destructuring
    assert!(
        code.contains("items: items as unknown as typeof items"),
        "items should be in destructuring: {}",
        code
    );
    // Negative: v-for scoped variable 'item' is NOT in destructuring (it's a loop variable)
    assert!(
        !code.contains("item: item as unknown"),
        "v-for scoped variable 'item' should NOT be in destructuring: {}",
        code
    );
    // Positive: item.active appears in comp function guard (v-if on v-for element with ref)
    assert!(
        code.contains("item.active"),
        "v-for scoped variable 'item' should be referenced bare in comp guard: {}",
        code
    );
}

// ── IDE: Self-Import and Instance Type (G) ────────────────────

/// @ai-generated — G25: self-import no longer emitted
#[test]
fn self_import_uses_filename() {
    let (code, _) = gen_tsx_script(
        r#"<script setup lang="ts">
const x = 1
</script>"#,
    );
    // Negative: self-import should no longer be emitted
    assert!(
        !code.contains("import type { default as ___VERTER___Self }"),
        "self-import should no longer be emitted: {}",
        code
    );
}

/// @ai-generated — G26: instance type no longer emitted
#[test]
fn instance_type_uses_self() {
    let (_code, _, tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const x = 1
</script>"#,
    );
    // Negative: instance type should no longer be emitted
    assert!(
        !tc.contains("type ___VERTER___Instance"),
        "instance type should no longer be emitted: {}",
        tc
    );
    // Negative: old patterns should not appear
    assert!(
        !tc.contains("PublicInstanceFromMacro"),
        "old PublicInstanceFromMacro pattern should not appear: {}",
        tc
    );
    assert!(
        !tc.contains("OmitConstructorSignature"),
        "old OmitConstructorSignature pattern should not appear: {}",
        tc
    );
}

/// @ai-generated — G27: getCurrentInstance emits ComponentInstance type override
#[test]
fn get_current_instance_emits_type() {
    let (_code, _, tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
import { getCurrentInstance } from 'vue'
const instance = getCurrentInstance()
</script>"#,
    );
    // Positive: should emit ComponentInternalInstance type alias
    assert!(
        tc.contains("ComponentInternalInstance"),
        "should emit ComponentInternalInstance type when getCurrentInstance is used: {}",
        tc
    );
    // Positive: should emit declare function override
    assert!(
        tc.contains("declare function getCurrentInstance()"),
        "should emit getCurrentInstance function override: {}",
        tc
    );
}

/// @ai-generated — G28: without getCurrentInstance, no CurrentComponentInstance type
#[test]
fn no_get_current_instance_no_type() {
    let (_code, _, tc) = gen_tsx_script_full(
        r#"<script setup lang="ts">
const x = 1
</script>"#,
    );
    // Negative: CurrentComponentInstance should NOT be emitted
    assert!(
        !tc.contains("CurrentComponentInstance"),
        "CurrentComponentInstance should NOT be emitted without getCurrentInstance: {}",
        tc
    );
    // Negative: Instance type should no longer be emitted either
    assert!(
        !tc.contains("type ___VERTER___Instance"),
        "Instance type should no longer be emitted: {}",
        tc
    );
}

// ── IDE: Generic Components (H) ───────────────────────────────

/// @ai-generated — H29: generic component uses generic parameter in wrapper function
#[test]
fn generic_block_scope() {
    let (code, _, tc) = gen_tsx_script_full(
        r#"<script setup lang="ts" generic="T extends string">
const value = {} as unknown as T
</script>
<template><div>{{ value }}</div></template>"#,
    );
    // Positive: wrapper function has generic parameter
    assert!(
        code.contains("export function ___VERTER___TemplateBindingFN<T extends string>()"),
        "wrapper function should have generic parameter: {}",
        code
    );
    // Positive: value in destructuring
    assert!(
        code.contains("value: value as unknown as typeof value"),
        "value should be in destructuring: {}",
        code
    );
    // Negative: instance type should no longer be emitted
    assert!(
        !tc.contains("InstanceType<typeof ___VERTER___Self>"),
        "instance type should no longer be emitted: {}",
        tc
    );
    // Negative: no non-generic wrapper
    assert!(
        !code.contains("function ___VERTER___TemplateBindingFN()"),
        "wrapper function should NOT be non-generic: {}",
        code
    );
}

/// @ai-generated — H30: multiple generic parameters all appear in wrapper function
#[test]
fn generic_multiple_params() {
    let (code, _, _tc) = gen_tsx_script_full(
        r#"<script setup lang="ts" generic="T, U extends Record<string, T>">
const t = {} as unknown as T
const u = {} as unknown as U
</script>"#,
    );
    // Positive: wrapper function has both generic parameters
    assert!(
        code.contains("function ___VERTER___TemplateBindingFN<T, U extends Record<string, T>>()"),
        "wrapper function should have both generic parameters: {}",
        code
    );
    // Positive: both bindings in destructuring
    assert!(
        code.contains("t: t as unknown as typeof t"),
        "t should be in destructuring: {}",
        code
    );
    assert!(
        code.contains("u: u as unknown as typeof u"),
        "u should be in destructuring: {}",
        code
    );
    // Negative: no non-generic wrapper
    assert!(
        !code.contains("function ___VERTER___TemplateBindingFN()"),
        "wrapper function should NOT be non-generic: {}",
        code
    );
}

// ── Recursive component self-reference (#28) ─────────────────

#[test]
fn recursive_component_self_reference_binding() {
    let source = r#"<script setup lang="ts">
const items = [1, 2, 3]
</script>
<template><div><TreeNode /></div></template>"#;
    let (code, bindings, _) =
        gen_tsx_script_full_with_opts(source, "TreeNode", "TreeNode.vue", vec![]);

    assert!(
        bindings.contains_key("TreeNode"),
        "TreeNode must be in bindings for template resolution: {:?}",
        bindings.keys().collect::<Vec<_>>()
    );
    assert!(
        code.contains("const TreeNode"),
        "self-reference const declaration must be emitted:\n{code}"
    );
    assert!(
        code.contains("import('./TreeNode.vue')"),
        "self-reference must use self-import:\n{code}"
    );
}

#[test]
fn recursive_component_self_ref_no_shadow() {
    let source = r#"<script setup lang="ts">
import TreeNode from './other/TreeNode.vue'
</script>
<template><div><TreeNode /></div></template>"#;
    let (code, bindings, _) =
        gen_tsx_script_full_with_opts(source, "TreeNode", "TreeNode.vue", vec![]);

    assert!(
        bindings.contains_key("TreeNode"),
        "TreeNode must be in bindings (from import)"
    );
    assert!(
        !code.contains("as typeof import('./TreeNode.vue').default"),
        "must not emit self-reference when user imports same name:\n{code}"
    );
}

#[test]
fn recursive_component_kebab_case_filename() {
    let source = r#"<script setup lang="ts">
const x = 1
</script>
<template><div><TreeNode /></div></template>"#;
    let (code, bindings, _) =
        gen_tsx_script_full_with_opts(source, "tree-node", "tree-node.vue", vec![]);

    assert!(
        bindings.contains_key("TreeNode"),
        "kebab-case filename must produce PascalCase binding: {:?}",
        bindings.keys().collect::<Vec<_>>()
    );
    assert!(
        code.contains("const TreeNode"),
        "self-reference const must use PascalCase name:\n{code}"
    );
}

// ── Issue #28 negative: recursive component NOT used in template ────

#[test]
fn recursive_component_not_used_no_declaration() {
    let source = r#"<script setup lang="ts">
const items = [1, 2, 3]
</script>
<template><div>{{ items }}</div></template>"#;
    let (code, bindings, _) =
        gen_tsx_script_full_with_opts(source, "TreeNode", "TreeNode.vue", vec![]);

    // Negative: TreeNode must NOT be in bindings when not referenced in template
    assert!(
        !bindings.contains_key("TreeNode"),
        "TreeNode must NOT be in bindings when not used in template: {:?}",
        bindings.keys().collect::<Vec<_>>()
    );
    // Negative: no self-reference declaration emitted
    assert!(
        !code.contains("const TreeNode"),
        "self-reference const must NOT be emitted when not used in template:\n{code}"
    );
}

// ── CSS module support (#76) ────────────────────────────────

#[test]
fn css_module_emits_style_binding() {
    let source = r#"<script setup lang="ts">
const x = 1
</script>
<template><div :class="$style.btn">click</div></template>"#;
    let css_modules = vec![CssModuleInfo {
        binding_name: "$style".to_string(),
        class_names: vec!["btn".to_string(), "card".to_string()],
    }];
    let (code, bindings, _) = gen_tsx_script_full_with_opts(source, "App", "App.vue", css_modules);

    assert!(
        bindings.contains_key("$style"),
        "$style must be in bindings: {:?}",
        bindings.keys().collect::<Vec<_>>()
    );
    assert!(
        code.contains("const $style"),
        "$style const declaration must be emitted:\n{code}"
    );
    assert!(
        code.contains("\"btn\": string"),
        "btn class must be in $style type:\n{code}"
    );
    assert!(
        code.contains("\"card\": string"),
        "card class must be in $style type:\n{code}"
    );
    // Negative: must not contain hashed class names
    assert!(
        !code.contains("Record<string, string>"),
        "should use typed object, not Record:\n{code}"
    );
}

#[test]
fn css_module_no_shadow_existing_binding() {
    // If user defines $style themselves, don't shadow it
    let source = r#"<script setup lang="ts">
const $style = useCssModule()
</script>
<template><div :class="$style.btn">click</div></template>"#;
    let css_modules = vec![CssModuleInfo {
        binding_name: "$style".to_string(),
        class_names: vec!["btn".to_string()],
    }];
    let (code, bindings, _) = gen_tsx_script_full_with_opts(source, "App", "App.vue", css_modules);

    assert!(
        bindings.contains_key("$style"),
        "$style must be in bindings (from user code)"
    );
    // Should NOT emit our generated declaration since user already has $style
    let count = code.matches("const $style").count();
    assert!(
        count <= 1,
        "should not emit duplicate $style declarations: found {count} in:\n{code}"
    );
}

#[test]
fn ide_script_with_multibyte_char_adjacent_to_binding_does_not_panic() {
    // A multibyte char wedged against a `defineProps` binding ident makes the
    // script un-parseable, so IDE error recovery runs its backward binding scan.
    // That scan must be UTF-8-boundary-safe: generating IDE TSX (CompileTarget::IDE
    // script half) must NOT panic ("byte index N is not a char boundary").
    let source = "<script setup lang=\"ts\">\nconst😀x = defineProps()\n</script>\n";
    let (code, _, _) = gen_tsx_script_full(source);
    assert!(
        !code.is_empty(),
        "IDE TSX generation must produce output for a recoverable broken setup, not panic"
    );
}
