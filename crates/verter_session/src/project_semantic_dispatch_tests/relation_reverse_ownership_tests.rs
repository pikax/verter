//! Reverse-mapped inference is owned by the relation session: the engine's
//! ownership check, run in this crate's lib suite where its guard is bound.

use verter_type_engine::project_semantic_dispatch::relation::session_cache_tests::{
    session_visible_answers_do_not_warm_pure_relations, SessionVerdictArm,
};

#[test]
pub(crate) fn reverse_mapped_inference_is_relation_owned_in_session() {
    verter_type_engine::project_semantic_dispatch::relation::reverse_ownership_tests::reverse_mapped_inference_is_relation_owned_in_session();
}

#[test]
fn independent_pairs_keep_reverse_mapping_visible() {
    let host = crate::VerterHost::new_standalone(Default::default());
    let dispatch = super::ProjectSemanticDispatch::new(&host);
    let _construction = dispatch.enter_construction_scope();
    verter_type_engine::project_semantic_dispatch::relation::reverse_ownership_tests::independent_pairs_keep_reverse_mapping_visible(&dispatch);
}

#[test]
fn session_visible_no_infer_answers_do_not_warm_pure_relations() {
    let host = crate::VerterHost::new_standalone(Default::default());
    let dispatch = super::ProjectSemanticDispatch::new(&host);
    let _construction = dispatch.enter_construction_scope();
    for arm in [
        SessionVerdictArm::NoInferTarget,
        SessionVerdictArm::ArrayNoInferTarget,
    ] {
        session_visible_answers_do_not_warm_pure_relations(&dispatch, arm);
    }
}

#[test]
fn session_visible_null_unions_do_not_warm_pure_relations() {
    let host = crate::VerterHost::new_standalone(Default::default());
    let dispatch = super::ProjectSemanticDispatch::new(&host);
    let _construction = dispatch.enter_construction_scope();
    session_visible_answers_do_not_warm_pure_relations(
        &dispatch,
        SessionVerdictArm::LooseNullUnion,
    );
}

#[test]
fn session_visible_indexed_access_keeps_the_pure_check_cold() {
    let host = crate::VerterHost::new_standalone(Default::default());
    let dispatch = super::ProjectSemanticDispatch::new(&host);
    let _construction = dispatch.enter_construction_scope();
    session_visible_answers_do_not_warm_pure_relations(
        &dispatch,
        SessionVerdictArm::RigidIndexedAccess,
    );
}

#[test]
fn withheld_call_arguments_do_not_warm_pure_relations() {
    let host = crate::VerterHost::new_standalone(Default::default());
    let dispatch = super::ProjectSemanticDispatch::new(&host);
    let _construction = dispatch.enter_construction_scope();
    session_visible_answers_do_not_warm_pure_relations(
        &dispatch,
        SessionVerdictArm::WithheldCallArgument,
    );
}
