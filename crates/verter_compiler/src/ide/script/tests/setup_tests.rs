//! Setup-pipeline tests (D1 cohort).

use super::*;

/// The `@verter/types` slot-argument extractor must match a slot member whose
/// type is optional (`((...) => any) | undefined`). An optional slot member's
/// type carries the `| undefined` arm, which the bare `(...args) => any`
/// conditional does NOT match — so it would resolve to `never` and break
/// `v-slot` destructuring for components with optional slots (e.g. RouterView's
/// `default?`). The generated declaration must unwrap the optional arm.
#[test]
fn standalone_and_ambient_types_preserve_slot_argument_maps() {
    let ambient_slot_signature =
        "TSlots extends Record<string, any>,\n    N extends keyof TSlots & string,";
    let standalone_slot_signature =
        "TSlots extends Record<string, any>,\n  N extends keyof TSlots & string,";
    let optional_aware_conditional =
        "): TSlots[N] extends ((...args: infer P) => any) | undefined ? P[0] : never;";
    let bare_non_optional_conditional =
        "): TSlots[N] extends (...args: infer P) => any ? P[0] : never;";

    assert!(
        VERTER_TYPES_AMBIENT_MODULE.contains(ambient_slot_signature),
        "ambient @verter/types declarations should infer the concrete slot map first"
    );
    assert!(
        VERTER_TYPES_AMBIENT_MODULE.contains(optional_aware_conditional),
        "ambient @verter/types declarations should preserve slot prop types for optional slots"
    );
    assert!(
        !VERTER_TYPES_AMBIENT_MODULE.contains(bare_non_optional_conditional),
        "ambient @verter/types declarations must not use the bare conditional that drops \
         optional slot props to `never`"
    );

    assert!(
        VERTER_TYPES_STANDALONE_DTS.contains(standalone_slot_signature),
        "standalone @verter/types stub should infer the concrete slot map first"
    );
    assert!(
        VERTER_TYPES_STANDALONE_DTS.contains(optional_aware_conditional),
        "standalone @verter/types stub should preserve slot prop types for optional slots"
    );
    assert!(
        !VERTER_TYPES_STANDALONE_DTS.contains(bare_non_optional_conditional),
        "standalone @verter/types stub must not use the bare conditional that drops \
         optional slot props to `never`"
    );
}

#[test]
fn vue_imports_rewritten_to_vue_ts() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
import MyComp from './MyComp.vue'
import { helper } from '../utils'
import Another from "@/components/Another.vue"
const x = 1
</script>"#,
    );

    // Positive: .vue imports should become .vue.ts
    assert!(
        code.contains("from './MyComp.vue.ts'"),
        "single-quoted .vue import should become .vue.ts: {code}"
    );
    assert!(
        code.contains("from \"@/components/Another.vue.ts\""),
        "double-quoted .vue import should become .vue.ts: {code}"
    );

    // Negative: non-.vue imports should NOT be rewritten
    assert!(
        code.contains("from '../utils'"),
        "non-.vue import must not be rewritten: {code}"
    );

    // Negative: should NOT have bare .vue' or .vue" (without .ts)
    assert!(
        !code.contains(".vue'") || code.contains(".vue.ts'"),
        "bare .vue' should not remain: {code}"
    );
    assert!(
        !code.contains(".vue\"") || code.contains(".vue.ts\""),
        "bare .vue\" should not remain: {code}"
    );
}

/// Companion `<script>` imports should also be rewritten to `.vue.ts`.
#[test]
fn companion_script_vue_imports_rewritten_to_vue_ts() {
    let (code, _) = gen_tsx_script(
        r#"<script>
import Base from './Base.vue'
export default { extends: Base }
</script>
<script setup>
const x = 1
</script>"#,
    );

    assert!(
        code.contains("from './Base.vue.ts'"),
        "companion script .vue import should become .vue.ts: {code}"
    );
}

/// Re-exports like `export { Foo } from './Foo.vue'` should also be rewritten.
#[test]
fn reexport_vue_specifier_rewritten_to_vue_ts() {
    let (code, _) = gen_tsx_script(
        r#"<script>
export { default as Dropdown } from './Dropdown.vue'
export * from './utils'
</script>
<script setup>
const x = 1
</script>"#,
    );

    // Positive: .vue re-export should become .vue.ts
    assert!(
        code.contains("from './Dropdown.vue.ts'"),
        "re-export .vue specifier should become .vue.ts: {code}"
    );

    // Negative: non-.vue re-export should NOT be rewritten
    assert!(
        code.contains("from './utils'"),
        "non-.vue re-export must not be rewritten: {code}"
    );
}

#[test]
fn basic_script_setup() {
    let (code, bindings) = gen_tsx_script(
        r#"<script setup>
const msg = 'hello'
</script>"#,
    );

    assert!(code.contains("function ___VERTER___TemplateBindingFN()"));
    assert!(code.contains("const msg = 'hello'"));
    assert!(bindings.contains_key("msg"));
}

// ── Instance declaration tests ───────────────────────────────

#[test]
fn instance_declaration_in_script_setup() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(
        code.contains("let ___VERTER___instance!:"),
        "Should declare instance variable. Got: {}",
        code
    );
    assert!(
        code.contains("InstanceType<import("),
        "Should use InstanceType import. Got: {}",
        code
    );
    assert!(
        code.contains("import('./App.vue.ts')"),
        "Should reference the component's own .vue.ts file. Got: {}",
        code
    );
    assert!(
        code.contains("void ___VERTER___instance;"),
        "Should void-suppress instance. Got: {}",
        code
    );
}

#[test]
fn instance_probe_line_in_script_setup() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(
        code.contains("(___VERTER___instance).valueOf"),
        "Should have probe line. Got: {}",
        code
    );
}

#[test]
fn instance_declaration_in_template_only() {
    let (code, _) = gen_tsx_script(r#"<template><div>hello</div></template>"#);
    assert!(
        code.contains("let ___VERTER___instance!:"),
        "Template-only SFC should declare instance. Got: {}",
        code
    );
    assert!(
        code.contains("(___VERTER___instance).valueOf"),
        "Template-only SFC should have probe line. Got: {}",
        code
    );
}

#[test]
fn instance_declaration_in_options_api() {
    let (code, _) = gen_tsx_script(
        r#"<script>
export default {
  data() { return { count: 0 } }
}
</script>
<template><div>{{ count }}</div></template>"#,
    );
    assert!(
        code.contains("declare let ___VERTER___instance:"),
        "Options API should use ambient instance declaration. Got: {}",
        code
    );
    assert!(
        code.contains("InstanceType<import("),
        "Options API should use InstanceType import. Got: {}",
        code
    );
}

#[test]
fn script_setup_with_imports() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
import { ref } from 'vue'
import type { Foo } from './types'
const count = ref(0)
</script>"#,
    );

    // Imports should be hoisted above the function wrapper
    let fn_pos = code.find("function ___VERTER___TemplateBindingFN").unwrap();
    let import_ref_pos = code.find("import { ref } from 'vue'").unwrap();
    let import_type_pos = code.find("import type { Foo } from './types'").unwrap();

    assert!(
        import_ref_pos < fn_pos,
        "Runtime import should be hoisted above function"
    );
    assert!(
        import_type_pos < fn_pos,
        "Type import should be hoisted above function"
    );
}

#[test]
fn script_setup_with_type_declarations() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
interface Props {
  msg: string
}
const msg = 'hello'
</script>"#,
    );

    // Type declaration should be hoisted
    let fn_pos = code.find("function ___VERTER___TemplateBindingFN").unwrap();
    let interface_pos = code.find("interface Props").unwrap();
    assert!(
        interface_pos < fn_pos,
        "Interface should be hoisted above function"
    );
}

#[test]
fn script_setup_preserves_macros() {
    let (code, _) = gen_tsx_script(
        r#"<script setup>
const props = defineProps<{ msg: string }>()
</script>"#,
    );

    // Macros should be preserved in the body (not transformed)
    assert!(code.contains("defineProps"));
}

#[test]
fn script_setup_extracts_ref_bindings() {
    let (_, bindings) = gen_tsx_script(
        r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>"#,
    );

    assert_eq!(
        bindings.get("count").copied(),
        Some(BindingType::SetupRef),
        "ref() binding should be SetupRef"
    );
}

#[test]
fn script_setup_extracts_const_bindings() {
    let (_, bindings) = gen_tsx_script(
        r#"<script setup>
const msg = 'hello'
const fn = () => {}
</script>"#,
    );

    assert!(
        matches!(
            bindings.get("msg").copied(),
            Some(BindingType::SetupConst) | Some(BindingType::LiteralConst)
        ),
        "String constant should be SetupConst or LiteralConst"
    );
}

#[test]
fn options_api_script() {
    let (code, _) = gen_tsx_script(
        r#"<script>
export default {
  data() {
return { msg: 'hello' }
  }
}
</script>"#,
    );

    assert!(
        code.contains("const __sfc__ ="),
        "export default should be converted to const __sfc__ ="
    );
    assert!(
        code.contains("export default __sfc__"),
        "Should have export default __sfc__ at the end"
    );
}

#[test]
fn no_script_blocks() {
    let (code, _) = gen_tsx_script(
        r#"<template>
  <div>hello</div>
</template>"#,
    );

    assert!(
        code.contains("function ___VERTER___TemplateBindingFN()"),
        "Should emit minimal component wrapper"
    );
}

#[test]
fn script_setup_lang_ts_with_type_define_props() {
    // Regression: lang="ts" with defineProps<{...}>() caused a panic because
    // type-based prop binding spans include the content offset (absolute),
    // while content_str is local (relative).
    let (code, bindings) = gen_tsx_script(
        r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
    );

    assert!(
        code.contains("defineProps"),
        "Should preserve defineProps call"
    );
    // "msg" should be classified as a Props binding
    assert_eq!(
        bindings.get("msg").copied(),
        Some(BindingType::Props),
        "msg should be Props, got: {:?}",
        bindings.get("msg")
    );
}

#[test]
fn script_setup_lang_ts_with_assigned_define_props() {
    // const props = defineProps<{...}>() — "props" is SetupConst, "count" is Props
    let (code, bindings) = gen_tsx_script(
        r#"<script setup lang="ts">
const props = defineProps<{ count: number }>()
</script>"#,
    );

    assert!(code.contains("defineProps"));
    assert_eq!(
        bindings.get("props").copied(),
        Some(BindingType::SetupConst),
        "props variable should be SetupConst"
    );
    assert_eq!(
        bindings.get("count").copied(),
        Some(BindingType::Props),
        "count should be Props, got: {:?}",
        bindings.get("count")
    );
}

#[test]
fn script_setup_lang_ts_with_interface_props() {
    // defineProps with a type reference to a local interface
    let (code, bindings) = gen_tsx_script(
        r#"<script setup lang="ts">
interface MyProps {
  title: string
  count?: number
}
defineProps<MyProps>()
</script>"#,
    );

    assert!(code.contains("defineProps"));
    assert_eq!(
        bindings.get("title").copied(),
        Some(BindingType::Props),
        "title should be Props, got: {:?}",
        bindings.get("title")
    );
    assert_eq!(
        bindings.get("count").copied(),
        Some(BindingType::Props),
        "count should be Props, got: {:?}",
        bindings.get("count")
    );
}

// ── ISSUE-7: unused `<script setup>` local liveness ───────────────────
//
// An unused top-level setup binding must NOT be kept artificially live by the
// `___VERTER___unwrapped` object. A binding used NOWHERE (template, script body,
// style v-bind) is OMITTED entirely from the unwrapped object AND the destructure
// block, so the user's `const foo` is its sole occurrence and TS6133 lands on the
// MAPPED source declaration. A type-only entry (`undefined as unknown as typeof
// foo`) would NOT work: the `typeof foo` query keeps the source decl live and the
// diagnostic falls on the unmapped destructure copy (collapsing to line 1). A
// binding used SOMEWHERE keeps a value-read entry (`foo as unknown as typeof
// foo`).

#[test]
fn unused_setup_binding_is_omitted_from_unwrap() {
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const foo = 1
</script>

<template>
  <div>hello</div>
</template>"#,
    );

    // The unused binding must be omitted from the unwrap surface entirely so its
    // SOURCE decl carries TS6133 at the mapped declaration line.
    assert_omitted(&code, "foo");
    // The user's `const foo` decl must survive untouched.
    assert!(
        code.contains("const foo = 1"),
        "the source `const foo` decl must remain.\nTSX:\n{}",
        code
    );
}

#[test]
fn template_used_setup_binding_keeps_value_read_unwrap_entry() {
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const foo = 1
</script>

<template>
  <div>{{ foo }}</div>
</template>"#,
    );

    // Used in template -> value read preserved (decl stays live, no false TS6133).
    assert!(
        code.contains("foo: foo as unknown as typeof foo"),
        "template-used `foo` must keep its value-read unwrap entry.\nTSX:\n{}",
        code
    );
    assert!(
        !code.contains("foo: undefined as unknown as typeof foo"),
        "template-used `foo` must NOT be demoted to a type-only entry.\nTSX:\n{}",
        code
    );
}

#[test]
fn script_used_setup_binding_keeps_value_read_unwrap_entry() {
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const foo = 1
console.log(foo)
</script>

<template>
  <div>hello</div>
</template>"#,
    );

    assert!(
        code.contains("foo: foo as unknown as typeof foo"),
        "script-body-used `foo` must keep its value-read unwrap entry.\nTSX:\n{}",
        code
    );
    assert!(
        !code.contains("foo: undefined as unknown as typeof foo"),
        "script-body-used `foo` must NOT be demoted to a type-only entry.\nTSX:\n{}",
        code
    );
}

#[test]
fn style_v_bind_used_setup_binding_keeps_value_read_unwrap_entry() {
    // `foo` appears only in a style `v-bind(foo)` -- modelled by the
    // style_v_bind_vars inventory the host supplies to IDE codegen.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const foo = 1
</script>

<template>
  <div>hello</div>
</template>

<style>
.x { color: v-bind(foo); }
</style>"#,
    );

    assert!(
        code.contains("foo: foo as unknown as typeof foo"),
        "style-v-bind-used `foo` must keep its value-read unwrap entry.\nTSX:\n{}",
        code
    );
    assert!(
        !code.contains("foo: undefined as unknown as typeof foo"),
        "style-v-bind-used `foo` must NOT be demoted to a type-only entry.\nTSX:\n{}",
        code
    );
}

#[test]
fn mixed_used_and_unused_setup_bindings_discriminate() {
    // `used` is referenced in the template; `dead` is referenced nowhere.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const used = 1
const dead = 2
</script>

<template>
  <div>{{ used }}</div>
</template>"#,
    );

    assert!(
        code.contains("used: used as unknown as typeof used"),
        "template-used `used` keeps its value read.\nTSX:\n{}",
        code
    );
    // `dead` is referenced nowhere → omitted from the unwrap surface so its
    // source decl carries TS6133 at the mapped line.
    assert_omitted(&code, "dead");
}

// ── ISSUE-7 REWORK: false-positive vectors (must NOT be omitted) ──────
//
// Each of these binds something used ONLY through a path the previous
// unsound usage detection missed. Omitting the binding here would be a
// false-positive TS6133 on a genuinely-used binding — strictly worse than the
// original no-diagnostic bug. Every test is discriminating: it FAILS if the
// usage union drops the path it exercises.

/// Helper: assert a binding keeps its value-read unwrap entry (NOT omitted).
fn assert_kept_live(code: &str, name: &str) {
    assert!(
        code.contains(&format!("{name}: {name} as unknown as typeof {name}")),
        "`{name}` must keep its value-read unwrap entry (no false TS6133).\nTSX:\n{code}"
    );
    // The retired type-only shape must never appear (it mis-positioned TS6133).
    assert!(
        !code.contains(&format!("{name}: undefined as unknown as typeof {name}")),
        "`{name}` must NOT use the retired type-only unwrap entry.\nTSX:\n{code}"
    );
}

#[test]
fn kebab_component_tag_keeps_camel_binding_live() {
    // `<my-comp/>` resolves to the `const myComp` binding (camelCase form). The
    // old detector only added the raw tag + PascalCase, missing `myComp`.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import { defineComponent } from 'vue'
const myComp = defineComponent({})
</script>

<template>
  <my-comp />
</template>"#,
    );
    assert_kept_live(&code, "myComp");
}

#[test]
fn v_slot_default_slot_reference_keeps_binding_live() {
    // `rowData` is referenced ONLY inside a v-slot default-slot expression.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import Table from './Table.vue'
const rowData = { id: 1 }
</script>

<template>
  <Table>
    <template #default="{ row }">{{ row }}{{ rowData }}</template>
  </Table>
</template>"#,
    );
    assert_kept_live(&code, "rowData");
}

#[test]
fn scoped_prefix_reference_keeps_binding_live() {
    // `item` is a v-for scope local; `items` is a setup binding that is a PREFIX
    // of `item`. With the completion overlay, `items` references were suppressed
    // (completion mis-attribution); the `ide_completion = false` liveness overlay
    // keeps `items` a real reference.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const items = [1, 2, 3]
</script>

<template>
  <div v-for="item in items" :key="item">{{ item }}</div>
</template>"#,
    );
    assert_kept_live(&code, "items");
}

#[test]
fn script_member_update_keeps_binding_live() {
    // `c` is used ONLY via `c.value++` — the old walker dropped the member root
    // of an update target. THE headline false-positive vector.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const c = ref(0)
function inc() { c.value++ }
</script>

<template>
  <button @click="inc()">go</button>
</template>"#,
    );
    assert_kept_live(&code, "c");
}

#[test]
fn script_member_assignment_keeps_binding_live() {
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import { reactive } from 'vue'
const foo = reactive({ x: 0 })
function set() { foo.x = 1 }
</script>

<template>
  <button @click="set()">go</button>
</template>"#,
    );
    assert_kept_live(&code, "foo");
}

#[test]
fn script_computed_member_assignment_keeps_binding_live() {
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import { reactive } from 'vue'
const foo = reactive<Record<string, number>>({})
const key = 'a'
function set() { foo[key] = 1 }
</script>

<template>
  <button @click="set()">go</button>
</template>"#,
    );
    assert_kept_live(&code, "foo");
    assert_kept_live(&code, "key");
}

#[test]
fn script_class_body_reference_keeps_binding_live() {
    // `dep` is used only inside a class method body — a `_ => {}` skip in the
    // old walker.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const dep = 1
class Thing { value() { return dep } }
const t = new Thing()
</script>

<template>
  <div>{{ t.value() }}</div>
</template>"#,
    );
    assert_kept_live(&code, "dep");
}

#[test]
fn script_labeled_statement_reference_keeps_binding_live() {
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const dep = 1
function run() { outer: { console.log(dep) } }
</script>

<template>
  <button @click="run()">go</button>
</template>"#,
    );
    assert_kept_live(&code, "dep");
}

#[test]
fn computed_initializer_reference_keeps_count_live() {
    // `count` is referenced ONLY inside the initializer of `doubled` (a top-level
    // decl). BindingVisitor would ignore the top-level decl and miss this use.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import { ref, computed } from 'vue'
const count = ref(0)
const doubled = computed(() => count.value * 2)
</script>

<template>
  <div>{{ doubled }}</div>
</template>"#,
    );
    assert_kept_live(&code, "count");
}

#[test]
fn malformed_template_fails_open_no_demotion() {
    // A malformed template yields `template_used_vars = None` (incomplete) → the
    // gate fails open and NO binding is demoted, even the otherwise-unused one.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const dead = 1
</script>

<template>
  <div>{{ }}</div
</template>"#,
    );
    // Fail open: `dead` keeps its value-read entry (no omission, no TS6133).
    assert_kept_live(&code, "dead");
}

#[test]
fn style_v_bind_complex_expression_keeps_binding_live() {
    // `gap` is used only in `v-bind(gap + 1)` — the host `.split('.')` heuristic
    // would yield the literal `"gap + 1"`, dropping `gap`. The sound style scan
    // parses the expression and keeps `gap` live.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const gap = 4
</script>

<template>
  <div>hello</div>
</template>

<style>
.x { margin: v-bind(gap + 1); }
</style>"#,
    );
    assert_kept_live(&code, "gap");
}

#[test]
fn malformed_style_v_bind_fails_open_no_demotion() {
    // An unparseable `v-bind()` marks style usage incomplete → fail open: NO
    // binding is demoted (even an otherwise-unused one).
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const dead = 1
</script>

<template>
  <div>hello</div>
</template>

<style>
.x { color: v-bind(@@@); }
</style>"#,
    );
    // Fail open: `dead` keeps its value-read entry (no omission, no TS6133).
    assert_kept_live(&code, "dead");
}

#[test]
fn global_named_binding_used_in_template_only_keeps_live() {
    // `Date` is a setup binding (shadows the JS global) used ONLY as `{{ Date }}`.
    // The template liveness path applied the `is_global` binding filter, dropping
    // `Date` from `template_used_vars` → false TS6133. It must stay live.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const Date = ref(0)
</script>

<template>
  <div>{{ Date }}</div>
</template>"#,
    );
    assert_kept_live(&code, "Date");
}

#[test]
fn global_named_binding_used_via_script_member_only_keeps_live() {
    // `Map` is a setup binding (shadows the JS global) used ONLY via `Map.value++`
    // in the script body. The script liveness path applied the `is_global` filter,
    // dropping `Map` → false TS6133. It must stay live.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const Map = ref(0)
function inc() { Map.value++ }
</script>

<template>
  <button @click="inc()">go</button>
</template>"#,
    );
    assert_kept_live(&code, "Map");
}

#[test]
fn style_computed_key_v_bind_keeps_binding_live() {
    // `obj` and `key` are used ONLY via `v-bind(obj[key])` (a computed-key member).
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const obj = { a: 1 }
const key = 'a'
</script>

<template>
  <div>hello</div>
</template>

<style>
.x { color: v-bind(obj[key]); }
</style>"#,
    );
    assert_kept_live(&code, "obj");
    assert_kept_live(&code, "key");
}

#[test]
fn unparseable_template_expression_fails_open_no_demotion() {
    // A single template EXPRESSION that fails to parse (`{{ count + }}`) makes the
    // template usage UNKNOWN — its references cannot be soundly determined. The
    // gate must fail open (template usage incomplete ⇒ `None`), so even the
    // otherwise-unused `dead` is NOT demoted. (This is a template-expression-level
    // parse error, distinct from an SFC-level tokenizer error.)
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const dead = 1
</script>

<template>
  <div>{{ count + }}</div>
</template>"#,
    );
    // Fail open: `dead` keeps its value-read entry (no omission, no TS6133).
    assert_kept_live(&code, "dead");
}

#[test]
fn script_parse_recovery_fails_open_no_demotion() {
    // The `<script setup>` body has a GENUINE syntax error, so OXC recovers and
    // the program it hands the ref collector is empty/degraded → the script usage
    // facts UNDER-COUNT. A binding that LOOKS unused (`dead`) may actually be used
    // by the broken/recovered code, so the gate must fail open: `script_complete`
    // is false and NO binding is demoted.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const dead = 1
const broken = (
</script>

<template>
  <div>hello</div>
</template>"#,
    );
    // Fail open: `dead` keeps its value-read entry (no omission, no TS6133).
    assert_kept_live(&code, "dead");
}

#[test]
fn global_named_binding_used_only_in_vfor_source_keeps_live() {
    // `Date` is a setup binding (shadows the JS global) used ONLY as the SOURCE of
    // a v-for (`v-for="x in Date"`). The v-for source-reference extraction routes
    // through `collect_expression_reference_spans`, which applied the `is_global`
    // filter, dropping `Date` from the liveness usage set → false TS6133. It must
    // stay live. Discriminating: FAILS pre-fix (v-for liveness drops globals).
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const Date = ref<number[]>([])
</script>

<template>
  <div v-for="x in Date" :key="x">{{ x }}</div>
</template>"#,
    );
    assert_kept_live(&code, "Date");
}

#[test]
fn global_named_binding_used_only_in_vslot_default_keeps_live() {
    // `Map` is a setup binding (shadows the JS global) used ONLY inside a v-slot
    // default-value expression (`#default="{ row = Map }"`). The v-slot reference
    // extraction routes through `collect_expression_reference_spans`, which applied
    // the `is_global` filter, dropping `Map` → false TS6133. It must stay live.
    // Discriminating: FAILS pre-fix (v-slot liveness drops globals).
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import Table from './Table.vue'
const Map = { id: 1 }
</script>

<template>
  <Table>
    <template #default="{ row = Map }">{{ row }}</template>
  </Table>
</template>"#,
    );
    assert_kept_live(&code, "Map");
}

#[test]
fn empty_interpolation_is_complete_and_unused_binding_still_omitted() {
    // An EMPTY interpolation (`{{ }}`) parses cleanly (`errors: None`, references
    // nothing) and must NOT mark the template usage set incomplete. The gate stays
    // CLOSED, so a genuinely-unused binding (`dead`) is STILL omitted — empty
    // interpolation must not fail open and silently suppress every diagnostic.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const dead = 1
</script>

<template>
  <div>{{ }}</div>
</template>"#,
    );
    // Empty interpolation is COMPLETE — the unused binding is still omitted from
    // the unwrap surface so its source decl carries TS6133.
    assert_omitted(&code, "dead");
}

#[test]
fn style_v_bind_spread_and_lhs_keep_bindings_live() {
    // `parts` (spread into an array) and `width` (LHS of a comma sequence) are each
    // referenced ONLY through a non-trivial style `v-bind()` shape. The sound style
    // scan parses each expression into identifier roots, so both stay live.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const parts = [1, 2]
const width = 4
</script>

<template>
  <div>hello</div>
</template>

<style>
.a { grid-template-columns: v-bind([...parts]); }
.b { width: v-bind((width, 0)); }
</style>"#,
    );
    assert_kept_live(&code, "parts");
    assert_kept_live(&code, "width");
}

#[test]
fn computed_getter_and_setter_keep_dependency_live() {
    // `count` is referenced ONLY inside a computed with BOTH a getter and a setter.
    // Codex noted earlier coverage tested only the getter form; this pins the
    // get/set object form keeps the dependency live end-to-end.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import { ref, computed } from 'vue'
const count = ref(0)
const proxy = computed({
  get: () => count.value,
  set: (v: number) => { count.value = v },
})
</script>

<template>
  <div>{{ proxy }}</div>
</template>"#,
    );
    assert_kept_live(&code, "count");
}

#[test]
fn static_block_used_binding_keeps_live_end_to_end() {
    // `dep` is referenced ONLY inside a class static block — end-to-end through the
    // full IDE codegen liveness path (the Visit collector descends into static
    // blocks). Pins the static-block keep-live at the integration layer.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const dep = 1
class Registry { static { console.log(dep) } }
const r = new Registry()
</script>

<template>
  <div>{{ r }}</div>
</template>"#,
    );
    assert_kept_live(&code, "dep");
}

// ── Nested-construct liveness ────────────────────────────────────────────────
//
// A setup binding referenced ONLY inside a nested expression — a callback body in
// a v-for source / v-slot default, or an IIFE inside an interpolation — must be
// counted as USED. Every liveness reference feeder runs through the single
// complete `Visit` collector (`collect_expression_free_refs`), which recurses into
// arrow / function bodies, call arguments, switch / try, template literals,
// spreads, computed keys and assignment LHS via the default `walk::*`. A nested
// reference therefore can never be silently dropped, so a genuinely-used binding
// is never demoted to a false TS6133. Each test below is discriminating: a
// reference collector that skipped callback bodies would demote the binding and
// fail the assertion.

/// Helper: assert a proven-unused binding is OMITTED entirely from the
/// `___VERTER___unwrapped` object AND the destructure block, so its SOURCE
/// `const name` decl is its sole remaining occurrence and TS6133 lands on the
/// mapped declaration (not an unmapped destructure copy collapsing to line 1).
/// The inverse of `assert_kept_live`.
///
/// A type-only entry (`undefined as unknown as typeof name`) is NOT acceptable:
/// the `typeof name` query keeps the source decl live, so the diagnostic would
/// fall on the unmapped destructure copy and collapse to line 1.
fn assert_omitted(code: &str, name: &str) {
    // The unwrapped object must NOT carry a value-read entry for the binding…
    assert!(
        !code.contains(&format!("{name}: {name} as unknown as typeof {name}")),
        "proven-unused `{name}` must NOT keep its value-read unwrap entry.\nTSX:\n{code}"
    );
    // …and must NOT carry the retired type-only entry (which keeps the source
    // decl live via `typeof name` and mis-positions TS6133 to line 1).
    assert!(
        !code.contains(&format!("{name}: undefined as unknown as typeof {name}")),
        "proven-unused `{name}` must NOT use a type-only unwrap entry (keeps decl live → wrong TS6133 position).\nTSX:\n{code}"
    );
    // …and must NOT be reachable through `typeof {name}` anywhere in the
    // generated unwrap surface (the keep-alive shape that suppressed the
    // diagnostic at the source decl).
    assert!(
        !code.contains(&format!("typeof {name}")),
        "proven-unused `{name}` must not be referenced via `typeof {name}` (would keep the source decl live).\nTSX:\n{code}"
    );
    // The user's source declaration must survive untouched so it carries TS6133.
    assert!(
        code.contains(&format!("const {name}")) || code.contains(&format!("let {name}")),
        "the source `const {name}`/`let {name}` decl must remain (it carries TS6133).\nTSX:\n{code}"
    );
}

#[test]
fn vfor_source_nested_callback_reference_keeps_binding_live() {
    // `fmt` is referenced ONLY inside a callback in the v-for SOURCE expression
    // (`rows.map(r => fmt(r))`). The partial liveness span walker recursed into
    // the `.map()` CallExpression but dropped the arrow-function ARGUMENT body
    // at `_ => {}`, missing `fmt` -> false TS6133. The complete Visit collector
    // descends into the callback body. Discriminating: FAILS pre-fix.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const rows = [{ id: 1 }]
const fmt = (r: { id: number }) => r.id
</script>

<template>
  <div v-for="x in rows.map(r => fmt(r))" :key="x">{{ x }}</div>
</template>"#,
    );
    // `rows` is the bare v-for source root (caught even by the old walker); the
    // class-defining case is `fmt`, used ONLY inside the callback body.
    assert_kept_live(&code, "rows");
    assert_kept_live(&code, "fmt");
}

#[test]
fn vfor_source_global_named_inside_callback_keeps_binding_live() {
    // `Date` is a setup binding (shadows the JS global) referenced ONLY inside a
    // zero-arg callback in the v-for source (`items.map(() => Date)`). This needs
    // BOTH fixes at once: descend into the callback body AND retain global-named
    // identifiers for liveness. Discriminating: FAILS pre-fix (callback body
    // dropped at `_ => {}`).
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import { ref } from 'vue'
const items = ref<number[]>([])
const Date = ref(0)
</script>

<template>
  <div v-for="x in items.value.map(() => Date.value)" :key="x">{{ x }}</div>
</template>"#,
    );
    assert_kept_live(&code, "items");
    assert_kept_live(&code, "Date");
}

#[test]
fn vslot_default_nested_callback_reference_keeps_binding_live() {
    // `fmt` is referenced ONLY inside a callback in a v-slot default-value
    // expression (`#default="{ row = list.map(r => fmt(r)) }"`). The v-slot
    // default-value liveness routed through the same partial walker, dropping the
    // arrow-function body. The complete Visit collector descends into it.
    // Discriminating: FAILS pre-fix.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import Table from './Table.vue'
const list = [1, 2]
const fmt = (n: number) => n + 1
</script>

<template>
  <Table>
    <template #default="{ row = list.map(r => fmt(r)) }">{{ row }}</template>
  </Table>
</template>"#,
    );
    assert_kept_live(&code, "list");
    assert_kept_live(&code, "fmt");
}

#[test]
fn template_interpolation_nested_statement_reference_keeps_binding_live() {
    // `dep` is referenced ONLY inside a `switch` statement in an IIFE body in a
    // template interpolation. The template-main-expression `BindingVisitor`
    // `visit_statement` dropped `SwitchStatement` at its `_ => {}` arm. Routing
    // the template-main liveness through the complete Visit collector descends
    // into every statement kind. Discriminating: FAILS pre-fix.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const dep = 1
</script>

<template>
  <div>{{ (() => { switch (dep) { case 1: return 'a'; default: return 'b' } })() }}</div>
</template>"#,
    );
    assert_kept_live(&code, "dep");
}

#[test]
fn style_v_bind_assignment_lhs_keeps_binding_live() {
    // `width` is referenced ONLY as the assignment LHS of a comma-sequence in a
    // style `v-bind()` (`v-bind((width = 4, width))`). Codex-2 noted the LHS
    // keep-live was covered only by a unit test; this pins it as a DEMOTION-PATH
    // E2E so a regression in the sound style scan surfaces at the integration
    // layer. The complete Visit collector records the assignment-target root.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
let width = 4
</script>

<template>
  <div>hello</div>
</template>

<style>
.x { width: v-bind((width = 4, width)); }
</style>"#,
    );
    assert_kept_live(&code, "width");
}

#[test]
fn genuinely_unused_binding_with_nested_callbacks_present_is_still_omitted() {
    // The structural fix must not blunt the diagnostic: a genuinely-unused binding
    // (`dead`) must STILL be omitted even when OTHER bindings are used through
    // nested callbacks. The v-for/v-slot callbacks reference `rows`/`fmt`, never
    // `dead`. Pins that routing through the complete collector did not turn every
    // binding live (over-suppression). Discriminating: FAILS if `dead` is kept.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const rows = [1, 2]
const fmt = (n: number) => n + 1
const dead = 99
</script>

<template>
  <div v-for="x in rows.map(r => fmt(r))" :key="x">{{ x }}</div>
</template>"#,
    );
    assert_kept_live(&code, "rows");
    assert_kept_live(&code, "fmt");
    assert_omitted(&code, "dead");
}

// ── v-slot TYPE-annotation liveness ──────────────────────────────────────────
//
// A setup VALUE binding referenced ONLY via a `typeof` query buried in a nested
// TS type position of a v-slot destructure annotation must be counted as USED.
// v-slot type-annotation liveness routes through the complete `Visit`-over-`TSType`
// collector (`collect_type_free_ref_names`), whose default `walk::*` traversal
// reaches every nested type position — function/constructor/method-signature/
// call/index/construct-signature parameters, mapped-type constraints, infer,
// import, template-literal, predicate, qualified-name roots. The retired
// hand-rolled type walker had `_ => {}` arms skipping those subtrees (it followed
// only a function type's `return_type`, a method signature's `return_type`, and a
// mapped type's value), so a `typeof Helper` in a function-type PARAM was never
// reached and `Helper` was demoted to a false TS6133. Each test is discriminating:
// it FAILS pre-fix.

#[test]
fn vslot_typeof_in_function_type_param_keeps_binding_live() {
    // THE headline vector: `Helper` is a setup VALUE binding used NOWHERE in the
    // script and ONLY via `typeof Helper` inside the function-type PARAM of a
    // v-slot destructure annotation (`(x: typeof Helper) => void`). The partial
    // type walker visited only the function type's return, so `Helper` was missed
    // and falsely demoted. Discriminating: FAILS pre-fix.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import Table from './Table.vue'
const Helper = makeHelper()
</script>

<template>
  <Table>
    <template #default="{ cb }: { cb: (x: typeof Helper) => void }">{{ cb }}</template>
  </Table>
</template>"#,
    );
    assert_kept_live(&code, "Helper");
}

#[test]
fn vslot_typeof_in_method_signature_param_keeps_binding_live() {
    // `Helper` is used ONLY via `typeof Helper` in a METHOD-SIGNATURE param of a
    // v-slot annotation. The partial walker visited only the method signature's
    // return. Discriminating: FAILS pre-fix.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import Table from './Table.vue'
const Helper = makeHelper()
</script>

<template>
  <Table>
    <template #default="{ api }: { api: { run(x: typeof Helper): void } }">{{ api }}</template>
  </Table>
</template>"#,
    );
    assert_kept_live(&code, "Helper");
}

#[test]
fn vslot_typeof_in_call_signature_param_keeps_binding_live() {
    // `Helper` is used ONLY via `typeof Helper` in a CALL-SIGNATURE param. A call
    // signature was entirely behind the partial walker's `_ => {}` arm.
    // Discriminating: FAILS pre-fix.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import Table from './Table.vue'
const Helper = makeHelper()
</script>

<template>
  <Table>
    <template #default="{ cb }: { cb: { (x: typeof Helper): void } }">{{ cb }}</template>
  </Table>
</template>"#,
    );
    assert_kept_live(&code, "Helper");
}

#[test]
fn vslot_typeof_in_index_signature_keeps_binding_live() {
    // `Helper` is used ONLY via `typeof Helper` in an INDEX-SIGNATURE value type.
    // An index signature was behind the partial walker's `_ => {}` arm.
    // Discriminating: FAILS pre-fix.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import Table from './Table.vue'
const Helper = makeHelper()
</script>

<template>
  <Table>
    <template #default="{ map }: { map: { [k: string]: typeof Helper } }">{{ map }}</template>
  </Table>
</template>"#,
    );
    assert_kept_live(&code, "Helper");
}

#[test]
fn vslot_typeof_in_construct_signature_param_keeps_binding_live() {
    // `Helper` is used ONLY via `typeof Helper` in a CONSTRUCT-SIGNATURE param. A
    // construct signature was behind the partial walker's `_ => {}` arm.
    // Discriminating: FAILS pre-fix.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import Table from './Table.vue'
const Helper = makeHelper()
</script>

<template>
  <Table>
    <template #default="{ ctor }: { ctor: { new (x: typeof Helper): Foo } }">{{ ctor }}</template>
  </Table>
</template>"#,
    );
    assert_kept_live(&code, "Helper");
}

#[test]
fn vslot_typeof_in_mapped_type_constraint_keeps_binding_live() {
    // `Helper` is used ONLY via `typeof Helper` in a MAPPED-TYPE constraint
    // (`{ [K in keyof typeof Helper]: V }`). The partial walker followed only the
    // mapped value, dropping the constraint. Discriminating: FAILS pre-fix.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import Table from './Table.vue'
const Helper = makeHelper()
</script>

<template>
  <Table>
    <template #default="{ rec }: { rec: { [K in keyof typeof Helper]: string } }">{{ rec }}</template>
  </Table>
</template>"#,
    );
    assert_kept_live(&code, "Helper");
}

#[test]
fn vslot_typeof_in_nested_function_type_return_param_keeps_binding_live() {
    // `Helper` is used ONLY via `typeof Helper` in the PARAM of a function type
    // that is itself the RETURN of an outer function type
    // (`() => (x: typeof Helper) => void`). The partial walker recursed return
    // types but never params, missing the inner param. Discriminating: FAILS
    // pre-fix.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import Table from './Table.vue'
const Helper = makeHelper()
</script>

<template>
  <Table>
    <template #default="{ make }: { make: () => (x: typeof Helper) => void }">{{ make }}</template>
  </Table>
</template>"#,
    );
    assert_kept_live(&code, "Helper");
}

#[test]
fn vslot_genuinely_unused_with_typeof_user_in_type_param_still_omitted() {
    // The fix must not blunt the diagnostic: a genuinely-unused binding (`dead`)
    // must STILL be omitted even when ANOTHER binding (`Helper`) is used only via
    // a `typeof` in a v-slot function-type param. Pins that routing the type
    // domain through the complete collector did not over-suppress. Discriminating:
    // FAILS if `dead` is kept live.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
import Table from './Table.vue'
const Helper = makeHelper()
const dead = 99
</script>

<template>
  <Table>
    <template #default="{ cb }: { cb: (x: typeof Helper) => void }">{{ cb }}</template>
  </Table>
</template>"#,
    );
    assert_kept_live(&code, "Helper");
    assert_omitted(&code, "dead");
}

// ── Additional value-position keep-live coverage (already on the complete Visit
//    collector; pinned end-to-end through the demotion path) ──────────────────

#[test]
fn script_try_catch_body_reference_keeps_binding_live() {
    // `dep` is referenced ONLY inside a `try { ... }` body in the script. The
    // complete Visit collector descends into the try block via the default walk.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const dep = 1
function run() { try { console.log(dep) } catch (e) { void e } }
</script>

<template>
  <button @click="run()">go</button>
</template>"#,
    );
    assert_kept_live(&code, "dep");
}

#[test]
fn directive_value_tagged_template_and_optional_chaining_keep_bindings_live() {
    // `tag` (tagged-template tag), `dep` (tagged-template interpolation), and `obj`
    // (optional-chaining root) are each referenced ONLY inside a directive VALUE
    // expression. The complete Visit collector over the parsed template expression
    // records each. Discriminating: a walker dropping tagged-template / optional-
    // chaining roots would demote them.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const tag = (s: TemplateStringsArray) => s
const dep = 1
const obj = { a: 1 }
</script>

<template>
  <div :title="tag`x${dep}` && obj?.a">hi</div>
</template>"#,
    );
    assert_kept_live(&code, "tag");
    assert_kept_live(&code, "dep");
    assert_kept_live(&code, "obj");
}

#[test]
fn vfor_source_array_spread_keeps_bindings_live() {
    // `base` and `extra` are referenced ONLY inside an ARRAY SPREAD in the v-for
    // SOURCE expression (`v-for="x in [...base, extra]"`). The complete Visit
    // collector descends into the array + spread element. Discriminating: a walker
    // dropping spread elements would demote them.
    let (code, _bindings) = gen_tsx_script_unwrap(
        r#"<script setup lang="ts">
const base = [1, 2]
const extra = 3
</script>

<template>
  <div v-for="x in [...base, extra]" :key="x">{{ x }}</div>
</template>"#,
    );
    assert_kept_live(&code, "base");
    assert_kept_live(&code, "extra");
}
