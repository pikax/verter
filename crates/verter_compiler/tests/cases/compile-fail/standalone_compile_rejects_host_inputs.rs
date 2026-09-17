//! Managed construction on the direct path: `StandaloneCompiler::compile`
//! cannot take host/managed execution inputs.

use verter_compiler::compile_request::{
    CompileProduct, FrameworkCompileRequest, RuntimeProductRequest, VueCompileRequest,
};
use verter_compiler::framework_common::VueHostExecutionInputs;
use verter_compiler::standalone::StandaloneCompiler;

fn main() {
    let request = verter_compiler::compile_request::CompileRequest::new(
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
    let _ = StandaloneCompiler.compile("", &request, VueHostExecutionInputs::default());
}
