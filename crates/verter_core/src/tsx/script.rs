//! TSX script generation.
//!
//! Generates the script portion of TSX output from `<script setup>` and `<script>` blocks.
//! Unlike the normal script codegen (which transforms macros into runtime code), this
//! preserves TypeScript types and macro call syntax for IDE type checking.
//!
//! ## Output structure
//!
//! For `<script setup>`:
//! ```tsx
//! // Hoisted imports
//! import { ref } from 'vue'
//! import type { Props } from './types'
//!
//! // Hoisted type declarations
//! interface Foo { ... }
//!
//! // Component function wrapper
//! function __verter_tsx_<ComponentName>(__props: ..., __ctx: ...) {
//!   // Setup body (macros preserved, bindings extracted)
//!   const count = ref(0)
//!   const props = defineProps<Props>()
//!
//!   return (
//!     // Template JSX goes here (separate block)
//!   )
//! }
//! ```

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, BindingPattern, CallExpression, Declaration, ExportDefaultDeclarationKind,
    Expression, ForStatementInit, Function, ObjectPropertyKind, Statement,
};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::ast::types::{AstNodeKind, TagType, TemplateAst};
use crate::code_transform::CodeTransform;
use crate::cursor::ScriptLanguage;
use crate::parser::types::RootNodeScript;
use crate::template::code_gen::binding::{is_simple_ident, BindingType};
use crate::template::code_gen::types::CodeGenOutput;
use crate::utils::oxc::vue::{parse_script, parse_script_with_companion, ScriptItem, ScriptMode};

use super::TsxScriptOptions;

/// Result of TSX script generation (internal, before building string).
pub struct TsxScriptGenResult<'alloc> {
    /// Binding metadata for template TSX generation.
    pub bindings: FxHashMap<&'alloc str, BindingType>,
}

/// Generate TSX script output from script blocks.
///
/// Returns the generated code, source map, and bindings for template generation.
pub fn generate_tsx_script<'alloc>(
    script: Option<&RootNodeScript>,
    script_setup: Option<&RootNodeScript>,
    template_ast: Option<&TemplateAst>,
    source: &'alloc str,
    ct: &mut CodeTransform<'alloc>,
    alloc: &'alloc Allocator,
    options: &TsxScriptOptions<'_>,
) -> TsxScriptGenResult<'alloc> {
    let mut out = CodeGenOutput::new(alloc);
    let mut bindings = FxHashMap::default();

    match (script, script_setup) {
        (_, Some(setup)) => {
            process_tsx_script_setup(
                setup,
                script,
                template_ast,
                source,
                &mut out,
                &mut bindings,
                alloc,
                options,
            );
        }
        (Some(normal), None) => {
            process_tsx_script_only(
                normal,
                template_ast,
                source,
                &mut out,
                &mut bindings,
                alloc,
                options,
            );
        }
        (None, None) => {
            // No script blocks — emit minimal component wrapper
            emit_minimal_wrapper(&mut out, options, 0);
        }
    }

    // Apply accumulated operations
    out.apply_to(ct);

    TsxScriptGenResult { bindings }
}

// ── Script Setup Processing ───────────────────────────────────────

fn process_tsx_script_setup<'alloc>(
    setup: &RootNodeScript,
    _normal_script: Option<&RootNodeScript>,
    template_ast: Option<&TemplateAst>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    bindings: &mut FxHashMap<&'alloc str, BindingType>,
    alloc: &'alloc Allocator,
    options: &TsxScriptOptions<'_>,
) {
    let content_span = match &setup.content {
        Some(span) => span,
        None => {
            // Self-closing <script setup />
            emit_minimal_wrapper(out, options, setup.tag_open.start);
            return;
        }
    };

    let content_start = content_span.start;
    let content_str = &source[content_span.start as usize..content_span.end as usize];
    let hoist_pos = setup.tag_open.start;

    // Parse with OXC
    let oxc_alloc = Allocator::default();
    let source_type = SourceType::tsx();
    let parser_ret = Parser::new(&oxc_alloc, content_str, source_type).parse();

    let parse_result = parse_script_with_companion(
        &parser_ret.program,
        ScriptMode::Setup,
        content_start,
        content_str,
        None, // No companion types needed for TSX — we preserve types as-is
    );

    // Infer event-handler parameter types from template usage (v5/process parity).
    if should_infer_function_types(setup.lang) {
        let available_bindings = collect_binding_names(&parse_result.bindings, source, content_str);
        apply_event_handler_param_inference(
            &parser_ret.program.body,
            template_ast,
            source,
            content_start,
            &available_bindings,
            out,
        );
        apply_template_ref_call_inference(
            &parser_ret.program.body,
            template_ast,
            source,
            content_str,
            content_start,
            &available_bindings,
            out,
        );
    }

    // Hoist imports to file top (before component wrapper)
    for item in &parse_result.items {
        if let ScriptItem::Import(imp) = item {
            let abs_start = content_start + imp.span.start;
            let abs_end = content_start + imp.span.end;

            // Hoist verbatim (keep all imports including type-only)
            let import_text = &source[abs_start as usize..abs_end as usize];
            out.overwrite(abs_start, abs_end, "");
            out.prepend_alloc(hoist_pos, &format!("{}\n", import_text));
        }
    }

    // Hoist type declarations to file top
    for item in &parse_result.items {
        if let ScriptItem::TypeDeclaration(td) = item {
            let abs_start = content_start + td.span.start;
            let abs_end = content_start + td.span.end;

            let td_text = &source[abs_start as usize..abs_end as usize];
            out.overwrite(abs_start, abs_end, "");
            out.prepend_alloc(hoist_pos, &format!("{}\n", td_text));
        }
    }

    // Extract bindings
    // Note: binding spans have mixed coordinate systems (see script/macros.rs:93):
    // - Props/PropsAliased spans are SFC-absolute (content_offset baked in by resolve_type)
    // - All other bindings are relative to content_str (0-based from OXC parser)
    for (span, bt) in &parse_result.bindings {
        let name = if *bt == BindingType::Props || *bt == BindingType::PropsAliased {
            // Absolute span — index into full SFC source
            &source[span.start as usize..span.end as usize]
        } else {
            // Relative span — index into content_str (script content only)
            &content_str[span.start as usize..span.end as usize]
        };
        let alloc_name = alloc.alloc_str(name);
        bindings.insert(alloc_name, *bt);
    }

    // Build component function wrapper opening
    // Replace <script setup> tag with function declaration
    let wrapper_start = format!("function __verter_tsx_{}() {{\n", options.js_component_name,);
    out.overwrite(setup.tag_open.start, setup.tag_open.end, &wrapper_start);

    // Replace </script> tag with closing
    if let Some(tag_close) = &setup.tag_close {
        let mut wrapper_end = String::with_capacity(128);
        wrapper_end.push_str("\nreturn (\n<>");

        // Placeholder — template JSX will be appended by the consumer
        wrapper_end.push_str("</>");
        wrapper_end.push_str("\n)\n}\n");

        out.overwrite(tag_close.start, tag_close.end, &wrapper_end);
    }
}

/// Infer untyped function-declaration parameters from template event bindings.
///
/// Mirrors the `v5/process` infer-function intent without porting plugin architecture:
/// for simple native-element handlers like `<button @click="handleClick">`,
/// rewrite `function handleClick(e) {}` into
/// `function handleClick(...[e]: Parameters<import('vue').IntrinsicElementAttributes["button"]["onClick"]>) {}`.
fn apply_event_handler_param_inference(
    body: &[Statement<'_>],
    template_ast: Option<&TemplateAst>,
    source: &str,
    content_start: u32,
    available_bindings: &FxHashSet<String>,
    out: &mut CodeGenOutput<'_>,
) {
    let Some(template_ast) = template_ast else {
        return;
    };

    let handler_type_hints =
        collect_event_handler_type_hints(template_ast, source, available_bindings);
    if handler_type_hints.is_empty() {
        return;
    }

    for stmt in body {
        match stmt {
            Statement::FunctionDeclaration(func) => {
                maybe_annotate_function_params(
                    func,
                    &handler_type_hints,
                    source,
                    content_start,
                    out,
                );
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(Declaration::FunctionDeclaration(func)) = &export.declaration {
                    maybe_annotate_function_params(
                        func,
                        &handler_type_hints,
                        source,
                        content_start,
                        out,
                    );
                }
            }
            _ => {}
        }
    }
}

fn maybe_annotate_function_params(
    func: &Function<'_>,
    handler_type_hints: &FxHashMap<String, String>,
    source: &str,
    content_start: u32,
    out: &mut CodeGenOutput<'_>,
) {
    let Some(id) = &func.id else {
        return;
    };
    let Some(type_expr) = handler_type_hints.get(id.name.as_str()) else {
        return;
    };

    // Keep existing typing intact.
    if func.params.rest.is_some() || func.params.items.is_empty() {
        return;
    }

    let mut param_names: Vec<&str> = Vec::with_capacity(func.params.items.len());
    for param in &func.params.items {
        if param.type_annotation.is_some() {
            return;
        }
        match &param.pattern {
            BindingPattern::BindingIdentifier(ident) => {
                param_names.push(ident.name.as_str());
            }
            _ => return,
        }
    }

    if param_names.is_empty() {
        return;
    }

    let params_start = content_start + func.params.span.start;
    let params_end = content_start + func.params.span.end;
    if params_end <= params_start {
        return;
    }

    let params_src = &source[params_start as usize..params_end as usize];
    let tuple_param = format!("...[{}]: {}", param_names.join(", "), type_expr);
    let replacement = if params_src.starts_with('(') && params_src.ends_with(')') {
        format!("({})", tuple_param)
    } else {
        tuple_param
    };

    out.overwrite(params_start, params_end, &replacement);
}

fn collect_event_handler_type_hints(
    ast: &TemplateAst,
    source: &str,
    available_bindings: &FxHashSet<String>,
) -> FxHashMap<String, String> {
    let mut hints = FxHashMap::default();

    let Some(content) = &ast.root.content else {
        return hints;
    };

    for &child in content.children.iter() {
        collect_event_handler_type_hints_from_node(
            child,
            ast,
            source,
            available_bindings,
            &mut hints,
        );
    }

    hints
}

fn collect_event_handler_type_hints_from_node(
    id: crate::types::NodeId,
    ast: &TemplateAst,
    source: &str,
    available_bindings: &FxHashSet<String>,
    hints: &mut FxHashMap<String, String>,
) {
    let node = &ast.nodes[id.0];
    let AstNodeKind::Element(el_box) = &node.kind else {
        return;
    };
    let el = el_box.as_ref();

    let tag_name = &source[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize];
    for prop in &el.props {
        if !is_event_directive(prop, source) {
            continue;
        }
        if prop.is_dynamic == Some(true) {
            continue;
        }
        let (Some(arg_start), Some(arg_end)) = (prop.arg_start, prop.arg_end) else {
            continue;
        };
        let (Some(value_start), Some(value_end)) = (prop.value_start, prop.value_end) else {
            continue;
        };

        let handler = source[value_start as usize..value_end as usize].trim();
        if !is_simple_ident(handler) {
            continue;
        }

        let event_name = source[arg_start as usize..arg_end as usize].trim();
        if event_name.is_empty() {
            continue;
        }

        let event_prop = event_to_jsx_name(event_name);
        let type_expr = match el.tag_type {
            TagType::Element => format!(
                "Parameters<NonNullable<import('vue').IntrinsicElementAttributes[\"{}\"][\"{}\"]>>",
                tag_name, event_prop
            ),
            TagType::Component => {
                let Some(component_binding) =
                    resolve_component_binding_name(tag_name, available_bindings)
                else {
                    continue;
                };
                format!(
                    "Parameters<NonNullable<Required<InstanceType<typeof {}>[\"$props\"]>[\"{}\"]>>",
                    component_binding, event_prop
                )
            }
            _ => continue,
        };

        // Keep first discovered hint for deterministic behavior.
        hints.entry(handler.to_string()).or_insert(type_expr);
    }

    if let Some(content) = &el.content {
        for &child in content.children.iter() {
            collect_event_handler_type_hints_from_node(
                child,
                ast,
                source,
                available_bindings,
                hints,
            );
        }
    }
}

fn resolve_component_binding_name(
    tag_name: &str,
    available_bindings: &FxHashSet<String>,
) -> Option<String> {
    if is_simple_ident(tag_name) && available_bindings.contains(tag_name) {
        return Some(tag_name.to_string());
    }

    if tag_name.contains('-') {
        let pascal = kebab_to_pascal_case(tag_name);
        if available_bindings.contains(&pascal) {
            return Some(pascal);
        }
    }

    None
}

fn kebab_to_pascal_case(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut upper_next = true;
    for ch in input.chars() {
        if ch == '-' || ch == '_' {
            upper_next = true;
            continue;
        }
        if upper_next {
            for up in ch.to_uppercase() {
                out.push(up);
            }
            upper_next = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn is_event_directive(prop: &crate::types::NodeProp, source: &str) -> bool {
    if !prop.is_directive {
        return false;
    }
    get_directive_name(prop, source) == "on"
}

fn get_directive_name<'a>(prop: &crate::types::NodeProp, source: &'a str) -> &'a str {
    let name = &source[prop.start as usize..prop.name_end as usize];

    if name.starts_with(':') || name.starts_with('.') {
        return "bind";
    }
    if name.starts_with('@') {
        return "on";
    }
    if name.starts_with('#') {
        return "slot";
    }

    // Full directive form: v-bind / v-on / v-if / ...
    name.strip_prefix("v-").unwrap_or(name)
}

fn event_to_jsx_name(event_name: &str) -> String {
    if let Some(rest) = event_name.strip_prefix("update:") {
        return format!("onUpdate:{}", rest);
    }

    let mut result = String::with_capacity(event_name.len() + 2);
    result.push_str("on");
    let mut chars = event_name.chars();
    if let Some(first) = chars.next() {
        for upper in first.to_uppercase() {
            result.push(upper);
        }
        result.extend(chars);
    }
    result
}

fn should_infer_function_types(lang: Option<ScriptLanguage>) -> bool {
    matches!(lang, Some(ScriptLanguage::TypeScript | ScriptLanguage::TSX))
}

fn collect_binding_names(
    bindings: &[(crate::common::Span, BindingType)],
    source: &str,
    content_str: &str,
) -> FxHashSet<String> {
    let mut out = FxHashSet::default();
    for (span, bt) in bindings {
        let name = if *bt == BindingType::Props || *bt == BindingType::PropsAliased {
            &source[span.start as usize..span.end as usize]
        } else {
            &content_str[span.start as usize..span.end as usize]
        };
        if !name.is_empty() {
            out.insert(name.to_string());
        }
    }
    out
}

#[derive(Debug, Clone)]
struct TemplateRefCandidate {
    name_for_match: String,
    name_type: String,
    target_type: String,
}

#[derive(Debug, Clone)]
enum TemplateRefSelector {
    Arg(String),
}

#[derive(Debug, Clone)]
enum TemplateRefCallKind {
    UseTemplateRef {
        selector: Option<TemplateRefSelector>,
    },
    RefVariable {
        var_name: String,
    },
}

#[derive(Debug, Clone)]
struct TemplateRefCallSite {
    kind: TemplateRefCallKind,
    callee_end: u32,
}

#[derive(Default)]
struct TemplateRefScriptScanner {
    call_sites: Vec<TemplateRefCallSite>,
    declaration_string_values: FxHashMap<String, String>,
}

fn apply_template_ref_call_inference(
    body: &[Statement<'_>],
    template_ast: Option<&TemplateAst>,
    source: &str,
    script_source: &str,
    content_start: u32,
    available_bindings: &FxHashSet<String>,
    out: &mut CodeGenOutput<'_>,
) {
    let Some(template_ast) = template_ast else {
        return;
    };

    let template_refs = collect_template_ref_candidates(template_ast, source, available_bindings);
    if template_refs.is_empty() {
        return;
    }

    let mut scanner = TemplateRefScriptScanner::default();
    for stmt in body {
        scanner.visit_statement(stmt, script_source);
    }

    if scanner.call_sites.is_empty() {
        return;
    }

    let all_name_types: Vec<String> = template_refs.iter().map(|r| r.name_type.clone()).collect();
    if all_name_types.is_empty() {
        return;
    }
    let names_union = join_type_union(&all_name_types);

    for call in &scanner.call_sites {
        let callee_abs_end = content_start + call.callee_end;
        match &call.kind {
            TemplateRefCallKind::UseTemplateRef { selector } => {
                let matched_types = select_matching_template_ref_types(
                    &template_refs,
                    selector.as_ref(),
                    &scanner.declaration_string_values,
                );
                let types_union = if matched_types.is_empty() {
                    "unknown".to_string()
                } else {
                    join_type_union(&matched_types)
                };
                let generic = format!("<{},{}>", types_union, names_union);
                out.prepend_alloc(callee_abs_end, &generic);
            }
            TemplateRefCallKind::RefVariable { var_name } => {
                let selector = TemplateRefSelector::Arg(var_name.clone());
                let matched_types = select_matching_template_ref_types(
                    &template_refs,
                    Some(&selector),
                    &scanner.declaration_string_values,
                );
                if matched_types.is_empty() {
                    continue;
                }
                let types_union = join_type_union(&matched_types);
                let generic = format!("<{}|null>", types_union);
                out.prepend_alloc(callee_abs_end, &generic);
            }
        }
    }
}

fn collect_template_ref_candidates(
    ast: &TemplateAst,
    source: &str,
    available_bindings: &FxHashSet<String>,
) -> Vec<TemplateRefCandidate> {
    let mut out = Vec::new();
    let Some(content) = &ast.root.content else {
        return out;
    };
    for &child in content.children.iter() {
        collect_template_ref_candidates_from_node(child, ast, source, available_bindings, &mut out);
    }
    out
}

fn collect_template_ref_candidates_from_node(
    id: crate::types::NodeId,
    ast: &TemplateAst,
    source: &str,
    available_bindings: &FxHashSet<String>,
    out: &mut Vec<TemplateRefCandidate>,
) {
    let node = &ast.nodes[id.0];
    let AstNodeKind::Element(el_box) = &node.kind else {
        return;
    };
    let el = el_box.as_ref();
    let tag_name = &source[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize];

    let mut target_type =
        resolve_template_ref_target_type(el, tag_name, source, available_bindings);
    if target_type.is_empty() {
        target_type = "unknown".to_string();
    }
    if element_is_inside_v_for(id, ast) {
        target_type.push_str("[]");
    }

    if let Some(v_ref) = &el.v_ref {
        if let (Some(vs), Some(ve)) = (v_ref.value_start, v_ref.value_end) {
            let name = source[vs as usize..ve as usize].trim();
            if !name.is_empty() {
                out.push(TemplateRefCandidate {
                    name_for_match: name.to_string(),
                    name_type: quote_ts_string(name),
                    target_type: target_type.clone(),
                });
            }
        }
    }

    for prop in &el.props {
        if !prop.is_directive {
            continue;
        }
        let base = &source[prop.start as usize..prop.name_end as usize];
        if base != ":" && base != "v-bind" {
            continue;
        }
        let (Some(arg_s), Some(arg_e)) = (prop.arg_start, prop.arg_end) else {
            continue;
        };
        if &source[arg_s as usize..arg_e as usize] != "ref" {
            continue;
        }
        let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) else {
            continue;
        };
        let expr = source[vs as usize..ve as usize].trim();
        if expr.is_empty() || is_function_ref_expression(expr) {
            continue;
        }
        out.push(TemplateRefCandidate {
            name_for_match: expr.to_string(),
            name_type: format!("typeof {}", expr),
            target_type: target_type.clone(),
        });
    }

    if let Some(content) = &el.content {
        for &child in content.children.iter() {
            collect_template_ref_candidates_from_node(child, ast, source, available_bindings, out);
        }
    }
}

fn resolve_template_ref_target_type(
    el: &crate::ast::types::ElementNode,
    tag_name: &str,
    source: &str,
    available_bindings: &FxHashSet<String>,
) -> String {
    if tag_name == "component" {
        if let Some(static_is) = find_component_static_is(el, source) {
            return resolve_tag_name_type(&static_is, false, available_bindings);
        }
        if let Some(dynamic_is_expr) = find_component_dynamic_is_expression(el, source) {
            return resolve_dynamic_component_is_type(&dynamic_is_expr, available_bindings);
        }
        return "unknown".to_string();
    }

    resolve_tag_name_type(
        tag_name,
        el.tag_type == TagType::Component,
        available_bindings,
    )
}

fn find_component_static_is(el: &crate::ast::types::ElementNode, source: &str) -> Option<String> {
    for prop in &el.props {
        if prop.is_directive {
            continue;
        }
        let name = &source[prop.start as usize..prop.name_end as usize];
        if name != "is" {
            continue;
        }
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let value = source[vs as usize..ve as usize].trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn find_component_dynamic_is_expression(
    el: &crate::ast::types::ElementNode,
    source: &str,
) -> Option<String> {
    for prop in &el.props {
        if !prop.is_directive {
            continue;
        }
        let base = &source[prop.start as usize..prop.name_end as usize];
        if base != ":" && base != "v-bind" {
            continue;
        }
        let (Some(arg_s), Some(arg_e)) = (prop.arg_start, prop.arg_end) else {
            continue;
        };
        if &source[arg_s as usize..arg_e as usize] != "is" {
            continue;
        }
        if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
            let expr = source[vs as usize..ve as usize].trim();
            if !expr.is_empty() {
                return Some(expr.to_string());
            }
        }
    }
    None
}

fn resolve_dynamic_component_is_type(expr: &str, available_bindings: &FxHashSet<String>) -> String {
    let tags = extract_string_literals(expr);
    if tags.is_empty() {
        return format!("typeof {}", expr);
    }

    let mut types = Vec::with_capacity(tags.len());
    for tag in tags {
        types.push(resolve_tag_name_type(&tag, false, available_bindings));
    }
    join_type_union(&types)
}

fn resolve_tag_name_type(
    tag_name: &str,
    is_component_hint: bool,
    available_bindings: &FxHashSet<String>,
) -> String {
    if is_component_hint {
        if let Some(binding) = resolve_component_binding_name(tag_name, available_bindings) {
            return format!("InstanceType<typeof {}>", binding);
        }
        if is_member_path(tag_name) {
            return format!("InstanceType<typeof {}>", tag_name);
        }
    } else if is_probably_native_tag(tag_name) {
        return native_tag_element_type(tag_name);
    } else if let Some(binding) = resolve_component_binding_name(tag_name, available_bindings) {
        return format!("InstanceType<typeof {}>", binding);
    } else if is_member_path(tag_name) {
        return format!("InstanceType<typeof {}>", tag_name);
    }

    native_tag_element_type(tag_name)
}

fn native_tag_element_type(tag_name: &str) -> String {
    format!(
        "(\"{0}\" extends keyof HTMLElementTagNameMap ? HTMLElementTagNameMap[\"{0}\"] : \"{0}\" extends keyof SVGElementTagNameMap ? SVGElementTagNameMap[\"{0}\"] : Element)",
        tag_name
    )
}

fn is_probably_native_tag(tag_name: &str) -> bool {
    let Some(first) = tag_name.chars().next() else {
        return false;
    };
    first.is_ascii_lowercase() && !tag_name.chars().any(|c| c.is_ascii_uppercase())
}

fn is_member_path(value: &str) -> bool {
    if !value.contains('.') {
        return false;
    }
    value.split('.').all(is_simple_ident)
}

fn element_is_inside_v_for(id: crate::types::NodeId, ast: &TemplateAst) -> bool {
    let mut current = Some(id);
    while let Some(node_id) = current {
        let node = &ast.nodes[node_id.0];
        if let AstNodeKind::Element(el_box) = &node.kind {
            if el_box.v_for.is_some() {
                return true;
            }
        }
        current = node.parent;
    }
    false
}

fn is_function_ref_expression(expr: &str) -> bool {
    let trimmed = expr.trim();
    trimmed.contains("=>") || trimmed.starts_with("function")
}

fn quote_ts_string(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

fn join_type_union(types: &[String]) -> String {
    let mut seen = FxHashSet::default();
    let mut ordered = Vec::with_capacity(types.len());
    for ty in types {
        if seen.insert(ty.clone()) {
            ordered.push(ty.clone());
        }
    }
    ordered.join("|")
}

fn select_matching_template_ref_types(
    candidates: &[TemplateRefCandidate],
    selector: Option<&TemplateRefSelector>,
    declaration_string_values: &FxHashMap<String, String>,
) -> Vec<String> {
    let mut out = Vec::new();
    if selector.is_none() {
        out.extend(candidates.iter().map(|c| c.target_type.clone()));
        return out;
    }

    let selector_text = match selector {
        Some(TemplateRefSelector::Arg(v)) => v.as_str(),
        None => "",
    };
    let selector_resolved = resolve_declared_string_value(selector_text, declaration_string_values)
        .unwrap_or(selector_text);

    for candidate in candidates {
        let candidate_text = candidate.name_for_match.as_str();
        let candidate_resolved =
            resolve_declared_string_value(candidate_text, declaration_string_values)
                .unwrap_or(candidate_text);

        if selector_text == candidate_text
            || selector_text == candidate_resolved
            || selector_resolved == candidate_text
            || selector_resolved == candidate_resolved
        {
            out.push(candidate.target_type.clone());
        }
    }

    out
}

fn resolve_declared_string_value<'a>(
    key: &'a str,
    declaration_string_values: &'a FxHashMap<String, String>,
) -> Option<&'a str> {
    declaration_string_values.get(key).map(|v| v.as_str())
}

fn extract_string_literals(expr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = expr.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let quote = bytes[i];
        if quote != b'\'' && quote != b'"' {
            i += 1;
            continue;
        }
        i += 1;
        let start = i;
        let mut escaped = false;
        while i < bytes.len() {
            let b = bytes[i];
            if escaped {
                escaped = false;
                i += 1;
                continue;
            }
            if b == b'\\' {
                escaped = true;
                i += 1;
                continue;
            }
            if b == quote {
                if i > start {
                    out.push(expr[start..i].to_string());
                } else {
                    out.push(String::new());
                }
                i += 1;
                break;
            }
            i += 1;
        }
    }
    out
}

impl TemplateRefScriptScanner {
    fn visit_statement(&mut self, stmt: &Statement, source: &str) {
        match stmt {
            Statement::VariableDeclaration(var_decl) => {
                self.visit_variable_declaration(var_decl, source);
            }
            Statement::ExpressionStatement(expr_stmt) => {
                self.visit_expression(&expr_stmt.expression, source);
            }
            Statement::ReturnStatement(ret) => {
                if let Some(arg) = &ret.argument {
                    self.visit_expression(arg, source);
                }
            }
            Statement::BlockStatement(block) => {
                for stmt in &block.body {
                    self.visit_statement(stmt, source);
                }
            }
            Statement::IfStatement(if_stmt) => {
                self.visit_expression(&if_stmt.test, source);
                self.visit_statement(&if_stmt.consequent, source);
                if let Some(alt) = &if_stmt.alternate {
                    self.visit_statement(alt, source);
                }
            }
            Statement::ForStatement(for_stmt) => {
                if let Some(init) = &for_stmt.init {
                    match init {
                        ForStatementInit::VariableDeclaration(var_decl) => {
                            self.visit_variable_declaration(var_decl, source);
                        }
                        _ => {
                            if let Some(expr) = init.as_expression() {
                                self.visit_expression(expr, source);
                            }
                        }
                    }
                }
                if let Some(test) = &for_stmt.test {
                    self.visit_expression(test, source);
                }
                if let Some(update) = &for_stmt.update {
                    self.visit_expression(update, source);
                }
                self.visit_statement(&for_stmt.body, source);
            }
            Statement::ForInStatement(for_in) => {
                if let oxc_ast::ast::ForStatementLeft::VariableDeclaration(var_decl) = &for_in.left
                {
                    self.visit_variable_declaration(var_decl, source);
                }
                self.visit_expression(&for_in.right, source);
                self.visit_statement(&for_in.body, source);
            }
            Statement::ForOfStatement(for_of) => {
                if let oxc_ast::ast::ForStatementLeft::VariableDeclaration(var_decl) = &for_of.left
                {
                    self.visit_variable_declaration(var_decl, source);
                }
                self.visit_expression(&for_of.right, source);
                self.visit_statement(&for_of.body, source);
            }
            Statement::WhileStatement(while_stmt) => {
                self.visit_expression(&while_stmt.test, source);
                self.visit_statement(&while_stmt.body, source);
            }
            Statement::DoWhileStatement(do_while) => {
                self.visit_statement(&do_while.body, source);
                self.visit_expression(&do_while.test, source);
            }
            Statement::SwitchStatement(switch_stmt) => {
                self.visit_expression(&switch_stmt.discriminant, source);
                for case in &switch_stmt.cases {
                    if let Some(test) = &case.test {
                        self.visit_expression(test, source);
                    }
                    for stmt in &case.consequent {
                        self.visit_statement(stmt, source);
                    }
                }
            }
            Statement::TryStatement(try_stmt) => {
                for stmt in &try_stmt.block.body {
                    self.visit_statement(stmt, source);
                }
                if let Some(handler) = &try_stmt.handler {
                    for stmt in &handler.body.body {
                        self.visit_statement(stmt, source);
                    }
                }
                if let Some(finalizer) = &try_stmt.finalizer {
                    for stmt in &finalizer.body {
                        self.visit_statement(stmt, source);
                    }
                }
            }
            Statement::ThrowStatement(throw_stmt) => {
                self.visit_expression(&throw_stmt.argument, source);
            }
            Statement::LabeledStatement(labeled) => {
                self.visit_statement(&labeled.body, source);
            }
            Statement::FunctionDeclaration(func) => {
                self.visit_function(func, source);
            }
            Statement::ExportNamedDeclaration(export) => {
                if let Some(declaration) = &export.declaration {
                    self.visit_declaration(declaration, source);
                }
            }
            Statement::ExportDefaultDeclaration(export) => match &export.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                    self.visit_function(func, source);
                }
                _ => {
                    if let Some(expr) = export.declaration.as_expression() {
                        self.visit_expression(expr, source);
                    }
                }
            },
            _ => {}
        }
    }

    fn visit_declaration(&mut self, declaration: &Declaration, source: &str) {
        match declaration {
            Declaration::VariableDeclaration(var_decl) => {
                self.visit_variable_declaration(var_decl, source);
            }
            Declaration::FunctionDeclaration(func) => {
                self.visit_function(func, source);
            }
            _ => {}
        }
    }

    fn visit_function(&mut self, function: &Function, source: &str) {
        if let Some(body) = &function.body {
            for stmt in &body.statements {
                self.visit_statement(stmt, source);
            }
        }
    }

    fn visit_variable_declaration(
        &mut self,
        var_decl: &oxc_ast::ast::VariableDeclaration,
        source: &str,
    ) {
        for declarator in &var_decl.declarations {
            if let Some(init) = &declarator.init {
                if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                    self.collect_declared_string_values(id.name.as_str(), init, source);
                    self.record_ref_variable_call(id.name.as_str(), init);
                }
                self.visit_expression(init, source);
            }
        }
    }

    fn record_ref_variable_call(&mut self, var_name: &str, init: &Expression) {
        let expr = unwrap_wrapped_expression(init);
        let Expression::CallExpression(call) = expr else {
            return;
        };
        if call.type_arguments.is_some() {
            return;
        }
        let Some(callee_name) = callee_identifier_name(&call.callee) else {
            return;
        };
        if callee_name != "ref" {
            return;
        }
        if call.arguments.len() > 1 {
            return;
        }
        if call.arguments.len() == 1 && !is_null_argument(&call.arguments[0]) {
            return;
        }

        self.call_sites.push(TemplateRefCallSite {
            kind: TemplateRefCallKind::RefVariable {
                var_name: var_name.to_string(),
            },
            callee_end: call.callee.span().end,
        });
    }

    fn collect_declared_string_values(&mut self, base_name: &str, init: &Expression, source: &str) {
        let expr = unwrap_wrapped_expression(init);
        match expr {
            Expression::StringLiteral(lit) => {
                self.declaration_string_values
                    .insert(base_name.to_string(), lit.value.to_string());
            }
            Expression::TemplateLiteral(tpl) => {
                if tpl.expressions.is_empty() && tpl.quasis.len() == 1 {
                    self.declaration_string_values.insert(
                        base_name.to_string(),
                        tpl.quasis[0].value.raw.as_str().to_string(),
                    );
                }
            }
            Expression::ObjectExpression(obj) => {
                for prop in &obj.properties {
                    let ObjectPropertyKind::ObjectProperty(obj_prop) = prop else {
                        continue;
                    };
                    if obj_prop.computed {
                        continue;
                    }
                    let key_span = obj_prop.key.span();
                    if key_span.end <= key_span.start {
                        continue;
                    }
                    let key_raw = source[key_span.start as usize..key_span.end as usize].trim();
                    let key = key_raw
                        .strip_prefix('"')
                        .and_then(|s| s.strip_suffix('"'))
                        .or_else(|| {
                            key_raw
                                .strip_prefix('\'')
                                .and_then(|s| s.strip_suffix('\''))
                        })
                        .unwrap_or(key_raw)
                        .trim();
                    if key.is_empty() || !is_simple_ident(key) {
                        continue;
                    }
                    let nested = format!("{}.{}", base_name, key);
                    self.collect_declared_string_values(&nested, &obj_prop.value, source);
                }
            }
            _ => {}
        }
    }

    fn visit_expression(&mut self, expr: &Expression, source: &str) {
        match expr {
            Expression::CallExpression(call) => {
                self.maybe_record_use_template_ref_call(call, source);
                self.visit_expression(&call.callee, source);
                for arg in &call.arguments {
                    if let Some(expr) = arg.as_expression() {
                        self.visit_expression(expr, source);
                    }
                }
            }
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    if let Some(expr) = element.as_expression() {
                        self.visit_expression(expr, source);
                    } else if let oxc_ast::ast::ArrayExpressionElement::SpreadElement(spread) =
                        element
                    {
                        self.visit_expression(&spread.argument, source);
                    }
                }
            }
            Expression::ObjectExpression(obj) => {
                for prop in &obj.properties {
                    match prop {
                        ObjectPropertyKind::ObjectProperty(obj_prop) => {
                            if obj_prop.computed {
                                if let Some(expr) = obj_prop.key.as_expression() {
                                    self.visit_expression(expr, source);
                                }
                            }
                            self.visit_expression(&obj_prop.value, source);
                        }
                        ObjectPropertyKind::SpreadProperty(spread) => {
                            self.visit_expression(&spread.argument, source);
                        }
                    }
                }
            }
            Expression::ArrowFunctionExpression(arrow) => {
                for stmt in &arrow.body.statements {
                    self.visit_statement(stmt, source);
                }
            }
            Expression::FunctionExpression(func) => {
                self.visit_function(func, source);
            }
            Expression::AssignmentExpression(assign) => {
                self.visit_expression(&assign.right, source);
            }
            Expression::BinaryExpression(bin) => {
                self.visit_expression(&bin.left, source);
                self.visit_expression(&bin.right, source);
            }
            Expression::LogicalExpression(logical) => {
                self.visit_expression(&logical.left, source);
                self.visit_expression(&logical.right, source);
            }
            Expression::ConditionalExpression(cond) => {
                self.visit_expression(&cond.test, source);
                self.visit_expression(&cond.consequent, source);
                self.visit_expression(&cond.alternate, source);
            }
            Expression::UnaryExpression(unary) => {
                self.visit_expression(&unary.argument, source);
            }
            Expression::AwaitExpression(await_expr) => {
                self.visit_expression(&await_expr.argument, source);
            }
            Expression::ParenthesizedExpression(paren) => {
                self.visit_expression(&paren.expression, source);
            }
            Expression::StaticMemberExpression(member) => {
                self.visit_expression(&member.object, source);
            }
            Expression::ComputedMemberExpression(member) => {
                self.visit_expression(&member.object, source);
                self.visit_expression(&member.expression, source);
            }
            Expression::PrivateFieldExpression(member) => {
                self.visit_expression(&member.object, source);
            }
            Expression::ChainExpression(chain) => match &chain.expression {
                oxc_ast::ast::ChainElement::CallExpression(call) => {
                    self.visit_expression(&call.callee, source);
                    for arg in &call.arguments {
                        if let Some(expr) = arg.as_expression() {
                            self.visit_expression(expr, source);
                        }
                    }
                }
                oxc_ast::ast::ChainElement::StaticMemberExpression(member) => {
                    self.visit_expression(&member.object, source);
                }
                oxc_ast::ast::ChainElement::ComputedMemberExpression(member) => {
                    self.visit_expression(&member.object, source);
                    self.visit_expression(&member.expression, source);
                }
                oxc_ast::ast::ChainElement::PrivateFieldExpression(member) => {
                    self.visit_expression(&member.object, source);
                }
                oxc_ast::ast::ChainElement::TSNonNullExpression(inner) => {
                    self.visit_expression(&inner.expression, source);
                }
            },
            Expression::TemplateLiteral(tpl) => {
                for expr in &tpl.expressions {
                    self.visit_expression(expr, source);
                }
            }
            Expression::SequenceExpression(seq) => {
                for expr in &seq.expressions {
                    self.visit_expression(expr, source);
                }
            }
            Expression::TSAsExpression(ts_as) => {
                self.visit_expression(&ts_as.expression, source);
            }
            Expression::TSSatisfiesExpression(ts_sat) => {
                self.visit_expression(&ts_sat.expression, source);
            }
            Expression::TSNonNullExpression(ts_non_null) => {
                self.visit_expression(&ts_non_null.expression, source);
            }
            Expression::TSTypeAssertion(ts_assertion) => {
                self.visit_expression(&ts_assertion.expression, source);
            }
            Expression::TSInstantiationExpression(ts_instantiation) => {
                self.visit_expression(&ts_instantiation.expression, source);
            }
            _ => {}
        }
    }

    fn maybe_record_use_template_ref_call(&mut self, call: &CallExpression, source: &str) {
        if call.type_arguments.is_some() {
            return;
        }
        let Some(callee_name) = callee_identifier_name(&call.callee) else {
            return;
        };
        if callee_name != "useTemplateRef" {
            return;
        }

        let selector = call.arguments.first().and_then(|arg| {
            let expr = arg.as_expression()?;
            Some(match unwrap_wrapped_expression(expr) {
                Expression::StringLiteral(lit) => TemplateRefSelector::Arg(lit.value.to_string()),
                other => {
                    let span = other.span();
                    if span.end <= span.start {
                        return None;
                    }
                    let raw = source[span.start as usize..span.end as usize].trim();
                    if raw.is_empty() {
                        return None;
                    }
                    TemplateRefSelector::Arg(raw.to_string())
                }
            })
        });

        self.call_sites.push(TemplateRefCallSite {
            kind: TemplateRefCallKind::UseTemplateRef { selector },
            callee_end: call.callee.span().end,
        });
    }
}

fn callee_identifier_name<'a>(expr: &'a Expression<'a>) -> Option<&'a str> {
    match unwrap_wrapped_expression(expr) {
        Expression::Identifier(ident) => Some(ident.name.as_str()),
        _ => None,
    }
}

fn is_null_argument(arg: &Argument) -> bool {
    matches!(
        arg.as_expression().map(unwrap_wrapped_expression),
        Some(Expression::NullLiteral(_))
    )
}

fn unwrap_wrapped_expression<'a>(expr: &'a Expression<'a>) -> &'a Expression<'a> {
    let mut current = expr;
    loop {
        current = match current {
            Expression::ParenthesizedExpression(p) => &p.expression,
            Expression::TSAsExpression(ts_as) => &ts_as.expression,
            Expression::TSSatisfiesExpression(ts_sat) => &ts_sat.expression,
            Expression::TSNonNullExpression(ts_non_null) => &ts_non_null.expression,
            Expression::TSTypeAssertion(ts_assertion) => &ts_assertion.expression,
            Expression::TSInstantiationExpression(ts_instantiation) => &ts_instantiation.expression,
            _ => break,
        };
    }
    current
}

// ── Script Only (Options API) Processing ──────────────────────────

fn process_tsx_script_only<'alloc>(
    script: &RootNodeScript,
    template_ast: Option<&TemplateAst>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    bindings: &mut FxHashMap<&'alloc str, BindingType>,
    _alloc: &'alloc Allocator,
    _options: &TsxScriptOptions<'_>,
) {
    let content_span = match &script.content {
        Some(span) => span,
        None => return,
    };

    let content_start = content_span.start;
    let content_str = &source[content_span.start as usize..content_span.end as usize];

    // Parse with OXC
    let oxc_alloc = Allocator::default();
    let source_type = SourceType::tsx();
    let parser_ret = Parser::new(&oxc_alloc, content_str, source_type).parse();
    let parse_result = parse_script(
        &parser_ret.program,
        ScriptMode::Options,
        content_start,
        content_str,
    );

    if should_infer_function_types(script.lang) {
        let available_bindings = collect_binding_names(&parse_result.bindings, source, content_str);
        apply_event_handler_param_inference(
            &parser_ret.program.body,
            template_ast,
            source,
            content_start,
            &available_bindings,
            out,
        );
        apply_template_ref_call_inference(
            &parser_ret.program.body,
            template_ast,
            source,
            content_str,
            content_start,
            &available_bindings,
            out,
        );
    }

    // Extract bindings from Options API
    // Same mixed-coordinate issue as script setup — see comment there.
    for (span, bt) in &parse_result.bindings {
        let name = if *bt == BindingType::Props || *bt == BindingType::PropsAliased {
            &source[span.start as usize..span.end as usize]
        } else {
            &content_str[span.start as usize..span.end as usize]
        };
        let alloc_name = out.alloc_str(name);
        bindings.insert(alloc_name, *bt);
    }

    // Remove script tags, pass content through
    out.overwrite(script.tag_open.start, script.tag_open.end, "");
    if let Some(tag_close) = &script.tag_close {
        // Append export default at end
        let mut close = String::with_capacity(32);
        close.push_str("\nexport default __sfc__;\n");
        out.overwrite(tag_close.start, tag_close.end, &close);
    }

    // Convert `export default` to `const __sfc__ =`
    for item in &parse_result.items {
        if let ScriptItem::DefaultExport(de) = item {
            let abs_start = content_start + de.span.start;
            let export_default_text = "export default";
            let replace_end = abs_start + export_default_text.len() as u32;
            out.overwrite(abs_start, replace_end, "const __sfc__ =");
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────

fn emit_minimal_wrapper(out: &mut CodeGenOutput<'_>, options: &TsxScriptOptions<'_>, pos: u32) {
    let wrapper = format!(
        "function __verter_tsx_{}() {{\n  return (<></>\n  )\n}}\n",
        options.js_component_name,
    );
    out.prepend_alloc(pos, &wrapper);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_transform::CodeTransform;

    fn gen_tsx_script(source: &str) -> (String, FxHashMap<String, BindingType>) {
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

        let options = TsxScriptOptions {
            component_name: "App",
            js_component_name: "App",
            scope_id: "data-v-abc123",
            has_scoped_style: false,
            runtime_module_name: "vue",
            is_vapor: false,
        };

        let result = generate_tsx_script(
            syntax.script(),
            syntax.script_setup(),
            syntax.template_ast(),
            source,
            &mut ct,
            &alloc,
            &options,
        );

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

        let code = ct.build_string();
        let bindings: FxHashMap<String, BindingType> = result
            .bindings
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();

        (code, bindings)
    }

    #[test]
    fn basic_script_setup() {
        let (code, bindings) = gen_tsx_script(
            r#"<script setup>
const msg = 'hello'
</script>"#,
        );

        assert!(code.contains("function __verter_tsx_App()"));
        assert!(code.contains("const msg = 'hello'"));
        assert!(bindings.contains_key("msg"));
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
        let fn_pos = code.find("function __verter_tsx_App").unwrap();
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
        let fn_pos = code.find("function __verter_tsx_App").unwrap();
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
            code.contains("function __verter_tsx_App()"),
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
}
