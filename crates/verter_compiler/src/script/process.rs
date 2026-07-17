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

use super::macros::{process_companion_script, process_macro_item, MacroState, StrippedSections};
use super::{ScriptCodeGenOptions, ScriptContext};

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
) {
    if setup.content.is_none() {
        // Self-closing <script setup /> — emit empty component
        emit_minimal_component(setup, ctx, options);
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
        Some(companion) => {
            process_companion_script(companion, ctx.source, &mut ctx.out, &mut macro_state)
        }
        None => Vec::new(),
    };

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

    // Process items
    for item in &parse_result.items {
        match item {
            ScriptItem::Import(imp) => {
                let abs_start = content_start + imp.span.start;
                let abs_end = content_start + imp.span.end;

                if !options.keep_ts_types && imp.is_type_only {
                    // Type-only import — strip entirely when not keeping TS types
                    ctx.out.overwrite(abs_start, abs_end, "");
                } else if !options.keep_ts_types {
                    // force_js mode: reconstruct import keeping only specifiers
                    // that have runtime usage (in script body or template)
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
                let abs_start = content_start + td.span.start;
                let abs_end = content_start + td.span.end;
                if options.keep_ts_types {
                    // Hoist to file top
                    let td_text = &ctx.source[abs_start as usize..abs_end as usize];
                    ctx.out.overwrite(abs_start, abs_end, "");
                    ctx.out.prepend_alloc(hoist_pos, &format!("{}\n", td_text));
                } else {
                    // Strip the type declaration
                    ctx.out.overwrite(abs_start, abs_end, "");
                }
            }
            ScriptItem::Macro(mac) => {
                process_macro_item(
                    mac,
                    content_start,
                    content_str,
                    ctx,
                    &mut macro_state,
                    stripped_sections.as_ref(),
                );
            }
            // Transform `await <arg>` → _withAsyncContext wrapper.
            // Vue wraps each top-level await to preserve component instance context.
            ScriptItem::Async(async_item) if async_item.kind == AsyncKind::AwaitExpression => {
                if let Some(arg_span) = &async_item.arg_span {
                    let abs_start = content_start + async_item.span.start;
                    let abs_arg_start = content_start + arg_span.start;
                    let abs_end = content_start + async_item.span.end;
                    let arg_text = &ctx.source[abs_arg_start as usize..abs_end as usize];

                    // Replace `await <arg>` with a parenthesised comma-expression:
                    // (([__temp,__restore] = _withAsyncContext(() => <arg>)),
                    //   __temp = await __temp, __restore(), __temp)
                    // The outer parens are critical — without them a `const x = ...`
                    // initializer would break at the first comma.
                    let replacement = format!(
                        "(([__temp,__restore] = _withAsyncContext(() => {})), __temp = await __temp, __restore(), __temp)",
                        arg_text
                    );
                    ctx.out.overwrite(abs_start, abs_end, &replacement);
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
        ctx.bindings.insert(name, *bt);
    }

    // Add companion script import names as SetupImport bindings.
    // Imports in the companion <script> block are available to the template at runtime
    // because the component factory merges both script blocks. We mark them as SetupImport
    // (not SetupConst) so they're filtered by template_used_vars — only companion imports
    // actually referenced in the template appear in __returned__. This matches Vue's
    // official compiler behavior and prevents type-only imports (e.g., CurrencyCodes used
    // only as `"EUR" as CurrencyCodes`) from leaking into __returned__ as runtime references.
    for name in &companion_import_names {
        // Skip if setup script already declares the same name (setup takes precedence)
        let alloc_name = ctx.out.alloc_str(name);
        ctx.bindings
            .entry(alloc_name)
            .or_insert(BindingType::SetupImport);
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
    if !macro_state.model_names.is_empty() {
        // Build model props object: { name: { type: T, default: v }, nameModifiers: {} }
        let mut model_props_obj = String::from("{\n");
        for (i, (name, options)) in macro_state.model_names.iter().enumerate() {
            if i > 0 {
                model_props_obj.push_str(",\n");
            }
            model_props_obj.push_str("    ");
            model_props_obj.push_str(name);
            // Forward defineModel options (type, default, etc.) if provided
            match options {
                Some(opts) => {
                    model_props_obj.push_str(": ");
                    model_props_obj.push_str(opts);
                }
                None => {
                    model_props_obj.push_str(": {}");
                }
            }
            model_props_obj.push_str(",\n    ");
            if name == "modelValue" {
                model_props_obj.push_str("modelModifiers: {}");
            } else {
                model_props_obj.push_str(name);
                model_props_obj.push_str("Modifiers: {}");
            }
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
            .model_names
            .iter()
            .map(|(name, _)| format!("\"update:{}\"", name))
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

    // Build wrapper opening (includes __name, props, emits, options sections)
    let wrapper_start = build_setup_wrapper_start(
        options.component_name,
        parse_result.is_async,
        macro_state.has_expose,
        macro_state.has_emit,
        macro_state.props_section.as_deref(),
        macro_state.emits_section.as_deref(),
        macro_state.options_section.as_deref(),
        options.ssr,
    );

    // Overwrite open tag with wrapper
    ctx.out
        .overwrite(setup.tag_open.start, setup.tag_open.end, &wrapper_start);

    // Build wrapper closing
    let returned = if !options.inline_template {
        Some(build_returned_object(
            &ctx.bindings,
            options.template_used_vars.as_ref(),
        ))
    } else {
        None
    };

    let wrapper_end = build_setup_wrapper_end(
        returned.as_deref(),
        if options.has_scoped_style {
            Some(options.scope_id)
        } else {
            None
        },
        options.is_vapor,
        options.ssr,
    );

    // Handle close tag
    if let Some(tag_close) = &setup.tag_close {
        ctx.out
            .overwrite(tag_close.start, tag_close.end, &wrapper_end);

        // Set inline inject position
        if options.inline_template {
            ctx.inline_inject_pos = Some(tag_close.start);
        }
    }

    // Track _defineComponent import
    ctx.imports.push("_defineComponent");
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
            ctx.out.overwrite(abs_start, replace_end, "const __sfc__ =");
        }
    }

    // Remove open tag
    ctx.out
        .overwrite(script.tag_open.start, script.tag_open.end, "");

    // Build close tag replacement
    let mut close_text = String::with_capacity(64);
    if !has_default_export {
        // No default export — create a minimal __sfc__
        close_text.push_str("\nconst __sfc__ = {};\n");
    }
    if options.is_vapor {
        close_text.push_str("__sfc__.__vapor = true;\n");
    }
    // Non-inline SSR attaches `ssrRender` separately (not returned from setup),
    // so do not claim `__ssrInlineRender`.
    if options.has_scoped_style && !options.scope_id.is_empty() {
        close_text.push_str("__sfc__.__scopeId = \"");
        close_text.push_str(options.scope_id);
        close_text.push_str("\";\n");
    }
    close_text.push_str("export default __sfc__;\n");

    if let Some(tag_close) = &script.tag_close {
        ctx.out
            .overwrite(tag_close.start, tag_close.end, &close_text);
    }
}

// ======================== Helpers ========================

/// Emit a minimal component definition for empty/self-closing script setup.
fn emit_minimal_component(
    setup: &RootNodeScript,
    ctx: &mut ScriptContext<'_>,
    options: &ScriptCodeGenOptions<'_>,
) {
    let mut s = String::with_capacity(128);
    s.push_str("const __sfc__ = /*@__PURE__*/_defineComponent({\n");
    if !options.component_name.is_empty() {
        s.push_str("  __name: '");
        s.push_str(options.component_name);
        s.push_str("',\n");
    }
    s.push_str("});\n");
    if options.is_vapor {
        s.push_str("__sfc__.__vapor = true;\n");
    }
    // Non-inline SSR attaches `ssrRender` separately — no `__ssrInlineRender`.
    if options.has_scoped_style && !options.scope_id.is_empty() {
        s.push_str("__sfc__.__scopeId = \"");
        s.push_str(options.scope_id);
        s.push_str("\";\n");
    }
    s.push_str("export default __sfc__;\n");

    let end = setup
        .tag_close
        .as_ref()
        .map(|t| t.end)
        .unwrap_or(setup.tag_open.end);
    ctx.out.overwrite(setup.tag_open.start, end, &s);

    ctx.imports.push("_defineComponent");
}

/// Build the opening part of the setup wrapper with sections.
///
/// ```js
/// const __sfc__ = /*@__PURE__*/_defineComponent({
///   inheritAttrs: false,           // from defineOptions
///   __name: 'ComponentName',
///   props: { title: String },      // from defineProps
///   emits: ['click'],              // from defineEmits
///   setup(__props, { expose: __expose, emit: __emit }) {
/// ```
#[allow(clippy::too_many_arguments)]
fn build_setup_wrapper_start(
    component_name: &str,
    is_async: bool,
    has_expose: bool,
    has_emit: bool,
    props_section: Option<&str>,
    emits_section: Option<&str>,
    options_section: Option<&str>,
    ssr: bool,
) -> String {
    let mut s = String::with_capacity(256);
    s.push_str("const __sfc__ = /*@__PURE__*/_defineComponent({\n");

    // defineOptions content (before __name)
    if let Some(opts) = options_section {
        s.push_str("  ");
        s.push_str(opts);
        s.push_str(",\n");
    }

    if !component_name.is_empty() {
        s.push_str("  __name: '");
        s.push_str(component_name);
        s.push_str("',\n");
    }

    // Non-inline SSR does not return a render function from setup; the template
    // is attached as `Component.ssrRender`. Do not set `__ssrInlineRender`.
    let _ = ssr;

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

    // Setup function signature
    if is_async {
        s.push_str("  async setup(__props");
    } else {
        s.push_str("  setup(__props");
    }

    // Add destructured context if needed
    if has_expose || has_emit {
        s.push_str(", { ");
        let mut first = true;
        if has_expose {
            s.push_str("expose: __expose");
            first = false;
        }
        if has_emit {
            if !first {
                s.push_str(", ");
            }
            s.push_str("emit: __emit");
        }
        s.push_str(" }");
    }

    s.push_str(") {\n");
    s
}

/// Build the closing part of the setup wrapper.
///
/// ```js
///   return { msg, count }
/// }});
/// __sfc__.__scopeId = "data-v-xxx";
/// export default __sfc__;
/// ```
fn build_setup_wrapper_end(
    returned: Option<&str>,
    scope_id: Option<&str>,
    is_vapor: bool,
    ssr: bool,
) -> String {
    let mut s = String::with_capacity(128);
    if let Some(ret) = returned {
        // Client (non-SSR): match Vue's official compiler — assign returned
        // bindings and mark with `__isScriptSetup` so @vue/test-utils can
        // identify script-setup components.
        //
        // SSR non-inline path: setup returns bindings and `ssrRender` is
        // attached separately, reading them via the instance proxy (`_ctx.*`).
        // Vue does NOT expose `__isScriptSetup` return keys on that proxy for
        // ssrRender, so emitting the marker here makes every `_ctx.n` /
        // `_ctx.Child` access miss (empty interpolations / missing children).
        // Official plugin-vue avoids this by true-inline SSR (setup returns the
        // render function). Until Verter does that, SSR must return a plain
        // object without the marker.
        s.push_str("\nconst __returned__ = ");
        s.push_str(ret);
        if ssr {
            s.push_str(";\nreturn __returned__;\n");
        } else {
            s.push_str(";\nObject.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true });\nreturn __returned__;\n");
        }
    }
    s.push_str("\n}});\n");
    if is_vapor {
        s.push_str("__sfc__.__vapor = true;\n");
    }
    // Non-inline SSR attaches `ssrRender` separately — do not set
    // `__ssrInlineRender` (that flag means setup returns the render function).
    let _ = ssr;
    if let Some(id) = scope_id {
        s.push_str("__sfc__.__scopeId = \"");
        s.push_str(id);
        s.push_str("\";\n");
    }
    s.push_str("export default __sfc__;\n");
    s
}

/// Build the `__returned__` object from bindings.
///
/// Includes all setup-type bindings (not props, data, or options).
/// `SetupImport` bindings are only included when their identifier appears in
/// the `template_used_vars` set (AST-based, from expression bindings + component
/// tag names). Returns a JS object literal like `{ msg, count }`.
fn build_returned_object(
    bindings: &FxHashMap<&str, BindingType>,
    template_used_vars: Option<&FxHashSet<String>>,
) -> String {
    let mut names: Vec<&str> = bindings
        .iter()
        .filter(|(name, bt)| {
            if !bt.is_setup() {
                return false;
            }
            // SetupImport: only include if identifier is used in the template
            if **bt == BindingType::SetupImport {
                match template_used_vars {
                    Some(vars) => vars.contains(name as &str),
                    // No template → include all (conservative)
                    None => true,
                }
            } else {
                true
            }
        })
        .map(|(name, _)| *name)
        .collect();
    names.sort(); // Deterministic order

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

/// Type-strip the setup `program` into a fresh JavaScript string, without
/// re-parsing the content.
fn strip_program_to_string(program: &Program, content_str: &str) -> String {
    let alloc = Allocator::new();
    let mut ct = CodeTransform::new(content_str, &alloc);
    crate::strip_types::typescript::strip_typescript_types(program, &mut ct, 0, content_str);
    ct.build_string()
}

/// Compute type-stripped script content with import lines blanked out.
///
/// Used to determine which import specifiers have runtime references (appear in
/// the script body after type annotations are removed) vs type-only usage.
/// Strips from the already-parsed setup `program` rather than re-parsing the
/// content. The blanked string is a throwaway used only for identifier-presence
/// scanning — it is never emitted, so it carries no source map.
fn compute_runtime_text(program: &Program, content_str: &str, items: &[ScriptItem]) -> String {
    // Blank out import statement regions so specifier names in the import line
    // itself don't cause false positives.
    let mut result = strip_program_to_string(program, content_str);
    for item in items {
        if let ScriptItem::Import(imp) = item {
            let start = imp.span.start as usize;
            let end = imp.span.end as usize;
            if end <= result.len() {
                // Replace with spaces to preserve byte positions
                result.replace_range(start..end, &" ".repeat(end - start));
            }
        }
    }
    result
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
    let mut named: Vec<&str> = Vec::new();

    for b in &imp.bindings {
        if b.is_type_only {
            continue;
        }

        // Check if specifier is used at runtime
        let is_runtime_used = is_specifier_runtime_used(b.name, runtime_text, template_used_vars);

        if !is_runtime_used {
            continue;
        }

        match b.import_kind {
            Some(ImportSpecifierKind::Default) => default_name = Some(b.name),
            Some(ImportSpecifierKind::Namespace) => namespace_name = Some(b.name),
            Some(ImportSpecifierKind::Named) | None => named.push(b.name),
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
            for (i, name) in named.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(name);
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
                match arg {
                    Expression::ObjectExpression(obj) => {
                        for prop in &obj.properties {
                            if let ObjectPropertyKind::ObjectProperty(p) = prop {
                                strip_section(&p.value, content_str, alloc, sections);
                            }
                        }
                    }
                    other => strip_section(other, content_str, alloc, sections),
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
