//! Script processing: OXC parse + item handling + component wrapping.
//!
//! All transformations use [`CodeGenOutput`] (batch overwrite/prepend).
//! Import hoisting and TS type hoisting are done via the "move" pattern:
//! `overwrite(src, src_end, "")` + `prepend_alloc(target, content)`.
//!
//! Macro processing (`defineProps`, `defineEmits`, etc.) is in [`super::macros`].

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::parser::types::RootNodeScript;
use crate::template::code_gen::binding::BindingType;
use crate::utils::oxc::vue::{
    parse_script, parse_script_with_companion, AsyncKind, ImportSpecifierKind, ScriptImport,
    ScriptItem, ScriptMode,
};

use super::macros::{process_companion_script, process_macro_item, MacroState};
use super::{ScriptCodeGenOptions, ScriptContext};

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
    normal_script: Option<&RootNodeScript>,
    ctx: &mut ScriptContext<'alloc>,
    options: &ScriptCodeGenOptions<'_>,
) {
    let content_span = match &setup.content {
        Some(span) => span,
        None => {
            // Self-closing <script setup /> — emit empty component
            emit_minimal_component(setup, ctx, options);
            return;
        }
    };

    let content_start = content_span.start;
    let content_str = &ctx.source[content_span.start as usize..content_span.end as usize];

    // Hoist insertion point: just before the open tag
    let hoist_pos = setup.tag_open.start;

    // Collect macro state
    let mut macro_state = MacroState::new();

    // Process companion <script> block FIRST: extract `export default` options,
    // remove duplicate default exports, extract type declarations for cross-block
    // type resolution, and collect non-type import names for template resolution.
    let (companion_types, companion_import_names) = match normal_script {
        Some(normal) => {
            let (types, imports) =
                process_companion_script(normal, ctx.source, &mut ctx.out, &mut macro_state);
            (Some(types), imports)
        }
        None => (None, Vec::new()),
    };

    // Parse setup script with OXC, passing companion types for cross-block resolution
    let oxc_alloc = oxc_allocator::Allocator::default();
    // Parse as TSX — OXC's TS parser is a superset of JS, so this works for all langs
    let source_type = SourceType::tsx();
    let parser_ret = Parser::new(&oxc_alloc, content_str, source_type).parse();

    // Merge external types (from host's cross-file resolution) into companion types
    let companion_types = match (companion_types, options.external_types.as_ref()) {
        (Some(mut ct), Some(ext)) => {
            for (k, v) in ext {
                ct.entry(k.clone()).or_insert_with(|| v.clone());
            }
            Some(ct)
        }
        (Some(ct), None) => Some(ct),
        (None, Some(ext)) => Some(ext.clone()),
        (None, None) => None,
    };

    let parse_result = parse_script_with_companion(
        &parser_ret.program,
        ScriptMode::Setup,
        content_start,
        content_str,
        companion_types,
    );

    // In force_js mode, compute type-stripped script content (sans imports) to
    // determine which import specifiers have runtime references vs type-only usage.
    let runtime_text = if !options.keep_ts_types {
        Some(compute_runtime_text(content_str, &parse_result.items))
    } else {
        None
    };

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
                process_macro_item(mac, content_start, content_str, ctx, &mut macro_state);
            }
            ScriptItem::Async(async_item) => {
                // Transform `await <arg>` → _withAsyncContext wrapper.
                // Vue wraps each top-level await to preserve component instance context.
                if async_item.kind == AsyncKind::AwaitExpression {
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

    // Strip TS types from macro-extracted sections.
    // These sections contain raw source text that was extracted from within macro
    // calls (defineProps, defineEmits, defineOptions). Since the macro call range
    // has been overwritten (to `__props`, `__emit`, or ""), the strip_types pass
    // that runs later on the original positions can't reach inside them.
    if !options.keep_ts_types {
        if let Some(ref mut section) = macro_state.props_section {
            *section = force_js_in_section(section, ctx.alloc);
        }
        if let Some(ref mut section) = macro_state.emits_section {
            *section = force_js_in_section(section, ctx.alloc);
        }
        if let Some(ref mut section) = macro_state.options_section {
            *section = force_js_in_section(section, ctx.alloc);
        }
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
        // Build model props object: { name: {}, nameModifiers: {} }
        let mut model_props_obj = String::from("{\n");
        for (i, name) in macro_state.model_names.iter().enumerate() {
            if i > 0 {
                model_props_obj.push_str(",\n");
            }
            model_props_obj.push_str("    ");
            model_props_obj.push_str(name);
            model_props_obj.push_str(": {},\n    ");
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
            .map(|name| format!("\"update:{}\"", name))
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
    ctx: &mut ScriptContext<'alloc>,
    options: &ScriptCodeGenOptions<'_>,
) {
    let content_span = match &script.content {
        Some(span) => span,
        None => return,
    };

    let content_start = content_span.start;
    let content_str = &ctx.source[content_span.start as usize..content_span.end as usize];

    // Parse with OXC to find default export
    let oxc_alloc = oxc_allocator::Allocator::default();
    let source_type = SourceType::tsx();
    let parser_ret = Parser::new(&oxc_alloc, content_str, source_type).parse();
    let parse_result = parse_script(
        &parser_ret.program,
        ScriptMode::Options,
        content_start,
        content_str,
    );

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
fn build_setup_wrapper_start(
    component_name: &str,
    is_async: bool,
    has_expose: bool,
    has_emit: bool,
    props_section: Option<&str>,
    emits_section: Option<&str>,
    options_section: Option<&str>,
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
) -> String {
    let mut s = String::with_capacity(128);
    if let Some(ret) = returned {
        // Match Vue's official compiler: assign returned bindings to a variable,
        // mark it with __isScriptSetup so @vue/test-utils (and other tools) can
        // identify script-setup components and apply stubs to setup-returned refs.
        s.push_str("\nconst __returned__ = ");
        s.push_str(ret);
        s.push_str(";\nObject.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true });\nreturn __returned__;\n");
    }
    s.push_str("\n}});\n");
    if is_vapor {
        s.push_str("__sfc__.__vapor = true;\n");
    }
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

/// Compute type-stripped script content with import lines blanked out.
///
/// Used to determine which import specifiers have runtime references (appear in
/// the script body after type annotations are removed) vs type-only usage.
fn compute_runtime_text(content_str: &str, items: &[ScriptItem]) -> String {
    let alloc = Allocator::new();
    let stripped = crate::strip_types::strip_types(content_str, &alloc);

    // Blank out import statement regions so specifier names in the import line
    // itself don't cause false positives.
    let mut result = stripped.code;
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

/// Strip TypeScript type annotations from a section string (props/emits/options).
///
/// These sections are raw source text extracted from macro calls. Since they're
/// inserted into the generated component definition as literal strings, the main
/// `strip_types` pass (which runs on original source positions) can't reach them.
///
/// Wraps the section in a parseable expression context, strips types,
/// then extracts the inner content back out.
fn force_js_in_section(section: &str, _allocator: &Allocator) -> String {
    // Wrap the section as a variable initializer so OXC can parse it.
    // The section may be a complete expression (object, array) or a fragment
    // of object properties — use `var _ = (...)` to handle both.
    let prefix = "var _ = (";
    let suffix = ")";
    let wrapped = format!("{}{}{}", prefix, section, suffix);
    let alloc = Allocator::new();
    let result = crate::strip_types::strip_types(&wrapped, &alloc);
    if result.code.starts_with(prefix) && result.code.ends_with(suffix) {
        result.code[prefix.len()..result.code.len() - suffix.len()].to_string()
    } else {
        // Fallback: return as-is if wrapper was altered
        section.to_string()
    }
}

#[cfg(test)]
#[path = "process_tests.rs"]
mod tests;
