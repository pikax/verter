//! Synthesise the implicit `default` export for a Svelte component scope.
//!
//! A `.svelte` component has no literal `export default` — the default export is
//! the component value the compiler produces. This module synthesises a
//! class-shaped `default` value symbol whose construct signature returns the
//! component's instance shape (`{ $props: Props }` plus the exported
//! instance-script members), exactly the way
//! [`super::vue_default_synth`] synthesises the Vue SFC's `default`.
//!
//! The synthesis consumes the PARSE-DOMAIN
//! [`SvelteScriptCandidates`](verter_semantic::analysis::framework_facts::svelte::SvelteScriptCandidates)
//! captured by the Svelte script-fact provider's syntax-capture half — never the
//! resolved-validation facts. Synth output is therefore a pure
//! function of parse-domain inputs: it is structurally identical whether the
//! workspace carries the real `svelte` package or a userland look-alike (the
//! `component_default_synth_parse_domain_only` behavioural half). The
//! resolved-validation-validated snippet classification surfaces ONLY through query-time
//! consumers, never through shallow state.

use std::sync::Arc;

use verter_semantic::analysis::framework_facts::svelte::SvelteScriptCandidates;
use verter_semantic::analysis::type_eval::{FunctionSignature, ValueDeclKind};
use verter_type_expr::{ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr};

use super::shallow_file_state::ShallowValueSymbol;
use super::vue_default_synth::VUE_INSTANCE_PROPS_MEMBER;

/// Build the synthetic `default` value symbol for a `.svelte` scope from its
/// parse-domain script candidates.
///
/// EVERY `.svelte` file is a component, so this ALWAYS produces a default — a
/// pure-markup component with no `$props()` and no exports still gets an instance
/// whose `$props` is the empty object `{}`. The returned symbol mimics a userland
/// `class default { ... }`: a single construct signature whose return type is the
/// instance shape `{ $props: Props, ...exported members }`. `Props` is the
/// `$props()` candidate type (runes mode) OR the synthesized object of legacy
/// `export let` props OR `{}` when the component declares no props.
#[must_use]
pub fn synthesise_svelte_default_value_symbol(
    candidates: &SvelteScriptCandidates,
) -> ShallowValueSymbol {
    let props_type = props_instance_type(candidates).unwrap_or_else(empty_object);
    let exported = instance_export_members(candidates);

    let mut members: Vec<ObjectMember> =
        vec![ObjectMember::Property(ObjectProperty::synthetic_public(
            VUE_INSTANCE_PROPS_MEMBER.to_string(),
            props_type,
            false,
            false,
        ))];
    members.extend(exported);

    let instance_shape = TypeExpr::Object(Arc::new(ObjectExpr {
        properties: members,
    }));

    ShallowValueSymbol {
        kind: ValueDeclKind::Class,
        type_annotation: None,
        signatures: vec![FunctionSignature {
            parameters: Vec::new(),
            return_type: Some(instance_shape),
            type_parameters: Vec::new(),
            has_implementation_body: true,
        }],
        object_shape: None,
        enum_members: None,
        // The SOLE construction site of the synthesized `.svelte` default — the
        // flag is the direct consumer proof the synthesized-default consumers
        // gate on (shared with the Vue synth's `default`).
        is_synthesised_component_default: true,
    }
}

/// The empty object type `{}` — the `$props` member type for a component that
/// declares no props.
fn empty_object() -> TypeExpr {
    TypeExpr::Object(Arc::new(ObjectExpr {
        properties: Vec::new(),
    }))
}

/// The `$props` member type for the synthesized instance: the runes `$props()`
/// type when present, else the synthesized object of legacy `export let` props,
/// else `None` (the caller substitutes `{}`).
fn props_instance_type(candidates: &SvelteScriptCandidates) -> Option<TypeExpr> {
    if let Some(props) = &candidates.props {
        // Runes mode: the `$props()` type REF is preserved verbatim
        // (shallow-by-default — never eagerly inlined). An un-annotated
        // `$props()` carries no type; the props member is still synthesized as
        // an empty object surface so `$props` exists.
        return Some(props.props_type.clone().unwrap_or_else(empty_object));
    }
    if !candidates.legacy_props.is_empty() {
        // Legacy `export let` props: synthesize an object surface whose members
        // are the exported props (optional when they carry a default).
        let properties = candidates
            .legacy_props
            .iter()
            .map(|p| {
                ObjectMember::Property(ObjectProperty::synthetic_public(
                    p.name.clone(),
                    TypeExpr::Primitive(PrimitiveName::Any),
                    // `optional`: a prop with a default value is optional.
                    p.has_default,
                    false,
                ))
            })
            .collect();
        return Some(TypeExpr::Object(Arc::new(ObjectExpr { properties })));
    }
    None
}

/// The exported instance-script members as instance properties (each exported
/// binding is a member of the component instance). The member values are left
/// as a `Ref` to the exported binding name so consumers re-resolve on demand
/// (shallow-by-default).
fn instance_export_members(candidates: &SvelteScriptCandidates) -> Vec<ObjectMember> {
    candidates
        .instance_exports
        .iter()
        .map(|name| {
            ObjectMember::Property(ObjectProperty::synthetic_public(
                name.clone(),
                TypeExpr::Ref {
                    name: Arc::from(name.as_str()),
                    type_arguments: Arc::from(Vec::new().into_boxed_slice()),
                },
                false,
                false,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_semantic::analysis::framework_facts::svelte::{
        SvelteLegacyProp, SveltePropsCandidate,
    };

    fn instance_members(symbol: &ShallowValueSymbol) -> Vec<String> {
        let sig = symbol.signatures.first().expect("construct signature");
        let return_type = sig.return_type.as_ref().expect("return type");
        match return_type {
            TypeExpr::Object(obj) => obj
                .properties
                .iter()
                .filter_map(|m| match m {
                    ObjectMember::Property(p) => Some(p.name.clone()),
                    _ => None,
                })
                .collect(),
            other => panic!("expected object instance shape, got {other:?}"),
        }
    }

    #[test]
    fn no_candidates_still_synthesises_an_empty_props_default() {
        // EVERY `.svelte` is a component: a pure-markup component with no
        // `$props()` and no exports still synthesizes a class-shaped default
        // whose `$props` is the empty object `{}`.
        let candidates = SvelteScriptCandidates::default();
        let sym = synthesise_svelte_default_value_symbol(&candidates);
        assert!(sym.is_synthesised_component_default);
        assert_eq!(sym.kind, ValueDeclKind::Class);
        assert_eq!(instance_members(&sym), vec!["$props".to_string()]);
        let sig = sym.signatures.first().unwrap();
        let TypeExpr::Object(obj) = sig.return_type.as_ref().unwrap() else {
            panic!("object instance shape");
        };
        let ObjectMember::Property(props) = &obj.properties[0] else {
            panic!("props member");
        };
        assert!(
            matches!(&props.ty, TypeExpr::Object(o) if o.properties.is_empty()),
            "pure-markup component's $props is the empty object, got {:?}",
            props.ty
        );
    }

    #[test]
    fn runes_props_synthesise_dollar_props_member() {
        let candidates = SvelteScriptCandidates {
            props: Some(SveltePropsCandidate {
                props_type: Some(TypeExpr::Ref {
                    name: Arc::from("Props"),
                    type_arguments: Arc::from(Vec::new().into_boxed_slice()),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let sym = synthesise_svelte_default_value_symbol(&candidates);
        assert_eq!(sym.kind, ValueDeclKind::Class);
        assert!(sym.is_synthesised_component_default);
        assert_eq!(instance_members(&sym), vec!["$props".to_string()]);
    }

    #[test]
    fn props_type_ref_is_preserved_not_inlined() {
        // Shallow-by-default: the props type REF stays a bare reference.
        let candidates = SvelteScriptCandidates {
            props: Some(SveltePropsCandidate {
                props_type: Some(TypeExpr::Ref {
                    name: Arc::from("MyProps"),
                    type_arguments: Arc::from(Vec::new().into_boxed_slice()),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };
        let sym = synthesise_svelte_default_value_symbol(&candidates);
        let sig = sym.signatures.first().unwrap();
        let TypeExpr::Object(obj) = sig.return_type.as_ref().unwrap() else {
            panic!("object shape");
        };
        let ObjectMember::Property(props) = &obj.properties[0] else {
            panic!("props property");
        };
        assert!(
            matches!(&props.ty, TypeExpr::Ref { name, .. } if name.as_ref() == "MyProps"),
            "the props type stays a bare Ref (shallow-by-default), got {:?}",
            props.ty
        );
    }

    #[test]
    fn exported_members_appear_on_the_instance() {
        let candidates = SvelteScriptCandidates {
            props: Some(SveltePropsCandidate {
                props_type: None,
                ..Default::default()
            }),
            instance_exports: vec!["focus".to_string(), "reset".to_string()],
            ..Default::default()
        };
        let sym = synthesise_svelte_default_value_symbol(&candidates);
        let mut members = instance_members(&sym);
        members.sort();
        assert_eq!(
            members,
            vec![
                "$props".to_string(),
                "focus".to_string(),
                "reset".to_string()
            ]
        );
    }

    #[test]
    fn legacy_export_let_synthesises_props_object() {
        let candidates = SvelteScriptCandidates {
            legacy_props: vec![
                SvelteLegacyProp {
                    name: "name".to_string(),
                    has_default: false,
                },
                SvelteLegacyProp {
                    name: "count".to_string(),
                    has_default: true,
                },
            ],
            ..Default::default()
        };
        let sym = synthesise_svelte_default_value_symbol(&candidates);
        assert_eq!(instance_members(&sym), vec!["$props".to_string()]);
        // The synthesized $props object carries the legacy props.
        let sig = sym.signatures.first().unwrap();
        let TypeExpr::Object(obj) = sig.return_type.as_ref().unwrap() else {
            panic!("object");
        };
        let ObjectMember::Property(props) = &obj.properties[0] else {
            panic!("props");
        };
        let TypeExpr::Object(props_obj) = &props.ty else {
            panic!("props object, got {:?}", props.ty);
        };
        let prop_names: Vec<&str> = props_obj
            .properties
            .iter()
            .filter_map(|m| match m {
                ObjectMember::Property(p) => Some(p.name.as_str()),
                _ => None,
            })
            .collect();
        assert!(prop_names.contains(&"name"));
        assert!(prop_names.contains(&"count"));
    }

    #[test]
    fn synth_is_parse_domain_pure_independent_of_snippet_validation() {
        // D-au: synth output is identical regardless of snippet (resolved-validation)
        // validation — the candidate set is parse-domain, and synth never reads
        // resolved facts. The same candidates always synthesise the same symbol.
        use verter_semantic::analysis::framework_facts::svelte::SvelteSnippetImportCandidate;
        let with_snippet = SvelteScriptCandidates {
            props: Some(SveltePropsCandidate {
                props_type: Some(TypeExpr::Ref {
                    name: Arc::from("Props"),
                    type_arguments: Arc::from(Vec::new().into_boxed_slice()),
                }),
                ..Default::default()
            }),
            snippet_candidates: vec![SvelteSnippetImportCandidate {
                local_binding: "Snippet".to_string(),
                import_source: "svelte".to_string(),
                member_name: "row".to_string(),
            }],
            ..Default::default()
        };
        let without_snippet = SvelteScriptCandidates {
            snippet_candidates: Vec::new(),
            ..with_snippet.clone()
        };
        let a = synthesise_svelte_default_value_symbol(&with_snippet);
        let b = synthesise_svelte_default_value_symbol(&without_snippet);
        // Snippet candidates do NOT enter synth output — both produce the same
        // instance member set.
        assert_eq!(instance_members(&a), instance_members(&b));
    }
}
