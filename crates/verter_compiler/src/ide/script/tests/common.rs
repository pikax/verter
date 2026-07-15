//! Shared test helpers for the IDE TSX script-generation tests.
//!
//! Each cohort sibling resolves these via `use super::*;` because
//! `tests/mod.rs` re-exports them at the parent (`tests`) module scope.

use oxc_allocator::Allocator;
use rustc_hash::FxHashMap;

use super::super::generate_ide_script;
use crate::code_transform::CodeTransform;
use crate::ide::{CssModuleInfo, IdeScriptOptions};
use crate::template::code_gen::binding::BindingType;

/// Generate TSX script and return (code, bindings, type_constructs).
pub fn gen_tsx_script_full(source: &str) -> (String, FxHashMap<String, BindingType>, String) {
    gen_tsx_script_full_with_opts(source, "App", "App.vue", vec![])
}

/// Compute the AST-driven `template_used_vars` set the production IDE pipeline
/// plumbs into `IdeScriptOptions`, so codegen unit tests exercise the real
/// unused-binding liveness decision instead of the conservative `None` fallback.
pub fn template_used_vars_for(
    syntax: &crate::parser::Syntax,
    source: &str,
    alloc: &Allocator,
) -> Option<rustc_hash::FxHashSet<String>> {
    use crate::compile::template_expr_overlay::{
        collect_template_used_vars, ParseOptionsKey, TemplateExprStore,
    };
    let template_ast = syntax.template_ast()?;
    let span = (
        template_ast.root.tag_open.start,
        template_ast
            .root
            .tag_close
            .as_ref()
            .map(|tc| tc.end)
            .unwrap_or(template_ast.root.tag_open.end),
    );
    let mut store = TemplateExprStore::new();
    let parse_options = ParseOptionsKey::new(None, None);
    let oxc = store.get_or_build(
        template_ast,
        source,
        alloc,
        span,
        &parse_options,
        oxc_span::SourceType::tsx(),
        false,
    );
    // Liveness REQUIRES completeness — an incomplete result (a template-expression
    // parse error) collapses to `None` so the gate fails open, exactly as
    // `compile_inner` composes it.
    let (used, complete) = collect_template_used_vars(oxc, template_ast, source);
    if complete {
        Some(used)
    } else {
        None
    }
}

/// Generate TSX script with the production template-usage inventory AND the
/// production SOUND style `v-bind()` usage wired in, exactly as `compile_inner`
/// composes them. Exercises the unused-binding type-only-unwrap liveness path
/// (and its conservative fail-open default) end to end at the codegen unit layer.
pub fn gen_tsx_script_unwrap(source: &str) -> (String, FxHashMap<String, BindingType>) {
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

    // Mirror production's `has_parse_errors` gate: a malformed SFC yields no
    // template usage facts (`None` ⇒ incomplete ⇒ the liveness gate fails open).
    let template_used_vars = if syntax.has_errors() {
        None
    } else {
        template_used_vars_for(&syntax, source, &alloc)
    };

    // SOUND style v-bind usage, parsed from the SFC's `<style>` bodies exactly as
    // production does — never an externally pre-split list.
    let style_usage = crate::compile::style_usage::extract_style_v_bind_usage(
        syntax
            .style_nodes()
            .iter()
            .filter_map(|s| s.content.as_ref())
            .map(|c| &source[c.start as usize..c.end as usize]),
    );

    let js_component_name = crate::ide::sanitize_js_identifier("App.vue");
    let options = IdeScriptOptions {
        component_name: "App",
        js_component_name: &js_component_name,
        filename: "App.vue",
        scope_id: "data-v-abc123",
        has_scoped_style: false,
        runtime_module_name: "vue",
        types_module_name: "@verter/types",
        is_vapor: false,
        embed_ambient_types: true,
        is_jsx: false,
        conditional_root_narrowing: false,
        style_v_bind_vars: style_usage.used.iter().cloned().collect(),
        style_usage_complete: style_usage.complete,
        css_modules: vec![],
        template_used_vars,
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
    let bindings: FxHashMap<String, BindingType> = result
        .bindings
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    (code, bindings)
}

/// Generate TSX script with custom component name and CSS modules.
pub fn gen_tsx_script_full_with_opts(
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
        style_usage_complete: true,
        css_modules,
        template_used_vars: None,
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

pub fn gen_tsx_script(source: &str) -> (String, FxHashMap<String, BindingType>) {
    let (code, bindings, _) = gen_tsx_script_full(source);
    (code, bindings)
}

/// Generate IDE TSX and its source-map JSON (carrying the `x_verter_helper_preamble_end` boundary),
/// mirroring the production compile pipeline's IDE source-map step. Returns `(code, sourcemap_json)`.
/// Used to pin that IDE codegen publishes the typed helper-import-preamble boundary end-to-end.
pub fn gen_tsx_script_with_sourcemap(source: &str) -> (String, String) {
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

    let js_component_name = crate::ide::sanitize_js_identifier("App.vue");
    let options = IdeScriptOptions {
        component_name: "App",
        js_component_name: &js_component_name,
        filename: "App.vue",
        scope_id: "data-v-abc123",
        has_scoped_style: false,
        runtime_module_name: "vue",
        types_module_name: "@verter/types",
        is_vapor: false,
        embed_ambient_types: true,
        is_jsx: false,
        conditional_root_narrowing: false,
        style_v_bind_vars: vec![],
        style_usage_complete: true,
        css_modules: vec![],
        template_used_vars: None,
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
    // Append type constructs as the production pipeline does (after the source map anchor region);
    // they sit past the preamble so they do not move the boundary.
    if !result.type_constructs.is_empty() {
        ct.append(&result.type_constructs);
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
    let json = ct.generate_map_json_with_preamble(crate::code_transform::SourceMapOptions {
        source: Some("App.vue"),
        file: Some("App.vue"),
        include_content: true,
    });
    (code, json)
}

/// Like gen_tsx_script_full but with conditional_root_narrowing enabled.
pub fn gen_tsx_script_narrowing(source: &str) -> String {
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
        style_usage_complete: true,
        css_modules: vec![],
        template_used_vars: None,
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
pub fn gen_tsx_script_full_with_options(
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
pub fn gen_jsx_script(source: &str) -> (String, String) {
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
        style_usage_complete: true,
        css_modules: vec![],
        template_used_vars: None,
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
