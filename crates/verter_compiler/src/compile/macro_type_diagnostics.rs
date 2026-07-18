//! Imported-macro-type validation diagnostics.
//!
//! When a `defineProps` / `defineEmits` type argument is a simple reference to
//! an imported type, the bundler/IDE lanes surface a dedicated diagnostic:
//! an UNRESOLVED imported reference degrades softly on the render-only lane
//! (`XUnresolvedImportedMacroType`), while a resolved-but-wrong-shape type is a
//! fatal `XInvalidMacroType` on both lanes. Split out of `compile::mod` so the
//! orchestrator stays under the `no_oversize_files` guard.

use rustc_hash::FxHashSet;

use crate::common::Span;
use crate::diagnostics::{CompilerErrorCode, Diagnostic};
use crate::script::prepared::PreparedScript;
use crate::utils::oxc::script::type_surface::RuntimeType;
use crate::utils::oxc::vue::{MacroTypeParams, ScriptItem, ScriptMacro};

fn is_simple_type_reference(text: &str) -> bool {
    let mut chars = text.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first == '_' || first == '$' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric())
}

fn collect_imported_type_names<'a>(items: &'a [ScriptItem<'a>]) -> FxHashSet<&'a str> {
    let mut imported = FxHashSet::default();
    for item in items {
        let ScriptItem::Import(import) = item else {
            continue;
        };
        for binding in &import.bindings {
            if import.is_type_only || binding.is_type_only {
                imported.insert(binding.name);
            }
        }
    }
    imported
}

fn props_type_is_object_like(type_params: &MacroTypeParams) -> bool {
    !type_params.resolved.props.is_empty()
        || type_params
            .resolved
            .root_runtime_types
            .iter()
            .any(|ty| matches!(ty, RuntimeType::Object))
}

fn push_invalid_macro_type_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    code: CompilerErrorCode,
    message: String,
    type_params: &MacroTypeParams,
    content_start: u32,
) {
    // The prepared setup parse runs at the SFC content offset, so `type_span` is
    // SFC-absolute. The public macro-type diagnostic span is content-local
    // (relative to the setup block content): localize it back here so the surfaced
    // span is identical regardless of where the block sits in the SFC.
    let local_span = Span::new(
        type_params.type_span.start - content_start,
        type_params.type_span.end - content_start,
    );
    diagnostics.push(Diagnostic::error_with_message("script", code, message).with_span(local_span));
}

fn validate_imported_macro_type(
    macro_name: &str,
    type_params: &MacroTypeParams,
    type_text: &str,
    imported_type_names: &FxHashSet<&str>,
    content_start: u32,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !is_simple_type_reference(type_text) || !imported_type_names.contains(type_text) {
        return;
    }

    if type_params.unresolved_type_ref {
        // An imported type reference that could not be RESOLVED. Distinct
        // code from the wrong-shape cases below: the render-only bundler
        // lane softens ONLY this (the type degrades to `Unknown`), while a
        // resolved-but-wrong-shape type stays a fatal `XInvalidMacroType`.
        push_invalid_macro_type_diagnostic(
            diagnostics,
            CompilerErrorCode::XUnresolvedImportedMacroType,
            format!(
                "{}() type argument '{}' could not be resolved.",
                macro_name, type_text
            ),
            type_params,
            content_start,
        );
        return;
    }

    match macro_name {
        "defineProps" => {
            if !props_type_is_object_like(type_params) {
                // Resolved-but-wrong-shape: a genuine local misuse. Always
                // fatal, on both lanes.
                push_invalid_macro_type_diagnostic(
                    diagnostics,
                    CompilerErrorCode::XInvalidMacroType,
                    format!(
                        "defineProps() type argument '{}' must resolve to an object-like props type.",
                        type_text
                    ),
                    type_params,
                    content_start,
                );
            }
        }
        "defineEmits" if type_params.resolved.call_signatures.is_empty() => {
            push_invalid_macro_type_diagnostic(
                diagnostics,
                CompilerErrorCode::XInvalidMacroType,
                format!(
                    "defineEmits() type argument '{}' must resolve to emit call signatures or a named-tuple emits object.",
                    type_text
                ),
                type_params,
                content_start,
            );
        }
        _ => {}
    }
}

/// Read a macro type argument's source text from the setup content slice.
///
/// [`MacroTypeParams::type_span`] is SFC-absolute (the prepared setup parse runs
/// at the setup content offset), so it is localized against `content_str` here.
fn macro_type_argument_text<'a>(
    content_str: &'a str,
    content_start: u32,
    type_params: &MacroTypeParams,
) -> &'a str {
    let start = (type_params.type_span.start - content_start) as usize;
    let end = (type_params.type_span.end - content_start) as usize;
    content_str[start..end].trim()
}

pub(super) fn collect_invalid_macro_type_diagnostics(prepared: &PreparedScript) -> Vec<Diagnostic> {
    let Some(setup) = prepared.setup() else {
        return Vec::new();
    };

    // The setup block was parsed once (companion + external types already folded
    // in) when the prepared script was built — read the macro surfaces from that
    // single parse instead of re-parsing and re-resolving here. Macro type-span
    // coordinates are SFC-absolute (the prepared parse runs at the setup content
    // offset), so they are localized against the content slice when read.
    let content_str = setup.content_str();
    let content_start = setup.content_start();
    let parsed_script = setup.parse_result();
    let imported_type_names = collect_imported_type_names(&parsed_script.items);
    let mut diagnostics = Vec::new();

    for item in &parsed_script.items {
        let ScriptItem::Macro(mac) = item else {
            continue;
        };

        match mac {
            ScriptMacro::DefineProps {
                type_params: Some(type_params),
                ..
            } => {
                let type_text = macro_type_argument_text(content_str, content_start, type_params);
                validate_imported_macro_type(
                    "defineProps",
                    type_params,
                    type_text,
                    &imported_type_names,
                    content_start,
                    &mut diagnostics,
                );
            }
            ScriptMacro::WithDefaults {
                define_props_type_params: Some(type_params),
                defaults,
                defaults_arg_span,
                ..
            } => {
                let type_text = macro_type_argument_text(content_str, content_start, type_params);
                let has_defaults_fallback = defaults.is_some() || defaults_arg_span.is_some();
                let skip_unresolved_import_error = has_defaults_fallback
                    && type_params.unresolved_type_ref
                    && is_simple_type_reference(type_text)
                    && imported_type_names.contains(type_text);
                if skip_unresolved_import_error {
                    continue;
                }
                validate_imported_macro_type(
                    "defineProps",
                    type_params,
                    type_text,
                    &imported_type_names,
                    content_start,
                    &mut diagnostics,
                );
            }
            ScriptMacro::DefineEmits {
                type_params: Some(type_params),
                ..
            } => {
                let type_text = macro_type_argument_text(content_str, content_start, type_params);
                validate_imported_macro_type(
                    "defineEmits",
                    type_params,
                    type_text,
                    &imported_type_names,
                    content_start,
                    &mut diagnostics,
                );
            }
            _ => {}
        }
    }

    diagnostics
}
