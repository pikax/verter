//! Setup pipeline (ownership-domain analysis).
//!
//! Hosts the two large entry points that drive `<script setup>` processing:
//! `process_tsx_script_setup` (clean parse path) and
//! `process_tsx_script_setup_error_mode` (truncate-and-reparse fallback).

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::ast::types::TemplateAst;
use crate::code_transform::CodeTransform;
use crate::compile::types::{DestructuredBindingInfo, DestructuredBlockMeta};
use crate::ide::{IdeGenericInfo, IdeScriptOptions};
use crate::parser::types::RootNodeScript;
use crate::template::code_gen::binding::BindingType;
use crate::template::code_gen::types::CodeGenOutput;
use crate::utils::oxc::bindings::collect_setup_binding_refs;
use crate::utils::oxc::vue::{parse_script_with_companion, ScriptItem, ScriptMode};

use super::{
    apply_event_handler_param_inference, apply_template_ref_call_inference,
    build_binding_source_info, collect_binding_names, detect_get_current_instance,
    detect_use_attrs_calls, directive_accessor_declaration, emit_attrs_type_aliases,
    emit_comp_functions_to_string, emit_get_root_component_to_string,
    emit_global_component_fallbacks, emit_helper_imports, emit_minimal_wrapper,
    emit_type_constructs, instance_declaration, instance_probe_line, kebab_to_pascal_case,
    process_companion_for_tsx, process_macros, rewrite_ts_type_assertions,
    should_infer_function_types, MacroSourceCtx, PREFIX,
};

// ── Script Setup Processing ───────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) fn process_tsx_script_setup<'alloc>(
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
) -> (Option<String>, Option<DestructuredBlockMeta>) {
    let content_span = match &setup.content {
        Some(span) => span,
        None => {
            // Self-closing <script setup />
            return (
                emit_minimal_wrapper(out, options, setup.tag_open.start, template_end),
                None,
            );
        }
    };

    let mut deferred_return_close: Option<String> = None;
    let mut destructured_block_meta: Option<DestructuredBlockMeta> = None;
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

    // ── Partial AST Recovery ──────────────────────────────────────
    // OXC (0.116) doesn't produce partial ASTs on errors (body is empty).
    // When real parse errors exist, we find the clean prefix before the first
    // error, re-parse it, and use that for normal codegen. The broken tail
    // passes through as-is in the CodeTransform output.
    //
    // Only enter recovery if TS-mode parse also fails. If only TSX fails
    // (but TS succeeds), the errors are from angle bracket type assertions
    // which `rewrite_ts_type_assertions` already handled.
    let mut damaged_macro_spans: Vec<verter_span::Span> = Vec::new();

    // This allocator + clean_prefix_str live for the rest of the function,
    // so parse results borrowing from them remain valid.
    let recovery_alloc;
    let mut clean_prefix_str: &str = "";
    let mut use_recovery_parse = false;

    if !parser_ret.errors.is_empty() {
        let ts_alloc = Allocator::default();
        let ts_check = Parser::new(&ts_alloc, content_str, SourceType::ts()).parse();
        if !ts_check.errors.is_empty() {
            // Find the earliest error offset (relative to content_str)
            let earliest_error = parser_ret
                .errors
                .iter()
                .flat_map(|e| e.labels.iter().flatten())
                .map(|label| label.offset())
                .min()
                .unwrap_or(0);

            // Find the last complete line boundary before the error.
            // If the error is at EOF, back up past any trailing newline first.
            let search_end = if earliest_error >= content_str.len() {
                content_str.trim_end().len()
            } else {
                earliest_error
            };
            let truncate_at = content_str[..search_end]
                .rfind('\n')
                .map(|p| p + 1) // include the newline
                .unwrap_or(0);

            if truncate_at == 0 {
                // Error is on the first line — nothing useful to recover
                return (
                    process_tsx_script_setup_error_mode(
                        setup,
                        source,
                        out,
                        type_constructs,
                        options,
                        builtin_components,
                        template_end,
                        hoist_pos,
                    ),
                    None,
                );
            }

            // Re-parse only the clean prefix
            clean_prefix_str = &content_str[..truncate_at];
            recovery_alloc = Allocator::default();
            let reparse_ret =
                Parser::new(&recovery_alloc, clean_prefix_str, SourceType::tsx()).parse();

            if reparse_ret.errors.is_empty() && !reparse_ret.program.body.is_empty() {
                use_recovery_parse = true;
                // parser_ret will be shadowed below with the re-parsed result

                // Use tokenizer to detect macros in the broken tail region
                let recovery =
                    crate::ide::script_recover::ScriptTokenScanner::new(content_str, content_start)
                        .recover();
                for m in &recovery.macros {
                    if m.call_span.end > content_start + truncate_at as u32 {
                        damaged_macro_spans.push(m.call_span);
                    }
                }
            } else {
                // Clean prefix also has errors — truly broken, use error recovery
                return (
                    process_tsx_script_setup_error_mode(
                        setup,
                        source,
                        out,
                        type_constructs,
                        options,
                        builtin_components,
                        template_end,
                        hoist_pos,
                    ),
                    None,
                );
            }
        }
    }

    // Use either the original full parse or the clean-prefix re-parse.
    // The recovery allocator must outlive `effective_program` which borrows from it.
    let recovery_alloc2 = Allocator::default();
    let recovery_ret = if use_recovery_parse {
        Some(Parser::new(&recovery_alloc2, clean_prefix_str, SourceType::tsx()).parse())
    } else {
        None
    };
    let effective_program = recovery_ret
        .as_ref()
        .map(|r| &r.program)
        .unwrap_or(&parser_ret.program);
    let effective_content_str = if use_recovery_parse {
        clean_prefix_str
    } else {
        content_str
    };

    let parse_result = parse_script_with_companion(
        effective_program,
        ScriptMode::Setup,
        content_start,
        effective_content_str,
        None, // No companion types needed for TSX — we preserve types as-is
    );

    // Build binding source info for JSDoc + offset comments
    let binding_source_info = build_binding_source_info(
        &effective_program.body,
        &effective_program.comments,
        effective_content_str,
        content_start,
    );

    // Infer event-handler parameter types from template usage (v5/process parity).
    if should_infer_function_types(setup.lang) {
        let available_bindings =
            collect_binding_names(&parse_result.bindings, source, effective_content_str);
        apply_event_handler_param_inference(
            &effective_program.body,
            template_ast,
            source,
            content_start,
            &available_bindings,
            out,
        );
        apply_template_ref_call_inference(
            &effective_program.body,
            template_ast,
            source,
            effective_content_str,
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
            // Rewrite .vue imports to .vue.ts so type providers resolve them
            // to the public API output instead of the IDE (.vue.tsx) output.
            // Uses prepend_left so the sourcemap accounts for the extra bytes.
            if imp.source.ends_with(".vue") {
                let quote_pos = content_start + imp.source_span.end - 1;
                ct.prepend_left(quote_pos, ".ts");
            }
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

    // Rewrite .vue specifiers in re-exports (e.g., `export { Foo } from './Foo.vue'`).
    // These aren't hoisted, but their specifiers still need .vue → .vue.ts.
    for item in &parse_result.items {
        if let ScriptItem::Export(exp) = item {
            if let (Some(src), Some(src_span)) = (exp.source, exp.source_span) {
                if src.ends_with(".vue") {
                    let quote_pos = content_start + src_span.end - 1;
                    ct.prepend_left(quote_pos, ".ts");
                }
            }
        }
    }

    // Rewrite .vue specifiers in dynamic imports (e.g., `import('./Foo.vue')`).
    for src_span in &parse_result.vue_dynamic_import_spans {
        let quote_pos = content_start + src_span.end - 1;
        ct.prepend_left(quote_pos, ".ts");
    }

    // Extract bindings
    // Note: binding spans have mixed coordinate systems (see script/macros.rs:93):
    // - Props/PropsAliased spans are SFC-absolute (content_offset baked in by resolve_type)
    // - All other bindings are relative to content_str (0-based from OXC parser)
    // Bounds-checked: partial ASTs may produce garbage spans, skip invalid ones.
    for (span, bt) in &parse_result.bindings {
        let name = if *bt == BindingType::Props || *bt == BindingType::PropsAliased {
            // Absolute span — index into full SFC source
            match source.get(span.start as usize..span.end as usize) {
                Some(s) if !s.is_empty() => s,
                _ => continue,
            }
        } else {
            // Relative span — index into content_str (script content only)
            match content_str.get(span.start as usize..span.end as usize) {
                Some(s) if !s.is_empty() => s,
                _ => continue,
            }
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
    let use_attrs_info = detect_use_attrs_calls(&effective_program.body, effective_content_str);
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
        .or(use_attrs_info.type_arg);

    // Insert type assertion casts for bare useAttrs() calls.
    // When explicit `attrs` attribute is specified and the function has `_attrs` param,
    // cast bare `useAttrs()` calls to `typeof _attrs` for type-safe attrs access.
    // When no explicit attrs type is provided and a template exists (so ___VERTER___Attrs
    // will be emitted), cast bare `useAttrs()` calls to `___VERTER___Attrs` so the
    // return type reflects root element fallthrough attributes.
    if !options.is_jsx && !use_attrs_info.bare_call_ends.is_empty() {
        let has_explicit_attrs = setup.attrs.is_some() && attrs_type.is_some();
        if has_explicit_attrs {
            // Explicit attrs="..." → cast to typeof _attrs (sourcemapped parameter)
            let cast = " as typeof _attrs";
            for &end_offset in &use_attrs_info.bare_call_ends {
                let sfc_offset = content_start + end_offset;
                out.prepend_alloc(sfc_offset, cast);
            }
        } else if attrs_type.is_none() && template_ast.is_some() {
            // No explicit attrs → cast to ___VERTER___Attrs (root element fallthrough)
            let gn = generic_info
                .as_ref()
                .map(|g| g.names_bracket())
                .unwrap_or_default();
            let cast = format!(" as unknown as {}Attrs{}", PREFIX, gn);
            for &end_offset in &use_attrs_info.bare_call_ends {
                let sfc_offset = content_start + end_offset;
                out.prepend_alloc(sfc_offset, &cast);
            }
        }
    }

    // Process macros: emit type aliases only (no boxing).
    // Skip macros whose spans overlap with parse errors (damaged by typing).
    let mut macro_ctx = MacroSourceCtx {
        source,
        content_str,
        content_start,
        out,
        is_jsx: options.is_jsx,
    };
    let macro_state = process_macros(&parse_result.items, &mut macro_ctx, &damaged_macro_spans);
    let out = macro_ctx.out;

    // For NormalSkipDamagedMacros: recover binding names from damaged macros,
    // variables, and functions using the lightweight token scanner, so templates
    // can still reference them.
    if !damaged_macro_spans.is_empty() {
        let recovery =
            crate::ide::script_recover::ScriptTokenScanner::new(content_str, content_start)
                .recover();
        for m in &recovery.macros {
            if let Some(name) = m.binding_name {
                let bt = match m.kind {
                    crate::ide::script_recover::RecoveredMacroKind::DefineProps => {
                        BindingType::Props
                    }
                    crate::ide::script_recover::RecoveredMacroKind::WithDefaults => {
                        BindingType::Props
                    }
                    crate::ide::script_recover::RecoveredMacroKind::DefineEmits => {
                        BindingType::SetupConst
                    }
                    crate::ide::script_recover::RecoveredMacroKind::DefineModel => {
                        BindingType::SetupRef
                    }
                    crate::ide::script_recover::RecoveredMacroKind::DefineSlots => {
                        BindingType::SetupConst
                    }
                    _ => BindingType::SetupConst,
                };
                let alloc_name = alloc.alloc_str(name);
                bindings.entry(alloc_name).or_insert(bt);
            }
        }
        // Recover variable bindings from the broken tail
        for v in &recovery.variables {
            let bt = match v.kind {
                crate::ide::script_recover::RecoveredVarKind::Const => BindingType::SetupConst,
                _ => BindingType::SetupLet,
            };
            let alloc_name = alloc.alloc_str(v.name);
            bindings.entry(alloc_name).or_insert(bt);
        }
        // Recover function bindings from the broken tail
        for f in &recovery.functions {
            let alloc_name = alloc.alloc_str(f.name);
            bindings
                .entry(alloc_name)
                .or_insert(BindingType::SetupConst);
        }
    }

    // Detect getCurrentInstance() usage for conditional type emission
    let has_get_current_instance = detect_get_current_instance(&effective_program.body);

    // Build component function wrapper opening
    // Replace <script setup> tag with ___VERTER___TemplateBindingFN function declaration.
    //
    // When `generic` or `attrs` attribute values are present, the overwrite is split
    // into segments that skip over those value spans, preserving them as original
    // content in the CodeTransform. This keeps sourcemaps accurate so that
    // hover/completions inside `generic="..."` and `attrs="..."` resolve correctly.
    //
    // In JSX mode, drop generics and attrs annotations (no TypeScript syntax in JS output).
    let async_prefix = if parse_result.is_async { "async " } else { "" };

    // Determine which sourcemapped spans to preserve in the function signature.
    // TypeScript syntax requires: function FN<Generic>(params)
    // So generic MUST appear before params, regardless of source attribute order.
    let gen_span = if !options.is_jsx {
        setup.generic.and_then(|s| {
            let content = &source[s.start as usize..s.end as usize];
            if content.trim().is_empty() {
                None
            } else {
                Some(s)
            }
        })
    } else {
        None
    };
    let attr_span = if !options.is_jsx {
        setup.attrs.and_then(|s| {
            let content = &source[s.start as usize..s.end as usize];
            if content.trim().is_empty() {
                None
            } else {
                Some(s)
            }
        })
    } else {
        None
    };

    match (gen_span, attr_span) {
        (None, None) => {
            // No preserved spans — single overwrite.
            let generic_bracket = if options.is_jsx {
                String::new()
            } else {
                generic_info
                    .as_ref()
                    .map(|g| g.source_bracket())
                    .or_else(|| raw_generic.as_ref().map(|r| format!("<{}>", r)))
                    .unwrap_or_default()
            };
            let wrapper_start = format!(
                ";export {}function {}TemplateBindingFN{}() {{\n",
                async_prefix, PREFIX, generic_bracket,
            );
            out.overwrite(setup.tag_open.start, setup.tag_open.end, &wrapper_start);
        }
        (Some(gen), None) => {
            // Only generic: preserve its span with <> wrapping.
            let fn_prefix = format!(
                ";export {}function {}TemplateBindingFN<",
                async_prefix, PREFIX
            );
            out.overwrite(setup.tag_open.start, gen.start, &fn_prefix);
            out.overwrite(gen.end, setup.tag_open.end, ">() {\n");
        }
        (None, Some(attr)) => {
            // Only attrs: preserve its span with (_attrs: ) wrapping.
            let fn_prefix = format!(
                ";export {}function {}TemplateBindingFN(_attrs: ",
                async_prefix, PREFIX
            );
            out.overwrite(setup.tag_open.start, attr.start, &fn_prefix);
            out.overwrite(attr.end, setup.tag_open.end, ") {\n");
        }
        (Some(gen), Some(attr)) => {
            // Both present. Output must be: FN<generic>(_attrs: attrs)
            // Source order may differ — handle both cases.
            if gen.start < attr.start {
                // Source order matches desired order: generic before attrs.
                // Emit segments left-to-right around both preserved spans.
                let fn_prefix = format!(
                    ";export {}function {}TemplateBindingFN<",
                    async_prefix, PREFIX
                );
                out.overwrite(setup.tag_open.start, gen.start, &fn_prefix);
                out.overwrite(gen.end, attr.start, ">(_attrs: ");
                out.overwrite(attr.end, setup.tag_open.end, ") {\n");
            } else {
                // Source order is attrs before generic — need to reorder.
                // Use move_wrapped to relocate generic content before attrs.
                //
                // Source: ...attrs="ATTRS"...generic="GEN"...>
                // Output: ...FN<GEN>(_attrs: ATTRS) {\n
                //
                // 1. Overwrite [tag_open.start, attr.start) → function prefix with "<"
                // 2. Move generic content to attr.start with suffix ">(_attrs: "
                //    This inserts "GEN>(_attrs: " just before attrs content.
                // 3. Attrs content stays in place (preserved, sourcemapped).
                // 4. Overwrite [attr.end, gen.start) → empty (removes gap text)
                // 5. Overwrite [gen.end, tag_open.end) → ") {\n"
                //    (gen content was moved away, original position is empty)
                let fn_prefix = format!(
                    ";export {}function {}TemplateBindingFN<",
                    async_prefix, PREFIX
                );
                out.overwrite(setup.tag_open.start, attr.start, &fn_prefix);
                out.move_wrapped(gen.start, gen.end, attr.start, "", ">(_attrs: ");
                // attrs content preserved at [attr.start, attr.end)
                out.overwrite(attr.end, gen.start, "");
                out.overwrite(gen.end, setup.tag_open.end, ") {\n");
            }
        }
    }

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
        if has_template {
            wrapper_end.push_str(&directive_accessor_declaration(options.is_jsx));
        }

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

        // Emit global component fallback consts BEFORE the block scope.
        // These provide types for globally registered components (e.g. RouterLink,
        // RouterView) that aren't imported. They must be declared before the block
        // scope so the template JSX inside can reference them without TDZ errors.
        emit_global_component_fallbacks(
            &mut wrapper_end,
            template_ast,
            source,
            bindings,
            options.is_jsx,
        );

        // Emit self-referencing component declaration (#28).
        // When a component's template uses its own name (e.g., <TreeNode /> inside
        // TreeNode.vue), this const provides the binding so TypeScript resolves
        // the JSX element. Only emitted when:
        // 1. The name is not already in bindings (user hasn't imported same name)
        // 2. The template actually references the component's own name as a tag
        let self_name_pascal = kebab_to_pascal_case(options.component_name);
        let template_uses_self = template_ast
            .and_then(|tpl| tpl.root.content.as_ref())
            .map(|c| {
                let tpl_src = &source[c.start as usize..c.end as usize];
                // Check for <PascalName or <kebab-name tag usage
                tpl_src.contains(&format!("<{}", self_name_pascal))
                    || tpl_src.contains(&format!("<{}", options.component_name))
            })
            .unwrap_or(false);
        if template_uses_self
            && !self_name_pascal.is_empty()
            && !bindings.contains_key(self_name_pascal.as_str())
        {
            let alloc_self_name = alloc.alloc_str(&self_name_pascal);
            bindings.insert(alloc_self_name, BindingType::SetupConst);
            let basename = options
                .filename
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(options.filename);
            if options.is_jsx {
                wrapper_end.push_str(&format!(
                    "const {} = /** @type {{any}} */ ({{}});\n",
                    self_name_pascal
                ));
            } else {
                wrapper_end.push_str(&format!(
                    "const {} = {{}} as typeof import('./{}').default;\n",
                    self_name_pascal, basename
                ));
            }
        }

        // Emit CSS module declarations (#76).
        // When <style module> exists, inject a typed $style binding so template
        // expressions like `:class="$style.btn"` get type checking and completions.
        for css_mod in &options.css_modules {
            if !bindings.contains_key(css_mod.binding_name.as_str()) {
                let alloc_name = alloc.alloc_str(&css_mod.binding_name);
                bindings.insert(alloc_name, BindingType::SetupConst);
                if options.is_jsx {
                    wrapper_end.push_str(&format!(
                        "const {} = /** @type {{Record<string, string>}} */ ({{}});\n",
                        css_mod.binding_name
                    ));
                } else {
                    let entries: String = css_mod
                        .class_names
                        .iter()
                        .map(|name| format!("  readonly \"{}\": string;", name))
                        .collect::<Vec<_>>()
                        .join("\n");
                    wrapper_end.push_str(&format!(
                        "const {} = {{}} as {{\n{}\n}};\n",
                        css_mod.binding_name, entries
                    ));
                }
            }
        }

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
            // Binding source positions are stored in DestructuredBlockMeta (no inline comments).
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
                    .map(|name| format!("\n    {}", name))
                    .collect::<Vec<_>>()
                    .join(",")
            };

            // Collect binding metadata from binding_source_info
            let mut destruct_bindings: Vec<DestructuredBindingInfo> = Vec::new();
            for name in const_names.iter().chain(let_names.iter()) {
                if let Some(info) = binding_source_info.get(name) {
                    destruct_bindings.push(DestructuredBindingInfo {
                        name: name.to_string(),
                        source_span: verter_span::Span::new(info.sfc_start, info.sfc_end),
                    });
                }
            }

            let mut destruct_block = String::from("{ /* verter-destructured-start */");
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
            destruct_block.push_str(" /* verter-destructured-end */\n");

            // Emit void(name) for bindings referenced in script body or style v-bind().
            // This prevents TS from flagging them as "unused" when they're only used
            // in script (not in template) or only in style v-bind() expressions.
            let setup_name_set: FxHashSet<&str> = setup_bindings.iter().map(|(n, _)| *n).collect();
            let mut script_refs = collect_setup_binding_refs(effective_program, &setup_name_set);
            // Merge style v-bind references
            for name in &options.style_v_bind_vars {
                if setup_name_set.contains(name.as_str()) {
                    // We need a &str that lives long enough — use the setup_bindings entry
                    if let Some((n, _)) = setup_bindings.iter().find(|(n, _)| *n == name.as_str()) {
                        script_refs.insert(n);
                    }
                }
            }
            if !script_refs.is_empty() {
                for (name, _) in &setup_bindings {
                    if script_refs.contains(name) {
                        destruct_block.push_str(&format!("void({});", name));
                    }
                }
                destruct_block.push('\n');
            }

            wrapper_end.push_str(&destruct_block);

            // Store metadata (block_start/block_end computed later from final TSX)
            if !destruct_bindings.is_empty() {
                destructured_block_meta = Some(DestructuredBlockMeta {
                    bindings: destruct_bindings,
                    block_start: 0,
                    block_end: 0,
                });
            }
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
            let prop_names: rustc_hash::FxHashSet<&str> = bindings
                .iter()
                .filter(|(_, bt)| bt.is_props())
                .map(|(name, _)| *name)
                .collect();
            let (root_comp_entries, all_comp_offsets) = emit_comp_functions_to_string(
                &mut tail,
                &gs,
                &gn,
                template_ast,
                source,
                options.is_jsx,
                &prop_names,
            );
            // Analyze root conditions for narrowing (when enabled and multiple branches)
            let narrowing_result = if options.conditional_root_narrowing
                && root_comp_entries.len() > 1
            {
                let prop_names: rustc_hash::FxHashSet<&str> = bindings
                    .iter()
                    .filter(|(_, bt)| bt.is_props())
                    .map(|(name, _)| *name)
                    .collect();
                let conditions: Vec<(Option<&str>, u32)> = root_comp_entries
                    .iter()
                    .map(|(offset, _, cond)| (cond.as_deref(), *offset))
                    .collect();
                crate::ide::condition_narrowing::analyze_conditional_chain(&conditions, &prop_names)
                    .ok()
            } else {
                None
            };

            // Always emit getRootComponent when there's a template (needed for implicit attrs)
            if template_ast.is_some() {
                emit_get_root_component_to_string(
                    &mut tail,
                    &gs,
                    &gn,
                    &root_comp_entries,
                    narrowing_result.as_ref(),
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
            // Suppress TS6133 for generated variables that may not be referenced in template
            if macro_state
                .macro_bindings
                .iter()
                .any(|e| e.macro_name == "defineProps" && e.var_name.is_some())
            {
                tail.push_str("\nvoid __props;");
            }
            for entry in &macro_state.macro_bindings {
                if let Some(ref var) = entry.var_name {
                    if var.starts_with(PREFIX) {
                        tail.push_str(&format!("\nvoid {};", var));
                    }
                }
            }
            for offset in &all_comp_offsets {
                tail.push_str(&format!(
                    "\nvoid {P}Comp{offset};",
                    P = PREFIX,
                    offset = offset,
                ));
            }

            // Emit instance completion probe line (LSP uses this for autocomplete)
            tail.push_str(&instance_probe_line());
            tail.push_str("\nreturn {};\n} // close templateBindingFN\n");
            deferred_return_close = Some(tail);
        } else {
            // No template: emit block scope + close immediately.
            wrapper_end.push_str("\n} // close block scope\n");
            wrapper_end.push_str("\nreturn {};\n} // close templateBindingFN\n");
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
        true, // has Comp functions
    );

    (deferred_return_close, destructured_block_meta)
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
            wrapper_open.push_str(&directive_accessor_declaration(options.is_jsx));
            out.overwrite(tag_close.start, tag_close.end, &wrapper_open);
            let mut close = String::from("\n");
            close.push_str(&instance_probe_line());
            close.push_str("return {};\n} // close templateBindingFN\n");
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
        true,  // emit attributes type (error mode still needs it)
    );

    deferred_return_close
}
