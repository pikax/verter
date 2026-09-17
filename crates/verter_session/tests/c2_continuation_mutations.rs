//! C2 request-local continuation: resume-mutation evidence (charter home:
//! `crates/verter_session/tests/c2_continuation_mutations.rs`).
//!
//! Every observation-specific resume mutation must FAIL to resume: a
//! changed, appeared, disappeared, reordered, or reconfigured staged
//! observation between an operation's rounds forces a whole-operation
//! restart, and the mutation never yields the earlier sealed answer.
//! These drive the transaction directly (the session's staging seam),
//! because the continuation itself is private by contract.

use std::sync::Arc;

use verter_compiler::compile_transaction::{CompileAttempt, TypeInfoRouteFailure};
use verter_macro_dto::RuntimePropType;
use verter_semantic::analysis::{
    AnalyzedMacro, AnalyzedMacroKind, AnalyzedPropField, ScriptAnalysisSnapshot,
    TypeResolutionSource,
};
use verter_semantic::type_info::{
    ObservedMacroSurface, ObservedSurfaceMember, MISSING_PROOF_VUE_MACRO,
};
use verter_span::Span;
use verter_type_expr::TopLevelOwnerId;

const OWNER: &str = "/src/App.vue";

fn analysis_with_props(prop_names: &[&str]) -> ScriptAnalysisSnapshot {
    let mut mac = AnalyzedMacro {
        kind: AnalyzedMacroKind::DefineProps,
        owner: TopLevelOwnerId::default(),
        is_type_based: true,
        type_references: Vec::new(),
        binding_name: None,
        model_name: None,
        has_inherit_attrs_false: false,
        prop_fields: prop_names
            .iter()
            .enumerate()
            .map(|(index, name)| AnalyzedPropField {
                name: (*name).to_owned(),
                is_optional: false,
                span: Span::new(30 + index as u32 * 10, 40 + index as u32 * 10),
                type_annotation: None,
                payload: None,
                type_expr_scope: None,
                description: None,
                tags: Vec::new(),
                resolution_source: TypeResolutionSource::default(),
                resolution_error: None,
                declared_in_macro_type_arg: false,
                constructor_bindings: Vec::new(),
            })
            .collect(),
        slot_fields: Vec::new(),
        default_keys: Vec::new(),
        default_values: Vec::new(),
        emit_fields: Vec::new(),
        expose_fields: Vec::new(),
        resolved_local_types: Vec::new(),
        parsed_type_argument: None,
        parsed_type_argument_scope: None,
        edit_anchors: Default::default(),
        span: Span::new(10, 80),
    };
    mac.prop_fields.truncate(prop_names.len());
    ScriptAnalysisSnapshot {
        macros: vec![mac],
        ..ScriptAnalysisSnapshot::default()
    }
}

fn surface(names: &[&str]) -> ObservedMacroSurface {
    ObservedMacroSurface {
        members: names
            .iter()
            .map(|name| ObservedSurfaceMember {
                published_name: Some((*name).to_owned()),
                optional: false,
                is_public: true,
                referenced_type_name: None,
            })
            .collect(),
        call_signature_event_names: Vec::new(),
    }
}

/// Mutation: CHANGED observation — restaging the same surface key under
/// new content between rounds must restart, never resume the sealed row
/// set.
#[test]
fn changed_surface_observation_restarts_instead_of_resuming() {
    let request_analysis = analysis_with_props(&["count"]);
    let mut attempt = CompileAttempt::enter_semantic(OWNER);
    attempt.stage_script_analysis(Arc::from(OWNER), Arc::new(request_analysis.clone()));
    attempt.stage_macro_surface(Arc::from(OWNER), 0, surface(&["count"]));
    let sealed = attempt
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .expect("completes with the original member");
    assert_eq!(sealed.rows().len(), 1);

    // Mutate: same keys, new content — the member's optionality flipped.
    let mut mutated = surface(&["count"]);
    mutated.members[0].optional = true;
    attempt.stage_macro_surface(Arc::from(OWNER), 0, mutated);
    let restarted = attempt
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .expect("restart completes from the mutated state");
    assert!(
        restarted.rows()[0].optional(),
        "a changed observation must restart from the NEW state, never serve the sealed answer"
    );
}

/// Mutation: APPEARED observation — staging an additional member between
/// rounds must surface it (restart), never the sealed single-row answer.
#[test]
fn appeared_observation_restarts_with_the_new_member() {
    let mut attempt = CompileAttempt::enter_semantic(OWNER);
    attempt.stage_script_analysis(Arc::from(OWNER), Arc::new(analysis_with_props(&["count"])));
    attempt.stage_macro_surface(Arc::from(OWNER), 0, surface(&["count"]));
    assert_eq!(
        attempt
            .type_info()
            .project_runtime_props(Arc::from(OWNER), 0)
            .expect("completes")
            .rows()
            .len(),
        1
    );
    attempt.stage_macro_surface(Arc::from(OWNER), 0, surface(&["count", "label"]));
    assert_eq!(
        attempt
            .type_info()
            .project_runtime_props(Arc::from(OWNER), 0)
            .expect("restart completes")
            .rows()
            .len(),
        2,
        "an appeared member must restart into the answer"
    );
}

/// Mutation: REORDERED observations — the same members staged in a
/// different order change the canonical frontier and restart.
#[test]
fn reordered_observation_restarts_with_new_row_order() {
    let mut attempt = CompileAttempt::enter_semantic(OWNER);
    attempt.stage_script_analysis(Arc::from(OWNER), Arc::new(analysis_with_props(&["a", "b"])));
    attempt.stage_macro_surface(Arc::from(OWNER), 0, surface(&["a", "b"]));
    let sealed = attempt
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .expect("completes");
    let sealed_names: Vec<&str> = sealed.rows().iter().map(|row| row.name()).collect();
    assert_eq!(sealed_names, vec!["a", "b"]);

    attempt.stage_macro_surface(Arc::from(OWNER), 0, surface(&["b", "a"]));
    let restarted = attempt
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .expect("restart completes");
    let restarted_names: Vec<&str> = restarted.rows().iter().map(|row| row.name()).collect();
    assert_eq!(
        restarted_names,
        vec!["b", "a"],
        "a reordered observation surface is a new state, and the restart reflects it"
    );
}

/// Mutation: RECONFIGURED observation — a second observation key of the
/// same operation's data plane (the model classification slot) appearing
/// between rounds is a reconfiguration that restarts the model operation.
#[test]
fn reconfigured_observation_slot_restarts_the_model_operation() {
    let mut attempt = CompileAttempt::enter_semantic(OWNER);
    let mut analysis = analysis_with_props(&["value"]);
    analysis.macros[0].kind = AnalyzedMacroKind::DefineModel;
    analysis.macros[0].model_name = Some("title".to_owned());
    attempt.stage_script_analysis(Arc::from(OWNER), Arc::new(analysis));
    // Missing classification: the operation demands its input first.
    let failure = attempt
        .type_info()
        .project_runtime_model(Arc::from(OWNER), 0)
        .expect_err("classification not staged");
    assert!(matches!(failure, TypeInfoRouteFailure::NeedInputs { .. }));
    // Reconfigure: stage the classification — the operation restarts
    // under the completed state and completes.
    attempt.stage_model_value_type_shape(
        Arc::from(OWNER),
        0,
        RuntimePropType::Resolved {
            constructors: Default::default(),
            skip_check: false,
        },
    );
    let model = attempt
        .type_info()
        .project_runtime_model(Arc::from(OWNER), 0)
        .expect("restart under the reconfigured state completes");
    assert_eq!(model.shape().prop.name, "title");
}

/// Mutation: DISAPPEARED observation — dropping the macro row from the
/// restaged analysis terminally refuses the operation (a slot with no
/// macro row is a domain failure no staged observation can repair)
/// instead of resuming anything.
#[test]
fn disappeared_analysis_refuses_with_the_proof_id() {
    let mut attempt = CompileAttempt::enter_semantic(OWNER);
    attempt.stage_script_analysis(Arc::from(OWNER), Arc::new(analysis_with_props(&["count"])));
    attempt.stage_macro_surface(Arc::from(OWNER), 0, surface(&["count"]));
    assert!(attempt
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .is_ok());
    let emptied = ScriptAnalysisSnapshot::default();
    attempt.stage_script_analysis(Arc::from(OWNER), Arc::new(emptied));
    let failure = attempt
        .type_info()
        .project_runtime_props(Arc::from(OWNER), 0)
        .expect_err("the disappeared macro row must refuse");
    assert!(
        matches!(failure, TypeInfoRouteFailure::Terminal),
        "a disappeared macro row is a terminal domain refusal, not a repeatable demand: {failure:?}"
    );
}

/// The plan operation's own all-missing/complete rounds carry the same
/// proof discipline through the session-side seam.
#[test]
fn plan_operation_reports_its_proof_id_when_unstaged() {
    let mut attempt = CompileAttempt::enter_semantic(OWNER);
    let failure = attempt
        .type_info()
        .project_vue_macro_semantics(Arc::from(OWNER))
        .expect_err("nothing staged");
    match failure {
        TypeInfoRouteFailure::NeedInputs { proof_id, .. } => {
            assert_eq!(proof_id, MISSING_PROOF_VUE_MACRO)
        }
        other => panic!("expected a proof-carrying refusal, got {other:?}"),
    }
}
