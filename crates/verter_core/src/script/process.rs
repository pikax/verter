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
use rustc_hash::FxHashMap;

use crate::parser::types::RootNodeScript;
use crate::template::code_gen::binding::BindingType;
use crate::utils::oxc::vue::{parse_script, parse_script_with_companion, ScriptItem, ScriptMode};

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

    // Process items
    for item in &parse_result.items {
        match item {
            ScriptItem::Import(imp) => {
                let abs_start = content_start + imp.span.start;
                let abs_end = content_start + imp.span.end;

                if !options.keep_ts_types && imp.is_type_only {
                    // Type-only import — strip entirely when not keeping TS types
                    ctx.out.overwrite(abs_start, abs_end, "");
                } else if !options.keep_ts_types && imp.bindings.iter().any(|b| b.is_type_only) {
                    // Mixed import with per-specifier `type` keywords — reconstruct
                    // keeping only runtime specifiers
                    let runtime_bindings: Vec<&str> = imp
                        .bindings
                        .iter()
                        .filter(|b| !b.is_type_only)
                        .map(|b| b.name)
                        .collect();
                    ctx.out.overwrite(abs_start, abs_end, "");
                    if !runtime_bindings.is_empty() {
                        let new_import = format!(
                            "import {{ {} }} from '{}'\n",
                            runtime_bindings.join(", "),
                            imp.source,
                        );
                        ctx.out.prepend_alloc(hoist_pos, &new_import);
                    }
                } else {
                    // Regular import — move to file top
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
            _ => {}
        }
    }

    // Extract bindings from parse result.
    // Binding spans are content-relative (0-based within content_str).
    // Skip Props/PropsAliased — those are extracted directly during macro processing
    // (parse_script returns inconsistent span coordinate systems for prop bindings:
    // object-syntax keys are SFC-absolute, array-syntax keys are content-relative).
    for (span, bt) in &parse_result.bindings {
        if bt.is_props() {
            continue;
        }
        let name = &content_str[span.start as usize..span.end as usize];
        ctx.bindings.insert(name, *bt);
    }

    // Add companion script import names as SetupConst bindings.
    // Imports in the companion <script> block are available to the template at runtime
    // because the component factory merges both script blocks. We mark them as SetupConst
    // so they appear in the setup return object and template uses $setup.xxx prefix.
    for name in &companion_import_names {
        // Skip if setup script already declares the same name (setup takes precedence)
        let alloc_name = ctx.out.alloc_str(name);
        ctx.bindings
            .entry(alloc_name)
            .or_insert(BindingType::SetupConst);
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
        Some(build_returned_object(&ctx.bindings))
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

    // Find default export and replace it
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
/// Returns a JS object literal like `{ msg, count }`.
fn build_returned_object(bindings: &FxHashMap<&str, BindingType>) -> String {
    let mut names: Vec<&str> = bindings
        .iter()
        .filter(|(_, bt)| bt.is_setup())
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
