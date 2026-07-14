//! Schema / taxonomy pins for the macro-codegen outcome vocabulary.
//!
//! What is pinned, and how:
//!
//! - the three-state [`MacroCodegenOutcome`] taxonomy is DISTINCT: a
//!   resolved-but-EMPTY `Complete` surface is a different value from
//!   `Partial` and from `Unresolved` (resolved-empty ≠ unavailable);
//! - `Partial` / `Unresolved` can never be read as `Complete`: the surface
//!   exists only behind the `Complete` arm, so the coercion is
//!   unrepresentable, not merely discouraged;
//! - every closed enum is destructured with a WILDCARD-FREE match, so adding
//!   a variant forces a deliberate compile-time update of this schema;
//! - every struct is destructured with a `..`-FREE pattern, pinning the
//!   exact field set BY ABSENCE: a redundant `required` twin next to the
//!   single positive `optional` fact, or a duplicated rendered-text sibling
//!   next to the structured emit payload, cannot exist without breaking
//!   compilation here. (This also rejects wholesale reintroduction of the
//!   retired span-bearing compiler-local DTO shape — its fields do not fit
//!   this schema.)

use verter_macro_dto::{
    MacroCodegenEntry, MacroCodegenKind, MacroCodegenOutcome, MacroCodegenSurface,
    MacroEmitCodegen, MacroEmitPayload, MacroEmitsCodegenSurface, MacroPropCodegen,
    MacroPropsCodegenSurface, MacroRootShape, MacroSyntaxAnchor, ResolvedMacroCodegenBundle,
    RuntimeCtorKind,
};

/// The ONLY way to reach a surface: the `Complete` arm. EXHAUSTIVE by
/// design (no wildcard) — adding an outcome variant fails compilation here
/// and forces this schema to be revisited.
fn complete_surface(outcome: &MacroCodegenOutcome) -> Option<&MacroCodegenSurface> {
    match outcome {
        MacroCodegenOutcome::Complete(surface) => Some(surface),
        MacroCodegenOutcome::Partial { reason: _ } => None,
        MacroCodegenOutcome::Unresolved { reason: _ } => None,
    }
}

/// EXHAUSTIVE (wildcard-free) kind destructure — a new macro kind fails
/// compilation here.
fn kind_label(kind: MacroCodegenKind) -> &'static str {
    match kind {
        MacroCodegenKind::Props => "props",
        MacroCodegenKind::Emits => "emits",
    }
}

fn anchor(macro_index: u32, ordinal: u32) -> MacroSyntaxAnchor {
    MacroSyntaxAnchor {
        macro_index,
        ordinal,
    }
}

fn resolved_empty_props() -> MacroCodegenOutcome {
    MacroCodegenOutcome::Complete(MacroCodegenSurface::Props(MacroPropsCodegenSurface {
        root_shape: MacroRootShape::ObjectLike,
        members: vec![],
    }))
}

fn resolved_empty_emits() -> MacroCodegenOutcome {
    MacroCodegenOutcome::Complete(MacroCodegenSurface::Emits(MacroEmitsCodegenSurface {
        events: vec![],
    }))
}

/// Resolved-empty `Complete` is a DIFFERENT fact from `Unresolved` and from
/// `Partial`, for both surface kinds — and the three outcome states are
/// pairwise distinct. A taxonomy collapse (normalising an empty resolved
/// surface into "unavailable", or merging Partial into Unresolved) fails
/// here.
#[test]
fn resolved_empty_complete_is_distinct_from_partial_and_unresolved() {
    let unresolved = MacroCodegenOutcome::Unresolved {
        reason: "type argument resolved to nothing".to_string(),
    };
    let partial = MacroCodegenOutcome::Partial {
        reason: "resolution ended before the surface was authoritative".to_string(),
    };

    for empty_complete in [resolved_empty_props(), resolved_empty_emits()] {
        assert_ne!(
            empty_complete, unresolved,
            "resolved-empty Complete must never equal Unresolved"
        );
        assert_ne!(
            empty_complete, partial,
            "resolved-empty Complete must never equal Partial"
        );
        assert!(matches!(empty_complete, MacroCodegenOutcome::Complete(_)));
    }
    assert_ne!(unresolved, partial, "Partial and Unresolved are distinct");

    // The resolved-empty props surface is REACHABLE and empty — the
    // emptiness lives on an authoritative surface, not on an error state.
    let props_outcome = resolved_empty_props();
    let surface = complete_surface(&props_outcome)
        .expect("a Complete outcome must expose its authoritative surface");
    match surface {
        MacroCodegenSurface::Props(props) => {
            assert_eq!(props.root_shape, MacroRootShape::ObjectLike);
            assert!(
                props.members.is_empty(),
                "this fixture is the resolved-EMPTY case"
            );
        }
        MacroCodegenSurface::Emits(_) => panic!("constructed a props surface"),
    }
}

/// `Partial` and `Unresolved` can never be read as `Complete`: the sole
/// surface accessor route (an exhaustive match on the outcome) yields no
/// surface for either, so "consume a partial surface as authoritative" is
/// unrepresentable.
#[test]
fn partial_and_unresolved_never_read_as_complete() {
    let partial = MacroCodegenOutcome::Partial {
        reason: "budget".to_string(),
    };
    let unresolved = MacroCodegenOutcome::Unresolved {
        reason: "no such type".to_string(),
    };

    assert!(complete_surface(&partial).is_none());
    assert!(complete_surface(&unresolved).is_none());
    assert!(!matches!(partial, MacroCodegenOutcome::Complete(_)));
    assert!(!matches!(unresolved, MacroCodegenOutcome::Complete(_)));
}

/// Exhaustive, `..`-free destructures over EVERY aggregate pin the exact
/// field sets. The load-bearing absences: `MacroPropCodegen` carries the
/// single positive `optional` fact (no `required` twin would fit the
/// pattern), and `MacroEmitCodegen` carries ONE structured payload (no
/// rendered-text sibling would fit). Any added/renamed/dropped field breaks
/// compilation here.
#[test]
fn field_sets_are_pinned_by_exhaustive_destructure() {
    let bundle = ResolvedMacroCodegenBundle {
        entries: vec![
            MacroCodegenEntry {
                macro_index: 0,
                kind: MacroCodegenKind::Props,
                outcome: MacroCodegenOutcome::Complete(MacroCodegenSurface::Props(
                    MacroPropsCodegenSurface {
                        root_shape: MacroRootShape::ObjectLike,
                        members: vec![MacroPropCodegen {
                            name: "disabled".to_string(),
                            optional: true,
                            runtime_ctors: vec![RuntimeCtorKind::Boolean],
                            anchor: anchor(0, 0),
                        }],
                    },
                )),
            },
            MacroCodegenEntry {
                macro_index: 1,
                kind: MacroCodegenKind::Emits,
                outcome: MacroCodegenOutcome::Complete(MacroCodegenSurface::Emits(
                    MacroEmitsCodegenSurface {
                        events: vec![MacroEmitCodegen {
                            name: "change".to_string(),
                            payload: MacroEmitPayload::Call {
                                params_text: "id: number".to_string(),
                            },
                            anchor: anchor(1, 0),
                        }],
                    },
                )),
            },
        ],
    };

    // Bundle: exactly `entries`.
    let ResolvedMacroCodegenBundle { entries } = &bundle;
    assert_eq!(entries.len(), 2);

    // Entry: exactly `macro_index` + `kind` + `outcome`.
    let MacroCodegenEntry {
        macro_index,
        kind,
        outcome,
    } = &entries[0];
    assert_eq!(*macro_index, 0);
    assert_eq!(kind_label(*kind), "props");

    // Props surface: exactly `root_shape` + `members`; prop member: exactly
    // `name` + `optional` + `runtime_ctors` + `anchor` — the single positive
    // requiredness fact and nothing else.
    match complete_surface(outcome).expect("props entry is Complete") {
        MacroCodegenSurface::Props(props) => {
            let MacroPropsCodegenSurface {
                root_shape,
                members,
            } = props;
            assert_eq!(*root_shape, MacroRootShape::ObjectLike);
            let MacroPropCodegen {
                name,
                optional,
                runtime_ctors,
                anchor,
            } = &members[0];
            assert_eq!(name, "disabled");
            assert!(*optional, "the single positive fact is `optional`");
            assert_eq!(runtime_ctors, &vec![RuntimeCtorKind::Boolean]);
            let MacroSyntaxAnchor {
                macro_index,
                ordinal,
            } = anchor;
            assert_eq!((*macro_index, *ordinal), (0, 0));
        }
        MacroCodegenSurface::Emits(_) => panic!("entry 0 is the props surface"),
    }

    // Emits surface: exactly `events`; emit event: exactly `name` +
    // `payload` + `anchor` — one structured payload, no rendered twin.
    let MacroCodegenEntry {
        macro_index,
        kind,
        outcome,
    } = &entries[1];
    assert_eq!(*macro_index, 1);
    assert_eq!(kind_label(*kind), "emits");
    match complete_surface(outcome).expect("emits entry is Complete") {
        MacroCodegenSurface::Emits(emits) => {
            let MacroEmitsCodegenSurface { events } = emits;
            let MacroEmitCodegen {
                name,
                payload,
                anchor,
            } = &events[0];
            assert_eq!(name, "change");
            // EXHAUSTIVE payload destructure (no wildcard): the payload's
            // structured form is the only carrier of its content.
            match payload {
                MacroEmitPayload::None => panic!("fixture carries a call payload"),
                MacroEmitPayload::Call { params_text } => assert_eq!(params_text, "id: number"),
                MacroEmitPayload::Tuple { tuple_text } => {
                    panic!("fixture is not a tuple payload: {tuple_text}")
                }
            }
            let MacroSyntaxAnchor {
                macro_index,
                ordinal,
            } = anchor;
            assert_eq!((*macro_index, *ordinal), (1, 0));
        }
        MacroCodegenSurface::Props(_) => panic!("entry 1 is the emits surface"),
    }

    // An SFC with no macros is the Default bundle: no entries — absence of a
    // macro is the absence of its entry, never a synthesized error row.
    let ResolvedMacroCodegenBundle { entries } = ResolvedMacroCodegenBundle::default();
    assert!(entries.is_empty());
}

/// EXHAUSTIVE (wildcard-free) destructure of every `RuntimeCtorKind`
/// variant, checked against its rendered constructor identifier. Adding a
/// variant fails compilation here; a wrong rendering fails the assert.
#[test]
fn runtime_ctor_kinds_are_closed_and_render_their_constructors() {
    let all = [
        RuntimeCtorKind::String,
        RuntimeCtorKind::Number,
        RuntimeCtorKind::Boolean,
        RuntimeCtorKind::Object,
        RuntimeCtorKind::Array,
        RuntimeCtorKind::Function,
        RuntimeCtorKind::Symbol,
        RuntimeCtorKind::Null,
        RuntimeCtorKind::BuiltIn("Date".to_string()),
        RuntimeCtorKind::Unknown,
    ];
    for kind in &all {
        // Wildcard-free: the parser's `RuntimeType` mirror is a CLOSED set;
        // a new inference variant must be added both here and in the array
        // above.
        let expected = match kind {
            RuntimeCtorKind::String => "String",
            RuntimeCtorKind::Number => "Number",
            RuntimeCtorKind::Boolean => "Boolean",
            RuntimeCtorKind::Object => "Object",
            RuntimeCtorKind::Array => "Array",
            RuntimeCtorKind::Function => "Function",
            RuntimeCtorKind::Symbol => "Symbol",
            RuntimeCtorKind::Null => "null",
            RuntimeCtorKind::BuiltIn(name) => name.as_str(),
            RuntimeCtorKind::Unknown => "null",
        };
        assert_eq!(kind.as_constructor(), expected);
    }
    // The array covers every variant exactly once (BuiltIn represented by
    // one carrier value); pairwise-distinct as values except the two
    // deliberate `null` renderings.
    assert_eq!(all.len(), 10);

    // Root-shape closed set: wildcard-free, two shapes, distinct.
    for shape in [MacroRootShape::ObjectLike, MacroRootShape::NonObject] {
        let object_like = match shape {
            MacroRootShape::ObjectLike => true,
            MacroRootShape::NonObject => false,
        };
        assert_eq!(object_like, shape == MacroRootShape::ObjectLike);
    }
    assert_ne!(MacroRootShape::ObjectLike, MacroRootShape::NonObject);
}
