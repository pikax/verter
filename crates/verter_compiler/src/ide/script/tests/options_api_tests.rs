//! Options-API + dual-script tests (D2 cohort).

use super::*;

// ── types_module_name tests ─────────────────────────────────────

#[test]
fn types_module_default_is_verter_types() {
    let (code, _, _) = gen_tsx_script_full(
        r#"<script setup lang="ts">const x = 1</script><template><div/></template>"#,
    );
    assert!(
        code.contains(r#"from "@verter/types""#),
        "default should be @verter/types, got:\n{}",
        code
    );
    assert!(
        !code.contains(r#"from "$verter/types$""#),
        "should NOT use $verter/types$"
    );
}

#[test]
fn types_module_custom_override() {
    let (code, _, _) = gen_tsx_script_full_with_options(
        r#"<script setup lang="ts">const x = 1</script><template><div/></template>"#,
        IdeScriptOptions {
            component_name: "App",
            js_component_name: "App",
            filename: "App.vue",
            scope_id: "data-v-abc123",
            has_scoped_style: false,
            runtime_module_name: "vue",
            types_module_name: "@custom/types",
            is_vapor: false,
            embed_ambient_types: true,
            is_jsx: false,
            conditional_root_narrowing: false,
            style_v_bind_vars: vec![],
            style_usage_complete: true,
            css_modules: vec![],
            template_used_vars: None,
        },
    );
    assert!(
        code.contains(r#"from "@custom/types""#),
        "custom path should be used, got:\n{}",
        code
    );
    assert!(
        !code.contains(r#"from "@verter/types""#),
        "default should be overridden"
    );
}

// ── Options API type constructs tests ────────────────────────────

#[test]
fn options_api_has_type_constructs() {
    let (code, _bindings, type_constructs) = gen_tsx_script_full(
        r#"<script lang="ts">
export default { props: ['msg'], emits: ['click'] }
</script>
<template><div>{{ msg }}</div></template>"#,
    );

    // OXC validation
    let full = format!("{}\n{}", code, type_constructs);
    let val_alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&val_alloc, &full, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "Full TSX must be valid: {:?}\n---\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        full
    );

    // Positive: helper imports
    assert!(
        code.contains(r#"from "@verter/types""#),
        "should import types"
    );
    // Negative: Instance type should no longer be emitted
    assert!(
        !type_constructs.contains("___VERTER___Instance"),
        "Instance type should not be emitted"
    );
    // Negative: no macro helpers (Options API has no macros)
    assert!(
        !code.contains("createMacroReturn"),
        "no macros in Options API"
    );
    // Negative: should not contain raw Vue syntax
    assert!(
        !code.contains("<script"),
        "script tags should be removed from output"
    );
}

#[test]
fn options_api_with_template_has_comp_functions() {
    let (_code, _, tc) = gen_tsx_script_full(
        r#"<script>export default { data() { return { x: 1 } } }</script>
<template><div><span>inner</span></div></template>"#,
    );
    // Instance type should no longer be emitted
    assert!(
        !tc.contains("___VERTER___Instance"),
        "should not emit Instance type, got:\n{}",
        tc
    );
    assert!(
        !tc.contains("___VERTER___Component"),
        "Component export should not be emitted"
    );
}

#[test]
fn options_api_template_only_parity() {
    // Options API should emit the same type constructs structure as template-only
    let (opt_code, _, opt_tc) = gen_tsx_script_full(
        r#"<script>export default {}</script>
<template><div>hello</div></template>"#,
    );
    let (tpl_code, _, tpl_tc) = gen_tsx_script_full(r#"<template><div>hello</div></template>"#);

    // Both should have helper imports
    assert!(
        opt_code.contains(r#"from "@verter/types""#),
        "Options API should have types imports"
    );
    assert!(
        tpl_code.contains(r#"from "@verter/types""#),
        "template-only should have types imports"
    );

    // Neither should have Instance type (removed)
    assert!(
        !opt_tc.contains("___VERTER___Instance"),
        "Options API should not have Instance"
    );
    assert!(
        !tpl_tc.contains("___VERTER___Instance"),
        "template-only should not have Instance"
    );
}

// ── Companion script processing (WS 2.7) ────────────────────

#[test]
fn companion_script_tags_removed_from_output() {
    let (code, _) = gen_tsx_script(
        r#"<script lang="ts">
export default {
  inheritAttrs: false,
};
</script>
<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
    );

    // Companion <script> tags must NOT appear in TSX output
    assert!(
        !code.contains("<script lang=\"ts\">"),
        "companion <script> open tag must be removed from output: {code}"
    );
    assert!(
        !code.contains("</script>"),
        "companion </script> close tag must be removed from output: {code}"
    );
    // Setup content should still be present
    assert!(
        code.contains("const msg = 'hello'"),
        "setup content should remain in output: {code}"
    );
}

#[test]
fn companion_script_imports_hoisted() {
    let (code, bindings) = gen_tsx_script(
        r#"<script lang="ts">
import MyComponent from './MyComponent.vue'
export default {
  components: { MyComponent },
};
</script>
<script setup lang="ts">
const count = ref(0)
</script>
<template><MyComponent/></template>"#,
    );

    // Companion imports should be hoisted above the wrapper function
    assert!(
        code.contains("import MyComponent from './MyComponent.vue.ts'"),
        "companion import should be hoisted with .vue.ts rewrite: {code}"
    );

    // Import should appear before the wrapper function
    let import_pos = code
        .find("import MyComponent")
        .expect("import should exist");
    let wrapper_pos = code
        .find("TemplateBindingFN")
        .expect("wrapper fn should exist");
    assert!(
        import_pos < wrapper_pos,
        "companion import should be hoisted before wrapper function"
    );

    // Companion import binding should be in bindings map
    assert!(
        bindings.contains_key("MyComponent"),
        "companion import binding should be tracked: {bindings:?}"
    );
}

#[test]
fn companion_script_export_default_removed() {
    let (code, _) = gen_tsx_script(
        r#"<script lang="ts">
export default {
  inheritAttrs: false,
  name: 'MyComp',
};
</script>
<script setup lang="ts">
const msg = 'hello'
</script>
<template><div/></template>"#,
    );

    // export default from companion should be removed (runtime-only, not needed for type checking)
    assert!(
        !code.contains("export default"),
        "companion export default should be removed: {code}"
    );
    assert!(
        !code.contains("inheritAttrs"),
        "companion options should not appear in TSX output: {code}"
    );
}

#[test]
fn companion_script_type_declarations_hoisted() {
    let (code, _) = gen_tsx_script(
        r#"<script lang="ts">
interface CompanionType {
  name: string
}
export default {};
</script>
<script setup lang="ts">
const item: CompanionType = { name: 'test' }
</script>
<template><div/></template>"#,
    );

    // Type declarations from companion should be hoisted
    assert!(
        code.contains("interface CompanionType"),
        "companion type declaration should be hoisted: {code}"
    );

    // Should appear before the wrapper function
    let type_pos = code
        .find("interface CompanionType")
        .expect("type decl should exist");
    let wrapper_pos = code
        .find("TemplateBindingFN")
        .expect("wrapper fn should exist");
    assert!(
        type_pos < wrapper_pos,
        "companion type declaration should be hoisted before wrapper function"
    );
}

#[test]
fn companion_script_value_declarations_available() {
    let (code, bindings) = gen_tsx_script(
        r#"<script lang="ts">
import { computed } from 'vue'
const doubled = computed(() => count.value * 2)
export default {};
</script>
<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div/></template>"#,
    );

    // Both setup and companion imports should be present
    assert!(
        code.contains("import { ref } from 'vue'"),
        "setup import should be present: {code}"
    );
    assert!(
        code.contains("import { computed } from 'vue'"),
        "companion import should be hoisted: {code}"
    );

    // Setup bindings should still work
    assert!(
        bindings.contains_key("count"),
        "setup binding should be tracked: {bindings:?}"
    );
}

// ── Dual-script JS SFC (is_jsx: true + companion script) ────────

#[test]
fn jsx_dual_script_companion_export_default() {
    let (code, _type_constructs) = gen_jsx_script(
        r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
<script>
export default {
  inheritAttrs: false,
}
</script>
<template><div>{{ count }}</div></template>"#,
    );

    // Positive: setup content should be present
    assert!(
        code.contains("const count = ref(0)"),
        "setup content should be present:\n{code}"
    );
    assert!(
        code.contains("TemplateBindingFN"),
        "wrapper function should exist:\n{code}"
    );
    assert!(
        code.contains("import { ref } from 'vue'"),
        "setup import should be present:\n{code}"
    );

    // Negative: script tags and export default must be removed
    assert!(
        !code.contains("<script"),
        "script tags must be removed:\n{code}"
    );
    assert!(
        !code.contains("</script>"),
        "close script tags must be removed:\n{code}"
    );
    assert!(
        !code.contains("export default"),
        "companion export default should be removed:\n{code}"
    );
    assert!(
        !code.contains("inheritAttrs"),
        "companion options should not appear:\n{code}"
    );

    // Should parse as valid JSX
    let alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, &code, oxc_span::SourceType::jsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC JSX ERROR: {err}");
    }
    assert!(
        parsed.errors.is_empty(),
        "generated JSX should have no parse errors, got {}:\n{code}",
        parsed.errors.len()
    );
}

#[test]
fn jsx_dual_script_companion_imports_hoisted() {
    let (code, _type_constructs) = gen_jsx_script(
        r#"<script>
import MyComponent from './MyComponent.vue'
export default {
  components: { MyComponent },
}
</script>
<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
<template><MyComponent/></template>"#,
    );

    // Companion imports should be hoisted
    assert!(
        code.contains("import MyComponent from './MyComponent.vue.ts'"),
        "companion import should be hoisted with .vue.ts rewrite:\n{code}"
    );

    // Import should appear before the wrapper function
    let import_pos = code
        .find("import MyComponent")
        .expect("import should exist");
    let wrapper_pos = code
        .find("TemplateBindingFN")
        .expect("wrapper fn should exist");
    assert!(
        import_pos < wrapper_pos,
        "companion import should be hoisted before wrapper function"
    );

    // Should parse as valid JSX
    let alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, &code, oxc_span::SourceType::jsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "generated JSX should have no parse errors, got {}:\n{code}",
        parsed.errors.len()
    );
}

#[test]
fn jsx_dual_script_companion_value_declarations() {
    let (code, _type_constructs) = gen_jsx_script(
        r#"<script>
const BASE_URL = 'https://example.com'
export default {}
</script>
<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
    );

    // Companion value declarations should be present outside wrapper
    assert!(
        code.contains("const BASE_URL = 'https://example.com'"),
        "companion value declaration should remain:\n{code}"
    );

    // export default should be removed
    assert!(
        !code.contains("export default"),
        "companion export default should be removed:\n{code}"
    );

    // Should parse as valid JSX
    let alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, &code, oxc_span::SourceType::jsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "generated JSX should have no parse errors, got {}:\n{code}",
        parsed.errors.len()
    );
}

#[test]
fn jsx_dual_script_no_export_default() {
    let (code, _type_constructs) = gen_jsx_script(
        r#"<script>
const SHARED = 42
</script>
<script setup>
import { ref } from 'vue'
const count = ref(SHARED)
</script>
<template><div>{{ count }}</div></template>"#,
    );

    // Companion content should be present
    assert!(
        code.contains("const SHARED = 42"),
        "companion value should remain:\n{code}"
    );

    // Should parse as valid JSX
    let alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, &code, oxc_span::SourceType::jsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "generated JSX should have no parse errors, got {}:\n{code}",
        parsed.errors.len()
    );
}

#[test]
fn jsx_dual_script_template_first() {
    let (code, _type_constructs) = gen_jsx_script(
        r#"<template><div>{{ count }}</div></template>
<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
<script>
export default {
  inheritAttrs: false,
}
</script>"#,
    );

    // Should still work with template-first ordering
    assert!(
        code.contains("const count = ref(0)"),
        "setup content should be present:\n{code}"
    );
    assert!(
        !code.contains("<script"),
        "script tags must be removed:\n{code}"
    );
    assert!(
        !code.contains("export default"),
        "companion export default should be removed:\n{code}"
    );

    // Should parse as valid JSX
    let alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, &code, oxc_span::SourceType::jsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "generated JSX should have no parse errors, got {}:\n{code}",
        parsed.errors.len()
    );
}

/// Test with actual vuetify-like pattern: template-first with defineProps
#[test]
fn jsx_dual_script_vuetify_figure_pattern() {
    let (code, _type_constructs) = gen_jsx_script(
        r#"<template>
  <figure>
<figcaption v-if="caption" v-text="caption" />
<slot v-else />
  </figure>
</template>

<script setup>
  import { computed, useAttrs } from 'vue'

  const attrs = useAttrs()

  defineProps({
name: String,
  })

  const caption = computed(() => attrs.title === 'null' ? null : attrs.title)
</script>

<script>
  export default {
inheritAttrs: false,
  }
</script>"#,
    );

    // Positive assertions
    assert!(
        code.contains("const caption = computed("),
        "setup computed should be present:\n{code}"
    );
    assert!(
        code.contains("TemplateBindingFN"),
        "wrapper function should exist:\n{code}"
    );

    // Negative assertions
    assert!(
        !code.contains("<script"),
        "script tags must be removed:\n{code}"
    );
    assert!(
        !code.contains("</script>"),
        "close script tags must be removed:\n{code}"
    );
    assert!(
        !code.contains("export default"),
        "companion export default should be removed:\n{code}"
    );

    // Should parse as valid JSX
    let alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, &code, oxc_span::SourceType::jsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC JSX ERROR: {err}");
    }
    assert!(
        parsed.errors.is_empty(),
        "generated JSX should have no parse errors, got {}:\n{code}",
        parsed.errors.len()
    );
}
