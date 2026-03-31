//! Code action: extract bare text + interpolation to computed() or function.
//!
//! When `no-bare-strings-in-template` fires (or when the cursor is inside an
//! element with mixed text + interpolation), this provider extracts the content
//! into either:
//! - A `computed()` (when all bindings are script-level)
//! - A helper function (when some bindings come from v-for / v-slot scope)
//!
//! The element's inner content is replaced with `{{ varName }}`.

// @ai-generated

use verter_diagnostics::LintDiagnostic;
use verter_semantic::analysis::template::{
    TemplateAnalysisSnapshot, TemplateElement, TemplateTextSegment,
};
use verter_semantic::analysis::types::{
    AnalyzedMacroKind, BindingInitializer, ReactivityKind, ScriptAnalysisSnapshot,
    VueApiClassification,
};
use verter_span::Span;

use crate::provider::{ActionContext, ActionProvider};
use crate::types::{ActionKind, AutofixSafety, CodeAction, FileEdit};

pub struct ExtractBareText;

impl ActionProvider for ExtractBareText {
    fn name(&self) -> &str {
        "extract-bare-text"
    }

    fn fixes_for_diagnostic(&self, diag: &LintDiagnostic, ctx: &ActionContext) -> Vec<CodeAction> {
        if diag.rule != "no-bare-strings-in-template" {
            return vec![];
        }

        let Some(template) = ctx.template.as_ref() else {
            return vec![];
        };
        let script = ctx.script;

        // Find the element whose content span matches the diagnostic
        let Some(el) = find_element_for_content_span(template, diag.span) else {
            return vec![];
        };

        build_extract_action(el, template, script, ctx.source)
    }

    fn actions_at(&self, offset: u32, ctx: &ActionContext) -> Vec<CodeAction> {
        let Some(template) = ctx.template.as_ref() else {
            return vec![];
        };
        let script = ctx.script;

        // Find element where cursor is in content area and has bare text
        let Some(el) = find_element_at_content(template, offset) else {
            return vec![];
        };

        build_extract_action(el, template, script, ctx.source)
    }
}

/// Find element whose content span (tag_span_end..content_end) matches the diagnostic span.
fn find_element_for_content_span(
    template: &TemplateAnalysisSnapshot,
    span: Span,
) -> Option<&TemplateElement> {
    template
        .elements
        .iter()
        .find(|el| el.has_bare_text && el.tag_span_end == span.start && el.content_end == span.end)
}

/// Find element where cursor offset falls within content area and has bare text.
fn find_element_at_content(
    template: &TemplateAnalysisSnapshot,
    offset: u32,
) -> Option<&TemplateElement> {
    template.elements.iter().find(|el| {
        el.has_bare_text
            && el.tag_span_end <= offset
            && offset <= el.content_end
            && el.content_end > el.tag_span_end
    })
}

/// Collect variables from v-for / v-slot scope by walking up parent chain.
fn collect_scoped_vars(
    el: &TemplateElement,
    elements: &[TemplateElement],
) -> std::collections::HashSet<String> {
    let mut scoped = std::collections::HashSet::new();
    let mut current = Some(el);

    // Also check the element itself
    while let Some(e) = current {
        // v-for variables
        if let Some(ref vfor) = e.v_for {
            scoped.insert(vfor.variable.clone());
            if let Some(ref idx) = vfor.index {
                scoped.insert(idx.clone());
            }
        }

        // v-slot params: directive with name == "slot" and an expression
        for dir in &e.directives {
            if dir.name == "slot" {
                if let Some(ref expr) = dir.expression {
                    // Parse comma-separated param names from e.g. "{ item, index }"
                    let cleaned = expr.trim().trim_start_matches('{').trim_end_matches('}');
                    for part in cleaned.split(',') {
                        let name = part.trim().split(':').next().unwrap_or("").trim();
                        if !name.is_empty() {
                            scoped.insert(name.to_string());
                        }
                    }
                }
            }
        }

        // Walk up parent chain
        current = e.parent_index.and_then(|idx| elements.get(idx as usize));
    }

    scoped
}

/// Extract simple identifiers from an interpolation expression.
/// E.g., `count` → ["count"], `items.length` → ["items"], `a + b` → ["a", "b"]
fn extract_identifiers(expr: &str) -> Vec<&str> {
    let mut idents = Vec::new();
    let mut start = None;
    for (i, c) in expr.char_indices() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
            if start.is_none() {
                // Don't start identifier with a digit
                if !c.is_ascii_digit() {
                    start = Some(i);
                }
            }
        } else if let Some(s) = start.take() {
            let word = &expr[s..i];
            // Skip JS keywords
            if !is_js_keyword(word) {
                idents.push(word);
            }
        }
    }
    // Trailing identifier
    if let Some(s) = start {
        let word = &expr[s..];
        if !is_js_keyword(word) {
            idents.push(word);
        }
    }
    idents
}

fn is_js_keyword(w: &str) -> bool {
    matches!(
        w,
        "true"
            | "false"
            | "null"
            | "undefined"
            | "new"
            | "typeof"
            | "instanceof"
            | "void"
            | "delete"
            | "in"
            | "of"
            | "if"
            | "else"
            | "return"
            | "this"
            | "const"
            | "let"
            | "var"
    )
}

/// Generate a camelCase name from the text children content.
fn generate_name(el: &TemplateElement, source: &str) -> String {
    // Take text from first Text segment
    let mut text_hint = String::new();
    let mut expr_hint = String::new();

    for seg in &el.text_children {
        match seg {
            TemplateTextSegment::Text { span, .. } => {
                if text_hint.is_empty() {
                    let s = span.start as usize;
                    let e = span.end as usize;
                    if s < source.len() && e <= source.len() {
                        text_hint = source[s..e].to_string();
                    }
                }
            }
            TemplateTextSegment::Interpolation {
                expression_span, ..
            } => {
                if expr_hint.is_empty() {
                    let s = expression_span.start as usize;
                    let e = expression_span.end as usize;
                    if s < source.len() && e <= source.len() {
                        let expr = source[s..e].trim();
                        // Use first simple identifier
                        if let Some(ident) = extract_identifiers(expr).into_iter().next() {
                            expr_hint = ident.to_string();
                        }
                    }
                }
            }
        }
    }

    // Clean text_hint: strip punctuation/whitespace, take first word(s)
    let cleaned: String = text_hint
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ')
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().take(2).collect();

    if !words.is_empty() {
        // camelCase the words + append expr hint
        let mut name = words[0].to_ascii_lowercase();
        for w in &words[1..] {
            let mut chars = w.chars();
            if let Some(first) = chars.next() {
                name.push(first.to_ascii_uppercase());
                name.extend(chars.map(|c| c.to_ascii_lowercase()));
            }
        }
        if !expr_hint.is_empty() {
            let mut chars = expr_hint.chars();
            if let Some(first) = chars.next() {
                name.push(first.to_ascii_uppercase());
                name.extend(chars);
            }
        } else {
            name.push_str("Text");
        }
        name
    } else if !expr_hint.is_empty() {
        format!("{}Text", expr_hint)
    } else {
        "textContent".to_string()
    }
}

/// Check if an `AnalyzedBinding` came from `defineProps` via its initializer.
fn is_define_props_binding(binding: &verter_semantic::analysis::types::AnalyzedBinding) -> bool {
    matches!(
        &binding.initializer,
        Some(BindingInitializer::FunctionCall {
            vue_api: Some(VueApiClassification::DefineProps),
            ..
        })
    )
}

/// Info about how to access props in generated code.
struct PropsInfo {
    accessor_name: String,
    needs_wrapping: bool,
    macro_span: Option<Span>,
}

/// Find how to access props: check macros for existing binding name or determine we need wrapping.
fn find_props_accessor(script: &ScriptAnalysisSnapshot) -> Option<PropsInfo> {
    let with_defaults = script
        .macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::WithDefaults);
    let define_props = script
        .macros
        .iter()
        .find(|m| m.kind == AnalyzedMacroKind::DefineProps);

    let primary = with_defaults.or(define_props)?;

    if let Some(ref name) = primary.binding_name {
        Some(PropsInfo {
            accessor_name: name.clone(),
            needs_wrapping: false,
            macro_span: None,
        })
    } else {
        let name = choose_props_name(script);
        Some(PropsInfo {
            accessor_name: name,
            needs_wrapping: true,
            macro_span: Some(primary.span),
        })
    }
}

/// Choose a non-conflicting name for the props accessor variable.
fn choose_props_name(script: &ScriptAnalysisSnapshot) -> String {
    let taken: std::collections::HashSet<&str> =
        script.bindings.iter().map(|b| b.name.as_str()).collect();

    for candidate in &["props", "_props", "componentProps"] {
        if !taken.contains(candidate) {
            return candidate.to_string();
        }
    }
    for i in 0u32.. {
        let name = format!("props{}", i);
        if !taken.contains(name.as_str()) {
            return name;
        }
    }
    unreachable!()
}

/// Replace an identifier at word boundaries within an expression.
fn replace_ident_at_word_boundary(expr: &str, ident: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(expr.len() + replacement.len());
    let bytes = expr.as_bytes();
    let ident_bytes = ident.as_bytes();
    let ident_len = ident.len();
    let mut i = 0;

    while i < bytes.len() {
        if i + ident_len <= bytes.len() && &bytes[i..i + ident_len] == ident_bytes {
            let before_ok = i == 0
                || !(bytes[i - 1].is_ascii_alphanumeric()
                    || bytes[i - 1] == b'_'
                    || bytes[i - 1] == b'$');
            let after_ok = i + ident_len >= bytes.len()
                || !(bytes[i + ident_len].is_ascii_alphanumeric()
                    || bytes[i + ident_len] == b'_'
                    || bytes[i + ident_len] == b'$');
            if before_ok && after_ok {
                result.push_str(replacement);
                i += ident_len;
                continue;
            }
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

/// Check if an identifier matches a prop field in any DefineProps macro.
fn is_prop_field(ident: &str, script: &ScriptAnalysisSnapshot) -> bool {
    script.macros.iter().any(|m| {
        m.kind == AnalyzedMacroKind::DefineProps && m.prop_fields.iter().any(|f| f.name == ident)
    })
}

/// Build the extraction action for an element with bare text.
fn build_extract_action(
    el: &TemplateElement,
    template: &TemplateAnalysisSnapshot,
    script: Option<&ScriptAnalysisSnapshot>,
    source: &str,
) -> Vec<CodeAction> {
    if el.text_children.is_empty() {
        return vec![];
    }

    // Find script setup end insertion point
    let Some(script_insert_pos) = find_script_end_insert_pos(source) else {
        return vec![];
    };

    let scoped_vars = collect_scoped_vars(el, &template.elements);
    let has_scoped = !scoped_vars.is_empty()
        && el.text_children.iter().any(|seg| {
            if let TemplateTextSegment::Interpolation {
                expression_span, ..
            } = seg
            {
                let s = expression_span.start as usize;
                let e = expression_span.end as usize;
                if s < source.len() && e <= source.len() {
                    let expr = source[s..e].trim();
                    extract_identifiers(expr)
                        .iter()
                        .any(|id| scoped_vars.contains(*id))
                } else {
                    false
                }
            } else {
                false
            }
        });

    // Resolve props accessor info (if defineProps is present)
    let props_info = script.and_then(find_props_accessor);

    let name = generate_name(el, source);

    // Build template literal body from text_children
    let mut template_parts = String::new();
    let mut needs_value_unwrap = Vec::new(); // (ident, ReactivityKind)
    let mut needs_unref = false;
    let mut scoped_params: Vec<String> = Vec::new();
    let mut used_props = false;

    for seg in &el.text_children {
        match seg {
            TemplateTextSegment::Text { span, .. } => {
                let s = span.start as usize;
                let e = span.end as usize;
                if s < source.len() && e <= source.len() {
                    let text = &source[s..e];
                    for c in text.chars() {
                        if c == '`' {
                            template_parts.push_str("\\`");
                        } else {
                            template_parts.push(c);
                        }
                    }
                }
            }
            TemplateTextSegment::Interpolation {
                expression_span, ..
            } => {
                let s = expression_span.start as usize;
                let e = expression_span.end as usize;
                if s < source.len() && e <= source.len() {
                    let expr = source[s..e].trim();
                    let idents = extract_identifiers(expr);

                    if has_scoped {
                        // Function mode: scoped vars become params
                        let mut transformed_expr = expr.to_string();
                        for id in &idents {
                            if scoped_vars.contains(*id) && !scoped_params.contains(&id.to_string())
                            {
                                scoped_params.push(id.to_string());
                            } else if let Some(script) = script {
                                // Non-scoped ident: check if it's a prop
                                let binding = script.bindings.iter().find(|b| b.name == *id);
                                if binding.is_some() {
                                    // Script binding wins — no prop prefix
                                } else if let Some(ref pi) = props_info {
                                    if is_prop_field(id, script) {
                                        let replacement = format!("{}.{}", pi.accessor_name, id);
                                        transformed_expr = replace_ident_at_word_boundary(
                                            &transformed_expr,
                                            id,
                                            &replacement,
                                        );
                                        used_props = true;
                                    }
                                }
                            }
                        }
                        template_parts.push_str(&format!("${{{}}}", transformed_expr));
                    } else {
                        // Computed mode: apply .value / unref() / props as needed
                        let mut transformed_expr = expr.to_string();
                        if let Some(script) = script {
                            for id in &idents {
                                if let Some(binding) =
                                    script.bindings.iter().find(|b| b.name == *id)
                                {
                                    // Script binding found — ALWAYS wins
                                    if is_define_props_binding(binding) {
                                        // Destructured prop → raw ident, no .value
                                        continue;
                                    }
                                    match binding.reactivity_kind {
                                        ReactivityKind::Ref | ReactivityKind::Computed => {
                                            if expr.trim() == *id {
                                                transformed_expr = format!("{}.value", id);
                                            }
                                            needs_value_unwrap
                                                .push((id.to_string(), binding.reactivity_kind));
                                        }
                                        ReactivityKind::MaybeRef => {
                                            if expr.trim() == *id {
                                                transformed_expr = format!("unref({})", id);
                                            }
                                            needs_unref = true;
                                        }
                                        _ => {}
                                    }
                                } else if let Some(ref pi) = props_info {
                                    // No script binding — check prop fields
                                    if is_prop_field(id, script) {
                                        let replacement = format!("{}.{}", pi.accessor_name, id);
                                        transformed_expr = replace_ident_at_word_boundary(
                                            &transformed_expr,
                                            id,
                                            &replacement,
                                        );
                                        used_props = true;
                                    }
                                }
                            }
                        }
                        template_parts.push_str(&format!("${{{}}}", transformed_expr));
                    }
                }
            }
        }
    }

    // Build edits
    let mut edits = Vec::new();

    // 1. Template edit: replace content area with {{ name }} or {{ name(args) }}
    let content_start = el.tag_span_end;
    let content_end = el.content_end;
    let template_replacement = if has_scoped {
        let args = scoped_params.join(", ");
        format!("{{{{ {}({}) }}}}", name, args)
    } else {
        format!("{{{{ {} }}}}", name)
    };
    edits.push(FileEdit {
        file_id: None,
        replacement: template_replacement,
        span: Span::new(content_start, content_end),
    });

    // 2. Script edit: insert computed/function before </script>
    let script_body = if has_scoped {
        let params = scoped_params.join(", ");
        format!(
            "\nfunction {}({}) {{\n  return `{}`\n}}\n",
            name, params, template_parts
        )
    } else {
        format!("\nconst {} = computed(() => `{}`)\n", name, template_parts)
    };
    edits.push(FileEdit {
        file_id: None,
        replacement: script_body,
        span: Span::new(script_insert_pos, script_insert_pos),
    });

    // 3. Import edit: add computed/unref to vue import if needed
    if !has_scoped {
        if let Some(edit) = find_or_extend_vue_import(source, script, "computed") {
            edits.push(edit);
        }
    }
    if needs_unref {
        if let Some(edit) = find_or_extend_vue_import(source, script, "unref") {
            edits.push(edit);
        }
    }

    // 4. Wrapping edit: if bare defineProps needs `const X = ` prepended
    if used_props {
        if let Some(ref pi) = props_info {
            if pi.needs_wrapping {
                if let Some(macro_span) = pi.macro_span {
                    let wrap_text = format!("const {} = ", pi.accessor_name);
                    edits.push(FileEdit {
                        file_id: None,
                        replacement: wrap_text,
                        span: Span::new(macro_span.start, macro_span.start),
                    });
                }
            }
        }
    }

    let title = if has_scoped {
        format!("Extract to function `{}`", name)
    } else {
        format!("Extract to computed `{}`", name)
    };

    vec![CodeAction {
        title,
        kind: ActionKind::Refactor,
        edits,
        is_preferred: true,
        diagnostic_rule: Some("no-bare-strings-in-template".to_string()),
        safety: AutofixSafety::Caution,
    }]
}

/// Find byte position just before `</script` in the source.
fn find_script_end_insert_pos(source: &str) -> Option<u32> {
    // Find </script — we want to insert before the newline preceding it
    let idx = source.find("</script")?;
    // Back up past any trailing whitespace/newline before </script
    let mut pos = idx;
    while pos > 0 && source.as_bytes()[pos - 1].is_ascii_whitespace() {
        pos -= 1;
    }
    // Insert after the last non-whitespace character before </script
    Some(pos as u32)
}

/// Find or extend a vue import to include `specifier`.
/// Returns `None` if the specifier is already imported.
fn find_or_extend_vue_import(
    source: &str,
    script: Option<&ScriptAnalysisSnapshot>,
    specifier: &str,
) -> Option<FileEdit> {
    let script = script?;

    // Check if already imported from 'vue'
    for imp in &script.imports {
        if imp.source == "vue" {
            if imp.bindings.iter().any(|b| b.name == specifier) {
                return None; // Already imported
            }

            // Extend existing vue import: find the `}` of the import and insert before it
            let imp_text = &source
                [imp.span.start as usize..std::cmp::min(imp.span.end as usize, source.len())];
            // Find `}` in the import text
            if let Some(brace_pos) = imp_text.rfind('}') {
                let insert_pos = imp.span.start as usize + brace_pos;
                // Check if there's already content (add comma + space)
                let before_brace = imp_text[..brace_pos].trim_end();
                let needs_comma = !before_brace.ends_with(',') && !before_brace.ends_with('{');
                let prefix = if needs_comma { ", " } else { " " };
                return Some(FileEdit {
                    file_id: None,
                    replacement: format!("{}{} ", prefix, specifier),
                    span: Span::new(insert_pos as u32, insert_pos as u32),
                });
            }
        }
    }

    // No vue import exists — create one at the top of the script
    // Find the start of the script content (after <script setup...>)
    let script_tag_end = source.find("<script")?;
    let after_tag = source[script_tag_end..].find('>')?;
    let insert_pos = (script_tag_end + after_tag + 1) as u32;

    Some(FileEdit {
        file_id: None,
        replacement: format!("\nimport {{ {} }} from 'vue'", specifier),
        span: Span::new(insert_pos, insert_pos),
    })
}

#[cfg(test)]
#[path = "extract_bare_text_tests.rs"]
mod extract_bare_text_tests;
