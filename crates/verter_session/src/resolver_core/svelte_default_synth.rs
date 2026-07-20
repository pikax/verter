//! Synthesise the implicit `default` export for a Svelte component scope.
//!
//! A `.svelte` component has no literal `export default` — the default export is
//! the component value the compiler produces. This module synthesises a
//! internal `default` carrier whose fabricated structural inventory is
//! `{ $props: Props }` plus the exported instance-script members, exactly the
//! way [`super::vue_default_synth`] synthesises the Vue SFC's `default`.
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
use verter_type_expr::facts::{
    FactOrLocator, LeafTypeFact, ResolvedLocalShape, SemanticTypeSource, SynthesizedLeafMember,
    SynthesizedMemberFact,
};
use verter_type_expr::locators::{
    AuthoredAnchor, AuthoredBodyLocator, AuthoredTypePayloadRef, LocatorSymbolSpace, TypeBodySlot,
};
use verter_type_expr::span_origins::{MemberSpansOrigin, SourceSynthetic};
use verter_type_expr::PrimitiveName;

use crate::decl_body_memo::{lowered_value_decl_for_synthesised_default, LoweredValueDecl};

use super::vue_default_synth::{VUE_INSTANCE_PROPS_MEMBER, VUE_INSTANCE_SLOTS_MEMBER};

/// The synthesized instance member carrying the component's event map (the
/// legacy `createEventDispatcher<E>` payload map). Mirrors Vue's `$emit` member
/// role. The API projector converts this legacy map to Svelte 5 callback props;
/// it never exposes a public class-like `$events` instance member. A component
/// with no dispatcher carries no `$events` inventory row.
pub const SVELTE_INSTANCE_EVENTS_MEMBER: &str = "$events";

/// Build the synthetic `default` value symbol for a `.svelte` scope from its
/// parse-domain script candidates.
///
/// EVERY `.svelte` file is a component, so this ALWAYS produces a default — a
/// pure-markup component with no `$props()` and no exports still gets an internal
/// inventory whose `$props` is the empty object `{}`. The returned symbol's
/// fabricated structural shape
/// (`{ $props: Props, ...exported members }`) rides the annotation FACT as a
/// synthesized CLOSED source ([`SemanticTypeSource::Synthesized`]). `Props` is
/// the `$props()` candidate's authored PAYLOAD LOCATOR (runes mode — lowered on
/// demand through the one dispatch, never eagerly) OR the fabricated
/// depth-closed object of legacy `export let` props OR the empty `{}` when the
/// component declares no props.
#[must_use]
pub fn synthesise_svelte_default_value_symbol(
    candidates: &SvelteScriptCandidates,
) -> LoweredValueDecl {
    let mut members: Vec<SynthesizedMemberFact> = vec![synthetic_member(
        VUE_INSTANCE_PROPS_MEMBER,
        false,
        props_instance_fact(candidates),
    )];

    // The legacy dispatcher event-map member, when the component declares a
    // `createEventDispatcher<E>` (parse-domain; provenance validation is a
    // query-time concern, never synth's). The shim renders the exact handler
    // types from this map; the derived callback-prop events come from `$props`
    // at the shim, so a dispatcher-less runes component carries no `$events`
    // member here. Shallow-by-default — the event-map payload stays its
    // authored locator, lowered on demand.
    if let Some(events_payload) = candidates.dispatcher_events.as_ref() {
        members.push(synthetic_member(
            SVELTE_INSTANCE_EVENTS_MEMBER,
            false,
            payload_ref_fact(events_payload),
        ));
    }

    // The snippet-typed slot members, when the component declares snippet props
    // (parse-domain candidate member names). The fabricated `$slots` map records
    // the exact slot KEYS so the consumer's `$slots[K]` index is name-exact (an
    // unknown slot name FAILS the `keyof` index); each value degrades to the
    // honest `any` leaf — the PRECISE snippet callable lives in the snippet
    // prop's own `Snippet<…>` type on `$props`, re-resolved by the consumer on
    // demand (shallow-by-default), and `any` stays callable and indexable. A
    // component with no snippet props carries no `$slots` member; the
    // consumer's `$slots[K]` then fails the `keyof {}` index (the correct
    // slot-less behaviour).
    let slot_members = snippet_slot_members(candidates);
    if !slot_members.is_empty() {
        members.push(synthetic_member(
            VUE_INSTANCE_SLOTS_MEMBER,
            false,
            FactOrLocator::LeafObject(Arc::from(slot_members.into_boxed_slice())),
        ));
    }

    // The exported instance-script members (each exported binding is a member
    // of the component instance). The member values retain the exact lexical
    // owner in an authored value-body locator so same-name module/instance
    // bindings cannot alias during on-demand lowering.
    for export in &candidates.instance_exports {
        members.push(synthetic_member(
            &export.exported_name,
            false,
            FactOrLocator::Locator(TypeBodySlot {
                anchor: AuthoredAnchor {
                    canonical_id: Arc::from(""),
                    owner: export.binding_key.owner,
                    symbol: Arc::clone(&export.binding_key.name),
                    space: LocatorSymbolSpace::Value,
                },
                path: Arc::from([]),
            }),
        ));
    }

    lowered_value_decl_for_synthesised_default(SemanticTypeSource::Synthesized(
        ResolvedLocalShape::Object(Arc::from(members.into_boxed_slice())),
    ))
}

/// One fabricated instance member (no authored member position exists for the
/// synthetic member name).
fn synthetic_member(name: &str, optional: bool, ty: FactOrLocator) -> SynthesizedMemberFact {
    SynthesizedMemberFact {
        name: name.to_string(),
        optional,
        ty,
        span_origin: MemberSpansOrigin::Synthetic(SourceSynthetic),
    }
}

/// Map an authored-type PAYLOAD REF onto the synthesized member vocabulary:
/// the macro-payload locator carrier (the svelte capture's only produced
/// arm), or the decl-body locator escape. The payload's structural hash is a
/// candidate-slot discriminator, not member identity — content identity of
/// the synthesized default rides the owner's `FileWholeHash` (the same
/// convention the locator-positioned signature facts follow).
fn payload_ref_fact(payload: &AuthoredTypePayloadRef) -> FactOrLocator {
    match &payload.locator {
        AuthoredBodyLocator::MacroPayload(locator) => FactOrLocator::MacroPayload(locator.clone()),
        AuthoredBodyLocator::DeclBody(slot) => FactOrLocator::Locator(slot.clone()),
        // The svelte capture mints macro-payload locators exclusively
        // (`authored_type_payload_ref`); an ambient-augmentation / JSDoc-typedef
        // payload ref has no synthesized-member carrier arm and degrades to the
        // honest `unknown` leaf (TS's sound top type) — never a fabricated
        // position.
        AuthoredBodyLocator::AugmentationBody(_) | AuthoredBodyLocator::JsdocTypedefBody(_) => {
            FactOrLocator::Leaf(LeafTypeFact::Primitive(PrimitiveName::Unknown))
        }
    }
}

/// The snippet-typed slot members for the internal `$slots` inventory row.
///
/// Each parse-domain snippet candidate (`member_name`) becomes one slot key
/// whose value is the honest `any` leaf. The PRECISE binding type lives in the
/// snippet prop's own `Snippet<[...]>` type on `$props` (the consumer
/// re-resolves it on demand — shallow-by-default); the synth records the exact
/// slot KEYS so the consumer's `$slots[K]` index is name-exact (an unknown
/// slot name FAILS the `keyof` index). De-duplicated by member name.
fn snippet_slot_members(candidates: &SvelteScriptCandidates) -> Vec<SynthesizedLeafMember> {
    let mut seen = std::collections::HashSet::new();
    candidates
        .snippet_candidates
        .iter()
        .filter(|c| seen.insert(c.member_name.clone()))
        .map(|c| SynthesizedLeafMember {
            name: c.member_name.clone(),
            optional: false,
            ty: LeafTypeFact::Primitive(PrimitiveName::Any),
        })
        .collect()
}

/// The `$props` member fact for the synthesized instance: the runes `$props()`
/// authored payload locator when present, else the fabricated depth-closed
/// object of legacy `export let` props, else the empty object `{}`.
fn props_instance_fact(candidates: &SvelteScriptCandidates) -> FactOrLocator {
    if let Some(props) = &candidates.props {
        // Runes mode with a depth-closed LEAF-able inline object literal: the
        // capture recorded the leaf display members — synthesize the
        // depth-closed leaf-object surface (member refs preserved
        // un-inlined; the dispatch-free api projector renders it shallowly).
        if let Some(members) = &props.props_leaf_members {
            return FactOrLocator::LeafObject(Arc::from(members.clone().into_boxed_slice()));
        }
        // Runes mode: the `$props()` authored payload stays a content-free
        // LOCATOR carrier (shallow-by-default — never eagerly inlined). An
        // un-annotated `$props()` carries no type; the props member is still
        // synthesized as an empty object surface so `$props` exists.
        return match props.props_type.as_ref() {
            Some(payload) => payload_ref_fact(payload),
            None => FactOrLocator::LeafObject(Arc::from(Vec::new().into_boxed_slice())),
        };
    }
    if !candidates.legacy_props.is_empty() {
        // Legacy `export let` props: a fabricated depth-closed object surface
        // whose members are the exported props (optional when they carry a
        // default), each valued the honest `any` leaf.
        let props: Vec<SynthesizedLeafMember> = candidates
            .legacy_props
            .iter()
            .map(|p| SynthesizedLeafMember {
                name: p.name.clone(),
                // `optional`: a prop with a default value is optional.
                optional: p.has_default,
                ty: LeafTypeFact::Primitive(PrimitiveName::Any),
            })
            .collect();
        return FactOrLocator::LeafObject(Arc::from(props.into_boxed_slice()));
    }
    FactOrLocator::LeafObject(Arc::from(Vec::new().into_boxed_slice()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_semantic::analysis::framework_facts::svelte::{
        SvelteInstanceExport, SvelteLegacyProp, SveltePropsCandidate,
    };
    use verter_semantic::analysis::type_eval::ValueDeclKind;
    use verter_type_expr::locators::{MacroPayloadLocator, MacroPayloadPosition};
    use verter_type_expr::{DeclBindingKey, TopLevelOwnerId};

    fn props_payload_ref(macro_index: u32, seed: u8) -> AuthoredTypePayloadRef {
        AuthoredTypePayloadRef {
            locator: AuthoredBodyLocator::MacroPayload(MacroPayloadLocator {
                anchor: AuthoredAnchor {
                    canonical_id: Arc::from(""),
                    owner: TopLevelOwnerId::instance(0),
                    symbol: Arc::from("default"),
                    space: LocatorSymbolSpace::Value,
                },
                macro_index,
                payload: MacroPayloadPosition::TypeArgument,
            }),
            payload_hash: [seed; 16],
        }
    }

    /// The synthesized instance members `(name, ty)` off the annotation-borne
    /// synthesized source.
    fn instance_members(symbol: &LoweredValueDecl) -> Vec<(String, FactOrLocator)> {
        let source = symbol
            .type_annotation
            .annotation
            .as_ref()
            .expect("synthesised default must carry the instance annotation source");
        let SemanticTypeSource::Synthesized(ResolvedLocalShape::Object(members)) = source else {
            panic!("expected a synthesized Object instance source, got {source:?}");
        };
        members
            .iter()
            .map(|m| (m.name.clone(), m.ty.clone()))
            .collect()
    }

    fn member_names(symbol: &LoweredValueDecl) -> Vec<String> {
        instance_members(symbol)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    fn member_ty(symbol: &LoweredValueDecl, name: &str) -> FactOrLocator {
        instance_members(symbol)
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, ty)| ty)
            .unwrap_or_else(|| panic!("missing instance member {name}"))
    }

    #[test]
    fn no_candidates_still_synthesises_an_empty_props_default() {
        // EVERY `.svelte` is a component: a pure-markup component with no
        // `$props()` and no exports still synthesizes a class-shaped default
        // whose `$props` is the empty object `{}`.
        let candidates = SvelteScriptCandidates::default();
        let sym = synthesise_svelte_default_value_symbol(&candidates);
        assert_eq!(sym.kind, ValueDeclKind::Class);
        assert_eq!(member_names(&sym), vec!["$props".to_string()]);
        match member_ty(&sym, "$props") {
            FactOrLocator::LeafObject(members) => assert!(
                members.is_empty(),
                "pure-markup component's $props is the empty object, got {members:?}"
            ),
            other => panic!("expected the empty depth-closed object, got {other:?}"),
        }
    }

    #[test]
    fn runes_props_synthesise_dollar_props_member() {
        let candidates = SvelteScriptCandidates {
            props: Some(SveltePropsCandidate {
                props_type: Some(props_payload_ref(0, 7)),
                ..Default::default()
            }),
            ..Default::default()
        };
        let sym = synthesise_svelte_default_value_symbol(&candidates);
        assert_eq!(sym.kind, ValueDeclKind::Class);
        assert_eq!(member_names(&sym), vec!["$props".to_string()]);
    }

    #[test]
    fn props_payload_stays_a_locator_carrier_not_inlined() {
        // Shallow-by-default: the props payload stays the authored macro
        // payload LOCATOR (lowered on demand through the one dispatch), never
        // an eagerly materialised body.
        let candidates = SvelteScriptCandidates {
            props: Some(SveltePropsCandidate {
                props_type: Some(props_payload_ref(2, 9)),
                ..Default::default()
            }),
            ..Default::default()
        };
        let sym = synthesise_svelte_default_value_symbol(&candidates);
        match member_ty(&sym, "$props") {
            FactOrLocator::MacroPayload(locator) => {
                assert_eq!(locator.macro_index, 2);
                assert!(matches!(
                    locator.payload,
                    MacroPayloadPosition::TypeArgument
                ));
            }
            other => panic!("the props payload stays a macro-payload locator, got {other:?}"),
        }
    }

    #[test]
    fn unannotated_props_synthesise_the_empty_object() {
        // An un-annotated `$props()` carries no type: the `$props` member is
        // still synthesized, as the empty object `{}`.
        let candidates = SvelteScriptCandidates {
            props: Some(SveltePropsCandidate {
                props_type: None,
                ..Default::default()
            }),
            ..Default::default()
        };
        let sym = synthesise_svelte_default_value_symbol(&candidates);
        match member_ty(&sym, "$props") {
            FactOrLocator::LeafObject(members) => assert!(members.is_empty()),
            other => panic!("expected the empty depth-closed object, got {other:?}"),
        }
    }

    #[test]
    fn exported_members_retain_exact_binding_owner_locators() {
        let instance_owner = TopLevelOwnerId::instance(0);
        let candidates = SvelteScriptCandidates {
            props: Some(SveltePropsCandidate {
                props_type: None,
                ..Default::default()
            }),
            instance_exports: vec![
                SvelteInstanceExport {
                    exported_name: "focus".to_string(),
                    local_name: "focus".to_string(),
                    owner: instance_owner,
                    binding_key: DeclBindingKey::new(instance_owner, "focus"),
                    source_span: verter_span::Span::new(0, 5),
                },
                SvelteInstanceExport {
                    exported_name: "reset".to_string(),
                    local_name: "reset".to_string(),
                    owner: instance_owner,
                    binding_key: DeclBindingKey::new(instance_owner, "reset"),
                    source_span: verter_span::Span::new(6, 11),
                },
            ],
            ..Default::default()
        };
        let sym = synthesise_svelte_default_value_symbol(&candidates);
        let mut members = member_names(&sym);
        members.sort();
        assert_eq!(
            members,
            vec![
                "$props".to_string(),
                "focus".to_string(),
                "reset".to_string()
            ]
        );
        // Each exported member stays an exact owner-qualified locator the
        // consumer dereferences on demand (shallow-by-default).
        match member_ty(&sym, "focus") {
            FactOrLocator::Locator(slot) => {
                assert_eq!(slot.anchor.owner, instance_owner);
                assert_eq!(slot.anchor.symbol.as_ref(), "focus");
                assert_eq!(slot.anchor.space, LocatorSymbolSpace::Value);
            }
            other => panic!("expected an owner-qualified value locator, got {other:?}"),
        }
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
        assert_eq!(member_names(&sym), vec!["$props".to_string()]);
        // The synthesized $props object carries the legacy props; a prop with
        // a default is optional.
        let FactOrLocator::LeafObject(props) = member_ty(&sym, "$props") else {
            panic!("expected the depth-closed legacy props object");
        };
        let entries: Vec<(&str, bool)> = props
            .iter()
            .map(|m| (m.name.as_str(), m.optional))
            .collect();
        assert!(entries.contains(&("name", false)));
        assert!(entries.contains(&("count", true)));
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
                props_type: Some(props_payload_ref(0, 3)),
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
        // member (an exact key map) — the consumer's `$slots[K]` index is
        // name-exact. A component with NO snippet props carries NO `$slots`
        // member.
        use verter_semantic::analysis::framework_facts::svelte::SvelteSnippetImportCandidate;
        let with_snippet = SvelteScriptCandidates {
            props: Some(SveltePropsCandidate {
                props_type: Some(props_payload_ref(0, 5)),
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
        let FactOrLocator::LeafObject(slots) = member_ty(&sym, "$slots") else {
            panic!(
                "a snippet prop synthesises a $slots member, got {:?}",
                member_names(&sym)
            );
        };
        // The slot KEY inventory is exact.
        assert_eq!(
            slots.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
            vec!["row"]
        );

        // No snippet candidates ⇒ no `$slots` member.
        let without_snippet = SvelteScriptCandidates {
            snippet_candidates: Vec::new(),
            ..with_snippet.clone()
        };
        let sym2 = synthesise_svelte_default_value_symbol(&without_snippet);
        assert!(
            !member_names(&sym2).contains(&"$slots".to_string()),
            "no snippet props ⇒ no $slots member, got {:?}",
            member_names(&sym2)
        );
    }

    #[test]
    fn dispatcher_events_synthesise_an_events_instance_member() {
        // F13: a legacy `createEventDispatcher<E>` (parse-domain `dispatcher_events`)
        // contributes a `$events` instance member carrying the event-map payload
        // locator. A component with NO dispatcher carries NO `$events` member
        // (the derived callback-prop events come from `$props` at the shim).
        let with_dispatcher = SvelteScriptCandidates {
            dispatcher_events: Some(props_payload_ref(1, 11)),
            ..Default::default()
        };
        let sym = synthesise_svelte_default_value_symbol(&with_dispatcher);
        match member_ty(&sym, "$events") {
            FactOrLocator::MacroPayload(locator) => assert_eq!(locator.macro_index, 1),
            other => panic!("the $events member carries the dispatcher payload, got {other:?}"),
        }

        let without = SvelteScriptCandidates::default();
        let sym2 = synthesise_svelte_default_value_symbol(&without);
        assert!(
            !member_names(&sym2).contains(&"$events".to_string()),
            "no dispatcher ⇒ no $events member, got {:?}",
            member_names(&sym2)
        );
    }
}
