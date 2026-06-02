//! Contract test for the compiler-owned macro-surface DTOs.
//!
//! Pins the full field set of [`ResolvedMacroSurfaces`] and its members. The
//! test constructs a fully-populated bundle — props covering **every**
//! [`RuntimeCtorKind`] variant plus a non-public visibility, a default, an
//! optional prop, and JSDoc; native props carrying the visibility + span
//! surface the `native_props` FFI carrier re-sources; non-empty emits (call +
//! tuple + none payload); non-empty slots; and an emits surface flagged
//! `unresolved = true` — and asserts the constructed value `PartialEq`-equals an
//! independently spelled-out expected literal. Any field dropped, renamed, or
//! re-typed breaks the `PartialEq` and fails this test.

use verter_compiler::compile::{
    MacroEmitDto, MacroEmitPayload, MacroEmitsSurface, MacroExposeSurface, MacroNativePropDto,
    MacroOptionsSurface, MacroPropDto, MacroPropsSurface, MacroSlotDto, MacroSlotsSurface,
    MacroVisibility, ResolvedMacroSurfaces, RuntimeCtorKind,
};

/// Build a props list that exercises every `RuntimeCtorKind` variant exactly
/// once across the prop set, plus the per-prop flags (optional, required,
/// default, jsdoc, visibility, declared_in_macro_type_arg).
fn sample_props() -> Vec<MacroPropDto> {
    vec![
        // String + Number multi-constructor, required, declared in T body,
        // with JSDoc — covers RuntimeCtorKind::String + ::Number.
        MacroPropDto {
            name: "id".to_string(),
            optional: false,
            required: true,
            default_value: None,
            constructors: vec![RuntimeCtorKind::String, RuntimeCtorKind::Number],
            ts_type: "string | number".to_string(),
            declared_in_macro_type_arg: true,
            jsdoc: Some("/** The identifier. */".to_string()),
            visibility: MacroVisibility::Public,
        },
        // Boolean, optional, with a withDefaults default — covers ::Boolean.
        MacroPropDto {
            name: "disabled".to_string(),
            optional: true,
            required: false,
            default_value: Some("false".to_string()),
            constructors: vec![RuntimeCtorKind::Boolean],
            ts_type: "boolean".to_string(),
            declared_in_macro_type_arg: true,
            jsdoc: None,
            visibility: MacroVisibility::Public,
        },
        // Object + Array — covers ::Object and ::Array.
        MacroPropDto {
            name: "items".to_string(),
            optional: false,
            required: true,
            default_value: None,
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
            default_value: None,
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
            default_value: Some("new Date()".to_string()),
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
        // Call-signature payload.
        MacroEmitDto {
            name: "change".to_string(),
            payload: MacroEmitPayload::Call {
                params_ts: "id: number".to_string(),
            },
            payload_ts: "id: number".to_string(),
        },
        // Tuple shorthand payload.
        MacroEmitDto {
            name: "update".to_string(),
            payload: MacroEmitPayload::Tuple {
                tuple_ts: "[value: string]".to_string(),
            },
            payload_ts: "[value: string]".to_string(),
        },
        // No payload.
        MacroEmitDto {
            name: "close".to_string(),
            payload: MacroEmitPayload::None,
            payload_ts: String::new(),
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

#[test]
fn resolved_macro_surfaces_full_contract() {
    let surfaces = ResolvedMacroSurfaces {
        props: MacroPropsSurface {
            props: sample_props(),
            root_constructors: vec![RuntimeCtorKind::Object],
            native_props: sample_native_props(),
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
                    default_value: None,
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
                    default_value: Some("false".to_string()),
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
                    default_value: None,
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
                    default_value: None,
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
                    default_value: Some("new Date()".to_string()),
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
                },
                MacroEmitDto {
                    name: "update".to_string(),
                    payload: MacroEmitPayload::Tuple {
                        tuple_ts: "[value: string]".to_string(),
                    },
                    payload_ts: "[value: string]".to_string(),
                },
                MacroEmitDto {
                    name: "close".to_string(),
                    payload: MacroEmitPayload::None,
                    payload_ts: String::new(),
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

    // Default value + optional flags survive on the right props.
    let disabled = surfaces
        .props
        .props
        .iter()
        .find(|p| p.name == "disabled")
        .expect("disabled prop present");
    assert!(disabled.optional);
    assert!(!disabled.required);
    assert_eq!(disabled.default_value.as_deref(), Some("false"));

    // JSDoc survives.
    let id = &surfaces.props.props[0];
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

/// An absent macro is the surface's `Default`: empty + not unresolved.
#[test]
fn default_surfaces_are_empty_and_resolved() {
    let surfaces = ResolvedMacroSurfaces::default();
    assert!(surfaces.props.props.is_empty());
    assert!(surfaces.props.root_constructors.is_empty());
    assert!(surfaces.props.native_props.is_empty());
    assert!(!surfaces.props.unresolved);
    assert!(surfaces.emits.emits.is_empty());
    assert!(!surfaces.emits.unresolved);
    assert!(surfaces.slots.slots.is_empty());
    assert!(surfaces.expose.type_ts.is_none());
    assert!(surfaces.options.inner_ts.is_none());
}
