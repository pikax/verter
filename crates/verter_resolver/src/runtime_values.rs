use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_analysis::type_eval::EvalEnv;
use verter_analysis::types::{AnalyzedImport, ImportBindingKind};

pub trait ImportedRuntimeValueResolver {
    fn dependency_eval_env(&self, canonical_id: &str) -> Option<Arc<EvalEnv>>;
}

pub fn materialize_imported_runtime_values_into_env<R: ImportedRuntimeValueResolver>(
    imports: &[AnalyzedImport],
    owner_local_value_names: &FxHashSet<String>,
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

        let dep_env = dep_env_cache
            .entry(dep_canonical_id.to_string())
            .or_insert_with(|| resolver.dependency_eval_env(dep_canonical_id));
        let Some(dep_env) = dep_env.as_ref() else {
            continue;
        };

        for binding in &import.bindings {
            if binding.is_type_only || matches!(binding.kind, ImportBindingKind::Namespace) {
                continue;
            }
            if owner_local_value_names.contains(&binding.name) {
                continue;
            }

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
    use std::sync::Arc;
    use verter_analysis::type_eval::{EvalEnv, ValueDeclInfo, ValueDeclKind};
    use verter_analysis::types::{AnalyzedImport, AnalyzedImportBinding, ImportBindingKind};
    use verter_span::Span;

    #[derive(Default)]
    struct TestResolver {
        dep_envs: FxHashMap<String, Arc<EvalEnv>>,
    }

    impl ImportedRuntimeValueResolver for TestResolver {
        fn dependency_eval_env(&self, canonical_id: &str) -> Option<Arc<EvalEnv>> {
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
            type_annotation: Some(verter_analysis::type_expr::TypeExpr::string_literal("dark")),
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
            type_annotation: Some(verter_analysis::type_expr::TypeExpr::string_literal("dark")),
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
            type_annotation: Some(verter_analysis::type_expr::TypeExpr::string_literal(
                "local",
            )),
            function_signature: None,
            object_shape: None,
        });
        materialize_imported_runtime_values_into_env(
            &imports,
            &FxHashSet::from_iter(["theme".to_string()].into_iter()),
            &mut env,
            &resolver,
        );

        assert_eq!(
            env.value_symbols
                .get("theme")
                .and_then(|value| value.type_annotation.clone()),
            Some(verter_analysis::type_expr::TypeExpr::string_literal(
                "local"
            ))
        );
    }
}
