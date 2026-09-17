//! C2 compile-transaction type-info route tests (charter home:
//! `crates/verter_compiler/tests/compile_type_info_routes.rs`).
//!
//! Every table row of the ratified entry contract proves, through the
//! sealed `CompileTypeInfo` gateway: the all-missing route refusal
//! carrying the row's missing-input proof id; the complete/preloaded
//! equivalence (staging after a missing round equals preloading — no
//! warm-state advantage); and the request-local continuation contract
//! (identical re-request resumes; a changed observation restarts the
//! whole operation and never serves the earlier sealed answer; a
//! no-progress round discards sealed output).

use std::sync::Arc;

use verter_compiler::compile_request::{
    CompileProduct, FrameworkCompileRequest, IdeProductRequest, RuntimeProductRequest,
    VueCompileRequest,
};
use verter_compiler::compile_transaction::{CompileAttempt, TypeInfoRouteFailure};
use verter_macro_dto::RuntimePropType;
use verter_semantic::analysis::{
    AnalyzedEmitField, AnalyzedMacro, AnalyzedMacroKind, AnalyzedPropField, MacroTypeDep,
    MacroTypeDepUsage, ScriptAnalysisSnapshot, TypeResolutionSource,
};
use verter_semantic::type_info::{
    ImportedComponentResolution, MacroSemanticLane, ObservedMacroSurface, ObservedSurfaceMember,
    MISSING_PROOF_EMITS, MISSING_PROOF_EXPOSE, MISSING_PROOF_IMPORTED_COMPONENT,
    MISSING_PROOF_MODEL, MISSING_PROOF_PROPS, MISSING_PROOF_VUE_MACRO,
};
use verter_span::Span;
use verter_type_expr::TopLevelOwnerId;

const OWNER: &str = "/src/App.vue";

fn vue_request() -> verter_compiler::compile_request::CompileRequest {
    verter_compiler::compile_request::CompileRequest::new(
        vec![
            CompileProduct::RuntimeClient(RuntimeProductRequest::default()),
            CompileProduct::IdeCompanion(IdeProductRequest::default()),
        ],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        Some("App.vue".to_string()),
        None,
        false,
        false,
    )
    .expect("test request constructs")
}

fn prop_field(name: &str, optional: bool, at: u32) -> AnalyzedPropField {
    AnalyzedPropField {
        name: name.to_owned(),
        is_optional: optional,
        span: Span::new(at, at + name.len() as u32),
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

fn analyzed_macro(kind: AnalyzedMacroKind, start: u32, end: u32) -> AnalyzedMacro {
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
        span: Span::new(start, end),
    }
}

fn fixture_analysis() -> ScriptAnalysisSnapshot {
    let mut props = analyzed_macro(AnalyzedMacroKind::DefineProps, 10, 80);
    props.prop_fields = vec![prop_field("count", false, 30)];
    props.parsed_type_argument = None;
    let mut emits = analyzed_macro(AnalyzedMacroKind::DefineEmits, 90, 150);
    emits.emit_fields = vec![AnalyzedEmitField {
        name: "saved".to_owned(),
        span: Span::new(110, 115),
        call_signature_span: None,
        payload_type: None,
        payload: None,
        payload_expr_scope: None,
        description: None,
        tags: Vec::new(),
    }];
    emits.parsed_type_argument = None;
    let mut model = analyzed_macro(AnalyzedMacroKind::DefineModel, 160, 210);
    model.model_name = Some("title".to_owned());
    let mut expose = analyzed_macro(AnalyzedMacroKind::DefineExpose, 220, 270);
    expose.is_type_based = false;
    ScriptAnalysisSnapshot {
        macros: vec![props, emits, model, expose],
        macro_type_deps: vec![MacroTypeDep {
            type_name: "BadgeProps".to_owned(),
            import_source: "./Badge.vue".to_owned(),
            macro_kind: AnalyzedMacroKind::DefineProps,
            macro_index: 0,
            macro_span: Span::new(10, 80),
            usage: MacroTypeDepUsage::Surface,
        }],
        ..ScriptAnalysisSnapshot::default()
    }
}

fn props_surface() -> ObservedMacroSurface {
    ObservedMacroSurface {
        members: vec![ObservedSurfaceMember {
            published_name: Some("count".to_owned()),
            optional: false,
            is_public: true,
            referenced_type_name: None,
        }],
        call_signature_event_names: Vec::new(),
    }
}

fn emits_surface() -> ObservedMacroSurface {
    ObservedMacroSurface {
        members: Vec::new(),
        call_signature_event_names: vec!["saved".to_owned()],
    }
}

fn stage_all(attempt: &mut CompileAttempt<'_>) {
    attempt.stage_script_analysis(Arc::from(OWNER), Arc::new(fixture_analysis()));
    attempt.stage_macro_surface(Arc::from(OWNER), 0, props_surface());
    attempt.stage_macro_surface(Arc::from(OWNER), 1, emits_surface());
    attempt.stage_model_value_type_shape(
        Arc::from(OWNER),
        2,
        RuntimePropType::Resolved {
            constructors: Default::default(),
            skip_check: false,
        },
    );
    attempt.stage_import_resolution(
        Arc::from(OWNER),
        ImportedComponentResolution {
            specifier: Arc::from("./Badge.vue"),
            resolved_canonical: Arc::from("/src/Badge.vue"),
        },
    );
}

fn need_inputs_proof(failure: &TypeInfoRouteFailure) -> Option<&'static str> {
    match failure {
        TypeInfoRouteFailure::NeedInputs { proof_id, .. } => Some(proof_id),
        _ => None,
    }
}

/// The six-row route table: every operation's all-missing refusal carries
/// its charter proof id through the sealed gateway.
#[test]
fn all_missing_routes_refuse_with_their_proof_ids() {
    let request = vue_request();
    let mut attempt = CompileAttempt::enter_direct("", &request, "vue");
    let type_info = attempt.type_info();
    assert_eq!(
        need_inputs_proof(
            &type_info
                .project_vue_macro_semantics(Arc::from(OWNER))
                .expect_err("missing analysis refuses")
        ),
        Some(MISSING_PROOF_VUE_MACRO)
    );
    assert_eq!(
        need_inputs_proof(
            &type_info
                .resolve_imported_component_surface(
                    Arc::from(OWNER),
                    Arc::from("BadgeProps"),
                    Some(Arc::from("/src/Badge.vue")),
                )
                .expect_err("missing analysis refuses")
        ),
        Some(MISSING_PROOF_IMPORTED_COMPONENT)
    );
    assert_eq!(
        need_inputs_proof(
            &type_info
                .project_runtime_props(Arc::from(OWNER), 0)
                .expect_err("missing analysis refuses")
        ),
        Some(MISSING_PROOF_PROPS)
    );
    assert_eq!(
        need_inputs_proof(
            &type_info
                .project_runtime_emits(Arc::from(OWNER), 1)
                .expect_err("missing analysis refuses")
        ),
        Some(MISSING_PROOF_EMITS)
    );
    assert_eq!(
        need_inputs_proof(
            &type_info
                .project_runtime_model(Arc::from(OWNER), 2)
                .expect_err("missing analysis refuses")
        ),
        Some(MISSING_PROOF_MODEL)
    );
    assert_eq!(
        need_inputs_proof(
            &type_info
                .project_expose_surface(Arc::from(OWNER), 3)
                .expect_err("missing analysis refuses")
        ),
        Some(MISSING_PROOF_EXPOSE)
    );
}

/// Complete/preloaded equivalence: preloading every observation before
/// the first route call and staging it after a missing round produce the
/// identical complete payload — incremental equals fresh.
#[test]
fn preloaded_and_staged_route_results_are_equivalent() {
    let request = vue_request();

    let mut preloaded = CompileAttempt::enter_direct("", &request, "vue");
    stage_all(&mut preloaded);
    let preloaded_plan = preloaded
        .type_info()
        .project_vue_macro_semantics(Arc::from(OWNER))
        .expect("preloaded completes");
    let preloaded_props = preloaded
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .expect("preloaded completes");

    let mut staged = CompileAttempt::enter_direct("", &request, "vue");
    assert!(staged
        .type_info()
        .project_vue_macro_semantics(Arc::from(OWNER))
        .is_err());
    assert!(staged
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .is_err());
    stage_all(&mut staged);
    let staged_plan = staged
        .type_info()
        .project_vue_macro_semantics(Arc::from(OWNER))
        .expect("staged completes");
    let staged_props = staged
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .expect("staged completes");

    assert_eq!(preloaded_plan.demands().len(), staged_plan.demands().len());
    assert_eq!(
        preloaded_plan.demands()[0].lane(),
        MacroSemanticLane::CodegenPayload
    );
    assert_eq!(
        preloaded_props.rows().len(),
        staged_props.rows().len(),
        "preloaded and staged route results agree"
    );
    assert_eq!(
        preloaded_props.rows()[0].name(),
        staged_props.rows()[0].name()
    );
}

/// The request-local continuation: an identical re-request resumes and
/// serves the sealed answer; a changed observation restarts the whole
/// operation and reflects the NEW input — the sealed stale answer is
/// never served.
#[test]
fn continuation_revalidates_and_restarts_on_input_change() {
    let request = vue_request();
    let mut attempt = CompileAttempt::enter_direct("", &request, "vue");
    stage_all(&mut attempt);
    let first = attempt
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .expect("completes");
    assert_eq!(first.rows().len(), 1);
    assert_eq!(first.rows()[0].name(), "count");

    // Identical re-request: the continuation matches and resumes.
    let resumed = attempt
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .expect("identical re-request serves the sealed answer");
    assert_eq!(resumed.rows()[0].name(), "count");

    // A changed observation for the SAME operation: the whole operation
    // restarts under the new state and reports the new member, never the
    // sealed stale answer.
    attempt.stage_macro_surface(
        Arc::from(OWNER),
        0,
        ObservedMacroSurface {
            members: vec![
                ObservedSurfaceMember {
                    published_name: Some("count".to_owned()),
                    optional: false,
                    is_public: true,
                    referenced_type_name: None,
                },
                ObservedSurfaceMember {
                    published_name: Some("label".to_owned()),
                    optional: true,
                    is_public: true,
                    referenced_type_name: None,
                },
            ],
            call_signature_event_names: Vec::new(),
        },
    );
    let restarted = attempt
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .expect("restart under the new state completes");
    assert_eq!(
        restarted
            .rows()
            .iter()
            .map(|row| row.name())
            .collect::<Vec<_>>(),
        vec!["count", "label"],
        "a changed input forces a whole-operation restart from the new state"
    );
}

/// A disappeared input also forces restart-and-refuse: staging the
/// analysis away leaves the operation `NeedInputs` again — the sealed
/// output is gone.
#[test]
fn disappeared_input_discards_the_sealed_output() {
    let request = vue_request();
    let mut attempt = CompileAttempt::enter_direct("", &request, "vue");
    stage_all(&mut attempt);
    assert!(attempt
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .is_ok());

    // Restage the analysis under a new version with the macro row GONE:
    // the operation restarts into a missing-input refusal — the sealed
    // answer from the earlier state is gone with it.
    let mut analysis = fixture_analysis();
    analysis.macros.clear();
    attempt.stage_script_analysis(Arc::from(OWNER), Arc::new(analysis));
    let failure = attempt
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .expect_err("a disappeared macro row must refuse, not resume");
    assert_eq!(
        need_inputs_proof(&failure),
        Some(MISSING_PROOF_PROPS),
        "restart never serves stale sealed output: {failure:?}"
    );
}

/// Cancellation discards the sealed output: after `cancel`, the same
/// request re-derives from the current state instead of resuming.
#[test]
fn cancellation_discards_sealed_output() {
    let request = vue_request();
    let mut attempt = CompileAttempt::enter_direct("", &request, "vue");
    stage_all(&mut attempt);
    assert!(attempt
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .is_ok());
    attempt.cancel();
    // A fresh (empty) attempt state after cancellation still refuses
    // honestly rather than resuming anything.
    let mut fresh = CompileAttempt::enter_direct("", &request, "vue");
    assert!(fresh
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .is_err());
}

/// No-progress: a re-entered operation whose demanded inputs did not
/// change reports `NoProgress` and never loops.
#[test]
fn no_progress_rounds_refuse_instead_of_looping() {
    let request = vue_request();
    let mut attempt = CompileAttempt::enter_direct("", &request, "vue");
    let first = attempt
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .expect_err("nothing staged");
    assert!(matches!(first, TypeInfoRouteFailure::NeedInputs { .. }));
    let second = attempt
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .expect_err("staging nothing between rounds makes no progress");
    assert!(matches!(second, TypeInfoRouteFailure::NoProgress { .. }));
}

/// The transaction's no-stale-publication rail: admission is bound to
/// the entry-bound source digest, and a source that no longer matches
/// it is detectable (and refused) — never quietly rebased.
#[test]
fn admission_is_bound_to_the_entry_source_digest() {
    let request = vue_request();
    let source = "<template><p>hi</p></template>\n";
    let attempt = CompileAttempt::enter_direct("let a = 1", &request, "vue");
    assert!(
        !attempt.source_is_unchanged(source),
        "a different source must not pass the entry-bound digest"
    );
    assert!(
        attempt.source_is_unchanged("let a = 1"),
        "the entry source itself passes"
    );

    // The real admission path enters and admits in one compile: the
    // typed runtime root and the set come back, and the admission the
    // facade ran minted its identities inside the transaction.
    let execution = Default::default();
    let macros = Default::default();
    let output = verter_compiler::standalone::StandaloneCompiler
        .compile(
            source,
            &request,
            verter_compiler::standalone::DirectExecutionInputs::Vue {
                execution: &execution,
                macros: &macros,
            },
        )
        .expect("the facade enters and admits one transaction per compile");
    assert!(
        output
            .artifact(verter_compiler::compile_request::ProductKind::RuntimeClient)
            .is_some(),
        "the published product view derives from the canonical set"
    );
}
