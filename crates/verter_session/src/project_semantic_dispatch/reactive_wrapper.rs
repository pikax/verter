//! Shared package-backed reactive-wrapper vocabulary and demand entries.
//!
//! ONE owner for the question "does this authored reference head resolve to a
//! reactive wrapper exported by the `vue` package?". Every consumer — the
//! template-class fact rows and the value-space function-signature return
//! position — composes the SAME three landed pieces:
//! [`ProjectSemanticDispatch::resolve_authored_reference_route`] for the
//! authored route proof, [`wrapper_candidate_for_route`] for the exact
//! package-backed gate plus the closed role vocabulary, and
//! [`ProjectSemanticDispatch::demand_terminal_symbol_instantiation`] for the
//! terminal identity proof over the shared semantic graph.
//!
//! There is no wrapper classifier here beyond [`wrapper_role_for_vue_export`],
//! and no name is matched outside it: a role is claimed only for a route that
//! TERMINATES at a package-backed `vue` export, never from terminal-name
//! equality.

use std::sync::Arc;

use verter_type_expr::facts::SemanticTypeSource;
use verter_type_expr::locators::AuthoredBodyLocator;
use verter_type_expr::{
    PropCallableRoleUnresolvedReason, ReactiveWrapperImportProvenance, ReactiveWrapperRole,
    ReactiveWrapperUnresolvedReason, ResolutionProvenance, ResolvedSymbolIdentity, TopLevelOwnerId,
};

use super::query_error_disposition::classify_query_error;
use super::semantic_source::{SourceRaiseContext, SourceRaiseOutcome};
use super::symbol_identity::TerminalSymbolInstantiationDemandOutcome;
use super::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{ProjectionMode, ProjectionReductionContext};

/// An exact package-backed wrapper candidate: the role the terminal export
/// names, the terminal identity the graph demand must confirm, and the complete
/// authored route provenance to publish once it does.
#[derive(Debug, Clone)]
pub(super) struct WrapperCandidate {
    pub(super) role: ReactiveWrapperRole,
    pub(super) symbol: ResolvedSymbolIdentity,
    pub(super) provenance: ReactiveWrapperImportProvenance,
}

/// The exact route gate: the composed route must terminate at a PACKAGE-BACKED
/// `vue` export whose name is in the closed wrapper vocabulary. Either half
/// alone is insufficient — a workspace file that spells `vue` as its terminal
/// import source, or a package-backed export named `Ref` reached through a
/// non-`vue` edge, both fail closed here.
pub(super) fn wrapper_candidate_for_route(
    ctx: &dyn ResolverContext,
    route: super::symbol_identity::ResolvedReferenceRoute,
) -> Option<WrapperCandidate> {
    if route.terminal_import_source.as_ref() != "vue"
        || !ctx.workspace_is_package_backed(route.terminal.canonical_id.as_ref())
    {
        return None;
    }
    let role = wrapper_role_for_vue_export(route.terminal.symbol.as_ref())?;
    Some(WrapperCandidate {
        role,
        symbol: route.terminal,
        provenance: ReactiveWrapperImportProvenance {
            authored_head: route.authored_head,
            package: Arc::from("vue"),
            import_source: route.import_source,
            local_binding: route.local_binding,
            owner_canonical: route.owner_canonical,
            imported_name: route.imported_name,
            terminal_import_source: route.terminal_import_source,
            local_alias_hops: route.local_alias_hops,
            exactness: route.exactness,
            provenance: ResolutionProvenance::FrameworkSurface,
        },
    })
}

/// The CLOSED wrapper vocabulary, keyed on the terminal `vue` export name that
/// the route proof already established. `WritableComputedRef` normalizes onto
/// `ComputedRef`; every other export outside the vocabulary is `None`.
pub(super) fn wrapper_role_for_vue_export(name: &str) -> Option<ReactiveWrapperRole> {
    match name {
        "Ref" => Some(ReactiveWrapperRole::Ref),
        "ShallowRef" => Some(ReactiveWrapperRole::ShallowRef),
        "ComputedRef" | "WritableComputedRef" => Some(ReactiveWrapperRole::ComputedRef),
        "ModelRef" => Some(ReactiveWrapperRole::ModelRef),
        "Reactive" => Some(ReactiveWrapperRole::Reactive),
        "ShallowReactive" => Some(ReactiveWrapperRole::ShallowReactive),
        _ => None,
    }
}

fn unresolved(
    reason: ReactiveWrapperUnresolvedReason,
) -> (ReactiveWrapperRole, Option<ReactiveWrapperImportProvenance>) {
    (ReactiveWrapperRole::Unresolved { reason }, None)
}

/// The reactive-wrapper role of ONE value-space function signature's AUTHORED
/// return annotation, resolved at consumer demand.
///
/// Composed entirely from landed shared machinery, in this order:
///
/// 1. `prepared_value_decl` — a fact COPY; no locator deref, no resolution.
/// 2. `signatures[signature_ordinal].return_reference_head` — the
///    producer-minted authored head. `Unavailable` (an inferred or absent
///    return annotation) has nothing to resolve and fails closed;
///    `NotReference` (an authored non-reference return) is a COMPLETE
///    non-wrapper proof.
/// 3. `resolve_authored_reference_route` — the shared authored-route walk over
///    local aliases, import edges, re-exports and barrels, under the existing
///    connected-demand work envelope and cycle set. Demand begins ONLY for this
///    requested subject: there is no owner-wide wrapper-candidate scan.
/// 4. `wrapper_candidate_for_route` — the exact package-backed `vue` gate.
/// 5. `raise_semantic_type_source_to_hot` over the signature's authored return
///    body locator, then `demand_terminal_symbol_instantiation` — the terminal
///    identity proof through the ONE shared type-resolution engine. A role is
///    published only when the graph actually reaches the candidate's terminal.
///
/// Publishes nothing: the result is served to the caller, and every cache the
/// underlying queries touch is governed by its own admission rails. A partial
/// at any step becomes `Unresolved { reason }` with no provenance — never a
/// guessed role, and never `None` (which is reserved for a COMPLETED
/// non-wrapper proof).
///
/// This ordinal-addressable form is reached in production through
/// [`wrapper_role_for_sole_value_signature_return`], the consumer entry that
/// first proves the declaration carries exactly one signature. The production
/// consumer is component-meta's script-binding surface: the component-meta
/// extract demands the role for a whole-value composable-call binding under its
/// own request-bound `ResolverContext` and fact tracer, and publishes it on
/// `BindingAnalysis.return_wrapper_role`.
pub(crate) fn wrapper_role_for_value_signature_return(
    dispatch: &ProjectSemanticDispatch<'_>,
    canonical: &str,
    owner: TopLevelOwnerId,
    symbol: &str,
    signature_ordinal: usize,
) -> (ReactiveWrapperRole, Option<ReactiveWrapperImportProvenance>) {
    let Some(prepared) = dispatch
        .ctx
        .prepared_value_decl_return_only(canonical, owner, symbol)
    else {
        return unresolved(ReactiveWrapperUnresolvedReason::AnalysisUnavailable);
    };
    let Some(signature) = prepared.signatures.get(signature_ordinal) else {
        return unresolved(ReactiveWrapperUnresolvedReason::AnalysisUnavailable);
    };
    let route = match dispatch.resolve_authored_reference_route(
        canonical,
        owner,
        &signature.return_reference_head,
    ) {
        Ok(Some(route)) => route,
        // `NotReference`: the return annotation was authored and is not a type
        // reference at all — a completed non-wrapper proof. Also the arm a
        // resolved LOCAL declaration reaches (no import edge participated), so a
        // local `interface Ref<T>` is proven non-Vue rather than guessed.
        Ok(None) => return (ReactiveWrapperRole::None, None),
        Err(reason) => return unresolved(wrapper_reason_from_identity(reason)),
    };
    // A resolved route that does not terminate at a package-backed `vue` export
    // in the closed vocabulary is a completed non-wrapper proof.
    let Some(candidate) = wrapper_candidate_for_route(dispatch.ctx, route) else {
        return (ReactiveWrapperRole::None, None);
    };
    let verter_type_expr::facts::FunctionReturnSource::Declared(return_locator) =
        &signature.return_source
    else {
        return unresolved(ReactiveWrapperUnresolvedReason::MissingDependency);
    };
    let source =
        SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(return_locator.slot().clone()));
    // TYPED raise boundary: a genuine absence stays `MissingDependency`, but a
    // typed query failure publishes ITS OWN reason (a budget trip, a
    // cancellation, a cycle, an unsupported surface, a fault) instead of being
    // erased into "missing dependency".
    let hot = match dispatch.raise_semantic_type_source_to_hot(
        &source,
        SourceRaiseContext {
            scope_canonical_id: canonical,
            scope_owner: owner,
            context: ProjectionReductionContext::structural_transit_with_mode(
                ProjectionMode::Navigate,
            ),
            interior_failures: None,
        },
    ) {
        SourceRaiseOutcome::Raised(hot) => hot,
        SourceRaiseOutcome::Absent => {
            return unresolved(ReactiveWrapperUnresolvedReason::MissingDependency);
        }
        SourceRaiseOutcome::Failed(err) => {
            return unresolved(classify_query_error(&err).wrapper_reason());
        }
    };
    let expected = [candidate.symbol.clone()];
    match dispatch.demand_terminal_symbol_instantiation(hot.node(), &expected) {
        TerminalSymbolInstantiationDemandOutcome::Complete(Some(terminal))
            if terminal.symbol == candidate.symbol =>
        {
            (candidate.role, Some(candidate.provenance))
        }
        TerminalSymbolInstantiationDemandOutcome::Partial(reason) => {
            unresolved(wrapper_reason_from_identity(reason))
        }
        // The authored route named a wrapper candidate, but the graph did not
        // reach that terminal instantiation. Publishing `None` here would claim
        // a completed non-wrapper proof the demand did not produce, so this
        // fails closed instead.
        TerminalSymbolInstantiationDemandOutcome::Complete(_) => {
            unresolved(ReactiveWrapperUnresolvedReason::Unsupported)
        }
    }
}

/// The role of a value declaration's SOLE function signature's authored return
/// annotation — the CONSUMER entry.
///
/// Delegates to [`wrapper_role_for_value_signature_return`] at ordinal 0 once it
/// has proven the declaration carries exactly one signature. That proof is a
/// closed semantic rule, not a convenience: a declaration carrying MORE THAN one
/// signature is a merged overload group, and which overload a call site selects
/// requires argument-based overload resolution. Guessing ordinal 0 would publish
/// one overload's wrapper family as if it were the call's, so this fails closed
/// with `Unresolved { Unsupported }` instead. A declaration with NO signature at
/// all is `AnalysisUnavailable`, at parity with the delegate.
pub(crate) fn wrapper_role_for_sole_value_signature_return(
    dispatch: &ProjectSemanticDispatch<'_>,
    canonical: &str,
    owner: TopLevelOwnerId,
    symbol: &str,
) -> (ReactiveWrapperRole, Option<ReactiveWrapperImportProvenance>) {
    let Some(prepared) = dispatch
        .ctx
        .prepared_value_decl_return_only(canonical, owner, symbol)
    else {
        return unresolved(ReactiveWrapperUnresolvedReason::AnalysisUnavailable);
    };
    match prepared.signatures.len() {
        1 => wrapper_role_for_value_signature_return(dispatch, canonical, owner, symbol, 0),
        0 => unresolved(ReactiveWrapperUnresolvedReason::AnalysisUnavailable),
        _ => unresolved(ReactiveWrapperUnresolvedReason::Unsupported),
    }
}

/// The wrapper half of the shared identity-partial reason mapping.
fn wrapper_reason_from_identity(
    reason: PropCallableRoleUnresolvedReason,
) -> ReactiveWrapperUnresolvedReason {
    super::template_class_facts::unresolved_reasons_from_identity(reason).1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{HostConfig, UpsertRequest};
    use crate::VerterHost;

    #[test]
    fn wrapper_export_vocabulary_is_closed_and_writable_computed_normalizes() {
        assert_eq!(
            wrapper_role_for_vue_export("WritableComputedRef"),
            Some(ReactiveWrapperRole::ComputedRef)
        );
        assert_eq!(
            wrapper_role_for_vue_export("ShallowReactive"),
            Some(ReactiveWrapperRole::ShallowReactive)
        );
        assert_eq!(wrapper_role_for_vue_export("LocalRef"), None);
    }

    /// Every identity-partial class keeps its OWN typed wrapper reason. An
    /// implementation that collapsed the envelope classes onto one value (or onto
    /// `Fault`) fails here — which is what makes the envelope arm of the demand
    /// entry's typed degradation meaningful.
    #[test]
    fn identity_partial_reasons_map_one_to_one_without_collapse() {
        let cases = [
            (
                PropCallableRoleUnresolvedReason::BudgetExceeded,
                ReactiveWrapperUnresolvedReason::BudgetExceeded,
            ),
            (
                PropCallableRoleUnresolvedReason::WorkLimitExceeded,
                ReactiveWrapperUnresolvedReason::WorkLimitExceeded,
            ),
            (
                PropCallableRoleUnresolvedReason::Cycle,
                ReactiveWrapperUnresolvedReason::Cycle,
            ),
            (
                PropCallableRoleUnresolvedReason::MissingDependency,
                ReactiveWrapperUnresolvedReason::MissingDependency,
            ),
            (
                PropCallableRoleUnresolvedReason::Unsupported,
                ReactiveWrapperUnresolvedReason::Unsupported,
            ),
            (
                PropCallableRoleUnresolvedReason::AnalysisUnavailable,
                ReactiveWrapperUnresolvedReason::AnalysisUnavailable,
            ),
            (
                PropCallableRoleUnresolvedReason::Fault,
                ReactiveWrapperUnresolvedReason::Fault,
            ),
        ];
        for (identity, expected) in cases {
            assert_eq!(
                wrapper_reason_from_identity(identity),
                expected,
                "{identity:?} must keep its own typed wrapper reason"
            );
        }
        // Distinctness of the mapped values (a constant mapping fails here).
        let mapped = cases
            .iter()
            .map(|(identity, _)| wrapper_reason_from_identity(*identity))
            .collect::<Vec<_>>();
        for index in 1..mapped.len() {
            assert!(
                !mapped[..index].contains(&mapped[index]),
                "mapped reason {:?} must not alias an earlier class",
                mapped[index]
            );
        }
    }

    /// A CLAMPED connected-work envelope degrades the return role to the exact
    /// typed envelope reason with no provenance — never a wrapper role guessed
    /// from the authored spelling. The unclamped control on the same host proves
    /// the subject is exactly resolvable, so the clamp is what degrades it.
    ///
    /// The envelope limit is settable only inside this module tree, which is why
    /// the host-boundary acceptance test
    /// (`return_wrapper_role_degrades_typed_and_is_never_warmed`) delegates this
    /// arm here: this test constructs its dispatch through the direct-host test seam, so
    /// `current_request_budget()` is `None` at the sole projection-op charge
    /// site (`project_semantic_dispatch/mod.rs`) and the envelope class is
    /// proven through the connected-work limit rather than the projection fuse.
    ///
    /// The production consumer resolves under the component-meta request's
    /// already-installed `RequestContext` instead — that the demand runs through
    /// the request's own context is proven at the public
    /// boundary by
    /// `component_meta_binding_return_wrapper_role_demand_is_request_bound`
    /// (a session overlay decides the role). No test asserts a projection-fuse
    /// TRIP on the consumer path; the envelope class stays proven here.
    #[test]
    fn clamped_connected_work_envelope_degrades_the_return_role_typed() {
        let upsert = |host: &VerterHost, canonical: &str, source: &str| {
            let _ = host
                .upsert(UpsertRequest {
                    canonical_id: Some(canonical.to_string()),
                    input_id: canonical.to_string(),
                    source: Arc::from(source),
                    file_language: verter_language::FileLanguage::script_ts(),
                    aliases: Vec::new(),
                })
                .expect("upsert");
        };
        let make = || {
            let host = VerterHost::new_standalone(HostConfig::default());
            upsert(
                &host,
                "/workspace/node_modules/vue/index.d.ts",
                "export interface Ref<T> { value: T }\n",
            );
            upsert(
                &host,
                "/workspace/src/subject.ts",
                "import type { Ref } from 'vue'\n\
                 export function getValue(): Ref<number> { return null as never; }\n",
            );
            host.set_import_dependencies(
                "/workspace/src/subject.ts",
                vec![crate::types::DependencyResolution {
                    specifier: "vue".to_string(),
                    resolved_canonical_id: Some(
                        "/workspace/node_modules/vue/index.d.ts".to_string(),
                    ),
                    possible_canonical_ids: Vec::new(),
                }],
            );
            host
        };
        let role_for = |host: &VerterHost, work_limit: Option<usize>| {
            let dispatch = ProjectSemanticDispatch::new(host);
            if let Some(work_limit) = work_limit {
                dispatch.set_connected_limits_for_tests(work_limit, u16::MAX);
            }
            wrapper_role_for_value_signature_return(
                &dispatch,
                "/workspace/src/subject.ts",
                TopLevelOwnerId::ordinary_file(),
                "getValue",
                0,
            )
        };

        // Control: unclamped, on its own cold host — exactly resolvable.
        let (role, provenance) = role_for(&make(), None);
        assert_eq!(role, ReactiveWrapperRole::Ref);
        assert_eq!(
            provenance
                .expect("route proof")
                .terminal_import_source
                .as_ref(),
            "vue"
        );

        // Clamped: the shared envelope trips and the role degrades typed.
        let (role, provenance) = role_for(&make(), Some(0));
        assert_eq!(
            role,
            ReactiveWrapperRole::Unresolved {
                reason: ReactiveWrapperUnresolvedReason::WorkLimitExceeded
            },
            "a tripped connected-work envelope must publish the exact typed reason"
        );
        assert!(
            provenance.is_none(),
            "a truncated demand must publish no provenance"
        );
        assert_ne!(
            role,
            ReactiveWrapperRole::None,
            "an envelope trip is NOT a completed non-wrapper proof"
        );
    }
}
