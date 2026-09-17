//! Managed construction on the direct path: `enter_direct` cannot bind a
//! project identity. If a fourth project argument appeared, this fixture
//! would compile and the guard would fail.

use verter_compiler::compile_request::{
    CompileProduct, CompileRequest, FrameworkCompileRequest, RuntimeProductRequest,
    VueCompileRequest,
};
use verter_compiler::compile_transaction::{CompileAttempt, ProjectIdentity};

fn main() {
    let request = CompileRequest::new(
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
    .expect("request");
    let _ = CompileAttempt::enter_direct("", &request, "vue", ProjectIdentity([1; 16]));
}
