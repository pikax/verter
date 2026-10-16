//! Compile-fail: a production build can neither compose a host over the
//! counted ownership regime nor name the regime that would select it.

use verter_session::{HostConfig, VerterHost};
use verter_type_engine::semantic_query_memo::GraphOwnership;

fn main() {
    let _ = VerterHost::new_standalone_with_counted_ownership_for_test(HostConfig::default());
    let _ = GraphOwnership::CountedForTests;
}
