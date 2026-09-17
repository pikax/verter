//! The Vue runtime macro projection has ONE carrier into compile: the
//! staged handoff on the sealed `CompileAttempt` transaction. If a raw
//! bundle field reappeared on `VueExecutionInputs`, a second semantic
//! input route would exist beside the transaction — this line would
//! compile and the guard would fail.

use std::sync::Arc;

use verter_compiler::compile::types::VueExecutionInputs;
use verter_macro_dto::MacroRuntimeBundle;

fn main() {
    let _inputs = VueExecutionInputs {
        macro_runtime: Some(Arc::new(MacroRuntimeBundle { entries: Vec::new() })),
        ..VueExecutionInputs::default()
    };
}
