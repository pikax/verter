//! Setup pipeline (ownership-domain analysis).
//!
//! Hosts `process_tsx_script_setup`, the single entry point that drives
//! `<script setup>` processing. OXC parses the original content once; a clean
//! parse takes the full codegen path, while a genuine syntax error routes
//! through error-tolerant recovery (a single token scan of the real source via
//! [`crate::ide::script_recover::ScriptSetupRecoveryPlan`]) that keeps the user's
//! body valid TSX and inside the `___VERTER___TemplateBindingFN` wrapper without
//! ever reparsing a synthetic view.

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::ast::types::TemplateAst;
use crate::code_transform::CodeTransform;
use crate::compile::types::{DestructuredBindingInfo, DestructuredBlockMeta};
use crate::cursor::ScriptLanguage;
use crate::ide::{IdeGenericInfo, IdeScriptOptions};
use crate::parser::types::RootNodeScript;
use crate::template::code_gen::binding::BindingType;
use crate::template::code_gen::types::CodeGenOutput;
use crate::utils::oxc::bindings::collect_setup_binding_refs;
use crate::utils::oxc::vue::{parse_script_with_companion, ScriptItem, ScriptMacro, ScriptMode};

use super::{
    apply_event_handler_param_inference, apply_template_ref_call_inference,
    build_binding_source_info, collect_binding_names, collect_global_component_fallbacks,
    detect_get_current_instance, detect_use_attrs_calls, directive_accessor_declaration,
    emit_attrs_type_aliases, emit_comp_functions_to_string, emit_get_root_component_to_string,
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
    template_component_fallbacks: &mut Vec<String>,
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

    // ── Error-Tolerant Recovery ───────────────────────────────────
    // OXC parses the ORIGINAL `content_str` exactly once (above). On a clean
    // parse the full codegen path below runs unchanged. On a GENUINE syntax
    // error — both the TSX parse AND a TS-mode parse fail (a TSX-only failure is
    // an angle-bracket assertion already handled by `rewrite_ts_type_assertions`)
    // — we DO NOT reparse anything. A single token scan of the REAL source
    // produces a `ScriptSetupRecoveryPlan`: its original-span import/macro/binding
    // facts feed hoisting and binding registration, and its OUTPUT-ONLY member /
    // expression holes plus scope closers keep the user's body valid AND inside
    // the `___VERTER___TemplateBindingFN` wrapper. OXC (0.116) yields an empty
    // body on error, so `parser_ret.program` contributes nothing on this path —
    // every recovered fact comes from the real-source scan, never a synthetic view.
    let recovery_plan: Option<crate::ide::script_recover::ScriptSetupRecoveryPlan> =
        if parser_ret.errors.is_empty() {
            None
        } else {
            let ts_alloc = Allocator::default();
            let ts_check = Parser::new(&ts_alloc, content_str, SourceType::ts()).parse();
            if ts_check.errors.is_empty() {
                // TSX-only failure (angle-bracket assertion) — clean-path metadata.
                None
            } else {
                Some(
                    crate::ide::script_recover::ScriptTokenScanner::new(content_str, content_start)
                        .recover_plan(),
                )
            }
        };

    // On the failure path OXC gives an empty body; the codegen below degrades to
    // wrapper + recovered facts. On the clean path this is the full parse.
    let effective_program = &parser_ret.program;
    let effective_content_str = content_str;

    // Unused-binding LIVENESS completeness for the script source: a clean parse
    // gives a complete program to the free-reference collector, but ANY OXC parse
    // error (a genuine syntax error routed through recovery, OR a TSX-only failure
    // whose degraded program OXC still hands back empty) means the collector
    // UNDER-COUNTS script references. An under-counted script ref would OMIT a
    // genuinely-used binding from the unwrap surface → false TS6133. So a script
    // with parse errors is INCOMPLETE and the liveness gate must fail open. (This
    // is purely a liveness signal; it does NOT change the recovery codegen path.)
    let script_complete = parser_ret.errors.is_empty();

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

    // Named template-driven event parameters are owned by exact TypeScript
    // script-setup scope. TSX remains eligible for unrelated template-ref
    // inference below, but its authored function parameters are not rewritten.
    if should_infer_function_types(setup.lang) {
        let available_bindings =
            collect_binding_names(&parse_result.bindings, source, effective_content_str);
        if setup.lang == Some(ScriptLanguage::TypeScript) {
            apply_event_handler_param_inference(
                &effective_program.body,
                template_ast,
                source,
                content_start,
                &available_bindings,
                out,
            );
        }
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
            // The in-project bare `.vue` specifier is emitted VERBATIM: a bare
            // framework-carrier import resolves natively to the `.d.vue.ts`
            // declaration carrier (emitted + proactively opened) through tsgo's
            // basename-append probe — no compile-time specifier rewrite.
            ct.move_with_suffix(abs_start, abs_end, hoist_pos, "\n");
        }
    }

    // On the recovery path the empty parse contributes no `ScriptItem::Import`, so
    // hoist the imports the token scanner recovered from the REAL source instead.
    // Their spans are already SFC-absolute. This keeps top-level imports out of the
    // function wrapper (TS1232) while the user types a broken statement below them.
    if let Some(plan) = &recovery_plan {
        for imp in &plan.imports {
            // The recovered import's bare `.vue` specifier is emitted verbatim —
            // it resolves natively to the `.d.vue.ts` declaration carrier (see the
            // clean-path hoist above). The recovered source span is needed only to
            // hoist the statement, not to rewrite the specifier.
            ct.move_with_suffix(imp.span.start, imp.span.end, hoist_pos, "\n");
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

    // In-project `.vue` re-export (`export { Foo } from './Foo.vue'`) and dynamic
    // import (`import('./Foo.vue')`) specifiers are emitted VERBATIM — a bare
    // framework-carrier import resolves natively to the `.d.vue.ts` declaration
    // carrier, so there is no compile-time specifier rewrite for either form.

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

    // The clean parse drives macro lowering; on the recovery path `parse_result`
    // is empty, so there are no AST macros to lower here (recovered macro facts
    // register their bindings below). No macro spans are ever "damaged" anymore.
    let mut macro_ctx = MacroSourceCtx {
        source,
        content_str,
        content_start,
        out,
        is_jsx: options.is_jsx,
    };
    let macro_state = process_macros(&parse_result.items, &mut macro_ctx, &[]);
    let out = macro_ctx.out;

    // Preserve the dual identity of defineProps destructuring: these names are
    // prop carriers for metadata/runtime purposes, and real source locals for
    // IDE TSX expressions. The dedicated binding kind lets the TSX resolver use
    // the local without weakening prop ownership anywhere else.
    let destructured_prop_bindings = destructured_define_props_bindings(&parse_result.items);
    for name in &destructured_prop_bindings {
        bindings.insert(alloc.alloc_str(name), BindingType::PropsDestructured);
    }

    // ── Failure-path recovery application ──────────────────────────
    // Register bindings the token scanner recovered from the REAL source (so the
    // template still resolves them) and emit the OUTPUT-ONLY holes that keep the
    // user's broken body valid TSX. Every fact here comes from an original source
    // span; the synthetic hole placeholders are never registered as bindings.
    if let Some(plan) = &recovery_plan {
        for m in &plan.macros {
            if let Some(name) = m.binding_name {
                let bt = match m.kind {
                    crate::ide::script_recover::RecoveredMacroKind::DefineProps
                    | crate::ide::script_recover::RecoveredMacroKind::WithDefaults => {
                        BindingType::Props
                    }
                    crate::ide::script_recover::RecoveredMacroKind::DefineModel => {
                        BindingType::SetupRef
                    }
                    _ => BindingType::SetupConst,
                };
                let alloc_name = alloc.alloc_str(name);
                bindings.entry(alloc_name).or_insert(bt);
            }
        }
        for v in &plan.variables {
            let bt = match v.kind {
                crate::ide::script_recover::RecoveredVarKind::Const => BindingType::SetupConst,
                _ => BindingType::SetupLet,
            };
            let alloc_name = alloc.alloc_str(v.name);
            bindings.entry(alloc_name).or_insert(bt);
        }
        for f in &plan.functions {
            let alloc_name = alloc.alloc_str(f.name);
            bindings
                .entry(alloc_name)
                .or_insert(BindingType::SetupConst);
        }
        // Imported value bindings (type-only imports introduce no value binding).
        for imp in &plan.imports {
            if imp.is_type_only {
                continue;
            }
            for name in &imp.binding_names {
                let alloc_name = alloc.alloc_str(name);
                bindings
                    .entry(alloc_name)
                    .or_insert(BindingType::SetupImport);
            }
        }
        // Emit the OUTPUT-ONLY recovery holes (unmapped synthetic chunks). A
        // member hole fills a dangling `a.` / `a?.` with a universal member so the
        // dot cannot absorb the following token; an expression hole completes a
        // trailing operator / assignment / arm. Neither becomes a source fact.
        for insert in &plan.inserts {
            match insert {
                crate::ide::script_recover::RecoveryInsert::MemberHole { at } => {
                    out.prepend_static(*at, "valueOf");
                }
                crate::ide::script_recover::RecoveryInsert::ExpressionHole { at } => {
                    out.prepend_static(*at, "(undefined)");
                }
            }
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

        // The defineProps/withDefaults LHS binding name — from the clean lowered
        // `macro_state` OR, on the recovery path, from the recovered macro fact.
        // Template codegen maps a Props binding to `__props.`, so the `__props`
        // alias MUST exist in BOTH paths or the template reference dangles. Recovery
        // marks the LHS Props (script/setup binding registration above); this gives
        // it the same alias semantics as clean macro lowering.
        let define_props_var: Option<&str> = macro_state
            .macro_bindings
            .iter()
            .find(|e| e.macro_name == "defineProps")
            .and_then(|e| e.var_name.as_deref())
            .or_else(|| {
                recovery_plan.as_ref().and_then(|plan| {
                    plan.macros
                        .iter()
                        .filter(|m| {
                            matches!(
                                m.kind,
                                crate::ide::script_recover::RecoveredMacroKind::DefineProps
                                    | crate::ide::script_recover::RecoveredMacroKind::WithDefaults
                            )
                        })
                        .find_map(|m| m.binding_name)
                })
            });

        // Recovery boundary (failure path only): this string is emitted right
        // after the user's body (it overwrites `</script>`). First close any
        // brackets the user left open, then terminate the now-complete trailing
        // statement, so the generated scaffolding below starts at a clean
        // statement boundary instead of being absorbed by a dangling expression.
        // These chunks are OUTPUT-ONLY (unmapped) and never become source facts.
        if let Some(plan) = &recovery_plan {
            wrapper_end.push_str(&plan.scope_closers);
            wrapper_end.push(';');
        }

        // Inject __props alias so template codegen's `__props.xxx` references resolve
        // (clean path and recovery path share the same alias semantics).
        if let Some(props_var) = define_props_var {
            wrapper_end.push_str(&format!("\nconst __props = {};", props_var));
        }

        // Template expressions now read PropsDestructured locals directly. Style
        // v-bind usage has no TSX expression of its own, so mirror that external
        // use with a value-read. Preserve the conservative liveness invariant:
        // incomplete template/style usage means "possibly used"; a completely
        // proven-unused local gets no synthetic read and remains eligible for TS6133.
        if !destructured_prop_bindings.is_empty() {
            let style_v_bind_set: FxHashSet<&str> = options
                .style_v_bind_vars
                .iter()
                .map(String::as_str)
                .collect();
            let usage_incomplete =
                options.template_used_vars.is_none() || !options.style_usage_complete;
            for name in &destructured_prop_bindings {
                let style_used = style_v_bind_set.contains(name);
                if usage_incomplete || style_used {
                    wrapper_end.push_str(&format!("\nvoid({name});"));
                }
            }
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
        // Includes every LIVE setup binding except Props/PropsAliased (accessed
        // via __props). Imports are already in scope from hoisting, so they're
        // excluded too. Bindings used in script/style (but not the template) are
        // intentionally included so IntelliSense always shows unwrapped types in
        // the template. A PROVEN-unused binding is OMITTED here (see the
        // liveness/omission block below) so its SOURCE `const name` carries TS6133
        // directly at its mapped span — never an unmapped destructure copy.
        let mut import_names: FxHashSet<&str> = parse_result
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
        // Recovered imports are in scope via hoisting, not destructuring — exclude
        // their local names from the shallowUnwrapRef block just like clean imports.
        if let Some(plan) = &recovery_plan {
            for imp in &plan.imports {
                for name in &imp.binding_names {
                    import_names.insert(name);
                }
            }
        }

        let setup_bindings: Vec<(&str, BindingType)> = bindings
            .iter()
            .filter(|(_, bt)| !bt.is_props() && !matches!(bt, BindingType::PropsAliased))
            .filter(|(name, _)| {
                let n: &str = name;
                !import_names.contains(n)
            })
            .map(|(name, bt)| (*name, *bt))
            .collect();

        // Collect, then emit, global component fallback consts BEFORE the block scope.
        // These provide types for globally registered components (e.g. RouterLink,
        // RouterView) that aren't imported. They must be declared before the block
        // scope so the template JSX inside can reference them without TDZ errors.
        // The collected list is also handed back to the template-typing inventory so a
        // global component's `@event` payload resolves through the same `InstanceType<typeof
        // Pascal>["$props"]` const that is emitted here.
        let global_fallbacks =
            collect_global_component_fallbacks(template_ast, source, |n| bindings.contains_key(n));
        emit_global_component_fallbacks(&mut wrapper_end, &global_fallbacks, options.is_jsx);
        *template_component_fallbacks = global_fallbacks;

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
            // Liveness inventory: a setup binding is "used somewhere" if it is
            // referenced by the template, the script body, or a style `v-bind()`.
            // This drives whether the binding PARTICIPATES in the
            // `___VERTER___unwrapped` object + destructure block. A used binding
            // keeps a VALUE-READ entry so its source decl stays live (correct —
            // it is used) and the template can read the unwrapped local. An
            // unused binding is OMITTED entirely from the unwrapped object AND
            // the destructure block, so the user's original `const name` is its
            // sole remaining occurrence and TypeScript surfaces TS6133 at the
            // SOURCE decl span (which the source map maps back to the real
            // declaration line — unlike a synthesized destructure copy, which
            // has no per-binding source mapping and would collapse the
            // diagnostic to line 1).
            //
            // A type-only entry (`undefined as unknown as typeof name`) does NOT
            // work: `typeof name` is itself a type-query REFERENCE to `name`, so
            // it keeps the SOURCE decl live and tsc never flags it; the
            // diagnostic instead falls on the unmapped `const { name } =
            // ___VERTER___unwrapped` destructure copy and collapses to line 1.
            // Omission is the only shape that lands TS6133 on the source decl.
            // Template IntelliSense / `shallowUnwrapRef` typing is unaffected
            // because an omitted binding is, by definition, referenced nowhere
            // (template included), so nothing reads its unwrapped property.
            //
            // ── THE CONSERVATIVE SAFETY INVARIANT: unknown => used ──
            // Omission is allowed ONLY on a PROVEN-unused binding. If usage
            // cannot be determined COMPLETELY and SOUNDLY across template +
            // script + style, EVERY binding keeps its value-read (no TS6133).
            // A false negative (a real unused binding left undiagnosed) is the
            // correct tradeoff for a diagnostic gate; a false positive (a used
            // binding flagged unused) is not acceptable. The gate is therefore
            // `all_sources_complete && !used`.
            //
            // All three sources are typed-IR/AST facts — no string heuristics,
            // no name matching:
            //  - template:   `options.template_used_vars` (expression bindings ∪
            //                 v-for/v-slot refs ∪ component-tag candidates), built
            //                 from the COMPLETE `ide_completion = false` overlay.
            //                 `None` ⇒ INCOMPLETE (parse errors / no template).
            //  - script body: `collect_setup_binding_refs` (complete `Visit`
            //                 free-reference collector), sound only when
            //                 `script_complete` (the `<script setup>` parsed
            //                 without error; a recovered program under-counts).
            //  - style:       `options.style_v_bind_vars`, sound only when
            //                 `options.style_usage_complete`.
            let setup_name_set: FxHashSet<&str> = setup_bindings.iter().map(|(n, _)| *n).collect();
            let script_refs = collect_setup_binding_refs(effective_program, &setup_name_set);
            let style_v_bind_set: FxHashSet<&str> = options
                .style_v_bind_vars
                .iter()
                .map(|s| s.as_str())
                .collect();

            // All usage sources must be complete for ANY omission to be sound.
            // Template usage is complete only when the overlay produced a set
            // (`Some`) with no per-expression parse error; style usage is complete
            // only when every `v-bind()` parsed; script usage is complete only when
            // the `<script setup>` parsed without error (a recovered/degraded
            // program makes the `Visit` collector under-count). ANY incomplete
            // source ⇒ the gate fails open (no omission).
            let all_sources_complete = options.template_used_vars.is_some()
                && options.style_usage_complete
                && script_complete;

            let is_used_somewhere = |name: &str| -> bool {
                script_refs.contains(name)
                    || style_v_bind_set.contains(name)
                    || define_props_var == Some(name)
                    || match &options.template_used_vars {
                        Some(tpl) => tpl.contains(name),
                        None => true,
                    }
            };

            // A binding is OMITTED from the unwrapped object + destructure block
            // ONLY when all usage sources are complete AND it is referenced
            // nowhere. Otherwise (any source incomplete, or used anywhere) it
            // keeps its value-read entry — the fail-open default.
            let should_omit =
                |name: &str| -> bool { all_sources_complete && !is_used_somewhere(name) };

            // Live (kept) bindings: everything not proven-unused. These are the
            // only bindings that appear in the unwrapped object and destructure;
            // proven-unused bindings are dropped so their SOURCE decl carries the
            // TS6133 at its mapped position.
            let live_bindings: Vec<(&str, BindingType)> = setup_bindings
                .iter()
                .copied()
                .filter(|(name, _)| !should_omit(name))
                .collect();

            let entries: String = live_bindings
                .iter()
                .map(|(name, _)| {
                    let jsdoc = binding_source_info
                        .get(name)
                        .and_then(|info| info.jsdoc.as_deref());
                    if options.is_jsx {
                        // JSX mode: no TS casts — a live binding reads the value.
                        if let Some(jsdoc) = jsdoc {
                            format!("{}\n    {}: {}", jsdoc, name, name)
                        } else {
                            format!("{}: {}", name, name)
                        }
                    } else {
                        // TSX mode: `name as unknown as typeof name` keeps the
                        // decl live (value-read) AND carries the unwrapped type.
                        if let Some(jsdoc) = jsdoc {
                            format!(
                                "{}\n    {}: {} as unknown as typeof {}",
                                jsdoc, name, name, name
                            )
                        } else {
                            format!("{}: {} as unknown as typeof {}", name, name, name)
                        }
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
            // so that v-model assignment handlers don't trigger TS2588. Only LIVE
            // bindings are destructured — a proven-unused binding is omitted so
            // its source decl, not an unmapped destructure copy, carries TS6133.
            let const_names: Vec<&str> = live_bindings
                .iter()
                .filter(|(_, bt)| matches!(bt, BindingType::SetupConst | BindingType::LiteralConst))
                .map(|(name, _)| *name)
                .collect();
            let let_names: Vec<&str> = live_bindings
                .iter()
                .filter(|(_, bt)| {
                    !matches!(bt, BindingType::SetupConst | BindingType::LiteralConst)
                })
                .map(|(name, _)| *name)
                .collect();

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

            // Emit the destructure block as ORDERED CT segments at the </script>
            // boundary: the scaffold text unmapped, every destructured binding
            // NAME source-mapped to its authored identifier span. The provider
            // resolves a template-usage definition to the destructured alias (it
            // shadows the authored binding in the block scope); without the
            // name-level mapping, the response remap fails closed and every
            // markup definition/reference/rename drops on the editor surface.
            // One ordered CT segment: (insertion offset, optional (source
            // start, source offset-delta), text) — the tuple shape
            // `batch_prepend_left_with_source_map` consumes.
            type DestructSegment<'a> = (u32, Option<(u32, u32)>, &'a str);
            let mut destruct_segments: Vec<DestructSegment<'alloc>> = Vec::new();
            let anchor = tag_close.end;
            destruct_segments.push((anchor, None, "{ /* verter-destructured-start */"));
            let push_group = |segments: &mut Vec<DestructSegment<'alloc>>,
                              keyword: &str,
                              names: &[&str],
                              ct: &mut CodeTransform<'alloc>| {
                if names.is_empty() {
                    return;
                }
                segments.push((anchor, None, ct.alloc_str(&format!("{keyword} {{ "))));
                for (index, name) in names.iter().enumerate() {
                    let separator = if index == 0 { "\n    " } else { ",\n    " };
                    let source_start = binding_source_info.get(name).map(|info| info.sfc_start);
                    if let Some(start) = source_start {
                        // A token-level authored boundary at the declaration
                        // identifier: the strict forward map (source → generated)
                        // takes the LARGEST srcCol ≤ query on the declaration
                        // line, so without this boundary every position past the
                        // identifier (the initializer, later statements on the
                        // line) GLBs onto the destructured-alias token, whose
                        // generated extent rejects the delta — hover/definition/
                        // completion anywhere on the declaration line fails
                        // closed. The declaration's own boundary accepts the
                        // whole line's delta and wins (earliest generated
                        // position), keeping the retained script's route. An
                        // invalid offset degrades to the line-level mapping
                        // only — never a wrong position.
                        let _ = ct.try_add_sourcemap_location(start);
                    }
                    let content = ct.alloc_str(&format!("{separator}{name}"));
                    segments.push((
                        anchor,
                        source_start.map(|start| (start, separator.len() as u32)),
                        content,
                    ));
                }
                segments.push((
                    anchor,
                    None,
                    ct.alloc_str(&format!(" }} = {PREFIX}unwrapped;")),
                ));
            };
            push_group(&mut destruct_segments, "const", &const_names, ct);
            if !let_names.is_empty() {
                if !const_names.is_empty() {
                    destruct_segments.push((anchor, None, " "));
                }
                push_group(&mut destruct_segments, "let", &let_names, ct);
            }
            destruct_segments.push((anchor, None, " /* verter-destructured-end */\n"));

            // Keep `___VERTER___unwrapped` itself live when NOTHING is
            // destructured from it: if EVERY setup binding is proven-unused (and
            // thus omitted), the destructure reads nothing and the temp would
            // become a spurious TS6133. `void X;` is a no-op value-read. When at
            // least one live binding is destructured, the destructure already
            // reads the temp, so the guard is unnecessary (and omitted to keep
            // the common-case output unchanged).
            if const_names.is_empty() && let_names.is_empty() {
                destruct_segments.push((
                    anchor,
                    None,
                    ct.alloc_str(&format!("void {PREFIX}unwrapped;\n")),
                ));
            }

            // Emit void(name) for bindings referenced in script body or style
            // v-bind(). This keeps the block-scoped DESTRUCTURED copy alive so
            // TS does not flag it when the binding is used only in the script
            // (not the template) or only in a style `v-bind()`. Reuses the same
            // typed-IR `script_refs` / `style_v_bind_set` computed above for the
            // unwrapped-entry liveness decision — one shared usage inventory.
            // Only LIVE bindings have a destructured copy; a script/style-used
            // binding is always live (used somewhere ⇒ not omitted), so iterating
            // `live_bindings` never voids a name that was dropped. These
            // keep-alive references stay UNMAPPED (they are scaffold liveness,
            // not user references).
            let mut block_copy_used = false;
            for (name, _) in &live_bindings {
                if script_refs.contains(name) || style_v_bind_set.contains(name) {
                    destruct_segments.push((anchor, None, ct.alloc_str(&format!("void({name});"))));
                    block_copy_used = true;
                }
            }
            if block_copy_used {
                destruct_segments.push((anchor, None, "\n"));
            }

            // No template: close the block scope and the binding function right
            // after the destructure block (the template path defers these to the
            // template-end tail).
            if template_end.is_none() {
                destruct_segments.push((anchor, None, "\n} // close block scope\n"));
                destruct_segments.push((
                    anchor,
                    None,
                    "\nreturn {};\n} // close templateBindingFN\n",
                ));
            }

            // The ordered batch preserves insertion order at the shared anchor
            // (the merged plain/mapped channel would reorder plains before
            // mapped at equal positions).
            ct.batch_prepend_left_with_source_map(&destruct_segments);

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
            // Suppress TS6133 for generated variables that may not be referenced in
            // template (matches the `__props` alias above on both paths).
            if define_props_var.is_some() {
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
            // No template: the destructured path already emitted the block-scope
            // and function closes into its ordered segments. The non-destructured
            // path (no setup bindings) emits them here into the wrapper tail.
            if setup_bindings.is_empty() {
                wrapper_end.push_str("\n} // close block scope\n");
                wrapper_end.push_str("\nreturn {};\n} // close templateBindingFN\n");
            }
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

/// Return the real local identifiers declared by destructured defineProps / withDefaults
/// patterns, in source declaration order. Both macro-pattern spans and declaration-name
/// spans are relative to the script content, so containment is an exact typed-AST fact.
fn destructured_define_props_bindings<'a>(items: &'a [ScriptItem<'a>]) -> Vec<&'a str> {
    let pattern_spans: Vec<_> = items
        .iter()
        .filter_map(|item| {
            let declarator = match item {
                ScriptItem::Macro(ScriptMacro::DefineProps { declarator, .. })
                | ScriptItem::Macro(ScriptMacro::WithDefaults { declarator, .. }) => {
                    declarator.as_ref()
                }
                _ => return None,
            }?;
            declarator.name.is_none().then_some(declarator.binding_span)
        })
        .collect();

    let mut seen = FxHashSet::default();
    items
        .iter()
        .filter_map(|item| {
            let ScriptItem::Declaration(declaration) = item else {
                return None;
            };
            let (name, name_span) = (declaration.name?, declaration.name_span?);
            pattern_spans
                .iter()
                .any(|pattern| name_span.start >= pattern.start && name_span.end <= pattern.end)
                .then_some(name)
        })
        .filter(|name| seen.insert(*name))
        .collect()
}
