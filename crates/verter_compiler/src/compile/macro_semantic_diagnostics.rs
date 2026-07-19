use verter_macro_dto::{
    MacroRuntimeBundle, MacroRuntimeOutcome, MacroRuntimeShape, MacroTscBundle, MacroTscOutcome,
    MacroTscProjection,
};

use crate::common::Span;
use crate::diagnostics::{CompilerErrorCode, Diagnostic};
use crate::script::prepared::PreparedScript;
use crate::utils::oxc::vue::{MacroTypeParams, ScriptItem, ScriptMacro};

use super::{CompileTarget, VueMacroSemanticInput};

#[derive(Clone, Copy)]
enum ExpectedMacroRole {
    Props,
    Emits,
    Model,
}

pub(super) fn collect_macro_semantic_diagnostics(
    prepared: &PreparedScript<'_>,
    target: CompileTarget,
    semantics: &VueMacroSemanticInput,
) -> Vec<Diagnostic> {
    let Some(setup) = prepared.setup() else {
        return Vec::new();
    };

    let mut diagnostics = Vec::new();
    let mut syntax_index = 0_u32;
    for item in &setup.parse_result().items {
        let ScriptItem::Macro(mac) = item else {
            continue;
        };
        let current_index = syntax_index;
        syntax_index = syntax_index.saturating_add(1);

        let Some((role, type_params)) = typed_codegen_role(mac) else {
            continue;
        };
        let anchor = semantic_anchor(mac, type_params, setup.content_start());

        if target.needs_script() {
            validate_runtime_entry(
                semantics.runtime(),
                current_index,
                role,
                anchor,
                &mut diagnostics,
            );
        }
        if target.needs_tsc() {
            validate_tsc_entry(
                semantics.tsc(),
                current_index,
                role,
                anchor,
                &mut diagnostics,
            );
        }
    }
    diagnostics
}

fn typed_codegen_role<'a>(
    mac: &'a ScriptMacro<'a>,
) -> Option<(ExpectedMacroRole, &'a MacroTypeParams)> {
    match mac {
        ScriptMacro::DefineProps {
            type_params: Some(type_params),
            ..
        } => Some((ExpectedMacroRole::Props, type_params)),
        ScriptMacro::WithDefaults {
            define_props_type_params: Some(type_params),
            ..
        } => Some((ExpectedMacroRole::Props, type_params)),
        ScriptMacro::DefineEmits {
            type_params: Some(type_params),
            ..
        } => Some((ExpectedMacroRole::Emits, type_params)),
        ScriptMacro::DefineModel {
            type_params: Some(type_params),
            ..
        } => Some((ExpectedMacroRole::Model, type_params)),
        _ => None,
    }
}

fn semantic_anchor(
    mac: &ScriptMacro<'_>,
    type_params: &MacroTypeParams,
    content_start: u32,
) -> Span {
    if type_params.type_span.start >= content_start {
        Span::new(
            type_params.type_span.start - content_start,
            type_params.type_span.end - content_start,
        )
    } else {
        mac.span()
    }
}

fn validate_runtime_entry(
    bundle: Option<&MacroRuntimeBundle>,
    syntax_index: u32,
    role: ExpectedMacroRole,
    anchor: Span,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(entry) = bundle.and_then(|bundle| {
        bundle
            .entries
            .iter()
            .find(|entry| entry.syntax_index == syntax_index)
    }) else {
        push_missing(diagnostics, "runtime", syntax_index, anchor);
        return;
    };

    let compatible = matches!(
        (&entry.outcome, role),
        (
            MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(_)),
            ExpectedMacroRole::Props
        ) | (
            MacroRuntimeOutcome::Complete(MacroRuntimeShape::Emits(_)),
            ExpectedMacroRole::Emits
        ) | (
            MacroRuntimeOutcome::Complete(MacroRuntimeShape::Model(_)),
            ExpectedMacroRole::Model
        )
    );
    if !compatible {
        push_unavailable(diagnostics, "runtime", syntax_index, anchor);
    }
}

fn validate_tsc_entry(
    bundle: Option<&MacroTscBundle>,
    syntax_index: u32,
    role: ExpectedMacroRole,
    anchor: Span,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(entry) = bundle.and_then(|bundle| {
        bundle
            .entries
            .iter()
            .find(|entry| entry.syntax_index == syntax_index)
    }) else {
        push_missing(diagnostics, "TSC", syntax_index, anchor);
        return;
    };

    let compatible = matches!(
        (&entry.outcome, role),
        (
            MacroTscOutcome::Complete(MacroTscProjection::Props { .. }),
            ExpectedMacroRole::Props
        ) | (
            MacroTscOutcome::Complete(MacroTscProjection::Emits { .. }),
            ExpectedMacroRole::Emits
        ) | (
            MacroTscOutcome::Complete(MacroTscProjection::Model { .. }),
            ExpectedMacroRole::Model
        )
    );
    if !compatible {
        push_unavailable(diagnostics, "TSC", syntax_index, anchor);
    }
}

fn push_missing(diagnostics: &mut Vec<Diagnostic>, lane: &str, syntax_index: u32, anchor: Span) {
    diagnostics.push(
        Diagnostic::error_with_message(
            "script",
            CompilerErrorCode::XMissingMacroSemanticBundle,
            format!(
                "Missing authoritative {lane} semantics for macro syntax index {syntax_index}."
            ),
        )
        .with_span(anchor),
    );
}

fn push_unavailable(
    diagnostics: &mut Vec<Diagnostic>,
    lane: &str,
    syntax_index: u32,
    anchor: Span,
) {
    diagnostics.push(
        Diagnostic::error_with_message(
            "script",
            CompilerErrorCode::XUnavailableMacroSemanticResult,
            format!(
                "Authoritative {lane} semantics for macro syntax index {syntax_index} are incomplete or incompatible."
            ),
        )
        .with_span(anchor),
    );
}
