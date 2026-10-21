//! A held answer's allocation charge witness is optional soak/test state,
//! absent from every build that enables neither `test-support` nor
//! `soak-support`: an ordinary consumer cannot observe an answer's charges.

use verter_type_engine::semantic_query::FlowReturnResult;
use verter_type_engine::semantic_query_memo::charge_witness::HeldChargeWitness;

fn observe(result: &FlowReturnResult) -> bool {
    HeldChargeWitness::of_flow_return(result).is_exact()
}

fn main() {}
