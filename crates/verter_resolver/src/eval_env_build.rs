use rustc_hash::FxHashSet;
use verter_analysis::type_eval::EvalEnv;
use verter_analysis::AnalyzedMacro;

use crate::ImportedEvalInputs;

pub fn collect_requested_binding_names(macros: &[AnalyzedMacro]) -> FxHashSet<String> {
    macros
        .iter()
        .flat_map(|mac| mac.expose_fields.iter().map(|field| field.name.clone()))
        .collect()
}

pub fn inject_imported_type_aliases(
    env: &mut EvalEnv,
    owner_local_type_names: &FxHashSet<String>,
    imported_inputs: &ImportedEvalInputs,
) {
    for alias in &imported_inputs.type_aliases {
        if owner_local_type_names.contains(&alias.local_name) {
            continue;
        }
        env.type_symbols
            .insert(alias.local_name.clone(), alias.decl.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::{collect_requested_binding_names, inject_imported_type_aliases};
    use crate::{ImportedEvalInputs, ImportedTypeAlias};
    use rustc_hash::FxHashSet;
    use std::collections::BTreeSet;
    use verter_analysis::type_eval::{EvalEnv, TypeDeclInfo, TypeDeclKind};
    use verter_analysis::type_expr::{PrimitiveName, TypeExpr};
    use verter_analysis::types::AnalyzedExposeField;
    use verter_analysis::{AnalyzedMacro, AnalyzedMacroKind};
    use verter_span::Span;

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
    fn inject_imported_type_aliases_skips_owner_shadowed_names() {
        let mut env = EvalEnv::new();
        env.add_type(TypeDeclInfo {
            name: "Local".to_string(),
            declaration_id: 1,
            kind: TypeDeclKind::Alias,
            type_parameters: Vec::new(),
            body: TypeExpr::Primitive(PrimitiveName::String),
        });

        let imported_inputs = ImportedEvalInputs {
            sources: Vec::new(),
            type_aliases: vec![
                ImportedTypeAlias {
                    local_name: "Local".to_string(),
                    source_canonical_id: "/src/dep.ts".to_string(),
                    exported_name: "Local".to_string(),
                    decl: TypeDeclInfo {
                        name: "Local".to_string(),
                        declaration_id: 2,
                        kind: TypeDeclKind::Alias,
                        type_parameters: Vec::new(),
                        body: TypeExpr::Primitive(PrimitiveName::Number),
                    },
                    requires_source_merge: false,
                },
                ImportedTypeAlias {
                    local_name: "Imported".to_string(),
                    source_canonical_id: "/src/dep.ts".to_string(),
                    exported_name: "Imported".to_string(),
                    decl: TypeDeclInfo {
                        name: "Imported".to_string(),
                        declaration_id: 3,
                        kind: TypeDeclKind::Alias,
                        type_parameters: Vec::new(),
                        body: TypeExpr::Primitive(PrimitiveName::Boolean),
                    },
                    requires_source_merge: false,
                },
            ],
            canonical_dependencies: BTreeSet::new(),
            overflow: None,
        };

        inject_imported_type_aliases(
            &mut env,
            &FxHashSet::from_iter(["Local".to_string()].into_iter()),
            &imported_inputs,
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
}
