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
//! // Temp variable (outside block scope to avoid TDZ)
//! const ___VERTER___unwrapped = ___VERTER___shallowUnwrapRef({
//!     /** My counter */
//!     count: count as unknown as typeof count,
//! });
//!
//! // Exported TemplateBinding wrapper function
//! ;export function ___VERTER___TemplateBindingFN() {
//!   // Setup body (macros boxed, bindings extracted)
//!   ;type ___VERTER___defineProps_Type=___VERTER___Prettify<Props>;
//!   const props = defineProps<___VERTER___defineProps_Type>()
//!   const count = ref(0)
//!
//!   // Block scope: destructure from temp with offset comments, then template JSX
//!   { const {
//!     /*45,50*/
//!     count } = ___VERTER___unwrapped;
//!     <div>{ count }</div>
//!   } // close block scope
//! } // close templateBindingFN
//! ```

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, BindingPattern, CallExpression, Declaration, ExportDefaultDeclarationKind,
    Expression, ForStatementInit, Function, ObjectPropertyKind, Statement,
};
use oxc_ast::{Comment, CommentContent};
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

use super::{event_to_jsx_name, get_directive_name, IdeGenericInfo, IdeScriptOptions};

// ── Macro State Types ────────────────────────────────────────────

/// Accumulated macro processing info for type constructs.
#[derive(Debug, Default)]
struct TsxMacroState {
    /// Per-macro binding info.
    macro_bindings: Vec<MacroBindingEntry>,
    /// DefineModel entries.
    model_bindings: Vec<ModelBindingEntry>,
    /// Whether `defineOptions({ inheritAttrs: false })` was detected.
    has_inherit_attrs_false: bool,
}

/// Info about a macro binding (defineProps, defineEmits, defineSlots, withDefaults).
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct MacroBindingEntry {
    /// Original macro name: "defineProps", "defineEmits", etc.
    macro_name: String,
    /// Variable name holding the macro result (e.g., `props` or `___VERTER___props`).
    var_name: Option<String>,
    /// Type alias name if type params were used (e.g., `___VERTER___defineProps_Type`).
    type_name: Option<String>,
    /// Whether this macro used type params.
    is_type: bool,
}

/// Info about a defineModel binding.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ModelBindingEntry {
    /// Model name (e.g., "modelValue" or "title").
    model_name: String,
    /// Variable name holding the model ref.
    var_name: String,
    /// Type alias name if type params were used.
    type_name: Option<String>,
    /// Whether this model used type params.
    is_type: bool,
}

/// Shared context for macro processing functions.
///
/// Groups the 4 parameters threaded through `process_single_macro`,
/// `process_standard_macro`, `process_define_model`, and `process_with_defaults`.
struct MacroSourceCtx<'a, 'alloc> {
    source: &'a str,
    content_str: &'a str,
    content_start: u32,
    out: &'a mut CodeGenOutput<'alloc>,
    is_jsx: bool,
}

/// Result of TSX script generation (internal, before building string).
pub struct IdeScriptGenResult<'alloc> {
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
pub fn generate_ide_script<'alloc>(
    script: Option<&RootNodeScript>,
    script_setup: Option<&RootNodeScript>,
    template_ast: Option<&TemplateAst>,
    source: &'alloc str,
    ct: &mut CodeTransform<'alloc>,
    alloc: &'alloc Allocator,
    options: &IdeScriptOptions<'_>,
    template_end: Option<u32>,
) -> IdeScriptGenResult<'alloc> {
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
            emit_helper_imports(&mut out, 0, options, &builtin_components, template_ast);
            emit_type_constructs(
                &mut type_constructs,
                &None, // no generics
                &None, // no attrs
                source,
                options,
                false, // no getCurrentInstance
            );
        }
    }

    // Apply accumulated operations
    out.apply_to(ct);

    IdeScriptGenResult {
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
    options: &IdeScriptOptions<'_>,
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

    // Rewrite `<Type>expr` angle bracket assertions to `(expr as Type)` for TSX validity.
    // Must run BEFORE the error check because angle bracket assertions like `<string>x`
    // cause OXC TSX parse errors (parsed as JSX), but are valid TS. The rewrite uses a
    // separate TS-mode parse and modifies ct directly.
    // Skip in JSX mode: JS files don't have angle-bracket type assertions.
    if !options.is_jsx {
        rewrite_ts_type_assertions(content_str, content_start, ct);
    }

    // ── Error Recovery: File-Scope Mode ──────────────────────────
    // When OXC has parse errors (e.g. `count.` during typing), it returns
    // an empty or partial AST. In this case, keep the script body at file
    // scope so TS can still resolve variables for IntelliSense, and emit
    // a minimal TemplateBindingFN wrapper for the template only.
    //
    // Only enter error mode if the TS-mode parse also has errors. If only the
    // TSX parse fails (but TS succeeds), the errors are from angle bracket
    // type assertions which `rewrite_ts_type_assertions` already handled.
    if !parser_ret.errors.is_empty() {
        let ts_alloc = Allocator::default();
        let ts_check = Parser::new(&ts_alloc, content_str, SourceType::ts()).parse();
        if !ts_check.errors.is_empty() {
            return process_tsx_script_setup_error_mode(
                setup,
                source,
                out,
                type_constructs,
                options,
                builtin_components,
                template_end,
                hoist_pos,
            );
        }
    }

    let parse_result = parse_script_with_companion(
        &parser_ret.program,
        ScriptMode::Setup,
        content_start,
        content_str,
        None, // No companion types needed for TSX — we preserve types as-is
    );

    // Build binding source info for JSDoc + offset comments
    let binding_source_info = build_binding_source_info(
        &parser_ret.program.body,
        &parser_ret.program.comments,
        content_str,
        content_start,
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
            match IdeGenericInfo::from_source(generic_str) {
                Some(info) => (Some(info), None),
                None => (None, Some(trimmed.to_string())),
            }
        }
    } else {
        (None, None)
    };

    // Extract attrs attribute value for typed $attrs.
    // Priority: `attrs` attribute > `useAttrs<T>()` > `{}` (default)
    let attrs_type = setup
        .attrs
        .and_then(|span| {
            let s = &source[span.start as usize..span.end as usize].trim();
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        })
        .or_else(|| detect_use_attrs_type_arg(&parser_ret.program.body, content_str));

    // Process macros: emit type aliases only (no boxing)
    let mut macro_ctx = MacroSourceCtx {
        source,
        content_str,
        content_start,
        out,
        is_jsx: options.is_jsx,
    };
    let macro_state = process_macros(&parse_result.items, &mut macro_ctx);
    let out = macro_ctx.out;

    // Detect getCurrentInstance() usage for conditional type emission
    let has_get_current_instance = detect_get_current_instance(&parser_ret.program.body);

    // Build component function wrapper opening
    // Replace <script setup> tag with ___VERTER___TemplateBindingFN function declaration.
    // Use parsed generic if available, otherwise raw fallback for invalid syntax.
    // In JSX mode, drop generics (no TypeScript syntax in JS output).
    let generic_bracket = if options.is_jsx {
        String::new()
    } else {
        generic_info
            .as_ref()
            .map(|g| g.source_bracket())
            .or_else(|| raw_generic.as_ref().map(|r| format!("<{}>", r)))
            .unwrap_or_default()
    };
    let async_prefix = if parse_result.is_async { "async " } else { "" };
    let wrapper_start = format!(
        ";export {}function {}TemplateBindingFN{}() {{\n",
        async_prefix, PREFIX, generic_bracket,
    );
    out.overwrite(setup.tag_open.start, setup.tag_open.end, &wrapper_start);

    // Replace </script> tag with block scope opening; close deferred to template end
    if let Some(tag_close) = &setup.tag_close {
        let mut wrapper_end = String::with_capacity(512);

        // Inject __props alias so template codegen's `__props.xxx` references resolve.
        if let Some(props_var) = macro_state
            .macro_bindings
            .iter()
            .find(|e| e.macro_name == "defineProps")
            .and_then(|e| e.var_name.as_deref())
        {
            wrapper_end.push_str(&format!("\nconst __props = {};", props_var));
        }

        // Declare ___VERTER___instance for instance property access in template.
        let has_template = template_ast.is_some();
        wrapper_end.push_str(&instance_declaration(
            options.filename,
            options.is_jsx,
            has_template,
        ));

        // Build block scope with shallowUnwrapRef destructuring.
        // Includes ALL setup bindings except Props/PropsAliased (accessed via __props).
        // Imports are already in scope from hoisting, so they're excluded too.
        // Non-template bindings are intentionally included so that:
        //  1. IntelliSense always shows unwrapped types in the template
        //  2. TS flags unused destructured bindings (the LSP remaps these
        //     diagnostics to the original declaration via the offset comments)
        let import_names: FxHashSet<&str> = parse_result
            .items
            .iter()
            .filter_map(|item| {
                if let ScriptItem::Import(imp) = item {
                    Some(imp.bindings.iter().map(|b| b.name))
                } else {
                    None
                }
            })
            .flatten()
            .collect();

        let setup_bindings: Vec<(&str, BindingType)> = bindings
            .iter()
            .filter(|(_, bt)| !bt.is_props() && !matches!(bt, BindingType::PropsAliased))
            .filter(|(name, _)| {
                let n: &str = name;
                !import_names.contains(n)
            })
            .map(|(name, bt)| (*name, *bt))
            .collect();

        if !setup_bindings.is_empty() {
            let entries: String = setup_bindings
                .iter()
                .map(|(name, _)| {
                    let jsdoc = binding_source_info
                        .get(name)
                        .and_then(|info| info.jsdoc.as_deref());
                    if options.is_jsx {
                        // JSX mode: plain binding (no TS cast)
                        if let Some(jsdoc) = jsdoc {
                            format!("{}\n    {}: {}", jsdoc, name, name)
                        } else {
                            format!("{}: {}", name, name)
                        }
                    } else if let Some(jsdoc) = jsdoc {
                        format!(
                            "{}\n    {}: {} as unknown as typeof {}",
                            jsdoc, name, name, name
                        )
                    } else {
                        format!("{}: {} as unknown as typeof {}", name, name, name)
                    }
                })
                .collect::<Vec<_>>()
                .join(",\n    ");

            // Temp variable OUTSIDE block scope — avoids TDZ where
            // `const { count } = shallowUnwrapRef({ count: count })` would
            // self-reference the uninitialized block-scoped `count`.
            wrapper_end.push_str(&format!(
                "\nconst {P}unwrapped = {P}shallowUnwrapRef({{\n    {entries}\n  }});\n",
                P = PREFIX,
                entries = entries,
            ));
            // Block scope with destructuring FROM the temp variable.
            // Each binding gets an offset comment /*sfc_start,sfc_end*/ for go-to-definition
            // AND for the LSP to remap "unused variable" diagnostics to the declaration site.
            //
            // Split into `const` (truly immutable: SetupConst, LiteralConst) and
            // `let` (assignable: SetupRef, SetupLet, SetupReactiveConst, SetupMaybeRef)
            // so that v-model assignment handlers don't trigger TS2588.
            let const_names: Vec<&str> = setup_bindings
                .iter()
                .filter(|(_, bt)| matches!(bt, BindingType::SetupConst | BindingType::LiteralConst))
                .map(|(name, _)| *name)
                .collect();
            let let_names: Vec<&str> = setup_bindings
                .iter()
                .filter(|(_, bt)| {
                    !matches!(bt, BindingType::SetupConst | BindingType::LiteralConst)
                })
                .map(|(name, _)| *name)
                .collect();

            let format_destruct_entries = |names: &[&str]| -> String {
                names
                    .iter()
                    .map(|name| {
                        if let Some(info) = binding_source_info.get(name) {
                            format!(
                                "\n    /*{},{}*/\n    {}",
                                info.sfc_start, info.sfc_end, name
                            )
                        } else {
                            format!("\n    {}", name)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(",")
            };

            let mut destruct_block = String::from("{ ");
            if !const_names.is_empty() {
                destruct_block.push_str(&format!(
                    "const {{ {} }} = {P}unwrapped;",
                    format_destruct_entries(&const_names),
                    P = PREFIX,
                ));
            }
            if !let_names.is_empty() {
                if !const_names.is_empty() {
                    destruct_block.push(' ');
                }
                destruct_block.push_str(&format!(
                    "let {{ {} }} = {P}unwrapped;",
                    format_destruct_entries(&let_names),
                    P = PREFIX,
                ));
            }
            destruct_block.push('\n');
            wrapper_end.push_str(&destruct_block);
        } else {
            wrapper_end.push_str("\n{\n");
        }

        if template_end.is_some() {
            // Unified CT: emit block scope opening at </script>.
            out.overwrite(tag_close.start, tag_close.end, &wrapper_end);

            // Build deferred close: } (block scope) + Comp functions + global fallbacks + } (function)
            let mut tail = String::with_capacity(512);
            tail.push_str("\n} // close block scope\n"); // close block scope

            // Emit Comp functions + getRootComponent inside templateBindingFN
            // In JSX mode, drop generics (no TypeScript syntax in JS output).
            let gs = if options.is_jsx {
                String::new()
            } else {
                generic_info
                    .as_ref()
                    .map(|g| g.source_bracket())
                    .unwrap_or_default()
            };
            let gn = if options.is_jsx {
                String::new()
            } else {
                generic_info
                    .as_ref()
                    .map(|g| g.names_bracket())
                    .unwrap_or_default()
            };
            let (root_comp_offset, root_props_literal, all_comp_offsets) =
                emit_comp_functions_to_string(
                    &mut tail,
                    &gs,
                    &gn,
                    template_ast,
                    source,
                    options.is_jsx,
                );
            // Always emit getRootComponent when there's a template (needed for implicit attrs)
            if template_ast.is_some() {
                emit_get_root_component_to_string(
                    &mut tail,
                    &gs,
                    &gn,
                    root_comp_offset,
                    root_props_literal.as_deref(),
                );
            }

            // Emit RootElement/RootElementProps/Attrs types inside the function scope
            // (these reference getRootComponent which is function-local)
            if template_ast.is_some() && !options.is_jsx {
                let inherit_attrs = !macro_state.has_inherit_attrs_false;
                emit_attrs_type_aliases(&mut tail, &generic_info, inherit_attrs);
            }

            // Emit void references to suppress unused warnings for Comp/getRootComponent
            if template_ast.is_some() {
                tail.push_str(&format!(
                    "\nvoid {P}getRootComponent; void {P}getRootComponentPassedProps;",
                    P = PREFIX,
                ));
            }
            for offset in &all_comp_offsets {
                tail.push_str(&format!(
                    "\nvoid {P}Comp{offset};",
                    P = PREFIX,
                    offset = offset,
                ));
            }

            // Emit global component fallback consts
            emit_global_component_fallbacks(
                &mut tail,
                template_ast,
                source,
                bindings,
                options.is_jsx,
            );

            // Emit instance completion probe line (LSP uses this for autocomplete)
            tail.push_str(&instance_probe_line());

            tail.push_str("\n} // close templateBindingFN\n");
            deferred_return_close = Some(tail);
        } else {
            // No template: emit block scope + close immediately.
            wrapper_end.push_str("\n} // close block scope\n");
            wrapper_end.push_str("\n} // close templateBindingFN\n");
            out.overwrite(tag_close.start, tag_close.end, &wrapper_end);
        }
    }

    // Emit helper imports (hoisted before wrapper)
    emit_helper_imports(out, hoist_pos, options, builtin_components, template_ast);

    // Emit type constructs (appended after source map, no sourcemap needed)
    emit_type_constructs(
        type_constructs,
        &generic_info,
        &attrs_type,
        source,
        options,
        has_get_current_instance,
    );

    deferred_return_close
}

// ── Script Setup Error Recovery ─────────────────────────────────

/// Error recovery mode for `<script setup>` when OXC has parse errors.
///
/// Keeps the script body at **file scope** (no function wrapper) so
/// TypeScript can still resolve variables for IntelliSense completions.
/// Emits a minimal `___VERTER___TemplateBindingFN` wrapper for the template
/// only. Skips shallowUnwrapRef destructuring, macro processing, and
/// binding extraction since the OXC AST is unreliable.
#[allow(clippy::too_many_arguments)]
fn process_tsx_script_setup_error_mode(
    setup: &RootNodeScript,
    source: &str,
    out: &mut CodeGenOutput<'_>,
    type_constructs: &mut String,
    options: &IdeScriptOptions<'_>,
    builtin_components: &[&str],
    template_end: Option<u32>,
    hoist_pos: u32,
) -> Option<String> {
    // Replace <script setup> tag with newline — script body stays at file scope.
    out.overwrite(setup.tag_open.start, setup.tag_open.end, "\n");

    // Replace </script> tag with TemplateBindingFN wrapper for template.
    let mut deferred_return_close: Option<String> = None;
    if let Some(tag_close) = &setup.tag_close {
        if template_end.is_some() {
            // Template exists: open the wrapper, defer the close
            let mut wrapper_open = format!("\nexport function {}TemplateBindingFN() {{\n", PREFIX);
            // Declare instance for instance property access in template.
            // Error mode: no Comp functions, so no $attrs override
            wrapper_open.push_str(&instance_declaration(
                options.filename,
                options.is_jsx,
                false,
            ));
            out.overwrite(tag_close.start, tag_close.end, &wrapper_open);
            let mut close = String::from("\n");
            close.push_str(&instance_probe_line());
            close.push_str("} // close templateBindingFN\n");
            deferred_return_close = Some(close);
        } else {
            // No template: just remove the tag
            out.overwrite(tag_close.start, tag_close.end, "\n");
        }
    }

    // Emit helper imports (hoisted before script body).
    // In error mode we still import shallowUnwrapRef — it's harmless and
    // avoids the need for a separate helper-import variant.
    emit_helper_imports(out, hoist_pos, options, builtin_components, None);

    // Emit minimal type constructs (instance type for self-import).
    emit_type_constructs(
        type_constructs,
        &None, // no generic info
        &None, // no attrs
        source,
        options,
        false, // no getCurrentInstance detection
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
    _tag_name: &str,
    _source: &str,
    _available_bindings: &FxHashSet<String>,
) -> String {
    // Elements with refs always get a ___VERTER___Comp{offset} build-node function emitted
    // (see walk_children_for_comp). Using ReturnType gives the correct type:
    // - For native elements: the enhanced element type with props
    // - For components: the component instance type (new Component({props}))
    let offset = el.tag_open.start;
    format!("ReturnType<typeof {}Comp{}>", PREFIX, offset)
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
    options: &IdeScriptOptions<'_>,
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
        let mut close = String::with_capacity(128);
        close.push_str("\nexport default __sfc__;\n");
        // Ambient instance declaration for template property access.
        close.push_str(&instance_declaration_ambient(
            options.filename,
            options.is_jsx,
        ));
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
    emit_helper_imports(out, hoist_pos, options, builtin_components, template_ast);

    emit_type_constructs(
        type_constructs,
        &None, // no generics (Options API can't have them)
        &None, // no attrs (Options API doesn't use script attrs)
        source,
        options,
        false, // no getCurrentInstance detection for Options API
    );
}

// ── Macro Boxing ──────────────────────────────────────────────────

/// Process all macros in the parsed items: emit type aliases (no boxing).
fn process_macros(items: &[ScriptItem<'_>], ctx: &mut MacroSourceCtx<'_, '_>) -> TsxMacroState {
    let mut state = TsxMacroState::default();

    for item in items {
        if let ScriptItem::Macro(mac) = item {
            process_single_macro(mac, ctx, &mut state);
        }
    }

    state
}

/// Process a single macro call: emit type alias if type params present.
fn process_single_macro(
    mac: &ScriptMacro<'_>,
    ctx: &mut MacroSourceCtx<'_, '_>,
    state: &mut TsxMacroState,
) {
    match mac {
        ScriptMacro::DefineProps {
            span,
            declarator,
            type_params,
            object_arg: _,
            array_arg: _,
        } => {
            let entry = process_standard_macro(
                "defineProps",
                "props",
                *span,
                declarator.as_ref(),
                type_params.as_ref(),
                false,
                ctx,
            );
            state.macro_bindings.push(entry);
        }
        ScriptMacro::DefineEmits {
            span,
            declarator,
            type_params,
            object_arg: _,
            array_arg: _,
        } => {
            let entry = process_standard_macro(
                "defineEmits",
                "emits",
                *span,
                declarator.as_ref(),
                type_params.as_ref(),
                false,
                ctx,
            );
            state.macro_bindings.push(entry);
        }
        ScriptMacro::DefineSlots {
            span,
            declarator,
            type_params,
        } => {
            let entry = process_standard_macro(
                "defineSlots",
                "slots",
                *span,
                declarator.as_ref(),
                type_params.as_ref(),
                false,
                ctx,
            );
            state.macro_bindings.push(entry);
        }
        ScriptMacro::DefineExpose {
            span,
            declarator,
            object_arg: _,
        } => {
            let entry = process_standard_macro(
                "defineExpose",
                "expose",
                *span,
                declarator.as_ref(),
                None,
                true,
                ctx,
            );
            state.macro_bindings.push(entry);
        }
        ScriptMacro::DefineOptions {
            span,
            declarator,
            object_arg: _,
            has_inherit_attrs_false,
        } => {
            if *has_inherit_attrs_false {
                state.has_inherit_attrs_false = true;
            }
            let entry = process_standard_macro(
                "defineOptions",
                "options",
                *span,
                declarator.as_ref(),
                None,
                true,
                ctx,
            );
            state.macro_bindings.push(entry);
        }
        ScriptMacro::DefineModel {
            span,
            declarator,
            type_params,
            name_span,
            options_span: _,
        } => {
            process_define_model(
                *span,
                declarator.as_ref(),
                type_params.as_ref(),
                *name_span,
                ctx,
                state,
            );
        }
        ScriptMacro::WithDefaults {
            span,
            declarator,
            define_props_span: _,
            define_props_type_params,
            defaults: _,
            defaults_arg_span: _,
        } => {
            process_with_defaults(
                *span,
                declarator.as_ref(),
                define_props_type_params.as_ref(),
                ctx,
                state,
            );
        }
    }
}

/// Process a standard macro (defineProps, defineEmits, defineSlots, defineExpose, defineOptions).
///
/// For type params: emit type alias, replace type param in call with alias name.
/// For no-declarator non-no-return macros: prepend `const ___VERTER___xxx=`.
#[allow(clippy::too_many_arguments)]
fn process_standard_macro(
    macro_name: &str,
    var_suffix: &str,
    call_span: crate::common::Span,
    declarator: Option<&MacroDeclarator<'_>>,
    type_params: Option<&MacroTypeParams>,
    is_no_return: bool,
    ctx: &mut MacroSourceCtx<'_, '_>,
) -> MacroBindingEntry {
    let type_name_str = format!("{}{}_Type", PREFIX, macro_name);
    let auto_var_name = format!("{}{}", PREFIX, var_suffix);

    let has_type_params = type_params.is_some();

    // Determine the statement start position for prepending
    let stmt_start = declarator
        .map(|d| ctx.content_start + d.statement_span.start)
        .unwrap_or(ctx.content_start + call_span.start);

    // Emit type alias for type params
    if let Some(tp) = type_params {
        if ctx.is_jsx {
            // JSX mode: remove the generic type brackets entirely (JS has no generics)
            ctx.out.overwrite(tp.lt_span.start, tp.gt_span.end, "");
        } else {
            let type_text = &ctx.source[tp.type_span.start as usize..tp.type_span.end as usize];
            let needs_prettify = !is_simple_type_reference(type_text);

            let prefix = if needs_prettify {
                format!(";type {}={}Prettify<", type_name_str, PREFIX)
            } else {
                format!(";type {}=", type_name_str)
            };
            let suffix = if needs_prettify { ">;" } else { ";" };

            // Move original type content to stmt_start, wrapped with type declaration.
            // This preserves fine-grained sourcemap for hover on individual properties.
            ctx.out.move_wrapped(
                tp.type_span.start,
                tp.type_span.end,
                stmt_start,
                &prefix,
                suffix,
            );

            // Fill the gap left by the move with the type alias name
            ctx.out.prepend_alloc(tp.type_span.start, &type_name_str);
        }
    }

    // Add variable assignment if no declarator and not a no-return macro
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
        is_type: has_type_params,
    }
}

/// Process defineModel macro.
fn process_define_model(
    call_span: crate::common::Span,
    declarator: Option<&MacroDeclarator<'_>>,
    type_params: Option<&MacroTypeParams>,
    name_span: Option<crate::common::Span>,
    ctx: &mut MacroSourceCtx<'_, '_>,
    state: &mut TsxMacroState,
) {
    // Determine model name
    let model_name = if let Some(ns) = name_span {
        let name_text = &ctx.content_str[ns.start as usize..ns.end as usize];
        name_text.trim_matches('\'').trim_matches('"').to_string()
    } else {
        "modelValue".to_string()
    };

    let prepend = format!("{}_", model_name);
    let type_name_str = format!("{}{}defineModel_Type", PREFIX, prepend);
    let auto_var_name = format!("{}models_{}", PREFIX, model_name);

    let has_type_params = type_params.is_some();

    let stmt_start = declarator
        .map(|d| ctx.content_start + d.statement_span.start)
        .unwrap_or(ctx.content_start + call_span.start);

    // Emit type alias for type params
    if let Some(tp) = type_params {
        if ctx.is_jsx {
            // JSX mode: remove the generic type brackets entirely (JS has no generics)
            ctx.out.overwrite(tp.lt_span.start, tp.gt_span.end, "");
        } else {
            let type_text = &ctx.source[tp.type_span.start as usize..tp.type_span.end as usize];
            let needs_prettify = !is_simple_type_reference(type_text);

            let prefix = if needs_prettify {
                format!(";type {}={}Prettify<", type_name_str, PREFIX)
            } else {
                format!(";type {}=", type_name_str)
            };
            let suffix = if needs_prettify { ">;" } else { ";" };

            // Move original type content to stmt_start, wrapped with type declaration.
            // This preserves fine-grained sourcemap for hover on individual properties.
            ctx.out.move_wrapped(
                tp.type_span.start,
                tp.type_span.end,
                stmt_start,
                &prefix,
                suffix,
            );

            // Fill the gap left by the move with the type alias name
            ctx.out.prepend_alloc(tp.type_span.start, &type_name_str);
        }
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
        is_type: has_type_params,
    });
}

/// Process withDefaults(defineProps<T>(), { defaults }).
fn process_with_defaults(
    call_span: crate::common::Span,
    declarator: Option<&MacroDeclarator<'_>>,
    define_props_type_params: Option<&MacroTypeParams>,
    ctx: &mut MacroSourceCtx<'_, '_>,
    state: &mut TsxMacroState,
) {
    let type_name_str = format!("{}defineProps_Type", PREFIX);
    let auto_var_name = format!("{}props", PREFIX);

    let has_type_params = define_props_type_params.is_some();

    let stmt_start = declarator
        .map(|d| ctx.content_start + d.statement_span.start)
        .unwrap_or(ctx.content_start + call_span.start);

    // Emit type alias for inner defineProps type params
    if let Some(tp) = define_props_type_params {
        if ctx.is_jsx {
            // JSX mode: remove the generic type brackets entirely (JS has no generics)
            ctx.out.overwrite(tp.lt_span.start, tp.gt_span.end, "");
        } else {
            let type_text = &ctx.source[tp.type_span.start as usize..tp.type_span.end as usize];
            let needs_prettify = !is_simple_type_reference(type_text);

            let prefix = if needs_prettify {
                format!(";type {}={}Prettify<", type_name_str, PREFIX)
            } else {
                format!(";type {}=", type_name_str)
            };
            let suffix = if needs_prettify { ">;" } else { ";" };

            // Move original type content to stmt_start, wrapped with type declaration.
            // This preserves fine-grained sourcemap for hover on individual properties.
            ctx.out.move_wrapped(
                tp.type_span.start,
                tp.type_span.end,
                stmt_start,
                &prefix,
                suffix,
            );

            // Fill the gap left by the move with the type alias name
            ctx.out.prepend_alloc(tp.type_span.start, &type_name_str);
        }
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

    // Register defineProps binding (withDefaults wraps it)
    state.macro_bindings.push(MacroBindingEntry {
        macro_name: "defineProps".to_string(),
        var_name: Some(effective_var_name),
        type_name: if has_type_params {
            Some(type_name_str)
        } else {
            None
        },
        is_type: has_type_params,
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

/// Detect `useAttrs<T>()` calls in the script setup body and return the type parameter text.
///
/// Used as a fallback for `attrs_type` when no `attrs` attribute is present on the script tag.
/// Priority: `attrs` attribute > `useAttrs<T>()` > `{}`.
fn detect_use_attrs_type_arg<'a>(body: &[Statement<'a>], source: &'a str) -> Option<String> {
    for stmt in body {
        let call = match stmt {
            Statement::VariableDeclaration(var_decl) => var_decl
                .declarations
                .iter()
                .find_map(|d| d.init.as_ref())
                .and_then(|e| {
                    if let Expression::CallExpression(c) = e {
                        Some(c.as_ref())
                    } else {
                        None
                    }
                }),
            Statement::ExpressionStatement(expr_stmt) => {
                if let Expression::CallExpression(c) = &expr_stmt.expression {
                    Some(c.as_ref())
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(call) = call {
            if let Some(name) = callee_identifier_name(&call.callee) {
                if name == "useAttrs" {
                    if let Some(tp) = &call.type_arguments {
                        if let Some(param) = tp.params.first() {
                            let span: oxc_span::Span = param.span();
                            let text = &source[span.start as usize..span.end as usize];
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                return Some(trimmed.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Detect if script setup body contains a `getCurrentInstance()` call.
fn detect_get_current_instance(body: &[Statement<'_>]) -> bool {
    for stmt in body {
        if detect_gci_in_stmt(stmt) {
            return true;
        }
    }
    false
}

fn detect_gci_in_stmt(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::VariableDeclaration(var_decl) => {
            for decl in &var_decl.declarations {
                if let Some(init) = &decl.init {
                    if detect_gci_in_expr(init) {
                        return true;
                    }
                }
            }
        }
        Statement::ExpressionStatement(expr_stmt) => {
            if detect_gci_in_expr(&expr_stmt.expression) {
                return true;
            }
        }
        Statement::ReturnStatement(ret) => {
            if let Some(arg) = &ret.argument {
                if detect_gci_in_expr(arg) {
                    return true;
                }
            }
        }
        Statement::BlockStatement(block) => {
            for s in &block.body {
                if detect_gci_in_stmt(s) {
                    return true;
                }
            }
        }
        Statement::IfStatement(if_stmt) => {
            if detect_gci_in_expr(&if_stmt.test) || detect_gci_in_stmt(&if_stmt.consequent) {
                return true;
            }
            if let Some(alt) = &if_stmt.alternate {
                if detect_gci_in_stmt(alt) {
                    return true;
                }
            }
        }
        _ => {}
    }
    false
}

fn detect_gci_in_expr(expr: &Expression<'_>) -> bool {
    match expr {
        Expression::CallExpression(call) => {
            if let Some(name) = callee_identifier_name(&call.callee) {
                if name == "getCurrentInstance" {
                    return true;
                }
            }
            if detect_gci_in_expr(&call.callee) {
                return true;
            }
            for arg in &call.arguments {
                if let Some(e) = arg.as_expression() {
                    if detect_gci_in_expr(e) {
                        return true;
                    }
                }
            }
        }
        Expression::AssignmentExpression(ae) => {
            if detect_gci_in_expr(&ae.right) {
                return true;
            }
        }
        Expression::ParenthesizedExpression(p) => {
            if detect_gci_in_expr(&p.expression) {
                return true;
            }
        }
        Expression::ConditionalExpression(c) => {
            if detect_gci_in_expr(&c.test)
                || detect_gci_in_expr(&c.consequent)
                || detect_gci_in_expr(&c.alternate)
            {
                return true;
            }
        }
        Expression::LogicalExpression(l) => {
            if detect_gci_in_expr(&l.left) || detect_gci_in_expr(&l.right) {
                return true;
            }
        }
        _ => {}
    }
    false
}

/// Emit global component fallback consts for unresolved components inside templateBindingFN.
fn emit_global_component_fallbacks(
    buf: &mut String,
    template_ast: Option<&TemplateAst>,
    source: &str,
    bindings: &FxHashMap<&str, BindingType>,
    is_jsx: bool,
) {
    let ast = match template_ast {
        Some(a) => a,
        None => return,
    };

    let binding_names: FxHashSet<&str> = bindings.keys().copied().collect();
    let mut seen = FxHashSet::default();

    for node in &ast.nodes {
        if let AstNodeKind::Element(ref el) = node.kind {
            if !el.tag_type.is_component() {
                continue;
            }
            let tag_name = &source[(el.tag_open.start + 1) as usize..el.tag_open.name_end as usize];

            // Skip builtins
            if crate::template::code_gen::shared::helpers::is_builtin_component(tag_name).is_some()
            {
                continue;
            }

            // Convert to PascalCase for binding lookup
            let pascal = to_pascal_case(tag_name);
            if binding_names.contains(pascal.as_str()) || binding_names.contains(tag_name) {
                continue;
            }

            // Skip member expressions like Foo.Bar
            if tag_name.contains('.') {
                continue;
            }

            if seen.insert(pascal.clone()) {
                use std::fmt::Write;
                if is_jsx {
                    write!(
                        buf,
                        "\nconst {pascal} = /** @type {{unknown}} */ ({{}});",
                        pascal = pascal,
                    )
                    .expect("write to String is infallible");
                } else {
                    write!(
                        buf,
                        "\nconst {pascal} = {{}} as import('vue').GlobalComponents extends {{ {pascal}: infer C }} ? C : unknown;",
                        pascal = pascal,
                    )
                    .expect("write to String is infallible");
                }
            }
        }
    }
}

/// Convert a kebab-case or camelCase tag name to PascalCase.
fn to_pascal_case(tag: &str) -> String {
    if tag.contains('-') {
        tag.split('-')
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(c) => {
                        let upper: String = c.to_uppercase().collect();
                        format!("{}{}", upper, chars.as_str())
                    }
                    None => String::new(),
                }
            })
            .collect()
    } else {
        // Already PascalCase or camelCase — capitalize first letter
        let mut chars = tag.chars();
        match chars.next() {
            Some(c) => {
                let upper: String = c.to_uppercase().collect();
                format!("{}{}", upper, chars.as_str())
            }
            None => String::new(),
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────

fn emit_minimal_wrapper(
    out: &mut CodeGenOutput<'_>,
    options: &IdeScriptOptions<'_>,
    pos: u32,
    template_end: Option<u32>,
) -> Option<String> {
    if template_end.is_some() {
        // Unified CT: function start at pos, close deferred
        let mut start = format!("export function {}TemplateBindingFN() {{\n", PREFIX);
        // Declare instance for instance property access in template.
        // Minimal wrapper: no Comp functions, so no $attrs override
        start.push_str(&instance_declaration(
            options.filename,
            options.is_jsx,
            false,
        ));
        out.prepend_alloc(pos, &start);
        let mut close = String::from("\n");
        close.push_str(&instance_probe_line());
        close.push_str("}\n");
        Some(close)
    } else {
        // No template: emit everything at pos
        let wrapper = format!("export function {}TemplateBindingFN() {{\n}}\n", PREFIX,);
        out.prepend_alloc(pos, &wrapper);
        None
    }
}

/// Prefix for all emitted ___VERTER___ types/functions.
const PREFIX: &str = "___VERTER___";

/// Emit the `___VERTER___instance` declaration and void suppression.
///
/// Uses `import()` type expression to get the full component instance type from the
/// `.vue.d.ts` default export, providing type checking for `$slots`, `$emit`, `$props`,
/// `$attrs`, and any custom global properties (e.g., `$t`, `$router`).
fn instance_declaration(filename: &str, is_jsx: bool, override_attrs: bool) -> String {
    if is_jsx {
        format!(
            "\n/** @type {{any}} */\nvar {P}instance = /** @type {{any}} */ (null);\nvoid {P}instance;\n",
            P = PREFIX,
        )
    } else if override_attrs {
        // With Comp functions + attrs type aliases: override $attrs with composed type
        format!(
            "\n// @ts-ignore\nlet {P}instance!: Omit<InstanceType<import('./{filename}')['default']>, '$attrs'> & {{ $attrs: {P}Attrs }};\nvoid {P}instance;\n",
            P = PREFIX,
            filename = filename,
        )
    } else {
        format!(
            "\n// @ts-ignore\nlet {P}instance!: InstanceType<import('./{filename}')['default']>;\nvoid {P}instance;\n",
            P = PREFIX,
            filename = filename,
        )
    }
}

/// Ambient variant for Options API (file scope, no TDZ issues).
///
/// Uses `declare let` so the declaration is available regardless of position in file.
/// Needed because template JSX may appear before the script block.
fn instance_declaration_ambient(filename: &str, is_jsx: bool) -> String {
    if is_jsx {
        format!(
            "\n/** @type {{any}} */\nvar {P}instance = /** @type {{any}} */ (null);\n",
            P = PREFIX,
        )
    } else {
        format!(
            "\n// @ts-ignore\ndeclare let {P}instance: InstanceType<import('./{filename}')['default']>;\n",
            P = PREFIX,
            filename = filename,
        )
    }
}

/// Emit the instance completion probe line.
///
/// Creates a member-access expression at a known position that the LSP can use
/// to request TSGO completions for all instance members.
fn instance_probe_line() -> String {
    format!("\nvoid ({P}instance).valueOf;\n", P = PREFIX)
}

/// Info about a binding's position and leading JSDoc.
struct BindingSourceInfo {
    /// Leading JSDoc comment text (e.g. `/** My counter */`), if any.
    jsdoc: Option<String>,
    /// SFC-absolute byte offset of identifier start.
    sfc_start: u32,
    /// SFC-absolute byte offset of identifier end.
    sfc_end: u32,
}

/// Find a leading JSDoc comment for a declaration at the given position.
///
/// OXC's `Comment.attached_to` is the byte offset of the token the comment precedes.
/// We match comments where `attached_to == target_start` and the comment is a JSDoc
/// block comment (starts with `/**`).
fn find_leading_jsdoc(
    comments: &[Comment],
    target_start: u32,
    content_str: &str,
) -> Option<String> {
    for comment in comments {
        if comment.attached_to == target_start
            && comment.is_block()
            && matches!(
                comment.content,
                CommentContent::Jsdoc | CommentContent::JsdocLegal
            )
        {
            let text = &content_str[comment.span.start as usize..comment.span.end as usize];
            return Some(text.to_string());
        }
    }
    None
}

/// Build a map of binding name → source info (JSDoc + SFC-absolute identifier span).
///
/// Walks OXC's parsed program body to find variable declarations and function declarations,
/// extracting identifier spans and any leading JSDoc comments.
fn build_binding_source_info<'a>(
    body: &'a [Statement<'a>],
    comments: &[Comment],
    content_str: &str,
    content_start: u32,
) -> FxHashMap<&'a str, BindingSourceInfo> {
    let mut info: FxHashMap<&'a str, BindingSourceInfo> = FxHashMap::default();

    for stmt in body {
        match stmt {
            Statement::VariableDeclaration(decl) => {
                let decl_start = decl.span.start;
                for declarator in &decl.declarations {
                    if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                        let jsdoc = find_leading_jsdoc(comments, decl_start, content_str);
                        info.insert(
                            id.name.as_str(),
                            BindingSourceInfo {
                                jsdoc,
                                sfc_start: content_start + id.span.start,
                                sfc_end: content_start + id.span.end,
                            },
                        );
                    }
                }
            }
            Statement::FunctionDeclaration(func) => {
                if let Some(id) = &func.id {
                    let jsdoc = find_leading_jsdoc(comments, func.span.start, content_str);
                    info.insert(
                        id.name.as_str(),
                        BindingSourceInfo {
                            jsdoc,
                            sfc_start: content_start + id.span.start,
                            sfc_end: content_start + id.span.end,
                        },
                    );
                }
            }
            _ => {}
        }
    }

    info
}

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
  export declare function enhanceElementWithProps<T, P>(el: T, props: P): T & P;
  export declare function shallowUnwrapRef<T>(obj: T): import("vue").ShallowUnwrapRef<T>;
  export type ExtractRenderComponent<T> = T extends { new (): infer I; } ? I extends { $props: any } ? T : I extends HTMLElement ? (props: {}) => I : I : T extends (...args: any) => infer R ? void extends R ? typeof import("vue").Comment : R extends Array<any> ? typeof import("vue").Fragment : HTMLElement : T extends HTMLElement ? (props: {}) => T : T extends keyof import("vue").NativeElements ? (props: import("vue").NativeElements[T]) => JSX.Element : (props: {}) => JSX.Element;
  export declare function extractRenderComponent<T extends string>(t: T): ExtractRenderComponent<T>;
  export declare function extractRenderComponent<T>(t: T): ExtractRenderComponent<T>;
  export type ExtractComponentProps<T> = T extends { new (): infer I } ? ExtractComponentProps<I> : T extends { $props: infer P } ? P : T extends HTMLElement ? import("vue").HTMLAttributes : T extends (p: infer P) => any ? P : {};
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
export declare function enhanceElementWithProps<T, P>(el: T, props: P): T & P;
export declare function shallowUnwrapRef<T>(obj: T): import("vue").ShallowUnwrapRef<T>;
export type ExtractRenderComponent<T> = T extends { new (): infer I; } ? I extends { $props: any } ? T : I extends HTMLElement ? (props: {}) => I : I : T extends (...args: any) => infer R ? void extends R ? typeof import("vue").Comment : R extends Array<any> ? typeof import("vue").Fragment : HTMLElement : T extends HTMLElement ? (props: {}) => T : T extends keyof import("vue").NativeElements ? (props: import("vue").NativeElements[T]) => JSX.Element : (props: {}) => JSX.Element;
export declare function extractRenderComponent<T extends string>(t: T): ExtractRenderComponent<T>;
export declare function extractRenderComponent<T>(t: T): ExtractRenderComponent<T>;
export type ExtractComponentProps<T> = T extends { new (): infer I } ? ExtractComponentProps<I> : T extends { $props: infer P } ? P : T extends HTMLElement ? import("vue").HTMLAttributes : T extends (p: infer P) => any ? P : {};
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
fn emit_helper_imports(
    out: &mut CodeGenOutput<'_>,
    pos: u32,
    options: &IdeScriptOptions<'_>,
    builtin_components: &[&str],
    template_ast: Option<&crate::ast::types::TemplateAst>,
) {
    use std::fmt::Write;

    let mut imports = String::with_capacity(512);

    // Type imports from @verter/types (TS mode only — JSDoc mode doesn't need import type)
    if !options.is_jsx {
        if template_ast.is_some() {
            writeln!(
                imports,
                "import type {{ Prettify as {P}Prettify, ExtractComponentProps as {P}ExtractComponentProps }} from \"{}\";",
                options.types_module_name,
                P = PREFIX,
            )
            .expect("write to String is infallible");
        } else {
            writeln!(
                imports,
                "import type {{ Prettify as {P}Prettify }} from \"{}\";",
                options.types_module_name,
                P = PREFIX,
            )
            .expect("write to String is infallible");
        }
    }

    // Runtime imports from @verter/types
    writeln!(
        imports,
        "import {{ shallowUnwrapRef as {P}shallowUnwrapRef, enhanceElementWithProps as {P}enhanceElementWithProps, extractRenderComponent as {P}extractRenderComponent }} from \"{}\";",
        options.types_module_name,
        P = PREFIX,
    )
    .expect("write to String is infallible");

    // Collect vue imports: built-in components + template helpers (normalizeClass, normalizeStyle)
    let mut vue_imports: Vec<&str> = Vec::new();
    for &name in builtin_components {
        vue_imports.push(name);
    }

    // Check if template needs normalizeClass/normalizeStyle for class/style merging
    if let Some(ast) = template_ast {
        let mut need_class = false;
        let mut need_style = false;
        for node in &ast.nodes {
            if let crate::ast::types::AstNodeKind::Element(ref el) = node.kind {
                if !need_class && el.needs_class_merge() {
                    need_class = true;
                }
                if !need_style && el.needs_style_merge() {
                    need_style = true;
                }
                if need_class && need_style {
                    break;
                }
            }
        }
        if need_class {
            vue_imports.push("normalizeClass as ___VERTER___normalizeClass");
        }
        if need_style {
            vue_imports.push("normalizeStyle as ___VERTER___normalizeStyle");
        }
    }

    if !vue_imports.is_empty() {
        let imports_str = vue_imports.join(", ");
        writeln!(imports, "import {{ {} }} from \"vue\";", imports_str)
            .expect("write to String is infallible");
    }

    out.prepend_alloc(pos, &imports);
}

/// Emit all type constructs to the `buf` string (no sourcemap).
fn emit_type_constructs(
    buf: &mut String,
    generic_info: &Option<IdeGenericInfo>,
    attrs_type: &Option<String>,
    _source: &str,
    options: &IdeScriptOptions<'_>,
    _has_get_current_instance: bool,
) {
    // Emit ___VERTER___attributes type alias
    if options.is_jsx {
        // JS mode: JSDoc @typedef
        if let Some(ref attrs) = attrs_type {
            buf.push_str(&format!(
                "\n/** @typedef {{{}}} {P}attributes */\n",
                attrs,
                P = PREFIX,
            ));
        } else {
            buf.push_str(&format!(
                "\n/** @typedef {{{{}}}} {P}attributes */\n",
                P = PREFIX,
            ));
        }
    } else {
        // TS mode: type alias
        let generic_suffix = generic_info
            .as_ref()
            .map(|g| g.source_bracket())
            .unwrap_or_default();
        if let Some(ref attrs) = attrs_type {
            buf.push_str(&format!(
                "\ntype {P}attributes{gs} = {attrs};\n",
                P = PREFIX,
                gs = generic_suffix,
                attrs = attrs,
            ));
        } else {
            buf.push_str(&format!("\ntype {P}attributes = {{}};\n", P = PREFIX,));
        }
    }

    // Append ambient module declaration (TS mode only — declare module is TS syntax)
    if options.embed_ambient_types && !options.is_jsx {
        buf.push_str(VERTER_TYPES_AMBIENT_MODULE);
    }
}

/// Emit RootElement, RootElementProps, and Attrs type aliases inside the function scope.
///
/// These must be inside `templateBindingFN` because they reference `getRootComponent`
/// and `getRootComponentPassedProps` which are function-local.
fn emit_attrs_type_aliases(
    buf: &mut String,
    generic_info: &Option<IdeGenericInfo>,
    inherit_attrs: bool,
) {
    let gs = generic_info
        .as_ref()
        .map(|g| g.source_bracket())
        .unwrap_or_default();
    let gn = generic_info
        .as_ref()
        .map(|g| g.names_bracket())
        .unwrap_or_default();

    buf.push_str(&format!(
        "\ntype {P}RootElement{gs} = ReturnType<typeof {P}getRootComponent{gn}>;\
         \ntype {P}RootElementProps{gs} = {P}Prettify<Omit<\
         \n  {P}ExtractComponentProps<{P}RootElement{gn}>,\
         \n  keyof ReturnType<typeof {P}getRootComponentPassedProps{gn}>\
         \n>>;\n",
        P = PREFIX,
        gs = gs,
        gn = gn,
    ));

    if inherit_attrs {
        buf.push_str(&format!(
            "\ntype {P}Attrs{gs} = {P}attributes{gn} & {P}RootElementProps{gn};\n",
            P = PREFIX,
            gs = gs,
            gn = gn,
        ));
    } else {
        buf.push_str(&format!(
            "\ntype {P}Attrs{gs} = {P}attributes{gn};\n",
            P = PREFIX,
            gs = gs,
            gn = gn,
        ));
    }
}

/// Emit Comp{offset} functions to a string buffer (inside templateBindingFN).
/// Returns (root_comp_offset, all_emitted_comp_offsets).
/// Returns (root_comp_offset, root_props_literal, all_comp_offsets).
fn emit_comp_functions_to_string(
    buf: &mut String,
    gs: &str,
    gn: &str,
    template_ast: Option<&TemplateAst>,
    source: &str,
    is_jsx: bool,
) -> (Option<u32>, Option<String>, Vec<u32>) {
    let ast = match template_ast {
        Some(a) => a,
        None => return (None, None, vec![]),
    };

    let root_children = ast
        .root
        .content
        .as_ref()
        .map(|c| c.children.as_slice())
        .unwrap_or(&[]);

    let mut root_comp_offset: Option<u32> = None;
    let mut root_props_literal: Option<String> = None;
    let mut all_comp_offsets: Vec<u32> = Vec::new();

    walk_children_for_comp(
        buf,
        gs,
        gn,
        ast,
        source,
        root_children,
        &[],
        &mut root_comp_offset,
        &mut root_props_literal,
        &mut all_comp_offsets,
        true,
        is_jsx,
    );

    (root_comp_offset, root_props_literal, all_comp_offsets)
}

/// Emit getRootComponent + getRootComponentPassedProps to a string buffer.
fn emit_get_root_component_to_string(
    buf: &mut String,
    gs: &str,
    gn: &str,
    root_comp_offset: Option<u32>,
    root_props_literal: Option<&str>,
) {
    use std::fmt::Write;

    let props = root_props_literal.unwrap_or("{}");

    if let Some(offset) = root_comp_offset {
        write!(
            buf,
            "\nfunction {P}getRootComponent{gs}() {{ return {P}Comp{offset}{gn}(); }}\
             \nfunction {P}getRootComponentPassedProps{gs}() {{ return {props}; }}",
            P = PREFIX,
            gs = gs,
            gn = gn,
            offset = offset,
            props = props,
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

/// Emit Comp{offset} functions for template elements.
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
    root_props_literal: &mut Option<String>,
    all_comp_offsets: &mut Vec<u32>,
    is_root: bool,
    is_jsx: bool,
) {
    for &child_id in children {
        let node = &ast.nodes[child_id.0];
        if let AstNodeKind::Element(el) = &node.kind {
            // Build condition scope using raw expressions (no binding prefixes)
            // because Comp functions receive variables from the enclosing scope
            let mut scopes = parent_scopes.to_vec();
            if let Some(scope) = build_condition_scope_raw(el, ast, child_id, source) {
                scopes.push(scope);
            }

            // Emit Comp function for:
            // - elements with ref (always, for template ref typing)
            // - the first root element (for implicit attrs / getRootComponent)
            let is_eligible = !matches!(el.tag_type, TagType::SlotOutlet | TagType::Template);
            let is_first_root = is_root && root_comp_offset.is_none();
            let needs_comp = is_eligible && (el.v_ref.is_some() || is_first_root);
            if needs_comp {
                let offset = el.tag_open.start;
                let props_lit = serialize_element_props(el, source);
                emit_comp_function_for_element(
                    buf, gs, gn, el, source, offset, &scopes, is_jsx, &props_lit,
                );
                all_comp_offsets.push(offset);
                if is_first_root {
                    *root_comp_offset = Some(offset);
                    *root_props_literal = Some(props_lit);
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
                    root_props_literal,
                    all_comp_offsets,
                    false,
                    is_jsx,
                );
            }
        }
    }
}

/// Build a condition scope using raw source expressions (no binding prefixes).
/// For use in Comp functions where the enclosing scope provides variables directly.
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

/// Serialize an element's template props as a TS object literal string.
///
/// Iterates `el.props` to produce `{"id": "app", "onClick": handler}`.
/// Skips `class` and `style` (Vue handles these specially).
/// Structural directives (v-if, v-for, etc.) are already taken out of `el.props`.
fn serialize_element_props(el: &ElementNode, source: &str) -> String {
    let mut entries: Vec<String> = Vec::new();

    for prop in &el.props {
        let name = &source[prop.start as usize..prop.name_end as usize];

        if !prop.is_directive {
            // Static attribute: name="value" or boolean attribute
            // Skip class and style (Vue handles these specially)
            if name.eq_ignore_ascii_case("class") || name.eq_ignore_ascii_case("style") {
                continue;
            }
            if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                let value = &source[vs as usize..ve as usize];
                // JSON-stringify the value
                let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
                entries.push(format!("\"{}\": \"{}\"", name, escaped));
            } else {
                // Boolean attribute (no value)
                entries.push(format!("\"{}\": true", name));
            }
        } else {
            // Directive
            let (arg_start, arg_end) = match (prop.arg_start, prop.arg_end) {
                (Some(s), Some(e)) => (s, e),
                _ => continue, // no argument (e.g., v-show with no arg → skip for props)
            };

            if name == ":" || name == "v-bind" {
                // Dynamic bind: :name="expr" → "name": expr
                let arg_name = &source[arg_start as usize..arg_end as usize];
                // Skip class and style
                if arg_name.eq_ignore_ascii_case("class") || arg_name.eq_ignore_ascii_case("style")
                {
                    continue;
                }
                if let (Some(vs), Some(ve)) = (prop.value_start, prop.value_end) {
                    let expr = &source[vs as usize..ve as usize];
                    entries.push(format!("\"{}\": {}", arg_name, expr));
                }
            } else if name == "@" || name == "v-on" {
                // Event handler: @name="expr" → "onName": () => {}
                // We use a placeholder function since only the key matters for
                // getRootComponentPassedProps (used in Omit for $attrs typing).
                let arg_name = &source[arg_start as usize..arg_end as usize];
                let on_name = camelize_event(arg_name);
                entries.push(format!("\"{}\": () => {{}}", on_name));
            }
            // Other directives (v-show, v-model, etc.) are not included in props
        }
    }

    if entries.is_empty() {
        "{}".to_string()
    } else {
        format!("{{{}}}", entries.join(", "))
    }
}

/// Convert an event name to `onXxx` format.
/// e.g., "click" → "onClick", "my-event" → "onMyEvent"
fn camelize_event(name: &str) -> String {
    let mut result = String::with_capacity(name.len() + 2);
    result.push_str("on");
    let mut capitalize_next = true;
    for ch in name.chars() {
        if ch == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Emit a single Comp{offset} function for an element, with optional condition guards.
#[allow(clippy::too_many_arguments)]
fn emit_comp_function_for_element(
    buf: &mut String,
    gs: &str,
    _gn: &str,
    el: &ElementNode,
    source: &str,
    offset: u32,
    condition_scopes: &[super::condition::ConditionScope],
    is_jsx: bool,
    props_literal: &str,
) {
    use std::fmt::Write;

    let tag_name = &source[el.tag_open.start as usize + 1..el.tag_open.name_end as usize];

    // Generate narrowing guard from condition scopes
    let guard = super::condition::generate_condition_text(condition_scopes)
        .map(|text| format!("\n  if(!({})) return null;", text))
        .unwrap_or_default();

    match el.tag_type {
        TagType::Element => {
            if is_jsx {
                write!(
                    buf,
                    "\nfunction {P}Comp{offset}{gs}() {{{guard}\
                     \n  return {P}enhanceElementWithProps(/** @type {{HTMLElementTagNameMap[\"{tag}\"]}} */ ({{}}), {props});\
                     \n}}",
                    P = PREFIX,
                    offset = offset,
                    gs = gs,
                    guard = guard,
                    tag = tag_name,
                    props = props_literal,
                )
                .expect("write to String is infallible");
            } else {
                write!(
                    buf,
                    "\nfunction {P}Comp{offset}{gs}() {{{guard}\
                     \n  return {P}enhanceElementWithProps({{}} as HTMLElementTagNameMap[\"{tag}\"], {props});\
                     \n}}",
                    P = PREFIX,
                    offset = offset,
                    gs = gs,
                    guard = guard,
                    tag = tag_name,
                    props = props_literal,
                )
                .expect("write to String is infallible");
            }
        }
        TagType::Component => {
            write!(
                buf,
                "\nfunction {P}Comp{offset}{gs}() {{{guard}\
                 \n  return new {tag}({props});\
                 \n}}",
                P = PREFIX,
                offset = offset,
                gs = gs,
                guard = guard,
                tag = tag_name,
                props = props_literal,
            )
            .expect("write to String is infallible");
        }
        TagType::SlotOutlet | TagType::Template => {
            // Skip <slot> and <template> wrappers
        }
    }
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
        if let (Some(return_close), Some(tpl_end)) = (&result.return_close, template_end) {
            ct.prepend_left(tpl_end, return_close);
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
            code.contains("import('./App.vue')"),
            "Should reference the component's own .vue file. Got: {}",
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
            !code.contains("import type { default as ___VERTER___Self }"),
            "self-import should no longer be emitted: {}",
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

    // ── Comp function tests ──────────────────────────────────────

    #[test]
    fn comp_function_html_element() {
        let (code, _, _tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
import { ref } from 'vue'
const el = ref<HTMLDivElement>()
</script>
<template><div ref="el">hello</div></template>"#,
        );
        assert!(
            code.contains(
                "___VERTER___enhanceElementWithProps({} as HTMLElementTagNameMap[\"div\"]"
            ),
            "should emit Comp for div with ref in code: {}",
            code
        );
    }

    #[test]
    fn comp_function_component() {
        let (code, _, _tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
import { ref } from 'vue'
import MyComp from './MyComp.vue'
const el = ref()
</script>
<template><MyComp ref="el" /></template>"#,
        );
        assert!(
            code.contains("return new MyComp({})"),
            "should emit new MyComp for component with ref in code: {}",
            code
        );
    }

    #[test]
    fn comp_function_generic() {
        let (code, _, _tc) = gen_tsx_script_full(
            r#"<script setup lang="ts" generic="T">
import { ref } from 'vue'
const el = ref<HTMLDivElement>()
const msg = {} as T
</script>
<template><div ref="el">{{ msg }}</div></template>"#,
        );
        assert!(
            code.contains("function ___VERTER___Comp") && code.contains("<T>()"),
            "Comp function should have generics in code: {}",
            code
        );
    }

    // ── getRootComponent tests ───────────────────────────────────

    #[test]
    fn get_root_component_with_template() {
        let (code, _, _tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
import { ref } from 'vue'
const el = ref<HTMLDivElement>()
</script>
<template><div ref="el">hello</div></template>"#,
        );
        assert!(
            code.contains("function ___VERTER___getRootComponent()")
                && code.contains("return ___VERTER___Comp"),
            "getRootComponent should delegate to Comp in code: {}",
            code
        );
    }

    #[test]
    fn get_root_component_generic() {
        let (code, _, _tc) = gen_tsx_script_full(
            r#"<script setup lang="ts" generic="T extends string">
import { ref } from 'vue'
const el = ref<HTMLDivElement>()
const msg = {} as T
</script>
<template><div ref="el">{{ msg }}</div></template>"#,
        );
        assert!(
            code.contains("function ___VERTER___getRootComponent<T extends string>()"),
            "getRootComponent should have generics in code: {}",
            code
        );
    }

    #[test]
    fn get_root_component_no_template() {
        let (code, _, _tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
const msg = 'hello'
</script>"#,
        );
        // No template: getRootComponent is not emitted (nothing to wrap)
        assert!(
            !code.contains("___VERTER___getRootComponent"),
            "getRootComponent should NOT be emitted when no template: {}",
            code
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
            !tc.contains("___VERTER___Component"),
            "Component export should not be emitted"
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
            !tc.contains("type ___VERTER___Instance"),
            "Instance type should no longer be emitted: {}",
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
            !tc.contains("type ___VERTER___Instance"),
            "Instance type should no longer be emitted: {}",
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
            !tc.contains("type ___VERTER___Instance"),
            "Instance type should no longer be emitted: {}",
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
            !tc.contains("type ___VERTER___Instance"),
            "Instance type should no longer be emitted: {}",
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
        // Runtime args stay as-is, variable assignment prepended, no boxing
        assert!(
            code.contains("const ___VERTER___props=defineProps({ a: String })"),
            "should prepend variable assignment with args as-is: {}",
            code
        );
        // No boxing
        assert!(
            !code.contains("_Box"),
            "no boxing should be present: {}",
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
        // Runtime args stay as-is, no boxing
        assert!(
            code.contains("const props = defineProps({ a: String })"),
            "should keep user variable and args: {}",
            code
        );
        // No boxing
        assert!(
            !code.contains("_Box"),
            "no boxing should be present: {}",
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
        // defineEmits with runtime args: variable assignment prepended, no boxing
        assert!(
            code.contains("const ___VERTER___emits=defineEmits(['change', 'update'])"),
            "should prepend variable assignment: {}",
            code
        );
        // No boxing
        assert!(
            !code.contains("_Box"),
            "no boxing should be present: {}",
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
        // defineExpose stays as-is, no boxing
        assert!(
            code.contains("defineExpose({ foo: 'bar' })"),
            "defineExpose call should be preserved: {}",
            code
        );
        // No boxing
        assert!(
            !code.contains("_Box"),
            "no boxing should be present: {}",
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
        // defineOptions stays as-is, no boxing
        assert!(
            code.contains("defineOptions({ inheritAttrs: false })"),
            "defineOptions call should be preserved: {}",
            code
        );
        // No boxing
        assert!(
            !code.contains("_Box"),
            "no boxing should be present: {}",
            code
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
        // withDefaults call stays with type alias replacement, no boxing
        assert!(
            code.contains(
                "withDefaults(defineProps<___VERTER___defineProps_Type>(), { msg: 'hello' })"
            ),
            "withDefaults call should stay with type alias replacement: {}",
            code
        );
        // No boxing
        assert!(
            !code.contains("_Box"),
            "no boxing should be present: {}",
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
        let (code, _, _tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
import { ref } from 'vue'
const isTypeA = true
const el = ref<HTMLDivElement>()
</script>
<template><div v-if="isTypeA" ref="el">A</div></template>"#,
        );
        // Comp function should have condition guard
        assert!(
            code.contains("if(!((isTypeA))) return null;"),
            "Comp for v-if should have condition guard, got:\n{}",
            code
        );
    }

    #[test]
    fn comp_v_else_if_negates_prior_siblings() {
        let (code, _, _tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
import { ref } from 'vue'
const isTypeA = true
const isTypeB = true
const el = ref<HTMLDivElement>()
</script>
<template>
  <div v-if="isTypeA">A</div>
  <div v-else-if="isTypeB" ref="el">B</div>
</template>"#,
        );
        // v-else-if Comp should negate prior v-if and include own condition
        assert!(
            code.contains("!((isTypeA)) && (isTypeB)"),
            "Comp for v-else-if should negate prior v-if, got:\n{}",
            code
        );
    }

    #[test]
    fn comp_v_else_negates_all_prior() {
        let (code, _, _tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
import { ref } from 'vue'
const isTypeA = true
const el = ref<HTMLDivElement>()
</script>
<template>
  <div v-if="isTypeA">A</div>
  <div v-else ref="el">B</div>
</template>"#,
        );
        // v-else Comp should negate all prior conditions
        assert!(
            code.contains("if(!(!((isTypeA)))) return null;"),
            "Comp for v-else should negate prior v-if, got:\n{}",
            code
        );
    }

    #[test]
    fn comp_nested_v_if_combines_parent_and_own() {
        let (code, _, _tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
import { ref } from 'vue'
const parent = true
const child = true
const el = ref<HTMLSpanElement>()
</script>
<template><div v-if="parent"><span v-if="child" ref="el">nested</span></div></template>"#,
        );
        // Nested Comp should combine parent + own condition
        // The span's Comp should have: if(!((parent) && (child))) return null;
        assert!(
            code.contains("(parent) && (child)"),
            "nested Comp should combine parent + own condition, got:\n{}",
            code
        );
    }

    #[test]
    fn comp_all_elements_get_functions_not_just_root() {
        let (code, _, _tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
import { ref } from 'vue'
const el1 = ref<HTMLDivElement>()
const el2 = ref<HTMLSpanElement>()
</script>
<template><div ref="el1"><span ref="el2">inner</span></div></template>"#,
        );
        // Both div and span should get Comp functions (both have ref)
        let comp_count = code.matches("function ___VERTER___Comp").count();
        assert!(
            comp_count >= 2,
            "should emit Comp for all ref elements (div + span), found {} Comp functions, got:\n{}",
            comp_count,
            code
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
        // Negative: Instance type should no longer be emitted
        assert!(
            !type_constructs.contains("___VERTER___Instance"),
            "should not emit Instance"
        );
        assert!(
            !type_constructs.contains("InstanceType<typeof ___VERTER___Self>"),
            "should not emit InstanceType<typeof Self>"
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
            !type_constructs.contains("___VERTER___Instance"),
            "should not emit Instance"
        );
    }

    // ── types_module_name tests ─────────────────────────────────────

    /// Generate TSX script with custom options and return (code, bindings, type_constructs).
    fn gen_tsx_script_full_with_options(
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
        if let (Some(return_close), Some(tpl_end)) = (&result.return_close, template_end) {
            ct.prepend_left(tpl_end, return_close);
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
            IdeScriptOptions {
                component_name: "App",
                js_component_name: "App",
                filename: "App.vue",
                scope_id: "data-v-abc123",
                has_scoped_style: false,
                runtime_module_name: "vue",
                types_module_name: "@verter/types",
                is_vapor: false,
                embed_ambient_types: false,
                is_jsx: false,
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
        // Positive: Comp function references Foo via new
        assert!(
            code.contains("new Foo("),
            "Comp function should construct Foo: {}",
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

    /// @ai-generated — G27: getCurrentInstance no longer emits CurrentComponentInstance type
    #[test]
    fn get_current_instance_emits_type() {
        let (_code, _, tc) = gen_tsx_script_full(
            r#"<script setup lang="ts">
import { getCurrentInstance } from 'vue'
const instance = getCurrentInstance()
</script>"#,
        );
        // Negative: CurrentComponentInstance type should no longer be emitted
        assert!(
            !tc.contains("type ___VERTER___CurrentComponentInstance"),
            "CurrentComponentInstance type should no longer be emitted: {}",
            tc
        );
        // Negative: Instance type should no longer be emitted
        assert!(
            !tc.contains("___VERTER___Instance"),
            "Instance type should no longer be emitted: {}",
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
            code.contains(
                "function ___VERTER___TemplateBindingFN<T, U extends Record<string, T>>()"
            ),
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

    // ── Script Error Recovery Tests ─────────────────────────────────

    #[test]
    fn imports_hoisted_with_script_syntax_error() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
count.
</script>"#,
        );
        // In error mode, script body is at file scope (not wrapped in function).
        // The import stays in its original position, which is before the
        // TemplateBindingFN that only wraps the template.
        assert!(
            code.contains("import { ref } from 'vue'"),
            "import must be present:\n{}",
            code
        );
        // Positive: the broken `count.` line must still appear (for TS resolution)
        assert!(
            code.contains("count."),
            "broken expression must be preserved:\n{}",
            code
        );
        // Negative: script body should NOT be wrapped in TemplateBindingFN
        // (error mode keeps it at file scope)
        let fn_pos = code.find("function ___VERTER___TemplateBindingFN");
        if let Some(fn_pos) = fn_pos {
            let import_pos = code.find("import { ref } from 'vue'").unwrap();
            assert!(
                import_pos < fn_pos,
                "import must be at file scope (before TemplateBindingFN):\n{}",
                code
            );
            let count_dot_pos = code.find("count.\n").unwrap();
            assert!(
                count_dot_pos < fn_pos,
                "broken expression must be at file scope (before TemplateBindingFN):\n{}",
                code
            );
        }
    }

    #[test]
    fn imports_hoisted_with_broken_expression_in_function() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
function increment() {
  count.value++
  count.
}
</script>"#,
        );
        // In error mode, script body is at file scope
        assert!(
            code.contains("import { ref } from 'vue'"),
            "import must be present:\n{}",
            code
        );
        // Positive: the `count.value++` line must appear
        assert!(
            code.contains("count.value++"),
            "valid expression must be preserved:\n{}",
            code
        );
        // Positive: `function increment()` must appear (user function preserved)
        assert!(
            code.contains("function increment()"),
            "user function must be preserved:\n{}",
            code
        );
    }

    #[test]
    fn template_wrapper_with_script_error() {
        let (code, _, _) = gen_tsx_script_full(
            r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
count.
</script>
<template><div>{{ count }}</div></template>"#,
        );
        // Positive: TemplateBindingFN wrapper must exist (for template)
        assert!(
            code.contains("function ___VERTER___TemplateBindingFN()"),
            "TemplateBindingFN wrapper must exist:\n{}",
            code
        );
        // Positive: import must appear before TemplateBindingFN (file scope)
        let fn_pos = code.find("function ___VERTER___TemplateBindingFN").unwrap();
        let import_pos = code.find("import { ref } from 'vue'").unwrap();
        assert!(
            import_pos < fn_pos,
            "import must be at file scope:\n{}",
            code
        );
        // Positive: script body must appear before TemplateBindingFN
        let count_pos = code.find("const count = ref(0)").unwrap();
        assert!(
            count_pos < fn_pos,
            "script body must be at file scope:\n{}",
            code
        );
        // Positive: wrapper must close
        assert!(
            code.contains("close templateBindingFN"),
            "TemplateBindingFN must be closed:\n{}",
            code
        );
    }

    #[test]
    fn script_error_no_block_scope_wrapping() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
count.
</script>"#,
        );
        // Negative: error mode should NOT use block scope destructuring
        assert!(
            !code.contains("___VERTER___unwrapped"),
            "error mode should not use block scope destructuring:\n{}",
            code
        );
        // Negative: no block scope opening/closing
        assert!(
            !code.contains("close block scope"),
            "error mode should not have block scope:\n{}",
            code
        );
    }

    #[test]
    fn script_error_file_scope_declarations() {
        let (code, _) = gen_tsx_script(
            r#"<script setup lang="ts">
import { ref } from 'vue'
const count = ref(0)
count.
</script>"#,
        );
        // Positive: declaration should appear in output at file scope
        assert!(
            code.contains("const count = ref(0)"),
            "declaration should be preserved:\n{}",
            code
        );
        // Positive: import should appear in output
        assert!(
            code.contains("import { ref } from 'vue'"),
            "import should be preserved:\n{}",
            code
        );
        // Negative: script body should NOT be inside a function
        // Check that `const count` comes before any TemplateBindingFN
        if let Some(fn_pos) = code.find("function ___VERTER___TemplateBindingFN") {
            let count_pos = code.find("const count = ref(0)").unwrap();
            assert!(
                count_pos < fn_pos,
                "declarations should be at file scope (before TemplateBindingFN):\n{}",
                code
            );
        }
    }

    // =========================================================================
    // JSX mode tests — verify JS SFCs produce JavaScript + JSDoc, not TypeScript
    // =========================================================================

    /// Helper: generate IDE script output with `is_jsx: true`.
    fn gen_jsx_script(source: &str) -> (String, String) {
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

        if let (Some(return_close), Some(tpl_end)) = (&result.return_close, template_end) {
            ct.prepend_left(tpl_end, return_close);
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

    #[test]
    fn jsx_mode_no_prettify_import() {
        let (code, type_constructs) = gen_jsx_script(
            r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>"#,
        );
        // Positive: should still have the vue import
        assert!(
            code.contains("import { ref } from 'vue'"),
            "vue import should be preserved:\n{}",
            code
        );
        // Negative: no TS-only Prettify import
        assert!(
            !code.contains("Prettify"),
            "JSX mode must not import Prettify:\n{}",
            code
        );
        assert!(
            !code.contains("import type"),
            "JSX mode must not have import type:\n{}",
            code
        );
        // Negative: no ambient module in type_constructs
        assert!(
            !type_constructs.contains("declare module"),
            "JSX mode must not have declare module in type_constructs:\n{}",
            type_constructs
        );
    }

    #[test]
    fn jsx_mode_no_as_unknown_cast() {
        let (code, _) = gen_jsx_script(
            r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
        );
        // Negative: no `as unknown as typeof`
        assert!(
            !code.contains("as unknown as typeof"),
            "JSX mode must not have 'as unknown as typeof' cast:\n{}",
            code
        );
        // Negative: no `as unknown`
        assert!(
            !code.contains("as unknown"),
            "JSX mode must not have any 'as unknown' cast:\n{}",
            code
        );
    }

    #[test]
    fn jsx_mode_no_type_alias() {
        let (code, _) = gen_jsx_script(
            r#"<script setup>
const props = defineProps<{ msg: string }>()
</script>"#,
        );
        // Negative: no `;type` alias
        assert!(
            !code.contains(";type "),
            "JSX mode must not have type aliases:\n{}",
            code
        );
        // Negative: generic brackets should be removed from defineProps<...>
        assert!(
            !code.contains("defineProps<"),
            "JSX mode must remove generic brackets from defineProps:\n{}",
            code
        );
    }

    #[test]
    fn jsx_mode_no_generic_on_wrapper() {
        let (code, _) = gen_jsx_script(
            r#"<script setup>
const props = defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
        );
        // Negative: no generic on TemplateBindingFN
        assert!(
            !code.contains("TemplateBindingFN<"),
            "JSX mode must not have generics on TemplateBindingFN:\n{}",
            code
        );
        // Positive: should still have the function
        assert!(
            code.contains("TemplateBindingFN"),
            "should still have TemplateBindingFN:\n{}",
            code
        );
    }

    #[test]
    fn jsx_mode_no_ambient_module() {
        let (_, type_constructs) = gen_jsx_script(
            r#"<script setup>
const count = ref(0)
</script>"#,
        );
        // Negative: no ambient module declaration
        assert!(
            !type_constructs.contains("declare module"),
            "JSX mode must not have 'declare module' in type_constructs:\n{}",
            type_constructs
        );
        assert!(
            !type_constructs.contains("@verter/types"),
            "JSX mode must not reference @verter/types module:\n{}",
            type_constructs
        );
    }

    #[test]
    fn jsx_mode_instance_declaration_no_ts_syntax() {
        let (code, _) = gen_jsx_script(
            r#"<script setup>
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
        );
        // Negative: no TS instance declaration syntax
        assert!(
            !code.contains("!:"),
            "JSX mode must not have definite assignment assertion '!:':\n{}",
            code
        );
        assert!(
            !code.contains("InstanceType<"),
            "JSX mode must not have InstanceType<>:\n{}",
            code
        );
        assert!(
            !code.contains("declare let"),
            "JSX mode must not have 'declare let':\n{}",
            code
        );
        // Positive: should have JSDoc-style instance declaration
        assert!(
            code.contains("/** @type {any} */"),
            "JSX mode should use JSDoc @type for instance:\n{}",
            code
        );
    }

    #[test]
    fn jsx_mode_global_component_no_conditional_type() {
        let (code, _) = gen_jsx_script(
            r#"<script setup>
</script>
<template><RouterView /></template>"#,
        );
        // Negative: no TS conditional type for global components
        assert!(
            !code.contains("infer C"),
            "JSX mode must not have 'infer C' conditional type:\n{}",
            code
        );
        assert!(
            !code.contains("import('vue').GlobalComponents"),
            "JSX mode must not reference GlobalComponents type:\n{}",
            code
        );
        // Positive: should have JSDoc unknown assertion
        assert!(
            code.contains("/** @type {unknown} */"),
            "JSX mode should use /** @type {{unknown}} */ for global components:\n{}",
            code
        );
    }

    #[test]
    fn jsx_mode_comp_function_no_ts_assertion() {
        let (code, _) = gen_jsx_script(
            r#"<script setup>
</script>
<template><div>hello</div></template>"#,
        );
        // Negative: no TS `as` cast for native elements
        assert!(
            !code.contains("{} as HTMLElementTagNameMap"),
            "JSX mode must not have '{{}} as HTMLElementTagNameMap':\n{}",
            code
        );
        // Positive: should use JSDoc for element type
        if code.contains("HTMLElementTagNameMap") {
            assert!(
                code.contains("/** @type"),
                "JSX mode should use JSDoc @type for element types:\n{}",
                code
            );
        }
    }

    #[test]
    fn jsx_mode_no_angle_bracket_rewrite() {
        let (code, _) = gen_jsx_script(
            r#"<script setup>
const x = (foo as Bar)
</script>"#,
        );
        // For JSX mode, TS type assertions should not be rewritten
        // (they're already not valid JS, but we skip the rewrite)
        assert!(
            !code.contains("( as "),
            "JSX mode should not rewrite angle bracket casts:\n{}",
            code
        );
    }

    #[test]
    fn jsx_mode_with_defaults() {
        let (code, _) = gen_jsx_script(
            r#"<script setup>
const props = withDefaults(defineProps<{ msg?: string }>(), {
  msg: 'hello'
})
</script>"#,
        );
        // Negative: no type alias
        assert!(
            !code.contains(";type "),
            "JSX mode withDefaults must not have type aliases:\n{}",
            code
        );
        // Negative: no Prettify
        assert!(
            !code.contains("Prettify"),
            "JSX mode withDefaults must not have Prettify:\n{}",
            code
        );
        // Negative: no generic brackets on defineProps
        assert!(
            !code.contains("defineProps<"),
            "JSX mode must remove generic brackets from defineProps:\n{}",
            code
        );
    }

    #[test]
    fn jsx_mode_define_emits() {
        let (code, _) = gen_jsx_script(
            r#"<script setup>
const emit = defineEmits<{
  (e: 'update', value: string): void
}>()
</script>"#,
        );
        // Negative: no type alias
        assert!(
            !code.contains(";type "),
            "JSX mode defineEmits must not have type aliases:\n{}",
            code
        );
        // Negative: no Prettify
        assert!(
            !code.contains("Prettify"),
            "JSX mode defineEmits must not have Prettify:\n{}",
            code
        );
    }

    #[test]
    fn jsx_mode_define_model() {
        let (code, _) = gen_jsx_script(
            r#"<script setup>
const modelValue = defineModel<string>()
</script>"#,
        );
        // Negative: no type alias
        assert!(
            !code.contains(";type "),
            "JSX mode defineModel must not have type aliases:\n{}",
            code
        );
    }

    #[test]
    fn jsx_mode_options_api_no_declare_let() {
        let (code, _) = gen_jsx_script(
            r#"<script>
export default {
  data() { return { count: 0 } }
}
</script>
<template><div>{{ count }}</div></template>"#,
        );
        // Negative: no TS-only syntax
        assert!(
            !code.contains("declare let"),
            "JSX options API must not have 'declare let':\n{}",
            code
        );
        assert!(
            !code.contains("!:"),
            "JSX options API must not have definite assignment:\n{}",
            code
        );
    }

    #[test]
    fn tsx_mode_still_has_ts_constructs() {
        // Contrast test: TSX mode (is_jsx = false) should still have TS constructs
        let (code, _, type_constructs) = gen_tsx_script_full(
            r#"<script setup lang="ts">
const props = defineProps<{ msg: string }>()
</script>
<template><div>{{ msg }}</div></template>"#,
        );
        // Positive: TSX mode should have TS constructs
        assert!(
            code.contains("Prettify") || code.contains("import type"),
            "TSX mode should have Prettify import:\n{}",
            code
        );
        assert!(
            code.contains("as unknown as typeof") || code.contains("TemplateBindingFN<"),
            "TSX mode should have TS casts or generics:\n{}",
            code
        );
        // Positive: type_constructs should have ambient module
        assert!(
            type_constructs.contains("declare module") || type_constructs.contains("@verter/types"),
            "TSX mode should have ambient module:\n{}",
            type_constructs
        );
    }

    // ── Bug 4: defineProps type parameter output structure ──

    #[test]
    fn define_props_type_param_output_structure() {
        let (code, _, _) = gen_tsx_script_full(
            r#"<script setup lang="ts">const props = defineProps<{ foo: string, bar: number }>()</script><template><div/></template>"#,
        );

        // Positive: type alias should be created
        assert!(
            code.contains("___VERTER___defineProps_Type"),
            "should create type alias: {code}"
        );
        // Positive: type alias should contain the original type content
        assert!(
            code.contains("{ foo: string, bar: number }"),
            "type alias should contain original type: {code}"
        );
        // Positive: defineProps call should reference the type alias
        assert!(
            code.contains("defineProps<___VERTER___defineProps_Type>()"),
            "defineProps should use type alias: {code}"
        );
    }

    #[test]
    fn define_emits_type_param_output_structure() {
        let (code, _, _) = gen_tsx_script_full(
            r#"<script setup lang="ts">const emit = defineEmits<{ click: [e: MouseEvent] }>()</script><template><div/></template>"#,
        );

        // Positive: type alias should be created
        assert!(
            code.contains("___VERTER___defineEmits_Type"),
            "should create emits type alias: {code}"
        );
        // Positive: defineEmits should use the type alias
        assert!(
            code.contains("defineEmits<___VERTER___defineEmits_Type>()"),
            "defineEmits should use type alias: {code}"
        );
    }

    #[test]
    fn define_props_type_content_is_source_mapped() {
        // Verify that individual properties inside defineProps<{ foo: string }>
        // have sourcemap coverage via move_wrapped.
        let source = r#"<script setup lang="ts">const props = defineProps<{ foo: string, bar: number }>()</script><template><div/></template>"#;
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

        if let (Some(return_close), Some(tpl_end)) = (&result.return_close, template_end) {
            ct.prepend_left(tpl_end, return_close);
        }

        // Generate the sourcemap
        let map =
            ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("App.vue"));

        let output = ct.build_string();

        // Find the position of the type block in the original source
        let type_block_start = source.find("{ foo:").unwrap() as u32;
        let type_block_end = source.find("}>()").unwrap() as u32;

        // Check that there's a sourcemap token covering the type block.
        // The move_wrapped operation preserves Original chunks, so there should
        // be a token at or near the start of the type block.
        let tokens: Vec<_> = map.get_tokens().collect();
        let has_type_coverage = tokens.iter().any(|t| {
            let src = t.get_src_col();
            src >= type_block_start && src < type_block_end
        });
        assert!(
            has_type_coverage,
            "type block [{}..{}) should have sourcemap coverage.\nOutput: {}\nTokens: {:?}",
            type_block_start, type_block_end, output, tokens
        );

        // Verify the type content appears in the output in the type alias
        assert!(
            output.contains("Prettify<{ foo: string, bar: number }>"),
            "type content should be in the Prettify wrapper: {output}"
        );
    }
}
