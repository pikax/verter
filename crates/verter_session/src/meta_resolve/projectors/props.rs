//! `defineProps<T>()` projector.
//!
//! Resolves the macro's parsed type argument to a payload node through
//! `ResolveMacroPayload`, then enumerates the prop surface via an
//! empty-path `ProjectPath` in `Shallow` mode. Each surface member
//! becomes one `ExpandedField` with its `raw_type` populated from the
//! parser-side `AnalyzedPropField.type_annotation` when available
//! (preserves §7.4b parity).

use verter_semantic::analysis::component_meta::{MacroExpansionDiagnostics, MacroExpansionKind};
use verter_semantic::analysis::type_expand::ExpandedField;
use verter_semantic::analysis::{AnalyzedMacro, AnalyzedMacroKind};

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::DeclIdentity;
use crate::types::FileAnalysisSnapshot;

use super::{
    read_surface_members, resolve_macro_payload, resolve_payload_surface,
    surface_member_to_expanded_field,
};

/// Project a `defineProps<T>()` macro to a `Vec<ExpandedField>` for
/// publication into `evaluated_types.props`.
///
/// The empty `Vec` return is reserved for two scenarios:
///
/// 1. The macro is not type-based (no `parsed_type_argument`). The
///    parser-side analysis already covers runtime `defineProps`.
/// 2. A `Recursive` or `Error` result from `ResolveMacroPayload` /
///    `ProjectPath`. In both cases a `MacroExpansionDiagnostics`
///    envelope has been pushed to `diag_sink` per §7.5
///    silent-miss prevention — the caller observes the failure
///    through the analysis-wide diagnostics stream rather than
///    silently treating an empty `Vec` as a successful empty
///    surface.
pub(crate) fn project_props(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner: &DeclIdentity,
    file: &str,
    macro_index: usize,
    mac: &AnalyzedMacro,
    _snapshot: &FileAnalysisSnapshot,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
) -> Vec<ExpandedField> {
    if !mac.is_type_based {
        return Vec::new();
    }

    let ctx: &dyn ResolverContext = query_engine.ctx;
    let members_with_raw = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let payload_node = match resolve_macro_payload(
            &dispatch,
            owner,
            file,
            macro_index,
            mac,
            AnalyzedMacroKind::DefineProps,
            MacroExpansionKind::DefineProps,
            diag_sink,
        ) {
            Some(node) => node,
            None => return Vec::new(),
        };

        let surface_node = match resolve_payload_surface(
            &dispatch,
            payload_node,
            macro_index,
            MacroExpansionKind::DefineProps,
            diag_sink,
        ) {
            Some(node) => node,
            None => return Vec::new(),
        };

        let members = read_surface_members(ctx, surface_node);
        members
            .into_iter()
            .map(|member| {
                let analyzed = mac
                    .prop_fields
                    .iter()
                    .find(|p| p.name == member.name.as_ref());
                let raw_type = analyzed.and_then(|p| p.type_annotation.clone());
                let shallow_type_expr = analyzed.and_then(|p| p.type_expr.clone());
                let shallow_type_expr_scope = analyzed.and_then(|p| p.type_expr_scope.clone());
                (member, raw_type, shallow_type_expr, shallow_type_expr_scope)
            })
            .collect::<Vec<_>>()
    };
    members_with_raw
        .into_iter()
        .map(
            |(member, raw_type, shallow_type_expr, shallow_type_expr_scope)| {
                surface_member_to_expanded_field(
                    query_engine,
                    file,
                    &member,
                    raw_type,
                    shallow_type_expr,
                    shallow_type_expr_scope,
                )
            },
        )
        .collect()
}
