use std::collections::BTreeSet;

use rustc_hash::FxHashSet;
use verter_analysis::type_eval::{EvalEnv, TypeDeclInfo};
use verter_analysis::type_expr::TypeExpr;
use verter_analysis::{AnalyzedBinding, AnalyzedImport, AnalyzedMacro, MacroTypeDep};

use crate::{ImportedEvalInputs, ImportedEvalOwnerSnapshot};

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

    fn required_import_names_for_decl(
        &self,
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
}

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
    if !resolver.enter_alias_env(&resolved_source_canonical_id) {
        return None;
    }

    let result = (|| {
        let context =
            resolver.load_imported_decl_context(&resolved_source_canonical_id, exported_name)?;
        let import_alloc = oxc_allocator::Allocator::new();
        let mut decl_required_import_names =
            verter_core::utils::oxc::vue::resolve_type::collect_required_import_names_for_external_type(
                exported_name,
                context.eval_source.as_str(),
                &import_alloc,
            );
        if decl_required_import_names.is_empty() && !context.imports.is_empty() {
            decl_required_import_names =
                resolver.required_import_names_for_decl(&context.decl, &context.env);
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
        Some(verter_analysis::type_eval::evaluate(
            &decl.body,
            &mut dep_env,
        ))
    })();

    resolver.leave_alias_env(&resolved_source_canonical_id);
    result
}

#[cfg(test)]
mod tests {
    use super::{
        evaluate_imported_decl_with_owner_env, ImportedDeclEvalResolver,
        PreparedImportedDeclContext,
    };
    use crate::{ImportedEvalInputs, ImportedEvalOverflow};
    use rustc_hash::FxHashSet;
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use verter_analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
    use verter_analysis::type_expr::{PrimitiveName, TypeExpr, TypeParam};

    struct TestResolver {
        exhausted: bool,
        allow_alias_enter: bool,
        contexts: std::collections::BTreeMap<(String, String), PreparedImportedDeclContext>,
        entered: RefCell<Vec<String>>,
        left: RefCell<Vec<String>>,
        built_inputs: ImportedEvalInputs,
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
            self.built_inputs.clone()
        }

        fn build_owner_eval_env_for_decl(
            &self,
            _canonical_id: &str,
            context: &PreparedImportedDeclContext,
            _imported_inputs: &ImportedEvalInputs,
        ) -> Option<EvalEnv> {
            Some(context.env.clone())
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
            },
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
            },
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
            },
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
                verter_analysis::type_expr::ObjectExpr {
                    properties: vec![verter_analysis::type_expr::ObjectMember::Property(
                        verter_analysis::type_expr::ObjectProperty {
                            name: "path".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            readonly: false,
                        },
                    )],
                },
            )),
        });
        env.add_type(TypeDeclInfo {
            name: "vt".to_string(),
            declaration_id: 3,
            kind: TypeDeclKind::Interface,
            type_parameters: Vec::new(),
            body: TypeExpr::Object(std::sync::Arc::new(
                verter_analysis::type_expr::ObjectExpr {
                    properties: vec![verter_analysis::type_expr::ObjectMember::Property(
                        verter_analysis::type_expr::ObjectProperty {
                            name: "name".to_string(),
                            ty: TypeExpr::Primitive(PrimitiveName::String),
                            optional: false,
                            readonly: false,
                        },
                    )],
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
            },
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
                    verter_analysis::type_expr::ObjectMember::Property(prop)
                        if prop.name == "path"
                )
            }),
            _ => false,
        }));
        assert!(types.iter().any(|ty| match ty {
            TypeExpr::Object(obj) => obj.properties.iter().any(|member| {
                matches!(
                    member,
                    verter_analysis::type_expr::ObjectMember::Property(prop)
                        if prop.name == "name"
                )
            }),
            _ => false,
        }));
    }
}
