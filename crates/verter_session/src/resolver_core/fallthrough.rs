use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::component_meta::{
    AcceptedEventAnalysis, AcceptedEventKind, AcceptedPropAnalysis, AcceptedPropKind,
    AcceptedSurfaceCompleteness, BranchStatus, ComponentMetaAnalysis, ConsumedRootBindings,
    FallthroughBranch, FallthroughEventEntry, FallthroughPropEntry, FallthroughSurface,
    InheritedSource, MemberAvailability, MemberProvenance, PartialBranchReason, ResolvedRootStep,
    RootReachability, RootTargetRef, UnresolvedBranchReason,
};
use verter_semantic::analysis::html_intrinsics::{
    html_intrinsic_catalog, IntrinsicMemberKind, IntrinsicTypeShape,
};
use verter_semantic::analysis::types::AnalyzedImport;
use verter_type_expr::facts::{ClosedTypeFact, LeafTypeFact, SemanticTypeSource, SourcePosition};
use verter_type_expr::intrinsics::StaticIntrinsicTypeId;
use verter_type_expr::TypeExpr;

use crate::resolver_core::{FactVersionRef, FallthroughNodeKey, FallthroughOverrideIdentity};

/// One intrinsic member on a native-root fallthrough surface: member identity
/// (`name`, `kind`) plus its type carried as [`IntrinsicMemberTypeSource`].
/// Both source arms are fact/id carriers (witnessed `NoTypeExpr`), so the
/// member is safe to retain in the fallthrough node cache; consumers recover
/// the type on demand (catalog shape lookup / dispatch-bridge raise).
#[derive(Debug, Clone, PartialEq, verter_no_typeexpr::NoTypeExpr)]
pub struct IntrinsicSurfaceMember {
    /// Attribute name for an attr; canonical event name for a listener.
    pub name: String,
    pub kind: IntrinsicMemberKind,
    pub source: IntrinsicMemberTypeSource,
}

/// The type slot of an [`IntrinsicSurfaceMember`].
#[derive(Debug, Clone, PartialEq, verter_no_typeexpr::NoTypeExpr)]
pub enum IntrinsicMemberTypeSource {
    /// A generated static-catalog member: the content-free
    /// [`StaticIntrinsicTypeId`]. The type SHAPE stays table-resident and is
    /// recovered lazily via
    /// [`html_intrinsic_catalog`]`().shape(id)` at the consuming boundary.
    Static(StaticIntrinsicTypeId),
    /// A project-resolved member (the project's own `JSX.IntrinsicElements` /
    /// `HTMLAttributes` surfaces): the resolved semantic SOURCE, raised to a
    /// graph handle on demand through the shared dispatch bridge.
    Resolved(SemanticTypeSource),
}

impl IntrinsicMemberTypeSource {
    /// Project this member type into the two fallthrough-entry channels: the
    /// semantic SOURCE channel (`FallthroughPropEntry::type_source` /
    /// `FallthroughEventEntry::payload`) and the DISPLAY channel (`raw_type` /
    /// `raw_signature`).
    ///
    /// A resolved member IS its source. A static member recovers its catalog
    /// shape on demand: a primitive shape is the closed leaf FACT (raised
    /// through the shared dispatch bridge when a consumer needs a node); a
    /// non-primitive shape is catalog DISPLAY text — published on the display
    /// channel only, a PROVEN unannotated semantic-source absence (the
    /// catalog carries no semantic source for this shape class), never
    /// fabricated into a semantic fact. An out-of-range id carries no type
    /// on either channel.
    #[must_use]
    pub fn type_channels(&self) -> (SourcePosition, Option<String>) {
        match self {
            Self::Resolved(source) => (SourcePosition::Present(source.clone()), None),
            Self::Static(id) => match html_intrinsic_catalog().shape(*id) {
                Some(IntrinsicTypeShape::Primitive(name)) => (
                    SourcePosition::Present(SemanticTypeSource::Closed(ClosedTypeFact::Leaf(
                        LeafTypeFact::Primitive(*name),
                    ))),
                    None,
                ),
                Some(IntrinsicTypeShape::AttrDisplay(text))
                | Some(IntrinsicTypeShape::ListenerFunction(text)) => {
                    (SourcePosition::unannotated(), Some(text.clone()))
                }
                None => (SourcePosition::unannotated(), None),
            },
        }
    }
}

pub trait FallthroughResolutionView {
    fn accepted_props(&self) -> &[AcceptedPropAnalysis];
    fn accepted_events(&self) -> &[AcceptedEventAnalysis];
    fn fallthrough_surface(&self) -> &FallthroughSurface;
    fn fact_versions(&self) -> &[FactVersionRef];
}

pub trait FallthroughResolverHost {
    type ChildResolution: FallthroughResolutionView;

    fn intrinsic_members_for_tag(
        &self,
        canonical_id: &str,
        tag: &str,
    ) -> Vec<IntrinsicSurfaceMember>;
    fn resolve_child_component_canonical(
        &self,
        parent_canonical: &str,
        component_name: &str,
        import_source: &str,
        imported_name: Option<&str>,
        binding_kind: Option<crate::resolver_core::symbol_resolver::ImportBindingKind>,
    ) -> Option<String>;
    fn current_dependency_fact_versions(&self, canonical_id: &str) -> Vec<FactVersionRef>;
    fn resolve_child_fallthrough(
        &self,
        canonical_id: &str,
        prop_type_overrides: Option<&FallthroughPropOverrideSet>,
        visiting: &mut FxHashSet<String>,
    ) -> Option<Self::ChildResolution>;
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedConsumedBindings {
    pub bindings: ConsumedRootBindings,
    pub partial_reasons: Vec<PartialBranchReason>,
}

#[derive(Debug, Clone)]
pub struct ResolvedFallthroughSurface {
    pub accepted_props: Vec<AcceptedPropAnalysis>,
    pub accepted_events: Vec<AcceptedEventAnalysis>,
    pub accepted_surface_completeness: AcceptedSurfaceCompleteness,
    pub fallthrough_surface: FallthroughSurface,
    pub fact_versions: Vec<FactVersionRef>,
}

pub trait FallthroughComputeHost: FallthroughResolverHost {
    type Snapshot;
    type EvalEnv;

    #[allow(clippy::too_many_arguments)]
    fn resolve_root_consumption(
        &self,
        canonical_id: &str,
        branch_key: &str,
        snapshot: &Self::Snapshot,
        element_index: u32,
        base: &ConsumedRootBindings,
        has_unknown_spread: bool,
        eval_env: &mut Option<Self::EvalEnv>,
        overrides: Option<&FallthroughPropOverrideSet>,
    ) -> ResolvedConsumedBindings;

    fn build_generic_child_prop_overrides(
        &self,
        canonical_id: &str,
        snapshot: &Self::Snapshot,
        usage_index: u32,
        eval_env: &mut Option<Self::EvalEnv>,
        overrides: Option<&FallthroughPropOverrideSet>,
    ) -> Option<FallthroughPropOverrideSet>;

    fn resolve_dynamic_root_candidates(
        &self,
        canonical_id: &str,
        snapshot: &Self::Snapshot,
        usage_index: u32,
        eval_env: &mut Option<Self::EvalEnv>,
        overrides: Option<&FallthroughPropOverrideSet>,
    ) -> Vec<DynamicRootCandidate>;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DynamicRootCandidate {
    NativeTag {
        tag: String,
    },
    ComponentImport {
        component_name: String,
        import_source: String,
        /// Original exported/imported name, when it differs from the local
        /// component binding name.
        imported_name: Option<String>,
        /// Import binding kind — preserved for value-space routing.
        /// Some hosts derive it later from the owning file's import snapshot.
        binding_kind: Option<crate::resolver_core::symbol_resolver::ImportBindingKind>,
    },
}

impl DynamicRootCandidate {
    /// Canonical total ordering for dynamic-root candidates: native tags
    /// before component imports; native tags by `tag`; component imports by
    /// `(component_name, import_source)`. Shared by the syntactic-combine site
    /// in `host_manage::fallthrough` and the node-walker dedup-set emit so the
    /// observable output order is identical regardless of which producer ran.
    #[must_use]
    pub(crate) fn ordering(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (Self::NativeTag { tag: left }, Self::NativeTag { tag: right }) => left.cmp(right),
            (Self::NativeTag { .. }, Self::ComponentImport { .. }) => std::cmp::Ordering::Less,
            (Self::ComponentImport { .. }, Self::NativeTag { .. }) => std::cmp::Ordering::Greater,
            (
                Self::ComponentImport {
                    component_name: left_name,
                    import_source: left_source,
                    ..
                },
                Self::ComponentImport {
                    component_name: right_name,
                    import_source: right_source,
                    ..
                },
            ) => (left_name, left_source).cmp(&(right_name, right_source)),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnownSpreadKeys {
    pub attrs: std::collections::BTreeSet<String>,
    pub listeners: std::collections::BTreeSet<String>,
    pub exact: bool,
}

/// A single child prop-type override carried in NODE domain: the prop name
/// plus the interned `SemanticNodeId` of the parent-propagated value type. The
/// node is consumed by the child evaluator directly (never materialised back to
/// a `TypeExpr` and re-injected into the child `EvalEnv`).
#[derive(Debug, Clone)]
pub struct FallthroughPropOverride {
    pub name: String,
    pub node: crate::semantic_query::SemanticNodeId,
}

/// Node-backed child prop-type override set threaded through fallthrough
/// recursion in place of the materialised `FxHashMap<String, TypeExpr>` map.
/// Each entry binds a prop name to its override value NODE. Cacheability is
/// derived from whether the set is empty: a non-empty override set is wholesale
/// uncacheable (see [`FallthroughOverrideIdentity::for_overrides`]), so the set
/// carries only its runtime `entries` — no precomputed identity.
#[derive(Debug, Clone, Default)]
pub struct FallthroughPropOverrideSet {
    pub entries: Vec<FallthroughPropOverride>,
}

impl FallthroughPropOverrideSet {
    /// Look up the override value node for `name`, if present.
    #[must_use]
    pub fn lookup(&self, name: &str) -> Option<crate::semantic_query::SemanticNodeId> {
        self.entries
            .iter()
            .find(|entry| entry.name == name)
            .map(|entry| entry.node)
    }

    /// `true` when the set carries no overrides.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

pub fn extend_unique_fact_versions<I>(fact_versions: &mut Vec<FactVersionRef>, new_facts: I)
where
    I: IntoIterator<Item = FactVersionRef>,
{
    let mut seen: FxHashSet<FactVersionRef> = fact_versions.iter().cloned().collect();
    for fact in new_facts {
        if seen.insert(fact.clone()) {
            fact_versions.push(fact);
        }
    }
}

pub fn fallthrough_cache_key(
    canonical_id: &str,
    generic_root_propagation: bool,
    prop_type_overrides: Option<&FallthroughPropOverrideSet>,
) -> FallthroughNodeKey {
    FallthroughNodeKey::BranchUnionMerge {
        canonical: canonical_id.to_string(),
        overrides: FallthroughOverrideIdentity::for_overrides(prop_type_overrides),
        generic_root_propagation,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn append_native_candidate_branch<H: FallthroughResolverHost>(
    host: &H,
    canonical_id: &str,
    tag: &str,
    branch_key: String,
    condition_text: Option<String>,
    consumed_attrs: &[String],
    consumed_listeners: &[String],
    parent_partial_reasons: &[PartialBranchReason],
    declared_prop_names: &FxHashSet<String>,
    declared_event_names: &FxHashSet<String>,
    declared_listener_aliases: &FxHashSet<String>,
    fallthrough_branches: &mut Vec<FallthroughBranch>,
    any_partial: &mut bool,
) {
    let intrinsic_members = host.intrinsic_members_for_tag(canonical_id, tag);

    let mut inherited_props = Vec::new();
    let mut inherited_events = Vec::new();

    for member in &intrinsic_members {
        match member.kind {
            IntrinsicMemberKind::Attr => {
                if declared_prop_names.contains(member.name.as_str()) {
                    continue;
                }
                if consumed_attrs.iter().any(|attr| attr == &member.name) {
                    continue;
                }
                let (type_source, raw_type) = member.source.type_channels();
                inherited_props.push(FallthroughPropEntry {
                    name: member.name.clone(),
                    type_source,
                    // Intrinsic sources carry lib-global / closed facts —
                    // no producing FILE scope (the owner scope applies).
                    type_source_scope: None,
                    raw_type,
                    sources: vec![InheritedSource::NativeTag {
                        tag: tag.to_string(),
                    }],
                });
            }
            IntrinsicMemberKind::Listener => {
                if declared_event_names.contains(member.name.as_str())
                    || declared_listener_aliases.contains(member.name.as_str())
                {
                    continue;
                }
                if consumed_listeners
                    .iter()
                    .any(|listener| listener == &member.name)
                {
                    continue;
                }
                let (payload, raw_signature) = member.source.type_channels();
                inherited_events.push(FallthroughEventEntry {
                    name: member.name.clone(),
                    payload,
                    // Intrinsic sources carry lib-global / closed facts —
                    // no producing FILE scope (the owner scope applies).
                    payload_scope: None,
                    raw_signature,
                    sources: vec![InheritedSource::NativeTag {
                        tag: tag.to_string(),
                    }],
                });
            }
        }
    }

    inherited_props.sort_by(|left, right| left.name.cmp(&right.name));
    inherited_events.sort_by(|left, right| left.name.cmp(&right.name));

    let status = if parent_partial_reasons.is_empty() {
        BranchStatus::Resolved
    } else {
        *any_partial = true;
        BranchStatus::PartiallyUnresolved {
            reasons: parent_partial_reasons.to_vec(),
        }
    };

    fallthrough_branches.push(FallthroughBranch {
        branch_key,
        condition_text,
        props: inherited_props,
        events: inherited_events,
        root_chain: vec![ResolvedRootStep::NativeTag {
            tag: tag.to_string(),
        }],
        status,
    });
}

#[allow(clippy::too_many_arguments)]
pub fn append_component_candidate_branches<H: FallthroughResolverHost>(
    host: &H,
    parent_canonical_id: &str,
    component_name: &str,
    import_source: &str,
    imported_name: Option<&str>,
    binding_kind: Option<crate::resolver_core::symbol_resolver::ImportBindingKind>,
    branch_key: String,
    condition_text: Option<String>,
    consumed_attrs: &[String],
    consumed_listeners: &[String],
    parent_partial_reasons: &[PartialBranchReason],
    child_prop_overrides: Option<&FallthroughPropOverrideSet>,
    declared_prop_names: &FxHashSet<String>,
    declared_event_names: &FxHashSet<String>,
    declared_listener_aliases: &FxHashSet<String>,
    fallthrough_branches: &mut Vec<FallthroughBranch>,
    any_partial: &mut bool,
    any_unresolved: &mut bool,
    fact_versions: &mut Vec<FactVersionRef>,
    visiting: &mut FxHashSet<String>,
) {
    let Some(child_id) = host.resolve_child_component_canonical(
        parent_canonical_id,
        component_name,
        import_source,
        imported_name,
        binding_kind,
    ) else {
        *any_unresolved = true;
        fallthrough_branches.push(unresolved_child_import_branch(
            branch_key,
            condition_text,
            component_name,
            Some(import_source.to_string()),
        ));
        return;
    };

    extend_unique_fact_versions(
        fact_versions,
        host.current_dependency_fact_versions(&child_id),
    );

    if !verter_language::LanguageRegistry::global()
        .classify_static(&child_id)
        .static_resolution()
        .is_framework_carrier()
    {
        *any_unresolved = true;
        fallthrough_branches.push(unresolved_child_resolution_branch(
            branch_key,
            condition_text,
            component_name,
            child_id,
        ));
        return;
    }

    let Some(child_resolution) =
        host.resolve_child_fallthrough(&child_id, child_prop_overrides, visiting)
    else {
        *any_unresolved = true;
        fallthrough_branches.push(unresolved_child_resolution_branch(
            branch_key,
            condition_text,
            component_name,
            child_id,
        ));
        return;
    };

    extend_unique_fact_versions(
        fact_versions,
        child_resolution.fact_versions().iter().cloned(),
    );

    match child_resolution.fallthrough_surface() {
        FallthroughSurface::None { .. } => {
            let inherited_props = inherited_component_props(
                child_resolution.accepted_props(),
                declared_prop_names,
                consumed_attrs,
                &child_id,
            );
            let inherited_events = inherited_component_events(
                child_resolution.accepted_events(),
                declared_event_names,
                declared_listener_aliases,
                consumed_listeners,
                &child_id,
            );

            let status = if parent_partial_reasons.is_empty() {
                BranchStatus::Resolved
            } else {
                *any_partial = true;
                BranchStatus::PartiallyUnresolved {
                    reasons: parent_partial_reasons.to_vec(),
                }
            };

            fallthrough_branches.push(FallthroughBranch {
                branch_key,
                condition_text,
                props: inherited_props,
                events: inherited_events,
                root_chain: vec![ResolvedRootStep::Component {
                    canonical_id: child_id,
                    component_name: component_name.to_string(),
                }],
                status,
            });
        }
        FallthroughSurface::Branches {
            branches: child_branches,
        } => {
            let child_declared_props: Vec<_> = child_resolution
                .accepted_props()
                .iter()
                .filter(|prop| matches!(prop.provenance, MemberProvenance::Declared))
                .collect();
            let child_declared_events: Vec<_> = child_resolution
                .accepted_events()
                .iter()
                .filter(|event| matches!(event.provenance, MemberProvenance::Declared))
                .collect();

            for child_branch in child_branches {
                let composed_key = format!("{}.{}", branch_key, child_branch.branch_key);

                let mut inherited_props = inherited_declared_component_props(
                    &child_declared_props,
                    declared_prop_names,
                    consumed_attrs,
                    &child_id,
                );
                for prop in &child_branch.props {
                    if declared_prop_names.contains(&prop.name) {
                        continue;
                    }
                    if consumed_attrs.iter().any(|attr| attr == &prop.name) {
                        continue;
                    }
                    inherited_props.push(prop.clone());
                }

                let mut inherited_events = inherited_declared_component_events(
                    &child_declared_events,
                    declared_event_names,
                    declared_listener_aliases,
                    consumed_listeners,
                    &child_id,
                );
                for event in &child_branch.events {
                    if declared_event_names.contains(&event.name)
                        || declared_listener_aliases.contains(&event.name)
                    {
                        continue;
                    }
                    if consumed_listeners
                        .iter()
                        .any(|listener| listener == &event.name)
                    {
                        continue;
                    }
                    inherited_events.push(event.clone());
                }

                inherited_props.sort_by(|left, right| left.name.cmp(&right.name));
                inherited_events.sort_by(|left, right| left.name.cmp(&right.name));

                let mut root_chain = vec![ResolvedRootStep::Component {
                    canonical_id: child_id.clone(),
                    component_name: component_name.to_string(),
                }];
                root_chain.extend(child_branch.root_chain.clone());

                let status = match &child_branch.status {
                    BranchStatus::Resolved => {
                        if parent_partial_reasons.is_empty() {
                            BranchStatus::Resolved
                        } else {
                            *any_partial = true;
                            BranchStatus::PartiallyUnresolved {
                                reasons: parent_partial_reasons.to_vec(),
                            }
                        }
                    }
                    BranchStatus::PartiallyUnresolved { reasons } => {
                        *any_partial = true;
                        let mut combined = reasons.clone();
                        combined.extend(parent_partial_reasons.iter().cloned());
                        combined.sort();
                        combined.dedup();
                        BranchStatus::PartiallyUnresolved { reasons: combined }
                    }
                    BranchStatus::Unresolved { reason } => {
                        if !parent_partial_reasons.is_empty() {
                            *any_partial = true;
                        }
                        *any_unresolved = true;
                        BranchStatus::Unresolved {
                            reason: reason.clone(),
                        }
                    }
                };

                fallthrough_branches.push(FallthroughBranch {
                    branch_key: composed_key,
                    condition_text: condition_text.clone(),
                    props: inherited_props,
                    events: inherited_events,
                    root_chain,
                    status,
                });
            }
        }
    }
}

pub fn merge_fallthrough_branches(
    accepted_props: &mut Vec<AcceptedPropAnalysis>,
    accepted_events: &mut Vec<AcceptedEventAnalysis>,
    fallthrough_branches: &[FallthroughBranch],
    any_partial: bool,
    any_unresolved: bool,
) -> AcceptedSurfaceCompleteness {
    let total_branches = fallthrough_branches.len();
    let force_conditional = any_partial || any_unresolved;

    let mut inherited_prop_map: FxHashMap<
        String,
        (AcceptedPropAnalysis, Vec<String>, MergedSourceState),
    > = FxHashMap::default();
    let mut inherited_event_map: FxHashMap<
        String,
        (AcceptedEventAnalysis, Vec<String>, MergedSourceState),
    > = FxHashMap::default();

    for branch in fallthrough_branches {
        if matches!(branch.status, BranchStatus::Unresolved { .. }) {
            continue;
        }

        for prop in &branch.props {
            let entry = inherited_prop_map
                .entry(prop.name.clone())
                .or_insert_with(|| {
                    (
                        AcceptedPropAnalysis {
                            name: prop.name.clone(),
                            // Finalized from the absorbing accumulator
                            // AFTER every branch folded — never per-branch.
                            type_source: SourcePosition::unannotated(),
                            type_source_scope: None,
                            raw_type: prop.raw_type.clone(),
                            // Inherited fallthrough props lose their
                            // origin source-annotation typed companion;
                            // they only carry the resolved type source.
                            raw_type_source: None,
                            required: false,
                            provenance: MemberProvenance::Inherited {
                                sources: prop.sources.clone(),
                            },
                            availability: MemberAvailability::Always,
                            kind: AcceptedPropKind::Attr,
                        },
                        Vec::new(),
                        MergedSourceState::Unset,
                    )
                });
            entry
                .2
                .fold(&prop.type_source, prop.type_source_scope.as_deref());
            if entry.0.raw_type != prop.raw_type {
                entry.0.raw_type = None;
            }
            if let MemberProvenance::Inherited { sources } = &mut entry.0.provenance {
                merge_inherited_sources(sources, &prop.sources);
            }
            entry.1.push(branch.branch_key.clone());
        }

        for event in &branch.events {
            let entry = inherited_event_map
                .entry(event.name.clone())
                .or_insert_with(|| {
                    (
                        AcceptedEventAnalysis {
                            name: event.name.clone(),
                            // Finalized from the absorbing accumulator
                            // AFTER every branch folded — never per-branch.
                            payload: SourcePosition::unannotated(),
                            payload_scope: None,
                            raw_signature: event.raw_signature.clone(),
                            provenance: MemberProvenance::Inherited {
                                sources: event.sources.clone(),
                            },
                            availability: MemberAvailability::Always,
                            kind: AcceptedEventKind::Listener,
                        },
                        Vec::new(),
                        MergedSourceState::Unset,
                    )
                });
            entry.2.fold(&event.payload, event.payload_scope.as_deref());
            if entry.0.raw_signature != event.raw_signature {
                entry.0.raw_signature = None;
            }
            if let MemberProvenance::Inherited { sources } = &mut entry.0.provenance {
                merge_inherited_sources(sources, &event.sources);
            }
            entry.1.push(branch.branch_key.clone());
        }
    }

    for (_, (prop, branch_keys, _)) in inherited_prop_map.iter_mut() {
        branch_keys.sort();
        branch_keys.dedup();
        if force_conditional || branch_keys.len() < total_branches {
            prop.availability = MemberAvailability::Conditional {
                branch_keys: branch_keys.clone(),
            };
        }
    }
    for (_, (event, branch_keys, _)) in inherited_event_map.iter_mut() {
        branch_keys.sort();
        branch_keys.dedup();
        if force_conditional || branch_keys.len() < total_branches {
            event.availability = MemberAvailability::Conditional {
                branch_keys: branch_keys.clone(),
            };
        }
    }

    let mut inherited_props: Vec<AcceptedPropAnalysis> = inherited_prop_map
        .into_values()
        .map(|(mut prop, _, source_state)| {
            let (source, scope) = source_state.finalize();
            prop.type_source = source;
            prop.type_source_scope = scope;
            prop
        })
        .collect();
    inherited_props.sort_by(|a, b| a.name.cmp(&b.name));
    accepted_props.extend(inherited_props);

    let mut inherited_events: Vec<AcceptedEventAnalysis> = inherited_event_map
        .into_values()
        .map(|(mut event, _, source_state)| {
            let (source, scope) = source_state.finalize();
            event.payload = source;
            event.payload_scope = scope;
            event
        })
        .collect();
    inherited_events.sort_by(|a, b| a.name.cmp(&b.name));
    accepted_events.extend(inherited_events);

    if any_partial || any_unresolved {
        AcceptedSurfaceCompleteness::LowerBound
    } else {
        AcceptedSurfaceCompleteness::Exact
    }
}

#[allow(clippy::too_many_arguments)]
pub fn resolve_fallthrough_surface<H: FallthroughComputeHost>(
    host: &H,
    canonical_id: &str,
    snapshot: &H::Snapshot,
    base_meta: &ComponentMetaAnalysis,
    prop_type_overrides: Option<&FallthroughPropOverrideSet>,
    mut eval_env: Option<H::EvalEnv>,
    mut fact_versions: Vec<FactVersionRef>,
    visiting: &mut FxHashSet<String>,
) -> ResolvedFallthroughSurface {
    let declared_prop_names: FxHashSet<String> = base_meta
        .props
        .iter()
        .map(|prop| prop.name.clone())
        .collect();
    let declared_event_names: FxHashSet<String> = base_meta
        .events
        .iter()
        .map(|event| event.name.clone())
        .collect();
    let declared_listener_aliases: FxHashSet<String> = base_meta
        .props
        .iter()
        .filter_map(|prop| {
            verter_semantic::analysis::html_intrinsics::on_prop_to_event_name(&prop.name)
        })
        .collect();

    let mut accepted_props: Vec<AcceptedPropAnalysis> = base_meta
        .props
        .iter()
        .map(|prop| AcceptedPropAnalysis {
            name: prop.name.clone(),
            type_source: prop.type_source.clone(),
            // Own declared rows: the source is spelled in the owner's own
            // file — the owner scope applies.
            type_source_scope: None,
            raw_type: prop.raw_type.clone(),
            raw_type_source: prop.raw_type_source.clone(),
            required: prop.required,
            provenance: MemberProvenance::Declared,
            availability: MemberAvailability::Always,
            kind: AcceptedPropKind::DeclaredProp,
        })
        .collect();

    let mut accepted_events: Vec<AcceptedEventAnalysis> = base_meta
        .events
        .iter()
        .map(|event| AcceptedEventAnalysis {
            name: event.name.clone(),
            payload: event.payload.clone(),
            // Own declared rows: the owner scope applies.
            payload_scope: None,
            raw_signature: event.raw_signature.clone(),
            provenance: MemberProvenance::Declared,
            availability: MemberAvailability::Always,
            kind: AcceptedEventKind::DeclaredEmit,
        })
        .collect();

    match &base_meta.root_reachability {
        RootReachability::NoFallthrough { reason } => ResolvedFallthroughSurface {
            accepted_props,
            accepted_events,
            accepted_surface_completeness: AcceptedSurfaceCompleteness::Exact,
            fallthrough_surface: FallthroughSurface::None {
                reason: reason.clone(),
            },
            fact_versions,
        },
        RootReachability::Branches { branches } => {
            let mut fallthrough_branches = Vec::new();
            let mut any_partial = false;
            let mut any_unresolved = false;

            for branch in branches {
                let branch_key = branch.branch_index.to_string();
                let element_index = match &branch.target {
                    RootTargetRef::NativeElement { element_index, .. }
                    | RootTargetRef::DynamicComponentUsage { element_index, .. }
                    | RootTargetRef::ComponentUsage { element_index, .. }
                    | RootTargetRef::UnresolvedTarget { element_index, .. } => *element_index,
                };
                let resolved_consumed = host.resolve_root_consumption(
                    canonical_id,
                    &branch_key,
                    snapshot,
                    element_index,
                    &branch.consumed,
                    branch.has_unknown_spread,
                    &mut eval_env,
                    prop_type_overrides,
                );
                let consumed = &resolved_consumed.bindings;
                let parent_partial_reasons = resolved_consumed.partial_reasons.clone();

                match &branch.target {
                    RootTargetRef::NativeElement { tag, .. } => {
                        append_native_candidate_branch(
                            host,
                            canonical_id,
                            tag,
                            branch_key,
                            branch.condition_text.clone(),
                            &consumed.attrs,
                            &consumed.listeners,
                            &parent_partial_reasons,
                            &declared_prop_names,
                            &declared_event_names,
                            &declared_listener_aliases,
                            &mut fallthrough_branches,
                            &mut any_partial,
                        );
                    }
                    RootTargetRef::DynamicComponentUsage { usage_index, .. } => {
                        let child_prop_overrides = host.build_generic_child_prop_overrides(
                            canonical_id,
                            snapshot,
                            *usage_index,
                            &mut eval_env,
                            prop_type_overrides,
                        );
                        let candidates = host.resolve_dynamic_root_candidates(
                            canonical_id,
                            snapshot,
                            *usage_index,
                            &mut eval_env,
                            prop_type_overrides,
                        );

                        if candidates.is_empty() {
                            any_unresolved = true;
                            fallthrough_branches.push(FallthroughBranch {
                                branch_key,
                                condition_text: branch.condition_text.clone(),
                                props: Vec::new(),
                                events: Vec::new(),
                                root_chain: vec![ResolvedRootStep::Unresolved {
                                    tag: "component".to_string(),
                                    reason: UnresolvedBranchReason::DynamicComponentIs,
                                }],
                                status: BranchStatus::Unresolved {
                                    reason: UnresolvedBranchReason::DynamicComponentIs,
                                },
                            });
                            continue;
                        }

                        let multiple_candidates = candidates.len() > 1;
                        for (candidate_index, candidate) in candidates.into_iter().enumerate() {
                            let candidate_key = if multiple_candidates {
                                format!("{}.{}", branch_key, candidate_index)
                            } else {
                                branch_key.clone()
                            };
                            match candidate {
                                DynamicRootCandidate::NativeTag { tag } => {
                                    append_native_candidate_branch(
                                        host,
                                        canonical_id,
                                        &tag,
                                        candidate_key,
                                        branch.condition_text.clone(),
                                        &consumed.attrs,
                                        &consumed.listeners,
                                        &parent_partial_reasons,
                                        &declared_prop_names,
                                        &declared_event_names,
                                        &declared_listener_aliases,
                                        &mut fallthrough_branches,
                                        &mut any_partial,
                                    );
                                }
                                DynamicRootCandidate::ComponentImport {
                                    component_name,
                                    import_source,
                                    imported_name,
                                    binding_kind,
                                } => {
                                    append_component_candidate_branches(
                                        host,
                                        canonical_id,
                                        &component_name,
                                        &import_source,
                                        imported_name.as_deref(),
                                        binding_kind,
                                        candidate_key,
                                        branch.condition_text.clone(),
                                        &consumed.attrs,
                                        &consumed.listeners,
                                        &parent_partial_reasons,
                                        child_prop_overrides.as_ref(),
                                        &declared_prop_names,
                                        &declared_event_names,
                                        &declared_listener_aliases,
                                        &mut fallthrough_branches,
                                        &mut any_partial,
                                        &mut any_unresolved,
                                        &mut fact_versions,
                                        visiting,
                                    );
                                }
                            }
                        }
                    }
                    RootTargetRef::ComponentUsage {
                        usage_index,
                        name,
                        import_source,
                        ..
                    } => {
                        let child_prop_overrides = host.build_generic_child_prop_overrides(
                            canonical_id,
                            snapshot,
                            *usage_index,
                            &mut eval_env,
                            prop_type_overrides,
                        );

                        match import_source.as_deref() {
                            Some(import_source) => {
                                append_component_candidate_branches(
                                    host,
                                    canonical_id,
                                    name,
                                    import_source,
                                    None,
                                    None,
                                    branch_key,
                                    branch.condition_text.clone(),
                                    &consumed.attrs,
                                    &consumed.listeners,
                                    &parent_partial_reasons,
                                    child_prop_overrides.as_ref(),
                                    &declared_prop_names,
                                    &declared_event_names,
                                    &declared_listener_aliases,
                                    &mut fallthrough_branches,
                                    &mut any_partial,
                                    &mut any_unresolved,
                                    &mut fact_versions,
                                    visiting,
                                );
                            }
                            None => {
                                any_unresolved = true;
                                fallthrough_branches.push(FallthroughBranch {
                                    branch_key,
                                    condition_text: branch.condition_text.clone(),
                                    props: Vec::new(),
                                    events: Vec::new(),
                                    root_chain: vec![ResolvedRootStep::Unresolved {
                                        tag: name.clone(),
                                        reason: UnresolvedBranchReason::UnresolvedChildImport {
                                            import_source: None,
                                        },
                                    }],
                                    status: BranchStatus::Unresolved {
                                        reason: UnresolvedBranchReason::UnresolvedChildImport {
                                            import_source: None,
                                        },
                                    },
                                });
                            }
                        }
                    }
                    RootTargetRef::UnresolvedTarget { tag, reason, .. } => {
                        any_unresolved = true;
                        fallthrough_branches.push(FallthroughBranch {
                            branch_key,
                            condition_text: branch.condition_text.clone(),
                            props: Vec::new(),
                            events: Vec::new(),
                            root_chain: vec![ResolvedRootStep::Unresolved {
                                tag: tag.clone(),
                                reason: UnresolvedBranchReason::RootTarget {
                                    reason: reason.clone(),
                                },
                            }],
                            status: BranchStatus::Unresolved {
                                reason: UnresolvedBranchReason::RootTarget {
                                    reason: reason.clone(),
                                },
                            },
                        });
                    }
                }
            }

            fallthrough_branches.sort_by(|a, b| a.branch_key.cmp(&b.branch_key));
            let completeness = merge_fallthrough_branches(
                &mut accepted_props,
                &mut accepted_events,
                &fallthrough_branches,
                any_partial,
                any_unresolved,
            );

            ResolvedFallthroughSurface {
                accepted_props,
                accepted_events,
                accepted_surface_completeness: completeness,
                fallthrough_surface: FallthroughSurface::Branches {
                    branches: fallthrough_branches,
                },
                fact_versions,
            }
        }
    }
}

/// Structural substitution of bare single-segment `TypeOf(ValueRef)`
/// references with the CONCRETE graph-free projection of the annotation fact
/// bound to that value name in a standalone evaluation environment.
///
/// Used by the node-domain fallthrough value evaluator
/// (`evaluate_fallthrough_value_node`) to fold imported runtime-value bindings
/// before node projection: a reference resolved here is a concrete value type
/// (lowered to a node directly); everything else — including an annotation
/// whose fact carries only a non-concrete source (an authored locator, a
/// projected / synthesized fact) — stays unsubstituted and routes through
/// dispatch. It does NOT carry child prop-type overrides — those ride the
/// fallthrough recursion as node carriers and are forwarded in node domain.
pub fn structural_substitute_typeof_refs(
    expr: &TypeExpr,
    env: &verter_semantic::analysis::type_eval::EvalEnv,
) -> TypeExpr {
    match expr {
        TypeExpr::TypeOf(value_ref) if value_ref.path.len() == 1 => env
            .value_symbols
            .get(value_ref.path[0].as_str())
            .and_then(|group| concrete_annotation_expr(&group.primary().type_annotation))
            .unwrap_or_else(|| expr.clone()),
        TypeExpr::Union(parts) => TypeExpr::union(
            parts
                .iter()
                .map(|part| structural_substitute_typeof_refs(part, env))
                .collect(),
        ),
        TypeExpr::Intersection(parts) => TypeExpr::intersection(
            parts
                .iter()
                .map(|part| structural_substitute_typeof_refs(part, env))
                .collect(),
        ),
        TypeExpr::Parenthesized(inner) => TypeExpr::Parenthesized(std::sync::Arc::new(
            structural_substitute_typeof_refs(inner, env),
        )),
        other => other.clone(),
    }
}

/// The graph-free CONCRETE projection of a value's annotation fact — the two
/// annotation classes representable without a dispatch raise:
///
/// - a `typeof x` annotation peels to its precomputed
///   [`typeof_alias_target`](verter_type_expr::facts::ValueTypeAnnotationFact::typeof_alias_target)
///   (the fact producer guarantees a single-hop, non-self target), rebuilt as
///   the target's `TypeOf` reference;
/// - a closed LEAF annotation projects through the shared closed-grammar
///   [`leaf_type_fact_expr`](crate::project_semantic_dispatch::lower::leaf_type_fact_expr)
///   projection.
///
/// Any other source (an authored body locator, a projected / synthesized
/// fact, a composite closed fact) returns `None`: the `typeof` reference
/// stays unsubstituted and the caller's node projection resolves it through
/// the one shared dispatch — never a fabricated stand-in body.
fn concrete_annotation_expr(
    fact: &verter_type_expr::facts::ValueTypeAnnotationFact,
) -> Option<TypeExpr> {
    use verter_type_expr::facts::{ClosedTypeFact, SemanticTypeSource};
    if let Some(target) = fact.typeof_alias_target.as_ref() {
        return Some(TypeExpr::TypeOf(verter_type_expr::ValueRef {
            path: std::iter::once(target.symbol.as_ref().to_string())
                .chain(target.member_path.iter().cloned())
                .collect(),
            type_args: Vec::new(),
        }));
    }
    match fact.annotation.as_ref()? {
        SemanticTypeSource::Closed(ClosedTypeFact::Leaf(leaf)) => {
            Some(crate::project_semantic_dispatch::lower::leaf_type_fact_expr(leaf))
        }
        _ => None,
    }
}

pub fn push_partial_reason(reasons: &mut Vec<PartialBranchReason>, reason: PartialBranchReason) {
    if !reasons.iter().any(|existing| existing == &reason) {
        reasons.push(reason);
    }
}

pub fn known_spread_keys_from_type_expr(ty: &TypeExpr) -> Option<KnownSpreadKeys> {
    match ty {
        TypeExpr::Object(obj) => Some(known_spread_keys_from_object(obj)),
        TypeExpr::Parenthesized(inner) => known_spread_keys_from_type_expr(inner),
        TypeExpr::Intersection(types) => {
            let mut result = KnownSpreadKeys {
                exact: true,
                ..KnownSpreadKeys::default()
            };
            let mut saw_any = false;
            for part in types.iter() {
                let Some(summary) = known_spread_keys_from_type_expr(part) else {
                    result.exact = false;
                    continue;
                };
                saw_any = true;
                result.attrs.extend(summary.attrs);
                result.listeners.extend(summary.listeners);
                result.exact &= summary.exact;
            }
            saw_any.then_some(result)
        }
        TypeExpr::Union(types) => {
            let mut iter = types.iter();
            let first = known_spread_keys_from_type_expr(iter.next()?)?;
            let mut result = first.clone();
            let mut exact_same_keys = first.exact;
            for branch in iter {
                let Some(summary) = known_spread_keys_from_type_expr(branch) else {
                    result.exact = false;
                    return Some(result);
                };
                exact_same_keys &= summary.exact
                    && summary.attrs == result.attrs
                    && summary.listeners == result.listeners;
                result = intersect_known_spread_keys(result, summary);
            }
            result.exact = exact_same_keys;
            Some(result)
        }
        _ => None,
    }
}

pub fn collect_dynamic_root_candidates_from_type(
    ty: &TypeExpr,
    imports: &[AnalyzedImport],
) -> Vec<DynamicRootCandidate> {
    use verter_type_expr::{LiteralValue, TypeExpr};

    match ty {
        TypeExpr::Literal(LiteralValue::String(tag)) => {
            vec![DynamicRootCandidate::NativeTag { tag: tag.clone() }]
        }
        TypeExpr::Union(types) => types
            .iter()
            .flat_map(|branch| collect_dynamic_root_candidates_from_type(branch, imports))
            .collect(),
        TypeExpr::Parenthesized(inner) => collect_dynamic_root_candidates_from_type(inner, imports),
        TypeExpr::TypeOf(value_ref) if value_ref.path.len() == 1 => {
            component_import_candidate_for_binding(imports, value_ref.path[0].as_str())
                .into_iter()
                .collect()
        }
        _ => Vec::new(),
    }
}

/// Map a single-segment value-reference NAME to a
/// [`DynamicRootCandidate::ComponentImport`] by matching it against a
/// non-type-only import binding (preserving the import-binding-kind mapping).
/// Shared by the `TypeExpr` reader and the node-domain
/// `collect_dynamic_root_candidates_from_node` so the kind mapping cannot
/// drift between them. `None` when no matching value binding exists.
pub fn component_import_candidate_for_binding(
    imports: &[AnalyzedImport],
    name: &str,
) -> Option<DynamicRootCandidate> {
    imports
        .iter()
        .filter(|import| !import.is_type_only)
        .find_map(|import| {
            import
                .bindings
                .iter()
                .find(|binding| !binding.is_type_only && binding.name == name)
                .map(|binding| DynamicRootCandidate::ComponentImport {
                    component_name: name.to_string(),
                    import_source: import.source.clone(),
                    imported_name: binding.imported_name.clone(),
                    binding_kind: Some(match binding.kind {
                        verter_semantic::analysis::types::ImportBindingKind::Named => {
                            crate::resolver_core::symbol_resolver::ImportBindingKind::Named
                        }
                        verter_semantic::analysis::types::ImportBindingKind::Default => {
                            crate::resolver_core::symbol_resolver::ImportBindingKind::Default
                        }
                        verter_semantic::analysis::types::ImportBindingKind::Namespace => {
                            crate::resolver_core::symbol_resolver::ImportBindingKind::Namespace
                        }
                    }),
                })
        })
}

fn unresolved_child_import_branch(
    branch_key: String,
    condition_text: Option<String>,
    component_name: &str,
    import_source: Option<String>,
) -> FallthroughBranch {
    FallthroughBranch {
        branch_key,
        condition_text,
        props: Vec::new(),
        events: Vec::new(),
        root_chain: vec![ResolvedRootStep::Unresolved {
            tag: component_name.to_string(),
            reason: UnresolvedBranchReason::UnresolvedChildImport {
                import_source: import_source.clone(),
            },
        }],
        status: BranchStatus::Unresolved {
            reason: UnresolvedBranchReason::UnresolvedChildImport { import_source },
        },
    }
}

fn unresolved_child_resolution_branch(
    branch_key: String,
    condition_text: Option<String>,
    component_name: &str,
    child_id: String,
) -> FallthroughBranch {
    FallthroughBranch {
        branch_key,
        condition_text,
        props: Vec::new(),
        events: Vec::new(),
        root_chain: vec![ResolvedRootStep::Component {
            canonical_id: child_id,
            component_name: component_name.to_string(),
        }],
        status: BranchStatus::Unresolved {
            reason: UnresolvedBranchReason::ChildResolutionFailed,
        },
    }
}

/// Self-anchor a CHILD-published source before it crosses into a PARENT
/// row (invariant: cross-owner sources have a defined effective scope). A
/// child's published source may carry producer-local (empty) anchors that
/// are relative to the CHILD file; cloned into the parent's fallthrough
/// rows they would silently re-anchor to the parent when the output
/// boundary raises them under the parent scope. Absolutizing against the
/// child canonical at the clone boundary makes every inherited source
/// self-anchoring, so the parent owner is never used blindly as the raise
/// scope — and a cross-branch merge of same-shaped sources from DIFFERENT
/// children correctly compares unequal (the flat column publishes `None`;
/// the branch-structured surface keeps each branch's exact source).
fn self_anchor_inherited_source(source: &SourcePosition, child_id: &str) -> SourcePosition {
    match source {
        SourcePosition::Present(source) => {
            SourcePosition::Present(source.absolutized_against(child_id))
        }
        // A proven absence stays absent; a typed FAILURE propagates
        // fail-closed through inheritance — a failed required position on a
        // child surface must not become an untyped parent success.
        SourcePosition::Absent(_) | SourcePosition::Failed(_) => source.clone(),
    }
}

/// The PRODUCING scope an inherited source's scope-relative names (bare
/// `Ref` leaf spellings — un-anchorable by `absolutized_against`) resolve
/// under, threaded across the owner-clone boundary: the child row's OWN
/// effective scope when the child's row was itself inherited (a multi-hop
/// chain — the terminal origin survives every hop), else the direct child
/// (the file the source is spelled in). Companion of
/// [`self_anchor_inherited_source`] — anchors self-describe after
/// absolutization; the positional scope covers the anchor-FREE positions
/// (cross-owner effective-scope invariant: the parent owner is never used
/// blindly as the raise scope for an inherited source).
///
/// `source` is the row's ALREADY-ABSOLUTIZED source (the
/// [`self_anchor_inherited_source`] output). A scope attaches ONLY while
/// that source is still SCOPE-RELATIVE: a source-less, fully-closed, or
/// fully-anchored row publishes `None` — it has no name left to resolve, so
/// a positional producer scope would be an irrelevant discriminator on the
/// output-materialization memo key (`(effective scope, source identity)`),
/// splitting warm entries that identical closed sources from different
/// children should share.
fn inherited_source_scope(
    source: &SourcePosition,
    child_row_scope: &Option<String>,
    child_id: &str,
) -> Option<String> {
    if !source
        .present()
        .is_some_and(verter_type_expr::facts::SemanticTypeSource::is_scope_relative)
    {
        return None;
    }
    child_row_scope
        .clone()
        .or_else(|| Some(child_id.to_string()))
}

fn inherited_component_props(
    props: &[AcceptedPropAnalysis],
    declared_prop_names: &FxHashSet<String>,
    consumed_attrs: &[String],
    child_id: &str,
) -> Vec<FallthroughPropEntry> {
    props
        .iter()
        .filter(|prop| !declared_prop_names.contains(&prop.name))
        .filter(|prop| !consumed_attrs.iter().any(|attr| attr == &prop.name))
        .map(|prop| {
            let type_source = self_anchor_inherited_source(&prop.type_source, child_id);
            // PRODUCING scope for the source's scope-relative names: the
            // child's OWN effective scope when the child's row was itself
            // inherited (multi-hop chain — the terminal origin survives),
            // else the direct child (the file the source is spelled in).
            // Attached ONLY while the absolutized source is scope-relative.
            let type_source_scope =
                inherited_source_scope(&type_source, &prop.type_source_scope, child_id);
            FallthroughPropEntry {
                name: prop.name.clone(),
                type_source,
                type_source_scope,
                raw_type: prop.raw_type.clone(),
                sources: vec![InheritedSource::Component {
                    canonical_id: child_id.to_string(),
                }],
            }
        })
        .collect()
}

fn inherited_component_events(
    events: &[AcceptedEventAnalysis],
    declared_event_names: &FxHashSet<String>,
    declared_listener_aliases: &FxHashSet<String>,
    consumed_listeners: &[String],
    child_id: &str,
) -> Vec<FallthroughEventEntry> {
    events
        .iter()
        .filter(|event| {
            !declared_event_names.contains(&event.name)
                && !declared_listener_aliases.contains(&event.name)
        })
        .filter(|event| {
            !consumed_listeners
                .iter()
                .any(|listener| listener == &event.name)
        })
        .map(|event| {
            let payload = self_anchor_inherited_source(&event.payload, child_id);
            // Producer scope attached ONLY while the absolutized payload
            // source is scope-relative (see `inherited_source_scope`).
            let payload_scope = inherited_source_scope(&payload, &event.payload_scope, child_id);
            FallthroughEventEntry {
                name: event.name.clone(),
                payload,
                payload_scope,
                raw_signature: event.raw_signature.clone(),
                sources: vec![InheritedSource::Component {
                    canonical_id: child_id.to_string(),
                }],
            }
        })
        .collect()
}

fn inherited_declared_component_props(
    props: &[&AcceptedPropAnalysis],
    declared_prop_names: &FxHashSet<String>,
    consumed_attrs: &[String],
    child_id: &str,
) -> Vec<FallthroughPropEntry> {
    props
        .iter()
        .filter(|prop| !declared_prop_names.contains(&prop.name))
        .filter(|prop| !consumed_attrs.iter().any(|attr| attr == &prop.name))
        .map(|prop| {
            let type_source = self_anchor_inherited_source(&prop.type_source, child_id);
            // Producer scope attached ONLY while the absolutized source is
            // scope-relative (see `inherited_source_scope`).
            let type_source_scope =
                inherited_source_scope(&type_source, &prop.type_source_scope, child_id);
            FallthroughPropEntry {
                name: prop.name.clone(),
                type_source,
                type_source_scope,
                raw_type: prop.raw_type.clone(),
                sources: vec![InheritedSource::Component {
                    canonical_id: child_id.to_string(),
                }],
            }
        })
        .collect()
}

fn inherited_declared_component_events(
    events: &[&AcceptedEventAnalysis],
    declared_event_names: &FxHashSet<String>,
    declared_listener_aliases: &FxHashSet<String>,
    consumed_listeners: &[String],
    child_id: &str,
) -> Vec<FallthroughEventEntry> {
    events
        .iter()
        .filter(|event| {
            !declared_event_names.contains(&event.name)
                && !declared_listener_aliases.contains(&event.name)
        })
        .filter(|event| {
            !consumed_listeners
                .iter()
                .any(|listener| listener == &event.name)
        })
        .map(|event| {
            let payload = self_anchor_inherited_source(&event.payload, child_id);
            // Producer scope attached ONLY while the absolutized payload
            // source is scope-relative (see `inherited_source_scope`).
            let payload_scope = inherited_source_scope(&payload, &event.payload_scope, child_id);
            FallthroughEventEntry {
                name: event.name.clone(),
                payload,
                payload_scope,
                raw_signature: event.raw_signature.clone(),
                sources: vec![InheritedSource::Component {
                    canonical_id: child_id.to_string(),
                }],
            }
        })
        .collect()
}

/// Absorbing cross-branch accumulator for the flattened accepted column's
/// type SOURCE (props AND event payloads uniformly). Equal-identity sources
/// (the dominant case — the same child contributing in every branch) keep
/// the one source; an untyped branch side (`None`) never changes the state
/// (the one-sided merge adopts the typed side); GENUINELY CONFLICTING
/// sources land in the absorbing [`Self::Conflict`] state — once any two
/// branches disagreed, a later branch can never revive a value — and
/// finalize to `None` (honest "unknown") on the flat column ONLY after
/// every branch folded. The content-free source vocabulary carries no
/// session-composed union; the EXACT per-branch sources remain
/// authoritative on the branch-structured surface
/// (`FallthroughSurface::Branches`), where each entry keeps its own
/// branch's source (a consumer composing the cross-branch union raises each
/// branch's source through the shared dispatch and composes node-side).
///
/// The agreement identity is the source VALUE plus — while the source is
/// SCOPE-RELATIVE (`SemanticTypeSource::is_scope_relative`: a bare `Ref`
/// leaf spelling / a producer-local anchor) — the entry's positional
/// PRODUCING scope (`FallthroughPropEntry::type_source_scope` /
/// `FallthroughEventEntry::payload_scope` — the terminal origin of the
/// inheritance chain). Two children publishing the same closed-`Ref`
/// SPELLING from different declaration origins are NOT in agreement (the
/// spelling names two different declarations); two branches inheriting the
/// SAME terminal origin (even through different intermediate children)
/// are; two publishers of the same fully-ANCHORED source are (the anchor
/// pins one declaration regardless of publisher).
///
/// The agreed producing scope SURVIVES finalization onto the flat merged
/// row, so the output boundary raises the whole inherited source —
/// including nested scope-relative refs — under its PRODUCING scope, never
/// blindly under the parent owner.
enum MergedSourceState {
    /// No typed branch side folded yet.
    Unset,
    /// Every typed branch side so far carried this one source identity
    /// (boxed: the accumulator's other states carry no data).
    Agreed(Box<AgreedSourceIdentity>),
    /// Two typed branch sides disagreed. Absorbing (except by `Failed`).
    Conflict,
    /// A branch side carried a typed source-construction FAILURE. Absorbing
    /// over every other state: a failed required position inherited from any
    /// branch must not vanish into a merged "untyped" success.
    Failed(verter_type_expr::facts::SemanticSourceFailure),
}

/// The agreement identity [`MergedSourceState::Agreed`] carries.
struct AgreedSourceIdentity {
    source: verter_type_expr::facts::SemanticTypeSource,
    /// The first contributing entry's positional producing scope —
    /// identity-bearing only while `source.is_scope_relative()`, and
    /// carried onto the finalized flat row as its raise scope.
    scope: Option<String>,
}

impl MergedSourceState {
    fn fold(
        &mut self,
        incoming: &verter_type_expr::facts::SourcePosition,
        incoming_scope: Option<&str>,
    ) {
        use verter_type_expr::facts::SourcePosition;
        let incoming = match incoming {
            // An ABSENT branch side neither conflicts nor revives: the
            // one-sided merge adopts the typed side (state unchanged).
            SourcePosition::Absent(_) => return,
            // A FAILED branch side absorbs the whole merged row: a failed
            // required position must never vanish into a merged success.
            SourcePosition::Failed(failure) => {
                *self = MergedSourceState::Failed(*failure);
                return;
            }
            SourcePosition::Present(source) => source,
        };
        match self {
            MergedSourceState::Unset => {
                *self = MergedSourceState::Agreed(Box::new(AgreedSourceIdentity {
                    source: incoming.clone(),
                    scope: incoming_scope.map(str::to_string),
                }));
            }
            MergedSourceState::Agreed(agreed) => {
                let same_identity = agreed.source == *incoming
                    && (!agreed.source.is_scope_relative()
                        || agreed.scope.as_deref() == incoming_scope);
                if !same_identity {
                    *self = MergedSourceState::Conflict;
                }
            }
            MergedSourceState::Conflict => {}
            MergedSourceState::Failed(_) => {}
        }
    }

    /// Finalize AFTER every branch folded: `Unset` publishes the proven
    /// unannotated absence (no typed side existed); `Conflict` publishes the
    /// PROVEN branch-divergent absence (typed sides with distinct
    /// identities); a `Failed` side publishes the typed failure (fails
    /// output materialization); agreement publishes the source TOGETHER WITH
    /// its producing scope.
    fn finalize(self) -> (verter_type_expr::facts::SourcePosition, Option<String>) {
        use verter_type_expr::facts::{SchemaAbsence, SourcePosition};
        match self {
            MergedSourceState::Agreed(agreed) => {
                (SourcePosition::Present(agreed.source), agreed.scope)
            }
            MergedSourceState::Unset => (SourcePosition::unannotated(), None),
            MergedSourceState::Conflict => {
                (SourcePosition::Absent(SchemaAbsence::BranchDivergent), None)
            }
            MergedSourceState::Failed(failure) => (SourcePosition::Failed(failure), None),
        }
    }
}

fn merge_inherited_sources(existing: &mut Vec<InheritedSource>, incoming: &[InheritedSource]) {
    existing.extend(incoming.iter().cloned());
    existing.sort();
    existing.dedup();
}

pub(crate) fn normalize_public_spread_key(
    key: &str,
    attrs: &mut std::collections::BTreeSet<String>,
    listeners: &mut std::collections::BTreeSet<String>,
) {
    if key == "class" || key == "style" {
        return;
    }
    if let Some(event_name) = verter_semantic::analysis::html_intrinsics::on_prop_to_event_name(key)
    {
        listeners.insert(event_name.to_string());
    } else {
        attrs.insert(key.to_string());
    }
}

fn known_spread_keys_from_object(object: &verter_type_expr::ObjectExpr) -> KnownSpreadKeys {
    let mut result = KnownSpreadKeys {
        exact: true,
        ..KnownSpreadKeys::default()
    };

    for member in &object.properties {
        match member {
            verter_type_expr::ObjectMember::Property(prop) => {
                normalize_public_spread_key(&prop.name, &mut result.attrs, &mut result.listeners)
            }
            verter_type_expr::ObjectMember::Method(method) => {
                normalize_public_spread_key(&method.name, &mut result.attrs, &mut result.listeners)
            }
            verter_type_expr::ObjectMember::IndexSignature(_)
            | verter_type_expr::ObjectMember::CallSignature(_)
            | verter_type_expr::ObjectMember::ConstructSignature(_) => {
                result.exact = false;
            }
        }
    }

    result
}

pub(crate) fn intersect_known_spread_keys(
    mut left: KnownSpreadKeys,
    right: KnownSpreadKeys,
) -> KnownSpreadKeys {
    left.attrs = left.attrs.intersection(&right.attrs).cloned().collect();
    left.listeners = left
        .listeners
        .intersection(&right.listeners)
        .cloned()
        .collect();
    left.exact &= right.exact;
    left
}

#[cfg(test)]
mod tests {
    use super::{
        append_component_candidate_branches, append_native_candidate_branch,
        collect_dynamic_root_candidates_from_type, fallthrough_cache_key,
        inherited_component_events, inherited_component_props, known_spread_keys_from_type_expr,
        merge_fallthrough_branches, resolve_fallthrough_surface, structural_substitute_typeof_refs,
        DynamicRootCandidate, FallthroughComputeHost, FallthroughPropOverrideSet,
        FallthroughResolutionView, FallthroughResolverHost, IntrinsicMemberTypeSource,
        IntrinsicSurfaceMember, ResolvedConsumedBindings,
    };
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::sync::Arc;
    use verter_semantic::analysis::component_meta::{
        AcceptedEventAnalysis, AcceptedPropAnalysis, AcceptedPropKind, AcceptedSurfaceCompleteness,
        BranchStatus, ComponentMetaAnalysis, ConsumedRootBindings, FallthroughBranch,
        FallthroughSurface, InheritedSource, MemberAvailability, MemberProvenance,
        ResolvedRootStep, RootBranch, RootReachability, RootTargetRef,
    };
    use verter_semantic::analysis::html_intrinsics::{
        html_intrinsic_catalog, intrinsic_listeners_for_tag, owned_intrinsic_members_for_tag,
        IntrinsicMemberKind, IntrinsicTypeShape,
    };
    use verter_semantic::analysis::types::{
        AnalyzedImport, AnalyzedImportBinding, ImportBindingKind,
    };
    use verter_span::Span;
    use verter_type_expr::facts::{
        ClosedTypeFact, LeafTypeFact, SemanticTypeSource, SourcePosition,
    };
    use verter_type_expr::intrinsics::StaticIntrinsicTypeId;
    use verter_type_expr::{
        ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr, ValueRef,
    };

    #[derive(Clone)]
    struct TestResolution {
        accepted_props: Vec<AcceptedPropAnalysis>,
        accepted_events: Vec<AcceptedEventAnalysis>,
        fallthrough_surface: FallthroughSurface,
        fact_versions: Vec<crate::resolver_core::FactVersionRef>,
    }

    impl FallthroughResolutionView for TestResolution {
        fn accepted_props(&self) -> &[AcceptedPropAnalysis] {
            &self.accepted_props
        }

        fn accepted_events(&self) -> &[AcceptedEventAnalysis] {
            &self.accepted_events
        }

        fn fallthrough_surface(&self) -> &FallthroughSurface {
            &self.fallthrough_surface
        }

        fn fact_versions(&self) -> &[crate::resolver_core::FactVersionRef] {
            &self.fact_versions
        }
    }

    #[derive(Default)]
    struct TestHost {
        intrinsic_members: FxHashMap<String, Vec<IntrinsicSurfaceMember>>,
        canonical_routes: FxHashMap<(String, String), String>,
        child_resolutions: FxHashMap<String, TestResolution>,
    }

    impl FallthroughResolverHost for TestHost {
        type ChildResolution = TestResolution;

        fn intrinsic_members_for_tag(
            &self,
            _canonical_id: &str,
            tag: &str,
        ) -> Vec<IntrinsicSurfaceMember> {
            self.intrinsic_members.get(tag).cloned().unwrap_or_default()
        }

        fn resolve_child_component_canonical(
            &self,
            parent_canonical: &str,
            _component_name: &str,
            import_source: &str,
            _imported_name: Option<&str>,
            _binding_kind: Option<crate::resolver_core::symbol_resolver::ImportBindingKind>,
        ) -> Option<String> {
            self.canonical_routes
                .get(&(parent_canonical.to_string(), import_source.to_string()))
                .cloned()
        }

        fn current_dependency_fact_versions(
            &self,
            canonical_id: &str,
        ) -> Vec<crate::resolver_core::FactVersionRef> {
            vec![crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: canonical_id.to_string(),
                hash: [1; 16],
            }]
        }

        fn resolve_child_fallthrough(
            &self,
            canonical_id: &str,
            _prop_type_overrides: Option<&FallthroughPropOverrideSet>,
            _visiting: &mut FxHashSet<String>,
        ) -> Option<Self::ChildResolution> {
            self.child_resolutions.get(canonical_id).cloned()
        }
    }

    impl FallthroughComputeHost for TestHost {
        type Snapshot = ();
        type EvalEnv = ();

        fn resolve_root_consumption(
            &self,
            _canonical_id: &str,
            _branch_key: &str,
            _snapshot: &Self::Snapshot,
            _element_index: u32,
            base: &ConsumedRootBindings,
            _has_unknown_spread: bool,
            _eval_env: &mut Option<Self::EvalEnv>,
            _overrides: Option<&FallthroughPropOverrideSet>,
        ) -> ResolvedConsumedBindings {
            ResolvedConsumedBindings {
                bindings: base.clone(),
                partial_reasons: Vec::new(),
            }
        }

        fn build_generic_child_prop_overrides(
            &self,
            _canonical_id: &str,
            _snapshot: &Self::Snapshot,
            _usage_index: u32,
            _eval_env: &mut Option<Self::EvalEnv>,
            _overrides: Option<&FallthroughPropOverrideSet>,
        ) -> Option<FallthroughPropOverrideSet> {
            None
        }

        fn resolve_dynamic_root_candidates(
            &self,
            _canonical_id: &str,
            _snapshot: &Self::Snapshot,
            _usage_index: u32,
            _eval_env: &mut Option<Self::EvalEnv>,
            _overrides: Option<&FallthroughPropOverrideSet>,
        ) -> Vec<DynamicRootCandidate> {
            Vec::new()
        }
    }

    fn empty_component_meta(root_reachability: RootReachability) -> ComponentMetaAnalysis {
        ComponentMetaAnalysis {
            props: Vec::new(),
            events: Vec::new(),
            slots: Vec::new(),
            models: Vec::new(),
            exposed: Vec::new(),
            public_instance: None,
            sfc_blocks: None,
            type_registry: Vec::new(),
            components: Vec::new(),
            template_refs: Vec::new(),
            imports: Vec::new(),
            bindings: Vec::new(),
            vue_api_calls: Vec::new(),
            styles: Vec::new(),
            flags: Default::default(),
            root_reachability,
            accepted_props: Vec::new(),
            accepted_events: Vec::new(),
            accepted_surface_completeness: AcceptedSurfaceCompleteness::Exact,
            fallthrough_surface: FallthroughSurface::None {
                reason: verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
            },
            macro_expansion_diagnostics: Vec::new(),
            options_api: false,
            file_path: "/App.vue".to_string(),
        }
    }

    #[test]
    fn fallthrough_cache_key_is_uncacheable_for_any_override_set() {
        // Wholesale-uncacheable: a no-override key (`NoOverrides`) is cacheable;
        // a key built for ANY non-empty override set is `Uncacheable`, so the
        // two key values differ and the override-bearing one is never
        // warm-reused. An empty set canonicalizes to the same key as `None`.
        use crate::resolver_core::FallthroughPropOverride;
        use crate::semantic_query::SemanticNodeId;

        let none_key = fallthrough_cache_key("/App.vue", true, None);
        assert!(none_key.is_cacheable(), "a no-override key is cacheable");

        let overrides = FallthroughPropOverrideSet {
            entries: vec![FallthroughPropOverride {
                name: "p".to_string(),
                node: SemanticNodeId(1),
            }],
        };
        let override_key = fallthrough_cache_key("/App.vue", true, Some(&overrides));
        assert!(
            !override_key.is_cacheable(),
            "any non-empty override set makes the key wholesale uncacheable"
        );
        assert_ne!(
            none_key, override_key,
            "the override-bearing key differs from the no-override key"
        );

        let empty = FallthroughPropOverrideSet {
            entries: Vec::new(),
        };
        assert_eq!(
            fallthrough_cache_key("/App.vue", true, Some(&empty)),
            none_key,
            "an empty override set canonicalizes to the same key as None"
        );
    }

    #[test]
    fn for_overrides_maps_nonempty_to_uncacheable_and_empty_to_no_overrides() {
        use crate::resolver_core::{FallthroughOverrideIdentity, FallthroughPropOverride};
        use crate::semantic_query::SemanticNodeId;

        assert_eq!(
            FallthroughOverrideIdentity::for_overrides(None),
            FallthroughOverrideIdentity::NoOverrides,
            "None maps to NoOverrides"
        );
        let empty = FallthroughPropOverrideSet {
            entries: Vec::new(),
        };
        assert_eq!(
            FallthroughOverrideIdentity::for_overrides(Some(&empty)),
            FallthroughOverrideIdentity::NoOverrides,
            "an empty set maps to NoOverrides"
        );
        let non_empty = FallthroughPropOverrideSet {
            entries: vec![FallthroughPropOverride {
                name: "p".to_string(),
                node: SemanticNodeId(7),
            }],
        };
        assert_eq!(
            FallthroughOverrideIdentity::for_overrides(Some(&non_empty)),
            FallthroughOverrideIdentity::Uncacheable,
            "a non-empty set maps to Uncacheable (wholesale)"
        );
    }

    #[test]
    fn append_native_candidate_branch_filters_declared_and_consumed_members() {
        let string_attr_id = html_intrinsic_catalog()
            .id_for(&IntrinsicTypeShape::Primitive(PrimitiveName::String))
            .expect("the generated catalog interns string attr shapes");
        let mut host = TestHost::default();
        host.intrinsic_members.insert(
            "button".to_string(),
            vec![
                IntrinsicSurfaceMember {
                    name: "id".to_string(),
                    kind: IntrinsicMemberKind::Attr,
                    source: IntrinsicMemberTypeSource::Static(string_attr_id),
                },
                IntrinsicSurfaceMember {
                    name: "click".to_string(),
                    kind: IntrinsicMemberKind::Listener,
                    source: IntrinsicMemberTypeSource::Resolved(SemanticTypeSource::Closed(
                        ClosedTypeFact::Leaf(LeafTypeFact::Ref("MouseEvent".to_string())),
                    )),
                },
            ],
        );

        let mut branches = Vec::new();
        let mut any_partial = false;
        append_native_candidate_branch(
            &host,
            "/src/App.vue",
            "button",
            "0".to_string(),
            None,
            &["id".to_string()],
            &[],
            &[],
            &FxHashSet::default(),
            &FxHashSet::default(),
            &FxHashSet::default(),
            &mut branches,
            &mut any_partial,
        );

        assert_eq!(branches.len(), 1);
        assert!(branches[0].props.is_empty());
        assert_eq!(branches[0].events.len(), 1);
        // The surviving resolved-source listener carries its SOURCE on the
        // semantic channel and nothing on the display channel.
        assert_eq!(branches[0].events[0].name, "click");
        assert_eq!(
            branches[0].events[0].payload,
            verter_type_expr::facts::SourcePosition::Present(SemanticTypeSource::Closed(
                ClosedTypeFact::Leaf(LeafTypeFact::Ref("MouseEvent".to_string()))
            )),
            "a resolved member publishes its semantic source verbatim"
        );
        assert_eq!(branches[0].events[0].raw_signature, None);
    }

    /// The static-catalog member type projection: a PRIMITIVE catalog shape
    /// publishes the closed leaf FACT on the semantic channel (raised through
    /// the shared dispatch bridge on demand); a NON-PRIMITIVE catalog shape
    /// publishes its table-resident display text on the display channel only —
    /// never a fabricated semantic fact; an out-of-range id publishes neither.
    /// Exercised end-to-end through `append_native_candidate_branch` over real
    /// generated `div` members.
    #[test]
    fn native_branch_projects_static_member_types_onto_source_and_display_channels() {
        let catalog = html_intrinsic_catalog();
        let mut host = TestHost::default();
        let div_members: Vec<IntrinsicSurfaceMember> = owned_intrinsic_members_for_tag("div")
            .into_iter()
            .map(|fact| IntrinsicSurfaceMember {
                name: fact.name,
                kind: fact.kind,
                source: IntrinsicMemberTypeSource::Static(fact.type_id),
            })
            .collect();
        // Shape-class census up front so the per-entry loop below cannot pass
        // vacuously: the generated `div` surface must carry BOTH a primitive
        // attr (e.g. `id: string`) and a non-primitive display shape (every
        // listener, e.g. `click`).
        let shape_of = |member: &IntrinsicSurfaceMember| match &member.source {
            IntrinsicMemberTypeSource::Static(id) => catalog
                .shape(*id)
                .expect("generated member ids resolve in the catalog")
                .clone(),
            IntrinsicMemberTypeSource::Resolved(_) => {
                unreachable!("this fixture builds static members only")
            }
        };
        assert!(
            div_members
                .iter()
                .any(|m| matches!(shape_of(m), IntrinsicTypeShape::Primitive(_))),
            "the generated div surface must contain a primitive-typed attr"
        );
        assert!(
            div_members.iter().any(|m| matches!(
                shape_of(m),
                IntrinsicTypeShape::AttrDisplay(_) | IntrinsicTypeShape::ListenerFunction(_)
            )),
            "the generated div surface must contain a display-shaped member"
        );
        let expected: FxHashMap<(u8, String), IntrinsicTypeShape> = div_members
            .iter()
            .map(|m| {
                (
                    (
                        matches!(m.kind, IntrinsicMemberKind::Listener) as u8,
                        m.name.clone(),
                    ),
                    shape_of(m),
                )
            })
            .collect();
        host.intrinsic_members
            .insert("div".to_string(), div_members);

        let mut branches = Vec::new();
        let mut any_partial = false;
        append_native_candidate_branch(
            &host,
            "/src/App.vue",
            "div",
            "0".to_string(),
            None,
            &[],
            &[],
            &[],
            &FxHashSet::default(),
            &FxHashSet::default(),
            &FxHashSet::default(),
            &mut branches,
            &mut any_partial,
        );

        assert_eq!(branches.len(), 1);
        let branch = &branches[0];
        assert!(!branch.props.is_empty(), "div attrs must be inherited");
        assert!(!branch.events.is_empty(), "div listeners must be inherited");
        for prop in &branch.props {
            match &expected[&(0, prop.name.clone())] {
                IntrinsicTypeShape::Primitive(name) => {
                    assert_eq!(
                        prop.type_source,
                        verter_type_expr::facts::SourcePosition::Present(
                            SemanticTypeSource::Closed(ClosedTypeFact::Leaf(
                                LeafTypeFact::Primitive(*name)
                            ))
                        ),
                        "primitive attr `{}` publishes the closed leaf fact",
                        prop.name
                    );
                    assert_eq!(prop.raw_type, None);
                }
                IntrinsicTypeShape::AttrDisplay(text) => {
                    assert_eq!(
                        prop.type_source,
                        verter_type_expr::facts::SourcePosition::unannotated(),
                        "display attr `{}` must not fabricate a semantic fact",
                        prop.name
                    );
                    assert_eq!(prop.raw_type.as_deref(), Some(text.as_str()));
                }
                IntrinsicTypeShape::ListenerFunction(_) => {
                    panic!("attr `{}` cannot carry a listener shape", prop.name)
                }
            }
        }
        for event in &branch.events {
            match &expected[&(1, event.name.clone())] {
                IntrinsicTypeShape::ListenerFunction(text) => {
                    assert_eq!(
                        event.payload,
                        verter_type_expr::facts::SourcePosition::unannotated(),
                        "listener `{}` display text must not fabricate a semantic fact",
                        event.name
                    );
                    assert_eq!(event.raw_signature.as_deref(), Some(text.as_str()));
                }
                other => panic!(
                    "listener `{}` must carry a listener shape, got {other:?}",
                    event.name
                ),
            }
        }
    }

    /// Channel projection edge cases not reachable through the generated `div`
    /// surface: a listener whose catalog shape is recovered from a REAL
    /// generated listener id, and an out-of-range (fabricated) id, which
    /// carries no type on either channel.
    #[test]
    fn static_member_type_channels_recover_listener_shape_and_reject_fabricated_ids() {
        let catalog = html_intrinsic_catalog();
        let click = intrinsic_listeners_for_tag("div")
            .into_iter()
            .find(|member| member.name == "click")
            .expect("the generated div surface exposes the click listener");
        let (source, display) = IntrinsicMemberTypeSource::Static(click.type_id).type_channels();
        let Some(IntrinsicTypeShape::ListenerFunction(text)) = catalog.shape(click.type_id) else {
            panic!("click must intern a listener-function shape");
        };
        assert_eq!(
            source,
            verter_type_expr::facts::SourcePosition::unannotated(),
            "listener display text is not a semantic fact"
        );
        assert_eq!(display.as_deref(), Some(text.as_str()));

        let fabricated = StaticIntrinsicTypeId::from_u32(u32::MAX);
        assert_eq!(
            catalog.shape(fabricated),
            None,
            "the fabricated id must be out of range for this assertion to discriminate"
        );
        assert_eq!(
            IntrinsicMemberTypeSource::Static(fabricated).type_channels(),
            (verter_type_expr::facts::SourcePosition::unannotated(), None),
            "an out-of-range id is honestly untyped on both channels"
        );
    }

    #[test]
    fn append_component_candidate_branches_composes_child_branch_status_and_facts() {
        let mut host = TestHost::default();
        host.canonical_routes.insert(
            ("/App.vue".to_string(), "./Child.vue".to_string()),
            "/Child.vue".to_string(),
        );
        host.child_resolutions.insert(
            "/Child.vue".to_string(),
            TestResolution {
                accepted_props: vec![AcceptedPropAnalysis {
                    name: "title".to_string(),
                    type_source: verter_type_expr::facts::SourcePosition::Present(
                        SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
                            PrimitiveName::String,
                        ))),
                    ),
                    type_source_scope: None,
                    raw_type: Some("string".to_string()),
                    raw_type_source: None,
                    required: false,
                    provenance: MemberProvenance::Declared,
                    availability: MemberAvailability::Always,
                    kind: AcceptedPropKind::DeclaredProp,
                }],
                accepted_events: vec![],
                fallthrough_surface: FallthroughSurface::Branches {
                    branches: vec![FallthroughBranch {
                        branch_key: "0".to_string(),
                        condition_text: None,
                        props: vec![],
                        events: vec![],
                        root_chain: vec![ResolvedRootStep::NativeTag {
                            tag: "div".to_string(),
                        }],
                        status: BranchStatus::Resolved,
                    }],
                },
                fact_versions: vec![crate::resolver_core::FactVersionRef::FileWholeHash {
                    canonical_id: "/Child.vue".to_string(),
                    hash: [2; 16],
                }],
            },
        );

        let mut branches = Vec::new();
        let mut any_partial = false;
        let mut any_unresolved = false;
        let mut facts = Vec::new();
        append_component_candidate_branches(
            &host,
            "/App.vue",
            "Child",
            "./Child.vue",
            None,
            None,
            "0".to_string(),
            None,
            &[],
            &[],
            &[],
            None,
            &FxHashSet::default(),
            &FxHashSet::default(),
            &FxHashSet::default(),
            &mut branches,
            &mut any_partial,
            &mut any_unresolved,
            &mut facts,
            &mut FxHashSet::default(),
        );

        assert!(!any_partial);
        assert!(!any_unresolved);
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].branch_key, "0.0");
        assert_eq!(facts.len(), 2);
    }

    #[test]
    fn append_component_candidate_branches_resolves_svelte_child_fallthrough() {
        // A `.svelte` child component is a framework CARRIER, exactly like a
        // `.vue` SFC. The child-resolution gate is carrier-generic
        // (`is_framework_carrier()`, via the static classifier), so a resolved
        // `.svelte` child's fallthrough / root-attr inheritance IS computed —
        // its resolution branch is NOT forced unresolved.
        //
        // DISCRIMINATING: under the pre-fix `child_id.ends_with(".vue")` gate a
        // `.svelte` child fails the suffix check, so `*any_unresolved` is set,
        // an `unresolved_child_resolution_branch` (status `Unresolved`,
        // branch_key `"0"`) is pushed, and the function returns BEFORE calling
        // `resolve_child_fallthrough` — so the child resolution's facts never
        // merge. Every assertion below (no unresolved, the `"0.0"` resolved
        // branch_key, `Resolved` status, two facts) FAILS against the old gate
        // and PASSES with the carrier-generic one. This mirrors the `.vue`
        // sibling test above, swapping the carrier extension.
        let mut host = TestHost::default();
        host.canonical_routes.insert(
            ("/App.vue".to_string(), "./Child.svelte".to_string()),
            "/Child.svelte".to_string(),
        );
        host.child_resolutions.insert(
            "/Child.svelte".to_string(),
            TestResolution {
                accepted_props: vec![AcceptedPropAnalysis {
                    name: "title".to_string(),
                    type_source: verter_type_expr::facts::SourcePosition::Present(
                        SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
                            PrimitiveName::String,
                        ))),
                    ),
                    type_source_scope: None,
                    raw_type: Some("string".to_string()),
                    raw_type_source: None,
                    required: false,
                    provenance: MemberProvenance::Declared,
                    availability: MemberAvailability::Always,
                    kind: AcceptedPropKind::DeclaredProp,
                }],
                accepted_events: vec![],
                fallthrough_surface: FallthroughSurface::Branches {
                    branches: vec![FallthroughBranch {
                        branch_key: "0".to_string(),
                        condition_text: None,
                        props: vec![],
                        events: vec![],
                        root_chain: vec![ResolvedRootStep::NativeTag {
                            tag: "div".to_string(),
                        }],
                        status: BranchStatus::Resolved,
                    }],
                },
                fact_versions: vec![crate::resolver_core::FactVersionRef::FileWholeHash {
                    canonical_id: "/Child.svelte".to_string(),
                    hash: [2; 16],
                }],
            },
        );

        let mut branches = Vec::new();
        let mut any_partial = false;
        let mut any_unresolved = false;
        let mut facts = Vec::new();
        append_component_candidate_branches(
            &host,
            "/App.vue",
            "Child",
            "./Child.svelte",
            None,
            None,
            "0".to_string(),
            None,
            &[],
            &[],
            &[],
            None,
            &FxHashSet::default(),
            &FxHashSet::default(),
            &FxHashSet::default(),
            &mut branches,
            &mut any_partial,
            &mut any_unresolved,
            &mut facts,
            &mut FxHashSet::default(),
        );

        // The `.svelte` child resolved: NOT forced into the unresolved branch.
        assert!(!any_partial);
        assert!(
            !any_unresolved,
            "a resolved `.svelte` carrier child must not force the unresolved branch"
        );
        assert_eq!(branches.len(), 1);
        // The resolved-child fallthrough composition produces the nested
        // `"0.0"` branch_key, NOT the bare `"0"` of an unresolved branch.
        assert_eq!(
            branches[0].branch_key, "0.0",
            "the resolved-child fallthrough branch must be composed, not stubbed unresolved"
        );
        assert_eq!(
            branches[0].status,
            BranchStatus::Resolved,
            "the `.svelte` child branch must be Resolved, not Unresolved"
        );
        // The child resolution's fact version merged in (it would be absent had
        // the gate returned early before `resolve_child_fallthrough`).
        assert_eq!(facts.len(), 2);
    }

    #[test]
    fn merge_fallthrough_branches_marks_partial_surfaces_conditional() {
        let mut accepted_props = Vec::new();
        let mut accepted_events = Vec::new();
        let branches = vec![
            FallthroughBranch {
                branch_key: "0".to_string(),
                condition_text: None,
                props: vec![verter_semantic::analysis::component_meta::FallthroughPropEntry {
                    name: "id".to_string(),
                    type_source: verter_type_expr::facts::SourcePosition::Present(SemanticTypeSource::Closed(
                        ClosedTypeFact::Leaf(LeafTypeFact::Primitive(PrimitiveName::String))
                    )),
                    type_source_scope: None,
                    raw_type: Some("string".to_string()),
                    sources: vec![InheritedSource::NativeTag {
                        tag: "div".to_string(),
                    }],
                }],
                events: vec![],
                root_chain: vec![],
                status: BranchStatus::Resolved,
            },
            FallthroughBranch {
                branch_key: "1".to_string(),
                condition_text: None,
                props: vec![],
                events: vec![],
                root_chain: vec![],
                status: BranchStatus::Unresolved {
                    reason:
                        verter_semantic::analysis::component_meta::UnresolvedBranchReason::DynamicComponentIs,
                },
            },
        ];

        let completeness = merge_fallthrough_branches(
            &mut accepted_props,
            &mut accepted_events,
            &branches,
            false,
            true,
        );

        assert_eq!(completeness, AcceptedSurfaceCompleteness::LowerBound);
        assert_eq!(accepted_props.len(), 1);
        assert!(matches!(
            accepted_props[0].availability,
            MemberAvailability::Conditional { .. }
        ));
    }

    #[test]
    fn known_spread_keys_from_type_expr_intersects_union_keys() {
        let summary = known_spread_keys_from_type_expr(&TypeExpr::union(vec![
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                    "id".to_string(),
                    TypeExpr::primitive(PrimitiveName::String),
                    false,
                    false,
                ))],
            })),
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![
                    ObjectMember::Property(ObjectProperty::synthetic_public(
                        "id".to_string(),
                        TypeExpr::primitive(PrimitiveName::String),
                        false,
                        false,
                    )),
                    ObjectMember::Property(ObjectProperty::synthetic_public(
                        "title".to_string(),
                        TypeExpr::primitive(PrimitiveName::String),
                        false,
                        false,
                    )),
                ],
            })),
        ]))
        .expect("union object spread keys should resolve");

        assert!(summary.attrs.contains("id"));
        assert!(!summary.attrs.contains("title"));
    }

    #[test]
    fn collect_dynamic_root_candidates_from_type_maps_typeof_imports() {
        let imports = vec![AnalyzedImport {
            source: "./Child.vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "Child".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: None,
                is_type_only: false,
                vue_api: None,
                span: Span::new(0, 0),
            }],
            span: Span::new(0, 0),
            resolved_canonical_id: Some("/Child.vue".to_string()),
        }];

        let candidates = collect_dynamic_root_candidates_from_type(
            &TypeExpr::TypeOf(ValueRef {
                path: vec!["Child".to_string()],
                type_args: Vec::new(),
            }),
            imports.as_slice(),
        );

        assert_eq!(
            candidates,
            vec![super::DynamicRootCandidate::ComponentImport {
                component_name: "Child".to_string(),
                import_source: "./Child.vue".to_string(),
                imported_name: None,
                binding_kind: Some(crate::resolver_core::symbol_resolver::ImportBindingKind::Named),
            }]
        );
    }

    #[test]
    fn collect_dynamic_root_candidates_from_type_preserves_default_binding_kind() {
        let imports = vec![AnalyzedImport {
            source: "./Child.vue".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "Child".to_string(),
                kind: ImportBindingKind::Default,
                imported_name: Some("default".to_string()),
                is_type_only: false,
                vue_api: None,
                span: Span::new(0, 0),
            }],
            span: Span::new(0, 0),
            resolved_canonical_id: Some("/Child.vue".to_string()),
        }];

        let candidates = collect_dynamic_root_candidates_from_type(
            &TypeExpr::TypeOf(ValueRef {
                path: vec!["Child".to_string()],
                type_args: Vec::new(),
            }),
            imports.as_slice(),
        );

        assert_eq!(
            candidates,
            vec![super::DynamicRootCandidate::ComponentImport {
                component_name: "Child".to_string(),
                import_source: "./Child.vue".to_string(),
                imported_name: Some("default".to_string()),
                binding_kind: Some(
                    crate::resolver_core::symbol_resolver::ImportBindingKind::Default
                ),
            }]
        );
    }

    /// A const value decl whose annotation fact is a closed string-literal
    /// LEAF (the concrete graph-free class the substitution folds).
    fn leaf_annotated_const(
        name: &str,
        literal: &str,
    ) -> verter_semantic::analysis::type_eval::ValueDeclInfo {
        use verter_type_expr::facts::{
            ClosedTypeFact, LeafTypeFact, SemanticTypeSource, ValueAnnotationClass,
            ValueTypeAnnotationFact,
        };
        verter_semantic::analysis::type_eval::ValueDeclInfo {
            name: name.to_string(),
            declaration_id: 0,
            kind: verter_semantic::analysis::type_eval::ValueDeclKind::Const,
            type_annotation: ValueTypeAnnotationFact {
                typeof_alias_target: None,
                classification: ValueAnnotationClass::Direct,
                annotation: Some(SemanticTypeSource::Closed(ClosedTypeFact::Leaf(
                    LeafTypeFact::StringLiteral(literal.to_string()),
                ))),
            },
            signatures: Vec::new(),
            object_shape: None,
            enum_members: None,
            enum_member_names: None,
        }
    }

    #[test]
    fn structural_substitute_typeof_refs_substitutes_length_one_value_refs() {
        let mut env = verter_semantic::analysis::type_eval::EvalEnv::new();
        env.add_value(leaf_annotated_const("as", "input"));

        let lowered = TypeExpr::TypeOf(verter_type_expr::ValueRef {
            path: vec!["as".to_string()],
            type_args: Vec::new(),
        });

        assert_eq!(
            structural_substitute_typeof_refs(&lowered, &env),
            TypeExpr::string_literal("input")
        );
    }

    #[test]
    fn structural_substitute_typeof_refs_preserves_unresolved_refs() {
        let env = verter_semantic::analysis::type_eval::EvalEnv::new();
        let lowered = TypeExpr::TypeOf(verter_type_expr::ValueRef {
            path: vec!["missing".to_string()],
            type_args: Vec::new(),
        });

        assert_eq!(
            structural_substitute_typeof_refs(&lowered, &env),
            lowered,
            "bare refs without an env entry must round-trip unchanged"
        );
    }

    #[test]
    fn structural_substitute_typeof_refs_peels_typeof_alias_target() {
        use verter_type_expr::facts::{
            ValueAnnotationClass, ValueDeclIdentityPart, ValueTypeAnnotationFact,
        };
        let mut env = verter_semantic::analysis::type_eval::EvalEnv::new();
        env.add_value(verter_semantic::analysis::type_eval::ValueDeclInfo {
            name: "alias".to_string(),
            declaration_id: 0,
            kind: verter_semantic::analysis::type_eval::ValueDeclKind::Const,
            type_annotation: ValueTypeAnnotationFact {
                typeof_alias_target: Some(ValueDeclIdentityPart {
                    canonical_id: std::sync::Arc::from("/App.vue"),
                    symbol: std::sync::Arc::from("target"),
                    member_path: std::sync::Arc::from(Vec::<String>::new().into_boxed_slice()),
                }),
                classification: ValueAnnotationClass::TypeOfAlias,
                annotation: None,
            },
            signatures: Vec::new(),
            object_shape: None,
            enum_members: None,
            enum_member_names: None,
        });

        let lowered = TypeExpr::TypeOf(verter_type_expr::ValueRef {
            path: vec!["alias".to_string()],
            type_args: Vec::new(),
        });

        assert_eq!(
            structural_substitute_typeof_refs(&lowered, &env),
            TypeExpr::TypeOf(verter_type_expr::ValueRef {
                path: vec!["target".to_string()],
                type_args: Vec::new(),
            }),
            "a typeof-alias annotation peels one hop to its precomputed target"
        );
    }

    #[test]
    fn structural_substitute_typeof_refs_keeps_non_concrete_annotation_unsubstituted() {
        use verter_type_expr::facts::{
            SemanticTypeSource, ValueAnnotationClass, ValueTypeAnnotationFact,
        };
        use verter_type_expr::locators::{
            AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, TypeBodySlot,
        };
        let mut env = verter_semantic::analysis::type_eval::EvalEnv::new();
        env.add_value(verter_semantic::analysis::type_eval::ValueDeclInfo {
            name: "routes".to_string(),
            declaration_id: 0,
            kind: verter_semantic::analysis::type_eval::ValueDeclKind::Const,
            type_annotation: ValueTypeAnnotationFact {
                typeof_alias_target: None,
                classification: ValueAnnotationClass::Direct,
                annotation: Some(SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(
                    TypeBodySlot {
                        anchor: AuthoredAnchor {
                            canonical_id: std::sync::Arc::from("/routes.ts"),
                            symbol: std::sync::Arc::from("routes"),
                            space: LocatorSymbolSpace::Value,
                        },
                        path: std::sync::Arc::from(Vec::new().into_boxed_slice()),
                    },
                ))),
            },
            signatures: Vec::new(),
            object_shape: None,
            enum_members: None,
            enum_member_names: None,
        });

        let lowered = TypeExpr::TypeOf(verter_type_expr::ValueRef {
            path: vec!["routes".to_string()],
            type_args: Vec::new(),
        });

        assert_eq!(
            structural_substitute_typeof_refs(&lowered, &env),
            lowered,
            "an authored-locator annotation is not graph-free — the typeof \
             stays unsubstituted and routes through the shared dispatch"
        );
    }

    #[test]
    fn structural_substitute_typeof_refs_recurses_into_union_and_intersection() {
        let mut env = verter_semantic::analysis::type_eval::EvalEnv::new();
        env.add_value(leaf_annotated_const("a", "A"));
        env.add_value(leaf_annotated_const("b", "B"));

        let union = TypeExpr::union(vec![
            TypeExpr::TypeOf(verter_type_expr::ValueRef {
                path: vec!["a".to_string()],
                type_args: Vec::new(),
            }),
            TypeExpr::TypeOf(verter_type_expr::ValueRef {
                path: vec!["b".to_string()],
                type_args: Vec::new(),
            }),
        ]);

        assert_eq!(
            structural_substitute_typeof_refs(&union, &env),
            TypeExpr::union(vec![
                TypeExpr::string_literal("A"),
                TypeExpr::string_literal("B"),
            ])
        );
    }

    #[test]
    fn structural_substitute_typeof_refs_leaves_multi_segment_paths_untouched() {
        let mut env = verter_semantic::analysis::type_eval::EvalEnv::new();
        env.add_value(leaf_annotated_const("props", "ignored"));

        let lowered = TypeExpr::TypeOf(verter_type_expr::ValueRef {
            path: vec!["props".to_string(), "name".to_string()],
            type_args: Vec::new(),
        });

        assert_eq!(
            structural_substitute_typeof_refs(&lowered, &env),
            lowered,
            "multi-segment ValueRefs must not swap for the length-1 override binding",
        );
    }

    #[test]
    fn resolve_fallthrough_surface_orchestrates_component_branch_inheritance() {
        let mut host = TestHost::default();
        host.canonical_routes.insert(
            ("/App.vue".to_string(), "./Child.vue".to_string()),
            "/Child.vue".to_string(),
        );
        host.child_resolutions.insert(
            "/Child.vue".to_string(),
            TestResolution {
                accepted_props: vec![AcceptedPropAnalysis {
                    name: "id".to_string(),
                    type_source: verter_type_expr::facts::SourcePosition::Present(SemanticTypeSource::Closed(
                        ClosedTypeFact::Leaf(LeafTypeFact::Primitive(PrimitiveName::String))
                    )),
                    type_source_scope: None,
                    raw_type: Some("string".to_string()),
                    raw_type_source: None,
                    required: false,
                    provenance: MemberProvenance::Declared,
                    availability: MemberAvailability::Always,
                    kind: AcceptedPropKind::DeclaredProp,
                }],
                accepted_events: vec![],
                fallthrough_surface: FallthroughSurface::None {
                    reason: verter_semantic::analysis::component_meta::NoFallthroughReason::InheritAttrsFalse,
                },
                fact_versions: vec![crate::resolver_core::FactVersionRef::FileWholeHash {
                    canonical_id: "/Child.vue".to_string(),
                    hash: [3; 16],
                }],
            },
        );

        let meta = empty_component_meta(RootReachability::Branches {
            branches: vec![RootBranch {
                branch_index: 0,
                condition_text: None,
                target: RootTargetRef::ComponentUsage {
                    element_index: 0,
                    usage_index: 0,
                    name: "Child".to_string(),
                    import_source: Some("./Child.vue".to_string()),
                },
                consumed: ConsumedRootBindings::default(),
                has_unknown_spread: false,
            }],
        });

        let resolved = resolve_fallthrough_surface(
            &host,
            "/App.vue",
            &(),
            &meta,
            None,
            None,
            Vec::new(),
            &mut FxHashSet::default(),
        );

        assert_eq!(resolved.accepted_props.len(), 1);
        assert_eq!(resolved.accepted_props[0].name, "id");
        assert_eq!(resolved.fact_versions.len(), 2);
    }

    /// INVARIANT-16 clone-boundary discriminator: a CHILD-published source
    /// carrying a producer-local (empty) anchor becomes SELF-ANCHORING when
    /// cloned into a parent fallthrough row — the anchor is rewritten to the
    /// CHILD canonical at the clone site, so the parent owner is never used
    /// blindly as the raise scope, and a cross-branch merge of same-shaped
    /// sources from DIFFERENT children compares unequal.
    ///
    /// Discrimination: without the `absolutized_against(child_id)` map on
    /// the clone, the cloned source keeps the empty anchor and this test's
    /// canonical assertion fails.
    #[test]
    fn inherited_component_sources_self_anchor_to_the_child_at_the_clone_boundary() {
        use verter_type_expr::locators::{
            AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, TypeBodySlot,
        };
        let producer_local =
            SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(TypeBodySlot {
                anchor: AuthoredAnchor {
                    canonical_id: Arc::from(""),
                    symbol: Arc::from("ChildLocal"),
                    space: LocatorSymbolSpace::Type,
                },
                path: Arc::from(Vec::new().into_boxed_slice()),
            }));
        let child_props = vec![AcceptedPropAnalysis {
            name: "inherited".to_string(),
            type_source: verter_type_expr::facts::SourcePosition::Present(producer_local),
            type_source_scope: None,
            raw_type: None,
            raw_type_source: None,
            required: false,
            provenance: MemberProvenance::Declared,
            availability: MemberAvailability::Always,
            kind: AcceptedPropKind::DeclaredProp,
        }];
        let declared = FxHashSet::default();
        let rows = inherited_component_props(&child_props, &declared, &[], "/Child.vue");
        assert_eq!(rows.len(), 1);
        let Some(SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(slot))) =
            rows[0].type_source.present()
        else {
            panic!("the cloned row keeps the authored decl-body source arm");
        };
        assert_eq!(
            slot.anchor.canonical_id.as_ref(),
            "/Child.vue",
            "a producer-local (empty) anchor must self-anchor to the CHILD at the \
             clone boundary — the parent scope must never absolutize it later"
        );
        assert_eq!(slot.anchor.symbol.as_ref(), "ChildLocal");
    }

    /// SELF-ANCHORING through the ACTUAL cross-branch merge: TWO children
    /// each publish a producer-local (empty-anchor) source with the SAME
    /// symbol spelling; the clone boundary rewrites each to ITS OWN child
    /// canonical, so the two branch rows compare UNEQUAL (cross-child
    /// inequality) and the flat merged column publishes the honest `None`
    /// instead of collapsing onto one shared source.
    ///
    /// Discrimination: without the `absolutized_against(child_id)` map on
    /// the clone, both rows keep the empty anchor, compare EQUAL, and the
    /// cross-child inequality assertion fails RED.
    #[test]
    fn cross_child_producer_local_sources_stay_unequal_through_the_merge() {
        use verter_type_expr::locators::{
            AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, TypeBodySlot,
        };
        let producer_local = || {
            SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(TypeBodySlot {
                anchor: AuthoredAnchor {
                    canonical_id: Arc::from(""),
                    symbol: Arc::from("ChildLocal"),
                    space: LocatorSymbolSpace::Type,
                },
                path: Arc::from(Vec::new().into_boxed_slice()),
            }))
        };
        let child_props = |source: SemanticTypeSource| {
            vec![AcceptedPropAnalysis {
                name: "inherited".to_string(),
                type_source: verter_type_expr::facts::SourcePosition::Present(source),
                type_source_scope: None,
                raw_type: None,
                raw_type_source: None,
                required: false,
                provenance: MemberProvenance::Declared,
                availability: MemberAvailability::Always,
                kind: AcceptedPropKind::DeclaredProp,
            }]
        };
        let declared = FxHashSet::default();
        let rows_a = inherited_component_props(
            &child_props(producer_local()),
            &declared,
            &[],
            "/ChildA.vue",
        );
        let rows_b = inherited_component_props(
            &child_props(producer_local()),
            &declared,
            &[],
            "/ChildB.vue",
        );
        assert_eq!(rows_a.len(), 1);
        assert_eq!(rows_b.len(), 1);

        // CROSS-CHILD INEQUALITY: the same spelling from two different
        // children names two different declarations once self-anchored.
        assert_ne!(
            rows_a[0].type_source, rows_b[0].type_source,
            "self-anchoring must make same-shaped producer-local sources \
             from DIFFERENT children compare UNEQUAL — without the clone \
             boundary rewrite both keep the empty anchor and collapse"
        );

        // And through the ACTUAL cross-branch merge: the flat column
        // publishes the honest None (no shared collapsed source).
        let branches = vec![
            FallthroughBranch {
                branch_key: "0".to_string(),
                condition_text: None,
                props: rows_a,
                events: Vec::new(),
                root_chain: Vec::new(),
                status: BranchStatus::Resolved,
            },
            FallthroughBranch {
                branch_key: "1".to_string(),
                condition_text: None,
                props: rows_b,
                events: Vec::new(),
                root_chain: Vec::new(),
                status: BranchStatus::Resolved,
            },
        ];
        let mut props = Vec::new();
        let mut events = Vec::new();
        merge_fallthrough_branches(&mut props, &mut events, &branches, false, false);
        let merged = props
            .iter()
            .find(|p| p.name == "inherited")
            .expect("merged row");
        assert_eq!(
            merged.type_source,
            verter_type_expr::facts::SourcePosition::Absent(
                verter_type_expr::facts::SchemaAbsence::BranchDivergent
            ),
            "two different children's self-anchored sources are distinct \
             declarations — the flat column must not collapse onto one"
        );
    }

    // ── FIX-2: absorbing cross-branch source merge (Unset ≠ Conflict) ──

    fn resolved_branch(
        key: &str,
        props: Vec<verter_semantic::analysis::component_meta::FallthroughPropEntry>,
        events: Vec<verter_semantic::analysis::component_meta::FallthroughEventEntry>,
    ) -> FallthroughBranch {
        FallthroughBranch {
            branch_key: key.to_string(),
            condition_text: None,
            props,
            events,
            root_chain: Vec::new(),
            status: BranchStatus::Resolved,
        }
    }

    fn branch_prop(
        name: &str,
        source: Option<SemanticTypeSource>,
        origin: &str,
    ) -> verter_semantic::analysis::component_meta::FallthroughPropEntry {
        verter_semantic::analysis::component_meta::FallthroughPropEntry {
            name: name.to_string(),
            type_source: source
                .map(SourcePosition::Present)
                .unwrap_or_else(SourcePosition::unannotated),
            // Mirror the clone boundary: the producing scope is the origin
            // child the entry was inherited from.
            type_source_scope: Some(origin.to_string()),
            raw_type: None,
            sources: vec![InheritedSource::Component {
                canonical_id: origin.to_string(),
            }],
        }
    }

    fn branch_event(
        name: &str,
        payload: Option<SemanticTypeSource>,
        origin: &str,
    ) -> verter_semantic::analysis::component_meta::FallthroughEventEntry {
        verter_semantic::analysis::component_meta::FallthroughEventEntry {
            name: name.to_string(),
            payload: payload
                .map(SourcePosition::Present)
                .unwrap_or_else(SourcePosition::unannotated),
            // Mirror the clone boundary: the producing scope is the origin
            // child the entry was inherited from.
            payload_scope: Some(origin.to_string()),
            raw_signature: None,
            sources: vec![InheritedSource::Component {
                canonical_id: origin.to_string(),
            }],
        }
    }

    fn closed_primitive(kind: PrimitiveName) -> SemanticTypeSource {
        SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(kind)))
    }

    fn closed_ref(name: &str) -> SemanticTypeSource {
        SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Ref(name.to_string())))
    }

    fn anchored_decl_body(canonical: &str, symbol: &str) -> SemanticTypeSource {
        use verter_type_expr::locators::{
            AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, TypeBodySlot,
        };
        SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(TypeBodySlot {
            anchor: AuthoredAnchor {
                canonical_id: Arc::from(canonical),
                symbol: Arc::from(symbol),
                space: LocatorSymbolSpace::Type,
            },
            path: Arc::from(Vec::new().into_boxed_slice()),
        }))
    }

    /// FIX-2 regression (props): with THREE branches whose sources disagree
    /// (A: `string` → B: `number` → C: `boolean`), the flat accepted column
    /// publishes the honest `None` — NEVER a later branch's type. The former
    /// `Option` accumulator conflated "conflict" (`None` after A/B disagree)
    /// with "unset", so branch C's `boolean` REVIVED the flat value.
    #[test]
    fn three_branch_prop_disagreement_finalizes_flat_source_to_honest_unknown() {
        let branches = vec![
            resolved_branch(
                "0",
                vec![branch_prop(
                    "size",
                    Some(closed_primitive(PrimitiveName::String)),
                    "/A.vue",
                )],
                Vec::new(),
            ),
            resolved_branch(
                "1",
                vec![branch_prop(
                    "size",
                    Some(closed_primitive(PrimitiveName::Number)),
                    "/B.vue",
                )],
                Vec::new(),
            ),
            resolved_branch(
                "2",
                vec![branch_prop(
                    "size",
                    Some(closed_primitive(PrimitiveName::Boolean)),
                    "/C.vue",
                )],
                Vec::new(),
            ),
        ];
        let mut props = Vec::new();
        let mut events = Vec::new();
        merge_fallthrough_branches(&mut props, &mut events, &branches, false, false);
        let merged = props.iter().find(|p| p.name == "size").expect("merged row");
        assert_eq!(
            merged.type_source,
            verter_type_expr::facts::SourcePosition::Absent(
                verter_type_expr::facts::SchemaAbsence::BranchDivergent
            ),
            "conflict is ABSORBING: after A/B disagreed, branch C must not \
             revive a flat accepted type — the honest flat answer is the \
             proven branch-divergent absence"
        );
    }

    /// FIX-2 regression (events): the identical absorbing-conflict rule
    /// applies to the accepted-event payload column.
    #[test]
    fn three_branch_event_disagreement_finalizes_flat_payload_to_honest_unknown() {
        let branches = vec![
            resolved_branch(
                "0",
                Vec::new(),
                vec![branch_event(
                    "change",
                    Some(closed_primitive(PrimitiveName::String)),
                    "/A.vue",
                )],
            ),
            resolved_branch(
                "1",
                Vec::new(),
                vec![branch_event(
                    "change",
                    Some(closed_primitive(PrimitiveName::Number)),
                    "/B.vue",
                )],
            ),
            resolved_branch(
                "2",
                Vec::new(),
                vec![branch_event(
                    "change",
                    Some(closed_primitive(PrimitiveName::Boolean)),
                    "/C.vue",
                )],
            ),
        ];
        let mut props = Vec::new();
        let mut events = Vec::new();
        merge_fallthrough_branches(&mut props, &mut events, &branches, false, false);
        let merged = events
            .iter()
            .find(|e| e.name == "change")
            .expect("merged row");
        assert_eq!(
            merged.payload,
            verter_type_expr::facts::SourcePosition::Absent(
                verter_type_expr::facts::SchemaAbsence::BranchDivergent
            ),
            "conflict is ABSORBING on the event payload column too — a later \
             branch never revives a flat accepted payload"
        );
    }

    /// FIX-2 regression (scope-aware identity): two children publishing the
    /// SAME closed-`Ref` SPELLING from DIFFERENT declaration origins are NOT
    /// in agreement — the spelling names two different declarations, so the
    /// flat column publishes the honest `None` instead of collapsing onto
    /// one shared source.
    #[test]
    fn same_spelling_closed_ref_from_different_child_origins_does_not_collapse() {
        let branches = vec![
            resolved_branch(
                "0",
                vec![branch_prop(
                    "inherited",
                    Some(closed_ref("Alias")),
                    "/ChildA.vue",
                )],
                Vec::new(),
            ),
            resolved_branch(
                "1",
                vec![branch_prop(
                    "inherited",
                    Some(closed_ref("Alias")),
                    "/ChildB.vue",
                )],
                Vec::new(),
            ),
        ];
        let mut props = Vec::new();
        let mut events = Vec::new();
        merge_fallthrough_branches(&mut props, &mut events, &branches, false, false);
        let merged = props
            .iter()
            .find(|p| p.name == "inherited")
            .expect("merged row");
        assert_eq!(
            merged.type_source,
            verter_type_expr::facts::SourcePosition::Absent(
                verter_type_expr::facts::SchemaAbsence::BranchDivergent
            ),
            "a scope-relative source's identity includes its origin scope: \
             the same `Ref(\"Alias\")` spelling from two different children \
             must NOT collapse onto one shared flat source"
        );
    }

    /// FIX-2 control: genuine agreement still keeps the flat source — the
    /// same child contributing the same scope-relative spelling in every
    /// branch, AND two different children contributing the same fully
    /// ANCHORED source (the anchor pins one declaration regardless of
    /// publisher).
    /// FIX-3 regression (scope threading, merge half): the agreed
    /// PRODUCING scope survives `MergedSourceState::finalize` onto the flat
    /// merged row (`type_source_scope` / `payload_scope`), so the output
    /// boundary can raise the whole inherited source — nested
    /// scope-relative refs included — under the producing scope instead of
    /// blindly under the parent owner.
    ///
    /// Discriminating: with `finalize` dropping the scope (the pre-fix
    /// shape), the merged rows carry `None` and both asserts fail RED.
    #[test]
    fn merged_flat_row_carries_the_producing_scope() {
        let branches = vec![
            resolved_branch(
                "0",
                vec![branch_prop(
                    "inherited",
                    Some(closed_ref("Alias")),
                    "/Child.vue",
                )],
                vec![branch_event(
                    "change",
                    Some(closed_ref("PayloadAlias")),
                    "/Child.vue",
                )],
            ),
            resolved_branch(
                "1",
                vec![branch_prop(
                    "inherited",
                    Some(closed_ref("Alias")),
                    "/Child.vue",
                )],
                vec![branch_event(
                    "change",
                    Some(closed_ref("PayloadAlias")),
                    "/Child.vue",
                )],
            ),
        ];
        let mut props = Vec::new();
        let mut events = Vec::new();
        merge_fallthrough_branches(&mut props, &mut events, &branches, false, false);
        let merged = props
            .iter()
            .find(|p| p.name == "inherited")
            .expect("merged prop row");
        assert_eq!(
            merged.type_source_scope.as_deref(),
            Some("/Child.vue"),
            "the agreed producing scope must survive finalization onto the \
             flat prop row — it is the row's raise scope at the output \
             boundary"
        );
        let merged_event = events
            .iter()
            .find(|e| e.name == "change")
            .expect("merged event row");
        assert_eq!(
            merged_event.payload_scope.as_deref(),
            Some("/Child.vue"),
            "the agreed producing scope must survive finalization onto the \
             flat event row"
        );
    }

    /// FIX-3 regression (scope threading, clone half): the owner-clone
    /// boundary threads the TERMINAL origin scope across hops — a child
    /// accepted row that was ITSELF inherited (its `type_source_scope`
    /// already names the grandchild) keeps the grandchild as the producing
    /// scope; only a child-declared row (scope `None`) anchors to the
    /// direct child.
    ///
    /// Discriminating: with the clone fabricating the direct child
    /// unconditionally, the multi-hop assert fails RED.
    #[test]
    fn clone_boundary_threads_the_terminal_origin_scope_across_hops() {
        let child_rows = vec![
            AcceptedPropAnalysis {
                name: "fromGrandchild".to_string(),
                type_source: verter_type_expr::facts::SourcePosition::Present(closed_ref(
                    "GrandchildAlias",
                )),
                // The child's row was itself inherited: its producing
                // scope is the grandchild.
                type_source_scope: Some("/Grandchild.vue".to_string()),
                raw_type: None,
                raw_type_source: None,
                required: false,
                provenance: MemberProvenance::Inherited {
                    sources: vec![InheritedSource::Component {
                        canonical_id: "/Grandchild.vue".to_string(),
                    }],
                },
                availability: MemberAvailability::Always,
                kind: AcceptedPropKind::Attr,
            },
            AcceptedPropAnalysis {
                name: "fromChild".to_string(),
                type_source: verter_type_expr::facts::SourcePosition::Present(closed_ref(
                    "ChildAlias",
                )),
                // Child-declared: spelled in the child's own file.
                type_source_scope: None,
                raw_type: None,
                raw_type_source: None,
                required: false,
                provenance: MemberProvenance::Declared,
                availability: MemberAvailability::Always,
                kind: AcceptedPropKind::DeclaredProp,
            },
        ];
        let declared = FxHashSet::default();
        let rows = inherited_component_props(&child_rows, &declared, &[], "/Child.vue");
        let by_name = |name: &str| {
            rows.iter()
                .find(|row| row.name == name)
                .unwrap_or_else(|| panic!("cloned row `{name}`"))
        };
        assert_eq!(
            by_name("fromGrandchild").type_source_scope.as_deref(),
            Some("/Grandchild.vue"),
            "a multi-hop inherited row keeps its TERMINAL origin scope — \
             the direct child is not the file its source is spelled in"
        );
        assert_eq!(
            by_name("fromChild").type_source_scope.as_deref(),
            Some("/Child.vue"),
            "a child-declared row anchors to the direct child (the file the \
             source is spelled in)"
        );
    }

    /// PRODUCER-SCOPE HYGIENE at the clone boundary: an inherited row
    /// carries a positional producing scope ONLY while its (absolutized)
    /// source is SCOPE-RELATIVE — the scope exists to resolve bare `Ref`
    /// spellings, nothing else. A source-less, fully-CLOSED, or
    /// fully-ANCHORED row must publish `None`: attaching an irrelevant
    /// producer scope adds a spurious discriminator to the
    /// output-materialization memo key, so identical closed sources
    /// inherited from DIFFERENT children could never share a warm entry.
    ///
    /// Discriminating: with the clone allocating the producer scope
    /// unconditionally, every `None` assert below fails RED.
    #[test]
    fn producer_scope_attaches_only_to_scope_relative_inherited_sources() {
        use verter_semantic::analysis::component_meta::AcceptedEventKind;

        let prop_row = |name: &str,
                        source: Option<SemanticTypeSource>,
                        row_scope: Option<&str>|
         -> AcceptedPropAnalysis {
            AcceptedPropAnalysis {
                name: name.to_string(),
                type_source: source
                    .map(SourcePosition::Present)
                    .unwrap_or_else(SourcePosition::unannotated),
                type_source_scope: row_scope.map(str::to_string),
                raw_type: None,
                raw_type_source: None,
                required: false,
                provenance: MemberProvenance::Declared,
                availability: MemberAvailability::Always,
                kind: AcceptedPropKind::DeclaredProp,
            }
        };
        let child_props = vec![
            // (1) Source-less row: no source ⇒ no producer scope.
            prop_row("sourceless", None, None),
            // (2) Fully-CLOSED source (a primitive leaf) ⇒ no producer scope.
            prop_row(
                "closed",
                Some(closed_primitive(PrimitiveName::String)),
                None,
            ),
            // (3) Fully-ANCHORED authored source ⇒ no producer scope (the
            // anchor self-describes; no bare Ref spelling to resolve).
            prop_row(
                "anchored",
                Some(anchored_decl_body("/lib/types.ts", "LibType")),
                None,
            ),
            // (4) DIRECT scope-relative ref (bare `Ref` leaf spelled in the
            // child's own file) ⇒ the direct child scope.
            prop_row("directRelative", Some(closed_ref("ChildAlias")), None),
            // (5) MULTI-HOP scope-relative ref (the child's row was itself
            // inherited) ⇒ the terminal origin scope survives the hop.
            prop_row(
                "multiHopRelative",
                Some(closed_ref("GrandchildAlias")),
                Some("/Grandchild.vue"),
            ),
        ];
        let declared = FxHashSet::default();
        let rows = inherited_component_props(&child_props, &declared, &[], "/Child.vue");
        let by_name = |name: &str| {
            rows.iter()
                .find(|row| row.name == name)
                .unwrap_or_else(|| panic!("cloned row `{name}`"))
        };
        assert_eq!(
            by_name("sourceless").type_source_scope,
            None,
            "a source-less inherited row must carry NO producer scope"
        );
        assert_eq!(
            by_name("closed").type_source_scope,
            None,
            "a fully-CLOSED inherited source needs no raise scope — an \
             irrelevant producer scope fragments the output memo"
        );
        assert_eq!(
            by_name("anchored").type_source_scope,
            None,
            "a fully-ANCHORED inherited source self-describes — an \
             irrelevant producer scope fragments the output memo"
        );
        assert_eq!(
            by_name("directRelative").type_source_scope.as_deref(),
            Some("/Child.vue"),
            "a scope-relative source keeps the direct child as its raise scope"
        );
        assert_eq!(
            by_name("multiHopRelative").type_source_scope.as_deref(),
            Some("/Grandchild.vue"),
            "a multi-hop scope-relative source keeps its TERMINAL origin scope"
        );

        // The EVENT clone boundary applies the identical rule.
        let event_row = |name: &str,
                         payload: Option<SemanticTypeSource>,
                         row_scope: Option<&str>|
         -> AcceptedEventAnalysis {
            AcceptedEventAnalysis {
                name: name.to_string(),
                payload: payload
                    .map(SourcePosition::Present)
                    .unwrap_or_else(SourcePosition::unannotated),
                payload_scope: row_scope.map(str::to_string),
                raw_signature: None,
                provenance: MemberProvenance::Declared,
                availability: MemberAvailability::Always,
                kind: AcceptedEventKind::DeclaredEmit,
            }
        };
        let child_events = vec![
            event_row(
                "closedEvent",
                Some(closed_primitive(PrimitiveName::Number)),
                None,
            ),
            event_row("relativeEvent", Some(closed_ref("PayloadAlias")), None),
        ];
        let event_rows = inherited_component_events(
            &child_events,
            &FxHashSet::default(),
            &FxHashSet::default(),
            &[],
            "/Child.vue",
        );
        let event_by_name = |name: &str| {
            event_rows
                .iter()
                .find(|row| row.name == name)
                .unwrap_or_else(|| panic!("cloned event row `{name}`"))
        };
        assert_eq!(
            event_by_name("closedEvent").payload_scope,
            None,
            "a fully-CLOSED inherited event payload needs no raise scope"
        );
        assert_eq!(
            event_by_name("relativeEvent").payload_scope.as_deref(),
            Some("/Child.vue"),
            "a scope-relative event payload keeps the direct child scope"
        );

        // A PRODUCER-LOCAL authored source is absolutized BEFORE the scope
        // decision (the clone boundary self-anchors it to the child), so it
        // arrives fully anchored and needs no producer scope either.
        let producer_local = {
            use verter_type_expr::locators::{
                AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, TypeBodySlot,
            };
            SemanticTypeSource::Authored(AuthoredBodyLocator::DeclBody(TypeBodySlot {
                anchor: AuthoredAnchor {
                    canonical_id: Arc::from(""),
                    symbol: Arc::from("ChildLocal"),
                    space: LocatorSymbolSpace::Type,
                },
                path: Arc::from(Vec::new().into_boxed_slice()),
            }))
        };
        let local_rows = inherited_component_props(
            &[prop_row("localAuthored", Some(producer_local), None)],
            &declared,
            &[],
            "/Child.vue",
        );
        assert_eq!(
            local_rows[0].type_source_scope, None,
            "the scope decision runs on the ABSOLUTIZED source — a \
             producer-local anchor self-anchors to the child first, so no \
             positional producer scope is needed"
        );
    }

    /// MEMO-SHARING consequence of producer-scope hygiene: two inherited
    /// rows from DIFFERENT children carrying the IDENTICAL fully-closed
    /// source produce entries with equal `(source, scope)` memo identity —
    /// the output-materialization memo key — so the envelope materializes
    /// the source ONCE. With the pre-fix unconditional scope allocation the
    /// two entries key `(/ChildA.vue, S)` / `(/ChildB.vue, S)` and can
    /// never share a warm entry.
    #[test]
    fn identical_closed_sources_from_different_children_share_memo_identity() {
        let child_props = |name: &str| {
            vec![AcceptedPropAnalysis {
                name: name.to_string(),
                type_source: verter_type_expr::facts::SourcePosition::Present(closed_primitive(
                    PrimitiveName::String,
                )),
                type_source_scope: None,
                raw_type: None,
                raw_type_source: None,
                required: false,
                provenance: MemberProvenance::Declared,
                availability: MemberAvailability::Always,
                kind: AcceptedPropKind::DeclaredProp,
            }]
        };
        let declared = FxHashSet::default();
        let rows_a =
            inherited_component_props(&child_props("shared"), &declared, &[], "/ChildA.vue");
        let rows_b =
            inherited_component_props(&child_props("shared"), &declared, &[], "/ChildB.vue");
        assert_eq!(
            (
                rows_a[0].type_source.present(),
                rows_a[0].type_source_scope.as_deref()
            ),
            (
                rows_b[0].type_source.present(),
                rows_b[0].type_source_scope.as_deref()
            ),
            "identical fully-closed sources inherited from two different \
             children must carry the identical (source, scope) memo \
             identity — the now-dropped producer scope was the only \
             discriminator keeping them apart"
        );
    }

    #[test]
    fn genuine_cross_branch_agreement_keeps_the_flat_source() {
        // (a) Same origin, same scope-relative spelling → agreement.
        let branches = vec![
            resolved_branch(
                "0",
                vec![branch_prop(
                    "inherited",
                    Some(closed_ref("Alias")),
                    "/Child.vue",
                )],
                Vec::new(),
            ),
            resolved_branch(
                "1",
                vec![branch_prop(
                    "inherited",
                    Some(closed_ref("Alias")),
                    "/Child.vue",
                )],
                Vec::new(),
            ),
        ];
        let mut props = Vec::new();
        let mut events = Vec::new();
        merge_fallthrough_branches(&mut props, &mut events, &branches, false, false);
        assert_eq!(
            props
                .iter()
                .find(|p| p.name == "inherited")
                .expect("merged row")
                .type_source,
            verter_type_expr::facts::SourcePosition::Present(closed_ref("Alias")),
            "the same child's spelling agrees across branches"
        );

        // (b) Different origins, same fully-ANCHORED source → agreement.
        let anchored = anchored_decl_body("/shared.ts", "Shared");
        let branches = vec![
            resolved_branch(
                "0",
                vec![branch_prop("shared", Some(anchored.clone()), "/ChildA.vue")],
                Vec::new(),
            ),
            resolved_branch(
                "1",
                vec![branch_prop("shared", Some(anchored.clone()), "/ChildB.vue")],
                Vec::new(),
            ),
        ];
        let mut props = Vec::new();
        let mut events = Vec::new();
        merge_fallthrough_branches(&mut props, &mut events, &branches, false, false);
        assert_eq!(
            props
                .iter()
                .find(|p| p.name == "shared")
                .expect("merged row")
                .type_source,
            verter_type_expr::facts::SourcePosition::Present(anchored),
            "a fully-anchored source names ONE declaration — different \
             publishers still agree"
        );

        // (c) A typed side plus an untyped side adopts the typed side.
        let branches = vec![
            resolved_branch(
                "0",
                vec![branch_prop(
                    "oneSided",
                    Some(closed_primitive(PrimitiveName::String)),
                    "/ChildA.vue",
                )],
                Vec::new(),
            ),
            resolved_branch(
                "1",
                vec![branch_prop("oneSided", None, "/ChildB.vue")],
                Vec::new(),
            ),
        ];
        let mut props = Vec::new();
        let mut events = Vec::new();
        merge_fallthrough_branches(&mut props, &mut events, &branches, false, false);
        assert_eq!(
            props
                .iter()
                .find(|p| p.name == "oneSided")
                .expect("merged row")
                .type_source,
            verter_type_expr::facts::SourcePosition::Present(closed_primitive(
                PrimitiveName::String
            )),
            "an untyped branch side neither conflicts nor revives — the \
             one-sided merge adopts the typed side"
        );
    }
}
