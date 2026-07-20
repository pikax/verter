//! TSC/IDE splice-text projection for the Vue macro codegen producer: scope
//! requirement collection, class-member inference, and emit/parameter rendering.
//!
//! Sibling half of the parent module `impl VerterHost` orchestrator.

use super::runtime::{
    authored_emit_anchor, authored_emit_order, partial_failure, resolution_failure,
};
use super::*;

pub(super) fn render_tsc_node(
    ctx: &dyn ResolverContext,
    node: crate::semantic_query::SemanticNodeId,
    counters: &mut VueMacroCodegenCounters,
) -> Result<TscSpliceText, ProjectionFailure> {
    counters.tsc_materializations += 1;
    let rendered = crate::typeinfo::raise::render_node_display_with_ctx(ctx, node);
    if crate::request_context::current_cold_compute_completeness().is_partial() {
        return Err(partial_failure());
    }
    rendered
        .map(TscSpliceText::new)
        .ok_or(ProjectionFailure::Unsupported(
            UnsupportedReason::SemanticConstruct,
        ))
}

pub(super) fn tsc_scope_requirements(
    mac: &AnalyzedMacro,
    inventory: &TscScopeInventory<'_>,
) -> Result<TscScopeRequirements, ProjectionFailure> {
    let macro_owner = tsc_script_owner(mac.owner)?;
    let mut required_imports = BTreeMap::new();
    let mut roots = Vec::new();
    for name in &mac.type_references {
        match visible_type_binding(inventory, macro_owner, name)? {
            Some(VisibleTypeBinding::Import(owner)) => {
                required_imports.insert((owner, name.clone()), TscBindingUsage::TypePosition);
            }
            Some(VisibleTypeBinding::Local(owner)) => roots.push((owner, name.clone())),
            None => {}
        }
    }
    for dependency in inventory
        .analysis
        .macro_type_deps
        .iter()
        .filter(|dependency| dependency.macro_span == mac.span)
        .filter(|dependency| dependency.usage.is_value_query())
    {
        let Some(owner) = visible_import_owner(
            inventory,
            macro_owner,
            &dependency.type_name,
            Some(&dependency.import_source),
        )?
        else {
            continue;
        };
        if let Some(usage) = required_imports.get_mut(&(owner, dependency.type_name.clone())) {
            *usage = TscBindingUsage::ValueQuery;
        }
    }
    let mut direct_owner_value_dependencies = Vec::new();
    for name in &mac.type_references {
        if visible_import_owner(inventory, macro_owner, name, None)?.is_some() {
            continue;
        }
        let Ok(owner) = local_value_owner(inventory, macro_owner, name) else {
            continue;
        };
        let declaration_owner = top_level_owner(owner);
        if !inventory
            .shallow_state
            .has_value_symbol_in(declaration_owner, name)
            || inventory
                .shallow_state
                .has_type_symbol_in(declaration_owner, name)
        {
            continue;
        }
        direct_owner_value_dependencies.push(TscOwnerValueDependency {
            owner,
            name: name.clone(),
        });
    }
    direct_owner_value_dependencies.sort_by(|left, right| {
        (left.owner, left.name.as_str()).cmp(&(right.owner, right.name.as_str()))
    });
    direct_owner_value_dependencies.dedup();

    roots.sort_by_key(|(owner, name)| declaration_order(inventory, *owner, name));
    roots.dedup();

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut owner_value_dependencies =
        BTreeMap::<(TscScriptOwner, String), BTreeSet<TscOwnerValueDependency>>::new();
    let mut retained_value_dependencies =
        BTreeMap::<(TscScriptOwner, String), BTreeSet<TscRetainedValueCarrier>>::new();
    let mut declaration_ordered_names = Vec::new();
    for (owner, root) in roots {
        collect_local_declaration_closure(
            owner,
            &root,
            inventory,
            &mut required_imports,
            &mut visiting,
            &mut visited,
            &mut owner_value_dependencies,
            &mut retained_value_dependencies,
            &mut declaration_ordered_names,
        )?;
    }

    let retained_value_carriers = declaration_ordered_names
        .iter()
        .filter_map(|(owner, name)| retained_value_carrier(inventory, *owner, name).ok())
        .map(|carrier| ((carrier.owner, carrier.name.clone()), carrier))
        .collect::<BTreeMap<_, _>>();

    let mut dependency_declarations = Vec::new();
    for (owner, name) in declaration_ordered_names {
        let mut found = false;
        for (contributor_ordinal, entry) in inventory
            .analysis
            .declaration_entries
            .iter()
            .filter(|entry| {
                entry.name == name
                    && entry.owner == top_level_owner(owner)
                    && matches!(
                        entry.kind,
                        LocalDeclarationKind::Type | LocalDeclarationKind::TypeAndValue
                    )
            })
            .enumerate()
        {
            inventory
                .raw_source
                .get(entry.span.start as usize..entry.span.end as usize)
                .ok_or(ProjectionFailure::Unresolved(
                    UnresolvedReason::MissingDependency,
                ))?;
            found = true;
            let (inferred_class_members, inferred_value_dependencies, declaration_failure) =
                match inferred_class_members(
                    top_level_owner(owner),
                    &name,
                    contributor_ordinal,
                    entry.span,
                    entry.kind == LocalDeclarationKind::TypeAndValue,
                    inventory,
                ) {
                    Ok(inferred) => (inferred.members, inferred.value_dependencies, None),
                    Err(failure) => (
                        Vec::new(),
                        BTreeSet::new(),
                        Some(failure.declaration_reason()),
                    ),
                };
            for dependency in inferred_value_dependencies {
                let root = dependency.root();
                if let Some(import_owner) = visible_import_owner(inventory, owner, root, None)? {
                    let import_key = (import_owner, root.to_string());
                    merge_required_import(
                        &mut required_imports,
                        import_key,
                        TscBindingUsage::ValueQuery,
                    );
                } else {
                    let value_owner = local_value_owner(inventory, owner, root)?;
                    if let Some(carrier) =
                        retained_value_carriers.get(&(value_owner, root.to_string()))
                    {
                        retained_value_dependencies
                            .entry((owner, name.clone()))
                            .or_default()
                            .insert(carrier.clone());
                        continue;
                    }
                    owner_value_dependencies
                        .entry((owner, name.clone()))
                        .or_default()
                        .insert(TscOwnerValueDependency {
                            owner: value_owner,
                            name: root.to_string(),
                        });
                }
            }
            dependency_declarations.push(TscDependencyDeclaration {
                owner,
                name: name.clone(),
                contributor_ordinal: u32::try_from(contributor_ordinal).map_err(|_| {
                    ProjectionFailure::Unresolved(UnresolvedReason::MissingDependency)
                })?,
                owner_value_dependencies: owner_value_dependencies
                    .get(&(owner, name.clone()))
                    .map(|dependencies| dependencies.iter().cloned().collect())
                    .unwrap_or_default(),
                retained_value_carriers: retained_value_dependencies
                    .get(&(owner, name.clone()))
                    .map(|dependencies| dependencies.iter().cloned().collect())
                    .unwrap_or_default(),
                declaration_failure,
                inferred_class_members,
            });
        }
        if !found {
            return Err(ProjectionFailure::Unresolved(
                UnresolvedReason::MissingDependency,
            ));
        }
    }

    let mut retained_names = BTreeSet::new();
    let mut retained_bindings = Vec::new();
    for import in &inventory.analysis.imports {
        let owner = tsc_script_owner(import.owner)?;
        for binding in &import.bindings {
            let key = (owner, binding.name.clone());
            let Some(usage) = required_imports.get(&key).copied() else {
                continue;
            };
            if retained_names.insert(key) {
                retained_bindings.push(TscRetainedBinding {
                    owner,
                    local_name: binding.name.clone(),
                    usage,
                });
            }
        }
    }
    if retained_bindings.len() != required_imports.len() {
        return Err(ProjectionFailure::Unresolved(
            UnresolvedReason::MissingDependency,
        ));
    }

    Ok(TscScopeRequirements {
        owner_value_dependencies: direct_owner_value_dependencies,
        retained_bindings,
        dependency_declarations,
    })
}

fn local_value_owner(
    inventory: &TscScopeInventory<'_>,
    requester: TscScriptOwner,
    name: &str,
) -> Result<TscScriptOwner, ProjectionFailure> {
    visible_owner(
        requester,
        inventory
            .analysis
            .declaration_entries
            .iter()
            .filter(|entry| entry.name == name)
            .filter(|entry| {
                matches!(
                    entry.kind,
                    LocalDeclarationKind::Value | LocalDeclarationKind::TypeAndValue
                )
            })
            .map(|entry| tsc_script_owner(entry.owner))
            .collect::<Result<BTreeSet<_>, _>>()?,
    )
    .ok_or(ProjectionFailure::Unresolved(
        UnresolvedReason::MissingDependency,
    ))
}

fn declaration_order(
    inventory: &TscScopeInventory<'_>,
    owner: TscScriptOwner,
    name: &str,
) -> (u32, TscScriptOwner, String) {
    (
        inventory
            .analysis
            .declaration_entries
            .iter()
            .filter(|entry| entry.name == name && entry.owner == top_level_owner(owner))
            .map(|entry| entry.span.start)
            .min()
            .unwrap_or(u32::MAX),
        owner,
        name.to_owned(),
    )
}

#[derive(Clone, Copy)]
enum VisibleTypeBinding {
    Import(TscScriptOwner),
    Local(TscScriptOwner),
}

fn visible_type_binding(
    inventory: &TscScopeInventory<'_>,
    requester: TscScriptOwner,
    name: &str,
) -> Result<Option<VisibleTypeBinding>, ProjectionFailure> {
    for owner in visible_owner_order(requester) {
        let has_import = visible_import_owner(inventory, owner, name, None)? == Some(owner);
        let has_local = inventory.analysis.declaration_entries.iter().any(|entry| {
            entry.name == name
                && entry.owner == top_level_owner(owner)
                && matches!(
                    entry.kind,
                    LocalDeclarationKind::Type | LocalDeclarationKind::TypeAndValue
                )
        });
        match (has_import, has_local) {
            (true, false) => return Ok(Some(VisibleTypeBinding::Import(owner))),
            (false, true) => return Ok(Some(VisibleTypeBinding::Local(owner))),
            (true, true) => {
                return Err(ProjectionFailure::Unsupported(
                    UnsupportedReason::SemanticConstruct,
                ));
            }
            (false, false) => {}
        }
    }
    Ok(None)
}

fn visible_local_type_owner(
    inventory: &TscScopeInventory<'_>,
    requester: TscScriptOwner,
    name: &str,
) -> Result<TscScriptOwner, ProjectionFailure> {
    match visible_type_binding(inventory, requester, name)? {
        Some(VisibleTypeBinding::Local(owner)) => Ok(owner),
        Some(VisibleTypeBinding::Import(_)) => Err(ProjectionFailure::Unsupported(
            UnsupportedReason::SemanticConstruct,
        )),
        None => Err(ProjectionFailure::Unresolved(
            UnresolvedReason::MissingDependency,
        )),
    }
}

fn visible_import_owner(
    inventory: &TscScopeInventory<'_>,
    requester: TscScriptOwner,
    name: &str,
    source: Option<&str>,
) -> Result<Option<TscScriptOwner>, ProjectionFailure> {
    let mut owners = BTreeSet::new();
    for import in &inventory.analysis.imports {
        if source.is_none_or(|source| import.source == source)
            && import.bindings.iter().any(|binding| binding.name == name)
        {
            owners.insert(tsc_script_owner(import.owner)?);
        }
    }
    Ok(visible_owner(requester, owners))
}

fn visible_owner(
    requester: TscScriptOwner,
    owners: BTreeSet<TscScriptOwner>,
) -> Option<TscScriptOwner> {
    visible_owner_order(requester)
        .into_iter()
        .find(|owner| owners.contains(owner))
}

fn visible_owner_order(requester: TscScriptOwner) -> Vec<TscScriptOwner> {
    match requester {
        TscScriptOwner::Setup => vec![TscScriptOwner::Setup, TscScriptOwner::Companion],
        TscScriptOwner::Companion => vec![TscScriptOwner::Companion],
    }
}

struct InferredClassProjection {
    members: Vec<TscInferredClassMember>,
    value_dependencies: BTreeSet<verter_type_expr::facts::TypeDependencyPathFact>,
}

fn inferred_class_members(
    owner: verter_type_expr::TopLevelOwnerId,
    name: &str,
    contributor_ordinal: usize,
    declaration_span: verter_span::Span,
    include_static: bool,
    inventory: &TscScopeInventory<'_>,
) -> Result<InferredClassProjection, ClassInferenceFailure> {
    fn collect_overload_groups(
        ty: &verter_type_expr::TypeExpr,
        is_static: bool,
        groups: &mut BTreeSet<(String, bool)>,
    ) -> Result<(), verter_type_expr::facts::InferenceUnavailableReason> {
        use crate::resolver_core::shallow_file_state::SEMANTIC_INFERENCE_TRAVERSAL_BUDGET;

        let mut pending = vec![ty];
        let mut visited = 0usize;
        while let Some(current) = pending.pop() {
            visited = visited.saturating_add(1);
            if visited > SEMANTIC_INFERENCE_TRAVERSAL_BUDGET {
                return Err(
                    verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded,
                );
            }
            match current {
                verter_type_expr::TypeExpr::Object(object) => {
                    for member in &object.properties {
                        if let verter_type_expr::ObjectMember::Method(method) = member {
                            if !method.has_implementation_body {
                                groups.insert((method.name.clone(), is_static));
                            }
                        }
                    }
                }
                verter_type_expr::TypeExpr::Intersection(parts)
                | verter_type_expr::TypeExpr::Union(parts) => pending.extend(parts.iter()),
                verter_type_expr::TypeExpr::Parenthesized(inner) => pending.push(inner),
                _ => {}
            }
        }
        Ok(())
    }

    let Some(lowered) = inventory.shallow_state.effective_type_decl_in(owner, name) else {
        return Err(ClassInferenceFailure::Unresolved(
            UnresolvedReason::MissingDependency,
        ));
    };
    if lowered.kind != verter_semantic::analysis::type_eval::TypeDeclKind::Class {
        return Ok(InferredClassProjection {
            members: Vec::new(),
            value_dependencies: BTreeSet::new(),
        });
    }
    let contributors = match inventory
        .shallow_state
        .decl_bodies()
        .transient_type_bodies_in(owner, name)
    {
        crate::decl_body_memo::DemandOutcome::Ready(Some(contributors)) => contributors,
        _ => {
            return Err(ClassInferenceFailure::Unresolved(
                UnresolvedReason::MissingDependency,
            ));
        }
    };
    let Some(body) = contributors.get(contributor_ordinal) else {
        return Err(ClassInferenceFailure::Unresolved(
            UnresolvedReason::MissingDependency,
        ));
    };
    let Some(contributor_fact) = lowered.contributor_facts.get(contributor_ordinal) else {
        return Err(ClassInferenceFailure::Unresolved(
            UnresolvedReason::MissingDependency,
        ));
    };

    struct Candidate<'a> {
        start: u32,
        name: &'a str,
        is_static: bool,
        position: TscInferredClassTypePosition,
        ty: Option<&'a verter_type_expr::TypeExpr>,
        has_implementation_body: bool,
        return_inference: Option<verter_type_expr::facts::ReturnInferenceCompleteness>,
    }

    let mut overload_groups = BTreeSet::new();
    collect_overload_groups(body, false, &mut overload_groups)
        .map_err(ClassInferenceFailure::InferenceUnavailable)?;
    let mut candidates = Vec::new();
    let mut pending = vec![body];
    while let Some(current) = pending.pop() {
        match current {
            verter_type_expr::TypeExpr::Object(object) => {
                for (member_index, member) in object.properties.iter().enumerate() {
                    match member {
                        verter_type_expr::ObjectMember::Property(property)
                            if property.spans.type_annotation.is_none() =>
                        {
                            if let Some(span) = property.spans.declaration.filter(|span| {
                                span.start >= declaration_span.start
                                    && span.end <= declaration_span.end
                            }) {
                                candidates.push(Candidate {
                                    start: span.start,
                                    name: &property.name,
                                    is_static: false,
                                    position: TscInferredClassTypePosition::Property,
                                    ty: Some(&property.ty),
                                    has_implementation_body: false,
                                    return_inference: None,
                                });
                            }
                        }
                        verter_type_expr::ObjectMember::Method(method) => {
                            for parameter in &method.function.parameters {
                                if parameter.has_ts_annotation || parameter.is_parameter_property {
                                    continue;
                                }
                                if let (Some(name), Some(span)) = (
                                    parameter.name.as_deref(),
                                    parameter.span.filter(|span| {
                                        span.start >= declaration_span.start
                                            && span.end <= declaration_span.end
                                    }),
                                ) {
                                    candidates.push(Candidate {
                                        start: span.start,
                                        name,
                                        is_static: false,
                                        position: TscInferredClassTypePosition::Parameter,
                                        ty: Some(&parameter.ty),
                                        has_implementation_body: method.has_implementation_body,
                                        return_inference: None,
                                    });
                                }
                            }
                            if method.function.spans.return_type.is_none()
                                && method.method_kind != verter_type_expr::ObjectMethodKind::Set
                            {
                                if let Some(span) = method.spans.declaration.filter(|span| {
                                    span.start >= declaration_span.start
                                        && span.end <= declaration_span.end
                                }) {
                                    candidates.push(Candidate {
                                        start: span.start,
                                        name: &method.name,
                                        is_static: false,
                                        position: TscInferredClassTypePosition::Return,
                                        ty: method.function.return_type.as_deref(),
                                        has_implementation_body: method.has_implementation_body,
                                        return_inference: u32::try_from(member_index)
                                            .ok()
                                            .and_then(|member_ordinal| {
                                                let member_path = [member_ordinal];
                                                contributor_fact
                                                    .return_inference_for_member_path(&member_path)
                                            }),
                                    });
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            verter_type_expr::TypeExpr::Intersection(parts)
            | verter_type_expr::TypeExpr::Union(parts) => {
                pending.extend(parts.iter().rev());
            }
            verter_type_expr::TypeExpr::Parenthesized(inner) => pending.push(inner),
            _ => {}
        }
    }

    let static_decl = if include_static {
        Some(
            inventory
                .shallow_state
                .effective_value_decl_in(owner, name)
                .ok_or(ClassInferenceFailure::Unresolved(
                    UnresolvedReason::MissingDependency,
                ))?,
        )
    } else {
        None
    };
    let static_parts = if include_static {
        match inventory
            .shallow_state
            .decl_bodies()
            .transient_value_parts_in(owner, name)
        {
            crate::decl_body_memo::DemandOutcome::Ready(Some(parts)) => Some(parts),
            _ => {
                return Err(ClassInferenceFailure::Unresolved(
                    UnresolvedReason::MissingDependency,
                ));
            }
        }
    } else {
        None
    };
    if let Some(object) = static_parts
        .as_ref()
        .and_then(|parts| parts.object_shape.as_ref())
    {
        let Some(object_fact) = static_decl
            .as_ref()
            .and_then(|declaration| declaration.object_shape.as_ref())
        else {
            return Err(ClassInferenceFailure::Unresolved(
                UnresolvedReason::MissingDependency,
            ));
        };
        collect_overload_groups(
            &verter_type_expr::TypeExpr::Object(object.clone().into()),
            true,
            &mut overload_groups,
        )
        .map_err(ClassInferenceFailure::InferenceUnavailable)?;
        for (member_index, member) in object.properties.iter().enumerate() {
            match member {
                verter_type_expr::ObjectMember::Property(property)
                    if property.spans.type_annotation.is_none() =>
                {
                    if let Some(span) = property.spans.declaration.filter(|span| {
                        span.start >= declaration_span.start && span.end <= declaration_span.end
                    }) {
                        candidates.push(Candidate {
                            start: span.start,
                            name: &property.name,
                            is_static: true,
                            position: TscInferredClassTypePosition::Property,
                            ty: Some(&property.ty),
                            has_implementation_body: false,
                            return_inference: None,
                        });
                    }
                }
                verter_type_expr::ObjectMember::Method(method) => {
                    for parameter in &method.function.parameters {
                        if parameter.has_ts_annotation || parameter.is_parameter_property {
                            continue;
                        }
                        if let (Some(name), Some(span)) = (
                            parameter.name.as_deref(),
                            parameter.span.filter(|span| {
                                span.start >= declaration_span.start
                                    && span.end <= declaration_span.end
                            }),
                        ) {
                            candidates.push(Candidate {
                                start: span.start,
                                name,
                                is_static: true,
                                position: TscInferredClassTypePosition::Parameter,
                                ty: Some(&parameter.ty),
                                has_implementation_body: method.has_implementation_body,
                                return_inference: None,
                            });
                        }
                    }
                    if method.function.spans.return_type.is_none()
                        && method.method_kind != verter_type_expr::ObjectMethodKind::Set
                    {
                        if let Some(span) = method.spans.declaration.filter(|span| {
                            span.start >= declaration_span.start && span.end <= declaration_span.end
                        }) {
                            candidates.push(Candidate {
                                start: span.start,
                                name: &method.name,
                                is_static: true,
                                position: TscInferredClassTypePosition::Return,
                                ty: method.function.return_type.as_deref(),
                                has_implementation_body: method.has_implementation_body,
                                return_inference: object_fact.members.get(member_index).and_then(
                                    |member| match member {
                                        verter_type_expr::facts::ObjectMemberFact::Method(
                                            method,
                                        ) => Some(method.function.return_inference),
                                        _ => None,
                                    },
                                ),
                            });
                        }
                    }
                }
                verter_type_expr::ObjectMember::ConstructSignature(function) => {
                    for parameter in &function.parameters {
                        if parameter.has_ts_annotation || parameter.is_parameter_property {
                            continue;
                        }
                        if let (Some(name), Some(span)) = (
                            parameter.name.as_deref(),
                            parameter.span.filter(|span| {
                                span.start >= declaration_span.start
                                    && span.end <= declaration_span.end
                            }),
                        ) {
                            candidates.push(Candidate {
                                start: span.start,
                                name,
                                is_static: false,
                                position: TscInferredClassTypePosition::Parameter,
                                ty: Some(&parameter.ty),
                                has_implementation_body: false,
                                return_inference: None,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }
    candidates.retain(|candidate| {
        !(candidate.has_implementation_body
            && overload_groups.contains(&(candidate.name.to_owned(), candidate.is_static)))
    });
    candidates.sort_by_key(|candidate| candidate.start);

    let mut occurrences = std::collections::BTreeMap::new();
    let mut inferred = Vec::with_capacity(candidates.len());
    let mut value_dependencies = BTreeSet::new();
    for candidate in candidates {
        if candidate.position == TscInferredClassTypePosition::Return {
            let Some(completeness) = candidate.return_inference else {
                return Err(ClassInferenceFailure::Unsupported(
                    UnsupportedReason::SemanticConstruct,
                ));
            };
            match completeness {
                verter_type_expr::facts::ReturnInferenceCompleteness::Unavailable(reason) => {
                    return Err(ClassInferenceFailure::InferenceUnavailable(reason));
                }
                verter_type_expr::facts::ReturnInferenceCompleteness::Unsupported(_) => {
                    return Err(ClassInferenceFailure::Unsupported(
                        UnsupportedReason::SemanticConstruct,
                    ));
                }
                verter_type_expr::facts::ReturnInferenceCompleteness::NotInferred
                | verter_type_expr::facts::ReturnInferenceCompleteness::Complete { .. } => {}
            }
        }
        let Some(candidate_type) = candidate.ty else {
            return Err(ClassInferenceFailure::Unsupported(
                UnsupportedReason::SemanticConstruct,
            ));
        };
        match crate::resolver_core::shallow_file_state::type_expr_is_declaration_safe(
            candidate_type,
        ) {
            Ok(true) => {}
            Ok(false) => {
                return Err(ClassInferenceFailure::Unsupported(
                    UnsupportedReason::SemanticConstruct,
                ));
            }
            Err(reason) => return Err(ClassInferenceFailure::InferenceUnavailable(reason)),
        }
        let occurrence = occurrences
            .entry((candidate.name, candidate.is_static, candidate.position))
            .or_insert(0_u32);
        let type_text = verter_type_expr::render_type_expr_display(candidate_type)
            .map_err(|_| ClassInferenceFailure::Unsupported(UnsupportedReason::SemanticConstruct))?
            .text;
        crate::resolver_core::shallow_file_state::collect_typeof_roots(
            candidate_type,
            &mut value_dependencies,
        )
        .map_err(ClassInferenceFailure::InferenceUnavailable)?;
        inferred.push(TscInferredClassMember {
            name: candidate.name.to_owned(),
            occurrence: *occurrence,
            is_static: candidate.is_static,
            position: candidate.position,
            type_text: TscSpliceText::new(type_text),
        });
        *occurrence = occurrence.saturating_add(1);
    }
    Ok(InferredClassProjection {
        members: inferred,
        value_dependencies,
    })
}

fn collect_local_declaration_closure(
    owner: TscScriptOwner,
    name: &str,
    inventory: &TscScopeInventory<'_>,
    required_imports: &mut BTreeMap<(TscScriptOwner, String), TscBindingUsage>,
    visiting: &mut BTreeSet<(TscScriptOwner, String)>,
    visited: &mut BTreeSet<(TscScriptOwner, String)>,
    owner_value_dependencies: &mut BTreeMap<
        (TscScriptOwner, String),
        BTreeSet<TscOwnerValueDependency>,
    >,
    retained_value_dependencies: &mut BTreeMap<
        (TscScriptOwner, String),
        BTreeSet<TscRetainedValueCarrier>,
    >,
    ordered: &mut Vec<(TscScriptOwner, String)>,
) -> Result<(), ProjectionFailure> {
    let identity = (owner, name.to_owned());
    if visited.contains(&identity) || !visiting.insert(identity.clone()) {
        return Ok(());
    }

    let declaration_owner = top_level_owner(owner);
    let Some(deps) = inventory
        .shallow_state
        .type_deps_in(declaration_owner, name)
    else {
        return Err(resolution_failure());
    };
    if !deps.unroutable_declaration_dependencies.is_empty() || deps.has_unroutable_value_position {
        return Err(ProjectionFailure::Unsupported(
            UnsupportedReason::SemanticConstruct,
        ));
    }
    for external in &deps.declaration_external_deps {
        let usage = if deps.external_value_positions.contains(&external.local_name) {
            TscBindingUsage::ValuePosition
        } else if deps.external_value_queries.contains(&external.local_name) {
            TscBindingUsage::ValueQuery
        } else {
            TscBindingUsage::TypePosition
        };
        let import_owner = visible_import_owner(
            inventory,
            owner,
            &external.local_name,
            Some(&external.source_specifier),
        )?
        .ok_or(ProjectionFailure::Unresolved(
            UnresolvedReason::MissingDependency,
        ))?;
        merge_required_import(
            required_imports,
            (import_owner, external.local_name.clone()),
            usage,
        );
    }
    for dependency in &deps.owner_value_deps {
        let value_owner = local_value_owner(inventory, owner, dependency)?;
        owner_value_dependencies
            .entry(identity.clone())
            .or_default()
            .insert(TscOwnerValueDependency {
                owner: value_owner,
                name: dependency.clone(),
            });
    }
    for dependency in &deps.retained_value_carrier_deps {
        let value_owner = local_value_owner(inventory, owner, dependency)?;
        retained_value_dependencies
            .entry(identity.clone())
            .or_default()
            .insert(retained_value_carrier(inventory, value_owner, dependency)?);
    }

    let mut local_deps = deps.declaration_local_deps.clone();
    let mut local_deps = local_deps
        .drain(..)
        .map(|dependency| {
            let dependency_owner = visible_local_type_owner(inventory, owner, &dependency)?;
            Ok((dependency_owner, dependency))
        })
        .collect::<Result<Vec<_>, ProjectionFailure>>()?;
    local_deps.sort_by_key(|(owner, dependency)| declaration_order(inventory, *owner, dependency));
    local_deps.dedup();
    for (dependency_owner, dependency) in local_deps {
        if !inventory
            .shallow_state
            .effective_type_header_present_in(top_level_owner(dependency_owner), &dependency)
        {
            return Err(ProjectionFailure::Unresolved(
                UnresolvedReason::MissingDependency,
            ));
        }
        collect_local_declaration_closure(
            dependency_owner,
            &dependency,
            inventory,
            required_imports,
            visiting,
            visited,
            owner_value_dependencies,
            retained_value_dependencies,
            ordered,
        )?;
    }

    visiting.remove(&identity);
    if visited.insert(identity.clone()) {
        ordered.push(identity);
    }
    Ok(())
}

fn retained_value_carrier(
    inventory: &TscScopeInventory<'_>,
    owner: TscScriptOwner,
    name: &str,
) -> Result<TscRetainedValueCarrier, ProjectionFailure> {
    inventory
        .analysis
        .declaration_entries
        .iter()
        .filter(|entry| {
            entry.owner == top_level_owner(owner)
                && entry.name == name
                && matches!(
                    entry.kind,
                    LocalDeclarationKind::Type | LocalDeclarationKind::TypeAndValue
                )
        })
        .enumerate()
        .filter(|(_, entry)| entry.kind == LocalDeclarationKind::TypeAndValue)
        .last()
        .and_then(|(contributor_ordinal, _)| {
            u32::try_from(contributor_ordinal)
                .ok()
                .map(|contributor_ordinal| TscRetainedValueCarrier {
                    owner,
                    name: name.to_owned(),
                    contributor_ordinal,
                })
        })
        .ok_or(ProjectionFailure::Unresolved(
            UnresolvedReason::MissingDependency,
        ))
}

fn merge_required_import(
    required_imports: &mut BTreeMap<(TscScriptOwner, String), TscBindingUsage>,
    key: (TscScriptOwner, String),
    usage: TscBindingUsage,
) {
    required_imports
        .entry(key)
        .and_modify(|existing| {
            if binding_usage_precedence(usage) > binding_usage_precedence(*existing) {
                *existing = usage;
            }
        })
        .or_insert(usage);
}

fn binding_usage_precedence(usage: TscBindingUsage) -> u8 {
    match usage {
        TscBindingUsage::TypePosition => 0,
        TscBindingUsage::ValueQuery => 1,
        TscBindingUsage::ValuePosition => 2,
    }
}

/// Whether a `defineEmits<T>()` type argument is a bare reference to a type
/// declared in ANOTHER framework-carrier SFC (`.vue` / `.svelte`). The emit
/// surface is re-synthesized into per-event rows regardless, so the imported
/// carrier type name is never referenced in the output; retaining
/// `import type { … } from './Child.vue'` would leave a DANGLING type import
/// (the `*.vue` module shim resolves to `DefineComponent`, never a named type
/// export). This predicate flags exactly that case so the caller drops the
/// unused carrier emit-type binding. A SAME-file carrier reference (a local
/// interface in this SFC's own `<script>`) is retained normally (its
/// declaration is in scope). A cross-file reference into a plain `.ts`/`.d.ts`
/// module is also kept — that import resolves. The carrier classification is
/// the registry-backed [`verter_workspace::resolver::path_is_carrier`], the
/// single structural authority (a new carrier extends the registry, not this
/// predicate).
pub(super) fn emit_type_is_cross_sfc_carrier(
    dispatch: &ProjectSemanticDispatch<'_>,
    payload: crate::semantic_query::SemanticNodeId,
    owner_canonical: &str,
) -> bool {
    let Some(data) = crate::project_semantic_dispatch::node_data_for(dispatch.ctx, payload) else {
        return false;
    };
    let canonical: &str = match data.as_ref() {
        SemanticNodeData::DeclRef { identity } => identity.canonical_id.as_ref(),
        SemanticNodeData::InstantiationRef { base, .. } => base.canonical_id.as_ref(),
        _ => return false,
    };
    canonical != owner_canonical && verter_workspace::resolver::path_is_carrier(canonical)
}

/// Testing-surface type text for a prop whose resolved type is a type declared
/// in ANOTHER file reached through a NAMESPACE import (`OwnerNS.SetupQualified`).
///
/// The shared display renders such a reference as its bare resolved decl name
/// (`SetupQualified`), but only the namespace binding (`OwnerNS`) — not the bare
/// name — is in the generated testing surface's scope, so the emitted
/// `declare const qualifiedSetup: SetupQualified` fails to resolve (TS2304).
/// Emit a self-contained `import("<specifier>").<Name>` type instead, which
/// resolves without depending on the namespace binding (whose identity may be
/// shadowed across the setup/companion scopes) or on a retained named import.
///
/// Returns `None` — deferring to the display path — for a local type, or a
/// cross-file type whose bare name IS a directly-imported binding (the scope
/// requirements retain that named import, so the bare form already resolves).
pub(super) fn cross_file_namespace_import_type(
    ctx: &dyn ResolverContext,
    dispatch: &ProjectSemanticDispatch<'_>,
    node: crate::semantic_query::SemanticNodeId,
    owner_canonical: &str,
    inventory: &TscScopeInventory<'_>,
) -> Option<String> {
    let data = crate::project_semantic_dispatch::node_data_for(dispatch.ctx, node)?;
    let SemanticNodeData::DeclRef { identity } = data.as_ref() else {
        return None;
    };
    let canonical = identity.canonical_id.as_ref();
    if canonical == owner_canonical {
        return None;
    }
    let decl_name = identity.decl_name.as_ref();
    // A directly-imported named binding is retained by the scope requirements,
    // so its bare name already resolves — leave those to the display path.
    let bare_name_is_imported = inventory
        .analysis
        .imports
        .iter()
        .flat_map(|import| import.bindings.iter())
        .any(|binding| binding.name == decl_name);
    if bare_name_is_imported {
        return None;
    }
    // The authored specifier that resolves to the referenced file — reused
    // verbatim so the `import(...)` in the sibling testing surface resolves the
    // same target the component's own import does. Resolved through the shared
    // host import resolver (the analyzer snapshot's `resolved_canonical_id` is
    // not populated on this path).
    let specifier = inventory.analysis.imports.iter().find_map(|import| {
        (ctx.resolve_type_dependency_canonical(owner_canonical, &import.source)
            .as_deref()
            == Some(canonical))
        .then_some(import.source.as_str())
    })?;
    Some(format!("import(\"{specifier}\").{decl_name}"))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn tsc_emit_rows(
    ctx: &dyn ResolverContext,
    dispatch: &ProjectSemanticDispatch<'_>,
    surface: &TypeInfoSurface,
    mac: &AnalyzedMacro,
    payload_index: usize,
    effective_index: usize,
    counters: &mut VueMacroCodegenCounters,
) -> Result<Vec<TscEmitRow>, ProjectionFailure> {
    let context = ProjectionReductionContext::published(ProjectionMode::Navigate);
    let mut rows = Vec::new();

    for signature in surface.call_signatures.iter() {
        let callable = CallableNodeView::new(dispatch, signature.node);
        let Some(names) = callable.event_names(context) else {
            continue;
        };
        let Some(signature) = callable.signature(context) else {
            continue;
        };
        let parameters = render_function_parameters(ctx, &signature.raw_params()[1..], counters)?;
        for name in names {
            push_tsc_emit(
                &mut rows,
                name.as_ref(),
                parameters.clone(),
                authored_emit_anchor(mac, payload_index, effective_index, name.as_ref()),
            );
        }
    }

    for member in surface
        .members
        .iter()
        .filter(|member| member.visibility.is_public())
    {
        let parameters = render_emit_payload_parameters(ctx, dispatch, member.value, counters)?;
        push_tsc_emit(
            &mut rows,
            member.name.as_ref(),
            parameters,
            authored_emit_anchor(mac, payload_index, effective_index, member.name.as_ref()),
        );
    }

    for field in &mac.emit_fields {
        push_tsc_emit(
            &mut rows,
            field.name.as_str(),
            TscSpliceText::new("...args: unknown[]"),
            authored_emit_anchor(mac, payload_index, effective_index, field.name.as_str()),
        );
    }
    rows.sort_by_key(|row| authored_emit_order(row.anchor));

    if crate::request_context::current_cold_compute_completeness().is_partial() {
        return Err(partial_failure());
    }
    Ok(rows)
}

fn push_tsc_emit(
    rows: &mut Vec<TscEmitRow>,
    name: &str,
    parameters: TscSpliceText,
    anchor: MacroAnchor,
) {
    if rows.iter().any(|row| row.name == name) {
        return;
    }
    rows.push(TscEmitRow {
        name: name.to_owned(),
        emit_parameters: parameters.clone(),
        handler_parameters: parameters,
        anchor,
    });
}

fn render_emit_payload_parameters(
    ctx: &dyn ResolverContext,
    dispatch: &ProjectSemanticDispatch<'_>,
    node: crate::semantic_query::SemanticNodeId,
    counters: &mut VueMacroCodegenCounters,
) -> Result<TscSpliceText, ProjectionFailure> {
    use crate::semantic_query::SemanticNodeData;

    let context = ProjectionReductionContext::published(ProjectionMode::Navigate);
    let Some(node) = dispatch
        .normalize_node_for_structural_fact_demand(node, context)
        .into_complete_node()
    else {
        return Err(
            if crate::request_context::current_cold_compute_completeness().is_partial() {
                partial_failure()
            } else {
                resolution_failure()
            },
        );
    };
    match crate::project_semantic_dispatch::node_data_for(dispatch.ctx, node).as_deref() {
        Some(SemanticNodeData::Tuple { elements, .. }) => {
            render_tuple_parameters(ctx, elements, counters)
        }
        Some(SemanticNodeData::Function { params, .. }) => {
            render_function_parameters(ctx, params, counters)
        }
        _ => Ok(TscSpliceText::new("...args: unknown[]")),
    }
}

fn render_tuple_parameters(
    ctx: &dyn ResolverContext,
    elements: &[crate::semantic_query::TupleElement],
    counters: &mut VueMacroCodegenCounters,
) -> Result<TscSpliceText, ProjectionFailure> {
    let mut rendered = Vec::with_capacity(elements.len());
    for (index, element) in elements.iter().enumerate() {
        let ty = render_tsc_node(ctx, element.value, counters)?;
        let name = element
            .label
            .as_deref()
            .map_or_else(|| format!("arg{index}"), ToOwned::to_owned);
        rendered.push(render_tsc_parameter(
            &name,
            ty.as_str(),
            element.optional,
            element.rest,
        ));
    }
    Ok(TscSpliceText::new(format!(
        "...args: [{}]",
        rendered.join(", ")
    )))
}

fn render_function_parameters(
    ctx: &dyn ResolverContext,
    params: &[crate::semantic_query::FunctionParam],
    counters: &mut VueMacroCodegenCounters,
) -> Result<TscSpliceText, ProjectionFailure> {
    let mut rendered = Vec::with_capacity(params.len());
    for (index, param) in params.iter().enumerate() {
        let ty = render_tsc_node(ctx, param.ty, counters)?;
        let name = param
            .name
            .as_deref()
            .map_or_else(|| format!("arg{index}"), ToOwned::to_owned);
        rendered.push(render_tsc_parameter(
            &name,
            ty.as_str(),
            param.optional,
            param.rest,
        ));
    }
    Ok(TscSpliceText::new(rendered.join(", ")))
}

fn render_tsc_parameter(name: &str, ty: &str, optional: bool, rest: bool) -> String {
    format!(
        "{}{}{}: {}",
        if rest { "..." } else { "" },
        name,
        if optional && !rest { "?" } else { "" },
        ty
    )
}
