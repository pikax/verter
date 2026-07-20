//! Tests ported from `syntax/plugins/code_gen/script/mod.rs`.
//!
//! These tests exercise the new AST-based pipeline via the public `compile()` API.
//! Each test calls `compile()` directly and checks the appropriate result block
//! (script, template, or diagnostics).

use crate::compile::{compile, CodegenOptions, VerterCompileOptions};
use oxc_allocator::Allocator;
use rustc_hash::FxHashMap;

fn make_external_types(
    type_name: &str,
    dep_source: &str,
) -> FxHashMap<String, crate::utils::oxc::script::type_surface::ResolvedElements> {
    let alloc = Allocator::default();
    let resolved = crate::utils::oxc::script::type_surface::resolve_external_type(
        type_name, dep_source, &alloc,
    )
    .expect("failed to resolve external type");
    let mut map = FxHashMap::default();
    map.insert(type_name.to_string(), resolved);
    map
}

// =========================================================================
// Multi-script: <script setup> + <script> is valid Vue
// ported from syntax/plugins/code_gen/script/mod.rs
// =========================================================================

#[test]
fn test_multi_script_does_not_panic() {
    let input = r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
<script>
export default { name: 'MyComponent' }
</script>
<template><div>{{ count }}</div></template>"#;

    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let has_code = result.script.as_ref().is_some_and(|s| !s.code.is_empty());
    assert!(
        has_code || !result.errors.is_empty(),
        "Multi-script SFC should compile or report errors, not panic"
    );
}

#[test]
fn test_duplicate_script_setup_reports_error() {
    let input = r#"<script setup>
const a = 1
</script>
<script setup>
const b = 2
</script>
<template><div>test</div></template>"#;

    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    assert!(
        !result.errors.is_empty(),
        "Duplicate <script setup> should produce error diagnostics, got none"
    );
}

// =========================================================================
// Basic Script Wrapping
// =========================================================================

#[test]
fn test_script_setup_basic_dev() {
    let input =
        "<script setup>\nconst msg = 'Hello'\n</script>\n<template><div>hi</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("const __sfc__ = {"),
        "Should have plain const __sfc__ object (JS is not wrapped), got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("export default __sfc__"),
        "Should export __sfc__ at the end, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("__name: 'test'"),
        "Should have component name from filename, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("setup(__props"),
        "Dev mode should have setup function, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("const __returned__ = {"),
        "Dev mode (non-inline) should have __returned__ statement, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("__isScriptSetup"),
        "Dev mode should have __isScriptSetup marker, got:\n{}",
        script.code
    );
}

#[test]
fn test_script_setup_prod_template() {
    // Official production default is INLINE: the render is merged into
    // setup() as a returned closure — no separate template block.
    let input =
        "<script setup>\nconst msg = 'Hello'\n</script>\n<template><div>hi</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new()
        .with_filename("test.vue")
        .with_production(true);
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    assert!(
        result.template.is_none(),
        "production inlines the render into setup — no template block"
    );
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("return (_ctx,_cache) => {"),
        "production inlines the render as a setup-returned closure, got:\n{}",
        script.code
    );
}

#[test]
fn test_component_name_from_filename() {
    let input = "<script setup>\nconst x = 1\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("MyComponent.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("__name: 'MyComponent'"),
        "Should extract component name from filename, got:\n{}",
        script.code
    );
}

#[test]
fn test_script_no_setup() {
    let input =
        "<script>\nexport default { name: 'Foo' }\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        !script.code.contains("<script>"),
        "Should strip <script> tag, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("</script>"),
        "Should strip </script> tag, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("const __sfc__ = { name: 'Foo' }"),
        "Should replace export default with const __sfc__, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("export default __sfc__"),
        "Should export __sfc__ at the end, got:\n{}",
        script.code
    );
}

// =========================================================================
// defineProps
// =========================================================================

#[test]
fn test_define_props_object_arg() {
    let input = "<script setup>\nconst props = defineProps({ title: String })\n</script>\n<template><div>{{ props.title }}</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("props:"),
        "Should have props section, got:\n{}",
        script.code
    );
}

#[test]
fn test_define_props_typed_inline() {
    let input = "<script setup lang=\"ts\">\ndefineProps<{ title: string }>()\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("props:"),
        "Should have props section, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("title:"),
        "Should have title prop, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("String"),
        "Should resolve string to String, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("defineProps<"),
        "Should NOT leave defineProps as-is, got:\n{}",
        script.code
    );
}

#[test]
fn test_define_props_typed_optional() {
    let input = "<script setup lang=\"ts\">\ndefineProps<{ count?: number }>()\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("count:"),
        "Should have count prop, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("Number"),
        "Should resolve number to Number, got:\n{}",
        script.code
    );
}

#[test]
fn test_define_props_string_literal_union() {
    let input = "<script setup lang=\"ts\">\ndefineProps<{ view?: 'list' | 'board' | 'calendar' }>()\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("String"),
        "String literal union should resolve to String, got:\n{}",
        script.code
    );
}

#[test]
fn test_define_props_inline_no_stale_delimiters() {
    let input = "<script setup lang=\"ts\">\ndefineProps<{ title: string }>()\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        !script.code.contains("defineProps<"),
        "Should not leave defineProps< in output, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains(">()"),
        "Should not leave >() in output, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("props:"),
        "Should have props section, got:\n{}",
        script.code
    );
}

#[test]
fn test_define_props_interface_ref() {
    let input = "<script setup lang=\"ts\">\ninterface Props { title: string; count?: number }\ndefineProps<Props>()\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("props:"),
        "Should have props section, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("title:"),
        "Should resolve interface prop 'title', got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("count:"),
        "Should resolve interface prop 'count', got:\n{}",
        script.code
    );
}

#[test]
fn test_define_props_type_alias_ref() {
    let input = "<script setup lang=\"ts\">\ntype MyProps = { message: string }\ndefineProps<MyProps>()\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("props:"),
        "Should have props section, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("message:"),
        "Should resolve type alias prop 'message', got:\n{}",
        script.code
    );
}

#[test]
fn test_define_props_unresolvable_type() {
    // When the type reference is unresolvable (imported from external module),
    // the new pipeline strips the defineProps call (it's a type-only macro)
    // and does not emit a props section since the type can't be resolved.
    let input = "<script setup lang=\"ts\">\nimport type { ExternalProps } from './types'\ndefineProps<ExternalProps>()\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        !script.code.contains("defineProps<"),
        "Should NOT leave defineProps<> as-is, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("ExternalProps"),
        "Should strip the type reference, got:\n{}",
        script.code
    );
}

#[test]
fn test_define_props_empty_type_literal() {
    // Empty type literal defineProps<{}>() has no props to emit,
    // so the new pipeline strips the macro call without generating a props section.
    let input =
        "<script setup lang=\"ts\">\ndefineProps<{}>()\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        !script.code.contains("defineProps"),
        "Should strip defineProps call, got:\n{}",
        script.code
    );
}

#[test]
fn test_define_props_unresolvable_type_reports_error() {
    let input = "<script setup lang=\"ts\">\nimport type { ExternalProps } from './types'\ndefineProps<ExternalProps>()\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    // An UNRESOLVABLE imported type surfaces the resolution-failure code
    // `XUnresolvedImportedMacroType` (distinct from the `XInvalidMacroType`
    // a resolved-but-wrong-shape type emits — only the former is softened on
    // the render-only bundler lane).
    assert!(
        result
            .errors
            .iter()
            .any(|error| error.code == "XUnresolvedImportedMacroType"),
        "unresolvable imported props type should surface a compiler error, got: {:?}",
        result.errors
    );
}

#[test]
fn test_define_emits_invalid_imported_type_reports_error() {
    let input = "<script setup lang=\"ts\">\nimport type { ExternalEmits } from './types'\ndefineEmits<ExternalEmits>()\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        external_types: Some(make_external_types(
            "ExternalEmits",
            "export type ExternalEmits = string",
        )),
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);

    assert!(
        result
            .errors
            .iter()
            .any(|error| error.code == "XInvalidMacroType"),
        "invalid imported emits type should surface a compiler error, got: {:?}",
        result.errors
    );
    assert!(
        result.errors.iter().any(|error| error
            .message
            .contains("defineEmits() type argument 'ExternalEmits'")),
        "diagnostic should mention the invalid defineEmits import, got: {:?}",
        result.errors
    );
}

#[test]
fn test_define_props_invalid_imported_type_reports_error() {
    let input = "<script setup lang=\"ts\">\nimport type { ExternalProps } from './types'\ndefineProps<ExternalProps>()\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        external_types: Some(make_external_types(
            "ExternalProps",
            "export type ExternalProps = string",
        )),
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);

    assert!(
        result
            .errors
            .iter()
            .any(|error| error.code == "XInvalidMacroType"),
        "invalid imported props type should surface a compiler error, got: {:?}",
        result.errors
    );
    assert!(
        result.errors.iter().any(|error| error
            .message
            .contains("defineProps() type argument 'ExternalProps'")),
        "diagnostic should mention the invalid defineProps import, got: {:?}",
        result.errors
    );
}

#[test]
fn test_define_props_replaced_with_props() {
    let input = "<script setup>\nconst props = defineProps({ title: String })\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("__props"),
        "defineProps should be replaced with __props reference, got:\n{}",
        script.code
    );
}

// =========================================================================
// withDefaults
// =========================================================================

#[test]
fn test_with_defaults_typed_inline() {
    let input = "<script setup lang=\"ts\">\nconst props = withDefaults(defineProps<{ foo?: string }>(), { foo: 'bar' })\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("props:"),
        "Should have props section, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("default:"),
        "Should have default value, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("withDefaults("),
        "Should NOT leave withDefaults as-is, got:\n{}",
        script.code
    );
}

#[test]
fn test_with_defaults_interface_ref() {
    let input = "<script setup lang=\"ts\">\ninterface Props { foo?: string; bar?: number }\nconst props = withDefaults(defineProps<Props>(), { foo: 'hello' })\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("props:"),
        "Should have props, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("foo:"),
        "Should have foo prop, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("bar:"),
        "Should have bar prop, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("default:"),
        "Should have default for foo, got:\n{}",
        script.code
    );
}

// =========================================================================
// export interface / export type with defineProps
// =========================================================================

#[test]
fn test_define_props_export_interface_resolves() {
    let input = "<script setup lang=\"ts\">\nexport interface Props {\n  foo: string\n  bar?: number\n}\nconst props = defineProps<Props>()\n</script>\n<template><div>{{ props.foo }}</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("props:"),
        "Should have props section, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("foo:"),
        "Should have foo prop, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("bar:"),
        "Should have bar prop, got:\n{}",
        script.code
    );

    assert!(
        !result
            .errors
            .iter()
            .any(|e| e.message.contains("Unresolvable type reference")),
        "Should NOT emit 'Unresolvable type reference' for locally exported interface, got errors: {:?}",
        result.errors
    );
}

#[test]
fn test_define_props_export_type_alias_resolves() {
    let input = "<script setup lang=\"ts\">\nexport type Props = {\n  bar: number\n}\ndefineProps<Props>()\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("props:"),
        "Should have props section, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("bar:"),
        "Should have bar prop, got:\n{}",
        script.code
    );

    assert!(
        !result
            .errors
            .iter()
            .any(|e| e.message.contains("Unresolvable type reference")),
        "Should NOT emit 'Unresolvable type reference' for locally exported type alias, got errors: {:?}",
        result.errors
    );
}

#[test]
fn test_with_defaults_export_interface() {
    let input = "<script setup lang=\"ts\">\nexport interface Props {\n  size?: number\n  color?: string\n}\nconst props = withDefaults(defineProps<Props>(), {\n  size: 16,\n  color: 'red',\n})\n</script>\n<template><div>{{ props.size }}</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("props:"),
        "Should have props section, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("size:"),
        "Should have size prop, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("color:"),
        "Should have color prop, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("default:"),
        "Should have default values, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("withDefaults("),
        "Should NOT leave withDefaults as-is, got:\n{}",
        script.code
    );
}

// =========================================================================
// defineEmits
// =========================================================================

#[test]
fn test_define_emits_array() {
    let input = "<script setup>\nconst emit = defineEmits(['click', 'update'])\n</script>\n<template><div @click=\"emit('click')\">x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("emits:"),
        "Should have emits section, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("emit: __emit"),
        "Should have emit in setup signature, got:\n{}",
        script.code
    );
}

#[test]
fn test_define_emits_no_declarator() {
    // In the new pipeline, defineEmits always adds emit:__emit to the setup
    // signature regardless of whether there's a variable declarator.
    let input =
        "<script setup>\ndefineEmits(['click'])\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("emits:"),
        "Should have emits section, got:\n{}",
        script.code
    );
}

// =========================================================================
// defineModel
// =========================================================================

#[test]
fn test_define_model_basic() {
    let input =
        "<script setup>\nconst model = defineModel()\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("_useModel"),
        "defineModel should produce _useModel call, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("modelValue"),
        "Default model name should be modelValue, got:\n{}",
        script.code
    );
}

#[test]
fn test_define_model_named() {
    let input = "<script setup>\nconst count = defineModel('count')\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("\"count\"") || script.code.contains("'count'"),
        "Named model should use provided name, got:\n{}",
        script.code
    );
}

#[test]
fn test_define_model_with_props_merge() {
    // In the new pipeline, defineModel + defineProps coexist without _mergeModels.
    // defineProps handles its own props section and defineModel generates _useModel.
    let input = "<script setup>\nconst props = defineProps({ title: String })\nconst model = defineModel()\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("props:"),
        "Should have props section from defineProps, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("_useModel"),
        "Should have _useModel from defineModel, got:\n{}",
        script.code
    );
}

// =========================================================================
// defineExpose
// =========================================================================

#[test]
fn test_define_expose() {
    let input = "<script setup>\nconst publicFn = () => {}\ndefineExpose({ publicFn })\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("__expose("),
        "defineExpose should be replaced with __expose, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("expose: __expose"),
        "Should have expose in setup signature, got:\n{}",
        script.code
    );
}

// =========================================================================
// defineOptions
// =========================================================================

#[test]
fn test_define_options() {
    let input = "<script setup>\ndefineOptions({ inheritAttrs: false })\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("inheritAttrs: false"),
        "defineOptions object should be in output, got:\n{}",
        script.code
    );
}

#[test]
fn test_define_options_with_name_and_inherit_attrs() {
    let input = r#"<script setup lang="ts">
import type { LabelEmits, LabelProps } from './Label.ts'
import { Primitive } from '../primitive/index.ts'

defineOptions({
  name: 'RadixLabel',
  inheritAttrs: false,
})

withDefaults(defineProps<LabelProps>(), {})
const emit = defineEmits<LabelEmits>()
</script>

<template>
  <Primitive>
    <slot />
  </Primitive>
</template>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        !script.code.contains("defineOptions("),
        "defineOptions should be stripped from output, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("inheritAttrs: false"),
        "defineOptions object should be merged into component definition, got:\n{}",
        script.code
    );
}

// =========================================================================
// defineSlots
// =========================================================================

#[test]
fn test_define_slots() {
    let input =
        "<script setup>\nconst slots = defineSlots()\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("_useSlots"),
        "defineSlots should be replaced with _useSlots, got:\n{}",
        script.code
    );
}

// =========================================================================
// TypeScript vs JavaScript Wrapping
// =========================================================================

#[test]
fn test_ts_uses_define_component() {
    let input =
        "<script setup lang=\"ts\">\nconst x = 1\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("_defineComponent("),
        "TS script setup should use _defineComponent, got:\n{}",
        script.code
    );
}

#[test]
fn test_js_uses_plain_object_without_define_component() {
    // Official @vue/compiler-sfc non-inline: JS (no `lang="ts"`) script setup
    // emits a PLAIN component object — no `_defineComponent` call or import.
    // Only TS components are wrapped (see test_ts_uses_define_component).
    let input = "<script setup>\nconst x = 1\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("const __sfc__ = {"),
        "JS script setup should emit a plain object, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("_defineComponent"),
        "JS script setup must not reference _defineComponent, got:\n{}",
        script.code
    );
}

// =========================================================================
// Script Items
// =========================================================================

#[test]
fn test_import_hoisted() {
    let input = "<script setup>\nimport { ref } from 'vue'\nconst count = ref(0)\n</script>\n<template><div>{{ count }}</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    let import_pos = script.code.find("import { ref }");
    let export_pos = script.code.find("export default");
    assert!(
        import_pos.is_some() && export_pos.is_some(),
        "Should have both import and export, got:\n{}",
        script.code
    );
    assert!(
        import_pos.unwrap() < export_pos.unwrap(),
        "Import should appear before export default, got:\n{}",
        script.code
    );
}

#[test]
fn test_type_import_stripped() {
    let input = "<script setup lang=\"ts\">\nimport type { Ref } from 'vue'\nconst x = 1\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        !script.code.contains("import type"),
        "Type-only import should be stripped, got:\n{}",
        script.code
    );
}

#[test]
fn test_declarations_in_return() {
    let input = "<script setup>\nconst count = 0\nfunction increment() {}\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("count") && script.code.contains("increment"),
        "Both declarations should be in output, got:\n{}",
        script.code
    );
}

// =========================================================================
// Edge Cases
// =========================================================================

#[test]
fn test_script_setup_empty() {
    let input = "<script setup>\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("const __sfc__"),
        "Empty setup should still produce __sfc__ scaffolding, got:\n{}",
        script.code
    );
}

#[test]
fn test_script_setup_comment_only() {
    let input = "<script setup>\n// just a comment\n</script>\n<template><div>x</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("setup(__props"),
        "Comment-only setup should still produce setup function, got:\n{}",
        script.code
    );
}

#[test]
fn test_prod_return_bindings() {
    let input =
        "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new()
        .with_filename("test.vue")
        .with_production(true);
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("msg"),
        "Production mode should include bindings, got:\n{}",
        script.code
    );
}

// =========================================================================
// Import bindings in return object
// =========================================================================

#[test]
fn test_imported_component_in_returned() {
    let input = r#"<script setup>
import MyComp from './MyComp.vue'
</script>
<template><MyComp/></template>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("const __returned__ = {"),
        "Should have __returned__ statement, got:\n{}",
        script.code
    );
    let return_pos = script
        .code
        .find("const __returned__ = {")
        .map(|p| p + "const __returned__ = ".len())
        .or_else(|| script.code.find("return{"))
        .expect("Should have return");
    let return_end = script.code[return_pos..].find('}').unwrap() + return_pos;
    let return_section = &script.code[return_pos..=return_end];
    assert!(
        return_section.contains("MyComp"),
        "MyComp should be inside return {{...}}, got section: {}",
        return_section
    );
}

#[test]
fn test_named_import_in_returned() {
    let input = r#"<script setup>
import { SOME_CONST } from './constants'
const x = SOME_CONST
</script>
<template><div>{{ x }}</div></template>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    let return_pos = script
        .code
        .find("const __returned__ = {")
        .map(|p| p + "const __returned__ = ".len())
        .or_else(|| script.code.find("return{"))
        .expect("Should have return");
    let return_end = script.code[return_pos..].find('}').unwrap() + return_pos;
    let return_section = &script.code[return_pos..=return_end];
    // SOME_CONST is an import not referenced in the template (only used as
    // `const x = SOME_CONST` in script), so it should NOT be in __returned__.
    // The bundler's tree-shaking handles the unused import.
    assert!(
        !return_section.contains("SOME_CONST"),
        "Import not used in template should NOT be in __returned__, got section: {}",
        return_section
    );
    // x (SetupConst) should still be returned since it's used in the template
    assert!(
        return_section.contains("x"),
        "Local variable x should be in __returned__, got section: {}",
        return_section
    );
}

#[test]
fn test_type_import_not_in_returned() {
    let input = r#"<script setup lang="ts">
import type { Ref } from 'vue'
const x = 1
</script>
<template><div>{{ x }}</div></template>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        !script.code.contains("Ref"),
        "Type-only import should NOT appear in output, got:\n{}",
        script.code
    );
}

#[test]
fn test_props_not_in_returned() {
    let input = r#"<script setup lang="ts">
defineProps<{ store: any }>()
const localVar = 'hello'
</script>
<template><div>{{ localVar }}</div></template>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    let return_pos = script
        .code
        .find("const __returned__ = {")
        .map(|p| p + "const __returned__ = ".len())
        .or_else(|| script.code.find("return{"))
        .expect("Should have return");
    let return_end = script.code[return_pos..].find('}').unwrap() + return_pos;
    let return_section = &script.code[return_pos..=return_end];
    assert!(
        return_section.contains("localVar"),
        "Local var should be in return, got section: {}",
        return_section
    );
    assert!(
        !return_section.contains("store"),
        "Props should NOT be in return, got section: {}",
        return_section
    );
}

#[test]
fn test_mixed_imports_and_declarations_in_returned() {
    let input = r#"<script setup>
import Header from './Header.vue'
import { ref } from 'vue'
const count = ref(0)
function increment() {}
</script>
<template><Header/><div @click="increment">{{ count }}</div></template>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    let return_pos = script
        .code
        .find("const __returned__ = {")
        .map(|p| p + "const __returned__ = ".len())
        .or_else(|| script.code.find("return{"))
        .expect("Should have return");
    let return_end = script.code[return_pos..].find('}').unwrap() + return_pos;
    let return_section = &script.code[return_pos..=return_end];
    assert!(
        return_section.contains("Header"),
        "Imported component Header should be in return (used in template), got: {}",
        return_section
    );
    // `ref` is imported but not referenced in the template, so it should NOT
    // appear in __returned__. The bundler handles the unused import.
    assert!(
        !return_section.contains("ref"),
        "Import `ref` (not used in template) should NOT be in return, got: {}",
        return_section
    );
    assert!(
        return_section.contains("count"),
        "Declaration count should be in return, got: {}",
        return_section
    );
    assert!(
        return_section.contains("increment"),
        "Declaration increment should be in return, got: {}",
        return_section
    );
}

#[test]
fn test_per_specifier_type_import_not_in_returned() {
    let input = r#"<script setup lang="ts">
import { CONST_VAL, type MyType } from './types'
const x = CONST_VAL
</script>
<template><div>{{ x }}</div></template>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    let return_pos = script
        .code
        .find("const __returned__ = {")
        .map(|p| p + "const __returned__ = ".len())
        .or_else(|| script.code.find("return{"))
        .expect("Should have return");
    let return_end = script.code[return_pos..].find('}').unwrap() + return_pos;
    let return_section = &script.code[return_pos..=return_end];
    // CONST_VAL is an import not referenced in the template (only used as
    // `const x = CONST_VAL` in script), so it should NOT be in __returned__.
    assert!(
        !return_section.contains("CONST_VAL"),
        "Import CONST_VAL (not used in template) should NOT be in return, got section: {}",
        return_section
    );
    assert!(
        !return_section.contains("MyType"),
        "Per-specifier type import MyType should NOT be in return, got section: {}",
        return_section
    );
}

// =========================================================================
// Production Template
// =========================================================================

#[test]
fn test_prod_template_has_render_function() {
    // Official production default is INLINE: render is a setup-returned
    // closure, not a standalone `function render(` in a template block.
    let input =
        "<script setup>\nconst msg = 'Hello'\n</script>\n<template><div>{{ msg }}</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new()
        .with_filename("test.vue")
        .with_production(true);
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    assert!(
        result.template.is_none(),
        "production inlines the render — no template block"
    );
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("return (_ctx,_cache) => {"),
        "production inlines the render into setup, got:\n{}",
        script.code
    );
}

#[test]
fn test_prod_template_before_script() {
    // Official production default is INLINE — holds for template-before-script
    // block order too (TS keeps the _defineComponent wrapper, V1a gate).
    let input = "<template><div class=\"text-sm\">{{ msg }}</div></template>\n<script setup lang=\"ts\">\nconst msg = ref('Hello')\n</script>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new()
        .with_filename("test.vue")
        .with_production(true);
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    assert!(
        result.template.is_none(),
        "production inlines the render — no template block"
    );
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("return (_ctx,_cache) => {"),
        "template-before-script production inlines the render, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("/*@__PURE__*/_defineComponent({"),
        "TS inline keeps the _defineComponent wrapper, got:\n{}",
        script.code
    );
}

#[test]
fn test_dev_template_before_script() {
    let input = "<template><div class=\"text-sm\">{{ msg }}</div></template>\n<script setup lang=\"ts\">\nconst msg = ref('Hello')\n</script>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    // Both blocks should exist
    assert!(result.script.is_some(), "Should have script block");
    assert!(result.template.is_some(), "Should have template block");
}

#[test]
fn test_prod_script_has_return_statement() {
    // Official production default is INLINE: setup returns the render closure
    // directly — there is no `__returned__` bindings object.
    let input =
        "<script setup>\nconst msg = 'Hello'\n</script>\n<template><div>{{ msg }}</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new()
        .with_filename("test.vue")
        .with_production(true);
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        !script.code.contains("__returned__"),
        "inline production mode must not have a __returned__ object, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("return (_ctx,_cache) => {"),
        "inline production setup returns the render closure, got:\n{}",
        script.code
    );
}

#[test]
fn test_prod_options_api_uses_function_render() {
    let input = "<script>\nimport { defineComponent } from 'vue'\nexport default defineComponent({ data() { return { count: 0 } } })\n</script>\n<template><div>{{ count }}</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new()
        .with_filename("test.vue")
        .with_production(true);
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let template = result
        .template
        .as_ref()
        .expect("should have template block");
    assert!(
        template.code.contains("function render("),
        "Options API in production should use function render(), got:\n{}",
        template.code
    );
}

#[test]
fn test_prod_script_setup_template_has_render() {
    // Official production default is INLINE: render is a setup-returned
    // closure — no separate template block.
    let input =
        "<script setup>\nconst count = 0\n</script>\n<template><div>{{ count }}</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new()
        .with_filename("test.vue")
        .with_production(true);
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    assert!(
        result.template.is_none(),
        "production inlines the render — no template block"
    );
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("return (_ctx,_cache) => {"),
        "script setup in production inlines the render into setup, got:\n{}",
        script.code
    );
}

// =========================================================================
// CSS v-bind and Scoped Styles
// =========================================================================

#[test]
fn test_css_v_bind_ref_uses_value_accessor() {
    let input = r#"<script setup>
import { ref } from 'vue'
const themeColor = ref('red')
</script>
<template><div>{{ themeColor }}</div></template>
<style scoped>
.text { color: v-bind(themeColor); }
</style>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("_useCssVars"),
        "Should inject _useCssVars call, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("themeColor.value"),
        "Should use themeColor.value (not _ctx.themeColor) since it's a ref, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("_ctx.themeColor"),
        "Should NOT use _ctx.themeColor for setup ref bindings, got:\n{}",
        script.code
    );
}

#[test]
fn test_scoped_style_uses_sfc_variable() {
    let input = "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>\n<style scoped>\n.red { color: red }\n</style>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("const __sfc__ = {"),
        "Should use plain const __sfc__ object for scoped styles, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("__sfc__.__scopeId"),
        "Should set __scopeId on __sfc__, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("export default __sfc__"),
        "Should export __sfc__ at the end, got:\n{}",
        script.code
    );
    let export_count = script.code.matches("export default ").count();
    assert_eq!(
        export_count, 1,
        "Should have exactly one export default, got {export_count} in:\n{}",
        script.code
    );
}

#[test]
fn test_scoped_with_export_default_in_comment() {
    let input = r#"<script setup>
const msg = 'hi'
// Transform: export default X -> something
</script>
<template><div>{{ msg }}</div></template>
<style scoped>
.red { color: red }
</style>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("export default __sfc__"),
        "Should have export default __sfc__, got:\n{}",
        script.code
    );
    let export_stmt_count = script
        .code
        .lines()
        .filter(|line| line.trim_start().starts_with("export default "))
        .count();
    assert_eq!(
        export_stmt_count, 1,
        "Should have exactly one export default statement, got {export_stmt_count} in:\n{}",
        script.code
    );
}

#[test]
fn test_regular_script_scoped_style() {
    let input = "<script>\nexport default { name: 'Foo' }\n</script>\n<template><div>x</div></template>\n<style scoped>\n.red { color: red }\n</style>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("const __sfc__ ="),
        "Scoped regular script should use const __sfc__, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("__sfc__.__scopeId"),
        "Should set __scopeId on __sfc__, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("export default __sfc__"),
        "Should export __sfc__ at the end, got:\n{}",
        script.code
    );
    let export_count = script.code.matches("export default ").count();
    assert_eq!(
        export_count, 1,
        "Should have exactly one export default, got {export_count} in:\n{}",
        script.code
    );
}

#[test]
fn test_non_scoped_setup_uses_sfc_variable() {
    let input =
        "<script setup>\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>";
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("const __sfc__ = {"),
        "Should use plain const __sfc__ object pattern, got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("export default __sfc__"),
        "Should export __sfc__ at the end, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("__sfc__.__scopeId"),
        "Non-scoped should not set __scopeId, got:\n{}",
        script.code
    );
}

// =========================================================================
// TypeScript Declaration Stripping
// =========================================================================

#[test]
fn test_ts_interface_stripped() {
    let input = r#"<script lang="ts" setup>
import { computed } from 'vue';

interface Props {
  codes?: string[];
}

const props = withDefaults(defineProps<Props>(), {
  codes: () => [],
});

const hasAccess = computed(() => props.codes.length > 0);
</script>
<template><div v-if="hasAccess"><slot /></div></template>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        !script.code.contains("interface"),
        "TypeScript interface should be stripped, got:\n{}",
        script.code
    );
}

#[test]
fn test_ts_type_alias_stripped() {
    let input = r#"<script lang="ts" setup>
type Status = 'active' | 'inactive';

const status: Status = 'active';
</script>
<template><div>{{ status }}</div></template>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        !script.code.contains("type Status"),
        "TypeScript type alias should be stripped, got:\n{}",
        script.code
    );
}

#[test]
fn test_ts_enum_preserved() {
    // With force_js: true, enums are transpiled to their JavaScript runtime form
    // (var + IIFE pattern) rather than being stripped like interfaces/types.
    let input = r#"<script lang="ts" setup>
enum Direction {
  Up,
  Down,
  Left,
  Right,
}

const dir = Direction.Up;
</script>
<template><div>{{ dir }}</div></template>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        script.code.contains("Direction"),
        "Enum should be preserved in output (transpiled to JS), got:\n{}",
        script.code
    );
    assert!(
        script.code.contains("Direction.Up"),
        "Enum usage should be preserved, got:\n{}",
        script.code
    );
}

#[test]
fn test_ts_multiple_declarations_stripped() {
    let input = r#"<script lang="ts" setup>
interface User {
  name: string;
  age: number;
}

type Role = 'admin' | 'user';

const user = { name: 'test', age: 25 };
</script>
<template><div>{{ user.name }}</div></template>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        !script.code.contains("interface User"),
        "TypeScript interface should be stripped, got:\n{}",
        script.code
    );
    assert!(
        !script.code.contains("type Role"),
        "TypeScript type alias should be stripped, got:\n{}",
        script.code
    );
}

#[test]
fn test_ts_declare_stripped() {
    let input = r#"<script lang="ts" setup>
declare const __brand: unique symbol;
declare function assertNever(x: never): never;

const x = 1;
</script>
<template><div>{{ x }}</div></template>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        !script.code.contains("declare"),
        "TypeScript declare should be stripped, got:\n{}",
        script.code
    );
}

#[test]
fn test_ts_export_interface_stripped() {
    let input = r#"<script lang="ts" setup>
export interface MyProps {
  value: string;
}

const x = 1;
</script>
<template><div>{{ x }}</div></template>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        !script.code.contains("interface"),
        "Exported TypeScript interface should be stripped, got:\n{}",
        script.code
    );
}

#[test]
fn test_ts_export_type_alias_stripped() {
    let input = r#"<script lang="ts" setup>
export type MyType = string | number;

const x = 1;
</script>
<template><div>{{ x }}</div></template>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        !script.code.contains("type MyType"),
        "Exported TypeScript type alias should be stripped, got:\n{}",
        script.code
    );
}

#[test]
fn test_ts_namespace_stripped() {
    let input = r#"<script lang="ts" setup>
namespace MyNS {
  export interface Foo {
    bar: string;
  }
}

const x = 1;
</script>
<template><div>{{ x }}</div></template>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let script = result.script.as_ref().expect("should have script block");
    assert!(
        !script.code.contains("namespace"),
        "TypeScript namespace should be stripped, got:\n{}",
        script.code
    );
}

// =========================================================================
// Template-only Components (no <script> block)
// =========================================================================

#[test]
fn test_template_only_component_has_no_script_block() {
    let input = r#"<template><div>hello</div></template>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    assert!(
        result.script.is_none(),
        "Template-only component should have no script block"
    );
    assert!(
        result.template.is_some(),
        "Template-only component should have a template block"
    );
}

#[test]
fn test_template_only_component_has_render_function() {
    let input = r#"<template>
  <div class="footer">
    <span>Footer text</span>
  </div>
</template>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    let template = result
        .template
        .as_ref()
        .expect("should have template block");
    assert!(
        template.code.contains("function render("),
        "Template-only component should have a render function, got:\n{}",
        template.code
    );
}

#[test]
fn test_template_only_component_scoped_style() {
    let input = r#"<template><div class="red">hello</div></template>
<style scoped>
.red { color: red }
</style>"#;
    let allocator = Allocator::new();
    let options = CodegenOptions::new().with_filename("test.vue");
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(input, &options, &verter_opts, &allocator);
    // Template-only with scoped style should still have template block
    assert!(
        result.template.is_some(),
        "Template-only component with scoped style should have template block"
    );
    // Check that style was processed (scoped style should have scoped: true)
    assert!(
        !result.styles.is_empty(),
        "Template-only component with scoped style should have style blocks"
    );
    assert!(
        result.styles[0].scoped,
        "Style block should be marked as scoped"
    );
}
