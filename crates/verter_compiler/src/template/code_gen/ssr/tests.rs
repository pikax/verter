//! SSR codegen tests.
//!
//! Each test validates the SSR string-concatenation output against
//! the patterns produced by Vue's `@vue/compiler-ssr`.

use oxc_allocator::Allocator;

use crate::compile::legacy_test_support::{compile, CodegenOptions, VerterCompileOptions};
use crate::compile::{VerterCompileResult, VueMacroSemanticInput};

fn compile_sfc_ssr(source: &str) -> VerterCompileResult {
    compile_sfc_ssr_with_semantics(source, &VueMacroSemanticInput::Unavailable)
}

fn compile_sfc_ssr_with_semantics(
    source: &str,
    macro_semantics: &VueMacroSemanticInput,
) -> VerterCompileResult {
    let alloc = Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ssr: true,
        ..Default::default()
    };
    compile(source, &options, &verter_opts, macro_semantics, &alloc)
}

/// Helper: compile and return the template code, asserting no errors.
fn gen_ssr_template(source: &str) -> String {
    let result = compile_sfc_ssr(source);
    ssr_template_code(result)
}

fn gen_ssr_template_with_runtime(
    source: &str,
    runtime: std::sync::Arc<verter_macro_dto::MacroRuntimeBundle>,
) -> String {
    let result = compile_sfc_ssr_with_semantics(source, &VueMacroSemanticInput::Runtime(runtime));
    ssr_template_code(result)
}

fn ssr_template_code(result: VerterCompileResult) -> String {
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result
        .template
        .as_ref()
        .expect("should have template block");
    tpl.code.clone()
}

/// Helper: compile and return the script code, asserting no errors.
fn gen_ssr_script(source: &str) -> String {
    let result = compile_sfc_ssr(source);
    ssr_script_code(result)
}

fn ssr_script_code(result: VerterCompileResult) -> String {
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let script = result.script.as_ref().expect("should have script block");
    script.code.clone()
}

/// 0-based (line, UTF-16 column) for a byte offset — the column unit the
/// source-map spec uses.
fn ssr_map_byte_offset_to_line_col(text: &str, byte_offset: usize) -> (u32, u32) {
    let before = &text[..byte_offset];
    let line = before.matches('\n').count() as u32;
    let line_start = before.rfind('\n').map_or(0, |i| i + 1);
    (
        line,
        text[line_start..byte_offset].encode_utf16().count() as u32,
    )
}

mod components;
mod directives;
mod elements;
mod general;
mod optimization;
mod slots;
mod styles;
