use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_analysis::type_eval::EvalEnv;
use verter_analysis::type_expr::TypeExpr;
use verter_analysis::AnalyzedMacro;

use crate::fallthrough::inject_prop_type_overrides;
use crate::ImportedEvalInputs;

pub struct OwnerEvalEnvBuild {
    pub env: EvalEnv,
    pub requested_binding_names: FxHashSet<String>,
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
    let mut env = owner_env.or_else(|| {
        assembler
            .base_eval_env(canonical_id)
            .map(|env| (*env).clone())
    })?;
    let local_type_names: FxHashSet<String> = env.type_symbols.keys().cloned().collect();
    let local_value_names: FxHashSet<String> = env.value_symbols.keys().cloned().collect();
    let requested_binding_names = collect_requested_binding_names(macros);

    for dep_source in &imported_inputs.sources {
        let dep_env = assembler
            .base_eval_env(dep_source.canonical_id.as_str())
            .unwrap_or_else(|| {
                Arc::new(verter_analysis::type_eval_build::parse_and_build_env(
                    dep_source.source.as_ref(),
                ))
            });
        env.extend_missing_from_ref(dep_env.as_ref());
    }

    assembler.materialize_imported_type_aliases(
        snapshot,
        &local_type_names,
        imported_inputs,
        &mut env,
    );
    assembler.materialize_imported_runtime_values(
        snapshot,
        &local_value_names,
        required_runtime_value_names,
        &mut env,
    );

    if let Some(overrides) = prop_type_overrides {
        inject_prop_type_overrides(&mut env, overrides);
    }

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
    use crate::{ImportedEvalInputs, ImportedTypeAlias};
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use verter_analysis::type_eval::{
        EvalEnv, TypeDeclInfo, TypeDeclKind, ValueDeclInfo, ValueDeclKind,
    };
    use verter_analysis::type_expr::{PrimitiveName, TypeExpr};
    use verter_analysis::types::AnalyzedExposeField;
    use verter_analysis::{AnalyzedMacro, AnalyzedMacroKind};
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
            stats: crate::ImportedEvalStats::default(),
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
            sources: vec![crate::ImportedEvalSource {
                canonical_id: "/src/dep.ts".to_string(),
                source: std::sync::Arc::<str>::from("export interface DepOnly {}"),
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
            stats: crate::ImportedEvalStats::default(),
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
                stats: crate::ImportedEvalStats::default(),
            },
            None,
            None,
            Some(&required_runtime_value_names),
        )
        .expect("owner env should build");

        assert!(actual.env.value_symbols.contains_key("theme"));
        assert!(!actual.env.value_symbols.contains_key("helper"));
    }
}
