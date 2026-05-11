//! `defineExpose({ ... })` projector.
//!
//! `defineExpose` always takes an object expression argument (or no
//! argument). The exposed surface is computed by the parser's
//! [`extract_exposed_from_macro`] from `mac.expose_fields` plus
//! follow-on binding lookups in `input.bindings`. The dispatch path
//! through `ResolveMacroPayload { kind: DefineExpose }` is documented
//! as 0 args → Miss, else type_args[0] unchanged — so the projector
//! mostly mirrors what the parser already provides, but routes
//! through dispatch when a generic type argument is present.
//!
//! Dispatch return is reduced to surface members via empty-path
//! Shallow `ProjectPath`. For the common `defineExpose({ a, b })`
//! object-literal case, the parser's analyzed `expose_fields` is the
//! authoritative source. This projector therefore returns
//! `Vec::new()` when the macro lacks a `parsed_type_argument`.

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

/// Project a `defineExpose<T>()` (or `defineExpose({})`) macro.
///
/// Returns `Vec::new()` when the macro has no `parsed_type_argument`
/// — the parser-side analysis (which reads
/// `AnalyzedMacro::expose_fields` and `input.bindings`) is the
/// authoritative source for runtime `defineExpose` invocations.
pub(crate) fn project_exposed(
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
    let members = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let payload_node = match resolve_macro_payload(
            &dispatch,
            owner,
            file,
            macro_index,
            mac,
            AnalyzedMacroKind::DefineExpose,
            // Reuse DefineProps as the closest expansion-kind for now;
            // future enrichment of `MacroExpansionKind` can split this.
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

        read_surface_members(ctx, surface_node)
    };
    members
        .into_iter()
        .map(|member| {
            surface_member_to_expanded_field(query_engine, file, &member, None, None, None)
        })
        .collect()
}
