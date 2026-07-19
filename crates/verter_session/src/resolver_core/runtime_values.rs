use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_semantic::analysis::type_eval::{EvalEnv, ValueDeclInfo};
use verter_semantic::analysis::types::{AnalyzedImport, ImportBindingKind};

/// Exact identity of a top-level runtime value declaration.
///
/// `owner` is part of the identity for carrier files: module and instance
/// declarations with the same name are distinct values.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValueDeclIdentity {
    pub canonical_id: String,
    pub owner: verter_type_expr::TopLevelOwnerId,
    pub name: String,
}

pub trait ImportedRuntimeValueResolver {
    fn dependency_eval_env(&self, canonical_id: &str) -> Option<Arc<EvalEnv>>;

    /// Bounded, graph-native per-name value-symbol reader (graph-native consumer-reader readiness
    /// readiness): the equivalent of `dependency_eval_env(src)` followed
    /// by `value_symbols.get(name).primary().clone()`, but resolved
    /// through the per-symbol lazy memo WITHOUT materialising the
    /// dependency's whole env. The default returns `None` (test resolvers
    /// / callers without a host-backed per-symbol reader fall back to the
    /// whole-env oracle below).
    fn dependency_value_symbol_graph_native(
        &self,
        _source: &ValueDeclIdentity,
    ) -> Option<ValueDeclInfo> {
        None
    }

    fn prepared_value_decl(
        &self,
        _source: &ValueDeclIdentity,
    ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>> {
        None
    }

    fn resolve_value_export_target(
        &self,
        _requested: &ValueDeclIdentity,
    ) -> Option<ValueDeclIdentity> {
        None
    }
}

pub fn materialize_imported_runtime_values_into_env<R: ImportedRuntimeValueResolver>(
    imports: &[AnalyzedImport],
    owner_local_value_names: &FxHashSet<verter_type_expr::DeclKey>,
    required_binding_names: Option<&FxHashSet<String>>,
    env: &mut EvalEnv,
    resolver: &R,
) {
    let mut dep_env_cache: FxHashMap<String, Option<Arc<EvalEnv>>> = FxHashMap::default();

    for import in imports {
        if import.is_type_only {
            continue;
        }
        let dep_canonical_id = import.resolved_canonical_id.clone();
        let Some(dep_canonical_id) = dep_canonical_id.as_deref() else {
            continue;
        };

        let requested_bindings: Vec<_> = import
            .bindings
            .iter()
            .filter(|binding| {
                !binding.is_type_only
                    && !matches!(binding.kind, ImportBindingKind::Namespace)
                    && !owner_local_value_names.contains(&verter_type_expr::DeclKey::new(
                        import.owner,
                        binding.name.as_str(),
                    ))
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
            let requested = ValueDeclIdentity {
                canonical_id: dep_canonical_id.to_string(),
                owner: import.owner,
                name: imported_name.to_string(),
            };
            let Some(target) = resolver.resolve_value_export_target(&requested) else {
                continue;
            };
            if let Some(prepared_value) = resolver.prepared_value_decl(&target) {
                let mut alias = prepared_value_decl_to_value_decl_info(prepared_value.as_ref());
                alias.owner = import.owner;
                alias.name = binding.name.clone();
                env.add_value(alias);
                continue;
            }
            let source_env = match dep_env_cache
                .entry(target.canonical_id.clone())
                .or_insert_with(|| resolver.dependency_eval_env(&target.canonical_id))
                .as_ref()
            {
                Some(env) => Arc::clone(env),
                None => continue,
            };
            let Some(dep_group) = source_env.value_group_in(target.owner, target.name.as_str())
            else {
                continue;
            };

            let primary = dep_group.primary();
            // Non-breaking readiness cross-check (debug/test only): the
            // bounded graph-native per-name reader must reproduce the
            // whole-env value read field-for-field (modulo the opaque
            // `declaration_id`). A resolver without a host-backed reader
            // returns `None` and skips the check. Release builds skip this
            // entirely; the oracle below stays authoritative.
            #[cfg(debug_assertions)]
            if let Some(graph_native) = resolver.dependency_value_symbol_graph_native(&target) {
                debug_assert_eq!(
                    graph_native.kind, primary.kind,
                    "graph-native consumer-reader readiness: C4 graph-native value reader kind diverged for \
                     ({}, {:?}, {})",
                    target.canonical_id, target.owner, target.name,
                );
                debug_assert_eq!(
                    graph_native.type_annotation, primary.type_annotation,
                    "graph-native consumer-reader readiness: C4 graph-native value reader type_annotation diverged for \
                     ({}, {:?}, {})",
                    target.canonical_id, target.owner, target.name,
                );
                // `FunctionSignature` does not implement `PartialEq`;
                // compare via the structural debug projection.
                debug_assert_eq!(
                    format!("{:?}", graph_native.signatures),
                    format!("{:?}", primary.signatures),
                    "graph-native consumer-reader readiness: C4 graph-native value reader signatures diverged for \
                     ({}, {:?}, {})",
                    target.canonical_id, target.owner, target.name,
                );
                debug_assert_eq!(
                    graph_native.object_shape, primary.object_shape,
                    "graph-native consumer-reader readiness: C4 graph-native value reader object_shape diverged for \
                     ({}, {:?}, {})",
                    target.canonical_id, target.owner, target.name,
                );
                debug_assert_eq!(
                    graph_native.enum_members, primary.enum_members,
                    "graph-native consumer-reader readiness: C4 graph-native value reader enum_members diverged for \
                     ({}, {:?}, {})",
                    target.canonical_id, target.owner, target.name,
                );
            }

            let mut alias = primary.clone();
            alias.owner = import.owner;
            alias.name = binding.name.clone();
            env.add_value(alias);
        }
    }
}

fn prepared_value_decl_to_value_decl_info(
    prepared: &verter_semantic::analysis::type_solver::PreparedValueDecl,
) -> ValueDeclInfo {
    ValueDeclInfo {
        owner: prepared.root_identity.owner,
        name: prepared.root_identity.symbol_name.as_ref().to_string(),
        declaration_id: 0,
        kind: prepared.kind,
        type_annotation: prepared.type_annotation.clone(),
        signatures: prepared.signatures.clone(),
        object_shape: prepared.object_shape.clone(),
        // The prepared decl carries the FULL member inventory (every member's
        // scalar — foldable literal or deferred-and-degraded domain), so the
        // round-trip into the importing env is a lossless copy: a deferred member
        // of an imported enum keeps both its NAME and its degraded domain.
        enum_members: prepared.enum_members.clone(),
        // The prepared surface carries no member-NAME inventory fact; derive
        // it from the member inventory — lossless for the statically-named
        // set (both rails are producer-emitted from the same member walk).
        enum_member_names: prepared.enum_members.as_ref().map(|fact| {
            verter_type_expr::facts::EnumMemberNamesFact {
                names: fact
                    .members
                    .iter()
                    .map(|member| member.name.clone())
                    .collect(),
            }
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        materialize_imported_runtime_values_into_env, ImportedRuntimeValueResolver,
        ValueDeclIdentity,
    };
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::cell::RefCell;
    use std::sync::Arc;
    use verter_semantic::analysis::type_eval::{EvalEnv, ValueDeclInfo, ValueDeclKind};
    use verter_semantic::analysis::types::{
        AnalyzedImport, AnalyzedImportBinding, ImportBindingKind,
    };
    use verter_span::Span;
    use verter_type_expr::facts::{
        ClosedTypeFact, LeafTypeFact, SemanticTypeSource, ValueAnnotationClass,
        ValueTypeAnnotationFact,
    };

    /// A direct annotation fact carrying the closed string-literal leaf —
    /// the fact-vocabulary spelling of an authored `"lit"` annotation.
    fn string_literal_annotation(lit: &str) -> ValueTypeAnnotationFact {
        ValueTypeAnnotationFact {
            typeof_alias_target: None,
            classification: ValueAnnotationClass::Direct,
            annotation: Some(SemanticTypeSource::Closed(ClosedTypeFact::Leaf(
                LeafTypeFact::StringLiteral(lit.to_string()),
            ))),
        }
    }

    fn value_decl(
        owner: verter_type_expr::TopLevelOwnerId,
        name: &str,
        annotation: &str,
    ) -> ValueDeclInfo {
        ValueDeclInfo {
            owner,
            name: name.to_string(),
            declaration_id: 0,
            kind: ValueDeclKind::Const,
            type_annotation: string_literal_annotation(annotation),
            signatures: Vec::new(),
            object_shape: None,
            enum_members: None,
            enum_member_names: None,
        }
    }

    #[derive(Default)]
    struct TestResolver {
        dep_envs: FxHashMap<String, Arc<EvalEnv>>,
        prepared_values: FxHashMap<
            ValueDeclIdentity,
            Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>,
        >,
        lookup_counts: RefCell<FxHashMap<String, usize>>,
        value_export_targets: FxHashMap<ValueDeclIdentity, ValueDeclIdentity>,
    }

    impl TestResolver {
        fn add_direct_target(
            &mut self,
            canonical_id: &str,
            owner: verter_type_expr::TopLevelOwnerId,
            name: &str,
        ) {
            let identity = ValueDeclIdentity {
                canonical_id: canonical_id.to_string(),
                owner,
                name: name.to_string(),
            };
            self.value_export_targets.insert(identity.clone(), identity);
        }
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
            source: &ValueDeclIdentity,
        ) -> Option<Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>> {
            self.prepared_values.get(source).cloned()
        }

        fn resolve_value_export_target(
            &self,
            requested: &ValueDeclIdentity,
        ) -> Option<ValueDeclIdentity> {
            self.value_export_targets.get(requested).cloned()
        }
    }

    #[test]
    fn materialize_imported_runtime_values_keeps_same_name_named_imports() {
        let imports = vec![AnalyzedImport {
            source: "./dep".to_string(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
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
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            declaration_id: 1,
            kind: ValueDeclKind::Const,
            type_annotation: string_literal_annotation("dark"),
            signatures: Vec::new(),
            object_shape: None,
            enum_members: None,
            enum_member_names: None,
        });

        let mut resolver = TestResolver::default();
        resolver
            .dep_envs
            .insert("/src/dep.ts".to_string(), Arc::new(dep_env));
        resolver.add_direct_target(
            "/src/dep.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "theme",
        );
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
    fn materialize_imported_runtime_values_prefers_prepared_value_decl() {
        let imports = vec![AnalyzedImport {
            source: "./dep".to_string(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
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
        resolver.add_direct_target(
            "/src/dep.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "theme",
        );
        resolver.prepared_values.insert(
            ValueDeclIdentity {
                canonical_id: "/src/dep.ts".to_string(),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                name: "theme".to_string(),
            },
            {
                let mut decl = verter_semantic::analysis::type_solver::PreparedValueDecl::new(
                    verter_semantic::analysis::type_solver::ResolvedRootIdentity::new(
                        "/src/dep.ts",
                        "theme",
                    ),
                    ValueDeclKind::Const,
                );
                decl.exported_name = Some("theme".to_string());
                decl.type_annotation = string_literal_annotation("dark");
                Arc::new(decl)
            },
        );

        let mut env = EvalEnv::new();
        materialize_imported_runtime_values_into_env(
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
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
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
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            declaration_id: 2,
            kind: ValueDeclKind::Const,
            type_annotation: string_literal_annotation("dark"),
            signatures: Vec::new(),
            object_shape: None,
            enum_members: None,
            enum_member_names: None,
        });

        let mut resolver = TestResolver::default();
        resolver
            .dep_envs
            .insert("/src/dep.ts".to_string(), Arc::new(dep_env));

        let mut env = EvalEnv::new();
        env.add_value(ValueDeclInfo {
            name: "theme".to_string(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            declaration_id: 3,
            kind: ValueDeclKind::Const,
            type_annotation: string_literal_annotation("local"),
            signatures: Vec::new(),
            object_shape: None,
            enum_members: None,
            enum_member_names: None,
        });
        materialize_imported_runtime_values_into_env(
            &imports,
            &FxHashSet::from_iter([verter_type_expr::DeclKey::new(
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                "theme",
            )]),
            None,
            &mut env,
            &resolver,
        );

        assert_eq!(
            env.value_symbols
                .get("theme")
                .map(|value| value.primary().type_annotation.clone()),
            Some(string_literal_annotation("local"))
        );
    }

    #[test]
    fn materialize_imported_runtime_values_filters_to_requested_bindings() {
        let imports = vec![AnalyzedImport {
            source: "./dep".to_string(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
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
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            declaration_id: 1,
            kind: ValueDeclKind::Const,
            type_annotation: string_literal_annotation("dark"),
            signatures: Vec::new(),
            object_shape: None,
            enum_members: None,
            enum_member_names: None,
        });
        dep_env.add_value(ValueDeclInfo {
            name: "helper".to_string(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            declaration_id: 2,
            kind: ValueDeclKind::Const,
            type_annotation: string_literal_annotation("helper"),
            signatures: Vec::new(),
            object_shape: None,
            enum_members: None,
            enum_member_names: None,
        });

        let mut resolver = TestResolver::default();
        resolver
            .dep_envs
            .insert("/src/dep.ts".to_string(), Arc::new(dep_env));
        resolver.add_direct_target(
            "/src/dep.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "theme",
        );

        let mut env = EvalEnv::new();
        let required = FxHashSet::from_iter(["theme".to_string()]);
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
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
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
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
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
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            declaration_id: 1,
            kind: ValueDeclKind::Const,
            type_annotation: string_literal_annotation("dark"),
            signatures: Vec::new(),
            object_shape: None,
            enum_members: None,
            enum_member_names: None,
        });
        let mut unused_env = EvalEnv::new();
        unused_env.add_value(ValueDeclInfo {
            name: "helper".to_string(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            declaration_id: 2,
            kind: ValueDeclKind::Const,
            type_annotation: string_literal_annotation("helper"),
            signatures: Vec::new(),
            object_shape: None,
            enum_members: None,
            enum_member_names: None,
        });

        let mut resolver = TestResolver::default();
        resolver
            .dep_envs
            .insert("/src/used.ts".to_string(), Arc::new(used_env));
        resolver.add_direct_target(
            "/src/used.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "theme",
        );
        resolver
            .dep_envs
            .insert("/src/unused.ts".to_string(), Arc::new(unused_env));

        let mut env = EvalEnv::new();
        let required = FxHashSet::from_iter(["theme".to_string()]);
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

    #[test]
    fn materialize_imported_runtime_values_follows_default_export_target() {
        let imports = vec![AnalyzedImport {
            source: "./theme".to_string(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
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
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            declaration_id: 1,
            kind: ValueDeclKind::Const,
            type_annotation: string_literal_annotation("primary"),
            signatures: Vec::new(),
            object_shape: None,
            enum_members: None,
            enum_member_names: None,
        });
        dep_env.add_value(ValueDeclInfo {
            name: "default".to_string(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            declaration_id: 2,
            kind: ValueDeclKind::Const,
            type_annotation: ValueTypeAnnotationFact {
                typeof_alias_target: Some(verter_type_expr::facts::ValueDeclIdentityPart {
                    canonical_id: Arc::from("/src/theme.ts"),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    symbol: Arc::from("theme"),
                    member_path: Arc::from(Vec::<String>::new().into_boxed_slice()),
                }),
                classification: ValueAnnotationClass::TypeOfAlias,
                annotation: None,
            },
            signatures: Vec::new(),
            object_shape: None,
            enum_members: None,
            enum_member_names: None,
        });

        let mut resolver = TestResolver::default();
        resolver
            .dep_envs
            .insert("/src/theme.ts".to_string(), Arc::new(dep_env));
        resolver.value_export_targets.insert(
            ValueDeclIdentity {
                canonical_id: "/src/theme.ts".to_string(),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                name: "default".to_string(),
            },
            ValueDeclIdentity {
                canonical_id: "/src/theme.ts".to_string(),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                name: "theme".to_string(),
            },
        );

        let mut env = EvalEnv::new();
        materialize_imported_runtime_values_into_env(
            &imports,
            &FxHashSet::default(),
            Some(&FxHashSet::from_iter(["theme".to_string()])),
            &mut env,
            &resolver,
        );

        assert_eq!(
            env.value_symbols
                .get("theme")
                .map(|value| value.primary().type_annotation.clone()),
            Some(string_literal_annotation("primary")),
            "default imports should hydrate the underlying exported value, not the synthetic default wrapper",
        );
    }

    #[test]
    fn materialize_imported_runtime_values_skips_unresolved_import_without_canonical_id() {
        let imports = vec![AnalyzedImport {
            source: "./theme".to_string(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
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
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            declaration_id: 1,
            kind: ValueDeclKind::Const,
            type_annotation: string_literal_annotation("primary"),
            signatures: Vec::new(),
            object_shape: None,
            enum_members: None,
            enum_member_names: None,
        });

        let mut resolver = TestResolver::default();
        resolver
            .dep_envs
            .insert("/src/theme.ts".to_string(), Arc::new(dep_env));

        let mut env = EvalEnv::new();
        materialize_imported_runtime_values_into_env(
            &imports,
            &FxHashSet::default(),
            None,
            &mut env,
            &resolver,
        );

        assert!(
            !env.value_symbols.contains_key("theme"),
            "runtime value hydration should skip imports until resolved_canonical_id is populated"
        );
    }

    #[test]
    fn resolved_canonical_id_on_import_hydrates_without_fallback() {
        let mut dep_env = EvalEnv::new();
        dep_env.add_value(ValueDeclInfo {
            name: "defaults".to_string(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            declaration_id: 0,
            kind: ValueDeclKind::Const,
            type_annotation: string_literal_annotation("cached"),
            signatures: Vec::new(),
            object_shape: None,
            enum_members: None,
            enum_member_names: None,
        });

        let imports = vec![AnalyzedImport {
            source: "./dep".to_string(),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
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
        resolver.add_direct_target(
            "/resolved/dep.ts",
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            "defaults",
        );

        let mut env = EvalEnv::new();
        materialize_imported_runtime_values_into_env(
            &imports,
            &FxHashSet::default(),
            None,
            &mut env,
            &resolver,
        );

        assert_eq!(
            env.value_symbols
                .get("defaults")
                .map(|v| v.primary().type_annotation.clone()),
            Some(string_literal_annotation("cached")),
            "runtime value materialization should use import.resolved_canonical_id directly"
        );
    }

    #[test]
    fn vue_instance_source_is_selected_and_alias_is_rehomed_to_module_owner() {
        let module_owner = verter_type_expr::TopLevelOwnerId::module(0);
        let instance_owner = verter_type_expr::TopLevelOwnerId::instance(0);
        let imports = vec![AnalyzedImport {
            source: "./Child.vue".to_string(),
            owner: module_owner,
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "childShared".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: Some("shared".to_string()),
                is_type_only: false,
                vue_api: None,
                span: Span::new(0, 0),
            }],
            span: Span::new(0, 0),
            resolved_canonical_id: Some("/src/Child.vue".to_string()),
        }];
        let mut dep_env = EvalEnv::new();
        dep_env.add_value(value_decl(module_owner, "shared", "vue-module"));
        dep_env.add_value(value_decl(instance_owner, "shared", "vue-instance"));

        let requested = ValueDeclIdentity {
            canonical_id: "/src/Child.vue".to_string(),
            owner: module_owner,
            name: "shared".to_string(),
        };
        let target = ValueDeclIdentity {
            canonical_id: "/src/Child.vue".to_string(),
            owner: instance_owner,
            name: "shared".to_string(),
        };
        let mut resolver = TestResolver::default();
        resolver
            .dep_envs
            .insert("/src/Child.vue".to_string(), Arc::new(dep_env));
        resolver.value_export_targets.insert(requested, target);

        let mut env = EvalEnv::new();
        materialize_imported_runtime_values_into_env(
            &imports,
            &FxHashSet::default(),
            None,
            &mut env,
            &resolver,
        );

        assert_eq!(
            env.value_group_in(module_owner, "childShared")
                .map(|group| group.primary().type_annotation.clone()),
            Some(string_literal_annotation("vue-instance")),
            "the exact instance-owner source must win over the same-name module declaration",
        );
        assert!(
            env.value_group_in(instance_owner, "childShared").is_none(),
            "the hydrated alias belongs to the import's lexical module owner",
        );
    }

    #[test]
    fn svelte_module_source_is_selected_and_alias_is_rehomed_to_instance_owner() {
        let module_owner = verter_type_expr::TopLevelOwnerId::module(0);
        let instance_owner = verter_type_expr::TopLevelOwnerId::instance(0);
        let imports = vec![AnalyzedImport {
            source: "./Child.svelte".to_string(),
            owner: instance_owner,
            is_type_only: false,
            bindings: vec![AnalyzedImportBinding {
                name: "childShared".to_string(),
                kind: ImportBindingKind::Named,
                imported_name: Some("shared".to_string()),
                is_type_only: false,
                vue_api: None,
                span: Span::new(0, 0),
            }],
            span: Span::new(0, 0),
            resolved_canonical_id: Some("/src/Child.svelte".to_string()),
        }];
        let mut dep_env = EvalEnv::new();
        dep_env.add_value(value_decl(module_owner, "shared", "svelte-module"));
        dep_env.add_value(value_decl(instance_owner, "shared", "svelte-instance"));

        let requested = ValueDeclIdentity {
            canonical_id: "/src/Child.svelte".to_string(),
            owner: instance_owner,
            name: "shared".to_string(),
        };
        let target = ValueDeclIdentity {
            canonical_id: "/src/Child.svelte".to_string(),
            owner: module_owner,
            name: "shared".to_string(),
        };
        let mut resolver = TestResolver::default();
        resolver
            .dep_envs
            .insert("/src/Child.svelte".to_string(), Arc::new(dep_env));
        resolver.value_export_targets.insert(requested, target);

        let mut env = EvalEnv::new();
        materialize_imported_runtime_values_into_env(
            &imports,
            &FxHashSet::default(),
            None,
            &mut env,
            &resolver,
        );

        assert_eq!(
            env.value_group_in(instance_owner, "childShared")
                .map(|group| group.primary().type_annotation.clone()),
            Some(string_literal_annotation("svelte-module")),
            "the exact module-owner source must win over the same-name instance declaration",
        );
        assert!(
            env.value_group_in(module_owner, "childShared").is_none(),
            "the hydrated alias belongs to the import's lexical instance owner",
        );
    }
}
