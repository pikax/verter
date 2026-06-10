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

pub(crate) fn project_model(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner: &DeclIdentity,
    file: &str,
    macro_index: usize,
    mac: &AnalyzedMacro,
    _snapshot: &FileAnalysisSnapshot,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
    cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
) -> Option<ExpandedField> {
    if !mac.is_type_based {
        return None;
    }

    let model_name = mac
        .model_name
        .clone()
        .unwrap_or_else(|| DEFAULT_MODEL_NAME.to_string());

    // Descend into the published model member.
    // `descend_published_member` returns `None` (the model is dropped
    // from the published surface) when a narrowed projection does not
    // admit the model name; for the whole-surface default it yields a
    // terminal carrier cursor. `project_model` raises a single payload
    // (no surface walk) so the carrier mode does not gate a per-member
    // breadth loop here — the descend gate IS the load-bearing use.
    let _member_cursor = cursor.descend_published_member(&model_name)?;

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

        // Emit `PublishedField` for the model member. `defineModel<T>()`
        // publishes the raised payload under `model_name` (defaulting
        // to `modelValue`). payload_node serves as both parent surface
        // and member value because model is a single-field projection
        // (no wrapping object surface separate from the payload).
        let model_name_arc: std::sync::Arc<str> = std::sync::Arc::from(model_name.as_str());
        dispatch.record_published_field_edge(owner, payload_node, payload_node, &model_name_arc);

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

    // An operator-shape model type (`defineModel<Foo['a']>`) carries
    // EXPLICIT path demand inside the type expression — reduce it
    // path-precisely. A bare carrier (`defineModel<Tool<I, O>>`) has
    // no operator node, so `raised` is returned verbatim — published
    // as a carrier.
    let r#type = if super::type_expr_contains_reducible_operator(&raised) {
        super::super::materialize::materialize_component_meta_type_expr_until_stable(
            &raised,
            file,
            ProjectionMode::Navigate,
            query_engine,
        )
    } else {
        raised
    };

    let name = model_name;

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
        // `defineModel<T>()` synthesizes the model member at the
        // macro's T position. The member is structurally
        // author-declared in the macro's type argument by virtue of
        // the `defineModel` syntax itself — set `true`.
        declared_in_macro_type_arg: true,
    })
}
