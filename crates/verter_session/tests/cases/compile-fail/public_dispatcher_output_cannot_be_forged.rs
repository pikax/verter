//! Compile-fail: only the dispatcher mints a query output, together with
//! the delivery that owns its value; a consumer cannot assemble one around a
//! delivery of its own choosing.

use verter_type_engine::semantic_query::{ResultProvenance, SemanticNodeId, SemanticQueryOutput};
use verter_type_engine::semantic_query_memo::delivery_lease::SemanticDeliveryLease;

fn forge(value: SemanticNodeId) -> SemanticQueryOutput<SemanticNodeId> {
    SemanticQueryOutput {
        value,
        provenance: ResultProvenance::clean(),
        delivery: SemanticDeliveryLease::default(),
    }
}

fn main() {}
