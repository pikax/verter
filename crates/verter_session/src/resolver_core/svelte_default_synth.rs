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
use verter_type_expr::{
    FunctionExpr, FunctionParam, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName, TypeExpr,
};

use super::shallow_file_state::ShallowValueSymbol;
use super::vue_default_synth::{VUE_INSTANCE_PROPS_MEMBER, VUE_INSTANCE_SLOTS_MEMBER};

/// The synthesized instance member carrying the component's event map (the
/// legacy `createEventDispatcher<E>` payload map). Mirrors Vue's `$emit` member
/// role; the api-projector renders the shim `$events` index from it (UNIONed with
/// the derived callback-prop events). A component with no dispatcher carries no
/// `$events` member (the callback-prop events derive from `$props` at the shim).
pub const SVELTE_INSTANCE_EVENTS_MEMBER: &str = "$events";

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

    // The legacy dispatcher event-map member, when the component declares a
    // `createEventDispatcher<E>` (parse-domain; provenance validation is a
    // query-time concern, never synth's). The shim renders the exact handler
    // types from this map; the derived callback-prop events come from `$props`
    // at the shim, so a dispatcher-less runes component carries no `$events`
    // member here. Shallow-by-default — the event-map ref is preserved verbatim.
    if let Some(events_type) = candidates.dispatcher_events.clone() {
        members.push(ObjectMember::Property(ObjectProperty::synthetic_public(
            SVELTE_INSTANCE_EVENTS_MEMBER.to_string(),
            events_type,
            false,
            false,
        )));
    }

    // The snippet-typed slot members, when the component declares snippet props
    // (parse-domain candidate member names). Each becomes a callable slot member
    // `(bindings) => any` (the binding precision lives in the snippet `Snippet<…>`
    // type the consumer re-resolves on demand). A component with no snippet props
    // carries no `$slots` member; the consumer's `$slots[K]` then fails the
    // `keyof {}` index (the correct slot-less behaviour).
    let slot_members = snippet_slot_members(candidates);
    if !slot_members.is_empty() {
        members.push(ObjectMember::Property(ObjectProperty::synthetic_public(
            VUE_INSTANCE_SLOTS_MEMBER.to_string(),
            TypeExpr::Object(Arc::new(ObjectExpr {
                properties: slot_members,
            })),
            false,
            false,
        )));
    }

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

/// The snippet-typed slot members for the synthesized `$slots` instance member.
///
/// Each parse-domain snippet candidate (`member_name`) becomes one slot key
/// whose value is a callable carrier `(...args: any[]) => any`. The PRECISE
/// binding type lives in the snippet prop's own `Snippet<[...]>` type on `$props`
/// (the consumer re-resolves it on demand — shallow-by-default); the synth
/// records the exact slot KEYS so the consumer's `$slots[K]` index is name-exact
/// (an unknown slot name FAILS the `keyof` index). De-duplicated by member name.
fn snippet_slot_members(candidates: &SvelteScriptCandidates) -> Vec<ObjectMember> {
    let mut seen = std::collections::HashSet::new();
    candidates
        .snippet_candidates
        .iter()
        .filter(|c| seen.insert(c.member_name.clone()))
        .map(|c| {
            // A callable slot carrier — the slot member is function-like so a
            // consumer can CALL `$slots.row(bindings)`. The precise binding/return
            // types are recovered by the consumer from the snippet prop's own
            // `Snippet<…>` type (shallow-by-default); this carrier records the KEY.
            ObjectMember::Property(ObjectProperty::synthetic_public(
                c.member_name.clone(),
                TypeExpr::Function(Arc::new(FunctionExpr::synthetic(
                    vec![FunctionParam::synthetic(
                        Some("bindings".to_string()),
                        TypeExpr::Primitive(PrimitiveName::Any),
                        false,
                        false,
                    )],
                    Some(Arc::new(TypeExpr::Primitive(PrimitiveName::Any))),
                    Vec::new(),
                ))),
                false,
                false,
            ))
        })
        .collect()
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
    fn synth_reads_only_parse_domain_candidates_never_resolved_validation() {
        // Synth is a pure function of the PARSE-DOMAIN candidate set: it never
        // reads the resolved-validation facts (the snippet `svelte`-import
        // validation, the dispatcher provenance). The SAME candidates always
        // synthesise the SAME symbol regardless of any later validation outcome.
        use verter_semantic::analysis::framework_facts::svelte::SvelteSnippetImportCandidate;
        let candidates = SvelteScriptCandidates {
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
        // Two synth runs over the identical parse-domain candidates are identical.
        let a = synthesise_svelte_default_value_symbol(&candidates);
        let b = synthesise_svelte_default_value_symbol(&candidates);
        assert_eq!(instance_members(&a), instance_members(&b));
    }

    #[test]
    fn snippet_candidates_synthesise_a_slots_instance_member() {
        // F9: the parse-domain snippet candidates contribute a `$slots` instance
        // member (an exact key map of snippet callables) — the consumer's
        // `$slots[K]` index is name-exact. A component with NO snippet props
        // carries NO `$slots` member.
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
        let sym = synthesise_svelte_default_value_symbol(&with_snippet);
        assert!(
            instance_members(&sym).contains(&"$slots".to_string()),
            "a snippet prop synthesises a $slots member, got {:?}",
            instance_members(&sym)
        );

        // No snippet candidates ⇒ no `$slots` member.
        let without_snippet = SvelteScriptCandidates {
            snippet_candidates: Vec::new(),
            ..with_snippet.clone()
        };
        let sym2 = synthesise_svelte_default_value_symbol(&without_snippet);
        assert!(
            !instance_members(&sym2).contains(&"$slots".to_string()),
            "no snippet props ⇒ no $slots member, got {:?}",
            instance_members(&sym2)
        );
    }

    #[test]
    fn dispatcher_events_synthesise_an_events_instance_member() {
        // F13: a legacy `createEventDispatcher<E>` (parse-domain `dispatcher_events`)
        // contributes a `$events` instance member carrying the event-map type. A
        // component with NO dispatcher carries NO `$events` member (the derived
        // callback-prop events come from `$props` at the shim).
        let with_dispatcher = SvelteScriptCandidates {
            dispatcher_events: Some(TypeExpr::Ref {
                name: Arc::from("Events"),
                type_arguments: Arc::from(Vec::new().into_boxed_slice()),
            }),
            ..Default::default()
        };
        let sym = synthesise_svelte_default_value_symbol(&with_dispatcher);
        assert!(
            instance_members(&sym).contains(&"$events".to_string()),
            "a dispatcher synthesises a $events member, got {:?}",
            instance_members(&sym)
        );

        let without = SvelteScriptCandidates::default();
        let sym2 = synthesise_svelte_default_value_symbol(&without);
        assert!(
            !instance_members(&sym2).contains(&"$events".to_string()),
            "no dispatcher ⇒ no $events member, got {:?}",
            instance_members(&sym2)
        );
    }
}
