//! Setup-pipeline tests (D1 cohort).

use super::*;

/// IDE output rewrites `.vue` import specifiers to `.vue.ts` so that
/// type providers (TSGO/tsserver) resolve them to the public API output.
/// The rewrite uses CodeTransform::prepend_left so the sourcemap stays correct.
#[test]
fn standalone_and_ambient_types_preserve_slot_argument_maps() {
    let slot_signature =
        "TSlots extends Record<string, any>,\n    N extends keyof TSlots & string,";

    assert!(
        VERTER_TYPES_AMBIENT_MODULE.contains(slot_signature),
        "ambient @verter/types declarations should infer the concrete slot map first"
    );
    assert!(
        VERTER_TYPES_AMBIENT_MODULE
            .contains("): TSlots[N] extends (...args: infer P) => any ? P[0] : never;"),
        "ambient @verter/types declarations should preserve slot prop types"
    );
    assert!(
        VERTER_TYPES_STANDALONE_DTS
            .contains("TSlots extends Record<string, any>,\n  N extends keyof TSlots & string,"),
        "standalone @verter/types stub should infer the concrete slot map first"
    );
    assert!(
        VERTER_TYPES_STANDALONE_DTS
            .contains("): TSlots[N] extends (...args: infer P) => any ? P[0] : never;"),
        "standalone @verter/types stub should preserve slot prop types"
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
