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

use verter_analysis::template::{TemplateAnalysisSnapshot, TemplateElement, TemplateTextSegment};
use verter_analysis::types::{
    AnalyzedMacroKind, BindingInitializer, ReactivityKind, ScriptAnalysisSnapshot,
    VueApiClassification,
};
use verter_diagnostics::LintDiagnostic;
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
fn is_define_props_binding(binding: &verter_analysis::types::AnalyzedBinding) -> bool {
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
mod tests {
    use super::*;
    use crate::provider::ActionContext;
    use verter_analysis::template::*;
    use verter_analysis::types::*;
    use verter_diagnostics::{DiagnosticSet, DiagnosticSpanKind, LintDiagnostic, Severity};

    fn make_diag(start: u32, end: u32) -> LintDiagnostic {
        LintDiagnostic {
            rule: "no-bare-strings-in-template".to_string(),
            category: "vue-recommended".to_string(),
            message: "test".to_string(),
            span: Span::new(start, end),
            severity: Severity::Hint,
            span_kind: DiagnosticSpanKind::ElementContent,
            tags: vec![],
            certainty: verter_diagnostics::Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        }
    }

    fn make_element(
        tag: &str,
        tag_span_end: u32,
        content_end: u32,
        text_children: Vec<TemplateTextSegment>,
    ) -> TemplateElement {
        TemplateElement {
            tag: tag.to_string(),
            has_bare_text: true,
            has_text_content: true,
            tag_span_end,
            content_end,
            text_children,
            span: Span::new(0, content_end + 10),
            ..Default::default()
        }
    }

    fn make_script_with_bindings(bindings: Vec<AnalyzedBinding>) -> ScriptAnalysisSnapshot {
        ScriptAnalysisSnapshot {
            bindings,
            ..Default::default()
        }
    }

    fn make_binding(name: &str, kind: ReactivityKind) -> AnalyzedBinding {
        AnalyzedBinding {
            name: name.to_string(),
            kind: AnalyzedBindingKind::Const,
            is_reactive: matches!(kind, ReactivityKind::Ref | ReactivityKind::Computed),
            reactivity_kind: kind,
            type_annotation: None,
            initializer: None,
            span: Span::new(0, 0),
            used_in_script: false,
            used_in_style: false,
        }
    }

    // ── Test 1: Simple text + interpolation → computed ──

    #[test]
    fn simple_text_and_interpolation_extracts_to_computed() {
        //                   0         1         2         3         4         5         6         7         8         9
        //                   0123456789012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789
        let source = concat!(
            r#"<script setup lang="ts">"#, // 0..23
            "\n",                          // 23..24
            "import { ref } from 'vue'\n", // 24..50
            "const count = ref(0)\n",      // 50..71
            "</script>\n",                 // 71..81
            "<template>\n",                // 81..92
            "<p>",                         // 92..95
            "Count: {{ count }}",          // 95..113
            "</p>\n",                      // 113..118
            "</template>",                 // 118..129
        );

        let tag_span_end = 95u32; // after <p>
        let content_end = 113u32; // before </p>

        // "Count: " is 95..103, "{{ count }}" is 103..113
        // expression_span for "count" is inner: " count " → trim → "count" at 106..111
        let text_children = vec![
            TemplateTextSegment::Text {
                span: Span::new(95, 103),
                is_entity: false,
            },
            TemplateTextSegment::Interpolation {
                span: Span::new(103, 113),
                expression_span: Span::new(106, 111),
            },
        ];

        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element("p", tag_span_end, content_end, text_children)],
            ..Default::default()
        };

        let script = make_script_with_bindings(vec![make_binding("count", ReactivityKind::Ref)]);

        let vue_import = AnalyzedImport {
            source: "vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "ref".to_string(),
                is_type_only: false,
                vue_api: None,
                span: Span::new(0, 0),
            }],
            span: Span::new(24, 49),
            resolved_canonical_id: None,
        };
        let script = ScriptAnalysisSnapshot {
            imports: vec![vue_import],
            ..script
        };

        let diag = make_diag(tag_span_end, content_end);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &set,
            template: Some(&template),
            script: Some(&script),
            styles: &[],
        };

        let actions = ExtractBareText.fixes_for_diagnostic(&diag, &ctx);

        // Positive assertions
        assert_eq!(actions.len(), 1, "should produce one action");
        assert!(
            actions[0].title.contains("computed"),
            "title should mention computed: got '{}'",
            actions[0].title
        );

        // Template edit: replaces content with {{ name }}
        let template_edit = &actions[0].edits[0];
        assert_eq!(template_edit.span.start, tag_span_end);
        assert_eq!(template_edit.span.end, content_end);
        assert!(
            template_edit.replacement.starts_with("{{"),
            "template replacement should be interpolation: got '{}'",
            template_edit.replacement
        );
        assert!(
            !template_edit.replacement.contains("Count:"),
            "template replacement should not contain original text"
        );

        // Script edit: inserts computed
        let script_edit = &actions[0].edits[1];
        assert!(
            script_edit.replacement.contains("computed("),
            "script edit should contain computed(): got '{}'",
            script_edit.replacement
        );
        assert!(
            script_edit.replacement.contains("count.value"),
            "Ref binding should use .value: got '{}'",
            script_edit.replacement
        );
        assert!(
            script_edit.replacement.contains("Count: "),
            "template literal should contain original text: got '{}'",
            script_edit.replacement
        );

        // Import edit: should add computed to existing vue import
        assert!(
            actions[0].edits.len() >= 3,
            "should have import edit: got {} edits",
            actions[0].edits.len()
        );
        let import_edit = &actions[0].edits[2];
        assert!(
            import_edit.replacement.contains("computed"),
            "import edit should add computed: got '{}'",
            import_edit.replacement
        );

        // Negative assertions
        assert!(
            !actions[0].title.contains("function"),
            "should NOT suggest function for non-scoped"
        );
        assert_eq!(
            actions[0].diagnostic_rule.as_deref(),
            Some("no-bare-strings-in-template"),
            "should reference the diagnostic rule"
        );
    }

    // ── Test 2: v-for scoped variable → function ──

    #[test]
    fn vfor_scoped_var_extracts_to_function() {
        let source = concat!(
            r#"<script setup lang="ts">"#,
            "\n",
            "import { ref } from 'vue'\n",
            "const items = ref([1, 2, 3])\n",
            "</script>\n",
            "<template>\n",
            r#"<ul><li v-for="item in items">"#,
            "Item: {{ item }}",
            "</li></ul>\n",
            "</template>",
        );

        let li_start = source.find("<li").unwrap() as u32;
        let tag_span_end = source.find(">Item").unwrap() as u32 + 1;
        let content_end = source.find("</li>").unwrap() as u32;

        let item_text_start = tag_span_end;
        let item_text_end = source.find("{{ item }}").unwrap() as u32;
        let interp_start = item_text_end;
        let interp_end = interp_start + 10; // "{{ item }}"
        let expr_start = interp_start + 3;
        let expr_end = expr_start + 4; // "item"

        let text_children = vec![
            TemplateTextSegment::Text {
                span: Span::new(item_text_start, item_text_end),
                is_entity: false,
            },
            TemplateTextSegment::Interpolation {
                span: Span::new(interp_start, interp_end),
                expression_span: Span::new(expr_start, expr_end),
            },
        ];

        let mut el = make_element("li", tag_span_end, content_end, text_children);
        el.v_for = Some(VForDirective {
            variable: "item".to_string(),
            index: None,
            iterable: "items".to_string(),
            has_key: false,
            key_expression: None,
            key_uses_index: false,
            span: Span::new(li_start + 4, li_start + 30),
        });
        el.span = Span::new(li_start, content_end + 5);

        let template = TemplateAnalysisSnapshot {
            elements: vec![el],
            ..Default::default()
        };

        let script = make_script_with_bindings(vec![]);
        let diag = make_diag(tag_span_end, content_end);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &set,
            template: Some(&template),
            script: Some(&script),
            styles: &[],
        };

        let actions = ExtractBareText.fixes_for_diagnostic(&diag, &ctx);

        assert_eq!(actions.len(), 1);
        assert!(
            actions[0].title.contains("function"),
            "title should mention function for scoped var: got '{}'",
            actions[0].title
        );

        // Script edit: should be a function, not computed
        let script_edit = &actions[0].edits[1];
        assert!(
            script_edit.replacement.contains("function"),
            "should generate function: got '{}'",
            script_edit.replacement
        );
        assert!(
            script_edit.replacement.contains("item"),
            "function should take scoped var as param: got '{}'",
            script_edit.replacement
        );

        // Template edit: should call function with args
        let template_edit = &actions[0].edits[0];
        assert!(
            template_edit.replacement.contains("(item)"),
            "template should call function with arg: got '{}'",
            template_edit.replacement
        );

        // Negative
        assert!(
            !script_edit.replacement.contains("computed"),
            "should NOT use computed for scoped vars"
        );
        assert!(
            !script_edit.replacement.contains(".value"),
            "scoped vars should NOT have .value"
        );
    }

    // ── Test 3: Reactive binding → no .value ──

    #[test]
    fn reactive_binding_no_dot_value() {
        let source = concat!(
            r#"<script setup lang="ts">"#,
            "\n",
            "import { reactive } from 'vue'\n",
            "const state = reactive({ count: 0 })\n",
            "</script>\n",
            "<template>\n",
            "<p>Value: {{ state.count }}</p>\n",
            "</template>",
        );

        let tag_span_end = source.find(">Value").unwrap() as u32 + 1;
        let content_end = source.find("</p>").unwrap() as u32;
        let interp_start = source.find("{{ state.count }}").unwrap() as u32;
        let interp_end = interp_start + 17;
        let expr_start = interp_start + 3;
        let expr_end = interp_end - 3;

        let text_children = vec![
            TemplateTextSegment::Text {
                span: Span::new(tag_span_end, interp_start),
                is_entity: false,
            },
            TemplateTextSegment::Interpolation {
                span: Span::new(interp_start, interp_end),
                expression_span: Span::new(expr_start, expr_end),
            },
        ];

        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element("p", tag_span_end, content_end, text_children)],
            ..Default::default()
        };

        let vue_import = AnalyzedImport {
            source: "vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "reactive".to_string(),
                is_type_only: false,
                vue_api: None,
                span: Span::new(0, 0),
            }],
            span: Span::new(24, 55),
            resolved_canonical_id: None,
        };
        let script = ScriptAnalysisSnapshot {
            imports: vec![vue_import],
            bindings: vec![make_binding("state", ReactivityKind::Reactive)],
            ..Default::default()
        };

        let diag = make_diag(tag_span_end, content_end);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &set,
            template: Some(&template),
            script: Some(&script),
            styles: &[],
        };

        let actions = ExtractBareText.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);

        let script_edit = &actions[0].edits[1];
        assert!(
            script_edit.replacement.contains("state.count"),
            "reactive binding should use direct access: got '{}'",
            script_edit.replacement
        );
        assert!(
            !script_edit.replacement.contains("state.value"),
            "reactive binding should NOT use .value"
        );
        assert!(
            !script_edit.replacement.contains("unref"),
            "reactive binding should NOT use unref()"
        );
    }

    // ── Test 4: MaybeRef binding → unref() ──

    #[test]
    fn maybe_ref_binding_uses_unref() {
        let source = concat!(
            r#"<script setup lang="ts">"#,
            "\n",
            "import { ref } from 'vue'\n",
            "const data = useFetch('/api')\n",
            "</script>\n",
            "<template>\n",
            "<p>Result: {{ data }}</p>\n",
            "</template>",
        );

        let tag_span_end = source.find(">Result").unwrap() as u32 + 1;
        let content_end = source.find("</p>").unwrap() as u32;
        let interp_start = source.find("{{ data }}").unwrap() as u32;
        let interp_end = interp_start + 10;
        let expr_start = interp_start + 3;
        let expr_end = interp_end - 3;

        let text_children = vec![
            TemplateTextSegment::Text {
                span: Span::new(tag_span_end, interp_start),
                is_entity: false,
            },
            TemplateTextSegment::Interpolation {
                span: Span::new(interp_start, interp_end),
                expression_span: Span::new(expr_start, expr_end),
            },
        ];

        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element("p", tag_span_end, content_end, text_children)],
            ..Default::default()
        };

        let vue_import = AnalyzedImport {
            source: "vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "ref".to_string(),
                is_type_only: false,
                vue_api: None,
                span: Span::new(0, 0),
            }],
            span: Span::new(24, 49),
            resolved_canonical_id: None,
        };
        let script = ScriptAnalysisSnapshot {
            imports: vec![vue_import],
            bindings: vec![make_binding("data", ReactivityKind::MaybeRef)],
            ..Default::default()
        };

        let diag = make_diag(tag_span_end, content_end);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &set,
            template: Some(&template),
            script: Some(&script),
            styles: &[],
        };

        let actions = ExtractBareText.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);

        let script_edit = &actions[0].edits[1];
        assert!(
            script_edit.replacement.contains("unref(data)"),
            "MaybeRef should use unref(): got '{}'",
            script_edit.replacement
        );

        // Should have unref import edit
        let has_unref_import = actions[0]
            .edits
            .iter()
            .any(|e| e.replacement.contains("unref"));
        assert!(has_unref_import, "should add unref to vue import");

        // Negative
        assert!(
            !script_edit.replacement.contains("data.value"),
            "MaybeRef should NOT use .value"
        );
    }

    // ── Test 5: Pure text (no interpolation) → computed with string ──

    #[test]
    fn pure_text_extracts_to_computed() {
        let source = concat!(
            r#"<script setup lang="ts">"#,
            "\n",
            "</script>\n",
            "<template>\n",
            "<p>Hello World</p>\n",
            "</template>",
        );

        let tag_span_end = source.find(">Hello").unwrap() as u32 + 1;
        let content_end = source.find("</p>").unwrap() as u32;

        let text_children = vec![TemplateTextSegment::Text {
            span: Span::new(tag_span_end, content_end),
            is_entity: false,
        }];

        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element("p", tag_span_end, content_end, text_children)],
            ..Default::default()
        };

        let script = make_script_with_bindings(vec![]);
        let diag = make_diag(tag_span_end, content_end);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &set,
            template: Some(&template),
            script: Some(&script),
            styles: &[],
        };

        let actions = ExtractBareText.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);

        let script_edit = &actions[0].edits[1];
        assert!(
            script_edit.replacement.contains("computed("),
            "pure text should use computed: got '{}'",
            script_edit.replacement
        );
        assert!(
            script_edit.replacement.contains("Hello World"),
            "computed should contain text: got '{}'",
            script_edit.replacement
        );
    }

    // ── Test 6: Ignores unrelated diagnostic ──

    #[test]
    fn ignores_unrelated_rule() {
        let diag = LintDiagnostic {
            rule: "no-v-html".to_string(),
            category: "vue-essential".to_string(),
            message: "test".to_string(),
            span: Span::new(0, 10),
            severity: Severity::Warning,
            span_kind: DiagnosticSpanKind::Directive,
            tags: vec![],
            certainty: verter_diagnostics::Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        };
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source: "<template><div /></template>",
            file_id: "test.vue",
            diagnostics: &set,
            template: None,
            script: None,
            styles: &[],
        };
        let actions = ExtractBareText.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "should not produce actions for unrelated rule"
        );
    }

    // ── Test 7: Cursor-based trigger ──

    #[test]
    fn actions_at_cursor_in_content() {
        let source = concat!(
            r#"<script setup lang="ts">"#,
            "\n",
            "</script>\n",
            "<template>\n",
            "<p>Hello World</p>\n",
            "</template>",
        );

        let tag_span_end = source.find(">Hello").unwrap() as u32 + 1;
        let content_end = source.find("</p>").unwrap() as u32;
        let cursor = tag_span_end + 3; // inside "Hello World"

        let text_children = vec![TemplateTextSegment::Text {
            span: Span::new(tag_span_end, content_end),
            is_entity: false,
        }];

        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element("p", tag_span_end, content_end, text_children)],
            ..Default::default()
        };

        let script = make_script_with_bindings(vec![]);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &set,
            template: Some(&template),
            script: Some(&script),
            styles: &[],
        };

        let actions = ExtractBareText.actions_at(cursor, &ctx);
        assert_eq!(actions.len(), 1, "cursor in content should trigger action");

        // Cursor outside content should not trigger
        let actions = ExtractBareText.actions_at(tag_span_end - 5, &ctx);
        assert!(
            actions.is_empty(),
            "cursor outside content should NOT trigger"
        );
    }

    // ── Test 8: Name generation ──

    #[test]
    fn name_generation_heuristic() {
        let source = "Count: {{ count }}";
        let el = make_element(
            "p",
            0,
            18,
            vec![
                TemplateTextSegment::Text {
                    span: Span::new(0, 8),
                    is_entity: false,
                },
                TemplateTextSegment::Interpolation {
                    span: Span::new(8, 18),
                    expression_span: Span::new(11, 16),
                },
            ],
        );
        let name = generate_name(&el, source);
        assert!(
            name.contains("count") || name.contains("Count"),
            "name should derive from content: got '{}'",
            name
        );
        assert!(!name.is_empty(), "name should not be empty");
    }

    // ── Test 9: Import management — computed already imported ──

    #[test]
    fn no_duplicate_computed_import() {
        let source = concat!(
            r#"<script setup lang="ts">"#,
            "\n",
            "import { ref, computed } from 'vue'\n",
            "const count = ref(0)\n",
            "</script>\n",
            "<template>\n",
            "<p>Count: {{ count }}</p>\n",
            "</template>",
        );

        let tag_span_end = source.find(">Count").unwrap() as u32 + 1;
        let content_end = source.find("</p>").unwrap() as u32;
        let interp_start = source.find("{{ count }}").unwrap() as u32;
        let interp_end = interp_start + 11;
        let expr_start = interp_start + 3;
        let expr_end = interp_end - 3;

        let text_children = vec![
            TemplateTextSegment::Text {
                span: Span::new(tag_span_end, interp_start),
                is_entity: false,
            },
            TemplateTextSegment::Interpolation {
                span: Span::new(interp_start, interp_end),
                expression_span: Span::new(expr_start, expr_end),
            },
        ];

        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element("p", tag_span_end, content_end, text_children)],
            ..Default::default()
        };

        let vue_import = AnalyzedImport {
            source: "vue".to_string(),
            is_type_only: false,
            bindings: vec![
                AnalyzedImportBinding {
                    name: "ref".to_string(),
                    is_type_only: false,
                    vue_api: None,
                    span: Span::new(0, 0),
                },
                AnalyzedImportBinding {
                    name: "computed".to_string(),
                    is_type_only: false,
                    vue_api: None,
                    span: Span::new(0, 0),
                },
            ],
            span: Span::new(24, 59),
            resolved_canonical_id: None,
        };
        let script = ScriptAnalysisSnapshot {
            imports: vec![vue_import],
            bindings: vec![make_binding("count", ReactivityKind::Ref)],
            ..Default::default()
        };

        let diag = make_diag(tag_span_end, content_end);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &set,
            template: Some(&template),
            script: Some(&script),
            styles: &[],
        };

        let actions = ExtractBareText.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);

        // Should have exactly 2 edits (template + script), no import edit
        assert_eq!(
            actions[0].edits.len(),
            2,
            "should NOT add import edit when computed already imported: got {} edits",
            actions[0].edits.len()
        );
    }

    // ── Test 10: Complex expression preserved ──

    #[test]
    fn complex_expression_preserved() {
        let source = concat!(
            r#"<script setup lang="ts">"#,
            "\n",
            "import { ref } from 'vue'\n",
            "const items = ref([1, 2, 3])\n",
            "</script>\n",
            "<template>\n",
            "<p>Total: {{ items.filter(x => x > 0).length }}</p>\n",
            "</template>",
        );

        let tag_span_end = source.find(">Total").unwrap() as u32 + 1;
        let content_end = source.find("</p>").unwrap() as u32;
        let interp_start = source.find("{{ items").unwrap() as u32;
        let interp_end = source.find("}} ").unwrap_or(source.find("}}</p>").unwrap()) as u32 + 2;
        let expr_start = interp_start + 3;
        let expr_end = interp_end - 3;

        let text_children = vec![
            TemplateTextSegment::Text {
                span: Span::new(tag_span_end, interp_start),
                is_entity: false,
            },
            TemplateTextSegment::Interpolation {
                span: Span::new(interp_start, interp_end),
                expression_span: Span::new(expr_start, expr_end),
            },
        ];

        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element("p", tag_span_end, content_end, text_children)],
            ..Default::default()
        };

        let vue_import = AnalyzedImport {
            source: "vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "ref".to_string(),
                is_type_only: false,
                vue_api: None,
                span: Span::new(0, 0),
            }],
            span: Span::new(24, 49),
            resolved_canonical_id: None,
        };
        let script = ScriptAnalysisSnapshot {
            imports: vec![vue_import],
            bindings: vec![make_binding("items", ReactivityKind::Ref)],
            ..Default::default()
        };

        let diag = make_diag(tag_span_end, content_end);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &set,
            template: Some(&template),
            script: Some(&script),
            styles: &[],
        };

        let actions = ExtractBareText.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);

        let script_edit = &actions[0].edits[1];
        // Complex expression should be preserved (not just "items")
        assert!(
            script_edit.replacement.contains("filter"),
            "complex expression should be preserved: got '{}'",
            script_edit.replacement
        );
    }

    // ── Test 11: v-slot scoped variable → function ──

    #[test]
    fn vslot_scoped_var_extracts_to_function() {
        let source = concat!(
            r#"<script setup lang="ts">"#,
            "\n",
            "</script>\n",
            "<template>\n",
            r#"<MyComp v-slot="{ item }">"#,
            "<p>",
            "Name: {{ item }}",
            "</p>",
            "</MyComp>\n",
            "</template>",
        );

        let p_start = source.find("<p>").unwrap() as u32;
        let tag_span_end = p_start + 3;
        let content_end = source.find("</p>").unwrap() as u32;
        let interp_start = source.find("{{ item }}").unwrap() as u32;
        let interp_end = interp_start + 10;
        let expr_start = interp_start + 3;
        let expr_end = expr_start + 4;

        let text_children = vec![
            TemplateTextSegment::Text {
                span: Span::new(tag_span_end, interp_start),
                is_entity: false,
            },
            TemplateTextSegment::Interpolation {
                span: Span::new(interp_start, interp_end),
                expression_span: Span::new(expr_start, expr_end),
            },
        ];

        // Parent element (MyComp) with v-slot
        let mycomp_start = source.find("<MyComp").unwrap() as u32;
        let parent_el = TemplateElement {
            tag: "MyComp".to_string(),
            is_component: true,
            directives: vec![TemplateDirective {
                name: "slot".to_string(),
                raw_name: "v-slot".to_string(),
                argument: None,
                modifiers: vec![],
                expression: Some("{ item }".to_string()),
                span: Span::new(mycomp_start + 8, mycomp_start + 26),
                name_end: 0,
                arg_span: None,
                expression_span: None,
                modifier_spans: Vec::new(),
            }],
            span: Span::new(mycomp_start, content_end + 20),
            tag_span_end: mycomp_start + 26,
            content_end: content_end + 4,
            ..Default::default()
        };

        let mut child_el = make_element("p", tag_span_end, content_end, text_children);
        child_el.parent_index = Some(0); // parent is MyComp at index 0

        let template = TemplateAnalysisSnapshot {
            elements: vec![parent_el, child_el],
            ..Default::default()
        };

        let script = make_script_with_bindings(vec![]);
        let diag = make_diag(tag_span_end, content_end);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &set,
            template: Some(&template),
            script: Some(&script),
            styles: &[],
        };

        let actions = ExtractBareText.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);

        assert!(
            actions[0].title.contains("function"),
            "v-slot scoped var should use function: got '{}'",
            actions[0].title
        );

        let script_edit = &actions[0].edits[1];
        assert!(
            script_edit.replacement.contains("function"),
            "should generate function: got '{}'",
            script_edit.replacement
        );
        assert!(
            script_edit.replacement.contains("item"),
            "function should take scoped var as param: got '{}'",
            script_edit.replacement
        );

        // Negative
        assert!(
            !script_edit.replacement.contains("computed"),
            "should NOT use computed for scoped vars"
        );
    }

    // ── Test 12: No script setup → no action ──

    #[test]
    fn no_script_setup_no_action() {
        let source = "<template><p>Hello</p></template>";

        let tag_span_end = 13u32;
        let content_end = 18u32;

        let text_children = vec![TemplateTextSegment::Text {
            span: Span::new(tag_span_end, content_end),
            is_entity: false,
        }];

        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element("p", tag_span_end, content_end, text_children)],
            ..Default::default()
        };

        let diag = make_diag(tag_span_end, content_end);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &set,
            template: Some(&template),
            script: None,
            styles: &[],
        };

        let actions = ExtractBareText.fixes_for_diagnostic(&diag, &ctx);
        assert!(
            actions.is_empty(),
            "should not produce action without <script setup>"
        );
    }

    // ── Test helper: build macro ──

    fn make_macro(
        kind: AnalyzedMacroKind,
        binding_name: Option<&str>,
        prop_names: &[&str],
        span: Span,
    ) -> AnalyzedMacro {
        AnalyzedMacro {
            kind,
            is_type_based: true,
            type_references: vec![],
            binding_name: binding_name.map(|s| s.to_string()),
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: prop_names
                .iter()
                .map(|n| AnalyzedPropField {
                    name: n.to_string(),
                    span: Span::new(0, 0),
                    type_annotation: None,
                })
                .collect(),
            emit_fields: vec![],
            slot_fields: vec![],
            span,
        }
    }

    fn make_script_with_macros(
        bindings: Vec<AnalyzedBinding>,
        macros: Vec<AnalyzedMacro>,
        imports: Vec<AnalyzedImport>,
    ) -> ScriptAnalysisSnapshot {
        ScriptAnalysisSnapshot {
            bindings,
            macros,
            imports,
            ..Default::default()
        }
    }

    fn make_props_binding(name: &str, kind: AnalyzedBindingKind) -> AnalyzedBinding {
        AnalyzedBinding {
            name: name.to_string(),
            kind,
            is_reactive: false,
            reactivity_kind: ReactivityKind::None,
            type_annotation: None,
            initializer: Some(BindingInitializer::FunctionCall {
                callee: "defineProps".to_string(),
                callee_import_source: None,
                vue_api: Some(VueApiClassification::DefineProps),
            }),
            span: Span::new(0, 0),
            used_in_script: false,
            used_in_style: false,
        }
    }

    /// Build a simple test context for prop tests.
    /// Source: `<script setup lang="ts">\n{script_body}\n</script>\n<template>\n<p>{content}</p>\n</template>`
    fn prop_test_action(
        script_body: &str,
        content: &str,
        script: ScriptAnalysisSnapshot,
    ) -> Vec<CodeAction> {
        let source_str = format!(
            "<script setup lang=\"ts\">\n{}\n</script>\n<template>\n<p>{}</p>\n</template>",
            script_body, content
        );
        // Leak source to get 'static lifetime for test convenience
        let source: &'static str = Box::leak(source_str.into_boxed_str());

        let p_tag_start = source.find("<p>").unwrap() as u32;
        let tag_span_end = p_tag_start + 3;
        let content_end = source.find("</p>").unwrap() as u32;
        let content_text = &source[tag_span_end as usize..content_end as usize];

        // Parse text_children from content
        let mut text_children = Vec::new();
        let mut pos = 0usize;
        while pos < content_text.len() {
            if let Some(interp_rel) = content_text[pos..].find("{{") {
                // Text before interpolation
                if interp_rel > 0 {
                    text_children.push(TemplateTextSegment::Text {
                        span: Span::new(
                            tag_span_end + pos as u32,
                            tag_span_end + pos as u32 + interp_rel as u32,
                        ),
                        is_entity: false,
                    });
                }
                let interp_abs_start = pos + interp_rel;
                let close = content_text[interp_abs_start..].find("}}").unwrap();
                let interp_abs_end = interp_abs_start + close + 2;
                let expr_start = interp_abs_start + 3; // skip "{{ "
                let expr_end = interp_abs_start + close - 1; // before " }}"
                text_children.push(TemplateTextSegment::Interpolation {
                    span: Span::new(
                        tag_span_end + interp_abs_start as u32,
                        tag_span_end + interp_abs_end as u32,
                    ),
                    expression_span: Span::new(
                        tag_span_end + expr_start as u32,
                        tag_span_end + expr_end as u32,
                    ),
                });
                pos = interp_abs_end;
            } else {
                // Remaining text
                text_children.push(TemplateTextSegment::Text {
                    span: Span::new(
                        tag_span_end + pos as u32,
                        tag_span_end + content_text.len() as u32,
                    ),
                    is_entity: false,
                });
                break;
            }
        }

        let template = TemplateAnalysisSnapshot {
            elements: vec![make_element("p", tag_span_end, content_end, text_children)],
            ..Default::default()
        };

        let diag = make_diag(tag_span_end, content_end);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &set,
            template: Some(&template),
            script: Some(&script),
            styles: &[],
        };

        ExtractBareText.fixes_for_diagnostic(&diag, &ctx)
    }

    // ── Test 13: Bare defineProps<{ title }>(), {{ title }} → props.title ──

    #[test]
    fn bare_define_props_uses_props_accessor() {
        let script = make_script_with_macros(
            vec![],
            vec![make_macro(
                AnalyzedMacroKind::DefineProps,
                None, // bare — no binding name
                &["title"],
                Span::new(24, 55),
            )],
            vec![],
        );

        let actions = prop_test_action(
            "defineProps<{ title: string }>()",
            "Hello {{ title }}",
            script,
        );
        assert_eq!(actions.len(), 1, "should produce one action");

        let script_edit = &actions[0].edits[1];
        assert!(
            script_edit.replacement.contains("props.title"),
            "bare defineProps should use props.title: got '{}'",
            script_edit.replacement
        );
        assert!(
            !script_edit.replacement.contains(".value"),
            "props should NOT use .value: got '{}'",
            script_edit.replacement
        );
        // Should have wrapping edit (const props = before defineProps)
        let has_wrap = actions[0]
            .edits
            .iter()
            .any(|e| e.replacement.contains("const ") && e.replacement.contains(" = "));
        assert!(has_wrap, "bare defineProps should generate wrapping edit");
    }

    // ── Test 14: const props = defineProps<{ title }>(), {{ title }} → props.title ──

    #[test]
    fn named_define_props_uses_existing_accessor() {
        let script = make_script_with_macros(
            vec![make_props_binding("props", AnalyzedBindingKind::Const)],
            vec![make_macro(
                AnalyzedMacroKind::DefineProps,
                Some("props"),
                &["title"],
                Span::new(24, 65),
            )],
            vec![],
        );

        let actions = prop_test_action(
            "const props = defineProps<{ title: string }>()",
            "Hello {{ title }}",
            script,
        );
        assert_eq!(actions.len(), 1);

        let script_edit = &actions[0].edits[1];
        assert!(
            script_edit.replacement.contains("props.title"),
            "named defineProps should use props.title: got '{}'",
            script_edit.replacement
        );
        assert!(
            !script_edit.replacement.contains(".value"),
            "props should NOT use .value"
        );
        // Should NOT have wrapping edit
        let has_wrap = actions[0]
            .edits
            .iter()
            .any(|e| e.replacement.contains("const props = ") && e.span.start == e.span.end);
        assert!(
            !has_wrap,
            "named defineProps should NOT generate wrapping edit"
        );
    }

    // ── Test 15: const { title } = defineProps<{ title }>() → raw title ──

    #[test]
    fn destructured_define_props_uses_raw_ident() {
        let script = make_script_with_macros(
            vec![make_props_binding("title", AnalyzedBindingKind::Const)],
            vec![make_macro(
                AnalyzedMacroKind::DefineProps,
                None, // destructured — no single binding name
                &["title"],
                Span::new(24, 70),
            )],
            vec![],
        );

        let actions = prop_test_action(
            "const { title } = defineProps<{ title: string }>()",
            "Hello {{ title }}",
            script,
        );
        assert_eq!(actions.len(), 1);

        let script_edit = &actions[0].edits[1];
        // Destructured prop binding → raw identifier, no .value, no prefix
        assert!(
            script_edit.replacement.contains("title"),
            "destructured prop should use raw title: got '{}'",
            script_edit.replacement
        );
        assert!(
            !script_edit.replacement.contains("props.title"),
            "destructured prop should NOT use props.title"
        );
        assert!(
            !script_edit.replacement.contains(".value"),
            "destructured prop should NOT use .value"
        );
    }

    // ── Test 16: Bare defineProps, multiple props in expression ──

    #[test]
    fn bare_define_props_multiple_idents() {
        let script = make_script_with_macros(
            vec![],
            vec![make_macro(
                AnalyzedMacroKind::DefineProps,
                None,
                &["first", "last"],
                Span::new(24, 65),
            )],
            vec![],
        );

        let actions = prop_test_action(
            "defineProps<{ first: string, last: string }>()",
            "{{ first + ' ' + last }}",
            script,
        );
        assert_eq!(actions.len(), 1);

        let script_edit = &actions[0].edits[1];
        assert!(
            script_edit.replacement.contains("props.first"),
            "should prefix first with props.: got '{}'",
            script_edit.replacement
        );
        assert!(
            script_edit.replacement.contains("props.last"),
            "should prefix last with props.: got '{}'",
            script_edit.replacement
        );
    }

    // ── Test 17: Bare defineProps + ref → props.title + count.value ──

    #[test]
    fn bare_define_props_mixed_with_ref() {
        let script = make_script_with_macros(
            vec![make_binding("count", ReactivityKind::Ref)],
            vec![make_macro(
                AnalyzedMacroKind::DefineProps,
                None,
                &["title"],
                Span::new(24, 55),
            )],
            vec![AnalyzedImport {
                source: "vue".to_string(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "ref".to_string(),
                    is_type_only: false,
                    vue_api: None,
                    span: Span::new(0, 0),
                }],
                span: Span::new(24, 49),
                resolved_canonical_id: None,
            }],
        );

        let actions = prop_test_action(
            "defineProps<{ title: string }>()\nconst count = ref(0)",
            "{{ title }}: {{ count }}",
            script,
        );
        assert_eq!(actions.len(), 1);

        let script_edit = &actions[0].edits[1];
        assert!(
            script_edit.replacement.contains("props.title"),
            "prop should use props.title: got '{}'",
            script_edit.replacement
        );
        assert!(
            script_edit.replacement.contains("count.value"),
            "ref should use count.value: got '{}'",
            script_edit.replacement
        );
    }

    // ── Test 18: Bare withDefaults(defineProps<...>()) → wrap before withDefaults ──

    #[test]
    fn bare_with_defaults_wraps_correctly() {
        let script = make_script_with_macros(
            vec![],
            vec![
                make_macro(
                    AnalyzedMacroKind::WithDefaults,
                    None,
                    &[],
                    Span::new(24, 90), // outer withDefaults span
                ),
                make_macro(
                    AnalyzedMacroKind::DefineProps,
                    None,
                    &["title"],
                    Span::new(37, 70), // inner defineProps span
                ),
            ],
            vec![],
        );

        let actions = prop_test_action(
            "withDefaults(defineProps<{ title: string }>(), { title: 'hi' })",
            "Hello {{ title }}",
            script,
        );
        assert_eq!(actions.len(), 1);

        let script_edit = &actions[0].edits[1];
        assert!(
            script_edit.replacement.contains("props.title"),
            "withDefaults prop should use props.title: got '{}'",
            script_edit.replacement
        );
        // Should wrap before withDefaults span (24), not defineProps span
        let has_wrap = actions[0].edits.iter().any(|e| {
            e.replacement.contains("const ")
                && e.replacement.contains(" = ")
                && e.span.start == e.span.end
        });
        assert!(has_wrap, "bare withDefaults should generate wrapping edit");
    }

    // ── Test 19: const props = withDefaults(...) → no wrap ──

    #[test]
    fn named_with_defaults_no_wrap() {
        let script = make_script_with_macros(
            vec![make_props_binding("props", AnalyzedBindingKind::Const)],
            vec![
                make_macro(
                    AnalyzedMacroKind::WithDefaults,
                    Some("props"),
                    &[],
                    Span::new(24, 100),
                ),
                make_macro(
                    AnalyzedMacroKind::DefineProps,
                    None,
                    &["title"],
                    Span::new(44, 80),
                ),
            ],
            vec![],
        );

        let actions = prop_test_action(
            "const props = withDefaults(defineProps<{ title: string }>(), { title: 'hi' })",
            "Hello {{ title }}",
            script,
        );
        assert_eq!(actions.len(), 1);

        let script_edit = &actions[0].edits[1];
        assert!(
            script_edit.replacement.contains("props.title"),
            "named withDefaults should use props.title: got '{}'",
            script_edit.replacement
        );
        // No wrap edit — wrapping edit is "const props = " without "computed"/"import"
        let has_wrap_edit = actions[0].edits.iter().any(|e| {
            e.span.start == e.span.end
                && e.replacement.starts_with("const ")
                && !e.replacement.contains("computed")
                && !e.replacement.contains("import")
        });
        assert!(
            !has_wrap_edit,
            "named withDefaults should NOT generate wrapping edit"
        );
    }

    // ── Test 20: "props" name taken → _props ──

    #[test]
    fn props_name_conflict_uses_underscore() {
        let script = make_script_with_macros(
            vec![make_binding("props", ReactivityKind::None)],
            vec![make_macro(
                AnalyzedMacroKind::DefineProps,
                None,
                &["title"],
                Span::new(24, 55),
            )],
            vec![],
        );

        let actions = prop_test_action(
            "defineProps<{ title: string }>()\nconst props = { x: 1 }",
            "Hello {{ title }}",
            script,
        );
        assert_eq!(actions.len(), 1);

        let script_edit = &actions[0].edits[1];
        assert!(
            script_edit.replacement.contains("_props.title"),
            "conflicting name should use _props.title: got '{}'",
            script_edit.replacement
        );
        // Check that we don't use bare "props.title" (without underscore prefix)
        // Note: _props.title contains "props.title" as substring, so we check for
        // the absence of "props.title" NOT preceded by an underscore
        assert!(
            !script_edit.replacement.contains("${props.title}"),
            "should NOT use conflicting 'props' name in expression: got '{}'",
            script_edit.replacement
        );
    }

    // ── Test 21: Bare defineProps + v-for (function mode) ──

    #[test]
    fn bare_define_props_with_vfor_function_mode() {
        let script_body = "defineProps<{ prefix: string }>()";
        let content = "{{ prefix }}: {{ item }}";
        let source_str = format!(
            "<script setup lang=\"ts\">\n{}\n</script>\n<template>\n<li v-for=\"item in items\">{}</li>\n</template>",
            script_body, content
        );
        let source: &'static str = Box::leak(source_str.into_boxed_str());

        let li_start = source.find("<li").unwrap() as u32;
        let tag_span_end = source.find(">{{ prefix }}").unwrap() as u32 + 1;
        let content_end = source.find("</li>").unwrap() as u32;
        let content_text = &source[tag_span_end as usize..content_end as usize];

        // Parse text_children
        let mut text_children = Vec::new();
        let mut pos = 0usize;
        while pos < content_text.len() {
            if let Some(interp_rel) = content_text[pos..].find("{{") {
                if interp_rel > 0 {
                    text_children.push(TemplateTextSegment::Text {
                        span: Span::new(
                            tag_span_end + pos as u32,
                            tag_span_end + pos as u32 + interp_rel as u32,
                        ),
                        is_entity: false,
                    });
                }
                let interp_abs_start = pos + interp_rel;
                let close = content_text[interp_abs_start..].find("}}").unwrap();
                let interp_abs_end = interp_abs_start + close + 2;
                let expr_start = interp_abs_start + 3;
                let expr_end = interp_abs_start + close - 1;
                text_children.push(TemplateTextSegment::Interpolation {
                    span: Span::new(
                        tag_span_end + interp_abs_start as u32,
                        tag_span_end + interp_abs_end as u32,
                    ),
                    expression_span: Span::new(
                        tag_span_end + expr_start as u32,
                        tag_span_end + expr_end as u32,
                    ),
                });
                pos = interp_abs_end;
            } else {
                text_children.push(TemplateTextSegment::Text {
                    span: Span::new(
                        tag_span_end + pos as u32,
                        tag_span_end + content_text.len() as u32,
                    ),
                    is_entity: false,
                });
                break;
            }
        }

        let mut el = make_element("li", tag_span_end, content_end, text_children);
        el.v_for = Some(VForDirective {
            variable: "item".to_string(),
            index: None,
            iterable: "items".to_string(),
            has_key: false,
            key_expression: None,
            key_uses_index: false,
            span: Span::new(li_start + 4, li_start + 30),
        });
        el.span = Span::new(li_start, content_end + 5);

        let template = TemplateAnalysisSnapshot {
            elements: vec![el],
            ..Default::default()
        };

        let script = make_script_with_macros(
            vec![],
            vec![make_macro(
                AnalyzedMacroKind::DefineProps,
                None,
                &["prefix"],
                Span::new(24, 57),
            )],
            vec![],
        );

        let diag = make_diag(tag_span_end, content_end);
        let set = DiagnosticSet::new();
        let ctx = ActionContext {
            source,
            file_id: "test.vue",
            diagnostics: &set,
            template: Some(&template),
            script: Some(&script),
            styles: &[],
        };

        let actions = ExtractBareText.fixes_for_diagnostic(&diag, &ctx);
        assert_eq!(actions.len(), 1);

        assert!(
            actions[0].title.contains("function"),
            "v-for should use function mode: got '{}'",
            actions[0].title
        );

        let script_edit = &actions[0].edits[1];
        assert!(
            script_edit.replacement.contains("props.prefix"),
            "prop in function mode should use props.prefix: got '{}'",
            script_edit.replacement
        );
        assert!(
            script_edit.replacement.contains("item"),
            "scoped var should be param: got '{}'",
            script_edit.replacement
        );
    }

    // ── Test 22: props.items.length in computed ──

    #[test]
    fn named_define_props_member_access() {
        let script = make_script_with_macros(
            vec![make_props_binding("props", AnalyzedBindingKind::Const)],
            vec![make_macro(
                AnalyzedMacroKind::DefineProps,
                Some("props"),
                &["items"],
                Span::new(24, 75),
            )],
            vec![],
        );

        let actions = prop_test_action(
            "const props = defineProps<{ items: string[] }>()",
            "Count: {{ items.length }}",
            script,
        );
        assert_eq!(actions.len(), 1);

        let script_edit = &actions[0].edits[1];
        assert!(
            script_edit.replacement.contains("props.items"),
            "member access on prop should prefix with props.: got '{}'",
            script_edit.replacement
        );
        assert!(
            !script_edit.replacement.contains(".value"),
            "props member access should NOT use .value"
        );
    }

    // ── Test 23: Script binding shadows prop name (ref wins) ──

    #[test]
    fn script_binding_shadows_prop() {
        let script = make_script_with_macros(
            vec![make_binding("title", ReactivityKind::Ref)],
            vec![make_macro(
                AnalyzedMacroKind::DefineProps,
                None,
                &["title"],
                Span::new(24, 55),
            )],
            vec![AnalyzedImport {
                source: "vue".to_string(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "ref".to_string(),
                    is_type_only: false,
                    vue_api: None,
                    span: Span::new(0, 0),
                }],
                span: Span::new(24, 49),
                resolved_canonical_id: None,
            }],
        );

        let actions = prop_test_action(
            "defineProps<{ title: string }>()\nconst title = ref('override')",
            "Hello {{ title }}",
            script,
        );
        assert_eq!(actions.len(), 1);

        let script_edit = &actions[0].edits[1];
        assert!(
            script_edit.replacement.contains("title.value"),
            "script binding should win (ref → .value): got '{}'",
            script_edit.replacement
        );
        assert!(
            !script_edit.replacement.contains("props.title"),
            "script binding should shadow prop: NOT props.title"
        );
    }

    // ── Test 24: Computed overrides prop ──

    #[test]
    fn computed_overrides_prop() {
        let script = make_script_with_macros(
            vec![make_binding("foo", ReactivityKind::Computed)],
            vec![make_macro(
                AnalyzedMacroKind::DefineProps,
                None,
                &["foo"],
                Span::new(24, 50),
            )],
            vec![AnalyzedImport {
                source: "vue".to_string(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "computed".to_string(),
                    is_type_only: false,
                    vue_api: None,
                    span: Span::new(0, 0),
                }],
                span: Span::new(24, 52),
                resolved_canonical_id: None,
            }],
        );

        let actions = prop_test_action(
            "defineProps<{ foo: string }>()\nconst foo = computed(() => 'x')",
            "Value: {{ foo }}",
            script,
        );
        assert_eq!(actions.len(), 1);

        let script_edit = &actions[0].edits[1];
        assert!(
            script_edit.replacement.contains("foo.value"),
            "computed should win over prop (→ .value): got '{}'",
            script_edit.replacement
        );
        assert!(
            !script_edit.replacement.contains("props.foo"),
            "computed should shadow prop: NOT props.foo"
        );
    }

    // ── Test 25: Function overrides prop ──

    #[test]
    fn function_overrides_prop() {
        let mut binding = make_binding("foo", ReactivityKind::None);
        binding.kind = AnalyzedBindingKind::Function;

        let script = make_script_with_macros(
            vec![binding],
            vec![make_macro(
                AnalyzedMacroKind::DefineProps,
                None,
                &["foo"],
                Span::new(24, 50),
            )],
            vec![],
        );

        let actions = prop_test_action(
            "defineProps<{ foo: string }>()\nfunction foo() { return 1 }",
            "Value: {{ foo }}",
            script,
        );
        assert_eq!(actions.len(), 1);

        let script_edit = &actions[0].edits[1];
        // Function wins — raw identifier, no .value, no props prefix
        assert!(
            script_edit.replacement.contains("foo"),
            "function should use raw identifier: got '{}'",
            script_edit.replacement
        );
        assert!(
            !script_edit.replacement.contains("props.foo"),
            "function should shadow prop"
        );
        assert!(
            !script_edit.replacement.contains(".value"),
            "function should NOT use .value"
        );
    }

    // ── Test 26: Class overrides prop ──

    #[test]
    fn class_overrides_prop() {
        let mut binding = make_binding("foo", ReactivityKind::None);
        binding.kind = AnalyzedBindingKind::Class;

        let script = make_script_with_macros(
            vec![binding],
            vec![make_macro(
                AnalyzedMacroKind::DefineProps,
                None,
                &["foo"],
                Span::new(24, 50),
            )],
            vec![],
        );

        let actions = prop_test_action(
            "defineProps<{ foo: string }>()\nclass foo {}",
            "Value: {{ foo }}",
            script,
        );
        assert_eq!(actions.len(), 1);

        let script_edit = &actions[0].edits[1];
        assert!(
            !script_edit.replacement.contains("props.foo"),
            "class should shadow prop"
        );
        assert!(
            !script_edit.replacement.contains(".value"),
            "class should NOT use .value"
        );
    }

    // ── Helper tests ──

    #[test]
    fn extract_identifiers_simple() {
        assert_eq!(extract_identifiers("count"), vec!["count"]);
    }

    #[test]
    fn extract_identifiers_member_access() {
        assert_eq!(extract_identifiers("state.count"), vec!["state", "count"]);
    }

    #[test]
    fn extract_identifiers_complex_expr() {
        let idents = extract_identifiers("items.filter(x => x > 0).length");
        assert!(idents.contains(&"items"));
        assert!(idents.contains(&"x"));
        assert!(idents.contains(&"length"));
        assert!(
            idents.contains(&"filter"),
            "filter should be found as identifier"
        );
    }

    #[test]
    fn extract_identifiers_skips_keywords() {
        let idents = extract_identifiers("typeof x === 'string' ? true : false");
        assert!(idents.contains(&"x"));
        assert!(!idents.contains(&"typeof"));
        assert!(!idents.contains(&"true"));
        assert!(!idents.contains(&"false"));
    }

    #[test]
    fn find_script_end_basic() {
        let source = "<script setup>\nconst x = 1\n</script>";
        let pos = find_script_end_insert_pos(source);
        assert!(pos.is_some());
        let pos = pos.unwrap() as usize;
        assert!(pos <= source.find("</script").unwrap());
    }

    #[test]
    fn find_script_end_no_script() {
        let source = "<template><div /></template>";
        let pos = find_script_end_insert_pos(source);
        assert!(pos.is_none());
    }
}
