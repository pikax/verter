//! C2 non-flow gateway contract tests (charter home:
//! `crates/verter_semantic/tests/type_info_non_flow.rs`).
//!
//! Every table row proves, for its operation: the all-missing
//! `NeedInputs` outcome with the operation's missing-input proof id and
//! load set; the complete payload's identities, provenance and
//! deterministic ordering; and the preloaded-vs-staged equivalence (a
//! snapshot preloaded before the attempt equals one staged after a
//! missing round — there is no warm-state advantage or penalty).

use std::sync::Arc;

use verter_macro_dto::{MacroAnchor, RuntimePropType, SynthesizedRowKind};
use verter_semantic::analysis::types::{AnalyzedExposeField, ImportBindingKind};
use verter_semantic::analysis::{
    AnalyzedEmitField, AnalyzedImport, AnalyzedImportBinding, AnalyzedMacro, AnalyzedMacroKind,
    AnalyzedPropField, MacroTypeDep, MacroTypeDepUsage, ScriptAnalysisSnapshot,
    TypeResolutionSource,
};
use verter_semantic::resolver_core::ResolutionBasis;
use verter_semantic::type_info::{
    ImportedComponentResolution, MacroSemanticLane, NonFlowObservationKey,
    NonFlowObservationSnapshot, NonFlowOperation, NonFlowOutcome, NonFlowPayload,
    ObservedMacroSurface, ObservedSurfaceMember, TypeInfoCore, MISSING_PROOF_EMITS,
    MISSING_PROOF_EXPOSE, MISSING_PROOF_IMPORTED_COMPONENT, MISSING_PROOF_MODEL,
    MISSING_PROOF_PROPS, MISSING_PROOF_VUE_MACRO,
};
use verter_span::Span;
use verter_type_expr::locators::{
    AuthoredAnchor, LocatorSymbolSpace, MacroPayloadLocator, MacroPayloadPosition,
};
use verter_type_expr::TopLevelOwnerId;

const OWNER: &str = "/src/App.vue";

fn basis() -> ResolutionBasis {
    ResolutionBasis::unbound_placeholder()
}

fn span(start: u32, end: u32) -> Span {
    Span::new(start, end)
}

fn prop_field(name: &str, optional: bool, at: u32) -> AnalyzedPropField {
    AnalyzedPropField {
        name: name.to_owned(),
        is_optional: optional,
        span: span(at, at + name.len() as u32),
        type_annotation: None,
        payload: None,
        type_expr_scope: None,
        description: None,
        tags: Vec::new(),
        resolution_source: TypeResolutionSource::default(),
        resolution_error: None,
        declared_in_macro_type_arg: false,
        constructor_bindings: Vec::new(),
    }
}

fn emit_field(name: &str, at: u32) -> AnalyzedEmitField {
    AnalyzedEmitField {
        name: name.to_owned(),
        span: span(at, at + name.len() as u32),
        call_signature_span: None,
        payload_type: None,
        payload: None,
        payload_expr_scope: None,
        description: None,
        tags: Vec::new(),
    }
}

fn expose_field(name: &str, at: u32) -> AnalyzedExposeField {
    AnalyzedExposeField {
        name: name.to_owned(),
        span: Some(span(at, at + name.len() as u32)),
        payload: None,
        type_expr_scope: None,
        referenced_binding: None,
        description: None,
        tags: Vec::new(),
    }
}

fn analyzed_macro(kind: AnalyzedMacroKind, at: u32, end: u32) -> AnalyzedMacro {
    AnalyzedMacro {
        kind,
        owner: TopLevelOwnerId::default(),
        is_type_based: true,
        type_references: Vec::new(),
        binding_name: None,
        model_name: None,
        has_inherit_attrs_false: false,
        prop_fields: Vec::new(),
        emit_fields: Vec::new(),
        slot_fields: Vec::new(),
        default_keys: Vec::new(),
        default_values: Vec::new(),
        expose_fields: Vec::new(),
        resolved_local_types: Vec::new(),
        parsed_type_argument: None,
        parsed_type_argument_scope: None,
        edit_anchors: Default::default(),
        span: span(at, end),
    }
}

/// A representative SFC analysis: one type-based `defineProps` with two
/// authored prop fields, one type-based `defineEmits` with one authored
/// field, one `defineModel`, one runtime-object `defineExpose`, one
/// `defineOptions` (skipped), and a member-tier + a surface-tier type
/// dependency on the props macro.
fn fixture_analysis() -> ScriptAnalysisSnapshot {
    let mut props = analyzed_macro(AnalyzedMacroKind::DefineProps, 10, 80);
    props.prop_fields = vec![
        prop_field("count", false, 30),
        prop_field("label", true, 45),
    ];
    props.parsed_type_argument = None;
    // Re-mark as carrying a type argument through the locator's presence:
    // the fixture builds it via the same public analyzer field the
    // session populates, so use a minimal locator placeholder.
    props.parsed_type_argument = macro_payload_locator();

    let mut emits = analyzed_macro(AnalyzedMacroKind::DefineEmits, 90, 150);
    emits.emit_fields = vec![emit_field("saved", 110)];
    emits.parsed_type_argument = macro_payload_locator();

    let mut model = analyzed_macro(AnalyzedMacroKind::DefineModel, 160, 210);
    model.model_name = Some("title".to_owned());
    model.prop_fields = vec![prop_field("title", true, 180)];

    let mut expose = analyzed_macro(AnalyzedMacroKind::DefineExpose, 220, 270);
    expose.is_type_based = false;
    expose.expose_fields = vec![expose_field("reset", 235), expose_field("refresh", 250)];

    let skipped_options = analyzed_macro(AnalyzedMacroKind::DefineOptions, 280, 320);

    let mut deps = Vec::new();
    for (type_name, import_source, usage) in [
        ("BadgeProps", "./Badge.vue", MacroTypeDepUsage::Surface),
        ("Count", "./types", MacroTypeDepUsage::Member),
    ] {
        deps.push(MacroTypeDep {
            type_name: type_name.to_owned(),
            import_source: import_source.to_owned(),
            macro_kind: AnalyzedMacroKind::DefineProps,
            macro_index: 0,
            macro_span: span(10, 80),
            usage,
        });
    }

    let mut import = AnalyzedImport {
        source: "./Badge.vue".to_owned(),
        owner: TopLevelOwnerId::default(),
        is_type_only: true,
        bindings: vec![AnalyzedImportBinding {
            name: "Badge".to_owned(),
            kind: ImportBindingKind::Named,
            imported_name: Some("Badge".to_owned()),
            is_type_only: true,
            vue_api: None,
            span: span(0, 9),
        }],
        span: span(0, 9),
        resolved_canonical_id: None,
    };
    import.bindings.clear();

    ScriptAnalysisSnapshot {
        macros: vec![props, emits, model, expose, skipped_options],
        macro_type_deps: deps,
        imports: vec![import],
        ..ScriptAnalysisSnapshot::default()
    }
}

fn macro_payload_locator() -> Option<MacroPayloadLocator> {
    // Content-free locator: presence is what the inventory reads.
    Some(MacroPayloadLocator {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(OWNER),
            owner: TopLevelOwnerId::default(),
            symbol: Arc::from("__macro_fixture__"),
            space: LocatorSymbolSpace::Type,
        },
        macro_index: 0,
        payload: MacroPayloadPosition::TypeArgument,
    })
}

fn props_surface() -> ObservedMacroSurface {
    ObservedMacroSurface {
        members: vec![
            ObservedSurfaceMember {
                published_name: Some("count".to_owned()),
                optional: false,
                is_public: true,
                referenced_type_name: Some("Count".to_owned()),
            },
            ObservedSurfaceMember {
                published_name: Some("label".to_owned()),
                optional: true,
                is_public: true,
                referenced_type_name: None,
            },
            ObservedSurfaceMember {
                published_name: Some("secret".to_owned()),
                optional: true,
                is_public: false,
                referenced_type_name: None,
            },
            ObservedSurfaceMember {
                published_name: None,
                optional: false,
                is_public: true,
                referenced_type_name: None,
            },
        ],
        call_signature_event_names: Vec::new(),
    }
}

fn emits_surface() -> ObservedMacroSurface {
    ObservedMacroSurface {
        members: vec![ObservedSurfaceMember {
            published_name: Some("saved".to_owned()),
            optional: false,
            is_public: true,
            referenced_type_name: None,
        }],
        call_signature_event_names: vec!["saved".to_owned(), "deleted".to_owned()],
    }
}

/// Fully-loaded snapshot for the fixture analysis.
fn complete_snapshot() -> Arc<NonFlowObservationSnapshot> {
    let mut snapshot = NonFlowObservationSnapshot::new();
    snapshot.stage_script_analysis(Arc::from(OWNER), Arc::new(fixture_analysis()));
    snapshot.stage_macro_surface(Arc::from(OWNER), 0, props_surface());
    snapshot.stage_macro_surface(Arc::from(OWNER), 1, emits_surface());
    snapshot.stage_model_value_type_shape(
        Arc::from(OWNER),
        2,
        RuntimePropType::Resolved {
            constructors: Default::default(),
            skip_check: false,
        },
    );
    snapshot.stage_import_resolution(
        Arc::from(OWNER),
        ImportedComponentResolution {
            specifier: Arc::from("./Badge.vue"),
            resolved_canonical: Arc::from("/src/Badge.vue"),
        },
    );
    Arc::new(snapshot)
}

fn core_of(snapshot: Arc<NonFlowObservationSnapshot>) -> TypeInfoCore {
    TypeInfoCore::from_observation_snapshot(snapshot, basis())
}

/// The six-row table: all-missing ⇒ `NeedInputs` whose load set is the
/// operation's own single derivation and whose proof id is the charter
/// row's stable id. The expected keys are pinned per row — they must
/// name the observation slots the kernel's route reads (staging each
/// of them satisfies the next round), never a fabricated resolver I/O
/// identity such as a `DeclBody` the gateway cannot consume.
#[test]
fn all_missing_rows_report_their_proof_ids() {
    fn script_analysis() -> NonFlowObservationKey {
        NonFlowObservationKey::ScriptAnalysis {
            owner_canonical: Arc::from(OWNER),
        }
    }
    fn macro_surface(macro_index: usize) -> NonFlowObservationKey {
        NonFlowObservationKey::MacroSurface {
            owner_canonical: Arc::from(OWNER),
            macro_index,
        }
    }
    fn model_value_type_shape(macro_index: usize) -> NonFlowObservationKey {
        NonFlowObservationKey::ModelValueTypeShape {
            owner_canonical: Arc::from(OWNER),
            macro_index,
        }
    }
    let table = [
        (
            NonFlowOperation::ProjectVueMacroSemantics {
                owner_canonical: Arc::from(OWNER),
            },
            MISSING_PROOF_VUE_MACRO,
            vec![script_analysis()],
        ),
        (
            NonFlowOperation::ResolveImportedComponentSurface {
                owner_canonical: Arc::from(OWNER),
                type_reference: Arc::from("BadgeProps"),
                referenced_canonical: Some(Arc::from("/src/Badge.vue")),
            },
            MISSING_PROOF_IMPORTED_COMPONENT,
            vec![script_analysis()],
        ),
        (
            NonFlowOperation::ProjectRuntimeProps {
                owner_canonical: Arc::from(OWNER),
                macro_index: 0,
            },
            MISSING_PROOF_PROPS,
            vec![script_analysis(), macro_surface(0)],
        ),
        (
            NonFlowOperation::ProjectRuntimeEmits {
                owner_canonical: Arc::from(OWNER),
                macro_index: 1,
            },
            MISSING_PROOF_EMITS,
            vec![script_analysis(), macro_surface(1)],
        ),
        (
            NonFlowOperation::ProjectRuntimeModel {
                owner_canonical: Arc::from(OWNER),
                macro_index: 2,
            },
            MISSING_PROOF_MODEL,
            vec![script_analysis(), model_value_type_shape(2)],
        ),
        (
            NonFlowOperation::ProjectExposeSurface {
                owner_canonical: Arc::from(OWNER),
                macro_index: 3,
            },
            MISSING_PROOF_EXPOSE,
            vec![script_analysis(), macro_surface(3)],
        ),
    ];
    let core = core_of(Arc::new(NonFlowObservationSnapshot::new()));
    for (operation, proof_id, expected_keys) in &table {
        let outcome = core.attempt(operation);
        let NonFlowOutcome::NeedInputs(load_set) = outcome else {
            panic!("{proof_id}: all-missing attempt must be NeedInputs, got {outcome:?}");
        };
        assert_eq!(
            operation.missing_input_proof_id(),
            *proof_id,
            "proof id is the charter row's stable id"
        );
        assert_eq!(
            load_set.keys(),
            expected_keys,
            "{proof_id}: keys must name the missing observation slots staging satisfies"
        );
        assert_eq!(
            load_set.keys(),
            operation.missing_input_load_set(basis()).keys(),
            "{proof_id}: load set is the operation's single derivation"
        );
    }
}

/// Partially-missing rows still report `NeedInputs` with their own proof
/// id: the per-macro operations need their specific observation, not just
/// the owner analysis.
#[test]
fn per_macro_missing_observations_report_their_proof_ids() {
    let mut snapshot = NonFlowObservationSnapshot::new();
    snapshot.stage_script_analysis(Arc::from(OWNER), Arc::new(fixture_analysis()));
    let core = core_of(Arc::new(snapshot));
    let table = [
        (
            NonFlowOperation::ProjectRuntimeProps {
                owner_canonical: Arc::from(OWNER),
                macro_index: 0,
            },
            MISSING_PROOF_PROPS,
        ),
        (
            NonFlowOperation::ProjectRuntimeEmits {
                owner_canonical: Arc::from(OWNER),
                macro_index: 1,
            },
            MISSING_PROOF_EMITS,
        ),
        (
            NonFlowOperation::ProjectRuntimeModel {
                owner_canonical: Arc::from(OWNER),
                macro_index: 2,
            },
            MISSING_PROOF_MODEL,
        ),
    ];
    for (operation, proof_id) in &table {
        assert!(
            matches!(core.attempt(operation), NonFlowOutcome::NeedInputs(_)),
            "{proof_id}: missing observation must stay NeedInputs"
        );
        assert_eq!(operation.missing_input_proof_id(), *proof_id);
    }
    // Expose needs no staged surface — the analysis carries the fields.
    let expose = NonFlowOperation::ProjectExposeSurface {
        owner_canonical: Arc::from(OWNER),
        macro_index: 3,
    };
    assert!(matches!(core.attempt(&expose), NonFlowOutcome::Complete(_)));
}

/// Preloaded-vs-staged equivalence: a snapshot that had everything before
/// the attempt and one that was staged after a missing round produce the
/// identical complete payload — no warm-state divergence.
#[test]
fn preloaded_and_staged_snapshots_are_equivalent() {
    let operations = [
        NonFlowOperation::ProjectVueMacroSemantics {
            owner_canonical: Arc::from(OWNER),
        },
        NonFlowOperation::ResolveImportedComponentSurface {
            owner_canonical: Arc::from(OWNER),
            type_reference: Arc::from("BadgeProps"),
            referenced_canonical: Some(Arc::from("/src/Badge.vue")),
        },
        NonFlowOperation::ProjectRuntimeProps {
            owner_canonical: Arc::from(OWNER),
            macro_index: 0,
        },
        NonFlowOperation::ProjectRuntimeEmits {
            owner_canonical: Arc::from(OWNER),
            macro_index: 1,
        },
        NonFlowOperation::ProjectRuntimeModel {
            owner_canonical: Arc::from(OWNER),
            macro_index: 2,
        },
        NonFlowOperation::ProjectExposeSurface {
            owner_canonical: Arc::from(OWNER),
            macro_index: 3,
        },
    ];
    let preloaded_core = core_of(complete_snapshot());
    let mut staged = NonFlowObservationSnapshot::new();
    let staged_core = TypeInfoCore::from_observation_snapshot(Arc::new(staged.clone()), basis());
    for operation in &operations {
        assert!(
            matches!(
                staged_core.attempt(operation),
                NonFlowOutcome::NeedInputs(_)
            ),
            "empty snapshot must report NeedInputs first"
        );
    }
    drop(staged_core);
    staged.stage_script_analysis(Arc::from(OWNER), Arc::new(fixture_analysis()));
    staged.stage_macro_surface(Arc::from(OWNER), 0, props_surface());
    staged.stage_macro_surface(Arc::from(OWNER), 1, emits_surface());
    staged.stage_model_value_type_shape(
        Arc::from(OWNER),
        2,
        RuntimePropType::Resolved {
            constructors: Default::default(),
            skip_check: false,
        },
    );
    staged.stage_import_resolution(
        Arc::from(OWNER),
        ImportedComponentResolution {
            specifier: Arc::from("./Badge.vue"),
            resolved_canonical: Arc::from("/src/Badge.vue"),
        },
    );
    let staged_core = core_of(Arc::new(staged));
    for operation in &operations {
        assert_eq!(
            preloaded_core.attempt(operation),
            staged_core.attempt(operation),
            "preloaded and staged snapshots agree for {operation:?}"
        );
    }
}

/// The route-removal mutation rail of the six-row table
/// (`C2-GAP3-WILDCARD-DISPATCH`). Every row must dispatch to its OWN
/// route: a `TypeInfoCore::attempt` arm swallowed to `need_inputs`, to a
/// sibling payload, or under a `_` wildcard fails the complete-snapshot
/// leg below (each row must complete with its own payload variant), and
/// a `missing_input_proof_id` arm collapsed onto a sibling's id fails
/// the pairwise-distinct rail. The variant match is exhaustive on
/// purpose — a seventh payload cannot strand this rail either.
#[test]
fn route_removal_mutations_fail_on_every_row() {
    fn payload_variant(payload: &NonFlowPayload) -> &'static str {
        match payload {
            NonFlowPayload::VueMacroSemanticInput(_) => "VueMacroSemanticInput",
            NonFlowPayload::ImportedComponentSurface(_) => "ImportedComponentSurface",
            NonFlowPayload::RuntimePropsProjection(_) => "RuntimePropsProjection",
            NonFlowPayload::RuntimeEmitsProjection(_) => "RuntimeEmitsProjection",
            NonFlowPayload::RuntimeModelProjection(_) => "RuntimeModelProjection",
            NonFlowPayload::ExposeSurfaceProjection(_) => "ExposeSurfaceProjection",
        }
    }
    let table = [
        (
            NonFlowOperation::ProjectVueMacroSemantics {
                owner_canonical: Arc::from(OWNER),
            },
            MISSING_PROOF_VUE_MACRO,
            "VueMacroSemanticInput",
        ),
        (
            NonFlowOperation::ResolveImportedComponentSurface {
                owner_canonical: Arc::from(OWNER),
                type_reference: Arc::from("BadgeProps"),
                referenced_canonical: Some(Arc::from("/src/Badge.vue")),
            },
            MISSING_PROOF_IMPORTED_COMPONENT,
            "ImportedComponentSurface",
        ),
        (
            NonFlowOperation::ProjectRuntimeProps {
                owner_canonical: Arc::from(OWNER),
                macro_index: 0,
            },
            MISSING_PROOF_PROPS,
            "RuntimePropsProjection",
        ),
        (
            NonFlowOperation::ProjectRuntimeEmits {
                owner_canonical: Arc::from(OWNER),
                macro_index: 1,
            },
            MISSING_PROOF_EMITS,
            "RuntimeEmitsProjection",
        ),
        (
            NonFlowOperation::ProjectRuntimeModel {
                owner_canonical: Arc::from(OWNER),
                macro_index: 2,
            },
            MISSING_PROOF_MODEL,
            "RuntimeModelProjection",
        ),
        (
            NonFlowOperation::ProjectExposeSurface {
                owner_canonical: Arc::from(OWNER),
                macro_index: 3,
            },
            MISSING_PROOF_EXPOSE,
            "ExposeSurfaceProjection",
        ),
    ];
    let complete = core_of(complete_snapshot());
    let all_missing = core_of(Arc::new(NonFlowObservationSnapshot::new()));
    let mut completed_variants = Vec::new();
    let mut reported_proof_ids = Vec::new();
    for (operation, proof_id, expected_variant) in &table {
        let NonFlowOutcome::Complete(payload) = complete.attempt(operation) else {
            panic!(
                "{proof_id}: route removal — a complete snapshot must complete, \
                 not degrade through a swallowed arm"
            );
        };
        assert_eq!(
            payload_variant(&payload),
            *expected_variant,
            "{proof_id}: route removal — the arm must carry its own payload"
        );
        assert!(
            matches!(
                all_missing.attempt(operation),
                NonFlowOutcome::NeedInputs(_)
            ),
            "{proof_id}: the all-missing snapshot must stay NeedInputs"
        );
        assert_eq!(operation.missing_input_proof_id(), *proof_id);
        completed_variants.push(payload_variant(&payload));
        reported_proof_ids.push(operation.missing_input_proof_id());
    }
    completed_variants.sort_unstable();
    completed_variants.dedup();
    assert_eq!(
        completed_variants.len(),
        6,
        "each row dispatches its own payload variant"
    );
    reported_proof_ids.sort_unstable();
    reported_proof_ids.dedup();
    assert_eq!(
        reported_proof_ids.len(),
        6,
        "each row reports its own proof id"
    );
}

/// The inventory plan: lane policy, index derivations, authored order,
/// and surface dependency failures are the kernel's decisions.
#[test]
fn vue_macro_semantic_input_plan_is_kernel_owned() {
    let core = core_of(complete_snapshot());
    let NonFlowOutcome::Complete(NonFlowPayload::VueMacroSemanticInput(plan)) =
        core.attempt(&NonFlowOperation::ProjectVueMacroSemantics {
            owner_canonical: Arc::from(OWNER),
        })
    else {
        panic!("complete snapshot must complete");
    };
    assert_eq!(plan.owner_canonical(), OWNER);
    let demands = plan.demands();
    assert_eq!(demands.len(), 4, "skipped macros contribute no row");
    // defineProps
    assert_eq!(demands[0].kind(), AnalyzedMacroKind::DefineProps);
    assert_eq!(demands[0].lane(), MacroSemanticLane::CodegenPayload);
    assert_eq!(demands[0].macro_index(), 0);
    assert!(demands[0].has_type_argument());
    assert_eq!(demands[0].defaults_macro_index(), None);
    // defineEmits
    assert_eq!(demands[1].kind(), AnalyzedMacroKind::DefineEmits);
    // defineModel keeps its model name in the plan.
    assert_eq!(demands[2].model_name(), Some("title"));
    // runtime-object defineExpose: the expose lane, no type argument.
    assert_eq!(demands[3].lane(), MacroSemanticLane::ExposeRuntimeObject);
    assert!(!demands[3].has_type_argument());
    // Surface-tier (not member-tier) dependency failures ride the row.
    let failures = demands[0].surface_dependency_failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].type_name(), "BadgeProps");
    assert_eq!(failures[0].import_source(), "./Badge.vue");
    // Syntax indices are top-level authored positions, in order.
    let syntax_indices: Vec<u32> = demands.iter().map(|row| row.syntax_index()).collect();
    let mut sorted = syntax_indices.clone();
    sorted.sort();
    assert_eq!(syntax_indices, sorted, "authored macro order is preserved");
}

/// Props rows: public published members only, authored anchors, the
/// member-dependency tier, and the `withDefaults` association.
#[test]
fn props_projection_shapes_rows_over_the_observed_surface() {
    let core = core_of(complete_snapshot());
    let NonFlowOutcome::Complete(NonFlowPayload::RuntimePropsProjection(projection)) = core
        .attempt(&NonFlowOperation::ProjectRuntimeProps {
            owner_canonical: Arc::from(OWNER),
            macro_index: 0,
        })
    else {
        panic!("complete snapshot must complete");
    };
    assert_eq!(projection.macro_index(), 0);
    let rows = projection.rows();
    assert_eq!(
        rows.iter().map(|row| row.name()).collect::<Vec<_>>(),
        vec!["count", "label"],
        "private and unpublished members never project"
    );
    assert!(
        rows[0].member_dependency(),
        "Count is a member-tier dependency"
    );
    assert!(!rows[1].member_dependency());
    assert!(rows[0].optional() != rows[1].optional());
    assert!(matches!(rows[0].anchor(), MacroAnchor::Authored { .. }));
    assert_eq!(
        projection.defaults_association(),
        verter_macro_dto::PropsDefaultsAssociation::None
    );
}

/// Emits rows: deduplicated by name on first admission, authored order.
#[test]
fn emits_projection_dedupes_and_orders_by_authored_order() {
    let core = core_of(complete_snapshot());
    let NonFlowOutcome::Complete(NonFlowPayload::RuntimeEmitsProjection(projection)) = core
        .attempt(&NonFlowOperation::ProjectRuntimeEmits {
            owner_canonical: Arc::from(OWNER),
            macro_index: 1,
        })
    else {
        panic!("complete snapshot must complete");
    };
    let emits = projection.emits();
    // "saved" appears in both the call-signature names and the member
    // surface; it is admitted once. "deleted" (no authored field) carries
    // the macro-argument anchor.
    assert_eq!(
        emits
            .iter()
            .map(|row| row.name.as_str())
            .collect::<Vec<_>>(),
        vec!["saved", "deleted"]
    );
    assert!(matches!(emits[0].anchor, MacroAnchor::Authored { .. }));
    assert!(matches!(emits[1].anchor, MacroAnchor::MacroArgument { .. }));
}

/// Model shape: synthesized names, anchors and optionality from the macro
/// row; the classification is the observed input, not a kernel decision.
#[test]
fn model_projection_synthesizes_the_shape_from_the_macro_row() {
    let core = core_of(complete_snapshot());
    let NonFlowOutcome::Complete(NonFlowPayload::RuntimeModelProjection(projection)) = core
        .attempt(&NonFlowOperation::ProjectRuntimeModel {
            owner_canonical: Arc::from(OWNER),
            macro_index: 2,
        })
    else {
        panic!("complete snapshot must complete");
    };
    let shape = projection.shape();
    assert_eq!(shape.prop.name, "title");
    assert!(shape.prop.optional);
    assert_eq!(shape.update_event.name, "update:title");
    assert_eq!(shape.modifiers_prop.name, "titleModifiers");
    assert!(matches!(
        shape.prop.anchor,
        MacroAnchor::Synthesized {
            row: SynthesizedRowKind::ModelProp,
            ..
        }
    ));
    // Without the observed classification the same operation reports
    // NeedInputs with the model proof id.
    let mut partial = NonFlowObservationSnapshot::new();
    partial.stage_script_analysis(Arc::from(OWNER), Arc::new(fixture_analysis()));
    let partial_core = core_of(Arc::new(partial));
    let operation = NonFlowOperation::ProjectRuntimeModel {
        owner_canonical: Arc::from(OWNER),
        macro_index: 2,
    };
    assert!(matches!(
        partial_core.attempt(&operation),
        NonFlowOutcome::NeedInputs(_)
    ));
    assert_eq!(operation.missing_input_proof_id(), MISSING_PROOF_MODEL);
}

/// Expose rows: authored field order, position-keyed anchors.
#[test]
fn expose_projection_orders_rows_by_authored_field_position() {
    let core = core_of(complete_snapshot());
    let NonFlowOutcome::Complete(NonFlowPayload::ExposeSurfaceProjection(projection)) = core
        .attempt(&NonFlowOperation::ProjectExposeSurface {
            owner_canonical: Arc::from(OWNER),
            macro_index: 3,
        })
    else {
        panic!("complete snapshot must complete");
    };
    let rows = projection.rows();
    assert_eq!(
        rows.iter().map(|row| row.name()).collect::<Vec<_>>(),
        vec!["reset", "refresh"]
    );
    let MacroAnchor::Authored {
        member_ordinal: first,
        ..
    } = rows[0].anchor()
    else {
        panic!("authored field anchors by position");
    };
    let MacroAnchor::Authored {
        member_ordinal: second,
        ..
    } = rows[1].anchor()
    else {
        panic!("authored field anchors by position");
    };
    assert!(first.get() < second.get());
}

/// Imported-component surface: bare-name retention, cross-file specifier
/// resolution from staged observations, local references.
#[test]
fn imported_component_surface_resolves_through_staged_observations() {
    let core = core_of(complete_snapshot());
    let resolve = |reference: &str, canonical: Option<&str>| {
        let NonFlowOutcome::Complete(NonFlowPayload::ImportedComponentSurface(surface)) = core
            .attempt(&NonFlowOperation::ResolveImportedComponentSurface {
                owner_canonical: Arc::from(OWNER),
                type_reference: Arc::from(reference),
                referenced_canonical: canonical.map(Arc::from),
            })
        else {
            panic!("analysis present must complete");
        };
        surface
    };
    // Cross-file reference through the staged resolution.
    let cross = resolve("BadgeProps", Some("/src/Badge.vue"));
    assert!(!cross.bare_name_is_imported());
    assert_eq!(cross.import_specifier(), Some("./Badge.vue"));
    assert_eq!(
        cross.qualified_testing_name().as_deref(),
        Some("import(\"./Badge.vue\").BadgeProps")
    );
    // A local reference (no resolved canonical) never takes the
    // qualified form.
    let local = resolve("LocalProps", None);
    assert!(!local.bare_name_is_imported());
    assert_eq!(local.import_specifier(), None);
    assert_eq!(local.qualified_testing_name(), None);
}

/// The observation frontier is canonically ordered — the identity a
/// driver binds for continuation revalidation.
#[test]
fn observation_frontier_is_canonically_ordered() {
    let frontier = complete_snapshot().observation_frontier();
    let mut keys = frontier.clone();
    keys.sort();
    assert_eq!(frontier, keys);
    assert!(frontier.len() >= 5);
}
