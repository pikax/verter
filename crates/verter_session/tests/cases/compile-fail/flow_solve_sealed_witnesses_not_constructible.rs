//! Compile-fail fixture: the discharge evidence, the convergence evidence,
//! and the sealed completion artifact of a flow solve are mintable ONLY by
//! the obligation runtime. Every field is private and no constructor exists
//! outside the runtime's discharge/seal methods, so an external caller can
//! never assemble arbitrary discharge evidence, author convergence state,
//! or forge a completion artifact. If any of these types' fields were ever
//! widened to `pub`, this fixture would COMPILE and trybuild would turn red.
#![allow(unreachable_code)]

use verter_session::for_tests::{
    DischargeEvidence, FlowConvergenceEvidence, SealedFlowCompletion,
};

fn main() {
    let _ = DischargeEvidence {
        input_basis: todo!(),
        result_contract: todo!(),
        dependencies: todo!(),
        suboperations: todo!(),
    };
    let _ = FlowConvergenceEvidence {
        policy: todo!(),
        iterations: 1,
        stable: true,
    };
    let _ = SealedFlowCompletion {
        basis: todo!(),
        value: todo!(),
        convergence: todo!(),
        proofs: todo!(),
    };
}
