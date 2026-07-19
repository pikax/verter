//! Host → FFI output conversions: virtual node kinds, diagnostics, update
//! results, virtual files, resolved IDs, remove results, cross-file results,
//! and host errors.

use verter_session as host;

use crate::types::*;

use super::offset::maybe_utf16_offset;
use super::string_helpers::{
    host_block_type_to_string, host_module_reference_analyzability_to_string,
    host_module_reference_semantics_to_string, host_module_reference_syntax_to_string,
};

/// Preserve the host's closed public-API failure identity at JS boundaries.
pub fn host_public_api_projection_error_to_ffi(
    error: host::PublicApiProjectionError,
) -> FfiPublicApiProjectionError {
    let unavailable_outcome = error.unavailable_outcome();
    let subject = match error.subject() {
        verter_compiler::tsc::TscFailureSubject::Macro { syntax_index } => {
            FfiTscFailureSubject::Macro { syntax_index }
        }
        verter_compiler::tsc::TscFailureSubject::ScriptSetupAttrs { source_range } => {
            FfiTscFailureSubject::ScriptSetupAttrs { source_range }
        }
    };
    FfiPublicApiProjectionError {
        code: error.code().to_string(),
        detail_code: error.detail_code().to_string(),
        subject,
        declaration_shape_reason: error
            .declaration_shape_reason()
            .map(|reason| reason.code().to_string()),
        member_ordinal: error.member_ordinal(),
        outcome_kind: unavailable_outcome.map(|outcome| outcome.kind_code().to_string()),
        outcome_reason: unavailable_outcome.map(|outcome| outcome.reason_code().to_string()),
        outcome_diagnostic: unavailable_outcome
            .and_then(|outcome| outcome.diagnostic().map(str::to_owned)),
    }
}

pub fn host_public_api_result_to_ffi(
    result: Result<Option<host::TscResponse>, host::PublicApiProjectionError>,
) -> FfiPublicApiResult {
    match result {
        Ok(value) => FfiPublicApiResult {
            value: value.map(|response| FfiTscResponse {
                code: response.code.to_string(),
                source_map: response.source_map.map(|map| map.to_string()),
            }),
            error: None,
        },
        Err(error) => FfiPublicApiResult {
            value: None,
            error: Some(host_public_api_projection_error_to_ffi(error)),
        },
    }
}

#[cfg(test)]
mod public_api_tests {
    use super::*;
    use verter_compiler::tsc::{
        TscFailureSubject, TscGenerationError, TscInvalidOutcome, TscUnavailableOutcome,
    };
    use verter_macro_dto::{
        MacroFailure, MacroInvalidReason, MacroPartialReason, UnresolvedReason, UnsupportedReason,
    };

    #[test]
    fn public_api_failure_preserves_closed_structured_identity() {
        let result = host_public_api_result_to_ffi(Err(host::PublicApiProjectionError::from(
            verter_compiler::tsc::TscGenerationError::UnsupportedDeclarationShape {
                subject: TscFailureSubject::Macro { syntax_index: 7 },
                reason: verter_compiler::tsc::TscDeclarationShapeReason::UnsupportedEnumShape,
            },
        )));

        assert!(result.value.is_none());
        let error = result.error.expect("failure must occupy the error rail");
        assert_eq!(error.code, "tsc-generation");
        assert_eq!(error.detail_code, "unsupported-declaration-shape");
        assert_eq!(
            error.subject,
            FfiTscFailureSubject::Macro { syntax_index: 7 }
        );
        assert_eq!(
            error.declaration_shape_reason.as_deref(),
            Some("unsupported-enum-shape")
        );
        assert_eq!(error.member_ordinal, None);
        assert_eq!(error.outcome_kind, None);
        assert_eq!(error.outcome_reason, None);
        assert_eq!(error.outcome_diagnostic, None);
    }

    #[test]
    fn public_api_failure_preserves_all_unavailable_outcome_arms() {
        let cases = [
            (
                TscUnavailableOutcome::Partial(MacroFailure::new(
                    MacroPartialReason::IncompleteTraversal,
                    Some("partial detail".to_string()),
                )),
                "partial",
                "incomplete-traversal",
                "partial detail",
            ),
            (
                TscUnavailableOutcome::Unresolved(MacroFailure::new(
                    UnresolvedReason::AmbiguousReference,
                    Some("unresolved detail".to_string()),
                )),
                "unresolved",
                "ambiguous-reference",
                "unresolved detail",
            ),
            (
                TscUnavailableOutcome::Unsupported(MacroFailure::new(
                    UnsupportedReason::SemanticConstruct,
                    Some("unsupported detail".to_string()),
                )),
                "unsupported",
                "semantic-construct",
                "unsupported detail",
            ),
            (
                TscUnavailableOutcome::Invalid(TscInvalidOutcome::Macro(MacroFailure::new(
                    MacroInvalidReason::NonObjectRoot,
                    Some("invalid detail".to_string()),
                ))),
                "invalid",
                "non-object-root",
                "invalid detail",
            ),
        ];

        for (syntax_index, (outcome, kind, reason, diagnostic)) in cases.into_iter().enumerate() {
            let result = host_public_api_result_to_ffi(Err(host::PublicApiProjectionError::from(
                TscGenerationError::UnavailableOutcome {
                    subject: TscFailureSubject::Macro {
                        syntax_index: syntax_index as u32,
                    },
                    outcome,
                },
            )));
            let error = result.error.expect("failure must occupy the error rail");

            assert!(result.value.is_none());
            assert_eq!(error.code, "tsc-generation");
            assert_eq!(error.detail_code, "unavailable-outcome");
            assert_eq!(
                error.subject,
                FfiTscFailureSubject::Macro {
                    syntax_index: syntax_index as u32,
                }
            );
            assert_eq!(error.declaration_shape_reason, None);
            assert_eq!(error.member_ordinal, None);
            assert_eq!(error.outcome_kind.as_deref(), Some(kind));
            assert_eq!(error.outcome_reason.as_deref(), Some(reason));
            assert_eq!(error.outcome_diagnostic.as_deref(), Some(diagnostic));
        }
    }

    #[test]
    fn public_api_failure_preserves_script_setup_attrs_subject() {
        let source_range = verter_span::Span::new(31, 37);
        let result = host_public_api_result_to_ffi(Err(host::PublicApiProjectionError::from(
            TscGenerationError::UnavailableOutcome {
                subject: TscFailureSubject::ScriptSetupAttrs { source_range },
                outcome: TscUnavailableOutcome::Invalid(TscInvalidOutcome::AuthoredTypeSyntax(
                    verter_compiler::tsc::TscInvalidAuthoredTypeReason::MalformedOrRecoveredTypeSyntax,
                )),
            },
        )));

        let error = result.error.expect("failure must occupy the error rail");
        assert_eq!(
            error.subject,
            FfiTscFailureSubject::ScriptSetupAttrs { source_range }
        );
        assert_eq!(error.outcome_kind.as_deref(), Some("invalid"));
        assert_eq!(
            error.outcome_reason.as_deref(),
            Some("malformed-or-recovered-type-syntax")
        );
        assert_eq!(error.outcome_diagnostic, None);
    }
}

pub fn host_preprocessor_request_to_ffi(req: &host::PreprocessorRequest) -> FfiPreprocessorRequest {
    FfiPreprocessorRequest {
        block_type: host_block_type_to_string(req.block_type),
        index: req.index as u32,
        lang: req.lang.clone(),
        content: req.content.clone(),
    }
}
pub(super) fn host_module_reference_to_ffi(
    input: host::ScriptModuleReference,
) -> FfiModuleReference {
    FfiModuleReference {
        syntax: host_module_reference_syntax_to_string(input.syntax),
        semantics: host_module_reference_semantics_to_string(input.semantics),
        is_type_only: input.is_type_only,
        raw_text: input.raw_text,
        literal_specifier: input.literal_specifier,
        finite_specifiers: input.finite_specifiers,
        static_prefix: input.static_prefix,
        analyzability: host_module_reference_analyzability_to_string(input.analyzability),
        span_start: input.span.start,
        span_end: input.span.end,
        expr_span_start: input.expr_span.start,
        expr_span_end: input.expr_span.end,
    }
}

pub fn host_node_kind_to_ffi(input: &host::VirtualNodeKind) -> FfiVirtualNodeKind {
    match input {
        host::VirtualNodeKind::Main => FfiVirtualNodeKind {
            kind: "main".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Script => FfiVirtualNodeKind {
            kind: "script".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Template => FfiVirtualNodeKind {
            kind: "template".to_string(),
            index: None,
        },
        host::VirtualNodeKind::Style { index } => FfiVirtualNodeKind {
            kind: "style".to_string(),
            index: Some(*index as u32),
        },
        host::VirtualNodeKind::Custom { index } => FfiVirtualNodeKind {
            kind: "custom".to_string(),
            index: Some(*index as u32),
        },
    }
}

pub fn host_diagnostics_to_ffi(
    input: &host::DiagnosticsSnapshot,
    source: Option<&str>,
) -> FfiDiagnosticsSnapshot {
    FfiDiagnosticsSnapshot {
        diagnostics: input
            .diagnostics
            .iter()
            .map(|d| FfiDiagnostic {
                severity: match d.severity {
                    host::HostSeverity::Error => "error".to_string(),
                    host::HostSeverity::Warning => "warning".to_string(),
                    host::HostSeverity::Info => "info".to_string(),
                },
                code: d.code.clone(),
                message: d.message.clone(),
                span_start: d
                    .span
                    .and_then(|s| maybe_utf16_offset(Some(s.start), source)),
                span_end: d.span.and_then(|s| maybe_utf16_offset(Some(s.end), source)),
            })
            .collect(),
        has_errors: input.has_errors,
    }
}

/// Convert a host update result to its FFI representation.
pub fn host_update_to_ffi(input: host::HostUpdateResult, source: Option<&str>) -> FfiUpdateResult {
    FfiUpdateResult {
        canonical_id: input.canonical_id,
        changed: input.changed,
        slice_changes: FfiSliceChanges {
            script_changed: input.slice_changes.script_changed,
            template_changed: input.slice_changes.template_changed,
            style_indices_changed: input
                .slice_changes
                .style_indices_changed
                .into_iter()
                .map(|i| i as u32)
                .collect(),
            custom_indices_changed: input
                .slice_changes
                .custom_indices_changed
                .into_iter()
                .map(|i| i as u32)
                .collect(),
            structure_changed: input.slice_changes.structure_changed,
            descriptor_changed: input.slice_changes.descriptor_changed,
        },
        changed_virtual_nodes: input
            .changed_virtual_nodes
            .iter()
            .map(host_node_kind_to_ffi)
            .collect(),
        removed_virtual_nodes: input
            .removed_virtual_nodes
            .iter()
            .map(host_node_kind_to_ffi)
            .collect(),
        changed_virtual_ids: input.changed_virtual_ids,
        removed_virtual_ids: input.removed_virtual_ids,
        changed_lsp_ids: input.changed_lsp_ids,
        removed_lsp_ids: input.removed_lsp_ids,
        diagnostics: host_diagnostics_to_ffi(&input.diagnostics, source),
        external_source_requests: input
            .external_source_requests
            .into_iter()
            .map(|req| FfiExternalSourceRequest {
                owner_canonical_id: req.owner_canonical_id,
                block_kind: match req.block_kind {
                    host::ExternalBlockKind::Script => "script".to_string(),
                    host::ExternalBlockKind::Template => "template".to_string(),
                    host::ExternalBlockKind::Style => "style".to_string(),
                    host::ExternalBlockKind::Custom => "custom".to_string(),
                },
                index: req.index as u32,
                specifier: req.specifier,
                resolved_canonical_id: req.resolved_canonical_id,
            })
            .collect(),
        import_specifiers: input
            .import_specifiers
            .into_iter()
            .map(|imp| FfiScriptImportInfo {
                source: imp.source,
                is_type_only: imp.is_type_only,
                bindings: imp.bindings,
            })
            .collect(),
        module_references: input
            .module_references
            .into_iter()
            .map(host_module_reference_to_ffi)
            .collect(),
        preprocessor_requests: input
            .preprocessor_requests
            .iter()
            .map(host_preprocessor_request_to_ffi)
            .collect(),
        export_signatures: input
            .export_signatures
            .into_iter()
            .map(|sig| FfiExportSignature {
                name: sig.name,
                is_type: sig.is_type,
                reexport_source: sig.reexport_source,
                reexport_local: sig.reexport_local,
            })
            .collect(),
        parse_duration_ms: input.parse_duration_ms,
    }
}

/// Convert a host virtual file response to its FFI representation.
pub fn host_virtual_file_to_ffi(
    input: host::VirtualFileResponse,
    source: Option<&str>,
) -> FfiVirtualFileResponse {
    FfiVirtualFileResponse {
        id: input.id,
        code: input.code.to_string(),
        source_map: input.source_map.as_ref().map(|s| s.to_string()),
        lang: input.lang,
        stale: input.stale,
        diagnostics: host_diagnostics_to_ffi(&input.diagnostics, source),
        meta: FfiVirtualMeta {
            scope_id: input.meta.scope_id,
            block_type: input.meta.block_type,
            style_index: input.meta.style_index.map(|i| i as u32),
            custom_index: input.meta.custom_index.map(|i| i as u32),
        },
        cache_hit: input.cache_hit,
        requested_mode: input.requested_mode.to_string(),
        actual_mode: input.actual_mode.to_string(),
        downgrade_reason: input.downgrade_reason.map(|r| r.to_string()),
    }
}

/// Convert a host resolved ID to its FFI representation.
pub fn host_resolved_id_to_ffi(input: host::ResolvedId) -> FfiResolvedId {
    FfiResolvedId {
        canonical_id: input.canonical_id,
        node_kind: host_node_kind_to_ffi(&input.node_kind),
        exists_in_host: input.exists_in_host,
        bundler_id: input.bundler_id,
        lsp_id: input.lsp_id,
    }
}

/// Convert a host remove result to its FFI representation.
pub fn host_remove_to_ffi(input: host::HostRemoveResult) -> FfiRemoveResult {
    FfiRemoveResult {
        canonical_id: input.canonical_id,
    }
}

/// Convert a `CrossFileResult` from the host to its FFI representation.
pub fn host_cross_file_result_to_ffi(
    input: host::cross_file::CrossFileResult,
) -> FfiCrossFileResult {
    FfiCrossFileResult {
        const_prop_overrides: input
            .const_prop_overrides
            .into_iter()
            .map(|(k, v)| (k, v.into_iter().collect()))
            .collect(),
        changed_files: input.changed_files,
        diagnostics: input
            .diagnostics
            .into_iter()
            .map(|d| FfiCrossFileDiagnostic {
                file_id: d.file_id,
                code: d.code,
                message: d.message,
            })
            .collect(),
    }
}

/// Convert a host error to a human-readable string.
///
/// Each consumer crate wraps this string in its native error type
/// (`napi::Error` or `JsValue`).
pub fn host_error_to_string(err: &host::HostError) -> String {
    match err {
        host::HostError::MissingSource { canonical_id } => {
            format!("HostError::MissingSource: {}", canonical_id)
        }
        host::HostError::InvalidQuery => "HostError::InvalidQuery".to_string(),
        host::HostError::MissingVirtualNode { canonical_id } => {
            format!("HostError::MissingVirtualNode: {}", canonical_id)
        }
        host::HostError::CompileError(failure) => {
            let summary = failure
                .diagnostics
                .diagnostics
                .iter()
                .map(|d| format!("[{}] {}", d.code, d.message))
                .collect::<Vec<_>>()
                .join("; ");
            format!("HostError::CompileError: {}", summary)
        }
        // Catch-all for scheduler-related errors (feature-gated in verter_session).
        #[allow(unreachable_patterns)]
        other => format!("HostError: {}", other),
    }
}
