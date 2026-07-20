use verter_macro_dto::{
    AuthoredMemberOrdinal, MacroAnchor, MacroFailure, MacroPartialReason, MacroRuntimeBundle,
    MacroRuntimeEntry, MacroRuntimeOutcome, MacroRuntimeShape, MacroTscBundle, MacroTscEntry,
    MacroTscOutcome, MacroTscProjection, ModelRuntimeShape, OrderedRuntimeConstructors,
    PropsDefaultsAssociation, PropsRuntimeShape, RuntimeConstructor, RuntimeEmit, RuntimeProp,
    RuntimePropType, SynthesizedRowKind, TscDeclarationFailureReason, TscPropRow,
    TscPropsProjection, TscPublicPropsProjection, TscScopeRequirements,
    TscSemanticInferenceUnavailableReason, TscSpliceText, UnresolvedReason, UnsupportedReason,
};

fn authored(macro_index: u32, ordinal: Option<u32>) -> MacroAnchor {
    match ordinal {
        Some(ordinal) => MacroAnchor::Authored {
            macro_index,
            member_ordinal: AuthoredMemberOrdinal::new(ordinal),
        },
        None => MacroAnchor::MacroArgument { macro_index },
    }
}

fn constructors(
    values: impl IntoIterator<Item = RuntimeConstructor>,
) -> OrderedRuntimeConstructors {
    OrderedRuntimeConstructors::from_ordered(values)
}

fn prop(name: &str, constructors: Vec<RuntimeConstructor>) -> RuntimeProp {
    RuntimeProp {
        name: name.to_owned(),
        optional: false,
        type_shape: RuntimePropType::Resolved {
            constructors: self::constructors(constructors),
            skip_check: false,
        },
        anchor: authored(0, None),
    }
}

#[test]
fn runtime_and_tsc_bundles_are_independent_contracts() {
    let runtime = MacroRuntimeBundle {
        entries: vec![MacroRuntimeEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(PropsRuntimeShape {
                defaults: PropsDefaultsAssociation::WithDefaults {
                    payload_macro_index: 0,
                    defaults_macro_index: 1,
                },
                props: vec![prop("enabled", vec![RuntimeConstructor::Boolean])],
            })),
        }],
    };
    let tsc = MacroTscBundle {
        entries: vec![MacroTscEntry {
            syntax_index: 0,
            macro_index: 0,
            outcome: MacroTscOutcome::Complete(MacroTscProjection::Props(TscPropsProjection {
                public: TscPublicPropsProjection::AuthoredArgument {
                    anchor: MacroAnchor::MacroArgument { macro_index: 0 },
                },
                testing_rows: vec![TscPropRow {
                    name: "enabled".to_owned(),
                    optional: false,
                    type_text: TscSpliceText::new("boolean"),
                    anchor: authored(0, Some(0)),
                }],
                scope: TscScopeRequirements::default(),
            })),
        }],
    };

    assert_eq!(runtime.entries.len(), 1);
    assert_eq!(tsc.entries.len(), 1);
    let MacroRuntimeBundle { entries } = runtime;
    let MacroRuntimeEntry {
        syntax_index,
        macro_index,
        outcome,
    } = &entries[0];
    assert_eq!(*syntax_index, 0);
    assert_eq!(*macro_index, 0);
    assert!(matches!(outcome, MacroRuntimeOutcome::Complete(_)));

    let MacroTscBundle { entries } = tsc;
    let MacroTscEntry {
        syntax_index,
        macro_index,
        outcome,
    } = &entries[0];
    assert_eq!(*syntax_index, 0);
    assert_eq!(*macro_index, 0);
    assert!(matches!(outcome, MacroTscOutcome::Complete(_)));
}

#[test]
fn complete_empty_partial_unresolved_and_unsupported_are_distinct() {
    let empty = MacroRuntimeOutcome::Complete(MacroRuntimeShape::Props(PropsRuntimeShape {
        defaults: PropsDefaultsAssociation::None,
        props: vec![],
    }));
    let partial = MacroRuntimeOutcome::Partial(MacroFailure::new(
        MacroPartialReason::BudgetExceeded,
        Some("projection work budget exhausted".to_owned()),
    ));
    let unresolved = MacroRuntimeOutcome::Unresolved(MacroFailure::new(
        UnresolvedReason::MissingDeclaration,
        None,
    ));
    let unsupported = MacroRuntimeOutcome::Unsupported(MacroFailure::new(
        UnsupportedReason::SemanticConstruct,
        None,
    ));

    assert_ne!(empty, partial);
    assert_ne!(empty, unresolved);
    assert_ne!(empty, unsupported);
    assert_ne!(partial, unresolved);
    assert_ne!(partial, unsupported);
    assert_ne!(unresolved, unsupported);

    fn complete(value: &MacroRuntimeOutcome) -> Option<&MacroRuntimeShape> {
        match value {
            MacroRuntimeOutcome::Complete(surface) => Some(surface),
            MacroRuntimeOutcome::Partial(_) => None,
            MacroRuntimeOutcome::Unresolved(_) => None,
            MacroRuntimeOutcome::Unsupported(_) => None,
            MacroRuntimeOutcome::Invalid(_) => None,
        }
    }
    assert!(complete(&empty).is_some());
    assert!(complete(&partial).is_none());
    assert!(complete(&unresolved).is_none());
    assert!(complete(&unsupported).is_none());
}

#[test]
fn constructors_are_closed_ordered_and_deduplicated_without_bigint() {
    let constructors = constructors([
        RuntimeConstructor::Boolean,
        RuntimeConstructor::String,
        RuntimeConstructor::Boolean,
        RuntimeConstructor::Date,
        RuntimeConstructor::Unknown,
    ]);
    assert_eq!(
        constructors.as_slice(),
        &[
            RuntimeConstructor::Boolean,
            RuntimeConstructor::String,
            RuntimeConstructor::Date,
            RuntimeConstructor::Unknown,
        ]
    );

    let every = [
        RuntimeConstructor::String,
        RuntimeConstructor::Number,
        RuntimeConstructor::Boolean,
        RuntimeConstructor::Symbol,
        RuntimeConstructor::Null,
        RuntimeConstructor::Array,
        RuntimeConstructor::Function,
        RuntimeConstructor::Date,
        RuntimeConstructor::Map,
        RuntimeConstructor::Set,
        RuntimeConstructor::WeakMap,
        RuntimeConstructor::WeakSet,
        RuntimeConstructor::Promise,
        RuntimeConstructor::Error,
        RuntimeConstructor::Object,
        RuntimeConstructor::Unknown,
    ];
    let labels: Vec<Option<&str>> = every
        .iter()
        .map(|constructor| constructor.as_runtime_expression())
        .collect();
    assert_eq!(labels[0], Some("String"));
    assert_eq!(labels[4], Some("null"));
    assert_eq!(labels[10], Some("WeakMap"));
    assert_eq!(labels[11], Some("WeakSet"));
    assert_eq!(labels[15], None);
}

#[test]
fn anchors_distinguish_exact_authored_members_from_macro_argument_fallbacks() {
    let fallback_without_ordinal = authored(4, None);
    let authored_with_ordinal = authored(4, Some(2));
    let macro_argument = MacroAnchor::MacroArgument { macro_index: 4 };
    let synthesized = MacroAnchor::Synthesized {
        macro_index: 4,
        row: SynthesizedRowKind::ModelModifiersProp,
    };

    assert_ne!(fallback_without_ordinal, authored_with_ordinal);
    assert_eq!(fallback_without_ordinal, macro_argument);
    assert_ne!(macro_argument, synthesized);
    match authored_with_ordinal {
        MacroAnchor::Authored {
            macro_index,
            member_ordinal,
        } => {
            assert_eq!(macro_index, 4);
            assert_eq!(member_ordinal, AuthoredMemberOrdinal::new(2));
        }
        MacroAnchor::MacroArgument { .. } | MacroAnchor::Synthesized { .. } => {
            panic!("constructed an authored anchor")
        }
    }
}

#[test]
fn props_emits_and_model_are_explicit_runtime_forms() {
    let shapes = [
        MacroRuntimeShape::Props(PropsRuntimeShape {
            defaults: PropsDefaultsAssociation::None,
            props: vec![prop("value", vec![RuntimeConstructor::String])],
        }),
        MacroRuntimeShape::Emits(vec![RuntimeEmit {
            name: "change".to_owned(),
            anchor: authored(1, None),
        }]),
        MacroRuntimeShape::Model(ModelRuntimeShape {
            prop: prop("modelValue", vec![RuntimeConstructor::String]),
            update_event: RuntimeEmit {
                name: "update:modelValue".to_owned(),
                anchor: MacroAnchor::Synthesized {
                    macro_index: 2,
                    row: SynthesizedRowKind::ModelUpdateEvent,
                },
            },
            modifiers_prop: RuntimeProp {
                name: "modelModifiers".to_owned(),
                optional: true,
                type_shape: RuntimePropType::Resolved {
                    constructors: OrderedRuntimeConstructors::default(),
                    skip_check: false,
                },
                anchor: MacroAnchor::Synthesized {
                    macro_index: 2,
                    row: SynthesizedRowKind::ModelModifiersProp,
                },
            },
        }),
    ];

    for shape in shapes {
        match shape {
            MacroRuntimeShape::Props(props) => assert_eq!(props.props.len(), 1),
            MacroRuntimeShape::Emits(events) => assert_eq!(events.len(), 1),
            MacroRuntimeShape::Model(model) => {
                assert_eq!(model.update_event.name, "update:modelValue")
            }
        }
    }
}

#[test]
fn reason_taxonomies_are_closed_and_tsc_text_is_terminal() {
    let partial_labels = [
        MacroPartialReason::BudgetExceeded,
        MacroPartialReason::Cancelled,
        MacroPartialReason::SupersededGeneration,
        MacroPartialReason::UnstableState,
        MacroPartialReason::Recursion,
        MacroPartialReason::IncompleteTraversal,
    ]
    .map(|reason| match reason {
        MacroPartialReason::BudgetExceeded => "budget",
        MacroPartialReason::Cancelled => "cancelled",
        MacroPartialReason::SupersededGeneration => "superseded",
        MacroPartialReason::UnstableState => "unstable",
        MacroPartialReason::Recursion => "recursion",
        MacroPartialReason::IncompleteTraversal => "incomplete",
    });
    assert_eq!(partial_labels.len(), 6);

    let unresolved = [
        UnresolvedReason::MissingTypeArgument,
        UnresolvedReason::MissingDeclaration,
        UnresolvedReason::AmbiguousReference,
        UnresolvedReason::MissingDependency,
    ];
    for reason in unresolved {
        match reason {
            UnresolvedReason::MissingTypeArgument
            | UnresolvedReason::MissingDeclaration
            | UnresolvedReason::AmbiguousReference
            | UnresolvedReason::MissingDependency => {}
        }
    }

    let unsupported = [
        UnsupportedReason::MacroKind,
        UnsupportedReason::SemanticConstruct,
    ];
    for reason in unsupported {
        match reason {
            UnsupportedReason::MacroKind | UnsupportedReason::SemanticConstruct => {}
        }
    }

    let declaration_failures = [
        TscDeclarationFailureReason::SemanticInferenceUnavailable(
            TscSemanticInferenceUnavailableReason::DepthBudgetExceeded,
        ),
        TscDeclarationFailureReason::SemanticInferenceUnavailable(
            TscSemanticInferenceUnavailableReason::WorkBudgetExceeded,
        ),
        TscDeclarationFailureReason::Unsupported(UnsupportedReason::SemanticConstruct),
        TscDeclarationFailureReason::Unresolved(UnresolvedReason::MissingDependency),
    ];
    for failure in declaration_failures {
        match failure {
            TscDeclarationFailureReason::SemanticInferenceUnavailable(
                TscSemanticInferenceUnavailableReason::DepthBudgetExceeded
                | TscSemanticInferenceUnavailableReason::WorkBudgetExceeded,
            )
            | TscDeclarationFailureReason::Unsupported(_)
            | TscDeclarationFailureReason::Unresolved(_) => {}
        }
    }

    let text = TscSpliceText::new("(event: 'change', value: number) => void");
    assert_eq!(text.as_str(), "(event: 'change', value: number) => void");
}
