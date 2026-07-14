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

use super::output_sink::{surface_member_to_expanded_field, MemberValuePosition};
use super::publication_authority::{
    admit_published_member, read_surface_member_candidates, resolve_macro_payload,
    resolve_payload_surface, AdmittedPublishedMember,
};

pub(crate) fn project_props(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner: &DeclIdentity,
    file: &str,
    macro_index: usize,
    mac: &AnalyzedMacro,
    _snapshot: &FileAnalysisSnapshot,
    diag_sink: &mut Vec<MacroExpansionDiagnostics>,
    cursor: crate::meta_resolve::projection_demand::ProjectionCursor<'_>,
) -> Vec<ExpandedField> {
    // `cursor` carries the publication-boundary
    // demand. Each surface member is descended via
    // `cursor.descend_published_member(name)`: the macro publishes
    // every member NAME, and each member's type body is published as
    // a CARRIER (`Navigate` mode) unless the consumer explicitly
    // walked a deep path. This closes the Rule-5 depth leak where a
    // `Tool<INPUT, OUTPUT>`-typed prop would breadth-enumerate
    // `Tool`'s `outputSchema` / `execute` members into the surface.
    if !mac.is_type_based {
        return Vec::new();
    }

    let ctx: &dyn ResolverContext = query_engine.ctx;
    // Resolve the payload + surface through the publication-authority token
    // API, enumerate candidates, and ADMIT each under the cursor. Admission
    // applies the public-visibility filter, the derived-kind/cursor match, the
    // `descend_published_member` gate, AND records the
    // `MemberEdgeProvenance::PublishedField` origin edge BEFORE the dispatch
    // drops — so the Rule-5 compliance validator can attest that every
    // published member is a declared surface member.
    //
    // Ref-carrier surfaces (cross-file generic payloads like
    // `defineProps<AccordionProps<T>>()`) lower to a `SemanticNodeData::Ref`
    // shell where `read_surface_members` returns empty, so admission (and its
    // edge record) fires ZERO times. That is the shallow-by-default (L1)
    // contract: the carrier is published symbolically and the consumer
    // re-resolves it on demand, so there are no eagerly-flattened members to
    // attach `PublishedField` edges to — only concretely enumerated surface
    // members carry a member-edge origin here.
    let admitted: Vec<(AdmittedPublishedMember<'_>, _, _)> = {
        let dispatch = ProjectSemanticDispatch::new(ctx);
        let payload = match resolve_macro_payload(
            &dispatch,
            owner,
            file,
            macro_index,
            mac,
            AnalyzedMacroKind::DefineProps,
            MacroExpansionKind::DefineProps,
            diag_sink,
        ) {
            Some(payload) => payload,
            None => return Vec::new(),
        };

        let surface = match resolve_payload_surface(
            &dispatch,
            &payload,
            MacroExpansionKind::DefineProps,
            diag_sink,
        ) {
            Some(surface) => surface,
            None => return Vec::new(),
        };

        read_surface_member_candidates(ctx, &surface)
            .into_iter()
            .filter_map(|candidate| {
                let analyzed = mac
                    .prop_fields
                    .iter()
                    .find(|p| p.name == candidate.member().name.as_ref());
                let raw_type = analyzed.and_then(|p| p.type_annotation.clone());
                let shallow_payload = analyzed.and_then(|p| p.payload.clone());
                let admitted = admit_published_member(candidate, &cursor, &dispatch)?;
                Some((admitted, raw_type, shallow_payload))
            })
            .collect()
    };
    admitted
        .into_iter()
        .map(|(admitted, raw_type, shallow_payload)| {
            surface_member_to_expanded_field(
                query_engine,
                file,
                &admitted,
                raw_type,
                shallow_payload,
                mac.parsed_type_argument.as_ref(),
                MemberValuePosition::ShallowMember,
            )
        })
        .collect()
}
