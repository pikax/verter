//! `defineModel<T>()` projector.
//!
//! `defineModel` is a special-cased macro form: a single invocation
//! produces both a prop entry (the model name) and a corresponding
//! `update:modelName` event. The shared `ResolveMacroPayload` query
//! returns the resolved T payload; this projector raises that to a
//! [`TypeExpr`] so the parser-side analysis (which already walks
//! `defineModel` macros and constructs `ModelAnalysis` rows from the
//! pair) can consume the resolved type without re-resolving it.
//!
//! The synthesis of the `{ model_name: T, "update:model_name": (val: T) -> void }`
//! shape lives on the parser side
//! ([`verter_semantic::analysis::component_meta::synthesize_model_prop_and_event`])
//! and reads `evaluated_types` to enrich the type expressions. This
//! projector therefore returns a single [`ExpandedField`] with the
//! resolved T as `r#type`. The field's `name` echoes the
//! analyzed_macro's `model_name` (or the default `modelValue`) so the
//! parser-side merge can match by name.

use verter_semantic::analysis::component_meta::{MacroExpansionDiagnostics, MacroExpansionKind};
use verter_semantic::analysis::type_expand::{ExpandedField, ExpansionExecutionStatus};
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::{AnalyzedMacro, AnalyzedMacroKind};

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::DeclIdentity;
use crate::types::FileAnalysisSnapshot;

use super::{macro_expansion_for_query_error, resolve_macro_payload};
use crate::meta_resolve::exactness::classify_node;

/// Default model property name when `defineModel()` is called without
/// an explicit name argument.
const DEFAULT_MODEL_NAME: &str = "modelValue";

/// Project a `defineModel<T>()` macro to a single [`ExpandedField`].
///
/// Returns `None` when:
/// - the macro is not type-based (no `parsed_type_argument` to
///   resolve), or
/// - `ResolveMacroPayload` returned `Recursive` / `Error` (a
///   diagnostic has been pushed to `diag_sink`).
///
/// The returned field's `name` is the resolved model name from
/// `mac.model_name` or the [`DEFAULT_MODEL_NAME`] fallback.
pub(crate) fn project_model(
    dispatch: &ProjectSemanticDispatch<'_>,
    _ctx: &dyn ResolverContext,
    owner: &DeclIdentity,
    file: &str,
    macro_index: usize,
    mac: &AnalyzedMacro,
    _snapshot: &FileAnalysisSnapshot,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) -> Option<ExpandedField> {
    if !mac.is_type_based {
        return None;
    }

    let payload_node = resolve_macro_payload(
        dispatch,
        owner,
        file,
        macro_index,
        mac,
        AnalyzedMacroKind::DefineModel,
        MacroExpansionKind::DefineProps,
        diag_sink,
    )?;

    let r#type = match dispatch.raise_node_to_type_expr(payload_node) {
        Some(expr) => expr,
        None => {
            // Raise failed — record an error so the consumer can
            // observe the projection failed.
            diag_sink.push(macro_expansion_for_query_error(
                macro_index,
                MacroExpansionKind::DefineProps,
                "model-payload-raise-failed".to_string(),
            ));
            TypeExpr::Unknown { raw: String::new() }
        }
    };

    let name = mac
        .model_name
        .clone()
        .unwrap_or_else(|| DEFAULT_MODEL_NAME.to_string());

    Some(ExpandedField {
        name,
        r#type,
        raw_type: None,
        optional: false,
        exactness: classify_node(dispatch, payload_node),
        execution_status: ExpansionExecutionStatus::Completed,
        diagnostics: Vec::new(),
    })
}
