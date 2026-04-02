use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Instant;

use rustc_hash::FxHashSet;
use verter_semantic::analysis::type_eval::EvalEnv;
use verter_semantic::analysis::type_expr::TypeExpr;
use verter_semantic::analysis::AnalyzedMacro;

use crate::resolver_core::fallthrough::inject_prop_type_overrides;
use crate::resolver_core::ImportedEvalInputs;

pub struct OwnerEvalEnvBuild {
    pub env: EvalEnv,
    pub requested_binding_names: FxHashSet<String>,
}

fn owner_eval_debug_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var_os("VERTER_COMPONENT_META_DEBUG").is_some()
            || std::env::var_os("VERTER_META_DEBUG").is_some()
    })
}

fn owner_eval_debug(message: impl FnOnce() -> String) {
    if owner_eval_debug_enabled() {
        eprintln!("[verter-owner-env] {}", message());
    }
}

pub trait OwnerEvalEnvAssembler {
    type Snapshot;

    fn base_eval_env(&self, canonical_id: &str) -> Option<Arc<EvalEnv>>;

    fn materialize_imported_type_aliases(
        &self,
        snapshot: &Self::Snapshot,
        owner_local_type_names: &FxHashSet<String>,
        imported_inputs: &ImportedEvalInputs,
        env: &mut EvalEnv,
    );

    fn materialize_imported_runtime_values(
        &self,
        snapshot: &Self::Snapshot,
        canonical_id: &str,
        owner_local_value_names: &FxHashSet<String>,
        required_runtime_value_names: Option<&FxHashSet<String>>,
        env: &mut EvalEnv,
    );
}

pub fn collect_requested_binding_names(macros: &[AnalyzedMacro]) -> FxHashSet<String> {
    macros
        .iter()
        .flat_map(|mac| mac.expose_fields.iter().map(|field| field.name.clone()))
        .collect()
}

pub fn build_owner_eval_env_with_inputs<A: OwnerEvalEnvAssembler>(
    assembler: &A,
    canonical_id: &str,
    snapshot: &A::Snapshot,
    macros: &[AnalyzedMacro],
    imported_inputs: &ImportedEvalInputs,
    prop_type_overrides: Option<&rustc_hash::FxHashMap<String, TypeExpr>>,
    owner_env: Option<EvalEnv>,
    required_runtime_value_names: Option<&FxHashSet<String>>,
) -> Option<OwnerEvalEnvBuild> {
    let started = owner_eval_debug_enabled().then(Instant::now);
    owner_eval_debug(|| {
        format!(
            "start owner={} dep_sources={} imported_aliases={} overrides={} owner_env={}",
            canonical_id,
            imported_inputs.sources.len(),
            imported_inputs.type_aliases.len(),
            prop_type_overrides
                .map(|overrides| overrides.len())
                .unwrap_or_default(),
            owner_env.is_some(),
        )
    });
    let mut env = owner_env.or_else(|| {
        assembler
            .base_eval_env(canonical_id)
            .map(|env| (*env).clone())
    })?;
    let local_type_names: FxHashSet<String> = env.type_symbols.keys().cloned().collect();
    let local_value_names: FxHashSet<String> = env.value_symbols.keys().cloned().collect();
    let requested_binding_names = collect_requested_binding_names(macros);

    let dep_merge_started = owner_eval_debug_enabled().then(Instant::now);
    for dep_source in &imported_inputs.sources {
        if let Some(dep_env) = assembler.base_eval_env(dep_source.canonical_id.as_str()) {
            env.extend_missing_from_ref(dep_env.as_ref());
        }
    }
    owner_eval_debug(|| {
        format!(
            "dep_merge owner={} dep_sources={} type_symbols={} value_symbols={} took {:?}",
            canonical_id,
            imported_inputs.sources.len(),
            env.type_symbols.len(),
            env.value_symbols.len(),
            dep_merge_started
                .map(|start| start.elapsed())
                .unwrap_or_default(),
        )
    });

    let type_alias_started = owner_eval_debug_enabled().then(Instant::now);
    owner_eval_debug(|| {
        format!(
            "type_aliases:start owner={} aliases={} type_symbols={}",
            canonical_id,
            imported_inputs.type_aliases.len(),
            env.type_symbols.len(),
        )
    });
    assembler.materialize_imported_type_aliases(
        snapshot,
        &local_type_names,
        imported_inputs,
        &mut env,
    );
    owner_eval_debug(|| {
        format!(
            "type_aliases:end owner={} aliases={} type_symbols={} took {:?}",
            canonical_id,
            imported_inputs.type_aliases.len(),
            env.type_symbols.len(),
            type_alias_started
                .map(|start| start.elapsed())
                .unwrap_or_default(),
        )
    });

    let runtime_started = owner_eval_debug_enabled().then(Instant::now);
    owner_eval_debug(|| {
        format!(
            "runtime_values:start owner={} required_runtime_values={} value_symbols={}",
            canonical_id,
            required_runtime_value_names
                .map(|required| required.len())
                .unwrap_or_default(),
            env.value_symbols.len(),
        )
    });
    assembler.materialize_imported_runtime_values(
        snapshot,
        canonical_id,
        &local_value_names,
        required_runtime_value_names,
        &mut env,
    );
    owner_eval_debug(|| {
        format!(
            "runtime_values:end owner={} value_symbols={} took {:?}",
            canonical_id,
            env.value_symbols.len(),
            runtime_started
                .map(|start| start.elapsed())
                .unwrap_or_default(),
        )
    });

    if let Some(overrides) = prop_type_overrides {
        inject_prop_type_overrides(&mut env, overrides);
    }

    owner_eval_debug(|| {
        format!(
            "end owner={} requested_bindings={} type_symbols={} value_symbols={} took {:?}",
            canonical_id,
            requested_binding_names.len(),
            env.type_symbols.len(),
            env.value_symbols.len(),
            started.map(|start| start.elapsed()).unwrap_or_default(),
        )
    });
    Some(OwnerEvalEnvBuild {
        env,
        requested_binding_names,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        build_owner_eval_env_with_inputs, collect_requested_binding_names, OwnerEvalEnvAssembler,
    };
    use crate::resolver_core::{ImportedEvalInputs, ImportedTypeAlias};
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use verter_semantic::analysis::type_eval::{
        EvalEnv, TypeDeclInfo, TypeDeclKind, ValueDeclInfo, ValueDeclKind,
    };
    use verter_semantic::analysis::type_expr::{PrimitiveName, TypeExpr};
    use verter_semantic::analysis::types::AnalyzedExposeField;
    use verter_semantic::analysis::{AnalyzedMacro, AnalyzedMacroKind};
    use verter_span::Span;

    #[derive(Default)]
    struct TestAssembler {
        base_envs: FxHashMap<String, Arc<EvalEnv>>,
        type_aliases: FxHashMap<(String, String), TypeDeclInfo>,
        runtime_values: FxHashMap<String, ValueDeclInfo>,
    }

    impl OwnerEvalEnvAssembler for TestAssembler {
        type Snapshot = ();

        fn base_eval_env(&self, canonical_id: &str) -> Option<Arc<EvalEnv>> {
            self.base_envs.get(canonical_id).cloned()
        }

        fn materialize_imported_type_aliases(
            &self,
            _snapshot: &Self::Snapshot,
            owner_local_type_names: &FxHashSet<String>,
            imported_inputs: &ImportedEvalInputs,
            env: &mut EvalEnv,
        ) {
            for alias in &imported_inputs.type_aliases {
                if owner_local_type_names.contains(&alias.local_name) {
                    continue;
                }
                if let Some(decl) = self.type_aliases.get(&(
                    alias.source_canonical_id.clone(),
                    alias.exported_name.clone(),
                )) {
                    let mut decl = decl.clone();
                    decl.name = alias.local_name.clone();
                    env.type_symbols.insert(alias.local_name.clone(), decl);
                }
            }
        }

        fn materialize_imported_runtime_values(
            &self,
            _snapshot: &Self::Snapshot,
            _canonical_id: &str,
            owner_local_value_names: &FxHashSet<String>,
            required_runtime_value_names: Option<&FxHashSet<String>>,
            env: &mut EvalEnv,
        ) {
            for (name, value) in &self.runtime_values {
                if owner_local_value_names.contains(name) {
                    continue;
                }
                if required_runtime_value_names.is_some_and(|required| !required.contains(name)) {
                    continue;
                }
                env.value_symbols.insert(name.clone(), value.clone());
            }
        }
    }

    #[test]
    fn collect_requested_binding_names_only_tracks_exposed_fields() {
        let macros = vec![AnalyzedMacro {
            kind: AnalyzedMacroKind::DefineExpose,
            is_type_based: false,
            type_references: Vec::new(),
            binding_name: None,
            model_name: None,
            has_inherit_attrs_false: false,
            prop_fields: Vec::new(),
            emit_fields: Vec::new(),
            slot_fields: Vec::new(),
            default_keys: Vec::new(),
            default_values: Vec::new(),
            expose_fields: vec![
                AnalyzedExposeField {
                    name: "foo".to_string(),
                    span: Span::new(0, 0),
                },
                AnalyzedExposeField {
                    name: "bar".to_string(),
                    span: Span::new(0, 0),
                },
            ],
            resolved_local_types: Vec::new(),
            span: Span::new(0, 0),
        }];

        let actual = collect_requested_binding_names(&macros);

        assert!(actual.contains("foo"));
        assert!(actual.contains("bar"));
        assert_eq!(actual.len(), 2);
    }

    #[test]
    fn materialize_imported_type_aliases_skips_owner_shadowed_names() {
        let imported_inputs = ImportedEvalInputs {
            sources: Vec::new(),
            type_aliases: vec![
                ImportedTypeAlias {
                    local_name: "Local".to_string(),
                    source_canonical_id: "/src/dep.ts".to_string(),
                    exported_name: "Local".to_string(),
                    requires_source_merge: false,
                    merge_root_canonical: "/src/dep.ts".to_string(),
                    merge_root_exported: "Local".to_string(),
                },
                ImportedTypeAlias {
                    local_name: "Imported".to_string(),
                    source_canonical_id: "/src/dep.ts".to_string(),
                    exported_name: "Imported".to_string(),
                    requires_source_merge: false,
                    merge_root_canonical: "/src/dep.ts".to_string(),
                    merge_root_exported: "Imported".to_string(),
                },
            ],
            canonical_dependencies: BTreeSet::new(),
            overflow: None,
            stats: crate::resolver_core::ImportedEvalStats::default(),
        };

        let mut assembler = TestAssembler::default();
        assembler.type_aliases.insert(
            ("/src/dep.ts".to_string(), "Imported".to_string()),
            TypeDeclInfo {
                name: "Imported".to_string(),
                declaration_id: 3,
                kind: TypeDeclKind::Alias,
                type_parameters: Vec::new(),
                body: TypeExpr::Primitive(PrimitiveName::Boolean),
            },
        );
        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "Local".to_string(),
            declaration_id: 1,
            kind: TypeDeclKind::Alias,
            type_parameters: Vec::new(),
            body: TypeExpr::Primitive(PrimitiveName::String),
        });

        assembler.materialize_imported_type_aliases(
            &(),
            &FxHashSet::from_iter(["Local".to_string()].into_iter()),
            &imported_inputs,
            &mut env,
        );

        assert_eq!(
            env.type_symbols.get("Local").map(|decl| &decl.body),
            Some(&TypeExpr::Primitive(PrimitiveName::String))
        );
        assert_eq!(
            env.type_symbols.get("Imported").map(|decl| &decl.body),
            Some(&TypeExpr::Primitive(PrimitiveName::Boolean))
        );
    }

    #[test]
    fn build_owner_eval_env_with_inputs_merges_deps_aliases_runtime_values_and_overrides() {
        let mut owner_env = EvalEnv::new();
        owner_env.add_type(TypeDeclInfo {
            name: "Local".to_string(),
            declaration_id: 1,
            kind: TypeDeclKind::Alias,
            type_parameters: Vec::new(),
            body: TypeExpr::Primitive(PrimitiveName::String),
        });
        owner_env.add_value(ValueDeclInfo {
            name: "shadowed".to_string(),
            declaration_id: 2,
            kind: ValueDeclKind::Const,
            type_annotation: Some(TypeExpr::Primitive(PrimitiveName::String)),
            function_signature: None,
            object_shape: None,
        });

        let mut dep_env = EvalEnv::new();
        dep_env.add_type(TypeDeclInfo {
            name: "DepOnly".to_string(),
            declaration_id: 3,
            kind: TypeDeclKind::Alias,
            type_parameters: Vec::new(),
            body: TypeExpr::Primitive(PrimitiveName::Boolean),
        });

        let mut assembler = TestAssembler::default();
        assembler
            .base_envs
            .insert("/src/owner.ts".to_string(), Arc::new(owner_env.clone()));
        assembler
            .base_envs
            .insert("/src/dep.ts".to_string(), Arc::new(dep_env.clone()));
        assembler.runtime_values.insert(
            "runtimeOnly".to_string(),
            ValueDeclInfo {
                name: "runtimeOnly".to_string(),
                declaration_id: 4,
                kind: ValueDeclKind::Const,
                type_annotation: Some(TypeExpr::Primitive(PrimitiveName::Number)),
                function_signature: None,
                object_shape: None,
            },
        );
        assembler.runtime_values.insert(
            "shadowed".to_string(),
            ValueDeclInfo {
                name: "shadowed".to_string(),
                declaration_id: 5,
                kind: ValueDeclKind::Const,
                type_annotation: Some(TypeExpr::Primitive(PrimitiveName::Boolean)),
                function_signature: None,
                object_shape: None,
            },
        );

        let imported_inputs = ImportedEvalInputs {
            sources: vec![crate::resolver_core::ImportedEvalSource {
                canonical_id: "/src/dep.ts".to_string(),
            }],
            type_aliases: vec![ImportedTypeAlias {
                local_name: "Imported".to_string(),
                source_canonical_id: "/src/dep.ts".to_string(),
                exported_name: "Imported".to_string(),
                requires_source_merge: false,
                merge_root_canonical: "/src/dep.ts".to_string(),
                merge_root_exported: "Imported".to_string(),
            }],
            canonical_dependencies: BTreeSet::new(),
            overflow: None,
            stats: crate::resolver_core::ImportedEvalStats::default(),
        };
        assembler.type_aliases.insert(
            ("/src/dep.ts".to_string(), "Imported".to_string()),
            TypeDeclInfo {
                name: "Imported".to_string(),
                declaration_id: 6,
                kind: TypeDeclKind::Alias,
                type_parameters: Vec::new(),
                body: TypeExpr::Primitive(PrimitiveName::Boolean),
            },
        );

        let overrides = FxHashMap::from_iter([(
            "Local".to_string(),
            TypeExpr::Primitive(PrimitiveName::Number),
        )]);

        let actual = build_owner_eval_env_with_inputs(
            &assembler,
            "/src/owner.ts",
            &(),
            &[],
            &imported_inputs,
            Some(&overrides),
            None,
            None,
        )
        .expect("owner env should build");

        assert_eq!(
            actual
                .env
                .type_symbols
                .get("DepOnly")
                .map(|decl| &decl.body),
            Some(&TypeExpr::Primitive(PrimitiveName::Boolean))
        );
        assert_eq!(
            actual
                .env
                .type_symbols
                .get("Imported")
                .map(|decl| &decl.body),
            Some(&TypeExpr::Primitive(PrimitiveName::Boolean))
        );
        assert_eq!(
            actual.env.type_symbols.get("Local").map(|decl| &decl.body),
            Some(&TypeExpr::Primitive(PrimitiveName::String))
        );
        assert_eq!(
            actual
                .env
                .value_symbols
                .get("Local")
                .and_then(|decl| decl.type_annotation.as_ref()),
            Some(&TypeExpr::Primitive(PrimitiveName::Number))
        );
        assert_eq!(
            actual
                .env
                .value_symbols
                .get("runtimeOnly")
                .and_then(|decl| decl.type_annotation.as_ref()),
            Some(&TypeExpr::Primitive(PrimitiveName::Number))
        );
        assert_eq!(
            actual
                .env
                .value_symbols
                .get("shadowed")
                .and_then(|decl| decl.type_annotation.as_ref()),
            Some(&TypeExpr::Primitive(PrimitiveName::String))
        );
        assert!(actual.requested_binding_names.is_empty());
    }

    #[test]
    fn build_owner_eval_env_with_inputs_filters_runtime_values_to_requested_bindings() {
        let mut owner_env = EvalEnv::new();
        owner_env.add_type(TypeDeclInfo {
            name: "Local".to_string(),
            declaration_id: 1,
            kind: TypeDeclKind::Alias,
            type_parameters: Vec::new(),
            body: TypeExpr::Primitive(PrimitiveName::String),
        });

        let mut assembler = TestAssembler::default();
        assembler
            .base_envs
            .insert("/src/owner.ts".to_string(), Arc::new(owner_env));
        assembler.runtime_values.insert(
            "theme".to_string(),
            ValueDeclInfo {
                name: "theme".to_string(),
                declaration_id: 2,
                kind: ValueDeclKind::Const,
                type_annotation: Some(TypeExpr::Primitive(PrimitiveName::String)),
                function_signature: None,
                object_shape: None,
            },
        );
        assembler.runtime_values.insert(
            "helper".to_string(),
            ValueDeclInfo {
                name: "helper".to_string(),
                declaration_id: 3,
                kind: ValueDeclKind::Const,
                type_annotation: Some(TypeExpr::Primitive(PrimitiveName::Boolean)),
                function_signature: None,
                object_shape: None,
            },
        );

        let required_runtime_value_names = FxHashSet::from_iter(["theme".to_string()].into_iter());
        let actual = build_owner_eval_env_with_inputs(
            &assembler,
            "/src/owner.ts",
            &(),
            &[],
            &ImportedEvalInputs {
                sources: Vec::new(),
                type_aliases: Vec::new(),
                canonical_dependencies: BTreeSet::new(),
                overflow: None,
                stats: crate::resolver_core::ImportedEvalStats::default(),
            },
            None,
            None,
            Some(&required_runtime_value_names),
        )
        .expect("owner env should build");

        assert!(actual.env.value_symbols.contains_key("theme"));
        assert!(!actual.env.value_symbols.contains_key("helper"));
    }

    #[test]
    fn build_owner_eval_env_with_inputs_skips_missing_imported_dep_envs() {
        let mut owner_env = EvalEnv::new();
        owner_env.add_type(TypeDeclInfo {
            name: "Local".to_string(),
            declaration_id: 1,
            kind: TypeDeclKind::Alias,
            type_parameters: Vec::new(),
            body: TypeExpr::Primitive(PrimitiveName::String),
        });

        let mut assembler = TestAssembler::default();
        assembler
            .base_envs
            .insert("/src/owner.ts".to_string(), Arc::new(owner_env));

        let actual = build_owner_eval_env_with_inputs(
            &assembler,
            "/src/owner.ts",
            &(),
            &[],
            &ImportedEvalInputs {
                sources: vec![crate::resolver_core::ImportedEvalSource {
                    canonical_id: "/src/missing.ts".to_string(),
                }],
                type_aliases: Vec::new(),
                canonical_dependencies: BTreeSet::new(),
                overflow: None,
                stats: crate::resolver_core::ImportedEvalStats::default(),
            },
            None,
            None,
            None,
        )
        .expect("owner env should build");

        assert!(actual.env.type_symbols.contains_key("Local"));
        assert!(!actual.env.type_symbols.contains_key("DepOnly"));
    }
}
