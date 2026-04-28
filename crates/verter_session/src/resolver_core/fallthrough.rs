use rustc_hash::{FxHashMap, FxHashSet};
use std::hash::{Hash, Hasher};
use verter_semantic::analysis::component_meta::{
    AcceptedEventAnalysis, AcceptedEventKind, AcceptedPropAnalysis, AcceptedPropKind,
    AcceptedSurfaceCompleteness, BranchStatus, ComponentMetaAnalysis, ConsumedRootBindings,
    FallthroughBranch, FallthroughEventEntry, FallthroughPropEntry, FallthroughSurface,
    InheritedSource, MemberAvailability, MemberProvenance, PartialBranchReason, ResolvedRootStep,
    RootReachability, RootTargetRef, UnresolvedBranchReason,
};
use verter_semantic::analysis::html_intrinsics::{IntrinsicMemberKind, OwnedIntrinsicMember};
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::types::AnalyzedImport;

use crate::resolver_core::{FactVersionRef, FallthroughNodeKey, FallthroughNodeKind};

pub trait FallthroughResolutionView {
    fn accepted_props(&self) -> &[AcceptedPropAnalysis];
    fn accepted_events(&self) -> &[AcceptedEventAnalysis];
    fn fallthrough_surface(&self) -> &FallthroughSurface;
    fn fact_versions(&self) -> &[FactVersionRef];
}

pub trait FallthroughResolverHost {
    type ChildResolution: FallthroughResolutionView;

    fn intrinsic_members_for_tag(&self, canonical_id: &str, tag: &str)
        -> Vec<OwnedIntrinsicMember>;
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
        prop_type_overrides: Option<&FxHashMap<String, TypeExpr>>,
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
    ) -> ResolvedConsumedBindings;

    fn build_generic_child_prop_overrides(
        &self,
        canonical_id: &str,
        snapshot: &Self::Snapshot,
        usage_index: u32,
        eval_env: &mut Option<Self::EvalEnv>,
    ) -> Option<FxHashMap<String, TypeExpr>>;

    fn resolve_dynamic_root_candidates(
        &self,
        canonical_id: &str,
        snapshot: &Self::Snapshot,
        usage_index: u32,
        eval_env: &mut Option<Self::EvalEnv>,
    ) -> Vec<DynamicRootCandidate>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnownSpreadKeys {
    pub attrs: std::collections::BTreeSet<String>,
    pub listeners: std::collections::BTreeSet<String>,
    pub exact: bool,
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
    prop_type_overrides: Option<&FxHashMap<String, TypeExpr>>,
) -> FallthroughNodeKey {
    FallthroughNodeKey {
        canonical_component_id: canonical_id.to_string(),
        node_kind: FallthroughNodeKind::BranchUnionMerge,
        override_fingerprint: prop_type_overrides
            .map(hash_prop_type_overrides)
            .unwrap_or_default(),
        behavior_flags: u32::from(generic_root_propagation),
        branch_selector: None,
    }
}

pub fn hash_prop_type_overrides(overrides: &FxHashMap<String, TypeExpr>) -> u64 {
    let mut pairs: Vec<_> = overrides.iter().collect();
    pairs.sort_by(|a, b| a.0.cmp(b.0));

    let mut hasher = rustc_hash::FxHasher::default();
    for (name, ty) in pairs {
        name.hash(&mut hasher);
        ty.hash(&mut hasher);
    }
    hasher.finish()
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
                inherited_props.push(FallthroughPropEntry {
                    name: member.name.clone(),
                    type_expr: member.type_expr.clone(),
                    raw_type: None,
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
                inherited_events.push(FallthroughEventEntry {
                    name: member.name.clone(),
                    payload: member.type_expr.clone(),
                    raw_signature: None,
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
    child_prop_overrides: Option<&FxHashMap<String, TypeExpr>>,
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

    if !child_id.ends_with(".vue") {
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

    let mut inherited_prop_map: FxHashMap<String, (AcceptedPropAnalysis, Vec<String>)> =
        FxHashMap::default();
    let mut inherited_event_map: FxHashMap<String, (AcceptedEventAnalysis, Vec<String>)> =
        FxHashMap::default();

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
                            type_expr: prop.type_expr.clone(),
                            raw_type: prop.raw_type.clone(),
                            required: false,
                            provenance: MemberProvenance::Inherited {
                                sources: prop.sources.clone(),
                            },
                            availability: MemberAvailability::Always,
                            kind: AcceptedPropKind::Attr,
                        },
                        Vec::new(),
                    )
                });
            merge_type_expr(&mut entry.0.type_expr, &prop.type_expr);
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
                            payload: event.payload.clone(),
                            raw_signature: event.raw_signature.clone(),
                            provenance: MemberProvenance::Inherited {
                                sources: event.sources.clone(),
                            },
                            availability: MemberAvailability::Always,
                            kind: AcceptedEventKind::Listener,
                        },
                        Vec::new(),
                    )
                });
            merge_type_expr(&mut entry.0.payload, &event.payload);
            if entry.0.raw_signature != event.raw_signature {
                entry.0.raw_signature = None;
            }
            if let MemberProvenance::Inherited { sources } = &mut entry.0.provenance {
                merge_inherited_sources(sources, &event.sources);
            }
            entry.1.push(branch.branch_key.clone());
        }
    }

    for (_, (prop, branch_keys)) in inherited_prop_map.iter_mut() {
        branch_keys.sort();
        branch_keys.dedup();
        if force_conditional || branch_keys.len() < total_branches {
            prop.availability = MemberAvailability::Conditional {
                branch_keys: branch_keys.clone(),
            };
        }
    }
    for (_, (event, branch_keys)) in inherited_event_map.iter_mut() {
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
        .map(|(prop, _)| prop)
        .collect();
    inherited_props.sort_by(|a, b| a.name.cmp(&b.name));
    accepted_props.extend(inherited_props);

    let mut inherited_events: Vec<AcceptedEventAnalysis> = inherited_event_map
        .into_values()
        .map(|(event, _)| event)
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
    _prop_type_overrides: Option<&FxHashMap<String, TypeExpr>>,
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
            type_expr: prop.type_expr.clone(),
            raw_type: prop.raw_type.clone(),
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
                        );
                        let candidates = host.resolve_dynamic_root_candidates(
                            canonical_id,
                            snapshot,
                            *usage_index,
                            &mut eval_env,
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

pub fn inject_prop_type_overrides(
    env: &mut verter_semantic::analysis::type_eval::EvalEnv,
    overrides: &FxHashMap<String, TypeExpr>,
) {
    for (name, ty) in overrides {
        env.add_value(verter_semantic::analysis::type_eval::ValueDeclInfo {
            name: name.clone(),
            declaration_id: 0,
            kind: verter_semantic::analysis::type_eval::ValueDeclKind::Const,
            type_annotation: Some(ty.clone()),
            function_signature: None,
            object_shape: None,
        });
    }
}

/// Structural substitution of bare `TypeOf(ValueRef)` references with
/// annotations from a standalone evaluation environment.
///
/// D-Cutover §5.8 WIP-W migration path: the session-side callers of
/// `evaluate_value_expression` now route value references through this
/// env-first substitution + `ComponentMetaQueryEngine` dispatch fallback
/// pair. This helper handles the injected-override hot path
/// (`inject_prop_type_overrides` writes length-1 value symbols that the
/// previous solver would resolve via `EvalEnvSolverHost`). Dispatch, not
/// env substitution, handles imported/declared types.
pub fn structural_substitute_typeof_refs(
    expr: &TypeExpr,
    env: &verter_semantic::analysis::type_eval::EvalEnv,
) -> TypeExpr {
    match expr {
        TypeExpr::TypeOf(value_ref) if value_ref.path.len() == 1 => env
            .value_symbols
            .get(value_ref.path[0].as_str())
            .and_then(|decl| decl.type_annotation.clone())
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

/// Evaluate a value expression by parsing it and resolving identifiers
/// via env-based substitution followed by component-meta dispatch.
///
/// Mirrors the pre-migration `evaluate_value_expression` contract:
/// env-level substitutions (including injected prop-type overrides) take
/// precedence; otherwise the lowered expression is routed through the
/// Class A dispatch helper in the owning canonical scope. Phase 5e
/// commit 5 migrated the engine's
/// `project_expr_surface_expr` / `lower_and_project_to_expanded`
/// callsites to `project_expr_class_a_via_dispatch_threaded`, which
/// covers BOTH the registry-route fast-path AND the generic
/// `ProjectPath { [], Expanded }` dispatch — collapsing the two
/// previous fallback layers (which both terminated at the same dispatch
/// query under the Phase 5c trampolines).
pub fn evaluate_value_expression_via_env_or_dispatch(
    expression: &str,
    canonical_id: &str,
    env: Option<&verter_semantic::analysis::type_eval::EvalEnv>,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> Option<TypeExpr> {
    let lowered =
        verter_semantic::analysis::type_eval_build::parse_value_expression_type(expression)?;
    if let Some(env) = env {
        let substituted = structural_substitute_typeof_refs(&lowered, env);
        if substituted != lowered {
            return Some(substituted);
        }
    }
    crate::meta_resolve::project_expr_class_a_via_dispatch_threaded(
        engine.host,
        Some(engine),
        canonical_id,
        &lowered,
    )
}

pub fn resolve_usage_prop_type<F>(
    prop: &verter_semantic::analysis::template::TemplatePropUsage,
    mut evaluator: F,
) -> Option<TypeExpr>
where
    F: FnMut(&str) -> Option<TypeExpr>,
{
    if prop.from_spread {
        return None;
    }

    if !prop.is_bound {
        return match &prop.expression {
            Some(expression) => Some(TypeExpr::string_literal(expression.clone())),
            None => Some(TypeExpr::boolean_literal(true)),
        };
    }

    if let Some(expression) = &prop.expression {
        if let Some(ty) = evaluator(expression) {
            return Some(ty);
        }

        if let Some(ty) =
            verter_semantic::analysis::type_eval_build::parse_value_expression_type(expression)
        {
            return Some(ty);
        }
    }

    if prop.is_shorthand {
        if let Some(ty) = evaluator(&prop.name) {
            return Some(ty);
        }

        if let Some(ty) =
            verter_semantic::analysis::type_eval_build::parse_value_expression_type(&prop.name)
        {
            return Some(ty);
        }
    }

    None
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
    use verter_semantic::analysis::type_expr::{LiteralValue, TypeExpr};

    match ty {
        TypeExpr::Literal(LiteralValue::String(tag)) => {
            vec![DynamicRootCandidate::NativeTag { tag: tag.clone() }]
        }
        TypeExpr::Union(types) => types
            .iter()
            .flat_map(|branch| collect_dynamic_root_candidates_from_type(branch, imports))
            .collect(),
        TypeExpr::Parenthesized(inner) => collect_dynamic_root_candidates_from_type(inner, imports),
        TypeExpr::TypeOf(value_ref) if value_ref.path.len() == 1 => imports
            .iter()
            .filter(|import| !import.is_type_only)
            .find_map(|import| {
                import
                    .bindings
                    .iter()
                    .find(|binding| !binding.is_type_only && binding.name == value_ref.path[0])
                    .map(|binding| DynamicRootCandidate::ComponentImport {
                        component_name: value_ref.path[0].clone(),
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
            .into_iter()
            .collect(),
        _ => Vec::new(),
    }
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
        .map(|prop| FallthroughPropEntry {
            name: prop.name.clone(),
            type_expr: prop.type_expr.clone(),
            raw_type: prop.raw_type.clone(),
            sources: vec![InheritedSource::Component {
                canonical_id: child_id.to_string(),
            }],
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
        .map(|event| FallthroughEventEntry {
            name: event.name.clone(),
            payload: event.payload.clone(),
            raw_signature: event.raw_signature.clone(),
            sources: vec![InheritedSource::Component {
                canonical_id: child_id.to_string(),
            }],
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
        .map(|prop| FallthroughPropEntry {
            name: prop.name.clone(),
            type_expr: prop.type_expr.clone(),
            raw_type: prop.raw_type.clone(),
            sources: vec![InheritedSource::Component {
                canonical_id: child_id.to_string(),
            }],
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
        .map(|event| FallthroughEventEntry {
            name: event.name.clone(),
            payload: event.payload.clone(),
            raw_signature: event.raw_signature.clone(),
            sources: vec![InheritedSource::Component {
                canonical_id: child_id.to_string(),
            }],
        })
        .collect()
}

fn merge_type_expr(existing: &mut TypeExpr, incoming: &TypeExpr) {
    if existing == incoming {
        return;
    }

    match existing {
        TypeExpr::Union(types) => {
            if !types.iter().any(|ty| ty == incoming) {
                let mut vec: Vec<TypeExpr> = types.iter().cloned().collect();
                vec.push(incoming.clone());
                *existing = TypeExpr::union(vec);
            }
        }
        _ => {
            *existing = TypeExpr::union(vec![existing.clone(), incoming.clone()]);
        }
    }
}

fn merge_inherited_sources(existing: &mut Vec<InheritedSource>, incoming: &[InheritedSource]) {
    existing.extend(incoming.iter().cloned());
    existing.sort();
    existing.dedup();
}

fn normalize_public_spread_key(
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

fn known_spread_keys_from_object(
    object: &verter_semantic::analysis::type_expr::ObjectExpr,
) -> KnownSpreadKeys {
    let mut result = KnownSpreadKeys {
        exact: true,
        ..KnownSpreadKeys::default()
    };

    for member in &object.properties {
        match member {
            verter_semantic::analysis::type_expr::ObjectMember::Property(prop) => {
                normalize_public_spread_key(&prop.name, &mut result.attrs, &mut result.listeners)
            }
            verter_semantic::analysis::type_expr::ObjectMember::Method(method) => {
                normalize_public_spread_key(&method.name, &mut result.attrs, &mut result.listeners)
            }
            verter_semantic::analysis::type_expr::ObjectMember::IndexSignature(_)
            | verter_semantic::analysis::type_expr::ObjectMember::CallSignature(_)
            | verter_semantic::analysis::type_expr::ObjectMember::ConstructSignature(_) => {
                result.exact = false;
            }
        }
    }

    result
}

fn intersect_known_spread_keys(
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
        collect_dynamic_root_candidates_from_type, fallthrough_cache_key, hash_prop_type_overrides,
        inject_prop_type_overrides, known_spread_keys_from_type_expr, merge_fallthrough_branches,
        resolve_fallthrough_surface, resolve_usage_prop_type, structural_substitute_typeof_refs,
        DynamicRootCandidate, FallthroughComputeHost, FallthroughResolutionView,
        FallthroughResolverHost, ResolvedConsumedBindings,
    };
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::sync::Arc;
    use verter_semantic::analysis::component_meta::{
        AcceptedEventAnalysis, AcceptedPropAnalysis, AcceptedPropKind, AcceptedSurfaceCompleteness,
        BranchStatus, ComponentMetaAnalysis, ConsumedRootBindings, FallthroughBranch,
        FallthroughSurface, InheritedSource, MemberAvailability, MemberProvenance,
        ResolvedRootStep, RootBranch, RootReachability, RootTargetRef,
    };
    use verter_semantic::analysis::html_intrinsics::{IntrinsicMemberKind, OwnedIntrinsicMember};
    use verter_semantic::analysis::template::{PropValueConstness, TemplatePropUsage};
    use verter_semantic::analysis::type_expr::{
        ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr, ValueRef,
    };
    use verter_semantic::analysis::types::{
        AnalyzedImport, AnalyzedImportBinding, ImportBindingKind,
    };
    use verter_span::Span;

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
        intrinsic_members: FxHashMap<String, Vec<OwnedIntrinsicMember>>,
        canonical_routes: FxHashMap<(String, String), String>,
        child_resolutions: FxHashMap<String, TestResolution>,
    }

    impl FallthroughResolverHost for TestHost {
        type ChildResolution = TestResolution;

        fn intrinsic_members_for_tag(
            &self,
            _canonical_id: &str,
            tag: &str,
        ) -> Vec<OwnedIntrinsicMember> {
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
            _prop_type_overrides: Option<&FxHashMap<String, TypeExpr>>,
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
        ) -> Option<FxHashMap<String, TypeExpr>> {
            None
        }

        fn resolve_dynamic_root_candidates(
            &self,
            _canonical_id: &str,
            _snapshot: &Self::Snapshot,
            _usage_index: u32,
            _eval_env: &mut Option<Self::EvalEnv>,
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
    fn fallthrough_cache_key_hashes_overrides_deterministically() {
        let mut left = FxHashMap::default();
        left.insert("b".to_string(), TypeExpr::primitive(PrimitiveName::String));
        left.insert("a".to_string(), TypeExpr::primitive(PrimitiveName::Number));

        let mut right = FxHashMap::default();
        right.insert("a".to_string(), TypeExpr::primitive(PrimitiveName::Number));
        right.insert("b".to_string(), TypeExpr::primitive(PrimitiveName::String));

        assert_eq!(
            hash_prop_type_overrides(&left),
            hash_prop_type_overrides(&right)
        );
        assert_eq!(
            fallthrough_cache_key("/App.vue", true, Some(&left)),
            fallthrough_cache_key("/App.vue", true, Some(&right))
        );
    }

    #[test]
    fn append_native_candidate_branch_filters_declared_and_consumed_members() {
        let mut host = TestHost::default();
        host.intrinsic_members.insert(
            "button".to_string(),
            vec![
                OwnedIntrinsicMember {
                    name: "id".to_string(),
                    kind: IntrinsicMemberKind::Attr,
                    type_expr: TypeExpr::primitive(PrimitiveName::String),
                },
                OwnedIntrinsicMember {
                    name: "click".to_string(),
                    kind: IntrinsicMemberKind::Listener,
                    type_expr: TypeExpr::primitive(PrimitiveName::Unknown),
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
                    type_expr: TypeExpr::primitive(PrimitiveName::String),
                    raw_type: Some("string".to_string()),
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
    fn merge_fallthrough_branches_marks_partial_surfaces_conditional() {
        let mut accepted_props = Vec::new();
        let mut accepted_events = Vec::new();
        let branches = vec![
            FallthroughBranch {
                branch_key: "0".to_string(),
                condition_text: None,
                props: vec![verter_semantic::analysis::component_meta::FallthroughPropEntry {
                    name: "id".to_string(),
                    type_expr: TypeExpr::primitive(PrimitiveName::String),
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
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "id".to_string(),
                    optional: false,
                    readonly: false,
                    ty: TypeExpr::primitive(PrimitiveName::String),
                })],
            })),
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![
                    ObjectMember::Property(ObjectProperty {
                        name: "id".to_string(),
                        optional: false,
                        readonly: false,
                        ty: TypeExpr::primitive(PrimitiveName::String),
                    }),
                    ObjectMember::Property(ObjectProperty {
                        name: "title".to_string(),
                        optional: false,
                        readonly: false,
                        ty: TypeExpr::primitive(PrimitiveName::String),
                    }),
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

    #[test]
    fn inject_prop_type_overrides_adds_value_bindings() {
        let mut env = verter_semantic::analysis::type_eval::EvalEnv::new();
        let mut overrides = FxHashMap::default();
        overrides.insert(
            "size".to_string(),
            TypeExpr::primitive(PrimitiveName::Number),
        );

        inject_prop_type_overrides(&mut env, &overrides);

        assert_eq!(
            env.value_symbols
                .get("size")
                .and_then(|value| value.type_annotation.clone()),
            Some(TypeExpr::primitive(PrimitiveName::Number))
        );
    }

    #[test]
    fn resolve_usage_prop_type_handles_static_and_bound_inputs() {
        let static_prop = TemplatePropUsage {
            name: "title".to_string(),
            is_bound: false,
            expression: Some("hello".to_string()),
            constness: PropValueConstness::Const,
            referenced_bindings: Vec::new(),
            is_shorthand: false,
            from_spread: false,
            span: Span::new(0, 0),
            name_span: Span::new(0, 0),
        };
        let bound_prop = TemplatePropUsage {
            name: "size".to_string(),
            is_bound: true,
            expression: Some("42".to_string()),
            constness: PropValueConstness::Const,
            referenced_bindings: Vec::new(),
            is_shorthand: false,
            from_spread: false,
            span: Span::new(0, 0),
            name_span: Span::new(0, 0),
        };

        assert_eq!(
            resolve_usage_prop_type(&static_prop, |_| None),
            Some(TypeExpr::string_literal("hello"))
        );
        assert_eq!(
            resolve_usage_prop_type(&bound_prop, |_| None),
            Some(TypeExpr::number_literal(42.0))
        );
    }

    #[test]
    fn structural_substitute_typeof_refs_substitutes_length_one_value_refs() {
        let mut env = verter_semantic::analysis::type_eval::EvalEnv::new();
        env.add_value(verter_semantic::analysis::type_eval::ValueDeclInfo {
            name: "as".to_string(),
            declaration_id: 0,
            kind: verter_semantic::analysis::type_eval::ValueDeclKind::Const,
            type_annotation: Some(TypeExpr::string_literal("input")),
            function_signature: None,
            object_shape: None,
        });

        let lowered = TypeExpr::TypeOf(verter_semantic::analysis::type_expr::ValueRef {
            path: vec!["as".to_string()],
        });

        assert_eq!(
            structural_substitute_typeof_refs(&lowered, &env),
            TypeExpr::string_literal("input")
        );
    }

    #[test]
    fn structural_substitute_typeof_refs_preserves_unresolved_refs() {
        let env = verter_semantic::analysis::type_eval::EvalEnv::new();
        let lowered = TypeExpr::TypeOf(verter_semantic::analysis::type_expr::ValueRef {
            path: vec!["missing".to_string()],
        });

        assert_eq!(
            structural_substitute_typeof_refs(&lowered, &env),
            lowered,
            "bare refs without an env entry must round-trip unchanged"
        );
    }

    #[test]
    fn structural_substitute_typeof_refs_recurses_into_union_and_intersection() {
        let mut env = verter_semantic::analysis::type_eval::EvalEnv::new();
        env.add_value(verter_semantic::analysis::type_eval::ValueDeclInfo {
            name: "a".to_string(),
            declaration_id: 0,
            kind: verter_semantic::analysis::type_eval::ValueDeclKind::Const,
            type_annotation: Some(TypeExpr::string_literal("A")),
            function_signature: None,
            object_shape: None,
        });
        env.add_value(verter_semantic::analysis::type_eval::ValueDeclInfo {
            name: "b".to_string(),
            declaration_id: 0,
            kind: verter_semantic::analysis::type_eval::ValueDeclKind::Const,
            type_annotation: Some(TypeExpr::string_literal("B")),
            function_signature: None,
            object_shape: None,
        });

        let union = TypeExpr::union(vec![
            TypeExpr::TypeOf(verter_semantic::analysis::type_expr::ValueRef {
                path: vec!["a".to_string()],
            }),
            TypeExpr::TypeOf(verter_semantic::analysis::type_expr::ValueRef {
                path: vec!["b".to_string()],
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
        env.add_value(verter_semantic::analysis::type_eval::ValueDeclInfo {
            name: "props".to_string(),
            declaration_id: 0,
            kind: verter_semantic::analysis::type_eval::ValueDeclKind::Const,
            type_annotation: Some(TypeExpr::string_literal("ignored")),
            function_signature: None,
            object_shape: None,
        });

        let lowered = TypeExpr::TypeOf(verter_semantic::analysis::type_expr::ValueRef {
            path: vec!["props".to_string(), "name".to_string()],
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
                    type_expr: TypeExpr::primitive(PrimitiveName::String),
                    raw_type: Some("string".to_string()),
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
}
