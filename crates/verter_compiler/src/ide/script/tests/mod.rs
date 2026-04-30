//! IDE TSX script-generation test cohort (Phase 11d test sibling root).
//!
//! `tests/common.rs` hosts the shared `gen_tsx_script*` helpers; the
//! per-cohort sibling files live alongside this `mod.rs` and target the
//! same production surface that the corresponding `ide/script/<x>.rs`
//! production sibling implements.

#[allow(dead_code)]
mod common;

mod comp_emit_tests;
mod integration_tests;
mod macros_tests;
mod options_api_tests;
mod setup_tests;
mod template_ref_tests;
mod wrapper_tests;

use super::*;
use crate::code_transform::CodeTransform;
use crate::ide::CssModuleInfo;

/// Generate TSX script and return (code, bindings, type_constructs).
pub(super) fn gen_tsx_script_full(
    source: &str,
) -> (String, FxHashMap<String, BindingType>, String) {
    gen_tsx_script_full_with_opts(source, "App", "App.vue", vec![])
}

/// Generate TSX script with custom component name and CSS modules.
pub(super) fn gen_tsx_script_full_with_opts(
    source: &str,
    component_name: &str,
    filename: &str,
    css_modules: Vec<CssModuleInfo>,
) -> (String, FxHashMap<String, BindingType>, String) {
    let alloc = Allocator::new();
    let mut ct = CodeTransform::new(source, &alloc);

    // Parse SFC to extract script blocks
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

    let js_component_name = crate::ide::sanitize_js_identifier(filename);
    let options = IdeScriptOptions {
        component_name,
        js_component_name: &js_component_name,
        filename,
        scope_id: "data-v-abc123",
        has_scoped_style: false,
        runtime_module_name: "vue",
        types_module_name: "@verter/types",
        is_vapor: false,
        embed_ambient_types: true,
        is_jsx: false,
        conditional_root_narrowing: false,
        style_v_bind_vars: vec![],
        css_modules,
    };

    // Use unified CT mode: pass template_end so comp functions are emitted in code
    let template_end = syntax.template_ast().map(|tpl| {
        tpl.root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(tpl.root.tag_open.end)
    });

    let result = generate_ide_script(
        syntax.script(),
        syntax.script_setup(),
        syntax.template_ast(),
        source,
        &mut ct,
        &alloc,
        &options,
        template_end,
    );

    // Apply deferred return+close after template (same as compile.rs)
    if let (Some(return_close), Some(pos)) = (&result.return_close, result.return_close_pos) {
        ct.prepend_left(pos, return_close);
    }

    // Remove template/style blocks from output
    if let Some(tpl) = syntax.template_ast() {
        let start = tpl.root.tag_open.start;
        let end = tpl
            .root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(tpl.root.tag_open.end);
        ct.remove(start, end);
    }
    for style_node in syntax.style_nodes() {
        let start = style_node.tag_open.start;
        let end = style_node
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(style_node.tag_open.end);
        ct.remove(start, end);
    }

    let code = ct.build_string();
    let bindings: FxHashMap<String, BindingType> = result
        .bindings
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();

    (code, bindings, result.type_constructs)
}

pub(super) fn gen_tsx_script(source: &str) -> (String, FxHashMap<String, BindingType>) {
    let (code, bindings, _) = gen_tsx_script_full(source);
    (code, bindings)
}

/// Like gen_tsx_script_full but with conditional_root_narrowing enabled.
pub(super) fn gen_tsx_script_narrowing(source: &str) -> String {
    let alloc = Allocator::new();
    let mut ct = CodeTransform::new(source, &alloc);

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

    let options = IdeScriptOptions {
        component_name: "App",
        js_component_name: "App",
        filename: "App.vue",
        scope_id: "data-v-abc123",
        has_scoped_style: false,
        runtime_module_name: "vue",
        types_module_name: "@verter/types",
        is_vapor: false,
        embed_ambient_types: true,
        is_jsx: false,
        conditional_root_narrowing: true,
        style_v_bind_vars: vec![],
        css_modules: vec![],
    };

    let template_end = syntax.template_ast().map(|tpl| {
        tpl.root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(tpl.root.tag_open.end)
    });

    let result = generate_ide_script(
        syntax.script(),
        syntax.script_setup(),
        syntax.template_ast(),
        source,
        &mut ct,
        &alloc,
        &options,
        template_end,
    );

    if let (Some(return_close), Some(pos)) = (&result.return_close, result.return_close_pos) {
        ct.prepend_left(pos, return_close);
    }

    if let Some(tpl) = syntax.template_ast() {
        let start = tpl.root.tag_open.start;
        let end = tpl
            .root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(tpl.root.tag_open.end);
        ct.remove(start, end);
    }
    for style_node in syntax.style_nodes() {
        let start = style_node.tag_open.start;
        let end = style_node
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(style_node.tag_open.end);
        ct.remove(start, end);
    }

    ct.build_string()
}
/// Generate TSX script with custom options and return (code, bindings, type_constructs).
pub(super) fn gen_tsx_script_full_with_options(
    source: &str,
    options: IdeScriptOptions<'_>,
) -> (String, FxHashMap<String, BindingType>, String) {
    let alloc = Allocator::new();
    let mut ct = CodeTransform::new(source, &alloc);

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

    // Use unified CT mode: pass template_end so comp functions are emitted in code
    let template_end = syntax.template_ast().map(|tpl| {
        tpl.root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(tpl.root.tag_open.end)
    });

    let result = generate_ide_script(
        syntax.script(),
        syntax.script_setup(),
        syntax.template_ast(),
        source,
        &mut ct,
        &alloc,
        &options,
        template_end,
    );

    // Apply deferred return+close after template (same as compile.rs)
    if let (Some(return_close), Some(pos)) = (&result.return_close, result.return_close_pos) {
        ct.prepend_left(pos, return_close);
    }

    if let Some(tpl) = syntax.template_ast() {
        let start = tpl.root.tag_open.start;
        let end = tpl
            .root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(tpl.root.tag_open.end);
        ct.remove(start, end);
    }
    for style_node in syntax.style_nodes() {
        let start = style_node.tag_open.start;
        let end = style_node
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(style_node.tag_open.end);
        ct.remove(start, end);
    }

    let code = ct.build_string();
    let bindings: FxHashMap<String, BindingType> = result
        .bindings
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();

    (code, bindings, result.type_constructs)
}
/// Helper: generate IDE script output with `is_jsx: true`.
pub(super) fn gen_jsx_script(source: &str) -> (String, String) {
    let alloc = Allocator::new();
    let mut ct = CodeTransform::new(source, &alloc);

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

    let options = IdeScriptOptions {
        component_name: "App",
        js_component_name: "App",
        filename: "App.vue",
        scope_id: "data-v-abc123",
        has_scoped_style: false,
        runtime_module_name: "vue",
        types_module_name: "@verter/types",
        is_vapor: false,
        embed_ambient_types: true,
        is_jsx: true,
        conditional_root_narrowing: false,
        style_v_bind_vars: vec![],
        css_modules: vec![],
    };

    let template_end = syntax.template_ast().map(|tpl| {
        tpl.root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(tpl.root.tag_open.end)
    });

    let result = generate_ide_script(
        syntax.script(),
        syntax.script_setup(),
        syntax.template_ast(),
        source,
        &mut ct,
        &alloc,
        &options,
        template_end,
    );

    if let (Some(return_close), Some(pos)) = (&result.return_close, result.return_close_pos) {
        ct.prepend_left(pos, return_close);
    }

    if let Some(tpl) = syntax.template_ast() {
        let start = tpl.root.tag_open.start;
        let end = tpl
            .root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(tpl.root.tag_open.end);
        ct.remove(start, end);
    }
    for style_node in syntax.style_nodes() {
        let start = style_node.tag_open.start;
        let end = style_node
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(style_node.tag_open.end);
        ct.remove(start, end);
    }

    let code = ct.build_string();
    (code, result.type_constructs)
}
