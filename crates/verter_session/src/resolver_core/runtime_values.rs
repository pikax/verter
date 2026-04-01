use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_eval::EvalEnv;
use verter_semantic::analysis::types::{AnalyzedImport, ImportBindingKind};

pub trait ImportedRuntimeValueResolver {
    fn dependency_eval_env(&self, canonical_id: &str) -> Option<Arc<EvalEnv>>;
}

pub fn materialize_imported_runtime_values_into_env<R: ImportedRuntimeValueResolver>(
    imports: &[AnalyzedImport],
    owner_local_value_names: &FxHashSet<String>,
    required_binding_names: Option<&FxHashSet<String>>,
    env: &mut EvalEnv,
    resolver: &R,
) {
    let mut dep_env_cache: FxHashMap<String, Option<Arc<EvalEnv>>> = FxHashMap::default();

    for import in imports {
        if import.is_type_only {
            continue;
        }
        let Some(dep_canonical_id) = import.resolved_canonical_id.as_deref() else {
            continue;
        };

        let requested_bindings: Vec<_> = import
            .bindings
            .iter()
            .filter(|binding| {
                !binding.is_type_only
                    && !matches!(binding.kind, ImportBindingKind::Namespace)
                    && !owner_local_value_names.contains(&binding.name)
                    && required_binding_names
                        .is_none_or(|required| required.contains(&binding.name))
            })
            .collect();
        if requested_bindings.is_empty() {
            continue;
        }

        let dep_env = dep_env_cache
            .entry(dep_canonical_id.to_string())
            .or_insert_with(|| resolver.dependency_eval_env(dep_canonical_id));
        let Some(dep_env) = dep_env.as_ref() else {
            continue;
        };

        for binding in requested_bindings {
            let imported_name = binding
                .imported_name
                .as_deref()
                .unwrap_or(binding.name.as_str());
            let Some(dep_value) = dep_env.value_symbols.get(imported_name).cloned() else {
                continue;
            };

            let mut alias = dep_value;
            alias.name = binding.name.clone();
            env.add_value(alias);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{materialize_imported_runtime_values_into_env, ImportedRuntimeValueResolver};
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::cell::RefCell;
    use std::sync::Arc;
    use verter_semantic::analysis::type_eval::{EvalEnv, ValueDeclInfo, ValueDeclKind};
    use verter_semantic::analysis::types::{
        AnalyzedImport, AnalyzedImportBinding, ImportBindingKind,
    };
    use verter_span::Span;

    #[derive(Default)]
    struct TestResolver {
        dep_envs: FxHashMap<String, Arc<EvalEnv>>,
        lookup_counts: RefCell<FxHashMap<String, usize>>,
    }

    impl ImportedRuntimeValueResolver for TestResolver {
        fn dependency_eval_env(&self, canonical_id: &str) -> Option<Arc<EvalEnv>> {
            *self
                .lookup_counts
                .borrow_mut()
                .entry(canonical_id.to_string())
                .or_default() += 1;
            self.dep_envs.get(canonical_id).cloned()
        }
    }

    #[test]
    fn materialize_imported_runtime_values_keeps_same_name_named_imports() {
        let imports = vec![AnalyzedImport {
            source: "./dep".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "theme".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: None,
                is_type_only: false,
                vue_api: None,
                span: Span::new(0, 0),
            }],
            span: Span::new(0, 0),
            resolved_canonical_id: Some("/src/dep.ts".to_string()),
        }];
        let mut dep_env = EvalEnv::new();
        dep_env.add_value(ValueDeclInfo {
            name: "theme".to_string(),
            declaration_id: 1,
            kind: ValueDeclKind::Const,
            type_annotation: Some(
                verter_semantic::analysis::type_expr::TypeExpr::string_literal("dark"),
            ),
            function_signature: None,
            object_shape: None,
        });

        let mut resolver = TestResolver::default();
        resolver
            .dep_envs
            .insert("/src/dep.ts".to_string(), Arc::new(dep_env));

        let mut env = EvalEnv::new();
        materialize_imported_runtime_values_into_env(
            &imports,
            &FxHashSet::default(),
            None,
            &mut env,
            &resolver,
        );

        assert!(env.value_symbols.contains_key("theme"));
    }

    #[test]
    fn materialize_imported_runtime_values_skips_owner_shadowed_values() {
        let imports = vec![AnalyzedImport {
            source: "./dep".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "theme".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: Some("themeConfig".to_string()),
                is_type_only: false,
                vue_api: None,
                span: Span::new(0, 0),
            }],
            span: Span::new(0, 0),
            resolved_canonical_id: Some("/src/dep.ts".to_string()),
        }];
        let mut dep_env = EvalEnv::new();
        dep_env.add_value(ValueDeclInfo {
            name: "themeConfig".to_string(),
            declaration_id: 2,
            kind: ValueDeclKind::Const,
            type_annotation: Some(
                verter_semantic::analysis::type_expr::TypeExpr::string_literal("dark"),
            ),
            function_signature: None,
            object_shape: None,
        });

        let mut resolver = TestResolver::default();
        resolver
            .dep_envs
            .insert("/src/dep.ts".to_string(), Arc::new(dep_env));

        let mut env = EvalEnv::new();
        env.add_value(ValueDeclInfo {
            name: "theme".to_string(),
            declaration_id: 3,
            kind: ValueDeclKind::Const,
            type_annotation: Some(
                verter_semantic::analysis::type_expr::TypeExpr::string_literal("local"),
            ),
            function_signature: None,
            object_shape: None,
        });
        materialize_imported_runtime_values_into_env(
            &imports,
            &FxHashSet::from_iter(["theme".to_string()].into_iter()),
            None,
            &mut env,
            &resolver,
        );

        assert_eq!(
            env.value_symbols
                .get("theme")
                .and_then(|value| value.type_annotation.clone()),
            Some(verter_semantic::analysis::type_expr::TypeExpr::string_literal("local"))
        );
    }

    #[test]
    fn materialize_imported_runtime_values_filters_to_requested_bindings() {
        let imports = vec![AnalyzedImport {
            source: "./dep".to_string(),
            is_type_only: false,
            bindings: vec![
                AnalyzedImportBinding {
                    name: "theme".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: None,
                    is_type_only: false,
                    vue_api: None,
                    span: Span::new(0, 0),
                },
                AnalyzedImportBinding {
                    name: "helper".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: None,
                    is_type_only: false,
                    vue_api: None,
                    span: Span::new(0, 0),
                },
            ],
            span: Span::new(0, 0),
            resolved_canonical_id: Some("/src/dep.ts".to_string()),
        }];
        let mut dep_env = EvalEnv::new();
        dep_env.add_value(ValueDeclInfo {
            name: "theme".to_string(),
            declaration_id: 1,
            kind: ValueDeclKind::Const,
            type_annotation: Some(
                verter_semantic::analysis::type_expr::TypeExpr::string_literal("dark"),
            ),
            function_signature: None,
            object_shape: None,
        });
        dep_env.add_value(ValueDeclInfo {
            name: "helper".to_string(),
            declaration_id: 2,
            kind: ValueDeclKind::Const,
            type_annotation: Some(
                verter_semantic::analysis::type_expr::TypeExpr::string_literal("helper"),
            ),
            function_signature: None,
            object_shape: None,
        });

        let mut resolver = TestResolver::default();
        resolver
            .dep_envs
            .insert("/src/dep.ts".to_string(), Arc::new(dep_env));

        let mut env = EvalEnv::new();
        let required = FxHashSet::from_iter(["theme".to_string()].into_iter());
        materialize_imported_runtime_values_into_env(
            &imports,
            &FxHashSet::default(),
            Some(&required),
            &mut env,
            &resolver,
        );

        assert!(env.value_symbols.contains_key("theme"));
        assert!(!env.value_symbols.contains_key("helper"));
    }

    #[test]
    fn materialize_imported_runtime_values_skips_unused_import_source_lookups() {
        let imports = vec![
            AnalyzedImport {
                source: "./used".to_string(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "theme".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: None,
                    is_type_only: false,
                    vue_api: None,
                    span: Span::new(0, 0),
                }],
                span: Span::new(0, 0),
                resolved_canonical_id: Some("/src/used.ts".to_string()),
            },
            AnalyzedImport {
                source: "./unused".to_string(),
                is_type_only: false,
                bindings: vec![AnalyzedImportBinding {
                    name: "helper".to_string(),
                    kind: ImportBindingKind::Named,
                    imported_name: None,
                    is_type_only: false,
                    vue_api: None,
                    span: Span::new(0, 0),
                }],
                span: Span::new(0, 0),
                resolved_canonical_id: Some("/src/unused.ts".to_string()),
            },
        ];
        let mut used_env = EvalEnv::new();
        used_env.add_value(ValueDeclInfo {
            name: "theme".to_string(),
            declaration_id: 1,
            kind: ValueDeclKind::Const,
            type_annotation: Some(
                verter_semantic::analysis::type_expr::TypeExpr::string_literal("dark"),
            ),
            function_signature: None,
            object_shape: None,
        });
        let mut unused_env = EvalEnv::new();
        unused_env.add_value(ValueDeclInfo {
            name: "helper".to_string(),
            declaration_id: 2,
            kind: ValueDeclKind::Const,
            type_annotation: Some(
                verter_semantic::analysis::type_expr::TypeExpr::string_literal("helper"),
            ),
            function_signature: None,
            object_shape: None,
        });

        let mut resolver = TestResolver::default();
        resolver
            .dep_envs
            .insert("/src/used.ts".to_string(), Arc::new(used_env));
        resolver
            .dep_envs
            .insert("/src/unused.ts".to_string(), Arc::new(unused_env));

        let mut env = EvalEnv::new();
        let required = FxHashSet::from_iter(["theme".to_string()].into_iter());
        materialize_imported_runtime_values_into_env(
            &imports,
            &FxHashSet::default(),
            Some(&required),
            &mut env,
            &resolver,
        );

        assert!(env.value_symbols.contains_key("theme"));
        assert!(!env.value_symbols.contains_key("helper"));
        assert_eq!(
            resolver
                .lookup_counts
                .borrow()
                .get("/src/used.ts")
                .copied()
                .unwrap_or_default(),
            1
        );
        assert_eq!(
            resolver
                .lookup_counts
                .borrow()
                .get("/src/unused.ts")
                .copied()
                .unwrap_or_default(),
            0,
            "unused import sources should not be resolved just to discover they are unnecessary"
        );
    }
}
