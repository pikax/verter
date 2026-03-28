use std::collections::BTreeSet;

use rustc_hash::FxHashMap;
use verter_analysis::type_eval::{
    BuiltinUtilitySource, EvalEnv, EvalLookup, TypeDeclInfo, ValueDeclInfo,
};
use verter_analysis::type_expr::{FunctionExpr, TypeExpr};
use verter_analysis::types::{AnalyzedImport, ImportBindingKind};

use crate::{resolve_type_declaration, DeclarationMetadataResolver, ResolvedExportTarget};

#[derive(Debug, Clone)]
pub struct ImportedTypeAliasResolveRequest {
    pub owner_canonical_id: String,
    pub import_source: String,
    pub local_name: String,
    pub imported_name: String,
    pub source_canonical_id: String,
    pub exported_name: String,
}

pub trait ImportedEvalLookupResolver: DeclarationMetadataResolver {
    fn resolve_import_canonical_id(
        &self,
        owner_canonical_id: &str,
        import: &AnalyzedImport,
    ) -> Option<String>;

    fn prepare_imported_type_alias(
        &mut self,
        request: ImportedTypeAliasResolveRequest,
        discovered_dependencies: &mut BTreeSet<String>,
    ) -> Option<TypeDeclInfo>;

    fn resolve_value_export_target(
        &self,
        dep_canonical_id: &str,
        imported_name: &str,
    ) -> Option<ResolvedExportTarget>;

    fn resolve_imported_type_root(
        &self,
        dep_canonical_id: &str,
        imported_name: &str,
    ) -> (String, String)
    where
        Self: Sized,
    {
        let declaration = resolve_type_declaration(self, dep_canonical_id, imported_name);
        let source_canonical_id = if declaration.canonical_source.is_empty() {
            dep_canonical_id.to_string()
        } else {
            declaration.canonical_source
        };
        let exported_name = if declaration.resolved_name.is_empty() {
            imported_name.to_string()
        } else {
            declaration.resolved_name
        };
        (source_canonical_id, exported_name)
    }

    fn dependency_eval_env(&self, canonical_id: &str) -> Option<EvalEnv>;
}

#[derive(Debug, Clone)]
struct ImportedTypeLookupTarget {
    import_source: String,
    dep_canonical_id: String,
    imported_name: String,
    local_name: String,
}

#[derive(Debug, Clone)]
struct ImportedValueLookupTarget {
    dep_canonical_id: String,
    source_canonical_id: String,
    source_name: String,
    local_name: String,
    remaining_path: Vec<String>,
}

pub struct ImportedEvalLookup<'a, R> {
    resolver: &'a mut R,
    owner_canonical_id: &'a str,
    imports: &'a [AnalyzedImport],
    discovered_dependencies: BTreeSet<String>,
    type_decl_cache: FxHashMap<String, Option<TypeDeclInfo>>,
    value_decl_cache: FxHashMap<Vec<String>, Option<ValueDeclInfo>>,
}

impl<'a, R> ImportedEvalLookup<'a, R> {
    pub fn new(
        resolver: &'a mut R,
        owner_canonical_id: &'a str,
        imports: &'a [AnalyzedImport],
    ) -> Self {
        Self {
            resolver,
            owner_canonical_id,
            imports,
            discovered_dependencies: BTreeSet::new(),
            type_decl_cache: FxHashMap::default(),
            value_decl_cache: FxHashMap::default(),
        }
    }

    pub fn into_discovered_dependencies(self) -> BTreeSet<String> {
        self.discovered_dependencies
    }
}

impl<R: ImportedEvalLookupResolver> ImportedEvalLookup<'_, R> {
    fn resolve_type_lookup_target(&self, name: &str) -> Option<ImportedTypeLookupTarget> {
        let (root_name, imported_name) = if let Some((root, member)) = name.split_once('.') {
            (root, Some(member.to_string()))
        } else {
            (name, None)
        };

        self.imports.iter().find_map(|import| {
            let binding = import.bindings.iter().find(|binding| {
                binding.name == root_name
                    && (binding.is_type_only || import.is_type_only)
                    && match (&imported_name, binding.kind) {
                        (Some(_), ImportBindingKind::Namespace) => true,
                        (None, ImportBindingKind::Namespace) => false,
                        (Some(_), _) => false,
                        (None, _) => true,
                    }
            })?;
            let dep_canonical_id = self
                .resolver
                .resolve_import_canonical_id(self.owner_canonical_id, import)?;
            let imported_name = imported_name.clone().unwrap_or_else(|| {
                binding
                    .imported_name
                    .clone()
                    .unwrap_or_else(|| binding.name.clone())
            });
            Some(ImportedTypeLookupTarget {
                import_source: import.source.clone(),
                dep_canonical_id,
                imported_name,
                local_name: name.to_string(),
            })
        })
    }

    fn resolve_value_lookup_target(&self, path: &[String]) -> Option<ImportedValueLookupTarget> {
        let root_name = path.first()?;

        self.imports.iter().find_map(|import| {
            let binding = import.bindings.iter().find(|binding| {
                !binding.is_type_only
                    && !import.is_type_only
                    && binding.name == *root_name
                    && match binding.kind {
                        ImportBindingKind::Namespace => path.len() >= 2,
                        _ => true,
                    }
            })?;
            let dep_canonical_id = self
                .resolver
                .resolve_import_canonical_id(self.owner_canonical_id, import)?;
            let (imported_name, remaining_path) = match binding.kind {
                ImportBindingKind::Namespace => (path.get(1)?.clone(), path[2..].to_vec()),
                _ => (
                    binding
                        .imported_name
                        .clone()
                        .unwrap_or_else(|| binding.name.clone()),
                    path[1..].to_vec(),
                ),
            };
            let resolved_export = self
                .resolver
                .resolve_value_export_target(&dep_canonical_id, &imported_name);
            let (source_canonical_id, source_name) = if let Some(export) = resolved_export {
                (
                    export
                        .source_canonical_id
                        .unwrap_or_else(|| dep_canonical_id.clone()),
                    export.source_name,
                )
            } else {
                (dep_canonical_id.clone(), imported_name)
            };
            Some(ImportedValueLookupTarget {
                dep_canonical_id,
                source_canonical_id,
                source_name,
                local_name: path.join("."),
                remaining_path,
            })
        })
    }

    fn project_value_member_path(
        &mut self,
        dep_env: &mut EvalEnv,
        decl: &ValueDeclInfo,
        remaining_path: &[String],
    ) -> Option<TypeExpr> {
        let mut current = if let Some(type_annotation) = decl.type_annotation.as_ref() {
            verter_analysis::type_eval::evaluate(type_annotation, dep_env)
        } else if let Some(function_signature) = decl.function_signature.as_ref() {
            TypeExpr::Function(std::sync::Arc::new(FunctionExpr {
                parameters: function_signature.parameters.clone(),
                return_type: function_signature
                    .return_type
                    .as_ref()
                    .map(|t| std::sync::Arc::new(t.clone())),
                type_parameters: function_signature.type_parameters.clone(),
            }))
        } else if let Some(object_shape) = decl.object_shape.as_ref() {
            TypeExpr::Object(std::sync::Arc::new(object_shape.clone()))
        } else {
            return None;
        };

        for segment in remaining_path {
            current = verter_analysis::type_eval::evaluate(
                &TypeExpr::IndexedAccess {
                    object: std::sync::Arc::new(current),
                    index: std::sync::Arc::new(TypeExpr::string_literal(segment.as_str())),
                },
                dep_env,
            );
        }

        Some(current)
    }
}

impl<R: ImportedEvalLookupResolver> EvalLookup for ImportedEvalLookup<'_, R> {
    fn resolve_type_decl(&mut self, name: &str) -> Option<TypeDeclInfo> {
        if let Some(cached) = self.type_decl_cache.get(name) {
            return cached.clone();
        }

        let resolved = self.resolve_type_lookup_target(name).and_then(|target| {
            let (source_canonical_id, exported_name) = self
                .resolver
                .resolve_imported_type_root(&target.dep_canonical_id, &target.imported_name);

            self.discovered_dependencies
                .insert(target.dep_canonical_id.clone());
            self.discovered_dependencies
                .insert(source_canonical_id.clone());

            self.resolver.prepare_imported_type_alias(
                ImportedTypeAliasResolveRequest {
                    owner_canonical_id: self.owner_canonical_id.to_string(),
                    import_source: target.import_source,
                    local_name: target.local_name,
                    imported_name: target.imported_name,
                    source_canonical_id,
                    exported_name,
                },
                &mut self.discovered_dependencies,
            )
        });

        self.type_decl_cache
            .insert(name.to_string(), resolved.clone());
        resolved
    }

    fn resolve_value_decl(&mut self, path: &[String]) -> Option<ValueDeclInfo> {
        if let Some(cached) = self.value_decl_cache.get(path) {
            return cached.clone();
        }

        let resolved = self.resolve_value_lookup_target(path).and_then(|target| {
            self.discovered_dependencies
                .insert(target.dep_canonical_id.clone());
            self.discovered_dependencies
                .insert(target.source_canonical_id.clone());
            let mut dep_env = self
                .resolver
                .dependency_eval_env(&target.source_canonical_id)?;
            let mut decl = dep_env.value_symbols.get(&target.source_name).cloned()?;
            decl.name = target.local_name.clone();

            if target.remaining_path.is_empty() {
                return Some(decl);
            }

            let projected =
                self.project_value_member_path(&mut dep_env, &decl, &target.remaining_path)?;
            Some(ValueDeclInfo {
                name: target.local_name,
                declaration_id: 0,
                kind: decl.kind,
                type_annotation: Some(projected),
                function_signature: None,
                object_shape: None,
            })
        });

        self.value_decl_cache
            .insert(path.to_vec(), resolved.clone());
        resolved
    }

    fn utility_source(&mut self, name: &str) -> BuiltinUtilitySource {
        if self
            .imports
            .iter()
            .flat_map(|import| import.bindings.iter())
            .any(|binding| binding.name == name)
        {
            return BuiltinUtilitySource::Shadowed;
        }

        match name {
            "Partial"
            | "Required"
            | "Readonly"
            | "Pick"
            | "Omit"
            | "Record"
            | "Extract"
            | "Exclude"
            | "NonNullable"
            | "ReturnType"
            | "Parameters"
            | "ConstructorParameters"
            | "InstanceType"
            | "Awaited" => BuiltinUtilitySource::Builtin,
            _ => BuiltinUtilitySource::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    use verter_analysis::type_eval::{TypeDeclKind, ValueDeclKind};
    use verter_analysis::type_expr::{LiteralValue, ObjectMember, ObjectProperty, PrimitiveName};
    use verter_span::Span;

    #[derive(Default)]
    struct TestResolver {
        type_requests: RefCell<Vec<ImportedTypeAliasResolveRequest>>,
        value_exports: FxHashMap<(String, String), ResolvedExportTarget>,
        dep_envs: FxHashMap<String, EvalEnv>,
        root_targets: FxHashMap<(String, String), (String, String)>,
        declaration_lookups: RefCell<u32>,
        root_lookups: RefCell<u32>,
    }

    impl DeclarationMetadataResolver for TestResolver {
        fn resolve_export_target(
            &self,
            _dep_canonical: &str,
            _requested_name: &str,
        ) -> Option<ResolvedExportTarget> {
            None
        }

        fn get_export_span_follow_reexports(
            &self,
            _dep_canonical: &str,
            _requested_name: &str,
        ) -> Option<Span> {
            None
        }

        fn read_source(&self, _canonical_source: &str) -> Option<String> {
            self.record_declaration_lookup();
            None
        }

        fn type_declaration_id(
            &self,
            _canonical_source: &str,
            _resolved_name: &str,
        ) -> Option<verter_analysis::type_eval::DeclarationId> {
            None
        }

        fn resolve_type_dependency_canonical(
            &self,
            _from_canonical: &str,
            _import_source: &str,
        ) -> Option<String> {
            None
        }
    }

    impl ImportedEvalLookupResolver for TestResolver {
        fn resolve_import_canonical_id(
            &self,
            _owner_canonical_id: &str,
            import: &AnalyzedImport,
        ) -> Option<String> {
            import.resolved_canonical_id.clone()
        }

        fn prepare_imported_type_alias(
            &mut self,
            request: ImportedTypeAliasResolveRequest,
            _discovered_dependencies: &mut BTreeSet<String>,
        ) -> Option<TypeDeclInfo> {
            self.type_requests.borrow_mut().push(request.clone());
            Some(TypeDeclInfo {
                name: request.local_name,
                declaration_id: 0,
                kind: TypeDeclKind::Alias,
                type_parameters: Vec::new(),
                body: TypeExpr::Primitive(PrimitiveName::String),
            })
        }

        fn resolve_value_export_target(
            &self,
            dep_canonical_id: &str,
            imported_name: &str,
        ) -> Option<ResolvedExportTarget> {
            self.value_exports
                .get(&(dep_canonical_id.to_string(), imported_name.to_string()))
                .cloned()
        }

        fn resolve_imported_type_root(
            &self,
            dep_canonical_id: &str,
            imported_name: &str,
        ) -> (String, String) {
            *self.root_lookups.borrow_mut() += 1;
            self.root_targets
                .get(&(dep_canonical_id.to_string(), imported_name.to_string()))
                .cloned()
                .unwrap_or_else(|| (dep_canonical_id.to_string(), imported_name.to_string()))
        }

        fn dependency_eval_env(&self, canonical_id: &str) -> Option<EvalEnv> {
            self.dep_envs.get(canonical_id).cloned()
        }
    }

    impl TestResolver {
        fn record_declaration_lookup(&self) {
            *self.declaration_lookups.borrow_mut() += 1;
        }
    }

    fn analyzed_import(
        source: &str,
        resolved_canonical_id: Option<&str>,
        bindings: Vec<verter_analysis::types::AnalyzedImportBinding>,
        is_type_only: bool,
    ) -> AnalyzedImport {
        AnalyzedImport {
            source: source.to_string(),
            is_type_only,
            bindings,
            span: Span::new(0, 0),
            resolved_canonical_id: resolved_canonical_id.map(str::to_string),
        }
    }

    fn binding(
        name: &str,
        kind: ImportBindingKind,
        imported_name: Option<&str>,
        is_type_only: bool,
    ) -> verter_analysis::types::AnalyzedImportBinding {
        verter_analysis::types::AnalyzedImportBinding {
            name: name.to_string(),
            kind,
            imported_name: imported_name.map(str::to_string),
            is_type_only,
            vue_api: None,
            span: Span::new(0, 0),
        }
    }

    #[test]
    fn imported_eval_lookup_resolves_namespace_type_members() {
        let imports = vec![analyzed_import(
            "./dep",
            Some("/src/dep.ts"),
            vec![binding("Types", ImportBindingKind::Namespace, None, true)],
            true,
        )];
        let mut resolver = TestResolver::default();
        let mut lookup = ImportedEvalLookup::new(&mut resolver, "/src/App.vue", &imports);

        let resolved = lookup.resolve_type_decl("Types.User").unwrap();

        assert_eq!(resolved.name, "Types.User");
        let requests = resolver.type_requests.borrow();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].imported_name, "User");
        assert_eq!(requests[0].source_canonical_id, "/src/dep.ts");
    }

    #[test]
    fn imported_eval_lookup_projects_namespace_value_member_paths() {
        let imports = vec![analyzed_import(
            "./dep",
            Some("/src/dep.ts"),
            vec![binding("Ns", ImportBindingKind::Namespace, None, false)],
            false,
        )];
        let mut resolver = TestResolver::default();
        resolver.value_exports.insert(
            ("/src/dep.ts".to_string(), "theme".to_string()),
            ResolvedExportTarget {
                source_canonical_id: Some("/src/shared.ts".to_string()),
                source_name: "themeConfig".to_string(),
            },
        );
        let mut env = EvalEnv::new();
        env.add_value(ValueDeclInfo {
            name: "themeConfig".to_string(),
            declaration_id: 0,
            kind: ValueDeclKind::Const,
            type_annotation: None,
            function_signature: None,
            object_shape: Some(verter_analysis::type_expr::ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "slots".to_string(),
                    ty: TypeExpr::Literal(LiteralValue::String("panel".to_string())),
                    optional: false,
                    readonly: false,
                })],
            }),
        });
        resolver.dep_envs.insert("/src/shared.ts".to_string(), env);
        let mut lookup = ImportedEvalLookup::new(&mut resolver, "/src/App.vue", &imports);

        let resolved = lookup
            .resolve_value_decl(&["Ns".to_string(), "theme".to_string(), "slots".to_string()])
            .unwrap();

        assert_eq!(resolved.name, "Ns.theme.slots");
        assert_eq!(
            resolved.type_annotation,
            Some(TypeExpr::Literal(LiteralValue::String("panel".to_string())))
        );
        assert!(resolved.function_signature.is_none());
        assert!(resolved.object_shape.is_none());
    }

    #[test]
    fn imported_eval_lookup_marks_shadowed_builtin_names() {
        let imports = vec![analyzed_import(
            "local-utils",
            Some("/src/utils.ts"),
            vec![binding(
                "Pick",
                ImportBindingKind::Named,
                Some("Pick"),
                false,
            )],
            false,
        )];
        let mut resolver = TestResolver::default();
        let mut lookup = ImportedEvalLookup::new(&mut resolver, "/src/App.vue", &imports);

        assert_eq!(
            lookup.utility_source("Pick"),
            BuiltinUtilitySource::Shadowed
        );
        assert_eq!(
            lookup.utility_source("Readonly"),
            BuiltinUtilitySource::Builtin
        );
    }

    #[test]
    fn imported_eval_lookup_prefers_cached_root_lookup_over_full_declaration_resolution() {
        let imports = vec![analyzed_import(
            "./dep",
            Some("/src/dep.ts"),
            vec![binding("Types", ImportBindingKind::Namespace, None, true)],
            true,
        )];
        let mut resolver = TestResolver::default();
        resolver.root_targets.insert(
            ("/src/dep.ts".to_string(), "User".to_string()),
            ("/src/types.ts".to_string(), "ResolvedUser".to_string()),
        );
        let mut lookup = ImportedEvalLookup::new(&mut resolver, "/src/App.vue", &imports);

        let resolved = lookup.resolve_type_decl("Types.User").unwrap();

        assert_eq!(resolved.name, "Types.User");
        assert_eq!(*resolver.root_lookups.borrow(), 1);
        assert_eq!(
            *resolver.declaration_lookups.borrow(),
            0,
            "cached root lookup should avoid full declaration resolution in imported lookup",
        );
        let requests = resolver.type_requests.borrow();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].source_canonical_id, "/src/types.ts");
        assert_eq!(requests[0].exported_name, "ResolvedUser");
    }
}
