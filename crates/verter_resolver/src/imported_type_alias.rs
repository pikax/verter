use std::{collections::BTreeSet, sync::Arc};

use verter_analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
use verter_analysis::type_expand::{expand_object_shape, ExpandedObjectShape, ExpansionBudget};
use verter_analysis::type_expr::{
    FunctionExpr, FunctionParam, IndexSignature, ObjectExpr, ObjectMember, ObjectProperty,
    PrimitiveName, TypeExpr, TypeParam,
};

use crate::{ImportedSymbolDependency, ImportedTypeAliasResolveRequest};

struct PreparedLocalDeclBody {
    body: TypeExpr,
    requires_source_merge: bool,
}

#[derive(Debug, Clone)]
pub struct PreparedLocalImportedTypeAlias {
    pub decl: TypeDeclInfo,
    pub requires_source_merge: bool,
}

#[derive(Debug, Clone)]
pub struct PreparedImportedTypeAlias {
    pub local_name: String,
    pub source_canonical_id: String,
    pub exported_name: String,
    pub decl: TypeDeclInfo,
    pub symbol_dependencies: Vec<ImportedSymbolDependency>,
    pub requires_source_merge: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportedTypeAliasPrepareError {
    StepLimitExceeded {
        limit: usize,
        type_name: String,
        last_dep: String,
    },
    Other,
}

#[derive(Debug, Clone)]
pub struct CachedPreparedImportedTypeAlias {
    pub decl: TypeDeclInfo,
    pub canonical_dependencies: BTreeSet<String>,
    pub symbol_dependencies: Vec<ImportedSymbolDependency>,
    pub requires_source_merge: bool,
}

pub trait ImportedTypeAliasResolver {
    fn dependency_eval_env(&self, canonical_id: &str) -> Option<Arc<EvalEnv>>;

    fn budget_is_exhausted(&self) -> bool;

    fn set_budget_overflow(&mut self, message: String);

    fn resolve_external_type_body(
        &mut self,
        request: &ImportedTypeAliasResolveRequest,
        tracked_deps: &mut BTreeSet<String>,
        resolution_deps: &mut BTreeSet<String>,
    ) -> Result<Option<TypeExpr>, ImportedTypeAliasPrepareError>;

    fn evaluate_imported_decl_with_owner_env(
        &mut self,
        source_canonical_id: &str,
        exported_name: &str,
        canonical_dependencies: &mut BTreeSet<String>,
    ) -> Option<TypeExpr>;

    fn imported_symbol_dependencies(
        &self,
        _source_canonical_id: &str,
        _exported_name: &str,
        _decl_body: &TypeExpr,
    ) -> Vec<ImportedSymbolDependency> {
        Vec::new()
    }

    fn cached_prepared_imported_type_alias(
        &self,
        _source_canonical_id: &str,
        _exported_name: &str,
    ) -> Option<CachedPreparedImportedTypeAlias> {
        None
    }

    fn cache_prepared_imported_type_alias(
        &self,
        _source_canonical_id: &str,
        _exported_name: &str,
        _cached: CachedPreparedImportedTypeAlias,
    ) {
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn prepare_imported_type_alias<R: ImportedTypeAliasResolver>(
    resolver: &mut R,
    request: ImportedTypeAliasResolveRequest,
    canonical_dependencies: &mut BTreeSet<String>,
) -> Option<PreparedImportedTypeAlias> {
    if resolver.budget_is_exhausted() {
        return None;
    }

    let resolved_source_canonical_id = request.source_canonical_id.clone();
    let resolved_exported_name = request.exported_name.clone();

    if let Some(cached) = resolver.cached_prepared_imported_type_alias(
        &resolved_source_canonical_id,
        resolved_exported_name.as_str(),
    ) {
        canonical_dependencies.extend(cached.canonical_dependencies.iter().cloned());
        let mut decl = cached.decl;
        decl.name = request.local_name.clone();
        return Some(PreparedImportedTypeAlias {
            local_name: request.local_name,
            source_canonical_id: resolved_source_canonical_id,
            exported_name: resolved_exported_name,
            decl,
            symbol_dependencies: cached.symbol_dependencies,
            requires_source_merge: cached.requires_source_merge,
        });
    }

    let mut dep_env = resolver.dependency_eval_env(&resolved_source_canonical_id);
    let mut decl = dep_env.as_ref().and_then(|env| {
        env.type_symbols
            .get(resolved_exported_name.as_str())
            .cloned()
    });

    if let Some(decl) = decl.as_ref() {
        canonical_dependencies.insert(resolved_source_canonical_id.clone());
        let mut decl = decl.clone();
        let local_body = prepare_local_decl_body(&decl);
        let symbol_dependencies = resolver.imported_symbol_dependencies(
            &resolved_source_canonical_id,
            resolved_exported_name.as_str(),
            &local_body.body,
        );
        decl.body = local_body.body;
        let requires_source_merge = local_body.requires_source_merge;
        resolver.cache_prepared_imported_type_alias(
            &resolved_source_canonical_id,
            resolved_exported_name.as_str(),
            CachedPreparedImportedTypeAlias {
                decl: decl.clone(),
                canonical_dependencies: canonical_dependencies.clone(),
                symbol_dependencies: symbol_dependencies.clone(),
                requires_source_merge,
            },
        );
        decl.name = request.local_name.clone();
        return Some(PreparedImportedTypeAlias {
            local_name: request.local_name,
            source_canonical_id: resolved_source_canonical_id,
            exported_name: resolved_exported_name,
            decl,
            symbol_dependencies,
            requires_source_merge,
        });
    }

    let mut tracked_deps = BTreeSet::new();
    let mut resolution_deps = BTreeSet::new();
    let resolved_body = match resolver.resolve_external_type_body(
        &request,
        &mut tracked_deps,
        &mut resolution_deps,
    ) {
        Ok(resolved) => resolved,
        Err(ImportedTypeAliasPrepareError::StepLimitExceeded {
            limit,
            type_name,
            last_dep,
        }) => {
            resolver.set_budget_overflow(format!(
                "component-meta external type resolution step budget exceeded (maxSteps={}) while resolving '{}#{}' for '{}' (lastDep='{}')",
                limit,
                resolved_source_canonical_id,
                type_name,
                request.owner_canonical_id,
                last_dep,
            ));
            None
        }
        Err(ImportedTypeAliasPrepareError::Other) => None,
    };

    let resolved_decl_body = match decl.as_ref() {
        Some(decl) if should_attempt_owner_env_resolution(decl, resolved_body.as_ref()) => resolver
            .evaluate_imported_decl_with_owner_env(
                &resolved_source_canonical_id,
                resolved_exported_name.as_str(),
                canonical_dependencies,
            ),
        None => resolver.evaluate_imported_decl_with_owner_env(
            &resolved_source_canonical_id,
            resolved_exported_name.as_str(),
            canonical_dependencies,
        ),
        _ => None,
    };

    if decl.is_none() {
        let body =
            choose_preferred_imported_type_body(resolved_body.clone(), resolved_decl_body.clone())?;
        decl = Some(TypeDeclInfo {
            name: resolved_exported_name.clone(),
            declaration_id: 0,
            kind: TypeDeclKind::Alias,
            type_parameters: Vec::new(),
            body,
        });
        dep_env.get_or_insert_with(|| Arc::new(EvalEnv::new()));
    }

    let mut dep_env = dep_env.as_deref().cloned().unwrap_or_default();
    let mut decl = decl.expect("decl must exist after synthesized fallback");
    let symbol_dependencies = resolver.imported_symbol_dependencies(
        &resolved_source_canonical_id,
        resolved_exported_name.as_str(),
        &decl.body,
    );

    canonical_dependencies.extend(tracked_deps);
    canonical_dependencies.extend(resolution_deps);
    canonical_dependencies.insert(resolved_source_canonical_id.clone());

    if resolver.budget_is_exhausted() {
        return None;
    }

    let body_has_structural_extends = body_has_structural_intersection_refs(&decl.body);
    let decl_materialized_body = if body_has_structural_extends {
        materialize_imported_type_body(&decl.body, &dep_env, &decl.type_parameters)
    } else {
        None
    };
    let preferred_body = choose_preferred_imported_type_body(
        choose_preferred_imported_type_body(resolved_body.clone(), resolved_decl_body.clone()),
        decl_materialized_body.clone(),
    );
    let selected_body = if body_has_structural_extends && decl_materialized_body.is_some() {
        choose_preferred_imported_type_body(decl_materialized_body.clone(), preferred_body.clone())
            .or(decl_materialized_body.clone())
            .or(preferred_body.clone())
    } else {
        choose_preferred_imported_type_body(Some(decl.body.clone()), preferred_body.clone())
            .or(preferred_body.clone())
    };
    let requires_source_merge = if body_has_structural_extends {
        match selected_body.as_ref() {
            Some(body) => {
                is_empty_object_surface(body)
                    || has_non_object_top_level_surface(body)
                    || contains_nested_resolution_targets(body)
            }
            None => true,
        }
    } else {
        selected_body.is_none()
    };

    if body_has_structural_extends && requires_source_merge {
        // Keep the raw intersection body for later source-merge-backed evaluation.
    } else if let Some(body) = selected_body {
        let mut normalized_env = dep_env.clone();
        for param in &decl.type_parameters {
            normalized_env.type_bindings.insert(
                param.name.clone(),
                std::sync::Arc::new(TypeExpr::type_parameter(param.clone())),
            );
        }
        let normalized_body = verter_analysis::type_eval::evaluate(&body, &mut normalized_env);
        decl.body = choose_preferred_imported_type_body(Some(body), Some(normalized_body))
            .expect("preferred imported type body should exist");
    } else {
        for param in &decl.type_parameters {
            dep_env.type_bindings.insert(
                param.name.clone(),
                std::sync::Arc::new(TypeExpr::type_parameter(param.clone())),
            );
        }
        decl.body = verter_analysis::type_eval::evaluate(&decl.body, &mut dep_env);
    }
    resolver.cache_prepared_imported_type_alias(
        &resolved_source_canonical_id,
        resolved_exported_name.as_str(),
        CachedPreparedImportedTypeAlias {
            decl: decl.clone(),
            canonical_dependencies: canonical_dependencies.clone(),
            symbol_dependencies: symbol_dependencies.clone(),
            requires_source_merge,
        },
    );
    decl.name = request.local_name.clone();

    Some(PreparedImportedTypeAlias {
        local_name: request.local_name,
        source_canonical_id: resolved_source_canonical_id,
        exported_name: resolved_exported_name,
        decl,
        symbol_dependencies,
        requires_source_merge,
    })
}

fn prepare_local_decl_body(decl: &TypeDeclInfo) -> PreparedLocalDeclBody {
    let body = normalize_local_decl_body(&decl.body, &decl.type_parameters);
    PreparedLocalDeclBody {
        requires_source_merge: contains_nested_resolution_targets(&body),
        body,
    }
}

pub fn prepare_local_imported_type_alias(decl: &TypeDeclInfo) -> PreparedLocalImportedTypeAlias {
    let local_body = prepare_local_decl_body(decl);
    let mut prepared = decl.clone();
    prepared.body = local_body.body;
    PreparedLocalImportedTypeAlias {
        decl: prepared,
        requires_source_merge: local_body.requires_source_merge,
    }
}

fn normalize_local_decl_body(expr: &TypeExpr, type_parameters: &[TypeParam]) -> TypeExpr {
    if type_parameters.is_empty() {
        return expr.clone();
    }

    let type_param_names = type_parameters
        .iter()
        .map(|param| (param.name.as_str(), param))
        .collect::<std::collections::HashMap<_, _>>();
    normalize_type_parameter_refs(expr, &type_param_names)
}

fn normalize_type_parameter_refs(
    expr: &TypeExpr,
    type_parameters: &std::collections::HashMap<&str, &TypeParam>,
) -> TypeExpr {
    match expr {
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => type_parameters
            .get(name.as_ref())
            .map(|param| TypeExpr::type_parameter((*param).clone()))
            .unwrap_or_else(|| expr.clone()),
        TypeExpr::Parenthesized(inner) | TypeExpr::KeyOf(inner) | TypeExpr::Rest(inner) => {
            let normalized = normalize_type_parameter_refs(inner, type_parameters);
            match expr {
                TypeExpr::Parenthesized(_) => TypeExpr::Parenthesized(Arc::new(normalized)),
                TypeExpr::KeyOf(_) => TypeExpr::KeyOf(Arc::new(normalized)),
                TypeExpr::Rest(_) => TypeExpr::Rest(Arc::new(normalized)),
                _ => unreachable!(),
            }
        }
        TypeExpr::Array { element, readonly } => TypeExpr::Array {
            element: Arc::new(normalize_type_parameter_refs(element, type_parameters)),
            readonly: *readonly,
        },
        TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
            elements: Arc::from(
                elements
                    .iter()
                    .map(|element| verter_analysis::type_expr::TupleElement {
                        label: element.label.clone(),
                        ty: normalize_type_parameter_refs(&element.ty, type_parameters),
                        optional: element.optional,
                        rest: element.rest,
                    })
                    .collect::<Vec<_>>(),
            ),
            readonly: *readonly,
        },
        TypeExpr::Union(types) => TypeExpr::Union(Arc::from(
            types
                .iter()
                .map(|ty| normalize_type_parameter_refs(ty, type_parameters))
                .collect::<Vec<_>>(),
        )),
        TypeExpr::Intersection(types) => TypeExpr::Intersection(Arc::from(
            types
                .iter()
                .map(|ty| normalize_type_parameter_refs(ty, type_parameters))
                .collect::<Vec<_>>(),
        )),
        TypeExpr::Object(obj) => TypeExpr::Object(Arc::new(ObjectExpr {
            properties: obj
                .properties
                .iter()
                .map(|member| match member {
                    ObjectMember::Property(prop) => ObjectMember::Property(ObjectProperty {
                        name: prop.name.clone(),
                        ty: normalize_type_parameter_refs(&prop.ty, type_parameters),
                        optional: prop.optional,
                        readonly: prop.readonly,
                    }),
                    ObjectMember::IndexSignature(sig) => {
                        ObjectMember::IndexSignature(IndexSignature {
                            key_name: sig.key_name.clone(),
                            key_type: normalize_type_parameter_refs(&sig.key_type, type_parameters),
                            value_type: normalize_type_parameter_refs(
                                &sig.value_type,
                                type_parameters,
                            ),
                            readonly: sig.readonly,
                        })
                    }
                    ObjectMember::CallSignature(func) => {
                        ObjectMember::CallSignature(normalize_function_expr(func, type_parameters))
                    }
                    ObjectMember::ConstructSignature(func) => ObjectMember::ConstructSignature(
                        normalize_function_expr(func, type_parameters),
                    ),
                    ObjectMember::Method(method) => {
                        ObjectMember::Method(verter_analysis::type_expr::MethodSignature {
                            name: method.name.clone(),
                            function: normalize_function_expr(&method.function, type_parameters),
                            optional: method.optional,
                        })
                    }
                })
                .collect(),
        })),
        TypeExpr::Function(func) => {
            TypeExpr::Function(Arc::new(normalize_function_expr(func, type_parameters)))
        }
        TypeExpr::Ref {
            name,
            type_arguments,
        } => TypeExpr::Ref {
            name: name.clone(),
            type_arguments: Arc::from(
                type_arguments
                    .iter()
                    .map(|arg| normalize_type_parameter_refs(arg, type_parameters))
                    .collect::<Vec<_>>(),
            ),
        },
        TypeExpr::IndexedAccess { object, index } => TypeExpr::IndexedAccess {
            object: Arc::new(normalize_type_parameter_refs(object, type_parameters)),
            index: Arc::new(normalize_type_parameter_refs(index, type_parameters)),
        },
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => TypeExpr::Conditional {
            check: Arc::new(normalize_type_parameter_refs(check, type_parameters)),
            extends: Arc::new(normalize_type_parameter_refs(extends, type_parameters)),
            true_type: Arc::new(normalize_type_parameter_refs(true_type, type_parameters)),
            false_type: Arc::new(normalize_type_parameter_refs(false_type, type_parameters)),
        },
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } => TypeExpr::Mapped {
            parameter: parameter.clone(),
            source: Arc::new(normalize_type_parameter_refs(source, type_parameters)),
            value: Arc::new(normalize_type_parameter_refs(value, type_parameters)),
            optional: *optional,
            readonly: *readonly,
            name_type: name_type.as_ref().map(|name_type| {
                Arc::new(normalize_type_parameter_refs(name_type, type_parameters))
            }),
        },
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => TypeExpr::TemplateLiteral {
            quasis: quasis.clone(),
            expressions: Arc::from(
                expressions
                    .iter()
                    .map(|expr| normalize_type_parameter_refs(expr, type_parameters))
                    .collect::<Vec<_>>(),
            ),
        },
        _ => expr.clone(),
    }
}

fn normalize_function_expr(
    func: &FunctionExpr,
    type_parameters: &std::collections::HashMap<&str, &TypeParam>,
) -> FunctionExpr {
    FunctionExpr {
        parameters: func
            .parameters
            .iter()
            .map(|param| FunctionParam {
                name: param.name.clone(),
                ty: normalize_type_parameter_refs(&param.ty, type_parameters),
                optional: param.optional,
                rest: param.rest,
            })
            .collect(),
        return_type: func.return_type.as_ref().map(|return_type| {
            Arc::new(normalize_type_parameter_refs(return_type, type_parameters))
        }),
        type_parameters: func.type_parameters.clone(),
    }
}

fn materialize_imported_type_body(
    body: &TypeExpr,
    dep_env: &EvalEnv,
    type_parameters: &[TypeParam],
) -> Option<TypeExpr> {
    let mut eval_env = dep_env.clone();
    for param in type_parameters {
        eval_env.type_bindings.insert(
            param.name.clone(),
            Arc::new(TypeExpr::type_parameter(param.clone())),
        );
    }
    let evaluated_body = verter_analysis::type_eval::evaluate(body, &mut eval_env);
    let mut expansion_env = eval_env.clone();
    let expanded = expand_object_shape(
        &evaluated_body,
        &mut expansion_env,
        &ExpansionBudget::default(),
    );
    let materialized_body = expanded_object_shape_to_type_expr(&expanded.value)?;
    choose_preferred_imported_type_body(Some(evaluated_body), Some(materialized_body))
}

fn expanded_object_shape_to_type_expr(shape: &ExpandedObjectShape) -> Option<TypeExpr> {
    if shape.properties.is_empty()
        && shape.index_signatures.is_empty()
        && shape.call_signatures.is_empty()
    {
        return None;
    }

    let mut properties = Vec::with_capacity(
        shape.properties.len() + shape.index_signatures.len() + shape.call_signatures.len(),
    );

    for prop in &shape.properties {
        properties.push(ObjectMember::Property(ObjectProperty {
            name: prop.name.clone(),
            ty: prop.ty.clone(),
            optional: prop.optional,
            readonly: prop.readonly,
        }));
    }
    for sig in &shape.index_signatures {
        properties.push(ObjectMember::IndexSignature(IndexSignature {
            key_name: "key".to_string(),
            key_type: sig.key_type.clone(),
            value_type: sig.value_type.clone(),
            readonly: sig.readonly,
        }));
    }
    for sig in &shape.call_signatures {
        properties.push(ObjectMember::CallSignature(FunctionExpr {
            parameters: sig
                .parameters
                .iter()
                .map(|param| FunctionParam {
                    name: Some(param.name.clone()),
                    ty: param.ty.clone(),
                    optional: param.optional,
                    rest: param.rest,
                })
                .collect(),
            return_type: Some(Arc::new(sig.return_type.clone())),
            type_parameters: sig.type_parameters.clone(),
        }));
    }

    Some(TypeExpr::Object(Arc::new(ObjectExpr { properties })))
}

fn body_has_structural_intersection_refs(body: &TypeExpr) -> bool {
    match body {
        TypeExpr::Intersection(parts) => parts
            .iter()
            .any(|part| matches!(part, TypeExpr::Ref { .. })),
        _ => false,
    }
}

pub fn choose_preferred_imported_type_body(
    resolved_body: Option<TypeExpr>,
    resolved_decl_body: Option<TypeExpr>,
) -> Option<TypeExpr> {
    match (resolved_body, resolved_decl_body) {
        (Some(left), Some(right)) => {
            let left_empty_object = is_empty_object_surface(&left);
            let right_empty_object = is_empty_object_surface(&right);
            if left_empty_object != right_empty_object {
                return Some(if left_empty_object { right } else { left });
            }

            let left_surface_props = extracted_surface_property_count(&left);
            let right_surface_props = extracted_surface_property_count(&right);
            if let (Some(left_count), Some(right_count)) = (left_surface_props, right_surface_props)
            {
                if left_count != right_count {
                    return Some(if left_count > right_count {
                        left
                    } else {
                        right
                    });
                }
            }

            let left_top_level_branching = top_level_branching_surface_score(&left);
            let right_top_level_branching = top_level_branching_surface_score(&right);
            if left_top_level_branching != right_top_level_branching {
                return Some(if left_top_level_branching > right_top_level_branching {
                    left
                } else {
                    right
                });
            }

            let left_nested = contains_nested_resolution_targets(&left);
            let right_nested = contains_nested_resolution_targets(&right);
            if left_nested != right_nested {
                return Some(if left_nested { right } else { left });
            }

            let left_non_object = has_non_object_top_level_surface(&left);
            let right_non_object = has_non_object_top_level_surface(&right);
            if left_non_object != right_non_object {
                return Some(if left_non_object { right } else { left });
            }

            if imported_type_body_specificity_score(&right)
                > imported_type_body_specificity_score(&left)
            {
                Some(right)
            } else {
                Some(left)
            }
        }
        (Some(body), None) | (None, Some(body)) => Some(body),
        (None, None) => None,
    }
}

pub fn should_attempt_owner_env_resolution(
    decl: &TypeDeclInfo,
    resolved_body: Option<&TypeExpr>,
) -> bool {
    let Some(resolved_body) = resolved_body else {
        return true;
    };

    if is_empty_object_surface(resolved_body) && !is_empty_object_surface(&decl.body) {
        return true;
    }

    if has_non_object_top_level_surface(resolved_body) {
        return true;
    }

    if contains_nested_resolution_targets(resolved_body) {
        return true;
    }

    if contains_nested_resolution_targets(&decl.body) {
        return true;
    }

    if !has_non_object_top_level_surface(&decl.body) {
        return false;
    }

    count_top_level_properties(resolved_body) <= count_top_level_properties(&decl.body)
}

fn has_non_object_top_level_surface(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Parenthesized(inner) => has_non_object_top_level_surface(inner),
        TypeExpr::Intersection(types) | TypeExpr::Union(types) => {
            types.iter().any(has_non_object_top_level_surface)
                || types.iter().any(|ty| !matches!(ty, TypeExpr::Object(_)))
        }
        TypeExpr::Ref { .. }
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. } => true,
        TypeExpr::Object(_) => false,
        _ => false,
    }
}

fn is_empty_object_surface(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Parenthesized(inner) => is_empty_object_surface(inner),
        TypeExpr::Object(obj) => obj.properties.is_empty(),
        _ => false,
    }
}

fn contains_nested_resolution_targets(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::TypeParameter(_) => false,
        TypeExpr::Ref { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. } => true,
        TypeExpr::Parenthesized(inner)
        | TypeExpr::Array { element: inner, .. }
        | TypeExpr::KeyOf(inner)
        | TypeExpr::Rest(inner) => contains_nested_resolution_targets(inner),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .any(|element| contains_nested_resolution_targets(&element.ty)),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            types.iter().any(contains_nested_resolution_targets)
        }
        TypeExpr::Object(_) => false,
        TypeExpr::Function(_) => false,
        TypeExpr::TemplateLiteral { expressions, .. } => {
            expressions.iter().any(contains_nested_resolution_targets)
        }
        TypeExpr::Infer { .. } => false,
    }
}

fn count_top_level_properties(expr: &TypeExpr) -> usize {
    match expr {
        TypeExpr::Parenthesized(inner) => count_top_level_properties(inner),
        TypeExpr::Intersection(types) | TypeExpr::Union(types) => {
            types.iter().map(count_top_level_properties).sum()
        }
        TypeExpr::Object(obj) => obj
            .properties
            .iter()
            .filter(|member| matches!(member, ObjectMember::Property(_) | ObjectMember::Method(_)))
            .count(),
        _ => 0,
    }
}

fn extracted_surface_property_count(expr: &TypeExpr) -> Option<usize> {
    match expr {
        TypeExpr::Parenthesized(inner) => extracted_surface_property_count(inner),
        TypeExpr::Object(obj) => Some(
            obj.properties
                .iter()
                .filter(|member| {
                    matches!(member, ObjectMember::Property(_) | ObjectMember::Method(_))
                })
                .count(),
        ),
        TypeExpr::Intersection(types) => {
            let mut total = 0usize;
            let mut saw_surface = false;
            for ty in types.iter() {
                let count = extracted_surface_property_count(ty)?;
                total += count;
                saw_surface = true;
            }
            saw_surface.then_some(total)
        }
        _ => None,
    }
}

fn top_level_branching_surface_score(expr: &TypeExpr) -> usize {
    match expr {
        TypeExpr::Parenthesized(inner) => top_level_branching_surface_score(inner),
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            let mut score = 0usize;
            for ty in types.iter() {
                match ty {
                    TypeExpr::Primitive(PrimitiveName::Undefined) => {}
                    TypeExpr::Unknown { .. } => {}
                    _ => score += 1,
                }
            }
            if score >= 2 {
                score
            } else {
                0
            }
        }
        _ => 0,
    }
}

const SPECIFICITY_UNKNOWN: usize = 0;
const SPECIFICITY_TYPEOF: usize = 4;
const SPECIFICITY_TERMINAL: usize = 8;
const SPECIFICITY_REF_BASE: usize = 16;
const SPECIFICITY_TEMPLATE_LITERAL_BASE: usize = 20;
const SPECIFICITY_WRAPPER_BASE: usize = 24;
const SPECIFICITY_INDEXED_ACCESS_BASE: usize = 28;
const SPECIFICITY_MAPPED_BASE: usize = 32;
const SPECIFICITY_TUPLE_BASE: usize = 40;
const SPECIFICITY_FUNCTION_BASE: usize = 48;
const SPECIFICITY_UNION_BASE: usize = 56;
const SPECIFICITY_INTERSECTION_BASE: usize = 64;
const SPECIFICITY_OBJECT_BASE: usize = 96;
const SPECIFICITY_OBJECT_PROPERTY: usize = 12;
const SPECIFICITY_INDEX_SIGNATURE: usize = 6;
const SPECIFICITY_CALL_LIKE_MEMBER: usize = 10;

pub fn imported_type_body_specificity_score(expr: &TypeExpr) -> usize {
    match expr {
        TypeExpr::Unknown { .. } => SPECIFICITY_UNKNOWN,
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) => SPECIFICITY_TERMINAL,
        TypeExpr::TypeOf(_) => SPECIFICITY_TYPEOF,
        TypeExpr::TypeParameter(param) => {
            SPECIFICITY_REF_BASE
                + param
                    .constraint
                    .as_deref()
                    .map(imported_type_body_specificity_score)
                    .unwrap_or_default()
                + param
                    .default
                    .as_deref()
                    .map(imported_type_body_specificity_score)
                    .unwrap_or_default()
        }
        TypeExpr::Ref { type_arguments, .. } => {
            SPECIFICITY_REF_BASE
                + type_arguments
                    .iter()
                    .map(imported_type_body_specificity_score)
                    .sum::<usize>()
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element) => {
            SPECIFICITY_WRAPPER_BASE + imported_type_body_specificity_score(element)
        }
        TypeExpr::Tuple { elements, .. } => {
            SPECIFICITY_TUPLE_BASE
                + elements
                    .iter()
                    .map(|element| imported_type_body_specificity_score(&element.ty))
                    .sum::<usize>()
        }
        TypeExpr::Union(types) => {
            SPECIFICITY_UNION_BASE
                + types
                    .iter()
                    .map(imported_type_body_specificity_score)
                    .sum::<usize>()
        }
        TypeExpr::Intersection(types) => {
            SPECIFICITY_INTERSECTION_BASE
                + types
                    .iter()
                    .map(imported_type_body_specificity_score)
                    .sum::<usize>()
        }
        TypeExpr::Object(obj) => {
            SPECIFICITY_OBJECT_BASE
                + obj
                    .properties
                    .iter()
                    .map(|member| match member {
                        ObjectMember::Property(prop) => {
                            SPECIFICITY_OBJECT_PROPERTY
                                + imported_type_body_specificity_score(&prop.ty)
                        }
                        ObjectMember::IndexSignature(sig) => {
                            SPECIFICITY_INDEX_SIGNATURE
                                + imported_type_body_specificity_score(&sig.key_type)
                                + imported_type_body_specificity_score(&sig.value_type)
                        }
                        ObjectMember::CallSignature(func)
                        | ObjectMember::ConstructSignature(func) => {
                            SPECIFICITY_CALL_LIKE_MEMBER + imported_function_specificity_score(func)
                        }
                        ObjectMember::Method(method) => {
                            SPECIFICITY_CALL_LIKE_MEMBER
                                + imported_function_specificity_score(&method.function)
                        }
                    })
                    .sum::<usize>()
        }
        TypeExpr::Function(func) => {
            SPECIFICITY_FUNCTION_BASE + imported_function_specificity_score(func)
        }
        TypeExpr::IndexedAccess { object, index } => {
            SPECIFICITY_INDEXED_ACCESS_BASE
                + imported_type_body_specificity_score(object)
                + imported_type_body_specificity_score(index)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            SPECIFICITY_WRAPPER_BASE
                + imported_type_body_specificity_score(check)
                + imported_type_body_specificity_score(extends)
                + imported_type_body_specificity_score(true_type)
                + imported_type_body_specificity_score(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            SPECIFICITY_MAPPED_BASE
                + imported_type_body_specificity_score(source)
                + imported_type_body_specificity_score(value)
                + name_type
                    .as_deref()
                    .map(imported_type_body_specificity_score)
                    .unwrap_or_default()
        }
        TypeExpr::TemplateLiteral { expressions, .. } => {
            SPECIFICITY_TEMPLATE_LITERAL_BASE
                + expressions
                    .iter()
                    .map(imported_type_body_specificity_score)
                    .sum::<usize>()
        }
        TypeExpr::Infer { .. } => SPECIFICITY_TYPEOF,
    }
}

fn imported_function_specificity_score(func: &FunctionExpr) -> usize {
    let params = func
        .parameters
        .iter()
        .map(|param| imported_type_body_specificity_score(&param.ty))
        .sum::<usize>();
    let ret = func
        .return_type
        .as_deref()
        .map(imported_type_body_specificity_score)
        .unwrap_or_default();
    let generics = func
        .type_parameters
        .iter()
        .map(|param| {
            param
                .constraint
                .as_deref()
                .map(imported_type_body_specificity_score)
                .unwrap_or_default()
                + param
                    .default
                    .as_deref()
                    .map(imported_type_body_specificity_score)
                    .unwrap_or_default()
        })
        .sum::<usize>();
    params + ret + generics
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashMap;
    use std::sync::Arc;
    use verter_analysis::type_eval::{DeclarationId, TypeDeclKind};
    use verter_analysis::type_expr::{
        ObjectExpr, ObjectProperty, PrimitiveName, TypeExpr, TypeParam,
    };

    #[derive(Default)]
    struct TestResolver {
        envs: FxHashMap<String, EvalEnv>,
        resolved_body: Option<TypeExpr>,
        owner_env_body: Option<TypeExpr>,
        owner_env_resolution_calls: usize,
        symbol_dependencies: FxHashMap<(String, String), Vec<ImportedSymbolDependency>>,
        error: Option<ImportedTypeAliasPrepareError>,
        overflow_message: Option<String>,
        external_resolution_calls: usize,
    }

    impl ImportedTypeAliasResolver for TestResolver {
        fn dependency_eval_env(&self, canonical_id: &str) -> Option<Arc<EvalEnv>> {
            self.envs.get(canonical_id).cloned().map(Arc::new)
        }

        fn budget_is_exhausted(&self) -> bool {
            self.overflow_message.is_some()
        }

        fn set_budget_overflow(&mut self, message: String) {
            self.overflow_message.get_or_insert(message);
        }

        fn resolve_external_type_body(
            &mut self,
            _request: &ImportedTypeAliasResolveRequest,
            tracked_deps: &mut BTreeSet<String>,
            resolution_deps: &mut BTreeSet<String>,
        ) -> Result<Option<TypeExpr>, ImportedTypeAliasPrepareError> {
            self.external_resolution_calls += 1;
            tracked_deps.insert("/deps/tracked.ts".to_string());
            resolution_deps.insert("/deps/resolved.ts".to_string());
            if let Some(error) = self.error.clone() {
                Err(error)
            } else {
                Ok(self.resolved_body.clone())
            }
        }

        fn evaluate_imported_decl_with_owner_env(
            &mut self,
            _source_canonical_id: &str,
            _exported_name: &str,
            _canonical_dependencies: &mut BTreeSet<String>,
        ) -> Option<TypeExpr> {
            self.owner_env_resolution_calls += 1;
            self.owner_env_body.clone()
        }

        fn imported_symbol_dependencies(
            &self,
            source_canonical_id: &str,
            exported_name: &str,
            _decl_body: &TypeExpr,
        ) -> Vec<ImportedSymbolDependency> {
            self.symbol_dependencies
                .get(&(source_canonical_id.to_string(), exported_name.to_string()))
                .cloned()
                .unwrap_or_default()
        }
    }

    fn object_with_string_prop(name: &str) -> TypeExpr {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: name.to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            })],
        }))
    }

    fn object_with_named_prop(name: &str, ty_name: &str) -> TypeExpr {
        TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: name.to_string(),
                ty: TypeExpr::named(ty_name),
                optional: false,
                readonly: false,
            })],
        }))
    }

    fn env_with_decl(name: &str, body: TypeExpr) -> EvalEnv {
        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: name.to_string(),
            declaration_id: DeclarationId::default(),
            kind: TypeDeclKind::Alias,
            type_parameters: vec![],
            body,
        });
        env
    }

    fn generic_type_param(name: &str) -> TypeParam {
        TypeParam {
            name: name.to_string(),
            constraint: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Number))),
            default: Some(Arc::new(TypeExpr::Primitive(PrimitiveName::String))),
        }
    }

    fn imported_symbol_dependency(
        local_name: &str,
        canonical_id: &str,
        exported_name: &str,
    ) -> ImportedSymbolDependency {
        ImportedSymbolDependency {
            local_name: local_name.to_string(),
            canonical_id: canonical_id.to_string(),
            exported_name: exported_name.to_string(),
        }
    }

    #[test]
    fn prepare_imported_type_alias_marks_structural_extends_for_source_merge() {
        let request = ImportedTypeAliasResolveRequest {
            owner_canonical_id: "/src/App.vue".to_string(),
            import_source: "./types".to_string(),
            local_name: "LocalProps".to_string(),
            imported_name: "ImportedProps".to_string(),
            source_canonical_id: "/src/types.ts".to_string(),
            exported_name: "ImportedProps".to_string(),
        };
        let mut deps = BTreeSet::new();
        let mut resolver = TestResolver::default();
        resolver.envs.insert(
            "/src/types.ts".to_string(),
            env_with_decl(
                "ImportedProps",
                TypeExpr::intersection(vec![
                    TypeExpr::named("BaseProps"),
                    TypeExpr::named("OtherProps"),
                ]),
            ),
        );

        let actual = prepare_imported_type_alias(&mut resolver, request, &mut deps).unwrap();

        assert!(actual.requires_source_merge);
        assert!(deps.contains("/src/types.ts"));
        assert!(!deps.contains("/deps/tracked.ts"));
        assert!(!deps.contains("/deps/resolved.ts"));
    }

    #[test]
    fn prepare_imported_type_alias_skips_external_resolution_for_self_contained_decl_body() {
        let request = ImportedTypeAliasResolveRequest {
            owner_canonical_id: "/src/App.vue".to_string(),
            import_source: "./types".to_string(),
            local_name: "LocalProps".to_string(),
            imported_name: "ImportedProps".to_string(),
            source_canonical_id: "/src/types.ts".to_string(),
            exported_name: "ImportedProps".to_string(),
        };
        let mut deps = BTreeSet::new();
        let mut resolver = TestResolver::default();
        resolver.envs.insert(
            "/src/types.ts".to_string(),
            env_with_decl("ImportedProps", object_with_string_prop("from_decl")),
        );
        resolver.resolved_body = Some(object_with_string_prop("from_external"));

        let actual = prepare_imported_type_alias(&mut resolver, request, &mut deps).unwrap();

        assert_eq!(
            resolver.external_resolution_calls, 0,
            "self-contained local declaration bodies should not pay the external resolution path"
        );
        let TypeExpr::Object(obj) = actual.decl.body else {
            panic!("expected object body");
        };
        let ObjectMember::Property(prop) = &obj.properties[0] else {
            panic!("expected property");
        };
        assert_eq!(prop.name, "from_decl");
        assert!(!deps.contains("/deps/tracked.ts"));
        assert!(!deps.contains("/deps/resolved.ts"));
    }

    #[test]
    fn prepare_imported_type_alias_keeps_imported_symbol_lookup_lazy_and_skips_owner_env_resolution(
    ) {
        let request = ImportedTypeAliasResolveRequest {
            owner_canonical_id: "/src/App.vue".to_string(),
            import_source: "./types".to_string(),
            local_name: "LocalProps".to_string(),
            imported_name: "ImportedProps".to_string(),
            source_canonical_id: "/src/types.ts".to_string(),
            exported_name: "ImportedProps".to_string(),
        };
        let mut deps = BTreeSet::new();
        let mut resolver = TestResolver::default();
        resolver.envs.insert(
            "/src/types.ts".to_string(),
            env_with_decl("ImportedProps", TypeExpr::named("FallbackRef")),
        );
        resolver.symbol_dependencies.insert(
            ("/src/types.ts".to_string(), "ImportedProps".to_string()),
            vec![imported_symbol_dependency(
                "FallbackRef",
                "/src/fallback.ts",
                "FallbackRef",
            )],
        );

        let actual = prepare_imported_type_alias(&mut resolver, request, &mut deps).unwrap();

        assert_eq!(resolver.external_resolution_calls, 0);
        assert_eq!(resolver.owner_env_resolution_calls, 0);
        assert!(actual.requires_source_merge);
        assert_eq!(actual.decl.body, TypeExpr::named("FallbackRef"));
        assert_eq!(
            actual.symbol_dependencies,
            vec![imported_symbol_dependency(
                "FallbackRef",
                "/src/fallback.ts",
                "FallbackRef",
            )]
        );
    }

    #[test]
    fn prepare_imported_type_alias_keeps_local_object_surface_with_nested_refs_shallow() {
        let request = ImportedTypeAliasResolveRequest {
            owner_canonical_id: "/src/App.vue".to_string(),
            import_source: "./types".to_string(),
            local_name: "LocalProps".to_string(),
            imported_name: "ImportedProps".to_string(),
            source_canonical_id: "/src/types.ts".to_string(),
            exported_name: "ImportedProps".to_string(),
        };
        let mut deps = BTreeSet::new();
        let mut resolver = TestResolver::default();
        resolver.envs.insert(
            "/src/types.ts".to_string(),
            env_with_decl("ImportedProps", object_with_named_prop("item", "NestedRef")),
        );

        let actual = prepare_imported_type_alias(&mut resolver, request, &mut deps).unwrap();

        assert_eq!(
            resolver.external_resolution_calls, 0,
            "object-shaped local symbol bodies with nested refs should stay lazy instead of forcing external resolution"
        );
        assert!(
            !actual.requires_source_merge,
            "plain object member refs should stay shallow instead of forcing source-merge follow-up"
        );
        let TypeExpr::Object(obj) = actual.decl.body else {
            panic!("expected object body");
        };
        let ObjectMember::Property(prop) = &obj.properties[0] else {
            panic!("expected property");
        };
        assert_eq!(prop.name, "item");
    }

    #[test]
    fn prepare_imported_type_alias_keeps_structural_local_surface_lazy_when_materialization_fails()
    {
        let request = ImportedTypeAliasResolveRequest {
            owner_canonical_id: "/src/App.vue".to_string(),
            import_source: "./types".to_string(),
            local_name: "LocalProps".to_string(),
            imported_name: "ImportedProps".to_string(),
            source_canonical_id: "/src/types.ts".to_string(),
            exported_name: "ImportedProps".to_string(),
        };
        let mut deps = BTreeSet::new();
        let mut resolver = TestResolver::default();
        resolver.envs.insert(
            "/src/types.ts".to_string(),
            env_with_decl(
                "ImportedProps",
                TypeExpr::intersection(vec![
                    TypeExpr::named("BaseProps"),
                    object_with_string_prop("current"),
                ]),
            ),
        );

        let actual = prepare_imported_type_alias(&mut resolver, request, &mut deps).unwrap();

        assert_eq!(
            resolver.external_resolution_calls, 0,
            "structural local declaration bodies with visible local members should stay lazy instead of forcing external resolution"
        );
        assert!(actual.requires_source_merge);
        assert!(deps.contains("/src/types.ts"));
        assert!(!deps.contains("/deps/tracked.ts"));
        assert!(!deps.contains("/deps/resolved.ts"));
        assert_eq!(
            actual.decl.body,
            TypeExpr::intersection(vec![
                TypeExpr::named("BaseProps"),
                object_with_string_prop("current"),
            ]),
        );
    }

    #[test]
    fn prepare_imported_type_alias_keeps_self_contained_non_object_surface_without_owner_env() {
        let request = ImportedTypeAliasResolveRequest {
            owner_canonical_id: "/src/App.vue".to_string(),
            import_source: "./types".to_string(),
            local_name: "LocalProps".to_string(),
            imported_name: "ImportedProps".to_string(),
            source_canonical_id: "/src/types.ts".to_string(),
            exported_name: "ImportedProps".to_string(),
        };
        let mut deps = BTreeSet::new();
        let mut resolver = TestResolver::default();
        resolver.envs.insert(
            "/src/types.ts".to_string(),
            env_with_decl(
                "ImportedProps",
                TypeExpr::union(vec![
                    TypeExpr::string_literal("solid"),
                    TypeExpr::string_literal("ghost"),
                ]),
            ),
        );

        let actual = prepare_imported_type_alias(&mut resolver, request, &mut deps).unwrap();

        assert_eq!(
            resolver.external_resolution_calls, 0,
            "self-contained literal unions should not pay the external or owner-env resolution paths"
        );
        assert!(!actual.requires_source_merge);
        assert!(deps.contains("/src/types.ts"));
        assert!(!deps.contains("/deps/tracked.ts"));
        assert!(!deps.contains("/deps/resolved.ts"));
    }

    #[test]
    fn prepare_imported_type_alias_marks_owner_env_nested_refs_for_source_merge() {
        let request = ImportedTypeAliasResolveRequest {
            owner_canonical_id: "/src/App.vue".to_string(),
            import_source: "./types".to_string(),
            local_name: "LocalProps".to_string(),
            imported_name: "ImportedProps".to_string(),
            source_canonical_id: "/src/types.ts".to_string(),
            exported_name: "ImportedProps".to_string(),
        };
        let mut deps = BTreeSet::new();
        let mut resolver = TestResolver::default();
        resolver.envs.insert(
            "/src/types.ts".to_string(),
            env_with_decl(
                "ImportedProps",
                TypeExpr::intersection(vec![
                    TypeExpr::named("Base"),
                    object_with_string_prop("current"),
                ]),
            ),
        );
        resolver.owner_env_body = Some(TypeExpr::intersection(vec![
            TypeExpr::named("Base"),
            object_with_string_prop("current"),
        ]));

        let actual = prepare_imported_type_alias(&mut resolver, request, &mut deps).unwrap();

        assert!(actual.requires_source_merge);
    }

    #[test]
    fn prepare_imported_type_alias_converts_step_limit_to_budget_overflow() {
        let request = ImportedTypeAliasResolveRequest {
            owner_canonical_id: "/src/App.vue".to_string(),
            import_source: "./types".to_string(),
            local_name: "LocalProps".to_string(),
            imported_name: "ImportedProps".to_string(),
            source_canonical_id: "/src/types.ts".to_string(),
            exported_name: "ImportedProps".to_string(),
        };
        let mut deps = BTreeSet::new();
        let mut resolver = TestResolver::default();
        resolver.error = Some(ImportedTypeAliasPrepareError::StepLimitExceeded {
            limit: 12,
            type_name: "ImportedProps".to_string(),
            last_dep: "/src/barrel.ts".to_string(),
        });

        let actual = prepare_imported_type_alias(&mut resolver, request, &mut deps);

        assert!(actual.is_none());
        assert!(resolver
            .overflow_message
            .as_ref()
            .is_some_and(|message| message.contains("maxSteps=12")));
    }

    #[test]
    fn choose_preferred_imported_type_body_keeps_meaningful_top_level_union_surface() {
        let flattened_object = object_with_string_prop("path");
        let symbolic_union = TypeExpr::union(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::named("St"),
            TypeExpr::named("vt"),
        ]);

        let preferred = choose_preferred_imported_type_body(
            Some(flattened_object.clone()),
            Some(symbolic_union.clone()),
        )
        .expect("preferred body should exist");

        assert_eq!(preferred, symbolic_union);
        assert_ne!(preferred, flattened_object);
    }

    #[test]
    fn prepare_imported_type_alias_preserves_generic_parameter_metadata() {
        let request = ImportedTypeAliasResolveRequest {
            owner_canonical_id: "/src/App.vue".to_string(),
            import_source: "./types".to_string(),
            local_name: "LocalProps".to_string(),
            imported_name: "ImportedProps".to_string(),
            source_canonical_id: "/src/types.ts".to_string(),
            exported_name: "ImportedProps".to_string(),
        };
        let generic = generic_type_param("T");
        let mut deps = BTreeSet::new();
        let mut resolver = TestResolver::default();
        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "ImportedProps".to_string(),
            declaration_id: DeclarationId::default(),
            kind: TypeDeclKind::Alias,
            type_parameters: vec![generic.clone()],
            body: TypeExpr::named("T"),
        });
        resolver.envs.insert("/src/types.ts".to_string(), env);

        let helper_body = materialize_imported_type_body(
            &resolver.envs["/src/types.ts"].type_symbols["ImportedProps"].body,
            &resolver.envs["/src/types.ts"],
            &[],
        );
        assert!(
            helper_body.is_none(),
            "helper should keep unconstrained generic bodies symbolic, got {:?}",
            helper_body
        );

        let actual = prepare_imported_type_alias(&mut resolver, request, &mut deps).unwrap();

        assert_eq!(actual.decl.body, TypeExpr::TypeParameter(generic));
    }

    #[test]
    fn prepare_imported_type_alias_keeps_structural_interface_heritage_symbolic() {
        let request = ImportedTypeAliasResolveRequest {
            owner_canonical_id: "/src/App.vue".to_string(),
            import_source: "./types".to_string(),
            local_name: "LocalProps".to_string(),
            imported_name: "ImportedProps".to_string(),
            source_canonical_id: "/src/types.ts".to_string(),
            exported_name: "ImportedProps".to_string(),
        };
        let mut deps = BTreeSet::new();
        let mut resolver = TestResolver::default();
        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "BaseProps".to_string(),
            declaration_id: DeclarationId::default(),
            kind: TypeDeclKind::Interface,
            type_parameters: vec![],
            body: TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "replace".to_string(),
                    ty: TypeExpr::Primitive(PrimitiveName::Boolean),
                    optional: true,
                    readonly: false,
                })],
            })),
        });
        env.add_type(TypeDeclInfo {
            name: "ImportedProps".to_string(),
            declaration_id: DeclarationId::default(),
            kind: TypeDeclKind::Interface,
            type_parameters: vec![],
            body: TypeExpr::intersection(vec![
                TypeExpr::named("BaseProps"),
                TypeExpr::Object(Arc::new(ObjectExpr {
                    properties: vec![ObjectMember::Property(ObjectProperty {
                        name: "activeClass".to_string(),
                        ty: TypeExpr::Primitive(PrimitiveName::String),
                        optional: true,
                        readonly: false,
                    })],
                })),
            ]),
        });
        resolver.envs.insert("/src/types.ts".to_string(), env);

        let actual = prepare_imported_type_alias(&mut resolver, request, &mut deps).unwrap();

        assert_eq!(resolver.external_resolution_calls, 0);
        assert_eq!(resolver.owner_env_resolution_calls, 0);
        assert!(actual.requires_source_merge);
        assert!(
            matches!(
                actual.decl.body,
                TypeExpr::Intersection(ref parts)
                    if parts.len() == 2
                        && parts[0] == TypeExpr::named("BaseProps")
                        && matches!(parts[1], TypeExpr::Object(_))
            ),
            "structural interface heritage should stay shallow and symbolic, got {:?}",
            actual.decl.body
        );
    }

    #[test]
    fn prepare_imported_type_alias_uses_resolved_root_export_name_for_cached_env_lookup() {
        let request = ImportedTypeAliasResolveRequest {
            owner_canonical_id: "/src/App.vue".to_string(),
            import_source: "./barrel".to_string(),
            local_name: "LocalProps".to_string(),
            imported_name: "PublicProps".to_string(),
            source_canonical_id: "/src/types.ts".to_string(),
            exported_name: "InternalProps".to_string(),
        };
        let mut deps = BTreeSet::new();
        let mut resolver = TestResolver::default();
        resolver.envs.insert(
            "/src/types.ts".to_string(),
            env_with_decl("InternalProps", object_with_string_prop("from_decl")),
        );
        resolver.resolved_body = Some(object_with_string_prop("from_external"));

        let actual = prepare_imported_type_alias(&mut resolver, request, &mut deps).unwrap();

        assert_eq!(
            resolver.external_resolution_calls, 0,
            "resolved root export names should hit the defining file env instead of falling back to external type resolution"
        );
        assert_eq!(actual.source_canonical_id, "/src/types.ts");
        assert_eq!(actual.exported_name, "InternalProps");
        let TypeExpr::Object(obj) = actual.decl.body else {
            panic!("expected object body");
        };
        let ObjectMember::Property(prop) = &obj.properties[0] else {
            panic!("expected property");
        };
        assert_eq!(prop.name, "from_decl");
    }
}
