use std::collections::BTreeSet;

use verter_analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
use verter_analysis::type_expr::{FunctionExpr, ObjectMember, TypeExpr};

use crate::{ImportedTypeAlias, ImportedTypeAliasResolveRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportedTypeAliasPrepareError {
    StepLimitExceeded {
        limit: usize,
        type_name: String,
        last_dep: String,
    },
    Other,
}

pub trait ImportedTypeAliasResolver {
    fn canonicalize_imported_source(&self, source_canonical_id: &str) -> String;

    fn dependency_eval_env(&self, canonical_id: &str) -> Option<EvalEnv>;

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
}

pub fn prepare_imported_type_alias<R: ImportedTypeAliasResolver>(
    resolver: &mut R,
    request: ImportedTypeAliasResolveRequest,
    canonical_dependencies: &mut BTreeSet<String>,
) -> Option<ImportedTypeAlias> {
    if resolver.budget_is_exhausted() {
        return None;
    }

    let resolved_source_canonical_id =
        resolver.canonicalize_imported_source(request.source_canonical_id.as_str());

    let mut dep_env = resolver.dependency_eval_env(&resolved_source_canonical_id);
    let mut decl = dep_env.as_ref().and_then(|env| {
        env.type_symbols
            .get(request.exported_name.as_str())
            .cloned()
    });

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
                request.exported_name.as_str(),
                canonical_dependencies,
            ),
        None => resolver.evaluate_imported_decl_with_owner_env(
            &resolved_source_canonical_id,
            request.exported_name.as_str(),
            canonical_dependencies,
        ),
        _ => None,
    };

    if decl.is_none() {
        let body =
            choose_preferred_imported_type_body(resolved_body.clone(), resolved_decl_body.clone())?;
        decl = Some(TypeDeclInfo {
            name: request.exported_name.clone(),
            declaration_id: 0,
            kind: TypeDeclKind::Alias,
            type_parameters: Vec::new(),
            body,
        });
        dep_env.get_or_insert_with(EvalEnv::new);
    }

    let mut dep_env = dep_env.unwrap_or_default();
    let mut decl = decl.expect("decl must exist after synthesized fallback");

    canonical_dependencies.extend(tracked_deps);
    canonical_dependencies.extend(resolution_deps);
    canonical_dependencies.insert(resolved_source_canonical_id.clone());

    if resolver.budget_is_exhausted() {
        return None;
    }

    let body_has_structural_extends = body_has_structural_intersection_refs(&decl.body);
    let preferred_body =
        choose_preferred_imported_type_body(resolved_body.clone(), resolved_decl_body.clone());
    let selected_body =
        choose_preferred_imported_type_body(Some(decl.body.clone()), preferred_body.clone())
            .or(preferred_body.clone());
    let requires_source_merge = if body_has_structural_extends {
        resolved_decl_body.is_none()
            && match selected_body.as_ref() {
                Some(body) => {
                    is_empty_object_surface(body) || has_non_object_top_level_surface(body)
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
            normalized_env
                .type_bindings
                .insert(param.name.clone(), TypeExpr::named(param.name.clone()));
        }
        let normalized_body = verter_analysis::type_eval::evaluate(&body, &mut normalized_env);
        decl.body = choose_preferred_imported_type_body(Some(body), Some(normalized_body))
            .expect("preferred imported type body should exist");
    } else {
        for param in &decl.type_parameters {
            dep_env
                .type_bindings
                .insert(param.name.clone(), TypeExpr::named(param.name.clone()));
        }
        decl.body = verter_analysis::type_eval::evaluate(&decl.body, &mut dep_env);
    }
    decl.name = request.local_name.clone();

    Some(ImportedTypeAlias {
        local_name: request.local_name,
        source_canonical_id: resolved_source_canonical_id,
        exported_name: request.exported_name,
        decl,
        requires_source_merge,
    })
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
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) | TypeExpr::Unknown { .. } => false,
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
        TypeExpr::Object(obj) => obj.properties.iter().any(|member| match member {
            ObjectMember::Property(prop) => contains_nested_resolution_targets(&prop.ty),
            ObjectMember::Method(method) => {
                contains_nested_resolution_targets_in_function(&method.function)
            }
            ObjectMember::IndexSignature(sig) => {
                contains_nested_resolution_targets(&sig.key_type)
                    || contains_nested_resolution_targets(&sig.value_type)
            }
            ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                contains_nested_resolution_targets_in_function(func)
            }
        }),
        TypeExpr::Function(func) => contains_nested_resolution_targets_in_function(func),
        TypeExpr::TemplateLiteral { expressions, .. } => {
            expressions.iter().any(contains_nested_resolution_targets)
        }
        TypeExpr::Infer { .. } => false,
    }
}

fn contains_nested_resolution_targets_in_function(func: &FunctionExpr) -> bool {
    func.parameters
        .iter()
        .any(|param| contains_nested_resolution_targets(&param.ty))
        || func
            .return_type
            .as_deref()
            .is_some_and(contains_nested_resolution_targets)
        || func.type_parameters.iter().any(|param| {
            param
                .constraint
                .as_deref()
                .is_some_and(contains_nested_resolution_targets)
                || param
                    .default
                    .as_deref()
                    .is_some_and(contains_nested_resolution_targets)
        })
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
            for ty in types {
                let count = extracted_surface_property_count(ty)?;
                total += count;
                saw_surface = true;
            }
            saw_surface.then_some(total)
        }
        _ => None,
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
    use verter_analysis::type_eval::{DeclarationId, TypeDeclKind};
    use verter_analysis::type_expr::{ObjectExpr, ObjectProperty, PrimitiveName, TypeExpr};

    #[derive(Default)]
    struct TestResolver {
        canonical_overrides: FxHashMap<String, String>,
        envs: FxHashMap<String, EvalEnv>,
        resolved_body: Option<TypeExpr>,
        owner_env_body: Option<TypeExpr>,
        error: Option<ImportedTypeAliasPrepareError>,
        overflow_message: Option<String>,
    }

    impl ImportedTypeAliasResolver for TestResolver {
        fn canonicalize_imported_source(&self, source_canonical_id: &str) -> String {
            self.canonical_overrides
                .get(source_canonical_id)
                .cloned()
                .unwrap_or_else(|| source_canonical_id.to_string())
        }

        fn dependency_eval_env(&self, canonical_id: &str) -> Option<EvalEnv> {
            self.envs.get(canonical_id).cloned()
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
            self.owner_env_body.clone()
        }
    }

    fn empty_object() -> TypeExpr {
        TypeExpr::Object(ObjectExpr { properties: vec![] })
    }

    fn object_with_string_prop(name: &str) -> TypeExpr {
        TypeExpr::Object(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty {
                name: name.to_string(),
                ty: TypeExpr::Primitive(PrimitiveName::String),
                optional: false,
                readonly: false,
            })],
        })
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
                TypeExpr::Intersection(vec![
                    TypeExpr::named("BaseProps"),
                    TypeExpr::named("OtherProps"),
                ]),
            ),
        );

        let actual = prepare_imported_type_alias(&mut resolver, request, &mut deps).unwrap();

        assert!(actual.requires_source_merge);
        assert!(deps.contains("/src/types.ts"));
        assert!(deps.contains("/deps/tracked.ts"));
        assert!(deps.contains("/deps/resolved.ts"));
    }

    #[test]
    fn prepare_imported_type_alias_prefers_owner_env_body_over_empty_external_body() {
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
        resolver.resolved_body = Some(empty_object());
        resolver.owner_env_body = Some(object_with_string_prop("from_owner_env"));

        let actual = prepare_imported_type_alias(&mut resolver, request, &mut deps).unwrap();

        assert!(!actual.requires_source_merge);
        let TypeExpr::Object(obj) = actual.decl.body else {
            panic!("expected object body");
        };
        assert_eq!(obj.properties.len(), 1);
        let ObjectMember::Property(prop) = &obj.properties[0] else {
            panic!("expected property");
        };
        assert_eq!(prop.name, "from_owner_env");
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
        resolver.envs.insert(
            "/src/types.ts".to_string(),
            env_with_decl("ImportedProps", object_with_string_prop("from_decl")),
        );
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
}
