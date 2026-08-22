//! Script processing: OXC parse + item handling + component wrapping.
//!
//! All transformations use [`CodeGenOutput`] (batch overwrite/prepend).
//! Import hoisting and TS type hoisting are done via the "move" pattern:
//! `overwrite(src, src_end, "")` + `prepend_alloc(target, content)`.
//!
//! Macro processing (`defineProps`, `defineEmits`, etc.) is in [`super::macros`].

use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, ObjectPropertyKind, Program, Statement};
use oxc_span::{GetSpan, SourceType};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::code_transform::CodeTransform;
use crate::cursor::ScriptLanguage;
use crate::parser::types::RootNodeScript;
use crate::script::prepared::{PreparedCompanion, PreparedScript};
use crate::template::code_gen::binding::BindingType;
use crate::utils::oxc::vue::{AsyncKind, ImportSpecifierKind, ScriptImport, ScriptItem};

use super::{push_default_export_statement, push_sfc_binding};

use super::macros::{
    js_string_literal, process_companion_script, process_macro_item, push_runtime_prop_key,
    MacroState, StrippedSections,
};
use super::{ScriptCodeGenOptions, ScriptContext};

/// Official compiler-sfc binding metadata for inline template refs.
/// Named imports (any source) and default imports from non-`vue`,
/// non-component sources are `setup-maybe-ref`. Namespace imports,
/// default imports from a component source, and `vue` imports are
/// `setup-const`. Component-source test uses the language registry.
pub(super) fn is_ref_bindable_import(source: &str, kind: Option<ImportSpecifierKind>) -> bool {
    !(matches!(kind, Some(ImportSpecifierKind::Namespace))
        || (matches!(kind, Some(ImportSpecifierKind::Default))
            && verter_language::LanguageRegistry::global()
                .classify_static(source)
                .static_resolution()
                .is_vue()))
        && source != "vue"
}

/// Determine OXC SourceType from a script block's `lang` attribute.
/// - `lang="tsx"` / `lang="jsx"` → TSX (JSX + TS superset)
/// - `lang="ts"` or no lang → TypeScript (angle-bracket casts like `<string>0` are valid)
/// - `lang="js"` → JavaScript
///
/// Default is TypeScript (not TSX) because Vue's `<script lang="ts">` uses angle-bracket
/// type assertions which conflict with JSX parsing.
pub(super) fn source_type_from_lang(lang: Option<&ScriptLanguage>) -> SourceType {
    match lang {
        Some(ScriptLanguage::TSX) | Some(ScriptLanguage::JSX) => SourceType::tsx(),
        Some(ScriptLanguage::JavaScript) => SourceType::mjs(),
        // TypeScript, Unknown, or None → TS (not TSX, to support angle-bracket casts)
        _ => SourceType::ts(),
    }
}

// ======================== process_script_setup ========================

/// How the runtime component object is wrapped, matching the official
/// `@vue/compiler-sfc` non-inline gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ComponentWrap {
    /// JS with no companion `export default` / `defineOptions`:
    /// plain object literal (`const __sfc__ = { ... }`).
    Plain,
    /// TS (`lang="ts"`/`"tsx"`): `/*@__PURE__*/_defineComponent({ ... })`.
    DefineComponent,
    /// JS with companion `export default` / `defineOptions`:
    /// `/*@__PURE__*/Object.assign({ ...options }, { ...runtime })`.
    ObjectAssign,
}

/// Pick the runtime wrapper for a `<script setup>` component, matching the
/// official `@vue/compiler-sfc` non-inline gate: `isTS` → `_defineComponent`;
/// JS with a companion default export / `defineOptions` → `Object.assign`
/// merge; otherwise a plain object literal (no `_defineComponent` import).
pub(super) fn component_wrap(is_ts: bool, has_options: bool) -> ComponentWrap {
    if is_ts {
        ComponentWrap::DefineComponent
    } else if has_options {
        ComponentWrap::ObjectAssign
    } else {
        ComponentWrap::Plain
    }
}

/// Process a `<script setup>` block (with optional companion `<script>`).
///
/// Parses the script content with OXC, then:
/// 1. Hoists imports to file top (overwrite + prepend = move)
/// 2. Hoists or strips TypeScript type declarations
/// 3. Processes Vue macros (defineProps, defineEmits, etc.)
/// 4. Extracts binding metadata for template codegen
/// 5. Overwrites open tag → component wrapper opening
/// 6. Overwrites close tag → return statement + closing braces + export
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn process_script_setup<'alloc>(
    setup: &RootNodeScript,
    prepared: &PreparedScript<'alloc>,
    ctx: &mut ScriptContext<'alloc>,
    options: &ScriptCodeGenOptions<'_>,
    is_ts: bool,
) {
    if setup.content.is_none() {
        // Self-closing <script setup /> — emit empty component. The companion
        // <script> still needs its codegen (its `export default <expr>` must
        // be rebound to `__default__` and merged — never dropped, and never
        // left as a duplicate default export).
        let mut macro_state = MacroState::new();
        if let Some(companion) = prepared.companion() {
            process_companion_script(companion, &mut ctx.out, &mut macro_state);
        }
        emit_minimal_component(
            setup,
            ctx,
            options,
            is_ts,
            macro_state.has_companion_default,
        );
        return;
    }

    // The setup block (and its companion) were parsed once when the prepared
    // script was built — read the macro surfaces, bindings, and async status
    // from that single parse. A present setup content span guarantees a prepared
    // setup parse.
    let prepared_setup = prepared
        .setup()
        .expect("setup content present implies a prepared setup parse");
    let content_start = prepared_setup.content_start();
    let content_str = prepared_setup.content_str();

    // Hoist insertion point: just before the open tag
    let hoist_pos = setup.tag_open.start;

    // Collect macro state
    let mut macro_state = MacroState::new();

    // Apply the companion <script> codegen FIRST: remove its duplicate default
    // export, lift `export default { ... }` options, and collect non-type import
    // names for template resolution. Its type inventory was already folded into
    // the setup parse, so only the import names flow back here.
    let companion_import_names = match prepared.companion() {
        Some(companion) => process_companion_script(companion, &mut ctx.out, &mut macro_state),
        None => Vec::new(),
    };

    // Merge the companion's ref-bindable imports into the context set
    // (companion imports are in scope at runtime — same official rule).
    if !macro_state.ref_bindable_imports.is_empty() {
        let names: Vec<&str> = macro_state
            .ref_bindable_imports
            .iter()
            .map(|n| ctx.out.alloc_str(n))
            .collect();
        ctx.ref_bindable_imports.extend(names);
    }

    let parse_result = prepared_setup.parse_result();

    // In force_js mode, compute type-stripped script content (sans imports) to
    // determine which import specifiers have runtime references vs type-only usage.
    let runtime_text = if !options.keep_ts_types {
        Some(compute_runtime_text(
            prepared_setup.program(),
            content_str,
            &parse_result.items,
        ))
    } else {
        None
    };

    // In force_js mode, pre-strip every macro-argument expression the synthesized
    // props/emits sections copy verbatim — `defineProps` / `defineEmits` object &
    // array arguments and the `withDefaults` defaults — keyed by content-local
    // span. The macro synthesis reads these so each section is TypeScript-free no
    // matter its shape (`withDefaults`, multi-declarator, object, array); the
    // macro call range is overwritten before the whole-program force-js pass, so
    // that pass can never reach inside the emitted sections.
    let stripped_sections = (!options.keep_ts_types)
        .then(|| collect_stripped_macro_sections(prepared_setup.program(), content_str));

    // Process items. The source-order top-level macro index is the stable DTO
    // join key; nested analyzer rows (notably withDefaults/defineProps) never
    // participate in this compiler inventory.
    let mut macro_syntax_index = 0_u32;
    for item in &parse_result.items {
        match item {
            ScriptItem::Import(imp) => {
                let abs_start = content_start + imp.span.start;
                let abs_end = content_start + imp.span.end;

                // Record official `setup-maybe-ref` import bindings — inline
                // template refs to these names bind `ref_key`/`ref: name`
                // (named/default user imports except vue-source, default
                // `.vue`-source, and namespace imports).
                if !imp.is_type_only {
                    for b in &imp.bindings {
                        if !b.is_type_only && is_ref_bindable_import(imp.source, b.import_kind) {
                            ctx.ref_bindable_imports.insert(ctx.out.alloc_str(b.name));
                        }
                    }
                }

                if !options.keep_ts_types && imp.is_type_only {
                    // Type-only import — strip entirely. process.rs is the SOLE
                    // owner of setup imports (the force_js body strip skips
                    // import declarations), so this is a single overwrite with
                    // no same-range double from the body strip.
                    ctx.out.overwrite(abs_start, abs_end, "");
                } else if !options.keep_ts_types {
                    // force_js mode: reconstruct import keeping only specifiers
                    // that have runtime usage (in script body or template). The
                    // body strip skips imports, so this reconstruct is the only
                    // edit on the span — no nested-overwrite corruption.
                    let kept = filter_import_specifiers(
                        imp,
                        runtime_text.as_deref(),
                        options.template_used_vars.as_ref(),
                    );
                    ctx.out.overwrite(abs_start, abs_end, "");
                    if let Some(reconstructed) = kept {
                        ctx.out.prepend_alloc(hoist_pos, &reconstructed);
                    }
                } else {
                    // Keep TS types mode — hoist verbatim
                    let import_text = &ctx.source[abs_start as usize..abs_end as usize];
                    ctx.out.overwrite(abs_start, abs_end, "");
                    ctx.out
                        .prepend_alloc(hoist_pos, &format!("{}\n", import_text));
                }
            }
            ScriptItem::TypeDeclaration(td) => {
                if options.keep_ts_types {
                    // Hoist to file top
                    let abs_start = content_start + td.span.start;
                    let abs_end = content_start + td.span.end;
                    let td_text = &ctx.source[abs_start as usize..abs_end as usize];
                    ctx.out.overwrite(abs_start, abs_end, "");
                    ctx.out.prepend_alloc(hoist_pos, &format!("{}\n", td_text));
                }
                // force_js: the whole-program body strip is the SINGLE owner of
                // type-declaration removal — interfaces/type aliases are removed
                // and enums lower to their runtime IIFE there. Blanking the same
                // span here too would double-overwrite it and corrupt the
                // CodeTransform (see the type-only-import note above).
            }
            ScriptItem::Macro(mac) => {
                process_macro_item(
                    mac,
                    macro_syntax_index,
                    content_start,
                    content_str,
                    ctx,
                    &mut macro_state,
                    stripped_sections.as_ref(),
                    options.macro_runtime,
                    options.is_production,
                    options.custom_element,
                );
                macro_syntax_index = macro_syntax_index.saturating_add(1);
            }
            // Transform `await <arg>` → _withAsyncContext wrapper.
            // Vue wraps each top-level await to preserve component instance context.
            ScriptItem::Async(async_item) if async_item.kind == AsyncKind::AwaitExpression => {
                if let Some(arg_span) = &async_item.arg_span {
                    let abs_start = content_start + async_item.span.start;
                    let abs_arg_start = content_start + arg_span.start;
                    let abs_arg_end = content_start + arg_span.end;

                    // Wrap `await <arg>` in a parenthesised comma-expression:
                    // (([__temp,__restore] = _withAsyncContext(() => <arg>)),
                    //   __temp = await __temp, __restore(), __temp)
                    // The outer parens are critical — without them a `const x = ...`
                    // initializer would break at the first comma.
                    //
                    // The argument region is left ORIGINAL (only the surrounding
                    // wrapper is overwritten) so the force_js body strip removes
                    // its TypeScript (e.g. `await load<Result>()` → the
                    // `<Result>` type arg). Embedding a raw slice would place the
                    // type args inside an Overwritten chunk the strip cannot
                    // reach (nested-overwrite no-op).
                    ctx.out.overwrite(
                        abs_start,
                        abs_arg_start,
                        "(([__temp,__restore] = _withAsyncContext(() => ",
                    );
                    ctx.out.prepend_alloc(
                        abs_arg_end,
                        ")), __temp = await __temp, __restore(), __temp)",
                    );
                }
            }
            _ => {}
        }
    }

    // Extract bindings from parse result.
    // Binding spans are content-relative (0-based within content_str).
    // Skip `Props` bindings — those are extracted directly during macro processing
    // via `extract_individual_props_from_expr` and have inconsistent span coordinate
    // systems (object-syntax keys are SFC-absolute, array-syntax keys are content-relative).
    // Allow `PropsAliased` through — those come from `extract_destructured_props` with
    // consistent content-relative spans, and are needed for destructured defineProps
    // (especially when the type parameter is unresolvable, e.g., an imported type).
    for (span, bt) in &parse_result.bindings {
        if *bt == BindingType::Props {
            continue;
        }
        let name = &content_str[span.start as usize..span.end as usize];
        if ctx.bindings.insert(name, *bt).is_none() {
            ctx.binding_order.push(name);
        }
        // A declaration's own identifier is a required `verbatim-carry`
        // source-map anchor: a top-level `const count = ref(0)` must map a
        // segment to `count`'s OWN position, not merely to its LINE's
        // start (confirmed against the mapping oracle). The identifier
        // text stays verbatim passthrough in the vast majority of cases;
        // when it doesn't (e.g. this binding's declaration was itself
        // overwritten elsewhere), the registered offset simply lands
        // outside any surviving `Original` chunk and is never consulted —
        // a harmless no-op, not a wrong mapping.
        ctx.out.add_sourcemap_location(content_start + span.start);
    }

    // Add companion script import names as SetupImport bindings.
    // Imports in the companion <script> block are available to the template at
    // runtime because the component factory merges both script blocks.
    // `build_returned_object` includes a SetupImport iff it is genuinely
    // runtime-used (script body or template) — see that function's doc
    // comment for the full unconditional-inclusion rule and its disclosed
    // companion-script-body-usage gap. Type-only imports (e.g., CurrencyCodes
    // used only as `"EUR" as CurrencyCodes`) never reach here as a runtime
    // binding in the first place — official excludes those too.
    for name in &companion_import_names {
        // Skip if setup script already declares the same name (setup takes precedence)
        let alloc_name = ctx.out.alloc_str(name);
        if let std::collections::hash_map::Entry::Vacant(entry) = ctx.bindings.entry(alloc_name) {
            entry.insert(BindingType::SetupImport);
            ctx.binding_order.push(alloc_name);
        }
    }

    // Inject _useCssVars if CSS v-bind vars are present
    if !options.css_v_binds.is_empty() {
        super::css_vars::inject_use_css_vars(
            options.css_v_binds,
            &ctx.bindings,
            content_start, // Insert at start of setup body
            &mut ctx.out,
            &mut ctx.imports,
        );
    }

    // Merge defineModel declarations into props/emits sections.
    // Each defineModel('name') needs:
    //   props: { name: {}, nameModifiers: {} }
    //   emits: ['update:name']
    //
    // Vue's official compiler uses _mergeModels() to combine props/emits from
    // defineProps/defineEmits with those from defineModel. This avoids brittle
    // string-level insertion (rfind) that breaks on non-object-literal props
    // sections (e.g., IIFE from withDefaults with runtime variable).
    if !macro_state.models.is_empty() {
        // Build model props from the authoritative semantic rows, merging only
        // the compiler-owned options expression captured from syntax.
        let mut model_props_obj = String::from("{\n");
        for (i, model) in macro_state.models.iter().enumerate() {
            if i > 0 {
                model_props_obj.push_str(",\n");
            }
            model_props_obj.push_str("    ");
            push_runtime_prop_key(&mut model_props_obj, &model.prop_name);
            model_props_obj.push_str(": ");
            model_props_obj.push_str(&model.prop_options);
            model_props_obj.push_str(",\n    ");
            push_runtime_prop_key(&mut model_props_obj, &model.modifiers_name);
            model_props_obj.push_str(": {}");
        }
        model_props_obj.push_str("\n  }");

        // Merge into existing props section using _mergeModels, or create new one
        match &mut macro_state.props_section {
            Some(existing) => {
                // Wrap: _mergeModels(existingProps, { modelProps })
                *existing = format!(
                    "/*@__PURE__*/_mergeModels({}, {})",
                    existing, model_props_obj
                );
                ctx.imports.push("_mergeModels");
            }
            None => {
                macro_state.props_section = Some(model_props_obj);
            }
        }

        // Build model emit entries
        let model_emits: Vec<String> = macro_state
            .models
            .iter()
            .map(|model| js_string_literal(&model.update_event))
            .collect();
        let model_emits_arr = format!("[{}]", model_emits.join(", "));

        // Merge into existing emits section using _mergeModels, or create new one
        match &mut macro_state.emits_section {
            Some(existing) => {
                *existing = format!(
                    "/*@__PURE__*/_mergeModels({}, {})",
                    existing, model_emits_arr
                );
                ctx.imports.push("_mergeModels");
            }
            None => {
                macro_state.emits_section = Some(model_emits_arr);
            }
        }
    }

    // Inject async context variables when setup has top-level await.
    // Vue's _withAsyncContext pattern requires __temp and __restore locals.
    if parse_result.is_async {
        ctx.out
            .prepend_alloc(content_start, "let __temp, __restore\n");
        ctx.imports.push("_withAsyncContext");
    }

    // Build wrapper opening (includes __name, props, emits, options sections).
    // The wrapper shape follows the official non-inline gate: TS components
    // keep `_defineComponent`; JS components are plain object literals unless
    // a companion `export default` / `defineOptions` forces the
    // `Object.assign` merge path.
    let wrap = component_wrap(
        is_ts,
        macro_state.has_companion_default || macro_state.options_expr.is_some(),
    );
    // Official inline `buildDestructureElements`: inject `attrs: $attrs` /
    // `slots: $slots` into the setup context destructure WHEN the template
    // uses them (on-use), so inline template references resolve to the
    // destructured bindings instead of `_ctx.*`.
    let (uses_attrs, uses_slots) = if options.inline_template {
        match options.template_used_vars.as_ref() {
            Some(vars) => (vars.contains("$attrs"), vars.contains("$slots")),
            None => (false, false),
        }
    } else {
        (false, false)
    };
    // Official `buildDestructureElements`: `expose: __expose` is destructured
    // whenever `defineExpose()` was authored OR the template is non-inline
    // (`ctx.hasDefineExposeCall || !inlineMode`) — non-inline setup always
    // needs a real `__expose` to hand the instance, even with no authored
    // call. When no `defineExpose()` was authored AND non-inline, official
    // also emits a bare `__expose();` statement at the top of the setup body
    // so an un-exposed non-inline component still gets its (empty) public
    // surface locked in (`!ctx.hasDefineExposeCall && !inlineMode`).
    let bind_expose = macro_state.has_expose || !options.inline_template;
    let emit_bare_expose_call = !macro_state.has_expose && !options.inline_template;
    let (mut wrapper_start, mut wrapper_start_binding_range) = build_setup_wrapper_start(
        options.component_name,
        parse_result.is_async,
        bind_expose,
        emit_bare_expose_call,
        macro_state.has_emit,
        macro_state.props_section.as_deref(),
        macro_state.emits_section.as_deref(),
        macro_state.options_expr.as_deref(),
        macro_state.has_companion_default,
        uses_attrs,
        uses_slots,
        wrap,
        options.is_vapor,
        options.ssr,
    );
    if prepared.companion().is_some() {
        // The setup open tag can immediately follow companion-script content.
        // Start the inserted wrapper on a new statement boundary even when the
        // carrier supplied no whitespace between the two script blocks.
        wrapper_start.insert(0, '\n');
        // The insert shifts every later byte offset by exactly one ('\n' is
        // one UTF-8 byte) — the declared range must track the same shift, or
        // it would drift by one byte from the binding it actually names.
        wrapper_start_binding_range.start += 1;
        wrapper_start_binding_range.end += 1;
    }

    // Overwrite open tag with wrapper
    let wrapper_start_content = ctx.out.alloc_str(&wrapper_start);
    ctx.out.overwrite_or_root_prefix_alloc(
        setup.tag_open.start,
        setup.tag_open.end,
        wrapper_start_content,
    );
    ctx.out.record_sfc_export_fact(
        wrapper_start_content,
        vec![wrapper_start_binding_range],
        None,
    );

    // Build wrapper closing
    let returned = if !options.inline_template {
        Some(build_returned_object(
            &ctx.bindings,
            &ctx.binding_order,
            options.template_used_vars.as_ref(),
            runtime_text.as_deref(),
        ))
    } else {
        None
    };

    let (wrapper_end, wrapper_end_binding_ranges, wrapper_end_export_range) =
        build_setup_wrapper_end(
            returned.as_deref(),
            if options.has_scoped_style {
                Some(options.scope_id)
            } else {
                None
            },
            wrap,
        );

    // Handle close tag
    if let Some(tag_close) = &setup.tag_close {
        let wrapper_end_content = ctx.out.alloc_str(&wrapper_end);
        ctx.out
            .overwrite_or_root_suffix_alloc(tag_close.start, tag_close.end, wrapper_end_content);
        ctx.out.record_sfc_export_fact(
            wrapper_end_content,
            wrapper_end_binding_ranges,
            Some(wrapper_end_export_range),
        );

        // Set inline inject position
        if options.inline_template {
            ctx.inline_inject_pos = Some(tag_close.start);
        }
    }

    // Track the _defineComponent import only when the wrapper emits the call
    // (TS components). Plain-object and Object.assign shapes need no helper.
    // A non-SSR Vapor TS component routes through the separate
    // `defineVaporComponent` runtime wrapper instead (see
    // `build_setup_wrapper_start`'s matching gate) — its runtime
    // implementation sets `.__vapor = true` itself, so no `_defineComponent`
    // import is tracked for that branch.
    if wrap == ComponentWrap::DefineComponent {
        if options.is_vapor && !options.ssr {
            ctx.imports.push("_defineVaporComponent");
        } else {
            ctx.imports.push("_defineComponent");
        }
    }
}

// ======================== process_script_only ========================

/// Process a standalone `<script>` block (Options API, no setup).
///
/// Handles:
/// 1. Removing `<script>` and `</script>` tags
/// 2. Converting `export default { ... }` to `const __sfc__ = { ... }`
/// 3. Appending `export default __sfc__`
pub fn process_script_only<'alloc>(
    script: &RootNodeScript,
    prepared_companion: Option<&PreparedCompanion<'alloc>>,
    ctx: &mut ScriptContext<'alloc>,
    options: &ScriptCodeGenOptions<'_>,
) {
    // The Options-API standalone `<script>` was parsed once into the prepared
    // companion; a present companion mirrors a present content span.
    let Some(prepared_companion) = prepared_companion else {
        return;
    };
    let content_start = prepared_companion.content_start();
    let content_str = prepared_companion.content_str();
    let parse_result = prepared_companion.parse_result();

    // Extract Options API bindings (data, props, computed, methods, inject)
    // so the template codegen can use the correct accessor prefix ($data., $props., _ctx.).
    for (span, bt) in &parse_result.bindings {
        let name = &content_str[span.start as usize..span.end as usize];
        ctx.bindings.insert(name, *bt);
    }

    // Find default export and replace it.
    // Regular <script> content is passed through as-is to the bundler
    // (with only the export default tweak). Import elision and TS stripping
    // are handled by the downstream toolchain (e.g., Vite's esbuild plugin).
    let mut has_default_export = false;
    for item in &parse_result.items {
        if let ScriptItem::DefaultExport(de) = item {
            has_default_export = true;
            let abs_start = content_start + de.span.start;
            // Replace "export default" with "const __sfc__ ="
            let export_default_text = "export default";
            let replace_end = abs_start + export_default_text.len() as u32;
            let mut replacement = String::with_capacity(16);
            replacement.push_str("const ");
            let binding_range = push_sfc_binding(&mut replacement);
            replacement.push_str(" =");
            let content = ctx.out.alloc_str(&replacement);
            ctx.out.overwrite_alloc(abs_start, replace_end, content);
            ctx.out
                .record_sfc_export_fact(content, vec![binding_range], None);
        }
    }

    // Remove open tag
    ctx.out
        .overwrite(script.tag_open.start, script.tag_open.end, "");

    // Build close tag replacement
    let mut close_text = String::with_capacity(64);
    let mut binding_ranges = Vec::with_capacity(3);
    // The authored default-export expression may end immediately before the
    // closing tag. Keep the generated module export in a fresh statement even
    // when the carrier supplied no trailing whitespace or semicolon.
    close_text.push('\n');
    if !has_default_export {
        // No default export — create a minimal __sfc__
        close_text.push_str("const ");
        binding_ranges.push(push_sfc_binding(&mut close_text));
        close_text.push_str(" = {};\n");
    }
    if options.is_vapor {
        binding_ranges.push(push_sfc_binding(&mut close_text));
        close_text.push_str(".__vapor = true;\n");
    }
    // Non-inline SSR attaches `ssrRender` separately (not returned from setup),
    // so do not claim `__ssrInlineRender`.
    if options.has_scoped_style && !options.scope_id.is_empty() {
        binding_ranges.push(push_sfc_binding(&mut close_text));
        close_text.push_str(".__scopeId = \"");
        close_text.push_str(options.scope_id);
        close_text.push_str("\";\n");
    }
    let (export_binding_range, export_statement_range) =
        push_default_export_statement(&mut close_text);
    binding_ranges.push(export_binding_range);

    if let Some(tag_close) = &script.tag_close {
        let content = ctx.out.alloc_str(&close_text);
        ctx.out
            .overwrite_or_root_suffix_alloc(tag_close.start, tag_close.end, content);
        ctx.out
            .record_sfc_export_fact(content, binding_ranges, Some(export_statement_range));
    }
}

// ======================== Helpers ========================

/// Emit a minimal component definition for empty/self-closing script setup.
fn emit_minimal_component(
    setup: &RootNodeScript,
    ctx: &mut ScriptContext<'_>,
    options: &ScriptCodeGenOptions<'_>,
    is_ts: bool,
    has_companion_default: bool,
) {
    let mut s = String::with_capacity(160);
    let mut binding_ranges = Vec::with_capacity(3);
    // Official gate: TS keeps `_defineComponent` (spreading `__default__`);
    // JS emits a plain object — or the `Object.assign(__default__, …)` merge
    // when a companion default exists.
    if is_ts {
        s.push_str("const ");
        binding_ranges.push(push_sfc_binding(&mut s));
        s.push_str(" = /*@__PURE__*/_defineComponent({\n");
        if has_companion_default {
            s.push_str("  ...__default__,\n");
        }
    } else if has_companion_default {
        s.push_str("const ");
        binding_ranges.push(push_sfc_binding(&mut s));
        s.push_str(" = /*@__PURE__*/Object.assign(__default__, {\n");
    } else {
        s.push_str("const ");
        binding_ranges.push(push_sfc_binding(&mut s));
        s.push_str(" = {\n");
    }
    if !options.component_name.is_empty() {
        s.push_str("  __name: '");
        s.push_str(options.component_name);
        s.push_str("',\n");
    }
    if is_ts || has_companion_default {
        s.push_str("});\n");
    } else {
        s.push_str("};\n");
    }
    if options.is_vapor {
        binding_ranges.push(push_sfc_binding(&mut s));
        s.push_str(".__vapor = true;\n");
    }
    // Non-inline SSR attaches `ssrRender` separately — no `__ssrInlineRender`.
    if options.has_scoped_style && !options.scope_id.is_empty() {
        binding_ranges.push(push_sfc_binding(&mut s));
        s.push_str(".__scopeId = \"");
        s.push_str(options.scope_id);
        s.push_str("\";\n");
    }
    let (export_binding_range, export_statement_range) = push_default_export_statement(&mut s);
    binding_ranges.push(export_binding_range);

    let end = setup
        .tag_close
        .as_ref()
        .map(|t| t.end)
        .unwrap_or(setup.tag_open.end);
    let content = ctx.out.alloc_str(&s);
    ctx.out.overwrite_alloc(setup.tag_open.start, end, content);
    ctx.out
        .record_sfc_export_fact(content, binding_ranges, Some(export_statement_range));

    if is_ts {
        ctx.imports.push("_defineComponent");
    }
}

/// Build the opening part of the setup wrapper with sections.
///
/// TS (`ComponentWrap::DefineComponent`) — official spreads the companion
/// default (`...__default__`) and `defineOptions` (`...<expr>`) before the
/// runtime options:
/// ```js
/// const __sfc__ = /*@__PURE__*/_defineComponent({
///   ...__default__,                  // from companion <script> export default
///   ...{ inheritAttrs: false },      // from defineOptions
///   __name: 'ComponentName',
///   props: { title: String },        // from defineProps
///   emits: ['click'],                // from defineEmits
///   setup(__props, { expose: __expose, emit: __emit }) {
/// ```
///
/// JS without options (`ComponentWrap::Plain`) — official emits a plain
/// object literal with no `_defineComponent` call or import:
/// ```js
/// const __sfc__ = {
///   __name: 'ComponentName',
///   setup(__props) {
/// ```
///
/// JS with companion default / defineOptions (`ComponentWrap::ObjectAssign`)
/// — official merges via `Object.assign` (official order: `__default__`,
/// `<definedOptions>`, runtime object):
/// ```js
/// const __sfc__ = /*@__PURE__*/Object.assign(__default__, { inheritAttrs: false }, {
///   __name: 'ComponentName',
///   setup(__props) {
/// ```
///
/// TS (`ComponentWrap::DefineComponent`) — official spreads both option
/// sources inside `_defineComponent`:
/// ```js
/// const __sfc__ = /*@__PURE__*/_defineComponent({
///   ...__default__,
///   ...{ inheritAttrs: false },
///   __name: 'ComponentName',
///   setup(__props) {
/// ```
#[allow(clippy::too_many_arguments)]
fn build_setup_wrapper_start(
    component_name: &str,
    is_async: bool,
    bind_expose: bool,
    emit_bare_expose_call: bool,
    has_emit: bool,
    props_section: Option<&str>,
    emits_section: Option<&str>,
    options_expr: Option<&str>,
    has_companion_default: bool,
    uses_attrs: bool,
    uses_slots: bool,
    wrap: ComponentWrap,
    is_vapor: bool,
    ssr: bool,
) -> (String, std::ops::Range<u32>) {
    let mut s = String::with_capacity(256);
    s.push_str("const ");
    let binding_range = push_sfc_binding(&mut s);
    match wrap {
        ComponentWrap::DefineComponent => {
            // Non-SSR Vapor routes through the dedicated `defineVaporComponent`
            // runtime wrapper instead of `defineComponent` (verified directly
            // against `@vue/compiler-sfc` 3.6.0-rc.5: `vapor && !ssr ?
            // defineVaporComponent : defineComponent`) — its own
            // implementation sets `.__vapor = true`, so `emits_vapor_flag_here`
            // below stays false for this branch.
            if is_vapor && !ssr {
                s.push_str(" = /*@__PURE__*/_defineVaporComponent({\n");
            } else {
                s.push_str(" = /*@__PURE__*/_defineComponent({\n");
            }
            // Official spreads the companion default and defineOptions, in
            // order, before the runtime options.
            if has_companion_default {
                s.push_str("  ...__default__,\n");
            }
            if let Some(expr) = options_expr {
                s.push_str("  ...");
                s.push_str(expr);
                s.push_str(",\n");
            }
        }
        ComponentWrap::Plain => {
            s.push_str(" = {\n");
        }
        ComponentWrap::ObjectAssign => {
            // Official merge targets, in order: `__default__` (companion),
            // the raw defineOptions expression, then the runtime object.
            s.push_str(" = /*@__PURE__*/Object.assign(");
            let mut first = true;
            if has_companion_default {
                s.push_str("__default__");
                first = false;
            }
            if let Some(expr) = options_expr {
                if !first {
                    s.push_str(", ");
                }
                s.push_str(expr);
            }
            s.push_str(", {\n");
        }
    }

    if !component_name.is_empty() {
        s.push_str("  __name: '");
        s.push_str(component_name);
        s.push_str("',\n");
    }

    // Props section
    if let Some(props) = props_section {
        s.push_str("  props: ");
        s.push_str(props);
        s.push_str(",\n");
    }

    // Emits section
    if let Some(emits) = emits_section {
        s.push_str("  emits: ");
        s.push_str(emits);
        s.push_str(",\n");
    }

    // `__vapor: true` — official's non-TS `compileScript` branch adds this
    // to the SAME accumulated `runtimeOptions` string as `__name`/`props`/
    // `emits` (unconditional on `ssr`), spliced into the object literal as
    // ONE inline property — never a separate trailing
    // `__sfc__.__vapor = true` assignment (confirmed directly against the
    // vendored rc.5 compiler source and the pinned rc.5 golden for
    // `basic-interpolation.vue`'s vapor cell). The TS `_defineComponent`
    // branch instead adds it only when `ssr && vapor`: a non-SSR TS Vapor
    // component routes through the SEPARATE `defineVaporComponent` runtime
    // wrapper instead of a `_defineComponent`-annotated object, and that
    // wrapper is not threaded through `wrap`/`ComponentWrap` here — a
    // non-SSR TS `<script setup>` Vapor component therefore emits no
    // `__vapor` flag on this path today.
    let emits_vapor_flag_here = match wrap {
        ComponentWrap::DefineComponent => is_vapor && ssr,
        ComponentWrap::Plain | ComponentWrap::ObjectAssign => is_vapor,
    };
    if emits_vapor_flag_here {
        s.push_str("  __vapor: true,\n");
    }

    // Setup function signature
    if is_async {
        s.push_str("  async setup(__props");
    } else {
        s.push_str("  setup(__props");
    }

    // Add destructured context if needed. Official order: expose, emit
    // (`emit: __emit` is pushed before buildDestructureElements), attrs,
    // slots (attrs/slots only for inline template mode, on-use).
    if bind_expose || has_emit || uses_attrs || uses_slots {
        s.push_str(", { ");
        let mut first = true;
        if bind_expose {
            s.push_str("expose: __expose");
            first = false;
        }
        if has_emit {
            if !first {
                s.push_str(", ");
            }
            s.push_str("emit: __emit");
            first = false;
        }
        if uses_attrs {
            if !first {
                s.push_str(", ");
            }
            s.push_str("attrs: $attrs");
            first = false;
        }
        if uses_slots {
            if !first {
                s.push_str(", ");
            }
            s.push_str("slots: $slots");
        }
        s.push_str(" }");
    }

    s.push_str(") {\n");
    // Official: a non-inline setup with no authored `defineExpose()` still
    // gets a bare `__expose();` at the top of the body — the destructured
    // `expose: __expose` above is otherwise never invoked, and the instance's
    // public exposed surface would stay uninitialized instead of locked to
    // empty.
    if emit_bare_expose_call {
        s.push_str("  __expose();\n\n");
    }
    (s, binding_range)
}

/// Build the closing part of the setup wrapper.
///
/// ```js
///   return { msg, count }
/// }});                    // _defineComponent / Object.assign shapes
/// }};                     // plain-object shape
/// __sfc__.__scopeId = "data-v-xxx";
/// export default __sfc__;
/// ```
fn build_setup_wrapper_end(
    returned: Option<&str>,
    scope_id: Option<&str>,
    wrap: ComponentWrap,
) -> (String, Vec<std::ops::Range<u32>>, std::ops::Range<u32>) {
    let mut s = String::with_capacity(128);
    let mut binding_ranges = Vec::with_capacity(2);
    if let Some(ret) = returned {
        // Matches Vue's official compiler unconditionally — assign returned
        // bindings and mark with `__isScriptSetup` so @vue/test-utils/devtools
        // can identify script-setup components. Confirmed directly against
        // the real `@vue/compiler-sfc` (`compileScript({ssr: true})` and
        // `{ssr: false}` produce byte-identical script output for this tail)
        // and the pinned rc.5 SSR goldens: the marker is present in BOTH.
        // Verter's SSR `ssrRender` uses official's real non-inline 8-param
        // signature with `$setup.*` member routing (never a free `_ctx.*`
        // alias for setup bindings), so the marker's presence never makes a
        // binding unreachable: `hasSetupBinding` skipping
        // `__isScriptSetup`-marked state would only hide a binding from a
        // free `_ctx.*` proxy, and this compiler never routes setup
        // bindings through one.
        s.push_str("\nconst __returned__ = ");
        s.push_str(ret);
        s.push_str(";\nObject.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true });\nreturn __returned__;\n");
    }
    match wrap {
        ComponentWrap::Plain => s.push_str("\n}};\n"),
        ComponentWrap::DefineComponent | ComponentWrap::ObjectAssign => s.push_str("\n}});\n"),
    }
    if let Some(id) = scope_id {
        binding_ranges.push(push_sfc_binding(&mut s));
        s.push_str(".__scopeId = \"");
        s.push_str(id);
        s.push_str("\";\n");
    }
    let (export_binding_range, export_statement_range) = push_default_export_statement(&mut s);
    binding_ranges.push(export_binding_range);
    (s, binding_ranges, export_statement_range)
}

/// Build the `__returned__` object from bindings.
///
/// Includes every setup-type binding (not props, data, or options) that is
/// NOT a `SetupImport`, unconditionally — official's non-inline
/// `genSetupReturn` (`compiler-sfc.cjs.js`) builds `allBindings` as
/// `{ ...scriptBindings, ...setupBindings }` and returns every key with no
/// template-usage filter. Filtering by template usage here would silently
/// drop a live script-only binding from `__returned__`, breaking any
/// `@vue/test-utils`/devtools consumer that reads it off the setup proxy —
/// so this stays unconditional, matching official exactly.
///
/// A `SetupImport` is included iff it is genuinely RUNTIME-USED (script body
/// or template) — the SAME predicate `filter_import_specifiers` uses to
/// decide whether the specifier survives in the import statement at all
/// (`is_specifier_runtime_used`). This is narrower than "unconditional" but
/// still matches official for every case the seed matrix exercises: official
/// never emits a `__returned__` reference to a name Verter has already
/// elided from its own import statement — doing so would be a hard
/// `ReferenceError`, not a cosmetic divergence, since (unlike official)
/// Verter's `filter_import_specifiers` genuinely drops a specifier with zero
/// runtime references anywhere (see that function's own doc comment) rather
/// than relying on the bundler to tree-shake it later.
///
/// Standing limitation: `runtime_text` here is the `<script setup>` block's
/// own stripped body (`compute_runtime_text`, computed in
/// `process_script_setup`), not a COMPANION `<script>` block's body. A
/// companion import used only inside the companion script's own body (e.g.
/// `export default defineComponent({})`) is therefore not detected as
/// runtime-used by this check and stays excluded, even though official
/// would include it (companion `scriptBindings` are spread into
/// `allBindings` unconditionally, same as setup bindings). Closing this
/// fully needs the companion script's own runtime text threaded in here too.
///
/// Official additionally emits a non-`vue`/non-`.vue`-sourced import as a
/// `get x() { return x }` getter (preserving live-binding semantics for an
/// external reactive re-export) rather than a plain shorthand property; that
/// branch is NOT implemented here — this function has no import SOURCE data
/// plumbed to it, so a `vue`-sourced import, a `.vue`-sourced import, or a
/// non-import setup binding all take official's own plain-shorthand arm
/// unconditionally, and a non-`vue`, non-`.vue`-sourced import would too
/// (incorrectly) rather than the getter form official emits for it.
/// Type-only imports stay excluded (never enter `bindings` as a runtime
/// `SetupImport` in the first place — official excludes those too). Returns
/// a JS object literal like `{ msg, count }`.
fn build_returned_object(
    bindings: &FxHashMap<&str, BindingType>,
    binding_order: &[&str],
    template_used_vars: Option<&FxHashSet<String>>,
    runtime_text: Option<&str>,
) -> String {
    // SOURCE-DECLARATION order, not alphabetical — but NOT raw textual
    // position either. Official's `genSetupReturn` builds `allBindings` as
    // `{ ...scriptBindings, ...setupBindings }` (LOCAL declarations only,
    // JS object insertion order = declaration order) and ONLY THEN merges in
    // `ctx.userImports` entries via a separate `for...in` loop that adds a
    // key iff it is not already present — so a used IMPORT always sorts
    // AFTER every local declaration, regardless of where the `import`
    // statement sits textually (almost always at the top of the file).
    // Proven against the exact rc.5 `basic-interpolation.vue` seed
    // fixture: `import { ref } from "vue"` precedes `const count = ref(0)`
    // textually, yet the golden `__returned__` is `{ count, items, ref }` —
    // the two local `const`s first, the import last. `props-emit.vue`'s
    // golden (`{ props, emit, onClick }`, no imports at all) is consistent
    // with either reading, which is why the import-ordering half of this
    // rule needed the basic-interpolation.vue cross-check to surface.
    //
    // `binding_order` is `bindings`' keys in first-seen TEXTUAL order
    // (`bindings` itself, an `FxHashMap`, cannot recover any order on its
    // own); this function re-partitions it into non-import declarations
    // first, then imports, each partition keeping its own relative order.
    let mut declared: Vec<&str> = Vec::new();
    let mut imported: Vec<&str> = Vec::new();
    for name in binding_order {
        let Some(bt) = bindings.get(name) else {
            continue;
        };
        if !bt.is_setup() {
            continue;
        }
        if *bt == BindingType::SetupImport {
            if is_specifier_runtime_used(name, runtime_text, template_used_vars) {
                imported.push(name);
            }
        } else {
            declared.push(name);
        }
    }
    declared.extend(imported);
    let names = declared;

    if names.is_empty() {
        return "{}".to_string();
    }

    let mut s = String::with_capacity(names.len() * 8 + 4);
    s.push_str("{ ");
    for (i, name) in names.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        s.push_str(name);
    }
    s.push_str(" }");
    s
}

/// Check whether `ident` appears as a whole-word in `text`.
///
/// Uses `memchr::memmem` for fast substring search, then verifies word boundaries.
/// Used for checking if an identifier has runtime references in type-stripped
/// script content (where type annotations have already been removed).
fn is_identifier_used_in_text(ident: &str, text: &str) -> bool {
    let finder = memchr::memmem::Finder::new(ident.as_bytes());
    let text_bytes = text.as_bytes();
    let ident_len = ident.len();

    let mut start = 0;
    while let Some(pos) = finder.find(&text_bytes[start..]) {
        let abs_pos = start + pos;
        let end_pos = abs_pos + ident_len;

        // Check left boundary: must not be preceded by [a-zA-Z0-9_$]
        let left_ok = abs_pos == 0 || {
            let c = text_bytes[abs_pos - 1];
            !c.is_ascii_alphanumeric() && c != b'_' && c != b'$'
        };

        // Check right boundary: must not be followed by [a-zA-Z0-9_$]
        let right_ok = end_pos >= text_bytes.len() || {
            let c = text_bytes[end_pos];
            !c.is_ascii_alphanumeric() && c != b'_' && c != b'$'
        };

        if left_ok && right_ok {
            return true;
        }

        start = abs_pos + 1;
    }
    false
}

/// Compute type-stripped script content with import lines blanked out.
///
/// Used to determine which import specifiers have runtime references (appear in
/// the script body after type annotations are removed) vs type-only usage.
/// Strips from the already-parsed setup `program` rather than re-parsing the
/// content. The blanked string is a throwaway used only for identifier-presence
/// scanning — it is never emitted, so it carries no source map.
///
/// Import ranges MUST be blanked on the original content *before* type stripping
/// mutates lengths. Blanking after strip reuses original spans on a shorter
/// string and overwrites the wrong body bytes (dropping real runtime uses of
/// `ref` / similar, which then get elided as "type-only" and leave the setup
/// body with unstripped TypeScript).
fn compute_runtime_text(program: &Program, content_str: &str, items: &[ScriptItem]) -> String {
    let alloc = Allocator::new();
    let mut ct = CodeTransform::new(content_str, &alloc);
    // Blank import statement regions first (original spans, pre-length-shift)
    // so specifier names on the import line itself don't false-positive.
    for item in items {
        if let ScriptItem::Import(imp) = item {
            let len = (imp.span.end - imp.span.start) as usize;
            if len > 0 {
                // Spaces preserve approximate layout for the throwaway scanner.
                ct.overwrite(imp.span.start, imp.span.end, &" ".repeat(len));
            }
        }
    }
    // Then strip TypeScript using the original program spans.
    crate::strip_types::typescript::strip_typescript_types(program, &mut ct, 0, content_str);
    ct.build_string()
}

/// Filter import specifiers, keeping only those with runtime usage.
///
/// Returns `Some(reconstructed_import)` if any specifiers survive, `None` otherwise.
/// Checks each specifier against `runtime_text` (type-stripped script body) and
/// `template_used_vars` (AST-based identifier set from template expressions).
fn filter_import_specifiers(
    imp: &ScriptImport,
    runtime_text: Option<&str>,
    template_used_vars: Option<&FxHashSet<String>>,
) -> Option<String> {
    let mut default_name: Option<&str> = None;
    let mut namespace_name: Option<&str> = None;
    // Named: (imported_export_name, local_name, imported_is_string_literal)
    let mut named: Vec<(&str, &str, bool)> = Vec::new();

    for b in &imp.bindings {
        if b.is_type_only {
            continue;
        }

        // Check if specifier is used at runtime (local binding name)
        let is_runtime_used = is_specifier_runtime_used(b.name, runtime_text, template_used_vars);

        if !is_runtime_used {
            continue;
        }

        match b.import_kind {
            Some(ImportSpecifierKind::Default) => default_name = Some(b.name),
            Some(ImportSpecifierKind::Namespace) => namespace_name = Some(b.name),
            Some(ImportSpecifierKind::Named) | None => {
                // Preserve `import { FixedSizeList as ElFixedSizeList }` — using
                // only the local name rewrites the export to `ElFixedSizeList`,
                // which does not exist on the module (element-plus virtual-list).
                let imported = b.imported.unwrap_or(b.name);
                named.push((imported, b.name, b.imported_is_string_literal));
            }
        }
    }

    // Nothing survived → drop entire import
    if default_name.is_none() && namespace_name.is_none() && named.is_empty() {
        return None;
    }

    // Reconstruct import statement
    let mut s = String::with_capacity(64);
    s.push_str("import ");

    if let Some(ns) = namespace_name {
        s.push_str("* as ");
        s.push_str(ns);
    } else {
        if let Some(def) = default_name {
            s.push_str(def);
            if !named.is_empty() {
                s.push_str(", ");
            }
        }
        if !named.is_empty() {
            s.push_str("{ ");
            for (i, (imported, local, is_str_lit)) in named.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                if *is_str_lit {
                    s.push('"');
                    s.push_str(imported);
                    s.push('"');
                } else {
                    s.push_str(imported);
                }
                if *imported != *local || *is_str_lit {
                    s.push_str(" as ");
                    s.push_str(local);
                }
            }
            s.push_str(" }");
        }
    }

    s.push_str(" from '");
    s.push_str(imp.source);
    s.push_str("'\n");

    Some(s)
}

/// Check whether an import specifier has runtime usage (in script body or template).
fn is_specifier_runtime_used(
    name: &str,
    runtime_text: Option<&str>,
    template_used_vars: Option<&FxHashSet<String>>,
) -> bool {
    // Check template identifier set (O(1) lookup)
    if let Some(vars) = template_used_vars {
        if vars.contains(name) {
            return true;
        }
    }
    // Check type-stripped script body
    if let Some(rt) = runtime_text {
        if is_identifier_used_in_text(name, rt) {
            return true;
        }
    }
    // If neither runtime_text nor template_used_vars is available, be conservative
    runtime_text.is_none() && template_used_vars.is_none()
}

/// Pre-strip every force-js-eligible macro-argument expression in the setup
/// program to plain JavaScript, keyed by its content-local `(start, end)` span.
///
/// The synthesized props/emits sections copy these expressions verbatim, and the
/// macro call range is overwritten before the whole-program force-js pass runs,
/// so the section text is stripped here — at the point it is produced. Scanning
/// the top-level statements (standalone calls and every `const x = …, y = …`
/// declarator) keys the strip to the actual macro section, so it is robust to
/// `withDefaults`, multi-declarator statements, and object/array shapes alike.
/// A shallow scan over the existing parse — never a re-parse.
fn collect_stripped_macro_sections<'a>(
    program: &Program<'a>,
    content_str: &str,
) -> StrippedSections {
    let alloc = Allocator::new();
    let mut sections = StrippedSections::default();
    for stmt in &program.body {
        match stmt {
            Statement::ExpressionStatement(es) => {
                collect_call_section(&es.expression, content_str, &alloc, &mut sections);
            }
            Statement::VariableDeclaration(decl) => {
                for d in &decl.declarations {
                    if let Some(init) = &d.init {
                        collect_call_section(init, content_str, &alloc, &mut sections);
                    }
                }
            }
            _ => {}
        }
    }
    sections
}

/// Strip the force-js-eligible argument expressions of a single top-level macro
/// call into `sections`. `defineProps` / `defineEmits` copy their first runtime
/// argument (object or array literal) verbatim; `withDefaults` copies its
/// defaults — each object-literal value individually, or any other defaults
/// expression whole.
fn collect_call_section<'a>(
    expr: &Expression<'a>,
    content_str: &str,
    alloc: &Allocator,
    sections: &mut StrippedSections,
) {
    let Expression::CallExpression(call) = expr else {
        return;
    };
    let Expression::Identifier(callee) = &call.callee else {
        return;
    };
    match callee.name.as_str() {
        "defineProps" | "defineEmits" => {
            if let Some(arg) = call.arguments.first().and_then(|a| a.as_expression()) {
                if matches!(
                    arg,
                    Expression::ObjectExpression(_) | Expression::ArrayExpression(_)
                ) {
                    strip_section(arg, content_str, alloc, sections);
                }
            }
        }
        "withDefaults" => {
            if let Some(arg) = call.arguments.get(1).and_then(|a| a.as_expression()) {
                // Cache the FULL defaults expression stripped. The spread path
                // (`{ ...Defaults, k: v }`) and the variable path both emit the
                // whole second argument verbatim into `_mergeDefaults(base, …)`,
                // and a spread object's own values (`make<number>(0)`) are
                // TypeScript that would otherwise leak into the JS output.
                strip_section(arg, content_str, alloc, sections);
                // Also cache each object-literal property value individually —
                // the resolved-type props path reads per-key defaults by their
                // value span (object-literal, non-spread case).
                if let Expression::ObjectExpression(obj) = arg {
                    for prop in &obj.properties {
                        if let ObjectPropertyKind::ObjectProperty(p) = prop {
                            strip_section(&p.value, content_str, alloc, sections);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Strip one expression and record its plain-JS text keyed by its span.
fn strip_section(
    expr: &Expression,
    content_str: &str,
    alloc: &Allocator,
    sections: &mut StrippedSections,
) {
    let span = expr.span();
    sections
        .entry((span.start, span.end))
        .or_insert_with(|| strip_expression_to_string(expr, content_str, alloc));
}

/// Type-strip a macro-argument expression and return the resulting JavaScript.
///
/// Strips through the shared expression visitor (the same one the whole-program
/// force-js pass uses), then slices the expression's region back out. All
/// removals fall inside the expression's span, so the unchanged prefix and
/// suffix bound the stripped region exactly.
fn strip_expression_to_string<'a>(
    expr: &Expression,
    content_str: &'a str,
    alloc: &'a Allocator,
) -> String {
    let span = expr.span();
    let mut ct = CodeTransform::new(content_str, alloc);
    crate::strip_types::typescript::strip_typescript_from_expression(expr, &mut ct, 0, content_str);
    let output = ct.build_string();
    let tail = content_str.len() - span.end as usize;
    output[span.start as usize..output.len() - tail].to_string()
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
