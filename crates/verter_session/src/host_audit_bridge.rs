//! Conversion shims between session-tier diagnostic types and the
//! audit-substrate DTOs in `verter_audit`.
//!
//! `verter_audit` is a leaf crate that depends only on `verter_span`.
//! The conversion from `MacroExpansionDiagnostics`
//! (`verter_semantic`) to `AuditDiagnosticEntry` (`verter_audit`)
//! cannot live in either crate without violating that boundary, so it
//! lives here in `verter_session` — which already depends on both.
//!
//! Producers call [`macro_expansion_to_audit_entries`] when a request
//! finishes to project the macro-expansion diagnostics collected in
//! `ComponentMetaAnalysis::macro_expansion_diagnostics` onto the
//! audit substrate's `ComponentMetaPayload::diagnostics` field.

#![allow(dead_code)] // The bridge is reachable only from unit tests until an audited producer consumes the projection.

use verter_audit::{AuditDiagnosticEntry, AuditDiagnosticKind};
use verter_semantic::analysis::component_meta::MacroExpansionDiagnostics;
use verter_semantic::analysis::type_expand::{ExpansionDiagnostic, ExpansionStopReason};

/// Project a slice of [`MacroExpansionDiagnostics`] onto the
/// audit-substrate's [`AuditDiagnosticEntry`] vector. One audit
/// entry is produced per [`ExpansionDiagnostic`] inside each macro
/// envelope; the macro_index is propagated so consumers can group
/// diagnostics by their owning macro invocation.
#[must_use]
pub(crate) fn macro_expansion_to_audit_entries(
    diags: &[MacroExpansionDiagnostics],
) -> Vec<AuditDiagnosticEntry> {
    let mut entries = Vec::new();
    for envelope in diags.iter() {
        for diag in envelope.diagnostics.iter() {
            entries.push(AuditDiagnosticEntry {
                kind: map_kind(diag),
                message: format_message(diag),
                span: None,
                macro_index: Some(envelope.macro_index),
            });
        }
    }
    entries
}

fn map_kind(diag: &ExpansionDiagnostic) -> AuditDiagnosticKind {
    // 1:1 routing now that `ExpansionStopReason` carries dedicated
    // discriminators for each shallow-walker diagnostic class.
    // Pre-fix this routine inspected `diag.context` substrings
    // (`starts_with("cyclic-")`, `starts_with("union-arm-empty")`)
    // to recover the lost variant info; that string inspection is
    // no longer necessary because the upstream projector
    // `meta_resolve::diagnostic_convert::shallow_to_expansion` now
    // emits dedicated variants.
    match diag.reason {
        ExpansionStopReason::BudgetExceeded | ExpansionStopReason::MappedDepthExceeded => {
            AuditDiagnosticKind::BudgetExceeded
        }
        ExpansionStopReason::IndeterminateConditional
        | ExpansionStopReason::ConditionalContextTruncated => AuditDiagnosticKind::OpenConditional,
        ExpansionStopReason::CyclicReference | ExpansionStopReason::CyclicInstantiation => {
            AuditDiagnosticKind::CyclicReference
        }
        ExpansionStopReason::IdempotentArm => AuditDiagnosticKind::IdempotentArm,
        ExpansionStopReason::EmptyUnionArm => AuditDiagnosticKind::EmptyUnionArm,
        ExpansionStopReason::InstantiationError | ExpansionStopReason::UnresolvedReference => {
            AuditDiagnosticKind::ResolverError
        }
        ExpansionStopReason::InfiniteKeySpace => AuditDiagnosticKind::ResolverError,
        ExpansionStopReason::UnsupportedOperator => AuditDiagnosticKind::Other,
    }
}

fn format_message(diag: &ExpansionDiagnostic) -> String {
    match &diag.property_name {
        Some(name) => format!("{:?} {} ({})", diag.reason, diag.context, name),
        None => format!("{:?} {}", diag.reason, diag.context),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_semantic::analysis::component_meta::MacroExpansionKind;
    use verter_semantic::analysis::type_expand::{
        ExpansionExactness, ExpansionExecutionStatus, ExpansionStopReason,
    };

    fn diag(reason: ExpansionStopReason, context: &str) -> ExpansionDiagnostic {
        ExpansionDiagnostic {
            reason,
            context: context.to_string(),
            property_name: None,
        }
    }

    fn envelope(macro_index: usize, diags: Vec<ExpansionDiagnostic>) -> MacroExpansionDiagnostics {
        MacroExpansionDiagnostics {
            macro_kind: MacroExpansionKind::DefineSlots,
            macro_index,
            diagnostics: diags,
            exactness: ExpansionExactness::ExactConcrete,
            execution_status: ExpansionExecutionStatus::Completed,
        }
    }

    #[test]
    fn maps_cyclic_instantiation_to_cyclic_reference_kind() {
        let envelopes = vec![envelope(
            7,
            vec![diag(
                ExpansionStopReason::CyclicInstantiation,
                "cyclic-instantiation::/x.ts::A",
            )],
        )];
        let out = macro_expansion_to_audit_entries(&envelopes);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, AuditDiagnosticKind::CyclicReference);
        assert_eq!(out[0].macro_index, Some(7));
        assert!(out[0].message.contains("cyclic-instantiation"));
    }

    #[test]
    fn maps_cyclic_reference_to_cyclic_reference_kind() {
        let envelopes = vec![envelope(
            0,
            vec![diag(
                ExpansionStopReason::CyclicReference,
                "cycle-short-circuited@n",
            )],
        )];
        let out = macro_expansion_to_audit_entries(&envelopes);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, AuditDiagnosticKind::CyclicReference);
    }

    #[test]
    fn maps_idempotent_arm_to_dedicated_audit_kind() {
        let envelopes = vec![envelope(
            0,
            vec![diag(
                ExpansionStopReason::IdempotentArm,
                "duplicate-arm-short-circuited@n",
            )],
        )];
        let out = macro_expansion_to_audit_entries(&envelopes);
        assert_eq!(out.len(), 1);
        // Dedicated routing — pre-fix this projected to
        // `CyclicReference` via substring inspection on `context`,
        // even though `IdempotentArm` is a distinct audit class.
        assert_eq!(out[0].kind, AuditDiagnosticKind::IdempotentArm);
    }

    #[test]
    fn maps_instantiation_error_to_resolver_error_kind() {
        let envelopes = vec![envelope(
            0,
            vec![diag(
                ExpansionStopReason::InstantiationError,
                "instantiation-error::/x.ts::A::Other(\"bad\")",
            )],
        )];
        let out = macro_expansion_to_audit_entries(&envelopes);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, AuditDiagnosticKind::ResolverError);
    }

    #[test]
    fn maps_budget_exceeded_to_budget_exceeded_kind() {
        let envelopes = vec![envelope(
            0,
            vec![diag(
                ExpansionStopReason::BudgetExceeded,
                "pathological-input@n",
            )],
        )];
        let out = macro_expansion_to_audit_entries(&envelopes);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, AuditDiagnosticKind::BudgetExceeded);
    }

    #[test]
    fn maps_indeterminate_conditional_to_open_conditional_kind() {
        let envelopes = vec![envelope(
            0,
            vec![diag(
                ExpansionStopReason::IndeterminateConditional,
                "open-conditional@n",
            )],
        )];
        let out = macro_expansion_to_audit_entries(&envelopes);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, AuditDiagnosticKind::OpenConditional);
    }

    #[test]
    fn maps_empty_union_arm_to_dedicated_kind() {
        let envelopes = vec![envelope(
            0,
            vec![diag(
                ExpansionStopReason::EmptyUnionArm,
                "union-arm-empty@n#0",
            )],
        )];
        let out = macro_expansion_to_audit_entries(&envelopes);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, AuditDiagnosticKind::EmptyUnionArm);
    }

    #[test]
    fn flattens_multiple_envelopes_into_one_vec() {
        let envelopes = vec![
            envelope(
                3,
                vec![diag(ExpansionStopReason::CyclicInstantiation, "cyclic-foo")],
            ),
            envelope(
                5,
                vec![
                    diag(ExpansionStopReason::BudgetExceeded, "pathological"),
                    diag(ExpansionStopReason::IndeterminateConditional, "open"),
                ],
            ),
        ];
        let out = macro_expansion_to_audit_entries(&envelopes);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].macro_index, Some(3));
        assert_eq!(out[1].macro_index, Some(5));
        assert_eq!(out[2].macro_index, Some(5));
    }
}
