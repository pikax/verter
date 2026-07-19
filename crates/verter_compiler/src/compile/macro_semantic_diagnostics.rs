use rustc_hash::FxHashSet;
use verter_macro_dto::{
    MacroAnchor, MacroFailure, MacroInvalidReason, MacroMemberReason, MacroPartialReason,
    MacroRuntimeBundle, MacroRuntimeOutcome, MacroRuntimeShape, MacroTscBundle, MacroTscOutcome,
    MacroTscProjection, PropsDefaultsAssociation, SynthesizedRowKind, UnresolvedReason,
    UnsupportedReason,
};

use crate::common::Span;
use crate::diagnostics::{CompilerErrorCode, Diagnostic};
use crate::script::prepared::PreparedScript;
use crate::utils::oxc::vue::{MacroTypeParams, ScriptItem, ScriptMacro};

use super::{CompileTarget, VueMacroSemanticInput};
use crate::tsc::TscUnavailableOutcome;

pub(super) fn tsc_generation_diagnostic(error: crate::tsc::TscGenerationError) -> Diagnostic {
    Diagnostic::error_with_message(
        "script",
        CompilerErrorCode::XUnavailableMacroSemanticResult,
        format!("Authoritative TSC generation failed: {error}."),
    )
}

pub(super) struct MacroSemanticValidation {
    pub diagnostics: Vec<Diagnostic>,
    /// A runtime bundle drives codegen only after its complete syntax join,
    /// identity, role, name, and anchor contract has been validated.
    pub runtime_valid: bool,
}

#[derive(Clone, Copy)]
enum ExpectedMacroRole {
    Props { with_defaults: bool },
    Emits,
    Model,
}

struct RuntimeSlot<'a> {
    syntax_index: u32,
    payload_macro_index: u32,
    effective_macro_index: u32,
    role: ExpectedMacroRole,
    type_params: &'a MacroTypeParams,
    model_name: Option<String>,
    model_name_span: Option<Span>,
}

impl RuntimeSlot<'_> {
    fn type_span(&self) -> Span {
        self.type_params.type_span
    }
}

pub(super) fn collect_macro_semantic_diagnostics(
    prepared: &PreparedScript<'_>,
    target: CompileTarget,
    semantics: &VueMacroSemanticInput,
) -> MacroSemanticValidation {
    let mut diagnostics = Vec::new();
    let mut runtime_valid = true;
    let Some(setup) = prepared.setup() else {
        if target.needs_runtime_macro_semantics() {
            runtime_valid = validate_no_runtime_slots(semantics.runtime(), &mut diagnostics);
        }
        return MacroSemanticValidation {
            diagnostics,
            runtime_valid,
        };
    };

    let mut syntax_index = 0_u32;
    let mut macro_index = 0_u32;
    let mut runtime_syntax_indices = FxHashSet::default();
    for item in &setup.parse_result().items {
        let ScriptItem::Macro(mac) = item else {
            continue;
        };
        let current_syntax_index = syntax_index;
        syntax_index = syntax_index.saturating_add(1);
        let payload_macro_index = macro_index;
        let effective_macro_index = if matches!(mac, ScriptMacro::WithDefaults { .. }) {
            macro_index.saturating_add(1)
        } else {
            macro_index
        };
        macro_index = effective_macro_index.saturating_add(1);

        let Some((role, type_params)) = typed_codegen_role(mac) else {
            continue;
        };
        runtime_syntax_indices.insert(current_syntax_index);
        let (model_name, model_name_span) = model_syntax(mac, setup.content_start());
        let slot = RuntimeSlot {
            syntax_index: current_syntax_index,
            payload_macro_index,
            effective_macro_index,
            role,
            type_params,
            model_name,
            model_name_span,
        };

        if target.needs_runtime_macro_semantics()
            && !validate_runtime_entry(semantics.runtime(), &slot, &mut diagnostics)
        {
            runtime_valid = false;
        }
        if target.needs_tsc() {
            validate_tsc_entry(
                semantics.tsc(),
                current_syntax_index,
                role,
                slot.type_span(),
                &mut diagnostics,
            );
        }
    }

    if target.needs_runtime_macro_semantics() {
        if let Some(bundle) = semantics.runtime() {
            for entry in &bundle.entries {
                if !runtime_syntax_indices.contains(&entry.syntax_index) {
                    runtime_valid = false;
                    push_runtime_join_failure(
                        &mut diagnostics,
                        entry.syntax_index,
                        None,
                        "unexpected-entry",
                    );
                }
            }
        }
    }

    MacroSemanticValidation {
        diagnostics,
        runtime_valid,
    }
}

fn typed_codegen_role<'a>(
    mac: &'a ScriptMacro<'a>,
) -> Option<(ExpectedMacroRole, &'a MacroTypeParams)> {
    match mac {
        ScriptMacro::DefineProps {
            type_params: Some(type_params),
            ..
        } => Some((
            ExpectedMacroRole::Props {
                with_defaults: false,
            },
            type_params,
        )),
        ScriptMacro::WithDefaults {
            define_props_type_params: Some(type_params),
            ..
        } => Some((
            ExpectedMacroRole::Props {
                with_defaults: true,
            },
            type_params,
        )),
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

fn model_syntax(mac: &ScriptMacro<'_>, content_start: u32) -> (Option<String>, Option<Span>) {
    let ScriptMacro::DefineModel {
        name, name_span, ..
    } = mac
    else {
        return (None, None);
    };
    let name = name.unwrap_or("modelValue").to_owned();
    let span = name_span.map(|span| {
        Span::new(
            content_start.saturating_add(span.start),
            content_start.saturating_add(span.end),
        )
    });
    (Some(name), span)
}

fn validate_no_runtime_slots(
    bundle: Option<&MacroRuntimeBundle>,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let Some(bundle) = bundle else {
        return true;
    };
    for entry in &bundle.entries {
        push_runtime_join_failure(diagnostics, entry.syntax_index, None, "unexpected-entry");
    }
    bundle.entries.is_empty()
}

fn validate_runtime_entry(
    bundle: Option<&MacroRuntimeBundle>,
    slot: &RuntimeSlot<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let Some(bundle) = bundle else {
        push_missing(diagnostics, "runtime", slot.syntax_index, slot.type_span());
        return false;
    };
    let mut matching = bundle
        .entries
        .iter()
        .filter(|entry| entry.syntax_index == slot.syntax_index);
    let Some(entry) = matching.next() else {
        push_missing(diagnostics, "runtime", slot.syntax_index, slot.type_span());
        return false;
    };
    if matching.next().is_some() {
        push_runtime_join_failure(
            diagnostics,
            slot.syntax_index,
            Some(slot.type_span()),
            "duplicate-entry",
        );
        return false;
    }
    if entry.macro_index != slot.effective_macro_index {
        push_runtime_join_failure(
            diagnostics,
            slot.syntax_index,
            Some(slot.type_span()),
            "macro-identity-mismatch",
        );
        return false;
    }

    let shape = match &entry.outcome {
        MacroRuntimeOutcome::Complete(shape) => shape,
        MacroRuntimeOutcome::Partial(failure) => {
            push_runtime_unavailable(
                diagnostics,
                slot.syntax_index,
                slot.type_span(),
                "partial",
                partial_reason_code(failure.reason),
                failure.diagnostic.as_deref(),
                false,
            );
            return false;
        }
        MacroRuntimeOutcome::Unresolved(failure) => {
            push_runtime_unavailable(
                diagnostics,
                slot.syntax_index,
                slot.type_span(),
                "unresolved",
                unresolved_reason_code(failure.reason),
                failure.diagnostic.as_deref(),
                false,
            );
            return false;
        }
        MacroRuntimeOutcome::Unsupported(failure) => {
            push_runtime_unavailable(
                diagnostics,
                slot.syntax_index,
                slot.type_span(),
                "unsupported",
                unsupported_reason_code(failure.reason),
                failure.diagnostic.as_deref(),
                false,
            );
            return false;
        }
        MacroRuntimeOutcome::Invalid(failure) => {
            push_runtime_unavailable(
                diagnostics,
                slot.syntax_index,
                slot.type_span(),
                "invalid",
                invalid_reason_code(failure.reason),
                failure.diagnostic.as_deref(),
                true,
            );
            return false;
        }
    };

    match (&slot.role, shape) {
        (ExpectedMacroRole::Props { with_defaults }, MacroRuntimeShape::Props(props)) => {
            let defaults_match = match (*with_defaults, props.defaults) {
                (false, PropsDefaultsAssociation::None) => true,
                (
                    true,
                    PropsDefaultsAssociation::WithDefaults {
                        payload_macro_index,
                        defaults_macro_index,
                    },
                ) => {
                    payload_macro_index == slot.payload_macro_index
                        && defaults_macro_index == slot.effective_macro_index
                }
                _ => false,
            };
            if !defaults_match {
                push_runtime_join_failure(
                    diagnostics,
                    slot.syntax_index,
                    Some(slot.type_span()),
                    "defaults-association-mismatch",
                );
                return false;
            }
            validate_props_shape(slot, props, diagnostics)
        }
        (ExpectedMacroRole::Emits, MacroRuntimeShape::Emits(emits)) => {
            validate_emits_shape(slot, emits, diagnostics)
        }
        (ExpectedMacroRole::Model, MacroRuntimeShape::Model(model)) => {
            validate_model_shape(slot, model, diagnostics)
        }
        _ => {
            push_runtime_join_failure(
                diagnostics,
                slot.syntax_index,
                Some(slot.type_span()),
                "role-mismatch",
            );
            false
        }
    }
}

fn validate_props_shape(
    slot: &RuntimeSlot<'_>,
    props: &verter_macro_dto::PropsRuntimeShape,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let mut valid = true;
    let mut names = FxHashSet::default();
    for prop in &props.props {
        if !names.insert(prop.name.as_str()) {
            valid = false;
            push_runtime_join_failure(
                diagnostics,
                slot.syntax_index,
                Some(slot.type_span()),
                "duplicate-public-name",
            );
            continue;
        }
        let span = match authored_anchor_span(slot, prop.anchor, Some(prop.name.as_str())) {
            Ok(span) => span,
            Err(code) => {
                valid = false;
                push_runtime_join_failure(
                    diagnostics,
                    slot.syntax_index,
                    Some(slot.type_span()),
                    code,
                );
                continue;
            }
        };
        if let verter_macro_dto::RuntimePropType::Degraded(failure) = &prop.type_shape {
            push_member_degraded(
                diagnostics,
                "prop",
                prop.name.as_str(),
                slot.syntax_index,
                span,
                failure,
            );
        }
    }
    valid
}

fn validate_emits_shape(
    slot: &RuntimeSlot<'_>,
    emits: &[verter_macro_dto::RuntimeEmit],
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let mut valid = true;
    let mut names = FxHashSet::default();
    for emit in emits {
        if !names.insert(emit.name.as_str()) {
            valid = false;
            push_runtime_join_failure(
                diagnostics,
                slot.syntax_index,
                Some(slot.type_span()),
                "duplicate-public-name",
            );
            continue;
        }
        if let Err(code) = authored_anchor_span(slot, emit.anchor, None) {
            valid = false;
            push_runtime_join_failure(diagnostics, slot.syntax_index, Some(slot.type_span()), code);
        }
    }
    valid
}

fn validate_model_shape(
    slot: &RuntimeSlot<'_>,
    model: &verter_macro_dto::ModelRuntimeShape,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let expected_name = slot.model_name.as_deref().unwrap_or("modelValue");
    let expected_modifiers = if expected_name == "modelValue" {
        "modelModifiers".to_owned()
    } else {
        format!("{expected_name}Modifiers")
    };
    if model.prop.name != expected_name
        || model.update_event.name != format!("update:{expected_name}")
        || model.modifiers_prop.name != expected_modifiers
    {
        push_runtime_join_failure(
            diagnostics,
            slot.syntax_index,
            Some(slot.type_span()),
            "public-name-mismatch",
        );
        return false;
    }

    let mut valid = true;
    for (anchor, expected_row) in [
        (model.prop.anchor, SynthesizedRowKind::ModelProp),
        (
            model.update_event.anchor,
            SynthesizedRowKind::ModelUpdateEvent,
        ),
        (
            model.modifiers_prop.anchor,
            SynthesizedRowKind::ModelModifiersProp,
        ),
    ] {
        if !matches!(
            anchor,
            MacroAnchor::Synthesized { macro_index, row }
                if macro_index == slot.effective_macro_index && row == expected_row
        ) {
            valid = false;
            push_runtime_join_failure(
                diagnostics,
                slot.syntax_index,
                Some(slot.type_span()),
                "invalid-macro-anchor",
            );
        }
    }
    if let verter_macro_dto::RuntimePropType::Degraded(failure) = &model.prop.type_shape {
        push_member_degraded(
            diagnostics,
            "model prop",
            model.prop.name.as_str(),
            slot.syntax_index,
            slot.model_name_span.unwrap_or_else(|| slot.type_span()),
            failure,
        );
    }
    valid
}

fn authored_anchor_span(
    slot: &RuntimeSlot<'_>,
    anchor: MacroAnchor,
    expected_prop_name: Option<&str>,
) -> Result<Span, &'static str> {
    match anchor {
        MacroAnchor::Authored {
            macro_index,
            member_ordinal,
        } if matches!(
            slot.role,
            ExpectedMacroRole::Props { .. } | ExpectedMacroRole::Emits
        ) && macro_index == slot.payload_macro_index =>
        {
            let ordinal = member_ordinal.get() as usize;
            match slot.role {
                ExpectedMacroRole::Props { .. } => {
                    let Some(member) = slot.type_params.prop_members.get(ordinal) else {
                        return Err("invalid-authored-member-ordinal");
                    };
                    if expected_prop_name.is_some_and(|name| name != member.name) {
                        return Err("public-name-mismatch");
                    }
                    Ok(member.key_span)
                }
                ExpectedMacroRole::Emits => slot
                    .type_params
                    .emit_member_spans
                    .get(ordinal)
                    .copied()
                    .ok_or("invalid-authored-member-ordinal"),
                ExpectedMacroRole::Model => Err("invalid-macro-anchor"),
            }
        }
        MacroAnchor::MacroArgument { macro_index }
            if matches!(
                slot.role,
                ExpectedMacroRole::Props { .. } | ExpectedMacroRole::Emits
            ) && macro_index == slot.payload_macro_index =>
        {
            Ok(slot.type_span())
        }
        _ => Err("invalid-macro-anchor"),
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

    if let Some(outcome) = TscUnavailableOutcome::from_macro_outcome(&entry.outcome) {
        push_tsc_unavailable(diagnostics, syntax_index, anchor, &outcome);
        return;
    }

    let compatible = matches!(
        (&entry.outcome, role),
        (
            MacroTscOutcome::Complete(MacroTscProjection::Props(_)),
            ExpectedMacroRole::Props { .. }
        ) | (
            MacroTscOutcome::Complete(MacroTscProjection::Emits(_)),
            ExpectedMacroRole::Emits
        ) | (
            MacroTscOutcome::Complete(MacroTscProjection::Model(_)),
            ExpectedMacroRole::Model
        )
    );
    if !compatible {
        push_unavailable(diagnostics, "TSC", syntax_index, anchor);
    }
}

fn push_tsc_unavailable(
    diagnostics: &mut Vec<Diagnostic>,
    syntax_index: u32,
    anchor: Span,
    outcome: &TscUnavailableOutcome,
) {
    let code = if matches!(outcome, TscUnavailableOutcome::Invalid(_)) {
        CompilerErrorCode::XInvalidMacroType
    } else {
        CompilerErrorCode::XUnavailableMacroSemanticResult
    };
    let mut message = format!(
        "Authoritative TSC semantics for macro syntax index {syntax_index} are {} ({}).",
        outcome.kind_code(),
        outcome.reason_code()
    );
    if let Some(diagnostic) = outcome.diagnostic() {
        message.push(' ');
        message.push_str(diagnostic);
    }
    diagnostics.push(Diagnostic::error_with_message("script", code, message).with_span(anchor));
}

fn push_runtime_unavailable(
    diagnostics: &mut Vec<Diagnostic>,
    syntax_index: u32,
    anchor: Span,
    kind: &str,
    reason: &str,
    detail: Option<&str>,
    invalid: bool,
) {
    let code = if invalid {
        CompilerErrorCode::XInvalidMacroType
    } else {
        CompilerErrorCode::XUnavailableMacroSemanticResult
    };
    let mut message = format!(
        "Authoritative runtime semantics for macro syntax index {syntax_index} are {kind} ({reason})."
    );
    if let Some(detail) = detail {
        message.push(' ');
        message.push_str(detail);
    }
    diagnostics.push(Diagnostic::error_with_message("script", code, message).with_span(anchor));
}

fn push_member_degraded(
    diagnostics: &mut Vec<Diagnostic>,
    role: &str,
    name: &str,
    syntax_index: u32,
    anchor: Span,
    failure: &MacroFailure<MacroMemberReason>,
) {
    let (kind, reason) = member_reason_codes(failure.reason);
    let mut message = format!(
        "Authoritative runtime {role} {name:?} for macro syntax index {syntax_index} is {kind} ({reason}); Vue runtime validation degrades this row to null."
    );
    if let Some(detail) = failure.diagnostic.as_deref() {
        message.push(' ');
        message.push_str(detail);
    }
    diagnostics.push(
        Diagnostic::warning("script", CompilerErrorCode::XUnresolvedImportedMacroType)
            .with_message(message)
            .with_span(anchor),
    );
}

fn push_runtime_join_failure(
    diagnostics: &mut Vec<Diagnostic>,
    syntax_index: u32,
    anchor: Option<Span>,
    reason: &str,
) {
    let diagnostic = Diagnostic::error_with_message(
        "script",
        CompilerErrorCode::XUnavailableMacroSemanticResult,
        format!(
            "Authoritative runtime semantics for macro syntax index {syntax_index} failed compiler syntax validation ({reason})."
        ),
    );
    diagnostics.push(match anchor {
        Some(anchor) => diagnostic.with_span(anchor),
        None => diagnostic,
    });
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

const fn partial_reason_code(reason: MacroPartialReason) -> &'static str {
    match reason {
        MacroPartialReason::BudgetExceeded => "budget-exceeded",
        MacroPartialReason::Cancelled => "cancelled",
        MacroPartialReason::SupersededGeneration => "superseded-generation",
        MacroPartialReason::UnstableState => "unstable-state",
        MacroPartialReason::Recursion => "recursion",
        MacroPartialReason::IncompleteTraversal => "incomplete-traversal",
    }
}

const fn unresolved_reason_code(reason: UnresolvedReason) -> &'static str {
    match reason {
        UnresolvedReason::MissingTypeArgument => "missing-type-argument",
        UnresolvedReason::MissingDeclaration => "missing-declaration",
        UnresolvedReason::AmbiguousReference => "ambiguous-reference",
        UnresolvedReason::MissingDependency => "missing-dependency",
    }
}

const fn unsupported_reason_code(reason: UnsupportedReason) -> &'static str {
    match reason {
        UnsupportedReason::MacroKind => "macro-kind",
        UnsupportedReason::SemanticConstruct => "semantic-construct",
    }
}

const fn invalid_reason_code(reason: MacroInvalidReason) -> &'static str {
    match reason {
        MacroInvalidReason::NonObjectRoot => "non-object-root",
    }
}

const fn member_reason_codes(reason: MacroMemberReason) -> (&'static str, &'static str) {
    match reason {
        MacroMemberReason::Partial(reason) => ("partial", partial_reason_code(reason)),
        MacroMemberReason::Unresolved(reason) => ("unresolved", unresolved_reason_code(reason)),
        MacroMemberReason::Unsupported(reason) => ("unsupported", unsupported_reason_code(reason)),
    }
}
