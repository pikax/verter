//! C4 public-API identity contract: prepared-batch (direct) and
//! project-aware entry bind exact `InputBasisId`s on one sealed facade.
//! Distinct projects cannot alias; unbound project-aware equals direct.

use verter_compiler::compile_request::{
    CompileProduct, FrameworkCompileRequest, RuntimeProductRequest, VueCompileRequest,
};
use verter_compiler::compile_transaction::{CompileAttempt, ProjectIdentity};

fn vue_request() -> verter_compiler::compile_request::CompileRequest {
    verter_compiler::compile_request::CompileRequest::new(
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

#[test]
fn direct_and_unbound_project_entries_share_input_basis() {
    let request = vue_request();
    let source = "<script setup>const n = 1</script><template>{{ n }}</template>";
    let direct = CompileAttempt::enter_direct(source, &request, "vue");
    let unbound =
        CompileAttempt::enter_project(source, &request, "vue", CompileAttempt::UNBOUND_PROJECT);
    assert_eq!(direct.input_basis(), unbound.input_basis());
}

#[test]
fn project_identity_is_an_input_basis_axis() {
    let request = vue_request();
    let source = "<script setup>const n = 1</script><template>{{ n }}</template>";
    let a = CompileAttempt::enter_project(source, &request, "vue", ProjectIdentity([1; 16]));
    let b = CompileAttempt::enter_project(source, &request, "vue", ProjectIdentity([2; 16]));
    assert_ne!(a.input_basis(), b.input_basis());
}
