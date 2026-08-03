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
    PropCallableRoleUnresolvedReason, ReactiveWrapperImportProvenance, ReactiveWrapperRole,
    ReactiveWrapperUnresolvedReason, ResolutionExactness, ResolutionProvenance,
    ResolvedSymbolIdentity, TopLevelOwnerId,
};

use super::evaluate::StructuralFactDemandOutcome;
use super::semantic_source::SourceRaiseContext;
use super::symbol_identity::TerminalSymbolInstantiationDemandOutcome;
use super::ProjectSemanticDispatch;
use crate::fact_signature_helpers::ReadSetSignature;
use crate::resolver_core::{FactReadSetFinalise, ResolverContext};
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RequestedSubject {
    Binding(Arc<str>),
    Prop {
        props_root: Arc<str>,
        member: Arc<str>,
    },
}

#[derive(Debug, Clone)]
struct WrapperCandidate {
    role: ReactiveWrapperRole,
    symbol: ResolvedSymbolIdentity,
    provenance: ReactiveWrapperImportProvenance,
}

/// Build exact facts through the caller's already-selected resolver context.
pub(crate) fn build_template_class_semantic_facts(
    ctx: &dyn ResolverContext,
    canonical: &str,
    whole_hash: verter_semantic::analysis::Hash16,
    script: TemplateClassScriptInputs<'_>,
    raw: &RawTemplateData,
    store_published: bool,
) -> SessionTemplateClassSemanticFacts {
    let host = ctx.host_for_fact_tracer_install();
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
        crate::fact_signature_helpers::install_fact_tracer(host, || {
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
                    } => {
                        classify_binding(&dispatch, canonical, declaration.clone(), subject.clone())
                    }
                    TemplateClassSubject::Prop { ref payload, .. } => {
                        classify_prop(&dispatch, canonical, payload, subject.clone())
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
        });

    let dependency_signature = match finalise {
        FactReadSetFinalise::Ok(facts) => ReadSetSignature::new(facts),
        FactReadSetFinalise::NonCacheable(_) | FactReadSetFinalise::Overflow => {
            completeness = TemplateClassFactsCompleteness::ReturnOnly;
            ReadSetSignature::overflow()
        }
    };
    if !store_published {
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

pub(crate) fn owner_only_publication_safe(facts: &SessionTemplateClassSemanticFacts) -> bool {
    facts.completeness() == TemplateClassFactsCompleteness::Complete
        && facts.dependency_signature().facts.iter().all(|fact| {
            fact.canonical_id()
                .is_some_and(|id| id == facts.owner_canonical())
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
        .prepared_value_decl(canonical, declaration.owner, declaration.name.as_ref())
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
    payload: &verter_type_expr::locators::MacroPayloadLocator,
    subject: TemplateClassSubject,
) -> TemplateClassSemanticFactRow {
    let mirror = crate::structural_carrier_producer::macro_type_arg_hot_ref(
        dispatch.ctx,
        canonical,
        payload.macro_index as usize,
    );
    let route_candidate = match (payload.payload, mirror.as_ref()) {
        (MacroPayloadPosition::Field { field_index }, Some(mirror)) => mirror
            .prop_reference_heads
            .get(field_index as usize)
            .and_then(Option::as_ref)
            .and_then(|head| {
                dispatch
                    .resolve_authored_reference_route(canonical, payload.anchor.owner, head)
                    .ok()
                    .flatten()
            })
            .and_then(|route| wrapper_candidate_for_route(dispatch.ctx, route)),
        _ => None,
    };
    let context =
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);
    let source = SemanticTypeSource::Authored(AuthoredBodyLocator::MacroPayload(payload.clone()));
    let Some(hot) = dispatch.raise_semantic_type_source_to_hot(
        &source,
        SourceRaiseContext {
            scope_canonical_id: canonical,
            scope_owner: payload.anchor.owner,
            context,
            interior_failures: None,
        },
    ) else {
        return unresolved_row(
            subject,
            ClosedLiteralDomainUnresolvedReason::MissingDependency,
            ReactiveWrapperUnresolvedReason::MissingDependency,
        );
    };
    classify_node(dispatch, hot.node(), subject, route_candidate.as_ref())
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
            reason: domain_reason_from_query(error),
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

fn wrapper_candidate_for_route(
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

fn wrapper_role_for_vue_export(name: &str) -> Option<ReactiveWrapperRole> {
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

fn unresolved_row_from_query(
    subject: TemplateClassSubject,
    error: &QueryError,
) -> TemplateClassSemanticFactRow {
    let domain = domain_reason_from_query(error);
    let wrapper = wrapper_reason_from_query(error);
    unresolved_row(subject, domain, wrapper)
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

fn domain_reason_from_query(error: &QueryError) -> ClosedLiteralDomainUnresolvedReason {
    match error {
        QueryError::Miss | QueryError::DeclPlaceholder { .. } | QueryError::RaiseMiss => {
            ClosedLiteralDomainUnresolvedReason::MissingDependency
        }
        QueryError::BudgetExceeded(_) => ClosedLiteralDomainUnresolvedReason::BudgetExceeded,
        QueryError::AliasCycle { .. }
        | QueryError::RecursiveRef { .. }
        | QueryError::RaiseAliasCycle
        | QueryError::TypeParamCycle => ClosedLiteralDomainUnresolvedReason::Cycle,
        QueryError::Cancelled => ClosedLiteralDomainUnresolvedReason::Cancelled,
        QueryError::UnsupportedIntrinsic { .. }
        | QueryError::UnrepresentableSurface
        | QueryError::UnrepresentableSurfaceMember => {
            ClosedLiteralDomainUnresolvedReason::Unsupported
        }
        QueryError::UnstableState { .. }
        | QueryError::Other(_)
        | QueryError::ValueDomainMismatch { .. } => ClosedLiteralDomainUnresolvedReason::Fault,
    }
}

fn wrapper_reason_from_query(error: &QueryError) -> ReactiveWrapperUnresolvedReason {
    match domain_reason_from_query(error) {
        ClosedLiteralDomainUnresolvedReason::AnalysisUnavailable => {
            ReactiveWrapperUnresolvedReason::AnalysisUnavailable
        }
        ClosedLiteralDomainUnresolvedReason::RevisionMismatch => {
            ReactiveWrapperUnresolvedReason::RevisionMismatch
        }
        ClosedLiteralDomainUnresolvedReason::MissingDependency => {
            ReactiveWrapperUnresolvedReason::MissingDependency
        }
        ClosedLiteralDomainUnresolvedReason::Cycle => ReactiveWrapperUnresolvedReason::Cycle,
        ClosedLiteralDomainUnresolvedReason::BudgetExceeded => {
            ReactiveWrapperUnresolvedReason::BudgetExceeded
        }
        ClosedLiteralDomainUnresolvedReason::WorkLimitExceeded => {
            ReactiveWrapperUnresolvedReason::WorkLimitExceeded
        }
        ClosedLiteralDomainUnresolvedReason::Cancelled => {
            ReactiveWrapperUnresolvedReason::Cancelled
        }
        ClosedLiteralDomainUnresolvedReason::Unsupported => {
            ReactiveWrapperUnresolvedReason::Unsupported
        }
        ClosedLiteralDomainUnresolvedReason::Fault => ReactiveWrapperUnresolvedReason::Fault,
    }
}

fn unresolved_reasons_from_identity(
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
