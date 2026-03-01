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
//! // TemplateBinding wrapper function
//! ;function ___VERTER___TemplateBindingFN() {
//!   // Setup body (macros boxed, bindings extracted)
//!   ;type ___VERTER___defineProps_Type=___VERTER___Prettify<Props>;
//!   const props = defineProps<___VERTER___defineProps_Type>()
//!   const count = ref(0)
//!
//!   ;return {...___VERTER___shallowUnwrapRef({
//!     count: count as unknown as typeof count,
//!   }),___VERTER___createMacroReturn({props:{...}})}
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

use crate::ast::types::{AstNodeKind, ElementNode, TagType, TemplateAst};
use crate::code_transform::CodeTransform;
use crate::cursor::ScriptLanguage;
use crate::parser::types::RootNodeScript;
use crate::template::code_gen::binding::{is_simple_ident, BindingType};
use crate::template::code_gen::types::CodeGenOutput;
use crate::utils::oxc::vue::{
    parse_script, parse_script_with_companion, MacroDeclarator, MacroTypeParams, ScriptItem,
    ScriptMacro, ScriptMode,
};

use super::{event_to_jsx_name, get_directive_name, TsxGenericInfo, TsxScriptOptions};

// ── Macro Boxing Types ────────────────────────────────────────────

/// Accumulated macro processing info for TemplateBinding and type constructs.
#[derive(Debug, Default)]
struct TsxMacroState {
    /// Per-macro binding info for createMacroReturn.
    macro_bindings: Vec<MacroBindingEntry>,
    /// DefineModel entries.
    model_bindings: Vec<ModelBindingEntry>,
    /// Whether defineOptions was used (with args), stores the boxed name.
    define_options_boxed: Option<String>,
    /// Set of Box helper imports needed (e.g., "defineProps_Box").
    needed_box_helpers: FxHashSet<String>,
    /// Whether any macro was processed (determines if createMacroReturn import is needed).
    has_macros: bool,
}

/// Info about a boxed macro binding (defineProps, defineEmits, defineSlots, withDefaults).
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct MacroBindingEntry {
    /// Original macro name: "defineProps", "defineEmits", etc.
    macro_name: String,
    /// Variable name holding the macro result (e.g., `props` or `___VERTER___props`).
    var_name: Option<String>,
    /// Type alias name if type params were used (e.g., `___VERTER___defineProps_Type`).
    type_name: Option<String>,
    /// Boxed const name if runtime args were used (e.g., `___VERTER___defineProps_Boxed`).
    boxed_name: Option<String>,
    /// Whether this macro used type params.
    is_type: bool,
}

/// Info about a boxed defineModel binding.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ModelBindingEntry {
    /// Model name (e.g., "modelValue" or "title").
    model_name: String,
    /// Variable name holding the model ref.
    var_name: String,
    /// Type alias name if type params were used.
    type_name: Option<String>,
    /// Boxed const name if runtime args were used.
    boxed_name: Option<String>,
    /// Whether this model used type params.
    is_type: bool,
}

/// Shared context for macro processing functions.
///
/// Groups the 4 parameters threaded through `process_single_macro`,
/// `box_standard_macro`, `process_define_model`, and `process_with_defaults`.
struct MacroSourceCtx<'a, 'alloc> {
    source: &'a str,
    content_str: &'a str,
    content_start: u32,
    out: &'a mut CodeGenOutput<'alloc>,
}

/// Result of TSX script generation (internal, before building string).
pub struct TsxScriptGenResult<'alloc> {
    /// Binding metadata for template TSX generation.
    pub bindings: FxHashMap<&'alloc str, BindingType>,
    /// Type constructs to append after the combined TSX code (no sourcemap).
    /// Concatenated by the caller after source map combination.
    pub type_constructs: String,
    /// Deferred return statement + function close for unified CT mode.
    /// When `template_end` is `Some(...)`, this contains the return+close string
    /// to be applied to the CT AFTER template codegen (to avoid interleaving).
    pub return_close: Option<String>,
}

/// Generate TSX script output from script blocks.
///
/// Returns the generated code, source map, and bindings for template generation.
#[allow(clippy::too_many_arguments)]
pub fn generate_tsx_script<'alloc>(
    script: Option<&RootNodeScript>,
    script_setup: Option<&RootNodeScript>,
    template_ast: Option<&TemplateAst>,
    source: &'alloc str,
    ct: &mut CodeTransform<'alloc>,
    alloc: &'alloc Allocator,
    options: &TsxScriptOptions<'_>,
    template_end: Option<u32>,
) -> TsxScriptGenResult<'alloc> {
    let mut out = CodeGenOutput::new(alloc);
    let mut bindings = FxHashMap::default();
    let mut type_constructs = String::new();
    let builtin_components = collect_builtin_components(template_ast, source);
    let mut return_close: Option<String> = None;

    match (script, script_setup) {
        (_, Some(setup)) => {
            return_close = process_tsx_script_setup(
                setup,
                script,
                template_ast,
                source,
                ct,
                &mut out,
                &mut bindings,
                &mut type_constructs,
                alloc,
                options,
                &builtin_components,
                template_end,
            );
        }
        (Some(normal), None) => {
            process_tsx_script_only(
                normal,
                template_ast,
                source,
                &mut out,
                &mut bindings,
                &mut type_constructs,
                alloc,
                options,
                &builtin_components,
            );
        }
        (None, None) => {
            // No script blocks — emit minimal wrapper + full type constructs
            return_close = emit_minimal_wrapper(&mut out, options, 0, template_end);
            let macro_state = TsxMacroState::default();
            emit_helper_imports(&mut out, 0, &macro_state, options, &builtin_components);
            emit_type_constructs(
                &mut type_constructs,
                &None,        // no generics
                &[],          // no binding names
                &[],          // no import binding names
                &[],          // no declaration texts
                template_ast, // needed for Comp functions
                source,
                options,
                &macro_state,
            );
        }
    }

    // Apply accumulated operations
    out.apply_to(ct);

    TsxScriptGenResult {
        bindings,
        type_constructs,
        return_close,
    }
}

// ── Script Setup Processing ───────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn process_tsx_script_setup<'alloc>(
    setup: &RootNodeScript,
    normal_script: Option<&RootNodeScript>,
    template_ast: Option<&TemplateAst>,
    source: &'alloc str,
    ct: &mut CodeTransform<'alloc>,
    out: &mut CodeGenOutput<'alloc>,
    bindings: &mut FxHashMap<&'alloc str, BindingType>,
    type_constructs: &mut String,
    alloc: &'alloc Allocator,
    options: &TsxScriptOptions<'_>,
    builtin_components: &[&str],
    template_end: Option<u32>,
) -> Option<String> {
    let content_span = match &setup.content {
        Some(span) => span,
        None => {
            // Self-closing <script setup />
            return emit_minimal_wrapper(out, options, setup.tag_open.start, template_end);
        }
    };

    let mut deferred_return_close: Option<String> = None;
    let content_start = content_span.start;
    let content_str = &source[content_span.start as usize..content_span.end as usize];
    // Hoist position: earliest of companion/setup tag starts so imports appear at top.
    let hoist_pos = normal_script
        .map(|ns| ns.tag_open.start.min(setup.tag_open.start))
        .unwrap_or(setup.tag_open.start);

    // Process companion <script> block if present.
    // Hoists imports and type declarations, removes tags and export default,
    // and registers companion import bindings for template resolution.
    if let Some(companion) = normal_script {
        process_companion_for_tsx(companion, source, ct, out, bindings, alloc, hoist_pos);
    }

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

    // Rewrite `<Type>expr` angle bracket assertions to `(expr as Type)` for TSX validity.
    rewrite_ts_type_assertions(content_str, content_start, ct);

    // Hoist imports to file top (before component wrapper).
    // Uses move_with_suffix to preserve sourcemap mappings — the moved content
    // produces Moved chunks that emit per-line source map tokens back to the
    // original SFC positions, unlike prepend_alloc which creates unmapped Inserted chunks.
    for item in &parse_result.items {
        if let ScriptItem::Import(imp) = item {
            let abs_start = content_start + imp.span.start;
            let abs_end = content_start + imp.span.end;
            ct.move_with_suffix(abs_start, abs_end, hoist_pos, "\n");
        }
    }

    // Hoist type declarations to file top (preserving sourcemap).
    for item in &parse_result.items {
        if let ScriptItem::TypeDeclaration(td) = item {
            let abs_start = content_start + td.span.start;
            let abs_end = content_start + td.span.end;
            ct.move_with_suffix(abs_start, abs_end, hoist_pos, "\n");
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

    // Parse generic attribute if present.
    // If parsing fails (invalid TS syntax), fall back to the raw string so
    // TypeScript can surface the actual error to the user.
    let (generic_info, raw_generic) = if let Some(span) = setup.generic {
        let generic_str = &source[span.start as usize..span.end as usize];
        let trimmed = generic_str.trim();
        if trimmed.is_empty() {
            (None, None)
        } else {
            match TsxGenericInfo::from_source(generic_str) {
                Some(info) => (Some(info), None),
                None => (None, Some(trimmed.to_string())),
            }
        }
    } else {
        (None, None)
    };

    // Process macros: box type params and runtime args
    let mut macro_ctx = MacroSourceCtx {
        source,
        content_str,
        content_start,
        out,
    };
    let macro_state = process_macros(&parse_result.items, &mut macro_ctx);
    let out = macro_ctx.out;

    // Build component function wrapper opening
    // Replace <script setup> tag with ___VERTER___TemplateBindingFN function declaration.
    // Use parsed generic if available, otherwise raw fallback for invalid syntax.
    let generic_bracket = generic_info
        .as_ref()
        .map(|g| g.source_bracket())
        .or_else(|| raw_generic.as_ref().map(|r| format!("<{}>", r)))
        .unwrap_or_default();
    let async_prefix = if parse_result.is_async { "async " } else { "" };
    let wrapper_start = format!(
        ";{}function {}TemplateBindingFN{}() {{\n",
        async_prefix, PREFIX, generic_bracket,
    );
    out.overwrite(setup.tag_open.start, setup.tag_open.end, &wrapper_start);

    // Replace </script> tag with setup suffix; emit return + close at template end
    if let Some(tag_close) = &setup.tag_close {
        let mut wrapper_end = String::with_capacity(256);

        // Inject TemplateBinding return statement with bindings and macro metadata
        let binding_entries: Vec<String> = bindings
            .keys()
            .map(|name| format!("{name}: {name} as unknown as typeof {name}", name = name))
            .collect();

        // Model returns: unwrap ModelRef
        let model_entries: Vec<String> = macro_state
            .model_bindings
            .iter()
            .map(|m| {
                format!(
                    "{}: {{}} as typeof {} extends import('vue').ModelRef<infer V> ? V extends boolean|undefined ? boolean : V & {{b: 1}} : import('vue').UnwrapRef<typeof {}> & {{a: 1}}",
                    m.model_name, m.var_name, m.var_name,
                )
            })
            .collect();

        // Props spread for TemplateBinding return
        let props_return: Vec<String> = macro_state
            .macro_bindings
            .iter()
            .filter(|e| e.macro_name == "defineProps" && e.var_name.is_some())
            .map(|e| {
                let var = e
                    .var_name
                    .as_ref()
                    .expect("invariant: filtered by var_name.is_some()");
                let key_type = if let Some(t) = &e.type_name {
                    format!("keyof {}", t)
                } else {
                    format!("keyof typeof {}", var)
                };
                format!("...({{}} as Pick<typeof {}, {}>)", var, key_type)
            })
            .collect();

        // Build macro return content
        let macro_return_str = if macro_state.has_macros {
            let content = build_macro_return_content(&macro_state);
            format!(",{}createMacroReturn({})", PREFIX, content)
        } else {
            String::new()
        };

        // Combine all return entries
        let all_return_entries: Vec<&str> = props_return
            .iter()
            .map(|s| s.as_str())
            .chain(binding_entries.iter().map(|s| s.as_str()))
            .chain(model_entries.iter().map(|s| s.as_str()))
            .filter(|s| !s.is_empty())
            .collect();

        // Inject __props alias so template codegen's `__props.xxx` references resolve.
        // BindingResolver emits `__props.xxx` for Props bindings, but only `props` (or
        // the user's variable) is declared by the macro expansion above.
        if let Some(props_var) = macro_state
            .macro_bindings
            .iter()
            .find(|e| e.macro_name == "defineProps")
            .and_then(|e| e.var_name.as_deref())
        {
            wrapper_end.push_str(&format!("\nconst __props = {};", props_var));
        }

        // Build return + function close string
        let return_close = format!(
            "\n;return {{...{}shallowUnwrapRef({{{}}}){}}}",
            PREFIX,
            all_return_entries.join(",\n"),
            if macro_return_str.is_empty() {
                String::new()
            } else {
                format!("\n{}", macro_return_str)
            },
        );

        if template_end.is_some() {
            // Unified CT: script wrapper extends to template end.
            // At </script>: emit only the setup suffix (e.g., __props alias).
            wrapper_end.push('\n');
            out.overwrite(tag_close.start, tag_close.end, &wrapper_end);

            // Return+close is deferred — compile/mod.rs will apply it after
            // template codegen to avoid interleaving with template mutations.
            deferred_return_close = Some({
                let mut tail = return_close;
                tail.push_str("\n}\n");
                tail
            });
        } else {
            // No template block: emit everything at </script>.
            wrapper_end.push_str(&return_close);
            wrapper_end.push_str("\n}\n");
            out.overwrite(tag_close.start, tag_close.end, &wrapper_end);
        }
    }

    // Collect binding names for FullContext emission
    let binding_names: Vec<String> = bindings.keys().map(|k| k.to_string()).collect();

    // Collect declaration source texts for FullContext body
    let declaration_texts: Vec<String> = parse_result
        .items
        .iter()
        .filter_map(|item| {
            if let ScriptItem::Declaration(decl) = item {
                let abs_start = content_start + decl.span.start;
                let abs_end = content_start + decl.span.end;
                Some(source[abs_start as usize..abs_end as usize].to_string())
            } else {
                None
            }
        })
        .collect();

    // Collect import binding names for FullContext
    let import_binding_names: Vec<String> = parse_result
        .items
        .iter()
        .filter_map(|item| {
            if let ScriptItem::Import(imp) = item {
                Some(
                    imp.bindings
                        .iter()
                        .map(|b| b.name.to_string())
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            }
        })
        .flatten()
        .collect();

    // Emit helper imports (hoisted before wrapper)
    emit_helper_imports(out, hoist_pos, &macro_state, options, builtin_components);

    // Emit type constructs (appended after source map, no sourcemap needed)
    emit_type_constructs(
        type_constructs,
        &generic_info,
        &binding_names,
        &import_binding_names,
        &declaration_texts,
        template_ast,
        source,
        options,
        &macro_state,
    );

    deferred_return_close
}

// ── Companion Script Processing ──────────────────────────────────

/// Process a companion `<script>` block for TSX output.
///
/// When both `<script>` and `<script setup>` exist, the companion script
/// needs to be integrated into the TSX output:
/// 1. Remove `<script>` and `</script>` tags (they're not valid TSX)
/// 2. Hoist imports to file top (same as setup imports)
/// 3. Hoist type declarations to file top
/// 4. Remove `export default { ... }` (runtime-only Options API config)
/// 5. Register non-type import bindings for template resolution
fn process_companion_for_tsx<'alloc>(
    companion: &RootNodeScript,
    source: &str,
    ct: &mut CodeTransform<'_>,
    out: &mut CodeGenOutput<'alloc>,
    bindings: &mut FxHashMap<&'alloc str, BindingType>,
    alloc: &'alloc Allocator,
    hoist_pos: u32,
) {
    // Remove companion <script> open tag
    ct.remove(companion.tag_open.start, companion.tag_open.end);
    // Remove companion </script> close tag
    if let Some(tag_close) = &companion.tag_close {
        ct.remove(tag_close.start, tag_close.end);
    }

    let content_span = match &companion.content {
        Some(span) => span,
        None => return, // Self-closing <script /> — nothing to process
    };

    let comp_start = content_span.start;
    let comp_str = &source[content_span.start as usize..content_span.end as usize];

    // Parse companion content with OXC
    let oxc_alloc = Allocator::default();
    let source_type = SourceType::tsx();
    let parser_ret = Parser::new(&oxc_alloc, comp_str, source_type).parse();
    let parse_result = parse_script(
        &parser_ret.program,
        ScriptMode::Options,
        comp_start,
        comp_str,
    );

    // Hoist imports to file top (preserving sourcemap).
    for item in &parse_result.items {
        if let ScriptItem::Import(imp) = item {
            let abs_start = comp_start + imp.span.start;
            let abs_end = comp_start + imp.span.end;
            ct.move_with_suffix(abs_start, abs_end, hoist_pos, "\n");

            // Register non-type import bindings for template resolution.
            // Companion imports (e.g., component imports) need to be in the
            // bindings map so template binding resolution works.
            if !imp.is_type_only {
                for binding in &imp.bindings {
                    if !binding.is_type_only {
                        let alloc_name = alloc.alloc_str(binding.name);
                        bindings
                            .entry(alloc_name)
                            .or_insert(BindingType::SetupImport);
                    }
                }
            }
        }
    }

    // Hoist type declarations to file top (preserving sourcemap).
    for item in &parse_result.items {
        if let ScriptItem::TypeDeclaration(td) = item {
            let abs_start = comp_start + td.span.start;
            let abs_end = comp_start + td.span.end;
            ct.move_with_suffix(abs_start, abs_end, hoist_pos, "\n");
        }
    }

    // Remove `export default { ... }` — runtime-only Options API config.
    for item in &parse_result.items {
        if let ScriptItem::DefaultExport(de) = item {
            let abs_start = comp_start + de.span.start;
            let abs_end = comp_start + de.span.end;
            out.overwrite(abs_start, abs_end, "");
        }
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

// get_directive_name and event_to_jsx_name are imported from super (tsx/mod.rs)

fn should_infer_function_types(lang: Option<ScriptLanguage>) -> bool {
    matches!(lang, Some(ScriptLanguage::TypeScript | ScriptLanguage::TSX))
}

/// Rewrite `<Type>expr` angle bracket type assertions to `(expr as Type)` for TSX validity.
///
/// TypeScript's `TSTypeAssertion` syntax (`<string>foo`) is ambiguous with JSX elements
/// in TSX files. This rewrites them to the equivalent `as` syntax: `(foo as string)`.
///
/// Since the main parse uses TSX mode (where `<T>expr` is parsed as JSX, not as a type
/// assertion), we perform a separate lightweight TS parse to correctly detect them.
fn rewrite_ts_type_assertions(content_str: &str, content_start: u32, ct: &mut CodeTransform<'_>) {
    // Parse as TypeScript (not TSX) so OXC produces TSTypeAssertion nodes
    let ts_alloc = Allocator::default();
    let ts_source_type = SourceType::ts();
    let ts_ret = Parser::new(&ts_alloc, content_str, ts_source_type).parse();

    let mut assertions: Vec<(u32, u32, u32)> = Vec::new(); // (assertion_start, expr_start, assertion_end)
    collect_type_assertions_from_stmts(&ts_ret.program.body, &mut assertions);

    if assertions.is_empty() {
        return;
    }

    for &(assertion_start, expr_start, assertion_end) in &assertions {
        // Extract type text from between `<` and `>`
        // The range `assertion_start..expr_start` in content_str is `<Type>`
        let type_text = &content_str[(assertion_start + 1) as usize..(expr_start - 1) as usize];

        let abs_start = content_start + assertion_start;
        let abs_expr_start = content_start + expr_start;
        let abs_end = content_start + assertion_end;

        // Replace `<Type>` with `(`
        ct.overwrite(abs_start, abs_expr_start, "(");
        // Append ` as Type)` after the expression
        ct.append_left(abs_end, &format!(" as {})", type_text));
    }
}

fn collect_type_assertions_from_stmts(
    stmts: &[oxc_ast::ast::Statement<'_>],
    out: &mut Vec<(u32, u32, u32)>,
) {
    for stmt in stmts {
        collect_type_assertions_from_stmt(stmt, out);
    }
}

fn collect_type_assertions_from_stmt(
    stmt: &oxc_ast::ast::Statement<'_>,
    out: &mut Vec<(u32, u32, u32)>,
) {
    use oxc_ast::ast::*;
    match stmt {
        Statement::ExpressionStatement(es) => {
            collect_type_assertions_from_expr(&es.expression, out);
        }
        Statement::VariableDeclaration(vd) => {
            for decl in &vd.declarations {
                if let Some(init) = &decl.init {
                    collect_type_assertions_from_expr(init, out);
                }
            }
        }
        Statement::ReturnStatement(ret) => {
            if let Some(arg) = &ret.argument {
                collect_type_assertions_from_expr(arg, out);
            }
        }
        Statement::IfStatement(ifs) => {
            collect_type_assertions_from_expr(&ifs.test, out);
            collect_type_assertions_from_stmt(&ifs.consequent, out);
            if let Some(alt) = &ifs.alternate {
                collect_type_assertions_from_stmt(alt, out);
            }
        }
        Statement::BlockStatement(block) => {
            collect_type_assertions_from_stmts(&block.body, out);
        }
        Statement::ForStatement(fs) => {
            if let Some(body) = Some(&fs.body) {
                collect_type_assertions_from_stmt(body, out);
            }
        }
        Statement::WhileStatement(ws) => {
            collect_type_assertions_from_expr(&ws.test, out);
            collect_type_assertions_from_stmt(&ws.body, out);
        }
        _ => {}
    }
}

fn collect_type_assertions_from_expr(
    expr: &oxc_ast::ast::Expression<'_>,
    out: &mut Vec<(u32, u32, u32)>,
) {
    use oxc_ast::ast::*;
    match expr {
        Expression::TSTypeAssertion(ta) => {
            // Record this assertion (process inner first for nesting)
            collect_type_assertions_from_expr(&ta.expression, out);
            out.push((ta.span.start, ta.expression.span().start, ta.span.end));
        }
        Expression::AssignmentExpression(ae) => {
            collect_type_assertions_from_expr(&ae.right, out);
        }
        Expression::BinaryExpression(be) => {
            collect_type_assertions_from_expr(&be.left, out);
            collect_type_assertions_from_expr(&be.right, out);
        }
        Expression::LogicalExpression(le) => {
            collect_type_assertions_from_expr(&le.left, out);
            collect_type_assertions_from_expr(&le.right, out);
        }
        Expression::ConditionalExpression(ce) => {
            collect_type_assertions_from_expr(&ce.test, out);
            collect_type_assertions_from_expr(&ce.consequent, out);
            collect_type_assertions_from_expr(&ce.alternate, out);
        }
        Expression::CallExpression(call) => {
            collect_type_assertions_from_expr(&call.callee, out);
            for arg in &call.arguments {
                if let Argument::SpreadElement(spread) = arg {
                    collect_type_assertions_from_expr(&spread.argument, out);
                } else {
                    collect_type_assertions_from_expr(arg.to_expression(), out);
                }
            }
        }
        Expression::ParenthesizedExpression(pe) => {
            collect_type_assertions_from_expr(&pe.expression, out);
        }
        Expression::SequenceExpression(se) => {
            for e in &se.expressions {
                collect_type_assertions_from_expr(e, out);
            }
        }
        Expression::ArrayExpression(ae) => {
            for el in &ae.elements {
                match el {
                    ArrayExpressionElement::SpreadElement(spread) => {
                        collect_type_assertions_from_expr(&spread.argument, out);
                    }
                    ArrayExpressionElement::TSTypeAssertion(ta) => {
                        collect_type_assertions_from_expr(&ta.expression, out);
                        out.push((ta.span.start, ta.expression.span().start, ta.span.end));
                    }
                    _ => {}
                }
            }
        }
        Expression::ObjectExpression(oe) => {
            for prop in &oe.properties {
                if let ObjectPropertyKind::ObjectProperty(op) = prop {
                    collect_type_assertions_from_expr(&op.value, out);
                }
            }
        }
        Expression::ArrowFunctionExpression(afe) => {
            collect_type_assertions_from_stmts(&afe.body.statements, out);
        }
        Expression::TSAsExpression(tsa) => {
            collect_type_assertions_from_expr(&tsa.expression, out);
        }
        Expression::TSSatisfiesExpression(tss) => {
            collect_type_assertions_from_expr(&tss.expression, out);
        }
        Expression::TSNonNullExpression(tsnn) => {
            collect_type_assertions_from_expr(&tsnn.expression, out);
        }
        Expression::AwaitExpression(ae) => {
            collect_type_assertions_from_expr(&ae.argument, out);
        }
        Expression::UnaryExpression(ue) => {
            collect_type_assertions_from_expr(&ue.argument, out);
        }
        Expression::TemplateLiteral(tl) => {
            for expr in &tl.expressions {
                collect_type_assertions_from_expr(expr, out);
            }
        }
        Expression::ComputedMemberExpression(cme) => {
            collect_type_assertions_from_expr(&cme.object, out);
            collect_type_assertions_from_expr(&cme.expression, out);
        }
        Expression::StaticMemberExpression(sme) => {
            collect_type_assertions_from_expr(&sme.object, out);
        }
        Expression::PrivateFieldExpression(pfe) => {
            collect_type_assertions_from_expr(&pfe.object, out);
        }
        _ => {}
    }
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

#[allow(clippy::too_many_arguments)]
fn process_tsx_script_only<'alloc>(
    script: &RootNodeScript,
    template_ast: Option<&TemplateAst>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    bindings: &mut FxHashMap<&'alloc str, BindingType>,
    type_constructs: &mut String,
    _alloc: &'alloc Allocator,
    options: &TsxScriptOptions<'_>,
    builtin_components: &[&str],
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

    // Remove script tags, emit wrapper + content
    // The Options API wraps the script content in a TemplateBindingFN for type construct parity.
    let hoist_pos = script.tag_open.start;
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

    // Emit helper imports + type constructs (same as template-only and setup paths)
    let macro_state = TsxMacroState::default();
    emit_helper_imports(out, hoist_pos, &macro_state, options, builtin_components);

    // Collect binding names for type constructs
    let binding_names: Vec<String> = bindings.keys().map(|k| k.to_string()).collect();

    // Collect import binding names
    let import_binding_names: Vec<String> = parse_result
        .items
        .iter()
        .filter_map(|item| {
            if let ScriptItem::Import(imp) = item {
                Some(
                    imp.bindings
                        .iter()
                        .map(|b| b.name.to_string())
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            }
        })
        .flatten()
        .collect();

    // Collect declaration source texts
    let declaration_texts: Vec<String> = parse_result
        .items
        .iter()
        .filter_map(|item| {
            if let ScriptItem::Declaration(decl) = item {
                let abs_start = content_start + decl.span.start;
                let abs_end = content_start + decl.span.end;
                Some(source[abs_start as usize..abs_end as usize].to_string())
            } else {
                None
            }
        })
        .collect();

    emit_type_constructs(
        type_constructs,
        &None, // no generics (Options API can't have them)
        &binding_names,
        &import_binding_names,
        &declaration_texts,
        template_ast,
        source,
        options,
        &macro_state,
    );
}

// ── Macro Boxing ──────────────────────────────────────────────────

/// Process all macros in the parsed items: box type params and runtime args.
fn process_macros(items: &[ScriptItem<'_>], ctx: &mut MacroSourceCtx<'_, '_>) -> TsxMacroState {
    let mut state = TsxMacroState::default();

    for item in items {
        if let ScriptItem::Macro(mac) = item {
            process_single_macro(mac, ctx, &mut state);
        }
    }

    state
}

/// Process a single macro call: box its type params and/or runtime args.
fn process_single_macro(
    mac: &ScriptMacro<'_>,
    ctx: &mut MacroSourceCtx<'_, '_>,
    state: &mut TsxMacroState,
) {
    state.has_macros = true;

    match mac {
        ScriptMacro::DefineProps {
            span,
            declarator,
            type_params,
            object_arg,
            array_arg,
        } => {
            let arg_span = object_arg
                .as_ref()
                .map(|o| o.span)
                .or(array_arg.as_ref().map(|a| a.span));
            let entry = box_standard_macro(
                "defineProps",
                "props",
                *span,
                declarator.as_ref(),
                type_params.as_ref(),
                arg_span,
                false,
                ctx,
                &mut state.needed_box_helpers,
            );
            state.macro_bindings.push(entry);
        }
        ScriptMacro::DefineEmits {
            span,
            declarator,
            type_params,
            object_arg,
            array_arg,
        } => {
            let arg_span = object_arg
                .as_ref()
                .map(|o| o.span)
                .or(array_arg.as_ref().map(|a| a.span));
            let entry = box_standard_macro(
                "defineEmits",
                "emits",
                *span,
                declarator.as_ref(),
                type_params.as_ref(),
                arg_span,
                false,
                ctx,
                &mut state.needed_box_helpers,
            );
            state.macro_bindings.push(entry);
        }
        ScriptMacro::DefineSlots {
            span,
            declarator,
            type_params,
        } => {
            let entry = box_standard_macro(
                "defineSlots",
                "slots",
                *span,
                declarator.as_ref(),
                type_params.as_ref(),
                None,
                false,
                ctx,
                &mut state.needed_box_helpers,
            );
            state.macro_bindings.push(entry);
        }
        ScriptMacro::DefineExpose {
            span,
            declarator,
            object_arg,
        } => {
            let arg_span = object_arg.as_ref().map(|o| o.span);
            // defineExpose is a no-return macro (no variable created when no declarator)
            let entry = box_standard_macro(
                "defineExpose",
                "expose",
                *span,
                declarator.as_ref(),
                None,
                arg_span,
                true,
                ctx,
                &mut state.needed_box_helpers,
            );
            state.macro_bindings.push(entry);
        }
        ScriptMacro::DefineOptions {
            span,
            declarator,
            object_arg,
        } => {
            let arg_span = object_arg.as_ref().map(|o| o.span);
            // defineOptions is a no-return macro
            let entry = box_standard_macro(
                "defineOptions",
                "options",
                *span,
                declarator.as_ref(),
                None,
                arg_span,
                true,
                ctx,
                &mut state.needed_box_helpers,
            );
            if entry.boxed_name.is_some() {
                state.define_options_boxed = entry.boxed_name.clone();
            }
            state.macro_bindings.push(entry);
        }
        ScriptMacro::DefineModel {
            span,
            declarator,
            type_params,
            name_span,
            options_span,
        } => {
            process_define_model(
                *span,
                declarator.as_ref(),
                type_params.as_ref(),
                *name_span,
                *options_span,
                ctx,
                state,
            );
        }
        ScriptMacro::WithDefaults {
            span,
            declarator,
            define_props_span,
            define_props_type_params,
            defaults: _,
            defaults_arg_span,
        } => {
            process_with_defaults(
                *span,
                declarator.as_ref(),
                *define_props_span,
                define_props_type_params.as_ref(),
                *defaults_arg_span,
                ctx,
                state,
            );
        }
    }
}

/// Box a standard macro (defineProps, defineEmits, defineSlots, defineExpose, defineOptions).
///
/// For type params: emit type alias, replace type param in call with alias name.
/// For runtime args: emit boxed const, replace arg in call with boxed name.
/// For no-declarator non-no-return macros: prepend `const ___VERTER___xxx=`.
#[allow(clippy::too_many_arguments)]
fn box_standard_macro(
    macro_name: &str,
    var_suffix: &str,
    call_span: crate::common::Span,
    declarator: Option<&MacroDeclarator<'_>>,
    type_params: Option<&MacroTypeParams>,
    arg_span: Option<crate::common::Span>,
    is_no_return: bool,
    ctx: &mut MacroSourceCtx<'_, '_>,
    needed_helpers: &mut FxHashSet<String>,
) -> MacroBindingEntry {
    let type_name_str = format!("{}{}_Type", PREFIX, macro_name);
    let boxed_name_str = format!("{}{}_Boxed", PREFIX, macro_name);
    let box_fn_name = format!("{}{}_Box", PREFIX, macro_name);
    let auto_var_name = format!("{}{}", PREFIX, var_suffix);

    let has_type_params = type_params.is_some();
    let has_args = arg_span.is_some();

    // Determine the statement start position for prepending
    let stmt_start = declarator
        .map(|d| ctx.content_start + d.statement_span.start)
        .unwrap_or(ctx.content_start + call_span.start);

    // 1. Box type params: emit type alias, replace type param content with alias name
    if let Some(tp) = type_params {
        // type_params spans are already absolute (include content_offset)
        let type_text = &ctx.source[tp.type_span.start as usize..tp.type_span.end as usize];
        let needs_prettify = !is_simple_type_reference(type_text);

        let type_decl = if needs_prettify {
            format!(";type {}={}Prettify<{}>;", type_name_str, PREFIX, type_text)
        } else {
            format!(";type {}={};", type_name_str, type_text)
        };
        ctx.out.prepend_alloc(stmt_start, &type_decl);

        // Replace type arg content with alias name (type_span is already absolute)
        ctx.out
            .overwrite(tp.type_span.start, tp.type_span.end, &type_name_str);

        needed_helpers.insert(format!("{}_Box", macro_name));
    }

    // 2. Box runtime args: emit boxed const, replace arg with boxed name
    if let Some(a_span) = arg_span {
        // arg spans are relative to content_str
        let arg_text = &ctx.content_str[a_span.start as usize..a_span.end as usize];
        let abs_arg_start = ctx.content_start + a_span.start;
        let abs_arg_end = ctx.content_start + a_span.end;

        let boxed_decl = format!(";const {}={}({});", boxed_name_str, box_fn_name, arg_text);
        ctx.out.prepend_alloc(stmt_start, &boxed_decl);

        // Replace arg with boxed name
        ctx.out
            .overwrite(abs_arg_start, abs_arg_end, &boxed_name_str);

        needed_helpers.insert(format!("{}_Box", macro_name));
    }

    // 3. Add variable assignment if no declarator and not a no-return macro
    if declarator.is_none() && !is_no_return {
        let call_abs_start = ctx.content_start + call_span.start;
        ctx.out
            .prepend_alloc(call_abs_start, &format!("const {}=", auto_var_name));
    }

    // Determine the effective variable name
    let effective_var_name = if is_no_return && declarator.is_none() {
        None
    } else {
        Some(
            declarator
                .and_then(|d| d.name.map(|n| n.to_string()))
                .unwrap_or_else(|| auto_var_name.clone()),
        )
    };

    MacroBindingEntry {
        macro_name: macro_name.to_string(),
        var_name: effective_var_name,
        type_name: if has_type_params {
            Some(type_name_str)
        } else {
            None
        },
        boxed_name: if has_args { Some(boxed_name_str) } else { None },
        is_type: has_type_params,
    }
}

/// Process defineModel macro: handles named models with tuple spread.
fn process_define_model(
    call_span: crate::common::Span,
    declarator: Option<&MacroDeclarator<'_>>,
    type_params: Option<&MacroTypeParams>,
    name_span: Option<crate::common::Span>,
    options_span: Option<crate::common::Span>,
    ctx: &mut MacroSourceCtx<'_, '_>,
    state: &mut TsxMacroState,
) {
    // Determine model name
    let model_name = if let Some(ns) = name_span {
        // name_span is relative to content_str
        let name_text = &ctx.content_str[ns.start as usize..ns.end as usize];
        // Strip quotes
        name_text.trim_matches('\'').trim_matches('"').to_string()
    } else {
        "modelValue".to_string()
    };

    let prepend = format!("{}_", model_name);
    let type_name_str = format!("{}{}defineModel_Type", PREFIX, prepend);
    let boxed_name_str = format!("{}{}defineModel_Boxed", PREFIX, prepend);
    // TS v5 parity: box function is always shared `___VERTER___defineModel_Box`,
    // NOT per-model `___VERTER___title_defineModel_Box`. The per-model prefix
    // only applies to the Boxed const and Type alias.
    let box_fn_name = format!("{}defineModel_Box", PREFIX);
    let auto_var_name = format!("{}models_{}", PREFIX, model_name);

    let has_type_params = type_params.is_some();
    let has_args = name_span.is_some() || options_span.is_some();

    let stmt_start = declarator
        .map(|d| ctx.content_start + d.statement_span.start)
        .unwrap_or(ctx.content_start + call_span.start);

    // Box type params
    if let Some(tp) = type_params {
        let type_text = &ctx.source[tp.type_span.start as usize..tp.type_span.end as usize];
        let needs_prettify = !is_simple_type_reference(type_text);

        let type_decl = if needs_prettify {
            format!(";type {}={}Prettify<{}>;", type_name_str, PREFIX, type_text)
        } else {
            format!(";type {}={};", type_name_str, type_text)
        };
        ctx.out.prepend_alloc(stmt_start, &type_decl);

        // Replace type arg content with alias name
        ctx.out
            .overwrite(tp.type_span.start, tp.type_span.end, &type_name_str);

        state
            .needed_box_helpers
            .insert("defineModel_Box".to_string());
    }

    // Box runtime args (name + options): defineModel_Box returns tuple [name, options]
    if has_args {
        let mut args_text = String::new();
        if let Some(ns) = name_span {
            let text = &ctx.content_str[ns.start as usize..ns.end as usize];
            args_text.push_str(text);
        }
        if let Some(os) = options_span {
            if !args_text.is_empty() {
                args_text.push_str(", ");
            }
            let text = &ctx.content_str[os.start as usize..os.end as usize];
            args_text.push_str(text);
        }

        // Emit boxed const — include type params if present (TS v5 parity)
        let type_param_str = if let Some(tp) = type_params {
            let type_text = &ctx.source[tp.type_span.start as usize..tp.type_span.end as usize];
            format!("<{}>", type_text)
        } else {
            String::new()
        };
        let boxed_decl = format!(
            ";const {}={}{}({});",
            boxed_name_str, box_fn_name, type_param_str, args_text
        );
        ctx.out.prepend_alloc(stmt_start, &boxed_decl);

        // Replace all args in the call with spread from boxed tuple
        // For named model: defineModel('name', opts) → defineModel(Boxed[0], Boxed[1])
        let first_arg_start = name_span.unwrap_or_else(|| {
            options_span.expect("invariant: has_args guarantees at least one span")
        });
        let last_arg_end = options_span.unwrap_or_else(|| {
            name_span.expect("invariant: has_args guarantees at least one span")
        });
        let abs_first = ctx.content_start + first_arg_start.start;
        let abs_last = ctx.content_start + last_arg_end.end;

        // TS v5 parity: ALWAYS use [0],[1] indexing for defineModel args,
        // regardless of whether it's name-only, options-only, or both.
        // defineModel_Box returns a tuple and the call must spread it back.
        let replacement = format!("{}[0],{}[1]", boxed_name_str, boxed_name_str);
        ctx.out.overwrite(abs_first, abs_last, &replacement);

        state
            .needed_box_helpers
            .insert("defineModel_Box".to_string());
    }

    // Add variable assignment if no declarator
    if declarator.is_none() {
        let call_abs_start = ctx.content_start + call_span.start;
        ctx.out
            .prepend_alloc(call_abs_start, &format!("const {}=", auto_var_name));
    }

    let effective_var_name = declarator
        .and_then(|d| d.name.map(|n| n.to_string()))
        .unwrap_or_else(|| auto_var_name.clone());

    state.model_bindings.push(ModelBindingEntry {
        model_name,
        var_name: effective_var_name,
        type_name: if has_type_params {
            Some(type_name_str)
        } else {
            None
        },
        boxed_name: if has_args { Some(boxed_name_str) } else { None },
        is_type: has_type_params,
    });
}

/// Process withDefaults(defineProps<T>(), { defaults }).
fn process_with_defaults(
    call_span: crate::common::Span,
    declarator: Option<&MacroDeclarator<'_>>,
    define_props_span: Option<crate::common::Span>,
    define_props_type_params: Option<&MacroTypeParams>,
    defaults_arg_span: Option<crate::common::Span>,
    ctx: &mut MacroSourceCtx<'_, '_>,
    state: &mut TsxMacroState,
) {
    let type_name_str = format!("{}defineProps_Type", PREFIX);
    let wd_boxed_name = format!("{}withDefaults_Boxed", PREFIX);
    let wd_box_fn = format!("{}withDefaults_Box", PREFIX);
    let auto_var_name = format!("{}props", PREFIX);

    let has_type_params = define_props_type_params.is_some();

    let stmt_start = declarator
        .map(|d| ctx.content_start + d.statement_span.start)
        .unwrap_or(ctx.content_start + call_span.start);

    // Box inner defineProps type params
    if let Some(tp) = define_props_type_params {
        let type_text = &ctx.source[tp.type_span.start as usize..tp.type_span.end as usize];
        let needs_prettify = !is_simple_type_reference(type_text);

        let type_decl = if needs_prettify {
            format!(";type {}={}Prettify<{}>;", type_name_str, PREFIX, type_text)
        } else {
            format!(";type {}={};", type_name_str, type_text)
        };
        ctx.out.prepend_alloc(stmt_start, &type_decl);

        // Replace type arg in the inner defineProps
        ctx.out
            .overwrite(tp.type_span.start, tp.type_span.end, &type_name_str);

        state
            .needed_box_helpers
            .insert("defineProps_Box".to_string());
    }

    // Names for boxing the inner defineProps args (runtime props case)
    let dp_boxed_name = format!("{}defineProps_Boxed", PREFIX);
    let dp_box_fn = format!("{}defineProps_Box", PREFIX);

    // Box defaults arg — withDefaults_Box(propsCall, defaultsObj)
    let has_defaults = defaults_arg_span.is_some();
    // Track whether we boxed the inner defineProps args (runtime props only)
    let mut boxed_define_props = false;

    if let Some(d_span) = defaults_arg_span {
        let arg_text = &ctx.content_str[d_span.start as usize..d_span.end as usize];

        // Build the first arg (the defineProps call text for the boxed declaration)
        let dp_call_text = if has_type_params {
            // Type was already aliased above — use the alias name
            format!("defineProps<{}>()", type_name_str)
        } else if let Some(dp_span) = define_props_span {
            // Runtime props: wrap inner args with defineProps_Box and capture in dp_boxed_name
            // TS v5 parity: defineProps(DP_Boxed=DP_Box({bar: String}))
            let full_dp_call =
                ctx.content_str[dp_span.start as usize..dp_span.end as usize].to_string();
            if let Some(paren_pos) = full_dp_call.find('(') {
                let dp_args = &full_dp_call[paren_pos + 1..full_dp_call.len() - 1];
                if !dp_args.trim().is_empty() {
                    boxed_define_props = true;
                    state
                        .needed_box_helpers
                        .insert("defineProps_Box".to_string());
                    format!("defineProps({}={}({}))", dp_boxed_name, dp_box_fn, dp_args)
                } else {
                    full_dp_call
                }
            } else {
                full_dp_call
            }
        } else {
            "defineProps()".to_string()
        };

        // Build the boxed declaration, optionally prepended by `let dp_boxed_name;`
        let let_decl = if boxed_define_props {
            format!(";let {};", dp_boxed_name)
        } else {
            String::new()
        };
        let boxed_decl = format!(
            "{}const {}={}({}, {});",
            let_decl, wd_boxed_name, wd_box_fn, dp_call_text, arg_text
        );
        ctx.out.prepend_alloc(stmt_start, &boxed_decl);

        // TS v5 parity: overwrite both args of withDefaults with [0]/[1] indexing.
        // Replace from defineProps call start to defaults arg end.
        let replacement = format!("{}[0],{}[1]", wd_boxed_name, wd_boxed_name);
        if let Some(dp_span) = define_props_span {
            let dp_abs_start = ctx.content_start + dp_span.start;
            let def_abs_end = ctx.content_start + d_span.end;
            ctx.out.overwrite(dp_abs_start, def_abs_end, &replacement);
        } else {
            // Fallback: only overwrite the defaults arg
            let abs_start = ctx.content_start + d_span.start;
            let abs_end = ctx.content_start + d_span.end;
            ctx.out.overwrite(abs_start, abs_end, &replacement);
        }

        state
            .needed_box_helpers
            .insert("withDefaults_Box".to_string());
    }

    // Add variable assignment if no declarator
    if declarator.is_none() {
        let call_abs_start = ctx.content_start + call_span.start;
        ctx.out
            .prepend_alloc(call_abs_start, &format!("const {}=", auto_var_name));
    }

    let effective_var_name = declarator
        .and_then(|d| d.name.map(|n| n.to_string()))
        .unwrap_or_else(|| auto_var_name.clone());

    // Register both defineProps and withDefaults bindings
    state.macro_bindings.push(MacroBindingEntry {
        macro_name: "defineProps".to_string(),
        var_name: Some(effective_var_name.clone()),
        type_name: if has_type_params {
            Some(type_name_str)
        } else {
            None
        },
        boxed_name: if boxed_define_props {
            Some(dp_boxed_name)
        } else {
            None
        },
        is_type: has_type_params,
    });
    state.macro_bindings.push(MacroBindingEntry {
        macro_name: "withDefaults".to_string(),
        var_name: Some(effective_var_name),
        type_name: None,
        boxed_name: if has_defaults {
            Some(wd_boxed_name)
        } else {
            None
        },
        is_type: false,
    });
}

/// Check if a type string is a simple reference (identifier) that doesn't need Prettify wrapping.
fn is_simple_type_reference(type_text: &str) -> bool {
    let trimmed = type_text.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Simple identifier: starts with letter/underscore, only alphanumeric/underscore/dots
    // Also handle qualified references like `Foo.Bar`
    trimmed
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
        && trimmed
            .chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_')
}

/// Build the `createMacroReturn({...})` content string from macro state.
fn build_macro_return_content(state: &TsxMacroState) -> String {
    let mut parts = Vec::new();

    for entry in &state.macro_bindings {
        let name = normalise_define_name(&entry.macro_name);
        let info_str = build_macro_info_string(
            entry.var_name.as_deref(),
            entry.type_name.as_deref(),
            entry.boxed_name.as_deref(),
        );
        if !info_str.is_empty() {
            parts.push(format!("{}:{{{}}}", name, info_str));
        }
    }

    if !state.model_bindings.is_empty() {
        let model_entries: Vec<String> = state
            .model_bindings
            .iter()
            .map(|m| {
                let info_str = build_macro_info_string(
                    Some(&m.var_name),
                    m.type_name.as_deref(),
                    m.boxed_name.as_deref(),
                );
                format!("{}:{{{}}}", m.model_name, info_str)
            })
            .collect();
        parts.push(format!("model:{{{}}}", model_entries.join(",")));
    }

    format!("{{{}}}", parts.join(","))
}

/// Build a single macro info string: `"value":{} as typeof x,"type":{} as T`.
fn build_macro_info_string(
    var_name: Option<&str>,
    type_name: Option<&str>,
    boxed_name: Option<&str>,
) -> String {
    let mut parts = Vec::new();
    if let Some(v) = var_name {
        parts.push(format!("\"value\":{{}} as typeof {}", v));
    }
    if let Some(t) = type_name {
        parts.push(format!("\"type\":{{}} as {}", t));
    }
    if let Some(o) = boxed_name {
        parts.push(format!("\"object\":{{}} as typeof {}", o));
    }
    // If no type_name and no boxed_name but we have var_name, also emit object
    if type_name.is_none() && boxed_name.is_none() {
        if let Some(v) = var_name {
            parts.push(format!("\"object\":{{}} as typeof {}", v));
        }
    }
    parts.join(",")
}

/// Strip "define" prefix and lowercase first char: "defineProps" → "props".
fn normalise_define_name(name: &str) -> String {
    if let Some(rest) = name.strip_prefix("define") {
        let mut chars = rest.chars();
        match chars.next() {
            Some(c) => {
                let lower: String = c.to_lowercase().collect();
                format!("{}{}", lower, chars.as_str())
            }
            None => name.to_string(),
        }
    } else {
        name.to_string()
    }
}

// ── Helpers ───────────────────────────────────────────────────────

fn emit_minimal_wrapper(
    out: &mut CodeGenOutput<'_>,
    options: &TsxScriptOptions<'_>,
    pos: u32,
    template_end: Option<u32>,
) -> Option<String> {
    let _ = options; // suppress unused warning
    if template_end.is_some() {
        // Unified CT: function start at pos, return + close deferred
        let start = format!("function {}TemplateBindingFN() {{\n", PREFIX);
        out.prepend_alloc(pos, &start);
        Some(format!(
            "\n;return {{...{}shallowUnwrapRef({{}})}}\n}}\n",
            PREFIX
        ))
    } else {
        // No template: emit everything at pos
        let wrapper = format!(
            "function {}TemplateBindingFN() {{\n;return {{...{}shallowUnwrapRef({{}})}}\n}}\n",
            PREFIX, PREFIX,
        );
        out.prepend_alloc(pos, &wrapper);
        None
    }
}

/// Prefix for all emitted ___VERTER___ types/functions.
const PREFIX: &str = "___VERTER___";

/// Instance keys to omit from the base component type.
const PATCHED_INSTANCE_KEYS: &str =
    "\"$\"|\"$data\"|\"$props\"|\"$attrs\"|\"$refs\"|\"$options\"|\"$emit\"|\"$el\"|\"$slots\"";

/// Ambient module declaration for `@verter/types`.
///
/// Appended to `type_constructs` so that every TSX file self-contains the module
/// declaration. TypeScript resolves ambient `declare module` from the same file,
/// making the `import ... from "@verter/types"` at the top resolvable without
/// installing the package or relying on TS plugin / TSGO overlay hacks.
///
/// Uses `import("vue").X` syntax because top-level imports are not allowed inside
/// `declare module` blocks.
///
/// See also [`VERTER_TYPES_STANDALONE_DTS`] for the unwrapped version (used by
/// the LSP to materialise `node_modules/@verter/types/index.d.ts` on disk).
const VERTER_TYPES_AMBIENT_MODULE: &str = r#"
declare module "@verter/types" {
  export type Prettify<T> = T extends { (...args: any[]): any } ? T : { [K in keyof T]: T[K] } & {};
  export declare function createMacroReturn<T>(o: T): { ____VERTER___MACRO_RETURN_KEY____: T };
  export type OmitConstructorSignature<T> = { [K in keyof T]: T[K] };
  export type ExtractComponentProps<T> = T extends { new (): infer I } ? { [K in keyof I]: I[K] } : {};
  export declare function enhanceElementWithProps<T, P>(el: T, props: P): T & P;
  export type PublicInstanceFromMacro<Props, Emits, Expose, Slots, Attrs, El extends Element = Element> = {
    $props: Props; $emit: Emits; $slots: Slots; $attrs: Attrs; $el: El;
  } & Props & Expose;
  export declare function shallowUnwrapRef<T>(obj: T): import("vue").ShallowUnwrapRef<T>;

  type Data = Record<string, unknown>;
  type DefaultFactory<T> = (props: Data) => T | null | undefined;
  type DefineModelOptions<T = any, G = T, S = T> = { get?: (v: T) => G; set?: (v: S) => any };
  type InferDefault<P, T> = ((props: P) => T & {}) | (T extends NativeType ? T : never);
  type InferDefaults<T> = { [K in keyof T]?: InferDefault<T, T[K]> };
  type NativeType = null | undefined | number | string | boolean | symbol | Function;
  interface PropOptions<T = any, D = T> {
    type?: import("vue").PropType<T> | true | null;
    required?: boolean;
    default?: D | DefaultFactory<D> | null | undefined | object;
    validator?(value: unknown, props: Data): boolean;
  }

  export declare function defineProps_Box<PropNames extends string = string>(props: PropNames[]): PropNames[];
  export declare function defineProps_Box<PP extends import("vue").ComponentObjectPropsOptions = import("vue").ComponentObjectPropsOptions>(props: PP): PP;
  export declare function defineProps_Box<TypeProps>(): TypeProps;

  export declare function withDefaults_Box<T, Defaults extends InferDefaults<T>>(props: T, defaults: Defaults): [T, Defaults];

  export declare function defineEmits_Box<EE extends string = string>(emitOptions: EE[]): EE[];
  export declare function defineEmits_Box<E extends import("vue").EmitsOptions = import("vue").EmitsOptions>(emitOptions: E): E;
  export declare function defineEmits_Box<T extends import("vue").ComponentTypeEmits>(): T;

  export declare function defineOptions_Box<
    RawBindings = {},
    D = {},
    C extends import("vue").ComputedOptions = {},
    M extends import("vue").MethodOptions = {},
    Mixin extends import("vue").ComponentOptionsMixin = import("vue").ComponentOptionsMixin,
    Extends extends import("vue").ComponentOptionsMixin = import("vue").ComponentOptionsMixin,
    InheritAttrs extends true | false = true,
    T = Record<string, any>
  >(
    options?: T &
      import("vue").ComponentOptionsBase<{}, RawBindings, D, C, M, Mixin, Extends, {}> & {
        props?: never;
        emits?: never;
        expose?: never;
        slots?: never;
        inheritAttrs?: InheritAttrs;
      }
  ): T;

  export declare function defineModel_Box<T, M extends PropertyKey = string, G = T, S = T>(
    options: ({ default: any } | { required: true }) & PropOptions<T> & DefineModelOptions<T, G, S>
  ): ({ default: any } | { required: true }) & PropOptions<T> & DefineModelOptions<T, G, S>;
  export declare function defineModel_Box<T, M extends PropertyKey = string, G = T, S = T>(
    options?: PropOptions<T> & DefineModelOptions<T, G, S>
  ): PropOptions<T> & DefineModelOptions<T, G, S>;
  export declare function defineModel_Box<T, M extends PropertyKey = string, G = T, S = T>(
    name: string,
    options: ({ default: any } | { required: true }) & PropOptions<T> & DefineModelOptions<T, G, S>
  ): [string, ({ default: any } | { required: true }) & PropOptions<T> & DefineModelOptions<T, G, S>];
  export declare function defineModel_Box<T, M extends PropertyKey = string, G = T, S = T>(
    name: string,
    options?: PropOptions<T> & DefineModelOptions<T, G, S>
  ): [string, PropOptions<T> & DefineModelOptions<T, G, S>];

  export declare function defineExpose_Box<Exposed extends Record<string, any> = Record<string, any>>(exposed?: Exposed): Exposed;
  export declare function defineSlots_Box<S extends Record<string, any> = Record<string, any>>(): S;
}
"#;

/// Standalone `@verter/types` type declarations as a `.d.ts` file.
///
/// This is the same content as [`VERTER_TYPES_AMBIENT_MODULE`] but without the
/// `declare module "@verter/types" { ... }` wrapper.  The LSP writes this to
/// `node_modules/@verter/types/index.d.ts` when the real package is not installed,
/// so that TSGO can resolve `import { ... } from "@verter/types"` via normal
/// `node_modules` resolution.
///
/// Uses `import("vue").X` syntax for Vue type references.
pub const VERTER_TYPES_STANDALONE_DTS: &str = r#"// Auto-generated by verter-lsp — do not edit.
// This file provides @verter/types declarations so that TSGO can resolve
// the imports emitted by Verter's TSX codegen.

export type Prettify<T> = T extends { (...args: any[]): any } ? T : { [K in keyof T]: T[K] } & {};
export declare function createMacroReturn<T>(o: T): { ____VERTER___MACRO_RETURN_KEY____: T };
export type OmitConstructorSignature<T> = { [K in keyof T]: T[K] };
export type ExtractComponentProps<T> = T extends { new (): infer I } ? { [K in keyof I]: I[K] } : {};
export declare function enhanceElementWithProps<T, P>(el: T, props: P): T & P;
export type PublicInstanceFromMacro<Props, Emits, Expose, Slots, Attrs, El extends Element = Element> = {
  $props: Props; $emit: Emits; $slots: Slots; $attrs: Attrs; $el: El;
} & Props & Expose;
export declare function shallowUnwrapRef<T>(obj: T): import("vue").ShallowUnwrapRef<T>;

type Data = Record<string, unknown>;
type DefaultFactory<T> = (props: Data) => T | null | undefined;
type DefineModelOptions<T = any, G = T, S = T> = { get?: (v: T) => G; set?: (v: S) => any };
type InferDefault<P, T> = ((props: P) => T & {}) | (T extends NativeType ? T : never);
type InferDefaults<T> = { [K in keyof T]?: InferDefault<T, T[K]> };
type NativeType = null | undefined | number | string | boolean | symbol | Function;
interface PropOptions<T = any, D = T> {
  type?: import("vue").PropType<T> | true | null;
  required?: boolean;
  default?: D | DefaultFactory<D> | null | undefined | object;
  validator?(value: unknown, props: Data): boolean;
}

export declare function defineProps_Box<PropNames extends string = string>(props: PropNames[]): PropNames[];
export declare function defineProps_Box<PP extends import("vue").ComponentObjectPropsOptions = import("vue").ComponentObjectPropsOptions>(props: PP): PP;
export declare function defineProps_Box<TypeProps>(): TypeProps;

export declare function withDefaults_Box<T, Defaults extends InferDefaults<T>>(props: T, defaults: Defaults): [T, Defaults];

export declare function defineEmits_Box<EE extends string = string>(emitOptions: EE[]): EE[];
export declare function defineEmits_Box<E extends import("vue").EmitsOptions = import("vue").EmitsOptions>(emitOptions: E): E;
export declare function defineEmits_Box<T extends import("vue").ComponentTypeEmits>(): T;

export declare function defineOptions_Box<
  RawBindings = {},
  D = {},
  C extends import("vue").ComputedOptions = {},
  M extends import("vue").MethodOptions = {},
  Mixin extends import("vue").ComponentOptionsMixin = import("vue").ComponentOptionsMixin,
  Extends extends import("vue").ComponentOptionsMixin = import("vue").ComponentOptionsMixin,
  InheritAttrs extends true | false = true,
  T = Record<string, any>
>(
  options?: T &
    import("vue").ComponentOptionsBase<{}, RawBindings, D, C, M, Mixin, Extends, {}> & {
      props?: never;
      emits?: never;
      expose?: never;
      slots?: never;
      inheritAttrs?: InheritAttrs;
    }
): T;

export declare function defineModel_Box<T, M extends PropertyKey = string, G = T, S = T>(
  options: ({ default: any } | { required: true }) & PropOptions<T> & DefineModelOptions<T, G, S>
): ({ default: any } | { required: true }) & PropOptions<T> & DefineModelOptions<T, G, S>;
export declare function defineModel_Box<T, M extends PropertyKey = string, G = T, S = T>(
  options?: PropOptions<T> & DefineModelOptions<T, G, S>
): PropOptions<T> & DefineModelOptions<T, G, S>;
export declare function defineModel_Box<T, M extends PropertyKey = string, G = T, S = T>(
  name: string,
  options: ({ default: any } | { required: true }) & PropOptions<T> & DefineModelOptions<T, G, S>
): [string, ({ default: any } | { required: true }) & PropOptions<T> & DefineModelOptions<T, G, S>];
export declare function defineModel_Box<T, M extends PropertyKey = string, G = T, S = T>(
  name: string,
  options?: PropOptions<T> & DefineModelOptions<T, G, S>
): [string, PropOptions<T> & DefineModelOptions<T, G, S>];

export declare function defineExpose_Box<Exposed extends Record<string, any> = Record<string, any>>(exposed?: Exposed): Exposed;
export declare function defineSlots_Box<S extends Record<string, any> = Record<string, any>>(): S;
"#;

/// Collect Vue built-in component names used in the template AST.
///
/// Walks the flat arena looking for elements with `TagType::Component` whose
/// tag matches a Vue built-in (Suspense, Teleport, KeepAlive, Transition, TransitionGroup).
/// Returns the user-facing Vue export names (e.g., `"Suspense"`, `"KeepAlive"`).
fn collect_builtin_components(
    template_ast: Option<&crate::ast::types::TemplateAst>,
    source: &str,
) -> Vec<&'static str> {
    use crate::template::code_gen::shared::helpers::is_builtin_component;

    let ast = match template_ast {
        Some(a) => a,
        None => return Vec::new(),
    };

    let mut seen = 0u8; // bitmask to deduplicate
    let mut result = Vec::new();

    for node in &ast.nodes {
        if let crate::ast::types::AstNodeKind::Element(ref el) = node.kind {
            if !el.tag_type.is_component() {
                continue;
            }
            let tag_name = &source[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize];
            if let Some((flag_bit, _helper_name)) = is_builtin_component(tag_name) {
                if seen & flag_bit == 0 {
                    seen |= flag_bit;
                    // Map to the user-facing Vue export name (PascalCase)
                    let vue_name = match tag_name {
                        "Teleport" | "teleport" => "Teleport",
                        "Suspense" | "suspense" => "Suspense",
                        "KeepAlive" | "keep-alive" => "KeepAlive",
                        "BaseTransition" | "base-transition" => "BaseTransition",
                        "Transition" | "transition" => "Transition",
                        "TransitionGroup" | "transition-group" => "TransitionGroup",
                        _ => continue,
                    };
                    result.push(vue_name);
                }
            }
        }
    }
    result
}

/// Emit helper imports hoisted before the wrapper function.
/// Imports are conditional based on what macros were used.
fn emit_helper_imports(
    out: &mut CodeGenOutput<'_>,
    pos: u32,
    macro_state: &TsxMacroState,
    options: &TsxScriptOptions<'_>,
    builtin_components: &[&str],
) {
    use std::fmt::Write;

    let type_imports = Vec::from([
        "Prettify",
        "PublicInstanceFromMacro",
        "ExtractComponentProps",
        "OmitConstructorSignature",
    ]);

    let type_import_str: String = type_imports
        .iter()
        .map(|name| format!("{} as {P}{}", name, name, P = PREFIX))
        .collect::<Vec<_>>()
        .join(", ");

    let mut runtime_verter_imports = vec!["shallowUnwrapRef", "enhanceElementWithProps"];
    if macro_state.has_macros {
        runtime_verter_imports.push("createMacroReturn");
    }
    // Box helpers are used as runtime function calls (e.g., `___VERTER___defineOptions_Box(...)`)
    // so they must be value imports, not type-only imports.
    for helper in &macro_state.needed_box_helpers {
        runtime_verter_imports.push(helper);
    }
    let runtime_import_str: String = runtime_verter_imports
        .iter()
        .map(|name| format!("{} as {P}{}", name, name, P = PREFIX))
        .collect::<Vec<_>>()
        .join(", ");

    let vue_imports = ["defineComponent"];

    let vue_import_str: String = vue_imports
        .iter()
        .map(|name| format!("{} as {P}{}", name, name, P = PREFIX))
        .collect::<Vec<_>>()
        .join(", ");

    let mut imports = String::with_capacity(512);
    writeln!(
        imports,
        "import type {{ {} }} from \"{}\";",
        type_import_str, options.types_module_name
    )
    .expect("write to String is infallible");
    writeln!(
        imports,
        "import {{ {} }} from \"{}\";",
        runtime_import_str, options.types_module_name
    )
    .expect("write to String is infallible");
    writeln!(imports, "import {{ {} }} from \"vue\";", vue_import_str)
        .expect("write to String is infallible");

    // Import built-in components without prefix — template JSX references them by bare name
    if !builtin_components.is_empty() {
        let builtin_str = builtin_components.join(", ");
        writeln!(imports, "import {{ {} }} from \"vue\";", builtin_str)
            .expect("write to String is infallible");
    }

    out.prepend_alloc(pos, &imports);
}

/// Emit all type constructs to the `buf` string (no sourcemap).
#[allow(clippy::too_many_arguments)]
fn emit_type_constructs(
    buf: &mut String,
    generic_info: &Option<TsxGenericInfo>,
    binding_names: &[String],
    import_binding_names: &[String],
    declaration_texts: &[String],
    template_ast: Option<&TemplateAst>,
    source: &str,
    options: &TsxScriptOptions<'_>,
    macro_state: &TsxMacroState,
) {
    // Generic helpers — empty strings when no generics
    let gs = generic_info
        .as_ref()
        .map(|g| g.source_bracket())
        .unwrap_or_default();
    let gn = generic_info
        .as_ref()
        .map(|g| g.names_bracket())
        .unwrap_or_default();
    let gd = generic_info
        .as_ref()
        .map(|g| g.declaration_bracket())
        .unwrap_or_default();
    let gsn = generic_info
        .as_ref()
        .map(|g| g.sanitised_names_bracket())
        .unwrap_or_default();

    // Emit TemplateBinding type alias (from the TemplateBindingFN wrapper)
    emit_template_binding_type(buf, &gs, &gn);

    emit_full_context(
        buf,
        &gs,
        &gn,
        binding_names,
        import_binding_names,
        declaration_texts,
    );
    let root_comp_offset = emit_comp_functions(buf, &gs, &gn, template_ast, source);
    emit_get_root_component(buf, &gs, &gn, root_comp_offset);
    emit_default_component(buf, macro_state);
    emit_attributes_type(buf, &gs);
    emit_root_element_types(buf, &gs, &gn);
    emit_instance_types(buf, &gd, &gsn);

    // Append ambient module declaration so imports from "@verter/types" resolve
    // without requiring the package in node_modules or TS plugin hooks.
    if options.embed_ambient_types {
        buf.push_str(VERTER_TYPES_AMBIENT_MODULE);
    }
}

/// Emit TemplateBinding type alias referencing the TemplateBindingFN return type.
fn emit_template_binding_type(buf: &mut String, gs: &str, gn: &str) {
    use std::fmt::Write;

    write!(
        buf,
        "\nexport type {P}TemplateBinding{gs}=ReturnType<typeof {P}TemplateBindingFN{gn}>;",
        P = PREFIX,
        gs = gs,
        gn = gn,
    )
    .expect("write to String is infallible");
}

/// Emit ___VERTER___attributes type (empty for now, used in Instance type).
fn emit_attributes_type(buf: &mut String, gs: &str) {
    use std::fmt::Write;

    write!(buf, "\ntype {P}attributes{gs}={{}};", P = PREFIX, gs = gs,)
        .expect("write to String is infallible");
}

/// Emit FullContext function + type alias.
/// Includes declaration source text in the body and import binding names.
fn emit_full_context(
    buf: &mut String,
    gs: &str,
    gn: &str,
    binding_names: &[String],
    import_binding_names: &[String],
    declaration_texts: &[String],
) {
    use std::fmt::Write;

    // Combine binding names with import binding names
    let mut all_names: Vec<&str> = binding_names.iter().map(|s| s.as_str()).collect();
    for name in import_binding_names {
        if !all_names.contains(&name.as_str()) {
            all_names.push(name);
        }
    }

    // Build binding entries: `name: {} as typeof name`
    let binding_entries: String = all_names
        .iter()
        .map(|name| format!("{}: {{}} as typeof {}", name, name))
        .collect::<Vec<_>>()
        .join(",");

    // Build declaration body content
    let decl_body = if declaration_texts.is_empty() {
        String::new()
    } else {
        declaration_texts.join("\n")
    };

    write!(
        buf,
        "\n;function {P}FullContextFN{gs}() {{{body};return {P}shallowUnwrapRef({{{entries}}})}};\
         \nexport type {P}FullContext{gs}=ReturnType<typeof {P}FullContextFN{gn}>;",
        P = PREFIX,
        gs = gs,
        gn = gn,
        body = decl_body,
        entries = binding_entries,
    )
    .expect("write to String is infallible");
}

/// Emit Comp{offset} functions for template elements.
/// Returns the offset of the root element's Comp function (if any).
///
/// Recursively walks ALL elements in the template AST (not just root children),
/// building condition scopes for v-if narrowing guards in each Comp function.
fn emit_comp_functions(
    buf: &mut String,
    gs: &str,
    gn: &str,
    template_ast: Option<&TemplateAst>,
    source: &str,
) -> Option<u32> {
    let ast = template_ast?;

    let root_children = ast
        .root
        .content
        .as_ref()
        .map(|c| c.children.as_slice())
        .unwrap_or(&[]);

    let mut root_comp_offset: Option<u32> = None;

    walk_children_for_comp(
        buf,
        gs,
        gn,
        ast,
        source,
        root_children,
        &[],
        &mut root_comp_offset,
        true,
    );

    root_comp_offset
}

/// Recursively walk children to emit Comp functions with condition scope tracking.
#[allow(clippy::too_many_arguments)]
fn walk_children_for_comp(
    buf: &mut String,
    gs: &str,
    gn: &str,
    ast: &TemplateAst,
    source: &str,
    children: &[crate::types::NodeId],
    parent_scopes: &[super::condition::ConditionScope],
    root_comp_offset: &mut Option<u32>,
    is_root: bool,
) {
    for &child_id in children {
        let node = &ast.nodes[child_id.0];
        if let AstNodeKind::Element(el) = &node.kind {
            // Build condition scope using raw expressions (no binding prefixes)
            // because Comp functions have their own FullContext setup
            let mut scopes = parent_scopes.to_vec();
            if let Some(scope) = build_condition_scope_raw(el, ast, child_id, source) {
                scopes.push(scope);
            }

            // Emit Comp function (skip Slot/Template)
            if !matches!(el.tag_type, TagType::SlotOutlet | TagType::Template) {
                let offset = el.tag_open.start;
                emit_comp_function_for_element(buf, gs, gn, el, source, offset, &scopes);
                if is_root && root_comp_offset.is_none() {
                    *root_comp_offset = Some(offset);
                }
            }

            // Recurse into children
            if let Some(content) = &el.content {
                walk_children_for_comp(
                    buf,
                    gs,
                    gn,
                    ast,
                    source,
                    &content.children,
                    &scopes,
                    root_comp_offset,
                    false,
                );
            }
        }
    }
}

/// Build a condition scope using raw source expressions (no binding prefixes).
/// For use in Comp functions where FullContext provides variables directly.
fn build_condition_scope_raw(
    el: &ElementNode,
    ast: &TemplateAst,
    node_id: crate::types::NodeId,
    source: &str,
) -> Option<super::condition::ConditionScope> {
    use crate::ast::types::ElementNodeConditionKind;

    let condition = el.v_condition.as_ref()?;

    let positive = match condition.kind {
        ElementNodeConditionKind::If | ElementNodeConditionKind::ElseIf => {
            let (Some(vs), Some(ve)) = (condition.prop.value_start, condition.prop.value_end)
            else {
                return None;
            };
            Some(source[vs as usize..ve as usize].to_string())
        }
        ElementNodeConditionKind::Else => None,
    };

    let sibling_negations = match condition.kind {
        ElementNodeConditionKind::If => vec![],
        ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else => {
            collect_sibling_negations_raw(ast, node_id, source)
        }
    };

    Some(super::condition::ConditionScope {
        positive,
        sibling_negations,
    })
}

/// Walk backward through siblings to collect raw condition expressions for negation.
fn collect_sibling_negations_raw(
    ast: &TemplateAst,
    node_id: crate::types::NodeId,
    source: &str,
) -> Vec<String> {
    use crate::ast::types::ElementNodeConditionKind;

    let mut negations = Vec::new();
    let mut current = node_id;

    while let Some(prev) = ast.prev_sibling(current) {
        let prev_node = &ast.nodes[prev.0];
        match &prev_node.kind {
            AstNodeKind::Element(prev_el) => {
                if let Some(ref cond) = prev_el.v_condition {
                    if let (Some(vs), Some(ve)) = (cond.prop.value_start, cond.prop.value_end) {
                        negations.push(source[vs as usize..ve as usize].to_string());
                    }
                    if matches!(cond.kind, ElementNodeConditionKind::If) {
                        break;
                    }
                } else {
                    break;
                }
            }
            AstNodeKind::Text(t) => {
                let text = &source[t.start as usize..t.end as usize];
                if text.trim().is_empty() {
                    current = prev;
                    continue;
                }
                break;
            }
            _ => break,
        }
        current = prev;
    }

    negations.reverse();
    negations
}

/// Emit a single Comp{offset} function for an element, with optional condition guards.
fn emit_comp_function_for_element(
    buf: &mut String,
    gs: &str,
    _gn: &str,
    el: &ElementNode,
    source: &str,
    offset: u32,
    condition_scopes: &[super::condition::ConditionScope],
) {
    use std::fmt::Write;

    let tag_name = &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];

    // Generate narrowing guard from condition scopes
    let guard = super::condition::generate_condition_text(condition_scopes)
        .map(|text| format!("\n  if(!({})) return null;", text))
        .unwrap_or_default();

    match el.tag_type {
        TagType::Element => {
            write!(
                buf,
                "\nfunction {P}Comp{offset}{gs}() {{{guard}\
                 \n  return {P}enhanceElementWithProps({{}} as HTMLElementTagNameMap[\"{tag}\"], {{}});\
                 \n}}",
                P = PREFIX,
                offset = offset,
                gs = gs,
                guard = guard,
                tag = tag_name,
            )
            .expect("write to String is infallible");
        }
        TagType::Component => {
            write!(
                buf,
                "\nfunction {P}Comp{offset}{gs}() {{{guard}\
                 \n  return new {tag}({{}});\
                 \n}}",
                P = PREFIX,
                offset = offset,
                gs = gs,
                guard = guard,
                tag = tag_name,
            )
            .expect("write to String is infallible");
        }
        TagType::SlotOutlet | TagType::Template => {
            // Skip <slot> and <template> wrappers
        }
    }
}

/// Emit getRootComponent + getRootComponentPassedProps.
fn emit_get_root_component(buf: &mut String, gs: &str, gn: &str, root_comp_offset: Option<u32>) {
    use std::fmt::Write;

    if let Some(offset) = root_comp_offset {
        write!(
            buf,
            "\nfunction {P}getRootComponent{gs}() {{ return {P}Comp{offset}{gn}(); }}\
             \nfunction {P}getRootComponentPassedProps{gs}() {{ return {{}}; }}",
            P = PREFIX,
            gs = gs,
            gn = gn,
            offset = offset,
        )
        .expect("write to String is infallible");
    } else {
        write!(
            buf,
            "\nfunction {P}getRootComponent{gs}() {{ return {{}}; }}\
             \nfunction {P}getRootComponentPassedProps{gs}() {{ return {{}}; }}",
            P = PREFIX,
            gs = gs,
        )
        .expect("write to String is infallible");
    }
}

/// Emit default_Component. If defineOptions was used with args, pass the boxed name.
fn emit_default_component(buf: &mut String, macro_state: &TsxMacroState) {
    use std::fmt::Write;

    if let Some(boxed) = &macro_state.define_options_boxed {
        write!(
            buf,
            "\n;const {P}default_Component = {P}defineComponent({boxed});",
            P = PREFIX,
            boxed = boxed,
        )
        .expect("write to String is infallible");
    } else {
        write!(
            buf,
            "\n;const {P}default_Component = {P}defineComponent({{}});",
            P = PREFIX,
        )
        .expect("write to String is infallible");
    }
}

/// Emit RootElement + RootElementProps type aliases.
fn emit_root_element_types(buf: &mut String, gs: &str, gn: &str) {
    use std::fmt::Write;

    write!(
        buf,
        "\ntype {P}RootElement{gs}=ReturnType<typeof {P}getRootComponent{gn}>;\
         \ntype {P}RootElementProps{gs}={P}Prettify<Omit<{P}ExtractComponentProps<{P}RootElement{gn}>,keyof ReturnType<typeof {P}getRootComponentPassedProps{gn}>>>;",
        P = PREFIX,
        gs = gs,
        gn = gn,
    )
    .expect("write to String is infallible");
}

/// Emit Instance, Instance_TEST, Component types.
/// Uses TemplateBinding (not FullContext) and includes attributes type.
fn emit_instance_types(buf: &mut String, gd: &str, gsn: &str) {
    use std::fmt::Write;

    // Instance — references TemplateBinding and attributes
    write!(
        buf,
        "\nexport type {P}Instance{gd} = \
         Omit<InstanceType<typeof {P}default_Component>, {keys}> \
         & {P}PublicInstanceFromMacro<\
         {P}TemplateBinding{gsn}, \
         {{}}&{P}attributes&{P}RootElementProps{gsn}, \
         {P}RootElement{gsn}, false, true\
         >;",
        P = PREFIX,
        gd = gd,
        gsn = gsn,
        keys = PATCHED_INSTANCE_KEYS,
    )
    .expect("write to String is infallible");

    // Instance_TEST
    write!(
        buf,
        "\nexport type {P}Instance_TEST{gd} = \
         Omit<InstanceType<typeof {P}default_Component>, {keys}> \
         & {P}PublicInstanceFromMacro<\
         {P}TemplateBinding{gsn}, \
         {{}}&{P}attributes&{P}RootElementProps{gsn}, \
         {P}RootElement{gsn}, true, true\
         >;",
        P = PREFIX,
        gd = gd,
        gsn = gsn,
        keys = PATCHED_INSTANCE_KEYS,
    )
    .expect("write to String is infallible");

    // Component
    write!(
        buf,
        "\nexport declare const {P}Component: {P}OmitConstructorSignature<typeof {P}default_Component> & {{\
         \n  new{gd}(props?: {P}Instance{gsn}['$props']): {P}Prettify<{P}Instance{gsn}>\
         \n}};\
         \nexport default {P}Component;",
        P = PREFIX,
        gd = gd,
        gsn = gsn,
    )
    .expect("write to String is infallible");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_transform::CodeTransform;

    /// Generate TSX script and return (code, bindings, type_constructs).
    fn gen_tsx_script_full(source: &str) -> (String, FxHashMap<String, BindingType>, String) {
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
            types_module_name: "@verter/types",
            is_vapor: false,
            embed_ambient_types: true,
        };

        let result = generate_tsx_script(
            syntax.script(),
            syntax.script_setup(),
            syntax.template_ast(),
            source,
            &mut ct,
            &alloc,
            &options,
            None, // template_end: tests use legacy two-CT pattern
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

    fn gen_tsx_script(source: &str) -> (String, FxHashMap<String, BindingType>) {
        let (code, bindings, _) = gen_tsx_script_full(source);
        (code, bindings)
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

    // ── Generic wrapper tests ─────────────────────────────────────

    #[test]
    fn generic_wrapper_simple() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts" generic="T">
const value = {} as unknown as T
</script>"#,
        );
        assert!(
            code.contains("function ___VERTER___TemplateBindingFN<T>()"),
            "wrapper should have <T>: {}",
            code
        );
    }

    #[test]
    fn generic_wrapper_with_extends() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts" generic="T extends string">
const value = {} as unknown as T
</script>"#,
        );
        assert!(
            code.contains("function ___VERTER___TemplateBindingFN<T extends string>()"),
            "wrapper should have <T extends string>: {}",
            code
        );
    }

    #[test]
    fn generic_wrapper_multiple() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts" generic="K extends string, V">
const k = {} as unknown as K
const v = {} as unknown as V
</script>"#,
        );
        assert!(
            code.contains("function ___VERTER___TemplateBindingFN<K extends string, V>()"),
            "wrapper should have multiple generics: {}",
            code
        );
    }

    #[test]
    fn non_generic_wrapper_unchanged() {
        let (code, _) = gen_tsx_script(
            r#"<script setup>
const msg = 'hello'
</script>"#,
        );
        assert!(
            code.contains("function ___VERTER___TemplateBindingFN()"),
            "non-generic should have no angle brackets: {}",
            code
        );
        assert!(
            !code.contains("function ___VERTER___TemplateBindingFN<"),
            "non-generic should NOT have angle brackets: {}",
            code
        );
    }

    #[test]
    fn generic_wrapper_invalid_syntax_fallback() {
        // "T in string" is invalid TS (should be "extends"), but the raw
        // string should still pass through so TypeScript surfaces the error.
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts" generic="T in string">
const value = 'hello'
</script>"#,
        );
        assert!(
            code.contains("function ___VERTER___TemplateBindingFN<T in string>()"),
            "invalid generic should still be emitted raw: {}",
            code
        );
    }

    // ── Helper imports tests ──────────────────────────────────────

    #[test]
    fn helper_imports_emitted() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
        );
        assert!(
            code.contains("import type { Prettify as ___VERTER___Prettify"),
            "should have Prettify import: {}",
            code
        );
        assert!(
            code.contains("import { shallowUnwrapRef as ___VERTER___shallowUnwrapRef"),
            "should have shallowUnwrapRef import: {}",
            code
        );
        assert!(
            code.contains("import { defineComponent as ___VERTER___defineComponent }"),
            "should have defineComponent import: {}",
            code
        );
    }

    #[test]
    fn helper_imports_hoisted_before_wrapper() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
        );
        let fn_pos = code.find("function ___VERTER___TemplateBindingFN").unwrap();
        let import_pos = code.find("import type { Prettify").unwrap();
        assert!(
            import_pos < fn_pos,
            "helper imports should be before wrapper function"
        );
    }

    // ── FullContext tests ─────────────────────────────────────────

    #[test]
    fn full_context_non_generic() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
import { ref } from 'vue'
const a = ref(0)
</script>"#,
        );
        assert!(
            tc.contains("function ___VERTER___FullContextFN()"),
            "non-generic FullContext should have no angle brackets: {}",
            tc
        );
        assert!(
            tc.contains("___VERTER___shallowUnwrapRef("),
            "FullContext should use shallowUnwrapRef: {}",
            tc
        );
        assert!(
            tc.contains(
                "export type ___VERTER___FullContext=ReturnType<typeof ___VERTER___FullContextFN>;"
            ),
            "FullContext type should reference FN: {}",
            tc
        );
    }

    #[test]
    fn full_context_generic() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts" generic="T">
const a = {} as unknown as T
</script>"#,
        );
        assert!(
            tc.contains("function ___VERTER___FullContextFN<T>()"),
            "generic FullContext should have <T>: {}",
            tc
        );
        assert!(
            tc.contains("export type ___VERTER___FullContext<T>=ReturnType<typeof ___VERTER___FullContextFN<T>>;"),
            "generic FullContext type alias: {}",
            tc
        );
    }

    #[test]
    fn full_context_includes_bindings() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
const foo = 'bar'
const baz = 123
</script>"#,
        );
        // Both bindings should appear in FullContext
        assert!(
            tc.contains("foo: {} as typeof foo"),
            "should include foo binding: {}",
            tc
        );
        assert!(
            tc.contains("baz: {} as typeof baz"),
            "should include baz binding: {}",
            tc
        );
    }

    // ── Comp function tests ──────────────────────────────────────

    #[test]
    fn comp_function_html_element() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
        );
        assert!(
            tc.contains("___VERTER___enhanceElementWithProps({} as HTMLElementTagNameMap[\"div\"]"),
            "should emit Comp for div: {}",
            tc
        );
    }

    #[test]
    fn comp_function_component() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
</script>
<template><MyComp /></template>"#,
        );
        assert!(
            tc.contains("return new MyComp({})"),
            "should emit new MyComp: {}",
            tc
        );
    }

    #[test]
    fn comp_function_generic() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts" generic="T">
const msg = {} as T
</script>
<template><div>{{ msg }}</div></template>"#,
        );
        assert!(
            tc.contains("function ___VERTER___Comp") && tc.contains("<T>()"),
            "Comp function should have generics: {}",
            tc
        );
    }

    // ── getRootComponent tests ───────────────────────────────────

    #[test]
    fn get_root_component_with_template() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div>{{ msg }}</div></template>"#,
        );
        assert!(
            tc.contains("function ___VERTER___getRootComponent()")
                && tc.contains("return ___VERTER___Comp"),
            "getRootComponent should delegate to Comp: {}",
            tc
        );
    }

    #[test]
    fn get_root_component_generic() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts" generic="T extends string">
const msg = {} as T
</script>
<template><div>{{ msg }}</div></template>"#,
        );
        assert!(
            tc.contains("function ___VERTER___getRootComponent<T extends string>()"),
            "getRootComponent should have generics: {}",
            tc
        );
    }

    #[test]
    fn get_root_component_no_template() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
        );
        assert!(
            tc.contains("function ___VERTER___getRootComponent()") && tc.contains("return {};"),
            "getRootComponent should return empty when no template: {}",
            tc
        );
    }

    // ── default_Component tests ──────────────────────────────────

    #[test]
    fn default_component_emitted() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
        );
        assert!(
            tc.contains("const ___VERTER___default_Component = ___VERTER___defineComponent({})"),
            "default_Component should use defineComponent: {}",
            tc
        );
    }

    // ── Instance types tests ─────────────────────────────────────

    #[test]
    fn instance_type_non_generic() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
        );
        assert!(
            tc.contains("export type ___VERTER___Instance ="),
            "Instance type should be emitted: {}",
            tc
        );
        assert!(
            tc.contains("export type ___VERTER___Instance_TEST ="),
            "Instance_TEST type should be emitted: {}",
            tc
        );
        assert!(
            tc.contains("export declare const ___VERTER___Component:"),
            "Component should be emitted: {}",
            tc
        );
        assert!(
            tc.contains("export default ___VERTER___Component;"),
            "default export should be emitted: {}",
            tc
        );
    }

    #[test]
    fn instance_type_generic() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts" generic="T">
const value = {} as unknown as T
</script>"#,
        );
        assert!(
            tc.contains("export type ___VERTER___Instance<__VERTER__TS__T = any>"),
            "Instance should have sanitised generic: {}",
            tc
        );
        assert!(
            tc.contains("___VERTER___TemplateBinding<__VERTER__TS__T>"),
            "Instance should reference TemplateBinding with sanitised name: {}",
            tc
        );
    }

    #[test]
    fn instance_type_generic_with_extends() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts" generic="T extends string">
const value = {} as unknown as T
</script>"#,
        );
        assert!(
            tc.contains("export type ___VERTER___Instance<__VERTER__TS__T extends string = any>"),
            "Instance should have constraint: {}",
            tc
        );
    }

    #[test]
    fn instance_type_multiple_generics() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts" generic="K extends string, V">
const k = {} as unknown as K
const v = {} as unknown as V
</script>"#,
        );
        assert!(
            tc.contains("<__VERTER__TS__K extends string = any, __VERTER__TS__V = any>"),
            "Instance should have multiple sanitised generics: {}",
            tc
        );
    }

    #[test]
    fn component_constructor_generic() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts" generic="T">
const value = {} as unknown as T
</script>"#,
        );
        assert!(
            tc.contains("new<__VERTER__TS__T = any>(props?: ___VERTER___Instance<__VERTER__TS__T>['$props'])"),
            "Component constructor should have generic: {}",
            tc
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

        // Type constructs have the full pipeline
        assert!(tc.contains("___VERTER___FullContextFN<T extends { id: number }>()"));
        assert!(tc.contains("___VERTER___Instance<__VERTER__TS__T extends { id: number } = any>"));
        assert!(tc.contains("export default ___VERTER___Component;"));
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

        // Type constructs — same structure, no angle brackets
        assert!(tc.contains("___VERTER___FullContextFN()"));
        assert!(tc.contains("export type ___VERTER___Instance ="));
        assert!(tc.contains("export default ___VERTER___Component;"));
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

    // ── Macro Boxing Tests ───────────────────────────────────────

    #[test]
    fn define_props_no_args() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
defineProps()
</script>"#,
        );
        assert!(
            code.contains("const ___VERTER___props=defineProps()"),
            "should prepend variable assignment: {}",
            code
        );
    }

    #[test]
    fn define_props_with_type_params() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>"#,
        );
        assert!(
            code.contains("___VERTER___defineProps_Type=___VERTER___Prettify<{ msg: string }>"),
            "should emit type alias with Prettify: {}",
            code
        );
        assert!(
            code.contains("defineProps<___VERTER___defineProps_Type>()"),
            "should replace type arg with alias: {}",
            code
        );
        assert!(
            code.contains("const ___VERTER___props=defineProps"),
            "should prepend variable assignment: {}",
            code
        );
    }

    #[test]
    fn define_props_with_type_params_assigned() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const props = defineProps<{ msg: string }>()
</script>"#,
        );
        assert!(
            code.contains("___VERTER___defineProps_Type=___VERTER___Prettify<{ msg: string }>"),
            "should emit type alias with Prettify: {}",
            code
        );
        assert!(
            code.contains("const props = defineProps<___VERTER___defineProps_Type>()"),
            "should keep user variable name: {}",
            code
        );
    }

    #[test]
    fn define_props_simple_type_ref_no_prettify() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
interface Props { msg: string }
defineProps<Props>()
</script>"#,
        );
        assert!(
            code.contains("___VERTER___defineProps_Type=Props;"),
            "simple type ref should NOT have Prettify wrapper: {}",
            code
        );
    }

    #[test]
    fn define_props_with_runtime_args() {
        let (code, _) = gen_tsx_script(
            r#"<script setup>
defineProps({ a: String })
</script>"#,
        );
        assert!(
            code.contains(
                "___VERTER___defineProps_Boxed=___VERTER___defineProps_Box({ a: String })"
            ),
            "should emit boxed const: {}",
            code
        );
        assert!(
            code.contains("defineProps(___VERTER___defineProps_Boxed)"),
            "should replace arg with boxed name: {}",
            code
        );
    }

    #[test]
    fn define_props_runtime_args_assigned() {
        let (code, _) = gen_tsx_script(
            r#"<script setup>
const props = defineProps({ a: String })
</script>"#,
        );
        assert!(
            code.contains(
                "___VERTER___defineProps_Boxed=___VERTER___defineProps_Box({ a: String })"
            ),
            "should emit boxed const: {}",
            code
        );
        assert!(
            code.contains("const props = defineProps(___VERTER___defineProps_Boxed)"),
            "should keep user variable, replace arg: {}",
            code
        );
    }

    #[test]
    fn define_emits_no_args() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
defineEmits()
</script>"#,
        );
        assert!(
            code.contains("const ___VERTER___emits=defineEmits()"),
            "should prepend variable assignment: {}",
            code
        );
    }

    #[test]
    fn define_emits_with_type_params() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
defineEmits<{ (e: 'change'): void }>()
</script>"#,
        );
        assert!(
            code.contains(
                "___VERTER___defineEmits_Type=___VERTER___Prettify<{ (e: 'change'): void }>"
            ),
            "should emit type alias: {}",
            code
        );
        assert!(
            code.contains("defineEmits<___VERTER___defineEmits_Type>()"),
            "should replace type arg: {}",
            code
        );
    }

    #[test]
    fn define_emits_with_array_arg() {
        let (code, _) = gen_tsx_script(
            r#"<script setup>
defineEmits(['change', 'update'])
</script>"#,
        );
        assert!(
            code.contains(
                "___VERTER___defineEmits_Boxed=___VERTER___defineEmits_Box(['change', 'update'])"
            ),
            "should emit boxed const: {}",
            code
        );
        assert!(
            code.contains("defineEmits(___VERTER___defineEmits_Boxed)"),
            "should replace arg: {}",
            code
        );
    }

    #[test]
    fn define_expose_no_return_no_var() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
defineExpose({ foo: 'bar' })
</script>"#,
        );
        // defineExpose is a no-return macro, so no `const xxx =` prepended
        assert!(
            !code.contains("const ___VERTER___expose=defineExpose"),
            "defineExpose should NOT have variable assignment: {}",
            code
        );
        assert!(
            code.contains(
                "___VERTER___defineExpose_Boxed=___VERTER___defineExpose_Box({ foo: 'bar' })"
            ),
            "should box the args: {}",
            code
        );
    }

    #[test]
    fn define_options_no_return() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
defineOptions({ inheritAttrs: false })
</script>"#,
        );
        assert!(
            !code.contains("const ___VERTER___options=defineOptions"),
            "defineOptions should NOT have variable assignment: {}",
            code
        );
        assert!(
            code.contains("___VERTER___defineOptions_Boxed=___VERTER___defineOptions_Box({ inheritAttrs: false })"),
            "should box the args: {}",
            code
        );
    }

    #[test]
    fn define_options_boxed_in_default_component() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
defineOptions({ inheritAttrs: false })
</script>"#,
        );
        assert!(
            tc.contains("___VERTER___defineComponent(___VERTER___defineOptions_Boxed)"),
            "default_Component should use defineOptions boxed name: {}",
            tc
        );
    }

    #[test]
    fn box_helpers_are_value_imports_not_type_imports() {
        // Box helpers are used as runtime function calls, so they must appear
        // in the value import line, not the type-only import line.
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
defineOptions({ inheritAttrs: false })
</script>"#,
        );

        // The value import line (no "type" keyword)
        let value_import = code
            .lines()
            .find(|l| l.starts_with("import {") && l.contains("@verter/types"))
            .expect("should have a value import from @verter/types");
        assert!(
            value_import.contains("defineOptions_Box"),
            "defineOptions_Box must be in value import, not type import: {code}"
        );

        // The type import line
        let type_import = code
            .lines()
            .find(|l| l.starts_with("import type {") && l.contains("@verter/types"))
            .expect("should have a type import from @verter/types");
        assert!(
            !type_import.contains("defineOptions_Box"),
            "defineOptions_Box must NOT be in type-only import: {code}"
        );
    }

    #[test]
    fn define_slots_no_args() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
defineSlots()
</script>"#,
        );
        assert!(
            code.contains("const ___VERTER___slots=defineSlots()"),
            "should prepend variable assignment: {}",
            code
        );
    }

    #[test]
    fn define_slots_with_type_params() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
defineSlots<{ default: (props: {}) => any }>()
</script>"#,
        );
        assert!(
            code.contains("___VERTER___defineSlots_Type"),
            "should emit type alias: {}",
            code
        );
    }

    // ── TemplateBinding Return Tests ─────────────────────────────

    #[test]
    fn template_binding_return_with_bindings() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
const msg = 'hello'
</script>"#,
        );
        assert!(
            code.contains("___VERTER___shallowUnwrapRef("),
            "should have shallowUnwrapRef in return: {}",
            code
        );
        assert!(
            code.contains("count: count as unknown as typeof count"),
            "should have count binding in return: {}",
            code
        );
        assert!(
            code.contains("msg: msg as unknown as typeof msg"),
            "should have msg binding in return: {}",
            code
        );
    }

    #[test]
    fn template_binding_return_with_macro_return() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const props = defineProps<{ msg: string }>()
</script>"#,
        );
        assert!(
            code.contains("___VERTER___createMacroReturn("),
            "should have createMacroReturn in return: {}",
            code
        );
        assert!(
            code.contains("\"value\":{} as typeof props"),
            "should have props value in macro return: {}",
            code
        );
        assert!(
            code.contains("\"type\":{} as ___VERTER___defineProps_Type"),
            "should have type info in macro return: {}",
            code
        );
    }

    #[test]
    fn template_binding_props_spread() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const props = defineProps<{ msg: string }>()
</script>"#,
        );
        assert!(
            code.contains("...({} as Pick<typeof props, keyof ___VERTER___defineProps_Type>)"),
            "should have props spread in return: {}",
            code
        );
    }

    // ── TemplateBinding Type Construct Tests ─────────────────────

    #[test]
    fn template_binding_type_emitted() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
        );
        assert!(
            tc.contains("export type ___VERTER___TemplateBinding=ReturnType<typeof ___VERTER___TemplateBindingFN>"),
            "should emit TemplateBinding type alias: {}",
            tc
        );
    }

    #[test]
    fn template_binding_type_generic() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts" generic="T">
const value = {} as unknown as T
</script>"#,
        );
        assert!(
            tc.contains("export type ___VERTER___TemplateBinding<T>=ReturnType<typeof ___VERTER___TemplateBindingFN<T>>"),
            "should emit generic TemplateBinding type: {}",
            tc
        );
    }

    #[test]
    fn attributes_type_emitted() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
        );
        assert!(
            tc.contains("type ___VERTER___attributes={}"),
            "should emit attributes type: {}",
            tc
        );
    }

    #[test]
    fn instance_references_template_binding() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
        );
        assert!(
            tc.contains("___VERTER___TemplateBinding,"),
            "Instance should reference TemplateBinding, not FullContext: {}",
            tc
        );
        assert!(
            tc.contains("{}&___VERTER___attributes&___VERTER___RootElementProps"),
            "Instance should include attributes in second param: {}",
            tc
        );
    }

    // ── Conditional Helper Imports Tests ─────────────────────────

    #[test]
    fn helper_imports_include_box_when_needed() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>"#,
        );
        assert!(
            code.contains("defineProps_Box as ___VERTER___defineProps_Box"),
            "should import defineProps_Box helper: {}",
            code
        );
    }

    #[test]
    fn helper_imports_include_create_macro_return() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
defineProps<{ msg: string }>()
</script>"#,
        );
        assert!(
            code.contains("createMacroReturn as ___VERTER___createMacroReturn"),
            "should import createMacroReturn when macros used: {}",
            code
        );
    }

    // ── FullContext with Declarations Tests ───────────────────────

    #[test]
    fn full_context_includes_import_bindings() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
</script>"#,
        );
        assert!(
            tc.contains("ref: {} as typeof ref"),
            "FullContext should include import binding 'ref': {}",
            tc
        );
        assert!(
            tc.contains("count: {} as typeof count"),
            "FullContext should include binding 'count': {}",
            tc
        );
    }

    // ── withDefaults Tests ───────────────────────────────────────

    #[test]
    fn with_defaults_type_params() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const props = withDefaults(defineProps<{ msg: string }>(), { msg: 'hello' })
</script>"#,
        );
        assert!(
            code.contains("___VERTER___defineProps_Type=___VERTER___Prettify<{ msg: string }>"),
            "should emit defineProps type alias: {}",
            code
        );
        assert!(
            code.contains(
                "___VERTER___withDefaults_Boxed=___VERTER___withDefaults_Box(defineProps<___VERTER___defineProps_Type>(), { msg: 'hello' })"
            ),
            "should box with both defineProps call and defaults arg: {}",
            code
        );
    }

    // ── is_simple_type_reference Tests ───────────────────────────

    #[test]
    fn simple_type_ref_detection() {
        assert!(is_simple_type_reference("Props"));
        assert!(is_simple_type_reference("MyType"));
        assert!(is_simple_type_reference("Foo.Bar"));
        assert!(!is_simple_type_reference("{ msg: string }"));
        assert!(!is_simple_type_reference("string | number"));
        assert!(!is_simple_type_reference("Array<string>"));
        assert!(!is_simple_type_reference(""));
        assert!(!is_simple_type_reference("  "));
    }

    // ── Part H: ___VERTER___Comp condition guards ────────────────────

    #[test]
    fn comp_v_if_gets_narrowing_guard() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
const isTypeA = true
</script>
<template><div v-if="isTypeA">A</div></template>"#,
        );
        // Comp function should have condition guard
        assert!(
            tc.contains("if(!((isTypeA))) return null;"),
            "Comp for v-if should have condition guard, got:\n{}",
            tc
        );
    }

    #[test]
    fn comp_v_else_if_negates_prior_siblings() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
const isTypeA = true
const isTypeB = true
</script>
<template>
  <div v-if="isTypeA">A</div>
  <div v-else-if="isTypeB">B</div>
</template>"#,
        );
        // v-else-if Comp should negate prior v-if and include own condition
        assert!(
            tc.contains("!((isTypeA)) && (isTypeB)"),
            "Comp for v-else-if should negate prior v-if, got:\n{}",
            tc
        );
    }

    #[test]
    fn comp_v_else_negates_all_prior() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
const isTypeA = true
</script>
<template>
  <div v-if="isTypeA">A</div>
  <div v-else>B</div>
</template>"#,
        );
        // v-else Comp should negate all prior conditions
        assert!(
            tc.contains("if(!(!((isTypeA)))) return null;"),
            "Comp for v-else should negate prior v-if, got:\n{}",
            tc
        );
    }

    #[test]
    fn comp_nested_v_if_combines_parent_and_own() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
const parent = true
const child = true
</script>
<template><div v-if="parent"><span v-if="child">nested</span></div></template>"#,
        );
        // Nested Comp should combine parent + own condition
        // The span's Comp should have: if(!((parent) && (child))) return null;
        assert!(
            tc.contains("(parent) && (child)"),
            "nested Comp should combine parent + own condition, got:\n{}",
            tc
        );
    }

    #[test]
    fn comp_all_elements_get_functions_not_just_root() {
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
const msg = 'hello'
</script>
<template><div><span>inner</span></div></template>"#,
        );
        // Both div and span should get Comp functions
        let comp_count = tc.matches("function ___VERTER___Comp").count();
        assert!(
            comp_count >= 2,
            "should emit Comp for all elements (div + span), found {} Comp functions, got:\n{}",
            comp_count,
            tc
        );
    }

    #[test]
    fn no_script_blocks_has_type_constructs() {
        let (code, bindings, type_constructs) =
            gen_tsx_script_full(r#"<template><div>hello</div></template>"#);

        // OXC validation: code + type_constructs must parse as valid TSX
        let full = format!("{}\n{}", code, type_constructs);
        let val_alloc = oxc_allocator::Allocator::new();
        let parsed =
            oxc_parser::Parser::new(&val_alloc, &full, oxc_span::SourceType::tsx()).parse();
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

        // Positive: minimal wrapper
        assert!(
            code.contains("___VERTER___TemplateBindingFN"),
            "should emit wrapper fn"
        );
        // Positive: helper imports
        assert!(
            code.contains("from \"@verter/types\""),
            "should import from @verter/types"
        );
        assert!(code.contains("from \"vue\""), "should import from vue");
        // Positive: type constructs
        assert!(
            type_constructs.contains("___VERTER___TemplateBinding"),
            "should emit TemplateBinding"
        );
        assert!(
            type_constructs.contains("___VERTER___FullContextFN"),
            "should emit FullContext"
        );
        assert!(
            type_constructs.contains("___VERTER___getRootComponent"),
            "should emit getRootComponent"
        );
        assert!(
            type_constructs.contains("___VERTER___default_Component"),
            "should emit default_Component"
        );
        assert!(
            type_constructs.contains("___VERTER___Instance"),
            "should emit Instance"
        );
        assert!(
            type_constructs.contains("export default"),
            "should have default export"
        );
        // Negative: no macro imports (template-only has no macros)
        assert!(
            !code.contains("createMacroReturn"),
            "should NOT import createMacroReturn"
        );
        // Bindings should be empty
        assert!(
            bindings.is_empty(),
            "template-only SFC should have no bindings"
        );
    }

    #[test]
    fn no_script_blocks_with_slot_and_style() {
        let (code, _, type_constructs) = gen_tsx_script_full(
            r#"<template><div class="wrapper"><slot /></div></template>
<style scoped>.wrapper { padding: 20px; }</style>"#,
        );

        let full = format!("{}\n{}", code, type_constructs);
        let val_alloc = oxc_allocator::Allocator::new();
        let parsed =
            oxc_parser::Parser::new(&val_alloc, &full, oxc_span::SourceType::tsx()).parse();
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

        assert!(
            code.contains("___VERTER___TemplateBindingFN"),
            "should emit wrapper"
        );
        assert!(
            type_constructs.contains("___VERTER___TemplateBinding"),
            "should emit TemplateBinding"
        );
        assert!(
            type_constructs.contains("export default"),
            "should export default Component"
        );
    }

    // ── types_module_name tests ─────────────────────────────────────

    /// Generate TSX script with custom options and return (code, bindings, type_constructs).
    fn gen_tsx_script_full_with_options(
        source: &str,
        options: TsxScriptOptions<'_>,
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

        let result = generate_tsx_script(
            syntax.script(),
            syntax.script_setup(),
            syntax.template_ast(),
            source,
            &mut ct,
            &alloc,
            &options,
            None, // template_end: tests use legacy two-CT pattern
        );

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
            TsxScriptOptions {
                component_name: "App",
                js_component_name: "App",
                scope_id: "data-v-abc123",
                has_scoped_style: false,
                runtime_module_name: "vue",
                types_module_name: "@custom/types",
                is_vapor: false,
                embed_ambient_types: true,
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
        let parsed =
            oxc_parser::Parser::new(&val_alloc, &full, oxc_span::SourceType::tsx()).parse();
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
        assert!(code.contains(r#"from "vue""#), "should import vue");
        // Positive: type constructs
        assert!(
            type_constructs.contains("___VERTER___TemplateBinding"),
            "TemplateBinding type construct missing"
        );
        assert!(
            type_constructs.contains("___VERTER___FullContextFN"),
            "FullContext type construct missing"
        );
        assert!(
            type_constructs.contains("export default"),
            "default export missing"
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
        let (_, _, tc) = gen_tsx_script_full(
            r#"<script>export default { data() { return { x: 1 } } }</script>
<template><div><span>inner</span></div></template>"#,
        );
        assert!(
            tc.contains("function ___VERTER___Comp"),
            "should emit Comp functions, got:\n{}",
            tc
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

        // Both should have type constructs
        assert!(
            opt_tc.contains("___VERTER___TemplateBinding"),
            "Options API should have TemplateBinding"
        );
        assert!(
            tpl_tc.contains("___VERTER___TemplateBinding"),
            "template-only should have TemplateBinding"
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
            code.contains("import MyComponent from './MyComponent.vue'"),
            "companion import should be hoisted: {code}"
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

    // ── #13: Async wrapper function ──────────────────────────────────

    // @ai-generated — Async setup must produce async wrapper function.
    #[test]
    fn script_setup_async_emits_async_wrapper() {
        let (code, _) = gen_tsx_script(
            r#"<script setup>
const data = await fetch('/api')
</script>"#,
        );
        assert!(
            code.contains("async function ___VERTER___TemplateBindingFN"),
            "async setup must emit async wrapper function: {code}"
        );
    }

    #[test]
    fn script_setup_sync_does_not_emit_async_wrapper() {
        let (code, _) = gen_tsx_script(
            r#"<script setup>
const x = 1
</script>"#,
        );
        assert!(
            !code.contains("async function"),
            "sync setup must NOT have async keyword: {code}"
        );
    }

    // ── #11: Angle bracket type assertions ───────────────────────────

    // @ai-generated — TSTypeAssertion <T>expr must be rewritten to (expr as T).
    #[test]
    fn script_setup_ts_type_assertion_rewrite_simple() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const value = <string>someExpr
</script>"#,
        );
        assert!(
            code.contains("(someExpr as string)"),
            "should rewrite <string>someExpr to (someExpr as string): {code}"
        );
        assert!(
            !code.contains("<string>someExpr"),
            "angle bracket assertion must not remain: {code}"
        );
    }

    #[test]
    fn script_setup_ts_type_assertion_rewrite_union() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
let a = <1 | 2>1
</script>"#,
        );
        assert!(
            code.contains("(1 as 1 | 2)"),
            "should rewrite <1|2>1 to (1 as 1|2): {code}"
        );
        assert!(
            !code.contains("<1 | 2>"),
            "angle bracket assertion must not remain: {code}"
        );
    }

    #[test]
    fn script_setup_ts_type_assertion_nested() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
let b = <string><number>x
</script>"#,
        );
        // Nested: <string><number>x → ((<number>x as string) → ((x as number) as string)
        assert!(
            code.contains("as number") && code.contains("as string"),
            "nested assertions should both be rewritten: {code}"
        );
        assert!(
            !code.contains("<string>") && !code.contains("<number>"),
            "angle bracket syntax must not remain: {code}"
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
            type_constructs.contains("export declare function createMacroReturn"),
            "ambient module must export createMacroReturn"
        );
        assert!(
            type_constructs.contains("export declare function shallowUnwrapRef"),
            "ambient module must export shallowUnwrapRef"
        );
        assert!(
            type_constructs.contains("export type PublicInstanceFromMacro"),
            "ambient module must export PublicInstanceFromMacro"
        );
        assert!(
            type_constructs.contains("export declare function defineProps_Box"),
            "ambient module must export defineProps_Box"
        );
        assert!(
            type_constructs.contains("export declare function defineEmits_Box"),
            "ambient module must export defineEmits_Box"
        );
        assert!(
            type_constructs.contains("export declare function defineModel_Box"),
            "ambient module must export defineModel_Box"
        );
        assert!(
            type_constructs.contains("export declare function defineSlots_Box"),
            "ambient module must export defineSlots_Box"
        );
        assert!(
            type_constructs.contains("export declare function defineExpose_Box"),
            "ambient module must export defineExpose_Box"
        );
        assert!(
            type_constructs.contains("export declare function withDefaults_Box"),
            "ambient module must export withDefaults_Box"
        );
        assert!(
            type_constructs.contains("export declare function defineOptions_Box"),
            "ambient module must export defineOptions_Box"
        );
        assert!(
            type_constructs.contains("export declare function enhanceElementWithProps"),
            "ambient module must export enhanceElementWithProps"
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
        let (_, _, type_constructs) =
            gen_tsx_script_full(r#"<template><div>hello</div></template>"#);

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
            TsxScriptOptions {
                component_name: "App",
                js_component_name: "App",
                scope_id: "data-v-abc123",
                has_scoped_style: false,
                runtime_module_name: "vue",
                types_module_name: "@verter/types",
                is_vapor: false,
                embed_ambient_types: false,
            },
        );

        assert!(
            !type_constructs.contains(r#"declare module "@verter/types""#),
            "ambient module should NOT be emitted when embed_ambient_types=false"
        );
    }

    #[test]
    fn create_macro_return_ambient_signature_matches_real() {
        let (_, _, type_constructs) = gen_tsx_script_full(
            r#"<script setup lang="ts">
const props = defineProps<{ msg: string }>()
</script>
<template><div>{{ props.msg }}</div></template>"#,
        );

        // The ambient createMacroReturn must accept 1 arg and return the keyed wrapper
        assert!(
            type_constructs
                .contains("createMacroReturn<T>(o: T): { ____VERTER___MACRO_RETURN_KEY____: T }"),
            "createMacroReturn signature must match real @verter/types: {}",
            type_constructs
        );
        // Negative: old 0-arg signature must NOT be present
        assert!(
            !type_constructs.contains("createMacroReturn<T>(): T"),
            "old 0-arg createMacroReturn signature must not be present"
        );
    }

    #[test]
    fn with_defaults_box_runtime_args_two_params() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const props = withDefaults(defineProps<{ msg: string }>(), { msg: 'hello' })
</script>"#,
        );
        // The boxed call must have 2 args: defineProps call + defaults object
        assert!(
            code.contains("___VERTER___withDefaults_Box(defineProps<___VERTER___defineProps_Type>(), { msg: 'hello' })"),
            "withDefaults_Box must have 2 args (defineProps call, defaults): {}",
            code
        );
    }

    #[test]
    fn with_defaults_box_runtime_no_type_params() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const props = withDefaults(defineProps({ msg: String }), { msg: 'hello' })
</script>"#,
        );
        // TS v5 parity: inner defineProps args are boxed with defineProps_Box
        assert!(
            code.contains(
                "___VERTER___withDefaults_Box(defineProps(___VERTER___defineProps_Boxed=___VERTER___defineProps_Box({ msg: String })), { msg: 'hello' })"
            ),
            "withDefaults_Box should box inner defineProps args with defineProps_Box: {}",
            code
        );
        // [0]/[1] indexing must be present
        assert!(
            code.contains(
                "withDefaults(___VERTER___withDefaults_Boxed[0],___VERTER___withDefaults_Boxed[1])"
            ),
            "withDefaults call must use [0]/[1] indexing: {}",
            code
        );
    }

    /// @ai-generated — Reproduction: withDefaults with runtime props must use [0]/[1]
    /// indexing on the boxed variable, matching the TS v5 transformer output.
    /// Without indexing, TypeScript resolves `props` as `any` because the full
    /// `[T, Defaults]` tuple is passed where `InferDefaults<T>` is expected.
    #[test]
    fn with_defaults_runtime_props_uses_indexing() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const props = withDefaults(defineProps({ bar: String }), {})
</script>"#,
        );
        // TS v5 behavior: uses [0]/[1] indexing on the boxed variable
        assert!(
            code.contains(
                "withDefaults(___VERTER___withDefaults_Boxed[0],___VERTER___withDefaults_Boxed[1])"
            ),
            "withDefaults call must use [0]/[1] indexing (TS v5 parity): {}",
            code
        );
        // Must NOT leave the original defineProps call as an arg to withDefaults
        assert!(
            !code.contains("withDefaults(defineProps({"),
            "original defineProps call must NOT appear inside withDefaults args: {}",
            code
        );
    }

    /// @ai-generated — Reproduction: withDefaults with type params must also use [0]/[1]
    /// indexing, matching the TS v5 transformer output.
    #[test]
    fn with_defaults_type_params_uses_indexing() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const props = withDefaults(defineProps<{ msg: string }>(), { msg: 'hello' })
</script>"#,
        );
        // TS v5 behavior: uses [0]/[1] indexing for type-param case too
        assert!(
            code.contains(
                "withDefaults(___VERTER___withDefaults_Boxed[0],___VERTER___withDefaults_Boxed[1])"
            ),
            "withDefaults call with type params must use [0]/[1] indexing: {}",
            code
        );
    }

    /// @ai-generated — Reproduction: withDefaults with runtime props must box the inner
    /// defineProps args with defineProps_Box, matching the TS v5 transformer output.
    #[test]
    fn with_defaults_runtime_props_boxes_define_props_args() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const props = withDefaults(defineProps({ bar: String }), {})
</script>"#,
        );
        // TS v5 behavior: inner defineProps args wrapped with defineProps_Box
        assert!(
            code.contains("___VERTER___defineProps_Boxed=___VERTER___defineProps_Box("),
            "defineProps args must be boxed with defineProps_Box: {}",
            code
        );
        // The let declaration for defineProps_Boxed must be present
        assert!(
            code.contains("let ___VERTER___defineProps_Boxed"),
            "must declare let ___VERTER___defineProps_Boxed: {}",
            code
        );
    }

    /// @ai-generated — Dump test: withDefaults with explicit import from 'vue'
    /// (matches examples/src/props/CompWithDefaults.object.no-var.vue)
    #[test]
    fn with_defaults_explicit_vue_import_dump() {
        let (code, _) = gen_tsx_script(
            r#"<script lang="ts" setup>
import { withDefaults, defineProps } from 'vue';

const props = withDefaults(
  defineProps({
    bar: String,
  }),
  {},
);

</script>"#,
        );
        eprintln!(
            "=== TSX output for explicit import withDefaults ===\n{}\n===",
            code
        );
        // Same checks as the no-import version (multiline-aware)
        assert!(
            code.contains("___VERTER___withDefaults_Boxed[0]")
                && code.contains("___VERTER___withDefaults_Boxed[1]"),
            "withDefaults call must use [0]/[1] indexing: {}",
            code
        );
        assert!(
            !code.contains("withDefaults(\n  defineProps("),
            "original defineProps call must NOT appear inside withDefaults args: {}",
            code
        );
        // Boxing must be present
        assert!(
            code.contains("___VERTER___defineProps_Boxed=___VERTER___defineProps_Box("),
            "defineProps args must be boxed: {}",
            code
        );
        assert!(
            code.contains("___VERTER___withDefaults_Box(defineProps("),
            "withDefaults_Box must wrap the full call: {}",
            code
        );
    }

    // ── defineModel Parity Tests ──────────────────────────────────────

    /// @ai-generated — TS v5 parity: defineModel("title") must use shared
    /// `___VERTER___defineModel_Box` function, NOT per-model `___VERTER___title_defineModel_Box`.
    #[test]
    fn define_model_named_uses_shared_box_fn() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const model = defineModel("title")
</script>"#,
        );
        // Box function must be the shared defineModel_Box
        assert!(
            code.contains("___VERTER___defineModel_Box(\"title\")"),
            "must use shared ___VERTER___defineModel_Box, not per-model variant: {}",
            code
        );
        // Must NOT use per-model box function name (check with trailing paren to
        // avoid matching ___VERTER___title_defineModel_Boxed which is the const name)
        assert!(
            !code.contains("___VERTER___title_defineModel_Box("),
            "must NOT use per-model box function ___VERTER___title_defineModel_Box(): {}",
            code
        );
    }

    /// @ai-generated — TS v5 parity: defineModel("title") must use [0],[1] indexing
    /// even for name-only arg. defineModel_Box returns a tuple.
    #[test]
    fn define_model_named_uses_indexing() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const model = defineModel("title")
</script>"#,
        );
        assert!(
            code.contains(
                "defineModel(___VERTER___title_defineModel_Boxed[0],___VERTER___title_defineModel_Boxed[1])"
            ),
            "defineModel call must use [0]/[1] indexing: {}",
            code
        );
    }

    /// @ai-generated — TS v5 parity: defineModel<string>('firstName') with type params
    /// must use shared box function and [0],[1] indexing.
    #[test]
    fn define_model_typed_named_uses_shared_box_and_indexing() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const firstName = defineModel<string>('firstName')
</script>"#,
        );
        // Shared box function
        assert!(
            code.contains("___VERTER___defineModel_Box<"),
            "must use shared ___VERTER___defineModel_Box for typed model: {}",
            code
        );
        // Check for the per-model box function call pattern (with opening paren or angle bracket
        // but NOT matching ___VERTER___firstName_defineModel_Boxed which is the const name)
        assert!(
            !code.contains("firstName_defineModel_Box(")
                && !code.contains("firstName_defineModel_Box<"),
            "must NOT use per-model box function: {}",
            code
        );
        // [0],[1] indexing
        assert!(
            code.contains(
                "defineModel<___VERTER___firstName_defineModel_Type>(___VERTER___firstName_defineModel_Boxed[0],___VERTER___firstName_defineModel_Boxed[1])"
            ),
            "defineModel call must use [0]/[1] indexing: {}",
            code
        );
    }

    /// @ai-generated — TS v5 parity: multiple defineModel calls must each use
    /// the shared defineModel_Box and per-model Boxed const names.
    #[test]
    fn define_model_multiple_uses_shared_box_fn() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const firstName = defineModel<string>('firstName')
const lastName = defineModel<string>('lastName')
</script>"#,
        );
        // Both must use the SHARED defineModel_Box, not per-model variants
        assert!(
            code.contains("___VERTER___firstName_defineModel_Boxed=___VERTER___defineModel_Box<"),
            "firstName must use shared defineModel_Box: {}",
            code
        );
        assert!(
            code.contains("___VERTER___lastName_defineModel_Boxed=___VERTER___defineModel_Box<"),
            "lastName must use shared defineModel_Box: {}",
            code
        );
        // Both must use [0],[1] indexing
        assert!(
            code.contains("___VERTER___firstName_defineModel_Boxed[0],___VERTER___firstName_defineModel_Boxed[1]"),
            "firstName defineModel must use [0]/[1] indexing: {}",
            code
        );
        assert!(
            code.contains("___VERTER___lastName_defineModel_Boxed[0],___VERTER___lastName_defineModel_Boxed[1]"),
            "lastName defineModel must use [0]/[1] indexing: {}",
            code
        );
    }

    // ── E2E Macro Type Checking ───────────────────────────────────────

    /// @ai-generated — TS v5 parity: defineProps with runtime args must produce
    /// correct types (not any).
    #[test]
    fn define_props_runtime_args_type_not_any() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const props = defineProps({ msg: String })
</script>"#,
        );
        // defineProps with runtime args should be boxed
        assert!(
            code.contains(
                "___VERTER___defineProps_Boxed=___VERTER___defineProps_Box({ msg: String })"
            ),
            "defineProps runtime args must be boxed: {}",
            code
        );
        // createMacroReturn should reference the boxed name
        assert!(
            code.contains("typeof ___VERTER___defineProps_Boxed"),
            "createMacroReturn must use typeof defineProps_Boxed: {}",
            code
        );
    }

    /// @ai-generated — TS v5 parity: defineEmits with runtime args must produce
    /// correct types (not any).
    #[test]
    fn define_emits_runtime_args_type_not_any() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
const emit = defineEmits(['change', 'update'])
</script>"#,
        );
        // defineEmits with runtime args should be boxed
        assert!(
            code.contains("___VERTER___defineEmits_Boxed=___VERTER___defineEmits_Box("),
            "defineEmits runtime args must be boxed: {}",
            code
        );
        // createMacroReturn should reference the boxed name
        assert!(
            code.contains("typeof ___VERTER___defineEmits_Boxed"),
            "createMacroReturn must use typeof defineEmits_Boxed: {}",
            code
        );
    }

    /// @ai-generated — TS v5 parity: defineExpose must produce correct types.
    #[test]
    fn define_expose_args_type_not_any() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
defineExpose({ focus: () => {} })
</script>"#,
        );
        // defineExpose with args should be boxed
        assert!(
            code.contains("___VERTER___defineExpose_Boxed=___VERTER___defineExpose_Box("),
            "defineExpose args must be boxed: {}",
            code
        );
    }
}
