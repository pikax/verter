//! C4: direct prepared-batch and project-aware entry share one sealed
//! transaction; project facts enter identity; cancelled admission fails closed.

use super::*;
use crate::compile_request::{
    CompileProduct, FrameworkCompileRequest, RuntimeProductRequest, VueCompileRequest,
};

fn vue_request() -> CompileRequest {
    CompileRequest::new(
        vec![CompileProduct::RuntimeClient(
            RuntimeProductRequest::default(),
        )],
        FrameworkCompileRequest::Vue(VueCompileRequest::default()),
        None,
        Some("Comp.vue".to_string()),
        None,
        false,
        false,
    )
    .expect("test request constructs")
}

fn project(byte: u8) -> ProjectIdentity {
    ProjectIdentity([byte; 16])
}

fn published(request: &CompileRequest, code: &str) -> crate::assembly::publish::ArtifactSet {
    use crate::assembly::fragment::FragmentDialect;
    use crate::assembly::publish::{publish, ArtifactContribution};
    use crate::assembly::ProductPlan;
    use crate::compile_request::ProductKind;
    let plan = ProductPlan::from_request(request);
    publish(
        &plan,
        vec![ArtifactContribution {
            kind: ProductKind::RuntimeClient,
            fragments: Vec::new(),
            code: code.to_owned(),
            emitted_imports: Vec::new(),
            dialect: FragmentDialect::Tsx,
            source_projection_map: None,
            runtime_source_map: None,
        }],
    )
    .expect("synthetic publication")
}

#[test]
fn enter_direct_and_enter_project_unbound_share_input_basis() {
    let request = vue_request();
    let source = "<template><p>hi</p></template>\n";
    let direct = CompileAttempt::enter_direct(source, &request, "vue");
    let project_unbound =
        CompileAttempt::enter_project(source, &request, "vue", CompileAttempt::UNBOUND_PROJECT);
    assert_eq!(
        direct.input_basis(),
        project_unbound.input_basis(),
        "unbound project-aware entry is the direct entry"
    );
    assert_eq!(direct.project_identity(), CompileAttempt::UNBOUND_PROJECT);
}

#[test]
fn distinct_projects_mint_distinct_input_bases_and_source_ids() {
    let request = vue_request();
    let source = "<template><p>hi</p></template>\n";
    let a = CompileAttempt::enter_project(source, &request, "vue", project(1));
    let b = CompileAttempt::enter_project(source, &request, "vue", project(2));
    assert_ne!(
        a.input_basis(),
        b.input_basis(),
        "project identity is an input-basis axis"
    );
    assert_ne!(a.project_identity(), b.project_identity());
    let id_a = mint_direct_source_id("Comp.vue", "vue", project(1));
    let id_b = mint_direct_source_id("Comp.vue", "vue", project(2));
    assert_ne!(id_a, id_b, "project identity is a source-identity axis");
}

#[test]
fn equivalent_products_stay_byte_identical_across_projects_while_provenance_names_the_project() {
    let request = vue_request();
    let source = "<template><p>hi</p></template>\n";
    let code = "export const n = 1;\n";
    let set = published(&request, code);
    let first = CompileAttempt::enter_project(source, &request, "vue", project(1))
        .admit_published_products(source, &set, Vec::new(), None, Vec::new())
        .expect("project A admits");
    let second = CompileAttempt::enter_project(source, &request, "vue", project(2))
        .admit_published_products(source, &set, Vec::new(), None, Vec::new())
        .expect("project B admits");
    let first_artifact = &first.set().artifacts()[0];
    let second_artifact = &second.set().artifacts()[0];
    assert_eq!(
        first_artifact.content, second_artifact.content,
        "equivalent products stay byte-identical across projects"
    );
    assert_ne!(
        first_artifact.provenance.input_basis, second_artifact.provenance.input_basis,
        "artifact provenance names the project-bound input basis"
    );
}

#[test]
fn cancelled_transaction_cannot_admit_a_complete_result() {
    let request = vue_request();
    let source = "<template><p>hi</p></template>\n";
    let mut attempt = CompileAttempt::enter_direct(source, &request, "vue");
    attempt.cancel();
    let err = attempt
        .admit_published_products(
            source,
            &published(&request, "export const n = 1;\n"),
            Vec::new(),
            None,
            Vec::new(),
        )
        .expect_err("cancelled admission must refuse");
    assert_eq!(err, CompileTransactionRefusal::Cancelled);
}

#[test]
fn enter_semantic_for_project_binds_project_into_the_same_facade() {
    let unbound = CompileAttempt::enter_semantic("Owner.vue");
    let project_bound = CompileAttempt::enter_semantic_for_project("Owner.vue", project(7));
    assert_eq!(unbound.project_identity(), CompileAttempt::UNBOUND_PROJECT);
    assert_eq!(project_bound.project_identity(), project(7));
    assert_ne!(unbound.input_basis(), project_bound.input_basis());
}
