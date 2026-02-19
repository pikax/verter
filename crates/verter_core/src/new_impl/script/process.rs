//! Script processing: OXC parse + item handling + component wrapping.
//!
//! All transformations use [`CodeGenOutput`] (batch overwrite/prepend).
//! Import hoisting and TS type hoisting are done via the "move" pattern:
//! `overwrite(src, src_end, "")` + `prepend_alloc(target, content)`.

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::FxHashMap;

use crate::new_impl::syntax::types::RootNodeScript;
use crate::new_impl::template::code_gen::binding::BindingType;
use crate::new_impl::template::code_gen::types::CodeGenOutput;
use crate::utils::oxc::vue::{parse_script, ScriptItem, ScriptMacro, ScriptMode};

use super::{convert_binding_type, ScriptCodeGenOptions};

// ======================== Macro state ========================

/// Accumulated macro data collected during item processing.
/// Used to build the component definition sections.
struct MacroState {
    /// Props section text (e.g., `{ title: { type: String } }`).
    props_section: Option<String>,
    /// Emits section text (e.g., `['click', 'update']`).
    emits_section: Option<String>,
    /// Options section text (e.g., `inheritAttrs: false`).
    options_section: Option<String>,
    /// Whether `defineExpose` was used.
    has_expose: bool,
    /// Whether `defineEmits` was used (needs `__emit` in setup params).
    has_emit: bool,
}

impl MacroState {
    fn new() -> Self {
        Self {
            props_section: None,
            emits_section: None,
            options_section: None,
            has_expose: false,
            has_emit: false,
        }
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
#[allow(clippy::too_many_arguments)]
pub fn process_script_setup<'alloc>(
    setup: &RootNodeScript,
    _normal_script: Option<&RootNodeScript>,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    bindings: &mut FxHashMap<&'alloc str, BindingType>,
    imports: &mut Vec<&'static str>,
    inline_inject_pos: &mut Option<u32>,
    alloc: &'alloc Allocator,
    options: &ScriptCodeGenOptions<'_>,
) {
    let content_span = match &setup.content {
        Some(span) => span,
        None => {
            // Self-closing <script setup /> — emit empty component
            emit_minimal_component(setup, out, alloc, options, imports);
            return;
        }
    };

    let content_start = content_span.start;
    let content_str = &source[content_span.start as usize..content_span.end as usize];

    // Parse with OXC
    let oxc_alloc = oxc_allocator::Allocator::default();
    // Parse as TSX — OXC's TS parser is a superset of JS, so this works for all langs
    let source_type = SourceType::tsx();
    let parser_ret = Parser::new(&oxc_alloc, content_str, source_type).parse();
    let parse_result = parse_script(
        &parser_ret.program,
        ScriptMode::Setup,
        content_start,
        content_str,
    );

    // Hoist insertion point: just before the open tag
    let hoist_pos = setup.tag_open.start;

    // Collect macro state
    let mut macro_state = MacroState::new();

    // Process items
    for item in &parse_result.items {
        match item {
            ScriptItem::Import(imp) => {
                let abs_start = content_start + imp.span.start;
                let abs_end = content_start + imp.span.end;

                if !options.keep_ts_types && imp.is_type_only {
                    // Type-only import — strip entirely when not keeping TS types
                    out.overwrite(abs_start, abs_end, "");
                } else if !options.keep_ts_types && imp.bindings.iter().any(|b| b.is_type_only) {
                    // Mixed import with per-specifier `type` keywords — reconstruct
                    // keeping only runtime specifiers
                    let runtime_bindings: Vec<&str> = imp
                        .bindings
                        .iter()
                        .filter(|b| !b.is_type_only)
                        .map(|b| b.name)
                        .collect();
                    out.overwrite(abs_start, abs_end, "");
                    if !runtime_bindings.is_empty() {
                        let new_import = format!(
                            "import {{ {} }} from '{}'\n",
                            runtime_bindings.join(", "),
                            imp.source,
                        );
                        out.prepend_alloc(hoist_pos, &new_import);
                    }
                } else {
                    // Regular import — move to file top
                    let import_text = &source[abs_start as usize..abs_end as usize];
                    out.overwrite(abs_start, abs_end, "");
                    out.prepend_alloc(hoist_pos, &format!("{}\n", import_text));
                }
            }
            ScriptItem::TypeDeclaration(td) => {
                let abs_start = content_start + td.span.start;
                let abs_end = content_start + td.span.end;
                if options.keep_ts_types {
                    // Hoist to file top
                    let td_text = &source[abs_start as usize..abs_end as usize];
                    out.overwrite(abs_start, abs_end, "");
                    out.prepend_alloc(hoist_pos, &format!("{}\n", td_text));
                } else {
                    // Strip the type declaration
                    out.overwrite(abs_start, abs_end, "");
                }
            }
            ScriptItem::Macro(mac) => {
                process_macro_item(
                    mac,
                    content_start,
                    content_str,
                    out,
                    &mut macro_state,
                    imports,
                    bindings,
                );
            }
            _ => {}
        }
    }

    // Extract bindings from parse result.
    // Binding spans are content-relative (0-based within content_str).
    // Skip Props/PropsAliased — those are extracted directly during macro processing
    // (parse_script returns inconsistent span coordinate systems for prop bindings:
    // object-syntax keys are SFC-absolute, array-syntax keys are content-relative).
    for (span, old_bt) in &parse_result.bindings {
        let bt = convert_binding_type(*old_bt);
        if bt.is_props() {
            continue;
        }
        let name = &content_str[span.start as usize..span.end as usize];
        bindings.insert(name, bt);
    }

    // Inject _useCssVars if CSS v-bind vars are present
    if !options.css_v_binds.is_empty() {
        super::css_vars::inject_use_css_vars(
            options.css_v_binds,
            bindings,
            content_start, // Insert at start of setup body
            out,
            imports,
        );
    }

    // Strip TS types from macro-extracted sections.
    // These sections contain raw source text that was extracted from within macro
    // calls (defineProps, defineEmits, defineOptions). Since the macro call range
    // has been overwritten (to `__props`, `__emit`, or ""), the strip_types pass
    // that runs later on the original positions can't reach inside them.
    if !options.keep_ts_types {
        if let Some(ref mut section) = macro_state.props_section {
            *section = strip_ts_from_section(section, alloc);
        }
        if let Some(ref mut section) = macro_state.emits_section {
            *section = strip_ts_from_section(section, alloc);
        }
        if let Some(ref mut section) = macro_state.options_section {
            *section = strip_ts_from_section(section, alloc);
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
    out.overwrite(setup.tag_open.start, setup.tag_open.end, &wrapper_start);

    // Build wrapper closing
    let returned = if !options.inline_template {
        Some(build_returned_object(bindings))
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
    );

    // Handle close tag
    if let Some(tag_close) = &setup.tag_close {
        out.overwrite(tag_close.start, tag_close.end, &wrapper_end);

        // Set inline inject position
        if options.inline_template {
            *inline_inject_pos = Some(tag_close.start);
        }
    }

    // Track _defineComponent import
    imports.push("_defineComponent");
}

// ======================== Macro processing ========================

/// Process a single Vue macro item.
///
/// Handles defineProps, defineEmits, defineExpose, defineOptions,
/// defineSlots. Emits CodeGenOutput operations for the macro call
/// replacement and collects section data into `MacroState`.
///
/// Also extracts prop names directly from defineProps arguments and
/// adds them to bindings with `BindingType::Props`. This avoids
/// relying on `parse_result.bindings` for Props, which has inconsistent
/// span coordinate systems (object-syntax keys are SFC-absolute, while
/// array-syntax keys are content-relative).
fn process_macro_item<'a>(
    mac: &ScriptMacro<'_>,
    content_start: u32,
    content_str: &'a str,
    out: &mut CodeGenOutput<'_>,
    state: &mut MacroState,
    imports: &mut Vec<&'static str>,
    bindings: &mut FxHashMap<&'a str, BindingType>,
) {
    match mac {
        ScriptMacro::DefineExpose { span, .. } => {
            state.has_expose = true;
            // Replace "defineExpose" (12 chars) with "__expose", keeping args
            let abs_start = content_start + span.start;
            out.overwrite(
                abs_start,
                abs_start + "defineExpose".len() as u32,
                "__expose",
            );
        }

        ScriptMacro::DefineSlots { span, .. } => {
            // Replace entire macro call with _useSlots()
            let abs_start = content_start + span.start;
            let abs_end = content_start + span.end;
            out.overwrite(abs_start, abs_end, "_useSlots()");
            imports.push("_useSlots");
        }

        ScriptMacro::DefineProps {
            span,
            declarator,
            object_arg,
            array_arg,
            ..
        } => {
            let abs_start = content_start + span.start;
            let abs_end = content_start + span.end;

            // Extract props section from runtime argument and prop names for bindings.
            // We extract prop names here (not from parse_result.bindings) because
            // parse_script returns inconsistent span coordinate systems for Props.
            if let Some(obj) = object_arg {
                let obj_text = &content_str[obj.span.start as usize..obj.span.end as usize];
                state.props_section = Some(obj_text.to_string());
                // Extract property key names from the object
                extract_object_prop_names(obj_text, content_str, obj.span.start, bindings);
            } else if let Some(arr) = array_arg {
                let arr_text = &content_str[arr.span.start as usize..arr.span.end as usize];
                state.props_section = Some(arr_text.to_string());
                // Extract prop names from array strings
                extract_array_prop_names(arr_text, content_str, arr.span.start, bindings);
            }
            // Type-based defineProps: type resolution deferred

            // Replace macro call
            if declarator.is_some() {
                // const props = defineProps({...}) → const props = __props
                out.overwrite(abs_start, abs_end, "__props");
            } else {
                // defineProps({...}) → (removed)
                out.overwrite(abs_start, abs_end, "");
            }
        }

        ScriptMacro::DefineEmits {
            span,
            declarator,
            object_arg,
            array_arg,
            ..
        } => {
            state.has_emit = true;
            let abs_start = content_start + span.start;
            let abs_end = content_start + span.end;

            // Extract emits section from runtime argument
            if let Some(obj) = object_arg {
                let obj_text = &content_str[obj.span.start as usize..obj.span.end as usize];
                state.emits_section = Some(obj_text.to_string());
            } else if let Some(arr) = array_arg {
                let arr_text = &content_str[arr.span.start as usize..arr.span.end as usize];
                state.emits_section = Some(arr_text.to_string());
            }
            // Type-based defineEmits: type resolution deferred to Phase 5c

            // Replace macro call
            if declarator.is_some() {
                // const emit = defineEmits([...]) → const emit = __emit
                out.overwrite(abs_start, abs_end, "__emit");
            } else {
                // defineEmits([...]) → (removed)
                out.overwrite(abs_start, abs_end, "");
            }
        }

        ScriptMacro::DefineOptions {
            span, object_arg, ..
        } => {
            let abs_start = content_start + span.start;
            let abs_end = content_start + span.end;

            // Extract options object content (the object's inner content)
            if let Some(obj) = object_arg {
                // Get the content between { and }, stripping the braces
                let obj_text = &content_str[obj.span.start as usize..obj.span.end as usize];
                // Remove outer braces: "{ inheritAttrs: false }" → " inheritAttrs: false "
                if obj_text.starts_with('{') && obj_text.ends_with('}') {
                    let inner = obj_text[1..obj_text.len() - 1].trim();
                    // Strip trailing comma to avoid double commas in the generated
                    // object literal (we add our own comma after each section).
                    let inner = inner.trim_end_matches(',').trim_end();
                    if !inner.is_empty() {
                        state.options_section = Some(inner.to_string());
                    }
                }
            }

            // Remove the entire macro call
            out.overwrite(abs_start, abs_end, "");
        }

        ScriptMacro::DefineModel {
            span, name_span, ..
        } => {
            let abs_start = content_start + span.start;
            let abs_end = content_start + span.end;

            // Get model name (default: 'modelValue')
            let model_name = name_span
                .map(|ns| &content_str[ns.start as usize..ns.end as usize])
                .unwrap_or("modelValue");

            // Replace with _useModel(__props, 'name')
            let replacement = format!("_useModel(__props, '{}')", model_name);
            out.overwrite(abs_start, abs_end, &replacement);

            imports.push("_useModel");
        }

        ScriptMacro::WithDefaults {
            span, declarator, ..
        } => {
            let abs_start = content_start + span.start;
            let abs_end = content_start + span.end;

            // For now, replace with __props (simplified)
            // Full withDefaults handling (merging defaults into props) deferred
            if declarator.is_some() {
                out.overwrite(abs_start, abs_end, "__props");
            } else {
                out.overwrite(abs_start, abs_end, "");
            }
        }
    }
}

// ======================== Prop name extraction ========================

/// Extract property key names from a defineProps object literal text.
///
/// Given text like `{ title: String, count: Number }`, extracts "title" and "count"
/// and inserts them into bindings as `BindingType::Props`.
///
/// Uses the full `content_str` with `obj_offset` to get `&'a str` slices with the
/// correct lifetime (tied to source).
fn extract_object_prop_names<'a>(
    _obj_text: &str,
    content_str: &'a str,
    obj_offset: u32,
    bindings: &mut FxHashMap<&'a str, BindingType>,
) {
    // Re-parse the object expression to extract property keys reliably.
    // We parse just the object text as an expression statement.
    let oxc_alloc = oxc_allocator::Allocator::default();
    let expr_src = &content_str[obj_offset as usize..];
    // Find the end of the object expression (matching brace)
    let obj_end = find_matching_brace(expr_src);
    if obj_end == 0 {
        return;
    }
    let obj_src = &expr_src[..obj_end];
    // Wrap in parens to make it a valid expression statement
    let wrapped = format!("({})", obj_src);
    let source_type = SourceType::tsx();
    let parser_ret = Parser::new(&oxc_alloc, &wrapped, source_type).parse();
    // Walk the parsed AST to find property keys
    for stmt in &parser_ret.program.body {
        if let oxc_ast::ast::Statement::ExpressionStatement(es) = stmt {
            if let oxc_ast::ast::Expression::ParenthesizedExpression(paren) = &es.expression {
                if let oxc_ast::ast::Expression::ObjectExpression(obj) = &paren.expression {
                    for prop_kind in &obj.properties {
                        if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) = prop_kind {
                            if let oxc_ast::ast::PropertyKey::StaticIdentifier(ident) = &p.key {
                                // ident.span is relative to `wrapped`, offset by 1 for the opening paren
                                let name_start = obj_offset + ident.span.start - 1; // -1 for wrapping paren
                                let name_end = obj_offset + ident.span.end - 1;
                                let name = &content_str[name_start as usize..name_end as usize];
                                bindings.insert(name, BindingType::Props);
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Find the position of the matching closing brace for text starting with `{`.
fn find_matching_brace(s: &str) -> usize {
    if !s.starts_with('{') {
        return 0;
    }
    let mut depth = 0i32;
    for (i, ch) in s.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    0
}

/// Extract prop names from a defineProps array literal text.
///
/// Given text like `['title', 'count']`, extracts "title" and "count"
/// from content_str with correct lifetime.
fn extract_array_prop_names<'a>(
    arr_text: &str,
    content_str: &'a str,
    arr_offset: u32,
    bindings: &mut FxHashMap<&'a str, BindingType>,
) {
    // Simple parsing: find string literals in the array
    let mut i = 0;
    let bytes = arr_text.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let quote = bytes[i];
            let start = i + 1;
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            if i < bytes.len() {
                // Found a string literal from start..i
                let abs_start = arr_offset as usize + start;
                let abs_end = arr_offset as usize + i;
                if abs_end <= content_str.len() {
                    let name = &content_str[abs_start..abs_end];
                    bindings.insert(name, BindingType::Props);
                }
            }
        }
        i += 1;
    }
}

// ======================== process_script_only ========================

/// Process a standalone `<script>` block (Options API, no setup).
///
/// Handles:
/// 1. Removing `<script>` and `</script>` tags
/// 2. Converting `export default { ... }` to `const __sfc__ = { ... }`
/// 3. Appending `export default __sfc__`
#[allow(clippy::too_many_arguments)]
pub fn process_script_only<'alloc>(
    script: &RootNodeScript,
    source: &'alloc str,
    out: &mut CodeGenOutput<'alloc>,
    _bindings: &mut FxHashMap<&'alloc str, BindingType>,
    _imports: &mut Vec<&'static str>,
    _alloc: &'alloc Allocator,
    options: &ScriptCodeGenOptions<'_>,
) {
    let content_span = match &script.content {
        Some(span) => span,
        None => return,
    };

    let content_start = content_span.start;
    let content_str = &source[content_span.start as usize..content_span.end as usize];

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
            out.overwrite(abs_start, replace_end, "const __sfc__ =");
        }
    }

    // Remove open tag
    out.overwrite(script.tag_open.start, script.tag_open.end, "");

    // Build close tag replacement
    let mut close_text = String::with_capacity(64);
    if !has_default_export {
        // No default export — create a minimal __sfc__
        close_text.push_str("\nconst __sfc__ = {};\n");
    }
    if options.has_scoped_style && !options.scope_id.is_empty() {
        close_text.push_str("__sfc__.__scopeId = \"");
        close_text.push_str(options.scope_id);
        close_text.push_str("\";\n");
    }
    close_text.push_str("export default __sfc__;\n");

    if let Some(tag_close) = &script.tag_close {
        out.overwrite(tag_close.start, tag_close.end, &close_text);
    }
}

// ======================== Helpers ========================

/// Emit a minimal component definition for empty/self-closing script setup.
fn emit_minimal_component<'alloc>(
    setup: &RootNodeScript,
    out: &mut CodeGenOutput<'alloc>,
    _alloc: &'alloc Allocator,
    options: &ScriptCodeGenOptions<'_>,
    imports: &mut Vec<&'static str>,
) {
    let mut s = String::with_capacity(128);
    s.push_str("const __sfc__ = /*@__PURE__*/_defineComponent({\n");
    if !options.component_name.is_empty() {
        s.push_str("  __name: '");
        s.push_str(options.component_name);
        s.push_str("',\n");
    }
    s.push_str("});\n");
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
    out.overwrite(setup.tag_open.start, end, &s);

    imports.push("_defineComponent");
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
fn build_setup_wrapper_end(returned: Option<&str>, scope_id: Option<&str>) -> String {
    let mut s = String::with_capacity(128);
    if let Some(ret) = returned {
        s.push_str("\nreturn ");
        s.push_str(ret);
        s.push_str(";\n");
    }
    s.push_str("\n}});\n");
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
fn strip_ts_from_section(section: &str, _allocator: &Allocator) -> String {
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
mod tests {
    use super::*;

    #[test]
    fn build_wrapper_start_basic() {
        let result = build_setup_wrapper_start("Test", false, false, false, None, None, None);
        assert!(result.contains("__name: 'Test'"));
        assert!(result.contains("setup(__props) {"));
        assert!(!result.contains("async"));
    }

    #[test]
    fn build_wrapper_start_async() {
        let result = build_setup_wrapper_start("Test", true, false, false, None, None, None);
        assert!(result.contains("async setup(__props"));
    }

    #[test]
    fn build_wrapper_start_no_name() {
        let result = build_setup_wrapper_start("", false, false, false, None, None, None);
        assert!(!result.contains("__name"));
    }

    #[test]
    fn build_wrapper_start_with_props() {
        let result = build_setup_wrapper_start(
            "Test",
            false,
            false,
            false,
            Some("{ title: String }"),
            None,
            None,
        );
        assert!(result.contains("props: { title: String }"));
    }

    #[test]
    fn build_wrapper_start_with_emits() {
        let result =
            build_setup_wrapper_start("Test", false, false, true, None, Some("['click']"), None);
        assert!(result.contains("emits: ['click']"));
        assert!(result.contains("emit: __emit"));
    }

    #[test]
    fn build_wrapper_start_with_expose() {
        let result = build_setup_wrapper_start("Test", false, true, false, None, None, None);
        assert!(result.contains("expose: __expose"));
    }

    #[test]
    fn build_wrapper_start_with_expose_and_emit() {
        let result = build_setup_wrapper_start("Test", false, true, true, None, None, None);
        assert!(result.contains("expose: __expose, emit: __emit"));
    }

    #[test]
    fn build_wrapper_start_with_options() {
        let result = build_setup_wrapper_start(
            "Test",
            false,
            false,
            false,
            None,
            None,
            Some("inheritAttrs: false"),
        );
        assert!(result.contains("inheritAttrs: false"));
        // Options should come before __name
        let opts_pos = result.find("inheritAttrs").unwrap();
        let name_pos = result.find("__name").unwrap();
        assert!(opts_pos < name_pos);
    }

    #[test]
    fn build_wrapper_end_with_return() {
        let result = build_setup_wrapper_end(Some("{ msg, count }"), None);
        assert!(result.contains("return { msg, count }"));
        assert!(result.contains("}});"));
        assert!(result.contains("export default __sfc__"));
    }

    #[test]
    fn build_wrapper_end_no_return() {
        let result = build_setup_wrapper_end(None, None);
        assert!(!result.contains("return"));
        assert!(result.contains("}});"));
    }

    #[test]
    fn build_wrapper_end_with_scope_id() {
        let result = build_setup_wrapper_end(None, Some("data-v-abc"));
        assert!(result.contains("__sfc__.__scopeId = \"data-v-abc\""));
    }

    #[test]
    fn build_returned_empty() {
        let bindings = FxHashMap::default();
        assert_eq!(build_returned_object(&bindings), "{}");
    }

    #[test]
    fn build_returned_setup_bindings_only() {
        let mut bindings = FxHashMap::default();
        bindings.insert("count", BindingType::SetupRef);
        bindings.insert("msg", BindingType::SetupConst);
        bindings.insert("title", BindingType::Props); // Not included
        let result = build_returned_object(&bindings);
        assert!(result.contains("count"));
        assert!(result.contains("msg"));
        assert!(!result.contains("title"));
    }

    #[test]
    fn build_returned_sorted() {
        let mut bindings = FxHashMap::default();
        bindings.insert("zebra", BindingType::SetupConst);
        bindings.insert("alpha", BindingType::SetupRef);
        let result = build_returned_object(&bindings);
        let alpha_pos = result.find("alpha").unwrap();
        let zebra_pos = result.find("zebra").unwrap();
        assert!(alpha_pos < zebra_pos);
    }
}
