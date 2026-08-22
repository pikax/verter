//! Demand-driven semantic facts for dynamic template-class subjects.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_compiler::compile::template_data::RawTemplateData;
use verter_semantic::analysis::{
    AnalyzedBinding, AnalyzedMacro, AnalyzedMacroKind, TemplateClassFactsCompleteness,
    TemplateClassSemanticFactRow, TemplateClassSemanticFacts, TemplateClassSubject,
};
use verter_type_expr::facts::{ClosedTypeFact, LeafTypeFact, SemanticTypeSource};
use verter_type_expr::locators::{AuthoredBodyLocator, MacroPayloadPosition};
use verter_type_expr::{
    ClosedLiteralDomain, ClosedLiteralDomainUnresolvedReason, DeclBindingKey,
    PropCallableRoleUnresolvedReason, ReactiveWrapperRole, ReactiveWrapperUnresolvedReason,
    ResolutionExactness, TopLevelOwnerId,
};

use super::evaluate::StructuralFactDemandOutcome;
use super::query_error_disposition::classify_query_error;
use super::reactive_wrapper::{wrapper_candidate_for_route, WrapperCandidate};
use super::semantic_source::{SourceRaiseContext, SourceRaiseOutcome};
use super::symbol_identity::TerminalSymbolInstantiationDemandOutcome;
use super::ProjectSemanticDispatch;
use crate::fact_signature_helpers::ReadSetSignature;
use crate::resolver_core::{FactReadSetFinalise, RequestBoundResolverContext, ResolverContext};
use crate::resolver_store::ColdSeedHostStoreView;
use crate::semantic_query::{
    LiteralValue, ProjectionMode, ProjectionReductionContext, QueryError, QueryResult, ScopeId,
    SemanticNodeData, SemanticNodeId, ValueRootKey,
};

pub(crate) type SessionTemplateClassSemanticFacts = TemplateClassSemanticFacts<ReadSetSignature>;

#[derive(Clone, Copy)]
pub(crate) struct TemplateClassScriptInputs<'a> {
    pub(crate) macros: &'a [AnalyzedMacro],
    pub(crate) bindings: &'a [AnalyzedBinding],
}

/// Why a template-class fact set may not populate BASE caches.
///
/// Every variant is a property of the computation's INPUTS (bytes the store
/// never published) or of the store-view seed the resolver context was built
/// from. Artifact-cache warmth is deliberately NOT one of them: "the
/// content-addressed artifact store holds no entry for this content hash yet"
/// is neither an overlay nor a fenced input, and it appears in no enumerated
/// `ReturnOnly` trigger (overflow, budget exhaustion, cancellation, generation
/// supersession, incomplete self-rooting, unresolved provenance).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TemplateClassFenceReason {
    /// The bytes come from a session overlay.
    SessionOverlay,
    /// The bytes are store-published, but the resolver context seeded from a
    /// known non-current (`StoreViewRead::ReturnOnly`) store-view read.
    NonCurrentSeed,
}

/// Whether a template-class fact set may populate BASE caches.
///
/// TYPED ADMISSION — no boolean flag decides cacheability. The only inputs
/// that can yield [`Self::Fenced`] are a call site's own attestation about the
/// bytes it is handing the builder and the store-view seed's OWN currentness
/// proof, composed through [`Self::narrowed_by_seed`]. That method takes the
/// real [`ColdSeedHostStoreView`], not a flag, and there is no `bool`
/// parameter, no `From<bool>`, and no cache-presence constructor anywhere on
/// this type — so a call site cannot re-derive the fence from artifact
/// warmth, which is exactly the derivation that made publication scope depend
/// on the entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TemplateClassPublicationScope {
    /// Store-published bytes over a seed that is not known-stale: the fact set
    /// may warm the class-fact rails and the base raw-template slot.
    BasePublishable,
    /// Return-only: serve the facts to this caller, publish nothing.
    Fenced(TemplateClassFenceReason),
}

impl TemplateClassPublicationScope {
    /// Compose a call site's bytes attestation with the store-view seed's OWN
    /// currentness proof ([`ColdSeedHostStoreView::is_current`]).
    ///
    /// An already-fenced attestation keeps its reason (a content override does
    /// not become base-publishable by seeding from a current read);
    /// store-published bytes over a known-stale seed narrow to
    /// [`TemplateClassFenceReason::NonCurrentSeed`].
    #[must_use]
    pub(crate) fn narrowed_by_seed(self, seed: &ColdSeedHostStoreView) -> Self {
        match self {
            Self::Fenced(reason) => Self::Fenced(reason),
            Self::BasePublishable if seed.is_current() => Self::BasePublishable,
            Self::BasePublishable => Self::Fenced(TemplateClassFenceReason::NonCurrentSeed),
        }
    }

    /// Whether this scope may reach the base publication rails.
    pub(crate) fn is_base_publishable(self) -> bool {
        matches!(self, Self::BasePublishable)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RequestedSubject {
    Binding(Arc<str>),
    Prop {
        props_root: Arc<str>,
        member: Arc<str>,
    },
}

/// Build exact facts through the caller's already-selected resolver context.
///
/// The context must be REQUEST-BOUND. `classify_binding` demands
/// [`ResolverContext::prepared_value_decl`] for every template `:class`
/// subject that resolves to a script binding, and a prepared declaration can
/// only be served against a per-request store view. Taking
/// [`RequestBoundResolverContext`] — the sealed marker implemented for
/// `HostResolverContext` and `SessionResolverContext` but never for
/// `VerterHost` — makes a bare-host binding a compile error instead of a
/// cache-presence-dependent runtime abort at the caller's chosen branch.
pub(crate) fn build_template_class_semantic_facts(
    ctx: &dyn RequestBoundResolverContext,
    canonical: &str,
    whole_hash: verter_semantic::analysis::Hash16,
    script: TemplateClassScriptInputs<'_>,
    raw: &RawTemplateData,
    scope: TemplateClassPublicationScope,
) -> SessionTemplateClassSemanticFacts {
    // Stamp the revision the classification ACTUALLY RESOLVED AGAINST — the
    // owner shallow state the selected resolver context serves — not the
    // caller's argument. Stamping the argument makes the converter's coherence
    // gate a tautology (every lane hands the same local variable to the builder
    // and to the index), so a future torn `(source, whole_hash)` capture pair
    // would produce facts computed on one revision, stamped with another, and
    // the gate would still accept them. Sourcing the stamp from the resolved
    // state makes the comparison a real cross-check.
    //
    // An ABSENT shallow state means nothing was resolved against, so there is
    // no independent revision to cross-check; the caller's revision is stamped
    // and the resulting rows are necessarily unresolved.
    let resolved_whole_hash = ctx
        .shallow_file_state(canonical)
        .map_or(whole_hash, |state| state.whole_hash);
    let ((subjects, rows, mut completeness), finalise) =
        crate::fact_signature_helpers::install_fact_tracer(
            &crate::fact_signature_helpers::FactTracerBasisSource::from_ctx(ctx),
            || {
                let requested = select_requested_subjects(raw, script);
                let dispatch = ProjectSemanticDispatch::new(ctx);
                let mut rows = Vec::with_capacity(requested.len());
                let mut subjects = Vec::with_capacity(requested.len());
                let mut completeness = TemplateClassFactsCompleteness::Complete;

                for requested_subject in &requested {
                    let subject = join_subject(ctx, canonical, script, requested_subject);
                    let row = match subject {
                        TemplateClassSubject::Binding {
                            ref declaration, ..
                        } => classify_binding(
                            &dispatch,
                            canonical,
                            declaration.clone(),
                            subject.clone(),
                        ),
                        TemplateClassSubject::Prop { ref payload, .. } => {
                            classify_prop(&dispatch, canonical, script, payload, subject.clone())
                        }
                        TemplateClassSubject::Unresolved { .. } => unresolved_row(
                            subject.clone(),
                            ClosedLiteralDomainUnresolvedReason::AnalysisUnavailable,
                            ReactiveWrapperUnresolvedReason::AnalysisUnavailable,
                        ),
                    };
                    if row.wrapper.completeness == TemplateClassFactsCompleteness::ReturnOnly {
                        completeness = TemplateClassFactsCompleteness::ReturnOnly;
                    }
                    subjects.push(subject);
                    rows.push(row);
                }
                (subjects, rows, completeness)
            },
        );

    let dependency_signature = match finalise {
        FactReadSetFinalise::Ok(facts) => ReadSetSignature::new(facts),
        FactReadSetFinalise::NonCacheable(_)
        | FactReadSetFinalise::Overflow
        | FactReadSetFinalise::MutationUnstable => {
            completeness = TemplateClassFactsCompleteness::ReturnOnly;
            ReadSetSignature::overflow()
        }
    };
    // A FENCED input is return-only: content-override bytes, session-overlay
    // bytes, and a known-stale store-view seed all serve their caller and
    // publish nothing. `BasePublishable` leaves the per-row / finalise
    // verdicts above untouched — in particular a dependency-free (zero
    // requested subject) fact set stays `Complete` with an EMPTY signature,
    // which is what `TemplateClassCacheAdmission::not_applicable` already says
    // in the same voice.
    if let TemplateClassPublicationScope::Fenced(_) = scope {
        completeness = TemplateClassFactsCompleteness::ReturnOnly;
    }

    // `ReturnOnly` declines the CLASS-FACT rails and nothing else. It must NOT
    // fan a non-cacheable mark out to the enclosing tracer:
    // `NonCacheableReadReason::PreparationFailure` propagates `Transitive` and
    // marks EVERY active tracer on the thread, so the enclosing compile /
    // component-meta signature would become `NonCacheable` — publishing
    // nothing, removing any prior slot, and skipping both the raw-template
    // persist and the artifact commit — for an ordinary valid SFC whose only
    // unresolved class subject is a `v-for` alias, a slot-scope alias, an
    // options-API binding, a typo, or a merely missing dependency.
    //
    // Nothing is lost: the inner tracer's OBSERVATIONS already fan out to the
    // enclosing signature, so the compile signature still invalidates on every
    // dependency the class facts read. The two narrow rails the design
    // specifies carry the decline instead — `complete_dependency_signature()`
    // returns `None` (declining the raw-template semantic slot) and
    // `owner_only_publication_safe()` returns `false` (declining the
    // pure-content publish).
    TemplateClassSemanticFacts::new(
        Arc::from(canonical),
        resolved_whole_hash,
        Arc::from(subjects.into_boxed_slice()),
        Arc::from(rows.into_boxed_slice()),
        completeness,
        dependency_signature,
    )
}

pub(crate) fn complete_dependency_signature(
    facts: &SessionTemplateClassSemanticFacts,
) -> Option<ReadSetSignature> {
    (facts.completeness() == TemplateClassFactsCompleteness::Complete)
        .then(|| facts.dependency_signature().clone())
}

/// Whether the pure-content publish is safe for these facts.
///
/// The pure-content slot is keyed on `(owner, owner_whole_hash,
/// profile)` and validated by that key ALONE — it carries no fact
/// signature. So the publish is safe exactly when the owner's own
/// content hash is a complete validity oracle for the signature, i.e.
/// when every fact in it is attributable to the owner.
pub(crate) fn owner_only_publication_safe(facts: &SessionTemplateClassSemanticFacts) -> bool {
    use verter_workspace::FactAttribution;
    facts.completeness() == TemplateClassFactsCompleteness::Complete
        && facts
            .dependency_signature()
            .facts
            .iter()
            .all(|fact| match fact.attribution() {
                FactAttribution::Canonical(canonical_id) => canonical_id == facts.owner_canonical(),
                // Both are UNATTRIBUTABLE, and for opposite reasons: a
                // project scalar describes no canonical, a domain
                // aggregate stands in for an unbounded set of them.
                // Either way the owner's content hash cannot be a
                // complete validity oracle, so the publish is declined.
                //
                // Stated as its own arm because through the `Option`
                // projection this was an ACCIDENT — an aggregate answered
                // `None`, `None` failed `is_some_and`, and the right
                // result arrived for a reason nothing recorded. That is
                // one refactor away from becoming `true`, which would
                // publish a cross-file-dependent compile output under a
                // key that only tracks the owner's bytes.
                FactAttribution::ProjectScalar
                | FactAttribution::DomainAggregate(_)
                | FactAttribution::StrictSelfRootWorld => false,
            })
}

#[derive(Debug, Clone)]
pub(crate) struct TemplateClassCacheAdmission {
    pub(crate) signature: Option<ReadSetSignature>,
    pub(crate) owner_only: bool,
}

impl TemplateClassCacheAdmission {
    pub(crate) fn from_facts(facts: &SessionTemplateClassSemanticFacts) -> Self {
        Self {
            signature: complete_dependency_signature(facts),
            owner_only: owner_only_publication_safe(facts),
        }
    }

    pub(crate) fn refused() -> Self {
        Self {
            signature: None,
            owner_only: false,
        }
    }

    pub(crate) fn not_applicable() -> Self {
        Self {
            signature: Some(ReadSetSignature::new(Arc::from([]))),
            owner_only: true,
        }
    }
}

fn select_requested_subjects(
    raw: &RawTemplateData,
    script: TemplateClassScriptInputs<'_>,
) -> Vec<RequestedSubject> {
    let props_root: Option<Arc<str>> =
        verter_semantic::analysis::props_root_binding(script.macros).map(Arc::from);
    let mut result = Vec::new();
    let mut seen = FxHashSet::default();

    let mut push_expr = |expr: &str| {
        let trimmed = expr.trim();
        let subject = if is_simple_identifier(trimmed) {
            Some(RequestedSubject::Binding(Arc::from(trimmed)))
        } else {
            props_root.as_ref().and_then(|root| {
                trimmed
                    .strip_prefix(root.as_ref())
                    .and_then(|rest| rest.strip_prefix('.'))
                    .filter(|member| is_simple_identifier(member))
                    .map(|member| RequestedSubject::Prop {
                        props_root: Arc::clone(root),
                        member: Arc::from(member),
                    })
            })
        };
        if let Some(subject) = subject {
            if seen.insert(subject.clone()) {
                result.push(subject);
            }
        }
    };

    for component in &raw.components {
        if let Some(expr) = component.dynamic_class_expr.as_deref() {
            push_expr(expr);
        }
    }
    for element in &raw.elements {
        for expr in element
            .attributes
            .iter()
            .filter(|attribute| attribute.is_dynamic && attribute.name == "class")
            .filter_map(|attribute| attribute.value.as_deref())
        {
            push_expr(expr);
        }
        for expr in element
            .directives
            .iter()
            .filter(|directive| {
                directive.name == "bind" && directive.argument.as_deref() == Some("class")
            })
            .filter_map(|directive| directive.expression.as_deref())
        {
            push_expr(expr);
        }
    }
    result
}

fn join_subject(
    ctx: &dyn ResolverContext,
    _canonical: &str,
    script: TemplateClassScriptInputs<'_>,
    requested: &RequestedSubject,
) -> TemplateClassSubject {
    match requested {
        RequestedSubject::Binding(label) => {
            let count = script
                .bindings
                .iter()
                .filter(|binding| binding.name == label.as_ref())
                .count();
            if count == 1 {
                let owner = ctx.shallow_file_state(_canonical).and_then(|shallow| {
                    match shallow
                        .visible_value_binding(TopLevelOwnerId::instance(0), label.as_ref())
                    {
                        Some(
                            crate::resolver_core::shallow_file_state::LexicalValueBinding::Local(
                                owner,
                            ),
                        ) => Some(owner),
                        _ => None,
                    }
                });
                if let Some(owner) = owner {
                    return TemplateClassSubject::Binding {
                        label: Arc::clone(label),
                        declaration: DeclBindingKey::new(owner, label.as_ref()),
                    };
                }
            }
            join_prop_field(script, label, None)
        }
        RequestedSubject::Prop { props_root, member } => {
            join_prop_field(script, member, Some(props_root))
        }
    }
}

fn join_prop_field(
    script: TemplateClassScriptInputs<'_>,
    label: &Arc<str>,
    props_root: Option<&Arc<str>>,
) -> TemplateClassSubject {
    let mut fields = script
        .macros
        .iter()
        .filter(|mac| mac.kind == AnalyzedMacroKind::DefineProps)
        .flat_map(|mac| &mac.prop_fields)
        .filter(|field| field.name == label.as_ref());
    let Some(field) = fields.next() else {
        return TemplateClassSubject::Unresolved {
            label: Arc::clone(label),
            props_root: props_root.cloned(),
        };
    };
    if fields.next().is_some() {
        return TemplateClassSubject::Unresolved {
            label: Arc::clone(label),
            props_root: props_root.cloned(),
        };
    }
    match (field.payload.clone(), field.type_expr_scope.clone()) {
        (Some(payload), Some(scope)) => TemplateClassSubject::Prop {
            label: Arc::clone(label),
            props_root: props_root.cloned().unwrap_or_else(|| Arc::<str>::from("")),
            payload,
            scope,
        },
        _ => TemplateClassSubject::Unresolved {
            label: Arc::clone(label),
            props_root: props_root.cloned(),
        },
    }
}

fn classify_binding(
    dispatch: &ProjectSemanticDispatch<'_>,
    canonical: &str,
    declaration: DeclBindingKey,
    subject: TemplateClassSubject,
) -> TemplateClassSemanticFactRow {
    let route_candidate = dispatch
        .ctx
        .prepared_value_decl_return_only(canonical, declaration.owner, declaration.name.as_ref())
        .and_then(|prepared| {
            dispatch
                .resolve_authored_reference_route(
                    canonical,
                    declaration.owner,
                    &prepared.type_annotation.reference_head,
                )
                .ok()
                .flatten()
        })
        .and_then(|route| wrapper_candidate_for_route(dispatch.ctx, route));
    let context =
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);
    let key = dispatch.typeof_key_for(
        ValueRootKey {
            scope: ScopeId::file(Arc::from(canonical), declaration.owner),
            name: Arc::clone(&declaration.name),
        },
        context,
    );
    let read = dispatch.execute_read(key);
    crate::meta_resolve::emit_dispatch_dep_signature_facts(dispatch.ctx, &read.dep_signature);
    crate::request_context::observe_component_meta_read_suppress(&read);
    match read.value {
        QueryResult::Value(node) if !read.result_is_partial => {
            classify_node(dispatch, node, subject, route_candidate.as_ref())
        }
        QueryResult::Recursive(_) => unresolved_row(
            subject,
            ClosedLiteralDomainUnresolvedReason::Cycle,
            ReactiveWrapperUnresolvedReason::Cycle,
        ),
        QueryResult::Error(error) => unresolved_row_from_query(subject, &error),
        QueryResult::Value(_) => unresolved_row(
            subject,
            ClosedLiteralDomainUnresolvedReason::Fault,
            ReactiveWrapperUnresolvedReason::Fault,
        ),
    }
}

fn classify_prop(
    dispatch: &ProjectSemanticDispatch<'_>,
    canonical: &str,
    script: TemplateClassScriptInputs<'_>,
    payload: &verter_type_expr::locators::MacroPayloadLocator,
    subject: TemplateClassSubject,
) -> TemplateClassSemanticFactRow {
    let mirror = crate::structural_carrier_producer::macro_type_arg_hot_ref(
        dispatch.ctx,
        canonical,
        payload.macro_index as usize,
    );
    // The AUTHORED head of this prop member, from whichever producer holds it.
    //
    // A DIRECT object-literal macro type argument (`defineProps<{ x: Ref<…> }>()`)
    // is borrowed whole by the structural macro hot mirror, which mints the
    // per-field head inline. A NAMED or ALIASED type argument
    // (`defineProps<Props>()`) leases a `TypeExpr::Ref`, so the mirror mints
    // nothing and the exact authored evidence lives on the props declaration's
    // prepared MEMBER fact instead.
    //
    // Both are the same KIND of producer-minted graph-free fact, and everything
    // after this point is ONE shared path: route → candidate → terminal demand.
    let authored_head = match (payload.payload, mirror.as_ref()) {
        (MacroPayloadPosition::Field { field_index }, Some(mirror)) => mirror
            .prop_reference_heads
            .get(field_index as usize)
            .and_then(Option::as_ref)
            .cloned(),
        _ => None,
    }
    .or_else(|| {
        named_type_argument_member_head(dispatch, canonical, script, payload, subject.label())
    });
    let route_candidate = authored_head
        .and_then(|head| {
            dispatch
                .resolve_authored_reference_route(canonical, payload.anchor.owner, &head)
                .ok()
                .flatten()
        })
        .and_then(|route| wrapper_candidate_for_route(dispatch.ctx, route));
    let context =
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);
    let source = SemanticTypeSource::Authored(AuthoredBodyLocator::MacroPayload(payload.clone()));
    // TYPED raise boundary: a genuine absence stays `MissingDependency`, but a
    // typed query failure publishes ITS OWN reason through the single
    // disposition authority instead of being erased into "missing dependency".
    let hot = match dispatch.raise_semantic_type_source_to_hot(
        &source,
        SourceRaiseContext {
            scope_canonical_id: canonical,
            scope_owner: payload.anchor.owner,
            context,
            interior_failures: None,
        },
    ) {
        SourceRaiseOutcome::Raised(hot) => hot,
        SourceRaiseOutcome::Absent => {
            return unresolved_row(
                subject,
                ClosedLiteralDomainUnresolvedReason::MissingDependency,
                ReactiveWrapperUnresolvedReason::MissingDependency,
            );
        }
        SourceRaiseOutcome::Failed(err) => return unresolved_row_from_query(subject, &err),
    };
    classify_node(dispatch, hot.node(), subject, route_candidate.as_ref())
}

/// The AUTHORED reference head of one prop member whose macro type argument is a
/// NAMED or ALIASED reference rather than a direct object literal.
///
/// Two producer-minted facts compose it, and neither is resolved here:
///
/// 1. `mac.resolved_local_types` — the analyzer's own record of the LOCAL type
///    declarations this macro's type argument directly names, carrying
///    `{ name, owner }`. Exactly ONE entry is required: a type argument naming
///    several local roots (`defineProps<A & B>()`) has no single declaration to
///    read a member from, and picking one would be a guess.
/// 2. `prepared_type_decl(...).member_index[member].reference_head` — a fact
///    COPY read of the head minted once at that declaration's lazy body
///    lowering. No locator deref, no `Instantiate`, no re-lowering.
///
/// The declaration lookup is deliberately NOT routed through
/// `resolve_authored_reference_route`: that walk returns `Ok(None)` unless an
/// import edge was crossed, so a same-file `Props` has no route to compose. The
/// props declaration is read directly and only the MEMBER's head is
/// route-resolved — ONE route resolution, not two chained ones.
///
/// An IMPORTED props type never reaches here: it yields no analyzer prop field,
/// so the subject joins as `Unresolved` and `classify_prop` is not called. That
/// boundary is a ruled fail-closed negative, pinned by
/// `template_class_imported_props_type_argument_fails_closed_with_local_control`.
fn named_type_argument_member_head(
    dispatch: &ProjectSemanticDispatch<'_>,
    canonical: &str,
    script: TemplateClassScriptInputs<'_>,
    payload: &verter_type_expr::locators::MacroPayloadLocator,
    member: &str,
) -> Option<verter_type_expr::facts::AuthoredReferenceHeadFact> {
    let mac = script.macros.get(payload.macro_index as usize)?;
    let [props_decl] = mac.resolved_local_types.as_slice() else {
        return None;
    };
    let head = dispatch
        .ctx
        .prepared_type_decl(canonical, props_decl.owner, props_decl.name.as_str())
        .ok()
        .flatten()?
        .member(&verter_type_expr::PropertyKey::from(member))?
        .reference_head
        .clone();
    Some(head)
}

fn classify_node(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    subject: TemplateClassSubject,
    candidate: Option<&WrapperCandidate>,
) -> TemplateClassSemanticFactRow {
    // Preserve the authored carrier until exact symbol identity has been
    // demanded. Structural peeling would expand Ref<T> to its object surface
    // and erase the wrapper head before provenance classification.
    //
    // The cheap `candidate` test is FIRST: `carrier_instantiation_args` walks
    // the graph, and with no candidate its answer is discarded.
    if let Some(candidate) = candidate
        .filter(|_| carrier_instantiation_args(dispatch, node, &mut FxHashSet::default()).is_some())
    {
        let expected = [candidate.symbol.clone()];
        match dispatch.demand_terminal_symbol_instantiation(node, &expected) {
            TerminalSymbolInstantiationDemandOutcome::Complete(Some(terminal)) => {
                let symbol = terminal.symbol;
                if candidate.symbol == symbol {
                    let Some(inner) = terminal.args.first().copied() else {
                        return unresolved_row(
                            subject,
                            ClosedLiteralDomainUnresolvedReason::Unsupported,
                            ReactiveWrapperUnresolvedReason::Unsupported,
                        );
                    };
                    let inner_domain = classify_closed_domain(dispatch, inner);
                    let completeness = domain_completeness(&inner_domain);
                    return TemplateClassSemanticFactRow {
                        subject,
                        domain: inner_domain.clone(),
                        wrapper: verter_semantic::analysis::ReactiveWrapperProof {
                            role: candidate.role.clone(),
                            symbol: Some(symbol),
                            import_provenance: Some(candidate.provenance.clone()),
                            inner_source: closed_domain_source(&inner_domain),
                            inner_domain,
                            completeness,
                        },
                    };
                }
            }
            TerminalSymbolInstantiationDemandOutcome::Partial(reason) => {
                let (domain_reason, wrapper_reason) = unresolved_reasons_from_identity(reason);
                return unresolved_row(subject, domain_reason, wrapper_reason);
            }
            TerminalSymbolInstantiationDemandOutcome::Complete(None) => {}
        }
    }

    let domain = classify_closed_domain(dispatch, node);
    let completeness = domain_completeness(&domain);
    TemplateClassSemanticFactRow {
        subject,
        domain,
        wrapper: verter_semantic::analysis::ReactiveWrapperProof {
            role: ReactiveWrapperRole::None,
            symbol: None,
            import_provenance: None,
            inner_source: None,
            inner_domain: ClosedLiteralDomain::NotClosed,
            completeness,
        },
    }
}

fn carrier_instantiation_args(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    visited: &mut FxHashSet<SemanticNodeId>,
) -> Option<Arc<[SemanticNodeId]>> {
    if !visited.insert(node) {
        return None;
    }
    match dispatch.graph().node_data(node)?.as_ref() {
        SemanticNodeData::InstantiationRef { args, .. } => Some(Arc::clone(args)),
        SemanticNodeData::Alias(next) => carrier_instantiation_args(dispatch, *next, visited),
        _ => None,
    }
}

fn classify_closed_domain(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> ClosedLiteralDomain {
    let context =
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);
    let normalized = match dispatch.normalize_node_for_structural_fact_demand(node, context) {
        StructuralFactDemandOutcome::Complete(node) => node,
        StructuralFactDemandOutcome::Partial(reasons) => {
            return ClosedLiteralDomain::Unresolved {
                reason: unresolved_reasons_from_partial(reasons).0,
                exactness: ResolutionExactness::Incomplete,
            };
        }
    };
    classify_normalized_domain(dispatch, normalized, &mut FxHashSet::default(), &mut 4_096)
}

fn classify_normalized_domain(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    visited: &mut FxHashSet<SemanticNodeId>,
    remaining: &mut usize,
) -> ClosedLiteralDomain {
    if *remaining == 0 {
        return ClosedLiteralDomain::Unresolved {
            reason: ClosedLiteralDomainUnresolvedReason::WorkLimitExceeded,
            exactness: ResolutionExactness::Incomplete,
        };
    }
    *remaining -= 1;
    if !visited.insert(node) {
        return ClosedLiteralDomain::Unresolved {
            reason: ClosedLiteralDomainUnresolvedReason::Cycle,
            exactness: ResolutionExactness::Incomplete,
        };
    }
    let Some(data) = dispatch.graph().node_data(node) else {
        return ClosedLiteralDomain::Unresolved {
            reason: ClosedLiteralDomainUnresolvedReason::Fault,
            exactness: ResolutionExactness::Incomplete,
        };
    };
    let result = match data.as_ref() {
        SemanticNodeData::Literal(LiteralValue::String(value)) => {
            ClosedLiteralDomain::Strings(Arc::from([Arc::from(value.as_str())]))
        }
        SemanticNodeData::Union(members) => {
            let mut values = Vec::<Arc<str>>::new();
            let mut seen = FxHashSet::<Arc<str>>::default();
            let mut open = false;
            let mut unresolved = None;
            for member in members.iter().copied() {
                let context = ProjectionReductionContext::structural_transit_with_mode(
                    ProjectionMode::Navigate,
                );
                let normalized =
                    match dispatch.normalize_node_for_structural_fact_demand(member, context) {
                        StructuralFactDemandOutcome::Complete(node) => node,
                        StructuralFactDemandOutcome::Partial(reasons) => {
                            unresolved = Some(ClosedLiteralDomain::Unresolved {
                                reason: unresolved_reasons_from_partial(reasons).0,
                                exactness: ResolutionExactness::Incomplete,
                            });
                            break;
                        }
                    };
                match classify_normalized_domain(dispatch, normalized, visited, remaining) {
                    ClosedLiteralDomain::Strings(member_values) => {
                        for value in member_values.iter().cloned() {
                            if seen.insert(Arc::clone(&value)) {
                                values.push(value);
                            }
                        }
                    }
                    ClosedLiteralDomain::NotClosed => open = true,
                    value @ ClosedLiteralDomain::Unresolved { .. } => {
                        unresolved = Some(value);
                        break;
                    }
                }
            }
            if let Some(unresolved) = unresolved {
                unresolved
            } else if open || values.is_empty() {
                ClosedLiteralDomain::NotClosed
            } else {
                ClosedLiteralDomain::Strings(Arc::from(values.into_boxed_slice()))
            }
        }
        SemanticNodeData::Alias(next) => {
            classify_normalized_domain(dispatch, *next, visited, remaining)
        }
        SemanticNodeData::Opaque(error) => ClosedLiteralDomain::Unresolved {
            reason: classify_query_error(error).domain_reason,
            exactness: ResolutionExactness::Incomplete,
        },
        SemanticNodeData::RawFallback { .. }
        | SemanticNodeData::BareRef(_)
        | SemanticNodeData::ImportType(_)
        | SemanticNodeData::DeclRef { .. }
        | SemanticNodeData::InstantiationRef { .. } => ClosedLiteralDomain::Unresolved {
            reason: ClosedLiteralDomainUnresolvedReason::Unsupported,
            exactness: ResolutionExactness::Incomplete,
        },
        _ => ClosedLiteralDomain::NotClosed,
    };
    visited.remove(&node);
    result
}

/// Publish an unresolved row for a typed [`QueryError`] through the SINGLE
/// disposition authority ([`classify_query_error`]) — never a local
/// re-classification of the error's arms.
fn unresolved_row_from_query(
    subject: TemplateClassSubject,
    error: &QueryError,
) -> TemplateClassSemanticFactRow {
    let class = classify_query_error(error);
    unresolved_row(subject, class.domain_reason, class.wrapper_reason())
}

fn unresolved_row(
    subject: TemplateClassSubject,
    domain_reason: ClosedLiteralDomainUnresolvedReason,
    wrapper_reason: ReactiveWrapperUnresolvedReason,
) -> TemplateClassSemanticFactRow {
    let domain = ClosedLiteralDomain::Unresolved {
        reason: domain_reason,
        exactness: ResolutionExactness::Incomplete,
    };
    TemplateClassSemanticFactRow {
        subject,
        domain: domain.clone(),
        wrapper: verter_semantic::analysis::ReactiveWrapperProof {
            role: ReactiveWrapperRole::Unresolved {
                reason: wrapper_reason,
            },
            symbol: None,
            import_provenance: None,
            inner_source: None,
            inner_domain: domain,
            completeness: TemplateClassFactsCompleteness::ReturnOnly,
        },
    }
}

fn domain_completeness(domain: &ClosedLiteralDomain) -> TemplateClassFactsCompleteness {
    if matches!(domain, ClosedLiteralDomain::Unresolved { .. }) {
        TemplateClassFactsCompleteness::ReturnOnly
    } else {
        TemplateClassFactsCompleteness::Complete
    }
}

fn closed_domain_source(domain: &ClosedLiteralDomain) -> Option<SemanticTypeSource> {
    let ClosedLiteralDomain::Strings(values) = domain else {
        return None;
    };
    let leaves = values
        .iter()
        .map(|value| LeafTypeFact::StringLiteral(value.to_string()))
        .collect::<Vec<_>>();
    let fact = match leaves.as_slice() {
        [leaf] => ClosedTypeFact::Leaf(leaf.clone()),
        _ => ClosedTypeFact::LeafUnion(Arc::from(leaves.into_boxed_slice())),
    };
    Some(SemanticTypeSource::Closed(fact))
}

/// Shared with the sibling [`super::reactive_wrapper`] demand entry so the
/// identity-partial → typed-reason mapping has exactly one owner.
pub(super) fn unresolved_reasons_from_identity(
    reason: PropCallableRoleUnresolvedReason,
) -> (
    ClosedLiteralDomainUnresolvedReason,
    ReactiveWrapperUnresolvedReason,
) {
    match reason {
        PropCallableRoleUnresolvedReason::AnalysisUnavailable => (
            ClosedLiteralDomainUnresolvedReason::AnalysisUnavailable,
            ReactiveWrapperUnresolvedReason::AnalysisUnavailable,
        ),
        PropCallableRoleUnresolvedReason::MissingDependency => (
            ClosedLiteralDomainUnresolvedReason::MissingDependency,
            ReactiveWrapperUnresolvedReason::MissingDependency,
        ),
        PropCallableRoleUnresolvedReason::Cycle => (
            ClosedLiteralDomainUnresolvedReason::Cycle,
            ReactiveWrapperUnresolvedReason::Cycle,
        ),
        PropCallableRoleUnresolvedReason::BudgetExceeded => (
            ClosedLiteralDomainUnresolvedReason::BudgetExceeded,
            ReactiveWrapperUnresolvedReason::BudgetExceeded,
        ),
        PropCallableRoleUnresolvedReason::WorkLimitExceeded => (
            ClosedLiteralDomainUnresolvedReason::WorkLimitExceeded,
            ReactiveWrapperUnresolvedReason::WorkLimitExceeded,
        ),
        PropCallableRoleUnresolvedReason::Unsupported => (
            ClosedLiteralDomainUnresolvedReason::Unsupported,
            ReactiveWrapperUnresolvedReason::Unsupported,
        ),
        PropCallableRoleUnresolvedReason::Fault => (
            ClosedLiteralDomainUnresolvedReason::Fault,
            ReactiveWrapperUnresolvedReason::Fault,
        ),
    }
}

fn unresolved_reasons_from_partial(
    reasons: crate::semantic_query::PartialReasonSet,
) -> (
    ClosedLiteralDomainUnresolvedReason,
    ReactiveWrapperUnresolvedReason,
) {
    if reasons.contains(crate::semantic_query::PartialReasonSet::BUDGET_EXCEEDED) {
        (
            ClosedLiteralDomainUnresolvedReason::BudgetExceeded,
            ReactiveWrapperUnresolvedReason::BudgetExceeded,
        )
    } else if reasons.contains(crate::semantic_query::PartialReasonSet::SAME_PATH_RECURSION) {
        (
            ClosedLiteralDomainUnresolvedReason::Cycle,
            ReactiveWrapperUnresolvedReason::Cycle,
        )
    } else if reasons.contains(crate::semantic_query::PartialReasonSet::PROJECTION_WORK_LIMIT)
        || reasons.contains(crate::semantic_query::PartialReasonSet::CONNECTED_QUERY_DEPTH_LIMIT)
    {
        (
            ClosedLiteralDomainUnresolvedReason::WorkLimitExceeded,
            ReactiveWrapperUnresolvedReason::WorkLimitExceeded,
        )
    } else if reasons.contains(crate::semantic_query::PartialReasonSet::MISSING_DEPENDENCY) {
        (
            ClosedLiteralDomainUnresolvedReason::MissingDependency,
            ReactiveWrapperUnresolvedReason::MissingDependency,
        )
    } else {
        (
            ClosedLiteralDomainUnresolvedReason::Fault,
            ReactiveWrapperUnresolvedReason::Fault,
        )
    }
}

fn is_simple_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || character == '_' || character == '$'
        })
        && !value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Typed admission (rule 16 — no boolean flag decides cacheability), pinned
    /// on the type's own surface.
    ///
    /// The scope composes from exactly two facts: a call site's attestation
    /// about the bytes it hands the builder, and the store-view seed's OWN
    /// currentness proof. `narrowed_by_seed` takes the REAL
    /// [`ColdSeedHostStoreView`] — both states are produced here by the manager
    /// itself, never manufactured — and the mapping is pinned in every
    /// direction:
    ///
    /// * `(store-published, proven-current seed)` → `BasePublishable`
    /// * `(store-published, non-current seed)` → `Fenced(NonCurrentSeed)`
    /// * `(fenced bytes, _)` → the caller's own `Fenced(reason)`, preserved
    ///
    /// The STRUCTURAL half is the absence of any `bool` / `From<bool>` /
    /// cache-presence constructor on [`TemplateClassPublicationScope`]: the only
    /// way to reach `Fenced(NonCurrentSeed)` is to hand `narrowed_by_seed` a
    /// seed that itself reports non-current, so no call site can re-derive the
    /// fence from artifact-cache warmth. That is enforced by the type's own
    /// surface, not by any source scan.
    ///
    /// Finally: `BasePublishable` leaves a zero-subject (dependency-free) fact
    /// set `Complete` with an EMPTY signature, and the fenced scope is the only
    /// thing that turns the SAME fact set `ReturnOnly` with no complete
    /// signature — empty and absent are different states.
    #[test]
    fn template_class_publication_scope_is_typed_and_cache_presence_cannot_fence() {
        use crate::types::{HostConfig, UpsertRequest};
        use crate::VerterHost;

        let host = VerterHost::new_standalone(HostConfig::default());
        let canonical = "/scope/ZeroSubject.vue";
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some(canonical.to_string()),
                input_id: canonical.to_string(),
                source: Arc::from(
                    "<script setup lang=\"ts\">\nconst label = 'plain'\n</script>\n<template><div>{{ label }}</div></template>",
                ),
                file_language: verter_language::LanguageRegistry::global()
                    .classify_static(canonical)
                    .static_resolution(),
                aliases: Vec::new(),
            })
            .expect("upsert must succeed");

        // ── Real seeds, both states, produced by the store-view manager ──
        let current_seed = host.resolver_store_view_read().into_cold_seed_view();
        assert!(
            current_seed.is_current(),
            "fixture invariant: a quiescent host yields a proven-current read",
        );
        // Force every publish to decline WITHOUT advancing a token dimension, so
        // the bounded retry exhausts and the read is a typed `ReturnOnly`.
        host.bump_store_view_epoch();
        crate::resolver_store::HostStoreView::arm_reset_fence_decline_always_for_tests();
        let stale_seed = host.resolver_store_view_read().into_cold_seed_view();
        crate::resolver_store::HostStoreView::disarm_reset_fence_decline_always_for_tests();
        assert!(
            !stale_seed.is_current(),
            "fixture invariant: the armed knob yields a KNOWN non-current seed",
        );

        // ── The mapping, pinned in every direction ──
        assert_eq!(
            TemplateClassPublicationScope::BasePublishable.narrowed_by_seed(&current_seed),
            TemplateClassPublicationScope::BasePublishable,
            "store-published bytes over a proven-current seed stay base-publishable",
        );
        assert_eq!(
            TemplateClassPublicationScope::BasePublishable.narrowed_by_seed(&stale_seed),
            TemplateClassPublicationScope::Fenced(TemplateClassFenceReason::NonCurrentSeed),
            "store-published bytes over a KNOWN-STALE seed are fenced — and this \
             is the ONLY fence the seed can produce",
        );
        {
            let reason = TemplateClassFenceReason::SessionOverlay;
            let fenced = TemplateClassPublicationScope::Fenced(reason);
            assert_eq!(
                fenced.narrowed_by_seed(&current_seed),
                fenced,
                "a fenced attestation keeps its own reason ({reason:?}) — seeding \
                 from a current read never launders fenced bytes",
            );
            assert_eq!(
                fenced.narrowed_by_seed(&stale_seed),
                fenced,
                "a fenced attestation keeps its own reason ({reason:?}) over a \
                 stale seed too",
            );
            assert!(
                !fenced.is_base_publishable(),
                "a fenced scope never reaches the base publication rails",
            );
        }
        assert!(
            TemplateClassPublicationScope::BasePublishable.is_base_publishable(),
            "the base-publishable scope reaches the base publication rails",
        );

        // ── `BasePublishable` leaves a zero-subject fact set Complete + EMPTY ──
        let source_snapshot = host
            .scheduler
            .try_get_source(canonical)
            .expect("source snapshot");
        let data = source_snapshot
            .downcast_data::<crate::host_executor::HostSourceData>()
            .expect("host source data");
        let raw = crate::parse::compile_template_data(
            &data.file_language,
            source_snapshot.source.as_ref(),
            data.framework_parse.as_deref(),
            true,
            &host.provenance,
        )
        .expect("raw template data");
        let script = TemplateClassScriptInputs {
            macros: &data.parse.script_analysis.macros,
            bindings: &data.parse.script_analysis.bindings,
        };

        // Mirror the production lane's binding exactly: a cold-seed
        // request-bound context, never the raw owned-view escape hatch.
        let base_view = host.resolver_store_view_read().into_cold_seed_view();
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let request_ctx =
            crate::resolver_core::HostResolverContext::from_cold_seed(&host, &base_view, overlay);

        let publishable = build_template_class_semantic_facts(
            &request_ctx,
            canonical,
            data.parse.whole_hash,
            script,
            &raw,
            TemplateClassPublicationScope::BasePublishable,
        );
        assert!(
            publishable.requested_subjects().is_empty() && publishable.rows().is_empty(),
            "fixture invariant: the file requests ZERO class subjects",
        );
        assert_eq!(
            publishable.completeness(),
            TemplateClassFactsCompleteness::Complete,
            "a dependency-free fact set over base-publishable bytes is COMPLETE — \
             a cold artifact store is not a fence",
        );
        assert!(
            publishable.dependency_signature().facts.is_empty()
                && !publishable.dependency_signature().overflowed,
            "a dependency-free fact set records an EMPTY PRESENT signature",
        );
        assert!(
            complete_dependency_signature(&publishable).is_some_and(|s| s.facts.is_empty()),
            "the raw-template slot's invalidation rail is PRESENT and empty, so \
             the slot admits",
        );
        assert!(
            owner_only_publication_safe(&publishable),
            "a dependency-free fact set is trivially owner-only",
        );

        // The fenced scope is the ONLY difference that flips the same fact set.
        let fenced = build_template_class_semantic_facts(
            &request_ctx,
            canonical,
            data.parse.whole_hash,
            script,
            &raw,
            TemplateClassPublicationScope::Fenced(TemplateClassFenceReason::NonCurrentSeed),
        );
        assert_eq!(
            fenced.completeness(),
            TemplateClassFactsCompleteness::ReturnOnly,
            "fenced bytes make the SAME fact set return-only",
        );
        assert!(
            complete_dependency_signature(&fenced).is_none(),
            "a return-only fact set has no complete signature, so the raw-template \
             slot declines",
        );
        assert!(
            !owner_only_publication_safe(&fenced),
            "a return-only fact set declines the pure-content publish",
        );
    }

    #[test]
    fn requested_subject_footprint_ignores_unrelated_declarations_and_complex_expressions() {
        let alloc = oxc_allocator::Allocator::new();
        let script = verter_semantic::analysis::build_script_analysis(
            "type Deep = { next: Deep }; const variant: 'a' | 'b' = 'a'; const unused: Deep = null as never; const props = withDefaults(defineProps<{ size: 'sm' | 'lg'; unusedProp: Deep }>(), { size: 'sm' });",
            oxc_span::SourceType::ts(),
            &alloc,
        );
        let component =
            |expression: &str| verter_compiler::compile::template_data::RawComponentUsage {
                tag_name: "X".to_string(),
                is_dynamic: false,
                props: Vec::new(),
                has_spread: false,
                slots_used: Vec::new(),
                static_classes: Vec::new(),
                has_dynamic_class: true,
                dynamic_class_expr: Some(expression.to_string()),
                bindings: Vec::new(),
                events: Vec::new(),
                span: verter_compiler::common::Span::new(0, 1),
            };
        let raw = RawTemplateData {
            components: vec![
                component("variant"),
                component("props.size"),
                component("{ active: unused }"),
                component("variant"),
            ],
            ..RawTemplateData::default()
        };
        let requested = select_requested_subjects(
            &raw,
            TemplateClassScriptInputs {
                macros: &script.macros,
                bindings: &script.bindings,
            },
        );
        assert_eq!(
            requested,
            [
                RequestedSubject::Binding(Arc::from("variant")),
                RequestedSubject::Prop {
                    props_root: Arc::from("props"),
                    member: Arc::from("size"),
                },
            ]
        );
    }
}

#[cfg(test)]
mod attribution_tests {
    use super::*;
    use verter_workspace::{
        AggregatePopulation, AggregateStamp, CompactionDomain, DomainGenerationFact,
        FactVersionRef, ViewPopulation,
    };

    const OWNER: &str = "/p/Owner.vue";

    fn facts_with(signature: ReadSetSignature) -> SessionTemplateClassSemanticFacts {
        SessionTemplateClassSemanticFacts::new(
            std::sync::Arc::from(OWNER),
            [3u8; 16],
            std::sync::Arc::from([]),
            std::sync::Arc::from([]),
            TemplateClassFactsCompleteness::Complete,
            signature,
        )
    }

    fn owner_whole_hash_fact() -> FactVersionRef {
        FactVersionRef::FileWholeHash {
            canonical_id: OWNER.to_string(),
            hash: [3u8; 16],
        }
    }

    fn content_aggregate() -> FactVersionRef {
        FactVersionRef::DomainGeneration(DomainGenerationFact {
            domain: CompactionDomain::Content,
            population: AggregatePopulation::View(ViewPopulation::Base),
            stamp: AggregateStamp::Generation(9),
        })
    }

    #[test]
    fn an_owner_only_signature_admits_the_pure_content_publish() {
        assert!(
            owner_only_publication_safe(&facts_with(ReadSetSignature::new(std::sync::Arc::from(
                [owner_whole_hash_fact()]
            )))),
            "every fact attributes to the owner, so the owner's content hash \
             IS a complete validity oracle for this signature"
        );
    }

    #[test]
    fn a_cross_file_signature_declines_the_pure_content_publish() {
        assert!(
            !owner_only_publication_safe(&facts_with(ReadSetSignature::new(std::sync::Arc::from(
                [
                    owner_whole_hash_fact(),
                    FactVersionRef::FileWholeHash {
                        canonical_id: "/p/Dep.ts".to_string(),
                        hash: [4u8; 16],
                    },
                ]
            )))),
            "the pure-content slot is keyed on the owner's hash alone, so a \
             dependency on another file's content cannot be published there"
        );
    }

    #[test]
    fn a_compacted_signature_declines_the_pure_content_publish() {
        // The aggregate stands in for every `Content` fact the compute read
        // — across an unbounded set of canonicals, none of which it names.
        // The owner's content hash therefore cannot decide whether this
        // entry is still valid, and the publish must decline.
        assert!(
            !owner_only_publication_safe(&facts_with(ReadSetSignature::new(std::sync::Arc::from(
                [owner_whole_hash_fact(), content_aggregate()]
            )))),
            "a terminal `Content` aggregate is UNATTRIBUTABLE: it stands in \
             for facts on files the owner's content hash says nothing about, \
             so the pure-content publish must decline"
        );
        // And an aggregate ALONE is not "no dependencies".
        assert!(
            !owner_only_publication_safe(&facts_with(ReadSetSignature::new(std::sync::Arc::from(
                [content_aggregate()]
            )))),
            "a signature that is nothing BUT an aggregate is maximally \
             cross-file, not dependency-free"
        );
    }

    #[test]
    fn a_project_scalar_signature_declines_the_pure_content_publish() {
        assert!(
            !owner_only_publication_safe(&facts_with(ReadSetSignature::new(std::sync::Arc::from(
                [
                    owner_whole_hash_fact(),
                    FactVersionRef::ProjectGeneration { generation: 5 },
                ]
            )))),
            "a whole-project scalar describes no canonical, so the owner's \
             content hash cannot decide it either"
        );
    }
}
