//! Compile-fail fixture: the completeness-proof carrier of a flow solve is
//! mintable ONLY by the proof finalizer. Its sole constructor is private to
//! the finalizer's module, so an external caller cannot invoke it — a proof
//! can never be forged outside the finalizer. If the constructor were
//! widened (`pub` or `pub(crate)` with a test-support re-export of it), this
//! fixture would COMPILE and trybuild would turn red.

use verter_session::for_tests::CompleteFlowResult;

fn main() {
    let _ = CompleteFlowResult::new(todo!());
}
