use std::collections::BTreeSet;
use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_semantic::analysis::type_eval::{EvalEnv, TypeDeclInfo};
use verter_semantic::analysis::type_expand::{
    expand_object_shape, ExpandedObjectShape, ExpansionBudget,
};
use verter_semantic::analysis::type_expr::{
    FunctionExpr, FunctionParam, IndexSignature, ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
};
use verter_semantic::analysis::{AnalyzedBinding, AnalyzedImport, AnalyzedMacro, MacroTypeDep};

use crate::{choose_preferred_imported_type_body, ImportedEvalInputs, ImportedEvalOwnerSnapshot};

#[derive(Debug, Clone)]
pub struct PreparedImportedDeclContext {
    pub imports: Vec<AnalyzedImport>,
    pub macros: Vec<AnalyzedMacro>,
    pub bindings: Vec<AnalyzedBinding>,
    pub macro_type_deps: Vec<MacroTypeDep>,
    pub eval_source: String,
    pub env: EvalEnv,
    pub decl: TypeDeclInfo,
}

impl PreparedImportedDeclContext {
    pub fn owner_snapshot(&self) -> ImportedEvalOwnerSnapshot<'_> {
        ImportedEvalOwnerSnapshot {
            imports: &self.imports,
            macros: &self.macros,
            bindings: &self.bindings,
            macro_type_deps: &self.macro_type_deps,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CachedEvaluatedImportedDecl {
    pub body: Arc<TypeExpr>,
    pub canonical_dependencies: BTreeSet<String>,
}

pub trait ImportedDeclEvalResolver {
    fn budget_is_exhausted(&self) -> bool;

    fn canonicalize_imported_source(&self, source_canonical_id: &str) -> String;

    fn enter_alias_env(&mut self, canonical_id: &str) -> bool;

    fn leave_alias_env(&mut self, canonical_id: &str);

    fn load_imported_decl_context(
        &self,
        source_canonical_id: &str,
        exported_name: &str,
    ) -> Option<PreparedImportedDeclContext>;

    fn required_import_names_for_exported_type(
        &self,
        source_canonical_id: &str,
        exported_name: &str,
        eval_source: &str,
    ) -> FxHashSet<String> {
        let _ = source_canonical_id;
        let alloc = oxc_allocator::Allocator::new();
        verter_compiler::utils::oxc::vue::resolve_type::collect_required_import_names_for_external_type(
            exported_name,
            eval_source,
            &alloc,
        )
    }

    fn required_import_names_for_decl(
        &self,
        source_canonical_id: &str,
        exported_name: &str,
        decl: &TypeDeclInfo,
        owner_env: &EvalEnv,
    ) -> FxHashSet<String>;

    fn build_imported_inputs_for_decl(
        &mut self,
        owner_canonical_id: &str,
        context: &PreparedImportedDeclContext,
        additional_required_import_names: &FxHashSet<String>,
    ) -> ImportedEvalInputs;

    fn build_owner_eval_env_for_decl(
        &self,
        canonical_id: &str,
        context: &PreparedImportedDeclContext,
        imported_inputs: &ImportedEvalInputs,
    ) -> Option<EvalEnv>;

    fn cached_evaluated_decl(
        &self,
        _source_canonical_id: &str,
        _exported_name: &str,
    ) -> Option<CachedEvaluatedImportedDecl> {
        None
    }

    fn cache_evaluated_decl(
        &self,
        _source_canonical_id: &str,
        _exported_name: &str,
        _cached: CachedEvaluatedImportedDecl,
    ) {
    }
}

#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn evaluate_imported_decl_with_owner_env<R: ImportedDeclEvalResolver>(
    resolver: &mut R,
    source_canonical_id: &str,
    exported_name: &str,
    canonical_dependencies: &mut BTreeSet<String>,
) -> Option<TypeExpr> {
    if resolver.budget_is_exhausted() {
        return None;
    }

    let resolved_source_canonical_id = resolver.canonicalize_imported_source(source_canonical_id);
    if let Some(cached) =
        resolver.cached_evaluated_decl(&resolved_source_canonical_id, exported_name)
    {
        canonical_dependencies.extend(cached.canonical_dependencies.iter().cloned());
        return Some((*cached.body).clone());
    }
    if !resolver.enter_alias_env(&resolved_source_canonical_id) {
        return None;
    }

    let result = (|| {
        let context =
            resolver.load_imported_decl_context(&resolved_source_canonical_id, exported_name)?;
        let mut decl_required_import_names = resolver.required_import_names_for_exported_type(
            &resolved_source_canonical_id,
            exported_name,
            context.eval_source.as_str(),
        );
        if decl_required_import_names.is_empty() && !context.imports.is_empty() {
            decl_required_import_names = resolver.required_import_names_for_decl(
                &resolved_source_canonical_id,
                exported_name,
                &context.decl,
                &context.env,
            );
        }
        let imported_inputs = resolver.build_imported_inputs_for_decl(
            &resolved_source_canonical_id,
            &context,
            &decl_required_import_names,
        );
        canonical_dependencies.extend(imported_inputs.canonical_dependencies.iter().cloned());
        if imported_inputs.overflow.is_some() {
            return None;
        }
        let mut dep_env = resolver.build_owner_eval_env_for_decl(
            &resolved_source_canonical_id,
            &context,
            &imported_inputs,
        )?;
        let decl = dep_env
            .type_symbols
            .get(context.decl.name.as_str())
            .or_else(|| dep_env.type_symbols.get(exported_name))?
            .clone();
        for param in &decl.type_parameters {
            dep_env.type_bindings.insert(
                param.name.clone(),
                std::sync::Arc::new(TypeExpr::type_parameter(param.clone())),
            );
        }
        let evaluated = verter_semantic::analysis::type_eval::evaluate(&decl.body, &mut dep_env);
        let cached = CachedEvaluatedImportedDecl {
            body: Arc::new(evaluated.clone()),
            canonical_dependencies: canonical_dependencies.clone(),
        };
        resolver.cache_evaluated_decl(&resolved_source_canonical_id, exported_name, cached);
        Some(evaluated)
    })();

    resolver.leave_alias_env(&resolved_source_canonical_id);
    result
}

pub fn materialize_imported_decl_with_owner_env<R: ImportedDeclEvalResolver>(
    resolver: &mut R,
    source_canonical_id: &str,
    exported_name: &str,
    canonical_dependencies: &mut BTreeSet<String>,
) -> Option<TypeExpr> {
    if resolver.budget_is_exhausted() {
        return None;
    }

    let resolved_source_canonical_id = resolver.canonicalize_imported_source(source_canonical_id);
    if !resolver.enter_alias_env(&resolved_source_canonical_id) {
        return None;
    }

    let result = (|| {
        let context =
            resolver.load_imported_decl_context(&resolved_source_canonical_id, exported_name)?;
        let mut decl_required_import_names = resolver.required_import_names_for_exported_type(
            &resolved_source_canonical_id,
            exported_name,
            context.eval_source.as_str(),
        );
        if decl_required_import_names.is_empty() && !context.imports.is_empty() {
            decl_required_import_names = resolver.required_import_names_for_decl(
                &resolved_source_canonical_id,
                exported_name,
                &context.decl,
                &context.env,
            );
        }
        let imported_inputs = resolver.build_imported_inputs_for_decl(
            &resolved_source_canonical_id,
            &context,
            &decl_required_import_names,
        );
        canonical_dependencies.extend(imported_inputs.canonical_dependencies.iter().cloned());
        if imported_inputs.overflow.is_some() {
            return None;
        }
        let mut dep_env = resolver.build_owner_eval_env_for_decl(
            &resolved_source_canonical_id,
            &context,
            &imported_inputs,
        )?;
        let decl = dep_env
            .type_symbols
            .get(context.decl.name.as_str())
            .or_else(|| dep_env.type_symbols.get(exported_name))?
            .clone();
        for param in &decl.type_parameters {
            dep_env.type_bindings.insert(
                param.name.clone(),
                std::sync::Arc::new(TypeExpr::type_parameter(param.clone())),
            );
        }
        let evaluated = verter_semantic::analysis::type_eval::evaluate(&decl.body, &mut dep_env);
        let materialized = materialize_imported_decl_body(&evaluated, &mut dep_env);
        choose_preferred_imported_type_body(Some(evaluated), materialized)
    })();

    resolver.leave_alias_env(&resolved_source_canonical_id);
    result
}

fn materialize_imported_decl_body(expr: &TypeExpr, env: &mut EvalEnv) -> Option<TypeExpr> {
    let expanded = expand_object_shape(expr, env, &ExpansionBudget::default());
    expanded_object_shape_to_type_expr(&expanded.value)
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

#[cfg(test)]
mod tests {
    use super::{
        evaluate_imported_decl_with_owner_env, materialize_imported_decl_with_owner_env,
        CachedEvaluatedImportedDecl, ImportedDeclEvalResolver, PreparedImportedDeclContext,
    };
    use crate::{ImportedEvalInputs, ImportedEvalOverflow};
    use rustc_hash::FxHashSet;
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use verter_semantic::analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
    use verter_semantic::analysis::type_expr::{
        ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr, TypeParam,
    };

    struct TestResolver {
        exhausted: bool,
        allow_alias_enter: bool,
        contexts: std::collections::BTreeMap<(String, String), PreparedImportedDeclContext>,
        entered: RefCell<Vec<String>>,
        left: RefCell<Vec<String>>,
        built_inputs: ImportedEvalInputs,
        cached: RefCell<std::collections::BTreeMap<(String, String), CachedEvaluatedImportedDecl>>,
        build_inputs_calls: RefCell<u32>,
        build_env_calls: RefCell<u32>,
    }

    impl ImportedDeclEvalResolver for TestResolver {
        fn budget_is_exhausted(&self) -> bool {
            self.exhausted
        }

        fn canonicalize_imported_source(&self, source_canonical_id: &str) -> String {
            source_canonical_id.to_string()
        }

        fn enter_alias_env(&mut self, canonical_id: &str) -> bool {
            self.entered.borrow_mut().push(canonical_id.to_string());
            self.allow_alias_enter
        }

        fn leave_alias_env(&mut self, canonical_id: &str) {
            self.left.borrow_mut().push(canonical_id.to_string());
        }

        fn load_imported_decl_context(
            &self,
            source_canonical_id: &str,
            exported_name: &str,
        ) -> Option<PreparedImportedDeclContext> {
            self.contexts
                .get(&(source_canonical_id.to_string(), exported_name.to_string()))
                .cloned()
        }

        fn required_import_names_for_decl(
            &self,
            _source_canonical_id: &str,
            _exported_name: &str,
            _decl: &TypeDeclInfo,
            _owner_env: &EvalEnv,
        ) -> FxHashSet<String> {
            FxHashSet::default()
        }

        fn build_imported_inputs_for_decl(
            &mut self,
            _owner_canonical_id: &str,
            _context: &PreparedImportedDeclContext,
            _additional_required_import_names: &FxHashSet<String>,
        ) -> ImportedEvalInputs {
            *self.build_inputs_calls.borrow_mut() += 1;
            self.built_inputs.clone()
        }

        fn build_owner_eval_env_for_decl(
            &self,
            _canonical_id: &str,
            context: &PreparedImportedDeclContext,
            _imported_inputs: &ImportedEvalInputs,
        ) -> Option<EvalEnv> {
            *self.build_env_calls.borrow_mut() += 1;
            Some(context.env.clone())
        }

        fn cached_evaluated_decl(
            &self,
            source_canonical_id: &str,
            exported_name: &str,
        ) -> Option<CachedEvaluatedImportedDecl> {
            self.cached
                .borrow()
                .get(&(source_canonical_id.to_string(), exported_name.to_string()))
                .cloned()
        }

        fn cache_evaluated_decl(
            &self,
            source_canonical_id: &str,
            exported_name: &str,
            cached: CachedEvaluatedImportedDecl,
        ) {
            self.cached.borrow_mut().insert(
                (source_canonical_id.to_string(), exported_name.to_string()),
                cached,
            );
        }
    }

    fn decl(name: &str, body: TypeExpr) -> TypeDeclInfo {
        TypeDeclInfo {
            name: name.to_string(),
            declaration_id: 1,
            kind: TypeDeclKind::Alias,
            type_parameters: Vec::new(),
            body,
        }
    }

    fn generic_type_param(name: &str) -> TypeParam {
        TypeParam {
            name: name.to_string(),
            constraint: Some(std::sync::Arc::new(TypeExpr::Primitive(
                PrimitiveName::Number,
            ))),
            default: Some(std::sync::Arc::new(TypeExpr::Primitive(
                PrimitiveName::String,
            ))),
        }
    }

    #[test]
    fn evaluate_imported_decl_with_owner_env_evaluates_decl_body() {
        let mut env = EvalEnv::new();
        env.add_type(decl("Props", TypeExpr::Primitive(PrimitiveName::String)));
        let mut contexts = std::collections::BTreeMap::new();
        contexts.insert(
            ("/src/types.ts".to_string(), "Props".to_string()),
            PreparedImportedDeclContext {
                imports: Vec::new(),
                macros: Vec::new(),
                bindings: Vec::new(),
                macro_type_deps: Vec::new(),
                eval_source: "export interface Props {}".to_string(),
                env,
                decl: decl("Props", TypeExpr::Primitive(PrimitiveName::String)),
            },
        );
        let mut resolver = TestResolver {
            exhausted: false,
            allow_alias_enter: true,
            contexts,
            entered: RefCell::new(Vec::new()),
            left: RefCell::new(Vec::new()),
            built_inputs: ImportedEvalInputs {
                sources: Vec::new(),
                type_aliases: Vec::new(),
                canonical_dependencies: BTreeSet::from_iter(
                    ["/src/dep.ts".to_string()].into_iter(),
                ),
                overflow: None,
                stats: crate::ImportedEvalStats::default(),
            },
            cached: RefCell::new(std::collections::BTreeMap::new()),
            build_inputs_calls: RefCell::new(0),
            build_env_calls: RefCell::new(0),
        };
        let mut deps = BTreeSet::new();

        let actual = evaluate_imported_decl_with_owner_env(
            &mut resolver,
            "/src/types.ts",
            "Props",
            &mut deps,
        );

        assert_eq!(actual, Some(TypeExpr::Primitive(PrimitiveName::String)));
        assert!(deps.contains("/src/dep.ts"));
        assert_eq!(resolver.entered.borrow().as_slice(), ["/src/types.ts"]);
        assert_eq!(resolver.left.borrow().as_slice(), ["/src/types.ts"]);
    }

    #[test]
    fn evaluate_imported_decl_with_owner_env_stops_on_overflow() {
        let mut env = EvalEnv::new();
        env.add_type(decl("Props", TypeExpr::Primitive(PrimitiveName::String)));
        let mut contexts = std::collections::BTreeMap::new();
        contexts.insert(
            ("/src/types.ts".to_string(), "Props".to_string()),
            PreparedImportedDeclContext {
                imports: Vec::new(),
                macros: Vec::new(),
                bindings: Vec::new(),
                macro_type_deps: Vec::new(),
                eval_source: "export interface Props {}".to_string(),
                env,
                decl: decl("Props", TypeExpr::Primitive(PrimitiveName::String)),
            },
        );
        let mut resolver = TestResolver {
            exhausted: false,
            allow_alias_enter: true,
            contexts,
            entered: RefCell::new(Vec::new()),
            left: RefCell::new(Vec::new()),
            built_inputs: ImportedEvalInputs {
                sources: Vec::new(),
                type_aliases: Vec::new(),
                canonical_dependencies: BTreeSet::from_iter(
                    ["/src/dep.ts".to_string()].into_iter(),
                ),
                overflow: Some(ImportedEvalOverflow {
                    message: "overflow".to_string(),
                }),
                stats: crate::ImportedEvalStats::default(),
            },
            cached: RefCell::new(std::collections::BTreeMap::new()),
            build_inputs_calls: RefCell::new(0),
            build_env_calls: RefCell::new(0),
        };

        let actual = evaluate_imported_decl_with_owner_env(
            &mut resolver,
            "/src/types.ts",
            "Props",
            &mut BTreeSet::new(),
        );

        assert!(actual.is_none());
        assert_eq!(resolver.left.borrow().as_slice(), ["/src/types.ts"]);
    }

    #[test]
    fn evaluate_imported_decl_with_owner_env_preserves_generic_parameter_metadata() {
        let generic = generic_type_param("T");
        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "Props".to_string(),
            declaration_id: 1,
            kind: TypeDeclKind::Alias,
            type_parameters: vec![generic.clone()],
            body: TypeExpr::named("T"),
        });
        let mut contexts = std::collections::BTreeMap::new();
        contexts.insert(
            ("/src/types.ts".to_string(), "Props".to_string()),
            PreparedImportedDeclContext {
                imports: Vec::new(),
                macros: Vec::new(),
                bindings: Vec::new(),
                macro_type_deps: Vec::new(),
                eval_source: "export type Props<T extends number = string> = T".to_string(),
                env,
                decl: TypeDeclInfo {
                    name: "Props".to_string(),
                    declaration_id: 1,
                    kind: TypeDeclKind::Alias,
                    type_parameters: vec![generic.clone()],
                    body: TypeExpr::named("T"),
                },
            },
        );
        let mut resolver = TestResolver {
            exhausted: false,
            allow_alias_enter: true,
            contexts,
            entered: RefCell::new(Vec::new()),
            left: RefCell::new(Vec::new()),
            built_inputs: ImportedEvalInputs {
                sources: Vec::new(),
                type_aliases: Vec::new(),
                canonical_dependencies: BTreeSet::new(),
                overflow: None,
                stats: crate::ImportedEvalStats::default(),
            },
            cached: RefCell::new(std::collections::BTreeMap::new()),
            build_inputs_calls: RefCell::new(0),
            build_env_calls: RefCell::new(0),
        };

        let actual = evaluate_imported_decl_with_owner_env(
            &mut resolver,
            "/src/types.ts",
            "Props",
            &mut BTreeSet::new(),
        );

        assert_eq!(actual, Some(TypeExpr::TypeParameter(generic)));
    }

    #[test]
    fn evaluate_imported_decl_with_owner_env_uses_resolved_decl_name_for_re_exports() {
        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "Lt".to_string(),
            declaration_id: 1,
            kind: TypeDeclKind::Alias,
            type_parameters: Vec::new(),
            body: TypeExpr::union(vec![
                TypeExpr::Primitive(PrimitiveName::String),
                TypeExpr::named("St"),
                TypeExpr::named("vt"),
            ]),
        });
        env.add_type(TypeDeclInfo {
            name: "St".to_string(),
            declaration_id: 2,
            kind: TypeDeclKind::Interface,
            type_parameters: Vec::new(),
            body: TypeExpr::Object(std::sync::Arc::new(
                verter_semantic::analysis::type_expr::ObjectExpr {
                    properties: vec![
                        verter_semantic::analysis::type_expr::ObjectMember::Property(
                            verter_semantic::analysis::type_expr::ObjectProperty {
                                name: "path".to_string(),
                                ty: TypeExpr::Primitive(PrimitiveName::String),
                                optional: false,
                                readonly: false,
                            },
                        ),
                    ],
                },
            )),
        });
        env.add_type(TypeDeclInfo {
            name: "vt".to_string(),
            declaration_id: 3,
            kind: TypeDeclKind::Interface,
            type_parameters: Vec::new(),
            body: TypeExpr::Object(std::sync::Arc::new(
                verter_semantic::analysis::type_expr::ObjectExpr {
                    properties: vec![
                        verter_semantic::analysis::type_expr::ObjectMember::Property(
                            verter_semantic::analysis::type_expr::ObjectProperty {
                                name: "name".to_string(),
                                ty: TypeExpr::Primitive(PrimitiveName::String),
                                optional: false,
                                readonly: false,
                            },
                        ),
                    ],
                },
            )),
        });
        let mut contexts = std::collections::BTreeMap::new();
        contexts.insert(
            (
                "/node_modules/vue-router/dist/index-typed.d.ts".to_string(),
                "RouteLocationRaw".to_string(),
            ),
            PreparedImportedDeclContext {
                imports: Vec::new(),
                macros: Vec::new(),
                bindings: Vec::new(),
                macro_type_deps: Vec::new(),
                eval_source: "export type Lt = string | St | vt".to_string(),
                env,
                decl: decl(
                    "Lt",
                    TypeExpr::union(vec![
                        TypeExpr::Primitive(PrimitiveName::String),
                        TypeExpr::named("St"),
                        TypeExpr::named("vt"),
                    ]),
                ),
            },
        );
        let mut resolver = TestResolver {
            exhausted: false,
            allow_alias_enter: true,
            contexts,
            entered: RefCell::new(Vec::new()),
            left: RefCell::new(Vec::new()),
            built_inputs: ImportedEvalInputs {
                sources: Vec::new(),
                type_aliases: Vec::new(),
                canonical_dependencies: BTreeSet::new(),
                overflow: None,
                stats: crate::ImportedEvalStats::default(),
            },
            cached: RefCell::new(std::collections::BTreeMap::new()),
            build_inputs_calls: RefCell::new(0),
            build_env_calls: RefCell::new(0),
        };

        let actual = evaluate_imported_decl_with_owner_env(
            &mut resolver,
            "/node_modules/vue-router/dist/index-typed.d.ts",
            "RouteLocationRaw",
            &mut BTreeSet::new(),
        );

        let Some(TypeExpr::Union(types)) = actual else {
            panic!("expected union route surface");
        };
        assert_eq!(types.len(), 3);
        assert!(types
            .iter()
            .any(|ty| matches!(ty, TypeExpr::Primitive(PrimitiveName::String))));
        assert!(types.iter().any(|ty| match ty {
            TypeExpr::Object(obj) => obj.properties.iter().any(|member| {
                matches!(
                    member,
                    verter_semantic::analysis::type_expr::ObjectMember::Property(prop)
                        if prop.name == "path"
                )
            }),
            _ => false,
        }));
        assert!(types.iter().any(|ty| match ty {
            TypeExpr::Object(obj) => obj.properties.iter().any(|member| {
                matches!(
                    member,
                    verter_semantic::analysis::type_expr::ObjectMember::Property(prop)
                        if prop.name == "name"
                )
            }),
            _ => false,
        }));
    }

    #[test]
    fn evaluate_imported_decl_with_owner_env_reuses_cached_body_and_dependencies() {
        let mut env = EvalEnv::new();
        env.add_type(decl("Props", TypeExpr::Primitive(PrimitiveName::String)));
        let mut contexts = std::collections::BTreeMap::new();
        contexts.insert(
            ("/src/types.ts".to_string(), "Props".to_string()),
            PreparedImportedDeclContext {
                imports: Vec::new(),
                macros: Vec::new(),
                bindings: Vec::new(),
                macro_type_deps: Vec::new(),
                eval_source: "export interface Props {}".to_string(),
                env,
                decl: decl("Props", TypeExpr::Primitive(PrimitiveName::String)),
            },
        );
        let mut resolver = TestResolver {
            exhausted: false,
            allow_alias_enter: true,
            contexts,
            entered: RefCell::new(Vec::new()),
            left: RefCell::new(Vec::new()),
            built_inputs: ImportedEvalInputs {
                sources: Vec::new(),
                type_aliases: Vec::new(),
                canonical_dependencies: BTreeSet::from_iter(
                    ["/src/dep.ts".to_string()].into_iter(),
                ),
                overflow: None,
                stats: crate::ImportedEvalStats::default(),
            },
            cached: RefCell::new(std::collections::BTreeMap::new()),
            build_inputs_calls: RefCell::new(0),
            build_env_calls: RefCell::new(0),
        };

        let first = evaluate_imported_decl_with_owner_env(
            &mut resolver,
            "/src/types.ts",
            "Props",
            &mut BTreeSet::new(),
        );
        let mut second_deps = BTreeSet::new();
        let second = evaluate_imported_decl_with_owner_env(
            &mut resolver,
            "/src/types.ts",
            "Props",
            &mut second_deps,
        );

        assert_eq!(first, Some(TypeExpr::Primitive(PrimitiveName::String)));
        assert_eq!(second, first);
        assert!(second_deps.contains("/src/dep.ts"));
        assert_eq!(
            *resolver.build_inputs_calls.borrow(),
            1,
            "cached imported decl bodies should skip rebuilding imported inputs on repeat lookups",
        );
        assert_eq!(
            *resolver.build_env_calls.borrow(),
            1,
            "cached imported decl bodies should skip rebuilding owner eval env on repeat lookups",
        );
        assert_eq!(
            resolver.entered.borrow().as_slice(),
            ["/src/types.ts"],
            "cache hits should bypass alias-env entry entirely",
        );
        assert_eq!(
            resolver.left.borrow().as_slice(),
            ["/src/types.ts"],
            "cache hits should bypass alias-env entry entirely",
        );
    }

    #[test]
    fn materialize_imported_decl_with_owner_env_prefers_expanded_object_shape() {
        let mut env = EvalEnv::new();
        env.add_value(verter_semantic::analysis::type_eval::ValueDeclInfo {
            name: "theme".to_string(),
            declaration_id: 3,
            kind: verter_semantic::analysis::type_eval::ValueDeclKind::Const,
            type_annotation: None,
            function_signature: None,
            object_shape: Some(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "slots".to_string(),
                    ty: TypeExpr::Object(Arc::new(ObjectExpr {
                        properties: vec![ObjectMember::Property(ObjectProperty {
                            name: "base".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            readonly: false,
                        })],
                    })),
                    optional: false,
                    readonly: false,
                })],
            }),
        });
        env.add_type(TypeDeclInfo {
            name: "ComponentConfig".to_string(),
            declaration_id: 2,
            kind: TypeDeclKind::Alias,
            type_parameters: vec![TypeParam {
                name: "T".to_string(),
                constraint: None,
                default: None,
            }],
            body: TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty {
                    name: "ui".to_string(),
                    ty: TypeExpr::IndexedAccess {
                        object: Arc::new(TypeExpr::TypeParameter(TypeParam {
                            name: "T".to_string(),
                            constraint: None,
                            default: None,
                        })),
                        index: Arc::new(TypeExpr::Literal(
                            verter_semantic::analysis::type_expr::LiteralValue::String(
                                "slots".to_string(),
                            ),
                        )),
                    },
                    optional: false,
                    readonly: false,
                })],
            })),
        });
        env.add_type(TypeDeclInfo {
            name: "Button".to_string(),
            declaration_id: 1,
            kind: TypeDeclKind::Alias,
            type_parameters: Vec::new(),
            body: TypeExpr::Ref {
                name: Arc::from("ComponentConfig"),
                type_arguments: Arc::from([TypeExpr::TypeOf(
                    verter_semantic::analysis::type_expr::ValueRef {
                        path: vec!["theme".to_string()],
                    },
                )]),
            },
        });

        let mut contexts = std::collections::BTreeMap::new();
        contexts.insert(
            ("/src/button-types.ts".to_string(), "Button".to_string()),
            PreparedImportedDeclContext {
                imports: Vec::new(),
                macros: Vec::new(),
                bindings: Vec::new(),
                macro_type_deps: Vec::new(),
                eval_source: String::new(),
                env,
                decl: decl(
                    "Button",
                    TypeExpr::Ref {
                        name: Arc::from("ComponentConfig"),
                        type_arguments: Arc::from([TypeExpr::TypeOf(
                            verter_semantic::analysis::type_expr::ValueRef {
                                path: vec!["theme".to_string()],
                            },
                        )]),
                    },
                ),
            },
        );
        let mut resolver = TestResolver {
            exhausted: false,
            allow_alias_enter: true,
            contexts,
            entered: RefCell::new(Vec::new()),
            left: RefCell::new(Vec::new()),
            built_inputs: ImportedEvalInputs {
                sources: Vec::new(),
                type_aliases: Vec::new(),
                canonical_dependencies: BTreeSet::new(),
                overflow: None,
                stats: crate::ImportedEvalStats::default(),
            },
            cached: RefCell::new(std::collections::BTreeMap::new()),
            build_inputs_calls: RefCell::new(0),
            build_env_calls: RefCell::new(0),
        };

        let actual = materialize_imported_decl_with_owner_env(
            &mut resolver,
            "/src/button-types.ts",
            "Button",
            &mut BTreeSet::new(),
        )
        .expect("materialized imported decl should exist");

        let TypeExpr::Object(shape) = actual else {
            panic!(
                "materialized imported decl should be an object, got {:?}",
                actual
            );
        };
        assert!(
            shape.properties.iter().any(
                |member| matches!(member, ObjectMember::Property(property) if property.name == "ui")
            ),
            "materialized imported decl should keep its ui member"
        );
    }
}
