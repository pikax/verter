//! Contract test for the compiler-owned macro-surface DTOs.
//!
//! Pins the full field set of [`ResolvedMacroSurfaces`] and its members. The
//! test constructs a fully-populated bundle — props covering **every**
//! [`RuntimeCtorKind`] variant plus a non-public visibility, an expression
//! default, a method-shorthand default, an optional prop, JSDoc, a source-span
//! (`map_span`), and a type-dependency closure (`ts_type_deps`) that exercises
//! all three [`MacroTypeImportBindingDto`] variants and a local declaration;
//! native props carrying the visibility + span surface the `native_props` FFI
//! carrier re-sources; non-empty emits (call + tuple + none payload, plus a
//! `map_span` and a `payload_deps` closure); non-empty slots; a `withDefaults`
//! surface with per-prop entries and an unresolved-import fallback; and an emits
//! surface flagged `unresolved = true` — and asserts the constructed value
//! `PartialEq`-equals an independently spelled-out expected literal. Any field
//! dropped, renamed, or re-typed breaks the `PartialEq` and fails this test.

use verter_compiler::compile::{
    MacroDefaultDto, MacroDefaultEntryDto, MacroDefaultKindDto, MacroDefaultsArgDto,
    MacroDefaultsFallbackDto, MacroDefaultsFallbackKindDto, MacroEmitDto, MacroEmitPayload,
    MacroEmitsSurface, MacroExposeSurface, MacroLocalTypeDeclDto, MacroNativePropDto,
    MacroOptionsSurface, MacroPropDto, MacroPropsSurface, MacroSlotDto, MacroSlotsSurface,
    MacroSourceSpanDto, MacroTypeDepsDto, MacroTypeImportBindingDto, MacroTypeImportDto,
    MacroVisibility, MacroWithDefaultsDto, ResolvedMacroSurfaces, RuntimeCtorKind,
};

/// Build a props list that exercises every `RuntimeCtorKind` variant exactly
/// once across the prop set, plus the per-prop flags (optional, required,
/// default — expression and method-shorthand kinds, jsdoc, visibility,
/// declared_in_macro_type_arg), a `map_span` source span, and a `ts_type_deps`
/// closure covering all three import-binding variants and a local declaration.
fn sample_props() -> Vec<MacroPropDto> {
    vec![
        // String + Number multi-constructor, required, declared in T body,
        // with JSDoc, a real map_span, and a ts_type_deps closure exercising
        // every MacroTypeImportBindingDto variant + a local declaration.
        // Covers RuntimeCtorKind::String + ::Number.
        MacroPropDto {
            name: "id".to_string(),
            optional: false,
            required: true,
            default: None,
            map_span: Some(MacroSourceSpanDto {
                start: 100,
                end: 112,
            }),
            ts_type_deps: MacroTypeDepsDto {
                imports: vec![
                    MacroTypeImportDto {
                        source: "./types".to_string(),
                        bindings: vec![
                            // Named import without alias.
                            MacroTypeImportBindingDto::Named {
                                imported: "Id".to_string(),
                                local: None,
                            },
                            // Named import with alias.
                            MacroTypeImportBindingDto::Named {
                                imported: "RawId".to_string(),
                                local: Some("IdAlias".to_string()),
                            },
                        ],
                    },
                    MacroTypeImportDto {
                        source: "./brand".to_string(),
                        bindings: vec![
                            // Default import.
                            MacroTypeImportBindingDto::Default {
                                local: "Brand".to_string(),
                            },
                            // Namespace import.
                            MacroTypeImportBindingDto::Namespace {
                                local: "ns".to_string(),
                            },
                        ],
                    },
                ],
                local_declarations: vec![MacroLocalTypeDeclDto {
                    name: "Id".to_string(),
                    decl_ts: "type Id = string | number;".to_string(),
                }],
            },
            constructors: vec![RuntimeCtorKind::String, RuntimeCtorKind::Number],
            ts_type: "string | number".to_string(),
            declared_in_macro_type_arg: true,
            jsdoc: Some("/** The identifier. */".to_string()),
            visibility: MacroVisibility::Public,
        },
        // Boolean, optional, with an expression-kind withDefaults default —
        // covers ::Boolean and MacroDefaultKindDto::Expression.
        MacroPropDto {
            name: "disabled".to_string(),
            optional: true,
            required: false,
            default: Some(MacroDefaultDto {
                expr: "false".to_string(),
                kind: MacroDefaultKindDto::Expression,
                span: MacroSourceSpanDto {
                    start: 200,
                    end: 205,
                },
            }),
            map_span: None,
            ts_type_deps: MacroTypeDepsDto::default(),
            constructors: vec![RuntimeCtorKind::Boolean],
            ts_type: "boolean".to_string(),
            declared_in_macro_type_arg: true,
            jsdoc: None,
            visibility: MacroVisibility::Public,
        },
        // Object + Array, with a method-shorthand withDefaults default — covers
        // ::Object, ::Array and MacroDefaultKindDto::MethodShorthand.
        MacroPropDto {
            name: "items".to_string(),
            optional: false,
            required: true,
            default: Some(MacroDefaultDto {
                expr: "items() { return [] }".to_string(),
                kind: MacroDefaultKindDto::MethodShorthand,
                span: MacroSourceSpanDto {
                    start: 210,
                    end: 231,
                },
            }),
            map_span: Some(MacroSourceSpanDto {
                start: 120,
                end: 158,
            }),
            ts_type_deps: MacroTypeDepsDto::default(),
            constructors: vec![RuntimeCtorKind::Object, RuntimeCtorKind::Array],
            ts_type: "Record<string, unknown> | unknown[]".to_string(),
            declared_in_macro_type_arg: false,
            jsdoc: None,
            visibility: MacroVisibility::Protected,
        },
        // Function + Symbol — covers ::Function and ::Symbol.
        MacroPropDto {
            name: "onTick".to_string(),
            optional: true,
            required: false,
            default: None,
            map_span: None,
            ts_type_deps: MacroTypeDepsDto::default(),
            constructors: vec![RuntimeCtorKind::Function, RuntimeCtorKind::Symbol],
            ts_type: "(() => void) | symbol".to_string(),
            declared_in_macro_type_arg: true,
            jsdoc: None,
            visibility: MacroVisibility::Private,
        },
        // Null + BuiltIn + Unknown — covers ::Null, ::BuiltIn(_) and ::Unknown.
        MacroPropDto {
            name: "stamp".to_string(),
            optional: true,
            required: false,
            default: Some(MacroDefaultDto {
                expr: "new Date()".to_string(),
                kind: MacroDefaultKindDto::Expression,
                span: MacroSourceSpanDto {
                    start: 240,
                    end: 250,
                },
            }),
            map_span: None,
            ts_type_deps: MacroTypeDepsDto::default(),
            constructors: vec![
                RuntimeCtorKind::Null,
                RuntimeCtorKind::BuiltIn("Date".to_string()),
                RuntimeCtorKind::Unknown,
            ],
            ts_type: "Date | null".to_string(),
            declared_in_macro_type_arg: false,
            jsdoc: Some("/** Creation time. */".to_string()),
            visibility: MacroVisibility::Public,
        },
    ]
}

fn sample_emits() -> Vec<MacroEmitDto> {
    vec![
        // Call-signature payload, with a real map_span and a payload_deps
        // closure (so the emit surface's type-dependency carrier is exercised).
        MacroEmitDto {
            name: "change".to_string(),
            payload: MacroEmitPayload::Call {
                params_ts: "id: number".to_string(),
            },
            payload_ts: "id: number".to_string(),
            map_span: Some(MacroSourceSpanDto {
                start: 300,
                end: 330,
            }),
            payload_deps: MacroTypeDepsDto {
                imports: vec![MacroTypeImportDto {
                    source: "./events".to_string(),
                    bindings: vec![MacroTypeImportBindingDto::Named {
                        imported: "ChangePayload".to_string(),
                        local: None,
                    }],
                }],
                local_declarations: vec![],
            },
        },
        // Tuple shorthand payload.
        MacroEmitDto {
            name: "update".to_string(),
            payload: MacroEmitPayload::Tuple {
                tuple_ts: "[value: string]".to_string(),
            },
            payload_ts: "[value: string]".to_string(),
            map_span: None,
            payload_deps: MacroTypeDepsDto::default(),
        },
        // No payload.
        MacroEmitDto {
            name: "close".to_string(),
            payload: MacroEmitPayload::None,
            payload_ts: String::new(),
            map_span: None,
            payload_deps: MacroTypeDepsDto::default(),
        },
    ]
}

fn sample_slots() -> Vec<MacroSlotDto> {
    vec![
        MacroSlotDto {
            name: "default".to_string(),
            bindings_ts: Some("{ item: T }".to_string()),
            slot_ts: "(props: { item: T }) => any".to_string(),
        },
        MacroSlotDto {
            name: "header".to_string(),
            bindings_ts: None,
            slot_ts: "() => any".to_string(),
        },
    ]
}

/// Build a native-props list exercising the visibility surface + span the
/// `native_props` FFI carrier re-sources: a protected member with a real span
/// and a type annotation, plus a private optional member.
fn sample_native_props() -> Vec<MacroNativePropDto> {
    vec![
        MacroNativePropDto {
            name: "internalId".to_string(),
            is_optional: false,
            type_annotation: Some("number".to_string()),
            visibility: MacroVisibility::Protected,
            span_start: 42,
            span_end: 57,
        },
        MacroNativePropDto {
            name: "secret".to_string(),
            is_optional: true,
            type_annotation: Some("string | undefined".to_string()),
            visibility: MacroVisibility::Private,
            span_start: 60,
            span_end: 80,
        },
    ]
}

/// The resolved `withDefaults(...)` surface: the raw defaults argument, a
/// per-prop entry breakdown (≥1 entry), and an unresolved-import fallback that
/// suppresses the `XInvalidMacroType` import diagnostic.
fn sample_with_defaults() -> MacroWithDefaultsDto {
    MacroWithDefaultsDto {
        arg: MacroDefaultsArgDto {
            expr: "{ disabled: false, items() { return [] } }".to_string(),
            span: MacroSourceSpanDto {
                start: 195,
                end: 235,
            },
        },
        entries: vec![
            MacroDefaultEntryDto {
                name: "disabled".to_string(),
                default: MacroDefaultDto {
                    expr: "false".to_string(),
                    kind: MacroDefaultKindDto::Expression,
                    span: MacroSourceSpanDto {
                        start: 200,
                        end: 205,
                    },
                },
            },
            MacroDefaultEntryDto {
                name: "items".to_string(),
                default: MacroDefaultDto {
                    expr: "items() { return [] }".to_string(),
                    kind: MacroDefaultKindDto::MethodShorthand,
                    span: MacroSourceSpanDto {
                        start: 210,
                        end: 231,
                    },
                },
            },
        ],
        fallback: Some(MacroDefaultsFallbackDto {
            kind: MacroDefaultsFallbackKindDto::ObjectLiteral,
            suppress_unresolved_import_diagnostic: true,
        }),
    }
}

#[test]
fn resolved_macro_surfaces_full_contract() {
    let surfaces = ResolvedMacroSurfaces {
        props: MacroPropsSurface {
            props: sample_props(),
            root_constructors: vec![RuntimeCtorKind::Object],
            native_props: sample_native_props(),
            with_defaults: Some(sample_with_defaults()),
            unresolved: false,
        },
        emits: MacroEmitsSurface {
            emits: sample_emits(),
            // A macro flagged unresolved — drives XInvalidMacroType downstream.
            unresolved: true,
        },
        slots: MacroSlotsSurface {
            slots: sample_slots(),
            unresolved: false,
        },
        expose: MacroExposeSurface {
            type_ts: Some("{ focus: () => void }".to_string()),
            unresolved: false,
        },
        options: MacroOptionsSurface {
            inner_ts: Some("inheritAttrs: false".to_string()),
            unresolved: false,
        },
    };

    // Independently spelled-out expected value. PartialEq over the whole bundle
    // means dropping/renaming/retyping ANY field on ANY of the DTOs fails here.
    let expected = ResolvedMacroSurfaces {
        props: MacroPropsSurface {
            props: vec![
                MacroPropDto {
                    name: "id".to_string(),
                    optional: false,
                    required: true,
                    default: None,
                    map_span: Some(MacroSourceSpanDto {
                        start: 100,
                        end: 112,
                    }),
                    ts_type_deps: MacroTypeDepsDto {
                        imports: vec![
                            MacroTypeImportDto {
                                source: "./types".to_string(),
                                bindings: vec![
                                    MacroTypeImportBindingDto::Named {
                                        imported: "Id".to_string(),
                                        local: None,
                                    },
                                    MacroTypeImportBindingDto::Named {
                                        imported: "RawId".to_string(),
                                        local: Some("IdAlias".to_string()),
                                    },
                                ],
                            },
                            MacroTypeImportDto {
                                source: "./brand".to_string(),
                                bindings: vec![
                                    MacroTypeImportBindingDto::Default {
                                        local: "Brand".to_string(),
                                    },
                                    MacroTypeImportBindingDto::Namespace {
                                        local: "ns".to_string(),
                                    },
                                ],
                            },
                        ],
                        local_declarations: vec![MacroLocalTypeDeclDto {
                            name: "Id".to_string(),
                            decl_ts: "type Id = string | number;".to_string(),
                        }],
                    },
                    constructors: vec![RuntimeCtorKind::String, RuntimeCtorKind::Number],
                    ts_type: "string | number".to_string(),
                    declared_in_macro_type_arg: true,
                    jsdoc: Some("/** The identifier. */".to_string()),
                    visibility: MacroVisibility::Public,
                },
                MacroPropDto {
                    name: "disabled".to_string(),
                    optional: true,
                    required: false,
                    default: Some(MacroDefaultDto {
                        expr: "false".to_string(),
                        kind: MacroDefaultKindDto::Expression,
                        span: MacroSourceSpanDto {
                            start: 200,
                            end: 205,
                        },
                    }),
                    map_span: None,
                    ts_type_deps: MacroTypeDepsDto::default(),
                    constructors: vec![RuntimeCtorKind::Boolean],
                    ts_type: "boolean".to_string(),
                    declared_in_macro_type_arg: true,
                    jsdoc: None,
                    visibility: MacroVisibility::Public,
                },
                MacroPropDto {
                    name: "items".to_string(),
                    optional: false,
                    required: true,
                    default: Some(MacroDefaultDto {
                        expr: "items() { return [] }".to_string(),
                        kind: MacroDefaultKindDto::MethodShorthand,
                        span: MacroSourceSpanDto {
                            start: 210,
                            end: 231,
                        },
                    }),
                    map_span: Some(MacroSourceSpanDto {
                        start: 120,
                        end: 158,
                    }),
                    ts_type_deps: MacroTypeDepsDto::default(),
                    constructors: vec![RuntimeCtorKind::Object, RuntimeCtorKind::Array],
                    ts_type: "Record<string, unknown> | unknown[]".to_string(),
                    declared_in_macro_type_arg: false,
                    jsdoc: None,
                    visibility: MacroVisibility::Protected,
                },
                MacroPropDto {
                    name: "onTick".to_string(),
                    optional: true,
                    required: false,
                    default: None,
                    map_span: None,
                    ts_type_deps: MacroTypeDepsDto::default(),
                    constructors: vec![RuntimeCtorKind::Function, RuntimeCtorKind::Symbol],
                    ts_type: "(() => void) | symbol".to_string(),
                    declared_in_macro_type_arg: true,
                    jsdoc: None,
                    visibility: MacroVisibility::Private,
                },
                MacroPropDto {
                    name: "stamp".to_string(),
                    optional: true,
                    required: false,
                    default: Some(MacroDefaultDto {
                        expr: "new Date()".to_string(),
                        kind: MacroDefaultKindDto::Expression,
                        span: MacroSourceSpanDto {
                            start: 240,
                            end: 250,
                        },
                    }),
                    map_span: None,
                    ts_type_deps: MacroTypeDepsDto::default(),
                    constructors: vec![
                        RuntimeCtorKind::Null,
                        RuntimeCtorKind::BuiltIn("Date".to_string()),
                        RuntimeCtorKind::Unknown,
                    ],
                    ts_type: "Date | null".to_string(),
                    declared_in_macro_type_arg: false,
                    jsdoc: Some("/** Creation time. */".to_string()),
                    visibility: MacroVisibility::Public,
                },
            ],
            root_constructors: vec![RuntimeCtorKind::Object],
            native_props: vec![
                MacroNativePropDto {
                    name: "internalId".to_string(),
                    is_optional: false,
                    type_annotation: Some("number".to_string()),
                    visibility: MacroVisibility::Protected,
                    span_start: 42,
                    span_end: 57,
                },
                MacroNativePropDto {
                    name: "secret".to_string(),
                    is_optional: true,
                    type_annotation: Some("string | undefined".to_string()),
                    visibility: MacroVisibility::Private,
                    span_start: 60,
                    span_end: 80,
                },
            ],
            with_defaults: Some(MacroWithDefaultsDto {
                arg: MacroDefaultsArgDto {
                    expr: "{ disabled: false, items() { return [] } }".to_string(),
                    span: MacroSourceSpanDto {
                        start: 195,
                        end: 235,
                    },
                },
                entries: vec![
                    MacroDefaultEntryDto {
                        name: "disabled".to_string(),
                        default: MacroDefaultDto {
                            expr: "false".to_string(),
                            kind: MacroDefaultKindDto::Expression,
                            span: MacroSourceSpanDto {
                                start: 200,
                                end: 205,
                            },
                        },
                    },
                    MacroDefaultEntryDto {
                        name: "items".to_string(),
                        default: MacroDefaultDto {
                            expr: "items() { return [] }".to_string(),
                            kind: MacroDefaultKindDto::MethodShorthand,
                            span: MacroSourceSpanDto {
                                start: 210,
                                end: 231,
                            },
                        },
                    },
                ],
                fallback: Some(MacroDefaultsFallbackDto {
                    kind: MacroDefaultsFallbackKindDto::ObjectLiteral,
                    suppress_unresolved_import_diagnostic: true,
                }),
            }),
            unresolved: false,
        },
        emits: MacroEmitsSurface {
            emits: vec![
                MacroEmitDto {
                    name: "change".to_string(),
                    payload: MacroEmitPayload::Call {
                        params_ts: "id: number".to_string(),
                    },
                    payload_ts: "id: number".to_string(),
                    map_span: Some(MacroSourceSpanDto {
                        start: 300,
                        end: 330,
                    }),
                    payload_deps: MacroTypeDepsDto {
                        imports: vec![MacroTypeImportDto {
                            source: "./events".to_string(),
                            bindings: vec![MacroTypeImportBindingDto::Named {
                                imported: "ChangePayload".to_string(),
                                local: None,
                            }],
                        }],
                        local_declarations: vec![],
                    },
                },
                MacroEmitDto {
                    name: "update".to_string(),
                    payload: MacroEmitPayload::Tuple {
                        tuple_ts: "[value: string]".to_string(),
                    },
                    payload_ts: "[value: string]".to_string(),
                    map_span: None,
                    payload_deps: MacroTypeDepsDto::default(),
                },
                MacroEmitDto {
                    name: "close".to_string(),
                    payload: MacroEmitPayload::None,
                    payload_ts: String::new(),
                    map_span: None,
                    payload_deps: MacroTypeDepsDto::default(),
                },
            ],
            unresolved: true,
        },
        slots: MacroSlotsSurface {
            slots: vec![
                MacroSlotDto {
                    name: "default".to_string(),
                    bindings_ts: Some("{ item: T }".to_string()),
                    slot_ts: "(props: { item: T }) => any".to_string(),
                },
                MacroSlotDto {
                    name: "header".to_string(),
                    bindings_ts: None,
                    slot_ts: "() => any".to_string(),
                },
            ],
            unresolved: false,
        },
        expose: MacroExposeSurface {
            type_ts: Some("{ focus: () => void }".to_string()),
            unresolved: false,
        },
        options: MacroOptionsSurface {
            inner_ts: Some("inheritAttrs: false".to_string()),
            unresolved: false,
        },
    };

    assert_eq!(surfaces, expected);

    // Field-access cross-checks: prove specific fields carry the expected
    // values (not just that two identical literals are equal), and exercise the
    // helper methods so they stay covered.

    // Every RuntimeCtorKind variant appears across the prop set.
    let all_ctors: Vec<&RuntimeCtorKind> = surfaces
        .props
        .props
        .iter()
        .flat_map(|p| p.constructors.iter())
        .collect();
    for expected_variant in [
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
    ] {
        assert!(
            all_ctors.iter().any(|c| **c == expected_variant),
            "RuntimeCtorKind variant {expected_variant:?} missing from the prop set"
        );
    }

    // Constructor rendering matches the Vue runtime constructor identifiers.
    assert_eq!(RuntimeCtorKind::String.as_constructor(), "String");
    assert_eq!(
        RuntimeCtorKind::BuiltIn("Map".to_string()).as_constructor(),
        "Map"
    );
    assert_eq!(RuntimeCtorKind::Null.as_constructor(), "null");
    assert_eq!(RuntimeCtorKind::Unknown.as_constructor(), "null");

    // Visibility is carried per prop and renders to the FFI wire strings.
    let items = surfaces
        .props
        .props
        .iter()
        .find(|p| p.name == "items")
        .expect("items prop present");
    assert_eq!(items.visibility, MacroVisibility::Protected);
    assert_eq!(items.visibility.as_wire_str(), "protected");
    assert!(!items.visibility.is_public());
    assert!(MacroVisibility::default().is_public());

    // Default value + optional flags survive on the right props, and the
    // expression-kind default carries its expr/kind/span.
    let disabled = surfaces
        .props
        .props
        .iter()
        .find(|p| p.name == "disabled")
        .expect("disabled prop present");
    assert!(disabled.optional);
    assert!(!disabled.required);
    let disabled_default = disabled
        .default
        .as_ref()
        .expect("disabled carries a withDefaults default");
    assert_eq!(disabled_default.expr, "false");
    assert_eq!(disabled_default.kind, MacroDefaultKindDto::Expression);
    assert_eq!(disabled_default.span.start, 200);
    assert_eq!(disabled_default.span.end, 205);
    assert!(disabled_default.span.start < disabled_default.span.end);

    // The method-shorthand default kind is distinguishable from the expression
    // kind (the consumer must not re-scan the text to recover this).
    let items_default = items
        .default
        .as_ref()
        .expect("items carries a withDefaults default");
    assert_eq!(items_default.kind, MacroDefaultKindDto::MethodShorthand);
    assert_ne!(items_default.kind, disabled_default.kind);

    // map_span is carried where the prop has a real source location and absent
    // otherwise.
    let id = &surfaces.props.props[0];
    let id_span = id.map_span.expect("id carries a map_span");
    assert_eq!(id_span.start, 100);
    assert_eq!(id_span.end, 112);
    assert!(id_span.start < id_span.end);
    assert!(disabled.map_span.is_none());

    // ts_type_deps exercises all three import-binding variants + a local decl.
    assert_eq!(id.ts_type_deps.imports.len(), 2);
    assert_eq!(id.ts_type_deps.imports[0].source, "./types");
    assert_eq!(
        id.ts_type_deps.imports[0].bindings[0],
        MacroTypeImportBindingDto::Named {
            imported: "Id".to_string(),
            local: None,
        }
    );
    assert_eq!(
        id.ts_type_deps.imports[0].bindings[1],
        MacroTypeImportBindingDto::Named {
            imported: "RawId".to_string(),
            local: Some("IdAlias".to_string()),
        }
    );
    assert_eq!(
        id.ts_type_deps.imports[1].bindings[0],
        MacroTypeImportBindingDto::Default {
            local: "Brand".to_string(),
        }
    );
    assert_eq!(
        id.ts_type_deps.imports[1].bindings[1],
        MacroTypeImportBindingDto::Namespace {
            local: "ns".to_string(),
        }
    );
    assert_eq!(id.ts_type_deps.local_declarations.len(), 1);
    assert_eq!(id.ts_type_deps.local_declarations[0].name, "Id");
    assert_eq!(
        id.ts_type_deps.local_declarations[0].decl_ts,
        "type Id = string | number;"
    );
    // Props with no type deps carry the empty (Default) closure.
    assert!(disabled.ts_type_deps.imports.is_empty());
    assert!(disabled.ts_type_deps.local_declarations.is_empty());

    // JSDoc survives.
    assert_eq!(id.jsdoc.as_deref(), Some("/** The identifier. */"));
    assert!(id.declared_in_macro_type_arg);

    // The unresolved flag is set on the emits surface only.
    assert!(surfaces.emits.unresolved);
    assert!(!surfaces.props.unresolved);
    assert!(!surfaces.slots.unresolved);

    // Emit payload shapes are distinguishable.
    assert!(matches!(
        surfaces.emits.emits[0].payload,
        MacroEmitPayload::Call { .. }
    ));
    assert!(matches!(
        surfaces.emits.emits[1].payload,
        MacroEmitPayload::Tuple { .. }
    ));
    assert_eq!(surfaces.emits.emits[2].payload, MacroEmitPayload::None);

    // Emit map_span + payload_deps carry on the right emit.
    let change = &surfaces.emits.emits[0];
    let change_span = change.map_span.expect("change emit carries a map_span");
    assert_eq!(change_span.start, 300);
    assert_eq!(change_span.end, 330);
    assert!(change_span.start < change_span.end);
    assert_eq!(change.payload_deps.imports.len(), 1);
    assert_eq!(change.payload_deps.imports[0].source, "./events");
    assert_eq!(
        change.payload_deps.imports[0].bindings[0],
        MacroTypeImportBindingDto::Named {
            imported: "ChangePayload".to_string(),
            local: None,
        }
    );
    // Emits without a payload closure carry the empty (Default) closure + no span.
    assert!(surfaces.emits.emits[1].map_span.is_none());
    assert!(surfaces.emits.emits[1].payload_deps.imports.is_empty());

    // The withDefaults surface carries the raw arg, per-prop entries, and the
    // unresolved-import fallback signal.
    let with_defaults = surfaces
        .props
        .with_defaults
        .as_ref()
        .expect("props surface carries a withDefaults surface");
    assert_eq!(
        with_defaults.arg.expr,
        "{ disabled: false, items() { return [] } }"
    );
    assert_eq!(with_defaults.arg.span.start, 195);
    assert_eq!(with_defaults.arg.span.end, 235);
    assert_eq!(with_defaults.entries.len(), 2);
    assert_eq!(with_defaults.entries[0].name, "disabled");
    assert_eq!(
        with_defaults.entries[0].default.kind,
        MacroDefaultKindDto::Expression
    );
    assert_eq!(with_defaults.entries[1].name, "items");
    assert_eq!(
        with_defaults.entries[1].default.kind,
        MacroDefaultKindDto::MethodShorthand
    );
    let fallback = with_defaults
        .fallback
        .as_ref()
        .expect("withDefaults carries a fallback signal");
    assert_eq!(fallback.kind, MacroDefaultsFallbackKindDto::ObjectLiteral);
    assert!(fallback.suppress_unresolved_import_diagnostic);

    // Root constructors carry the object-like marker used by the diagnostics
    // object-like check.
    assert_eq!(
        surfaces.props.root_constructors,
        vec![RuntimeCtorKind::Object]
    );

    // Native props carry the visibility + span surface the FFI carrier
    // re-sources. Assert each field directly so a dropped/renamed/retyped field
    // fails here, not only via the whole-bundle equality above.
    assert_eq!(surfaces.props.native_props.len(), 2);
    let internal_id = &surfaces.props.native_props[0];
    assert_eq!(internal_id.name, "internalId");
    assert!(!internal_id.is_optional);
    assert_eq!(internal_id.type_annotation.as_deref(), Some("number"));
    assert_eq!(internal_id.visibility, MacroVisibility::Protected);
    assert_eq!(internal_id.visibility.as_wire_str(), "protected");
    // Span is preserved and well-formed (start < end), matching the FFI
    // `span_start` / `span_end` fields.
    assert_eq!(internal_id.span_start, 42);
    assert_eq!(internal_id.span_end, 57);
    assert!(internal_id.span_start < internal_id.span_end);

    let secret = &surfaces.props.native_props[1];
    assert_eq!(secret.name, "secret");
    assert!(secret.is_optional);
    assert_eq!(
        secret.type_annotation.as_deref(),
        Some("string | undefined")
    );
    assert_eq!(secret.visibility, MacroVisibility::Private);
    assert!(!secret.visibility.is_public());
    assert_eq!(secret.span_start, 60);
    assert_eq!(secret.span_end, 80);
    assert!(secret.span_start < secret.span_end);
}

/// An absent macro is the surface's `Default`: empty + not unresolved, with all
/// the new optional/closure fields at their empty defaults.
#[test]
fn default_surfaces_are_empty_and_resolved() {
    let surfaces = ResolvedMacroSurfaces::default();
    assert!(surfaces.props.props.is_empty());
    assert!(surfaces.props.root_constructors.is_empty());
    assert!(surfaces.props.native_props.is_empty());
    // The withDefaults surface defaults to absent.
    assert!(surfaces.props.with_defaults.is_none());
    assert!(!surfaces.props.unresolved);
    assert!(surfaces.emits.emits.is_empty());
    assert!(!surfaces.emits.unresolved);
    assert!(surfaces.slots.slots.is_empty());
    assert!(surfaces.expose.type_ts.is_none());
    assert!(surfaces.options.inner_ts.is_none());
}

/// The new per-element optional/closure fields default to their empty forms when
/// a producer leaves them unset: no default, no `map_span`, and an empty
/// type-dependency closure on both a prop and an emit. `MacroPropDto` /
/// `MacroEmitDto` are not `Default`-deriving (they have required identity
/// fields), so this constructs minimal values explicitly — exercising the
/// `MacroTypeDepsDto::default()` empty closure that producers use for elements
/// with no cross-file dependencies.
#[test]
fn unset_element_new_fields_are_empty() {
    let bare_prop = MacroPropDto {
        name: "x".to_string(),
        optional: false,
        required: true,
        default: None,
        map_span: None,
        ts_type_deps: MacroTypeDepsDto::default(),
        constructors: vec![],
        ts_type: "number".to_string(),
        declared_in_macro_type_arg: false,
        jsdoc: None,
        visibility: MacroVisibility::default(),
    };
    assert!(bare_prop.default.is_none());
    assert!(bare_prop.map_span.is_none());
    assert!(bare_prop.ts_type_deps.imports.is_empty());
    assert!(bare_prop.ts_type_deps.local_declarations.is_empty());

    let bare_emit = MacroEmitDto {
        name: "y".to_string(),
        payload: MacroEmitPayload::None,
        payload_ts: String::new(),
        map_span: None,
        payload_deps: MacroTypeDepsDto::default(),
    };
    assert!(bare_emit.map_span.is_none());
    assert!(bare_emit.payload_deps.imports.is_empty());
    assert!(bare_emit.payload_deps.local_declarations.is_empty());
}
