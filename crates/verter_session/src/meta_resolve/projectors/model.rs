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
use verter_semantic::analysis::{AnalyzedMacro, AnalyzedMacroKind};
use verter_type_expr::TypeExpr;

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{DeclIdentity, ProjectionMode};
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
///
/// After raising the payload, the resulting `TypeExpr` is run
/// through [`materialize_component_meta_type_expr_until_stable`] in
/// `Expanded` mode so nested `IndexedAccess` chains collapse to the
/// concrete leaf shape — the same self-reduction contract every
/// per-macro projector honours.
pub(crate) fn project_model(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
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

    let ctx: &dyn ResolverContext = query_engine.ctx;
    let (raised, exactness, raise_failed) = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let payload_node = resolve_macro_payload(
            &dispatch,
            owner,
            file,
            macro_index,
            mac,
            AnalyzedMacroKind::DefineModel,
            MacroExpansionKind::DefineProps,
            diag_sink,
        )?;

        let (raised, raise_failed) = match dispatch.raise_node_to_type_expr(payload_node) {
            Some(expr) => (expr, false),
            None => (TypeExpr::Unknown { raw: String::new() }, true),
        };
        let exactness = classify_node(&dispatch, payload_node);
        (raised, exactness, raise_failed)
    };

    if raise_failed {
        // Record raise failure so the consumer observes the missing
        // payload through the diagnostic stream rather than as a
        // silent `Unknown` shell.
        diag_sink.push(macro_expansion_for_query_error(
            macro_index,
            MacroExpansionKind::DefineProps,
            "model-payload-raise-failed".to_string(),
        ));
    }

    let r#type = if super::type_expr_contains_reducible_operator(&raised) {
        super::super::materialize::materialize_component_meta_type_expr_until_stable(
            &raised,
            file,
            ProjectionMode::Expanded,
            query_engine,
        )
    } else {
        raised
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
        exactness,
        execution_status: ExpansionExecutionStatus::Completed,
        diagnostics: Vec::new(),
        shallow_type_expr: None,
        shallow_type_expr_scope: None,
    })
}
