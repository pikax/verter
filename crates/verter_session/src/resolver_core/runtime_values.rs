use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_eval::{EvalEnv, ValueDeclInfo};
use verter_semantic::analysis::types::{AnalyzedImport, ImportBindingKind};

pub trait ImportedRuntimeValueResolver {
    fn dependency_eval_env(&self, canonical_id: &str) -> Option<Arc<EvalEnv>>;

    fn prepared_value_decl(
        &self,
        _canonical_id: &str,
        _symbol_name: &str,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>> {
        None
    }

    fn resolve_import_canonical_id(
        &self,
        _owner_canonical_id: &str,
        _import: &AnalyzedImport,
    ) -> Option<String> {
        None
    }

    fn resolve_value_export_target(
        &self,
        _dep_canonical_id: &str,
        _imported_name: &str,
    ) -> Option<(String, String)> {
        None
    }
}

pub fn materialize_imported_runtime_values_into_env<R: ImportedRuntimeValueResolver>(
    owner_canonical_id: &str,
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
        let dep_canonical_id = import
            .resolved_canonical_id
            .clone()
            .or_else(|| resolver.resolve_import_canonical_id(owner_canonical_id, import));
        let Some(dep_canonical_id) = dep_canonical_id.as_deref() else {
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

        for binding in requested_bindings {
            let imported_name = binding
                .imported_name
                .as_deref()
                .unwrap_or(binding.name.as_str());
            let (source_canonical_id, source_name) = resolver
                .resolve_value_export_target(dep_canonical_id, imported_name)
                .unwrap_or_else(|| (dep_canonical_id.to_string(), imported_name.to_string()));
            if let Some(prepared_value) =
                resolver.prepared_value_decl(&source_canonical_id, &source_name)
            {
                let mut alias = prepared_value_decl_to_value_decl_info(prepared_value.as_ref());
                alias.name = binding.name.clone();
                env.add_value(alias);
                continue;
            }
            let dep_env = dep_env_cache
                .entry(dep_canonical_id.to_string())
                .or_insert_with(|| resolver.dependency_eval_env(dep_canonical_id));
            let Some(dep_env) = dep_env.as_ref().cloned() else {
                continue;
            };
            let source_env = if source_canonical_id == dep_canonical_id {
                Arc::clone(&dep_env)
            } else {
                match dep_env_cache
                    .entry(source_canonical_id.clone())
                    .or_insert_with(|| resolver.dependency_eval_env(&source_canonical_id))
                    .as_ref()
                {
                    Some(env) => Arc::clone(env),
                    None => continue,
                }
            };
            let Some(dep_value) = source_env.value_symbols.get(&source_name).cloned() else {
                continue;
            };

            let mut alias = dep_value;
            alias.name = binding.name.clone();
            env.add_value(alias);
        }
    }
}

fn prepared_value_decl_to_value_decl_info(
    prepared: &verter_semantic::analysis::type_solver::PreparedValueDecl,
) -> ValueDeclInfo {
    ValueDeclInfo {
        name: prepared.root_identity.symbol_name.clone(),
        declaration_id: 0,
        kind: prepared.kind,
        type_annotation: prepared.type_annotation.clone(),
        function_signature: prepared.function_signature.clone(),
        object_shape: prepared.object_shape.clone(),
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
        prepared_values: FxHashMap<
            (String, String),
            Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>,
        >,
        lookup_counts: RefCell<FxHashMap<String, usize>>,
        value_export_targets: FxHashMap<(String, String), (String, String)>,
        import_targets: FxHashMap<(String, String), String>,
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

        fn prepared_value_decl(
            &self,
            canonical_id: &str,
            symbol_name: &str,
        ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>> {
            self.prepared_values
                .get(&(canonical_id.to_string(), symbol_name.to_string()))
                .cloned()
        }

        fn resolve_import_canonical_id(
            &self,
            owner_canonical_id: &str,
            import: &AnalyzedImport,
        ) -> Option<String> {
            self.import_targets
                .get(&(owner_canonical_id.to_string(), import.source.clone()))
                .cloned()
        }

        fn resolve_value_export_target(
            &self,
            dep_canonical_id: &str,
            imported_name: &str,
        ) -> Option<(String, String)> {
            self.value_export_targets
                .get(&(dep_canonical_id.to_string(), imported_name.to_string()))
                .cloned()
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
            "/src/owner.ts",
            &imports,
            &FxHashSet::default(),
            None,
            &mut env,
            &resolver,
        );

        assert!(env.value_symbols.contains_key("theme"));
    }

    #[test]
    fn materialize_imported_runtime_values_prefers_prepared_value_decl() {
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

        let mut resolver = TestResolver::default();
        resolver
            .prepared_values
            .insert(("/src/dep.ts".to_string(), "theme".to_string()), {
                let mut decl = verter_semantic::analysis::type_solver::PreparedValueDecl::new(
                    verter_semantic::analysis::type_solver::ResolvedRootIdentity::new(
                        "/src/dep.ts",
                        "theme",
                    ),
                    ValueDeclKind::Const,
                );
                decl.exported_name = Some("theme".to_string());
                decl.type_annotation =
                    Some(verter_semantic::analysis::type_expr::TypeExpr::string_literal("dark"));
                Arc::new(decl)
            });

        let mut env = EvalEnv::new();
        materialize_imported_runtime_values_into_env(
            "/src/owner.ts",
            &imports,
            &FxHashSet::default(),
            None,
            &mut env,
            &resolver,
        );

        assert!(env.value_symbols.contains_key("theme"));
        assert!(
            resolver.lookup_counts.borrow().is_empty(),
            "prepared value decl path should not require dependency eval env lookup"
        );
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
            "/src/owner.ts",
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
            "/src/owner.ts",
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
            "/src/owner.ts",
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

    #[test]
    fn materialize_imported_runtime_values_follows_default_export_target() {
        let imports = vec![AnalyzedImport {
            source: "./theme".to_string(),
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "theme".to_string(),
                kind: ImportBindingKind::Default,
                imported_name: Some("default".to_string()),
                is_type_only: false,
                vue_api: None,
                span: Span::new(0, 0),
            }],
            span: Span::new(0, 0),
            resolved_canonical_id: Some("/src/theme.ts".to_string()),
        }];

        let mut dep_env = EvalEnv::new();
        dep_env.add_value(ValueDeclInfo {
            name: "theme".to_string(),
            declaration_id: 1,
            kind: ValueDeclKind::Const,
            type_annotation: Some(
                verter_semantic::analysis::type_expr::TypeExpr::string_literal("primary"),
            ),
            function_signature: None,
            object_shape: None,
        });
        dep_env.add_value(ValueDeclInfo {
            name: "default".to_string(),
            declaration_id: 2,
            kind: ValueDeclKind::Const,
            type_annotation: Some(verter_semantic::analysis::type_expr::TypeExpr::TypeOf(
                verter_semantic::analysis::type_expr::ValueRef {
                    path: vec!["theme".to_string()],
                },
            )),
            function_signature: None,
            object_shape: None,
        });

        let mut resolver = TestResolver::default();
        resolver
            .dep_envs
            .insert("/src/theme.ts".to_string(), Arc::new(dep_env));
        resolver.value_export_targets.insert(
            ("/src/theme.ts".to_string(), "default".to_string()),
            ("/src/theme.ts".to_string(), "theme".to_string()),
        );

        let mut env = EvalEnv::new();
        materialize_imported_runtime_values_into_env(
            "/src/owner.ts",
            &imports,
            &FxHashSet::default(),
            Some(&FxHashSet::from_iter(["theme".to_string()])),
            &mut env,
            &resolver,
        );

        assert_eq!(
            env.value_symbols
                .get("theme")
                .and_then(|value| value.type_annotation.clone()),
            Some(verter_semantic::analysis::type_expr::TypeExpr::string_literal("primary")),
            "default imports should hydrate the underlying exported value, not the synthetic default wrapper",
        );
    }

    #[test]
    fn materialize_imported_runtime_values_falls_back_to_resolved_import_targets() {
        let imports = vec![AnalyzedImport {
            source: "./theme".to_string(),
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
            resolved_canonical_id: None,
        }];

        let mut dep_env = EvalEnv::new();
        dep_env.add_value(ValueDeclInfo {
            name: "theme".to_string(),
            declaration_id: 1,
            kind: ValueDeclKind::Const,
            type_annotation: Some(
                verter_semantic::analysis::type_expr::TypeExpr::string_literal("primary"),
            ),
            function_signature: None,
            object_shape: None,
        });

        let mut resolver = TestResolver::default();
        resolver
            .dep_envs
            .insert("/src/theme.ts".to_string(), Arc::new(dep_env));
        resolver.import_targets.insert(
            ("/src/Button.vue".to_string(), "./theme".to_string()),
            "/src/theme.ts".to_string(),
        );

        let mut env = EvalEnv::new();
        materialize_imported_runtime_values_into_env(
            "/src/Button.vue",
            &imports,
            &FxHashSet::default(),
            None,
            &mut env,
            &resolver,
        );

        assert_eq!(
            env.value_symbols
                .get("theme")
                .and_then(|value| value.type_annotation.clone()),
            Some(verter_semantic::analysis::type_expr::TypeExpr::string_literal("primary")),
        );
    }

    #[test]
    fn resolved_canonical_id_on_import_skips_resolver_fallback() {
        // When import.resolved_canonical_id is set (from canonical shallow
        // import edges), the resolver's resolve_import_canonical_id must NOT
        // be called.
        let mut dep_env = EvalEnv::new();
        dep_env.value_symbols.insert(
            "defaults".to_string(),
            ValueDeclInfo {
                name: "defaults".to_string(),
                declaration_id: 0,
                kind: ValueDeclKind::Const,
                type_annotation: Some(
                    verter_semantic::analysis::type_expr::TypeExpr::string_literal("cached"),
                ),
                function_signature: None,
                object_shape: None,
            },
        );

        let imports = vec![AnalyzedImport {
            source: "./dep".to_string(),
            is_type_only: false,
            // Pre-resolved canonical ID from shallow import edge
            resolved_canonical_id: Some("/resolved/dep.ts".to_string()),
            bindings: vec![AnalyzedImportBinding {
                name: "defaults".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: Some("defaults".to_string()),
                is_type_only: false,
                vue_api: None,
                span: Span::new(0, 0),
            }],
            span: Span::new(0, 0),
        }];

        let mut resolver = TestResolver::default();
        resolver
            .dep_envs
            .insert("/resolved/dep.ts".to_string(), Arc::new(dep_env));
        // Add a WRONG target to the fallback — if the fallback fires,
        // the value won't be found and the test will fail.
        resolver.import_targets.insert(
            ("/src/owner.ts".to_string(), "./dep".to_string()),
            "/wrong/target.ts".to_string(),
        );

        let mut env = EvalEnv::new();
        materialize_imported_runtime_values_into_env(
            "/src/owner.ts",
            &imports,
            &FxHashSet::default(),
            None,
            &mut env,
            &resolver,
        );

        // The value should be found via the pre-resolved canonical ID,
        // NOT through the resolver fallback.
        assert_eq!(
            env.value_symbols
                .get("defaults")
                .and_then(|v| v.type_annotation.clone()),
            Some(verter_semantic::analysis::type_expr::TypeExpr::string_literal("cached")),
            "runtime value materialization should use import.resolved_canonical_id, \
             not fall back to resolver.resolve_import_canonical_id"
        );

        // The fallback resolver should NOT have been called for /resolved/dep.ts
        let lookup_counts = resolver.lookup_counts.borrow();
        assert!(
            lookup_counts.get("/wrong/target.ts").is_none()
                || *lookup_counts.get("/wrong/target.ts").unwrap() == 0,
            "fallback resolver should not have been consulted when resolved_canonical_id is set"
        );
    }
}
