//! A product backend cannot be driven without admission evidence: the
//! consume-once execution grant has a private field and a crate-private
//! mint, so an external caller can neither forge one by struct literal
//! nor mint one directly — the only out-of-crate sources are the
//! admission carves on the host-integration backends.

use verter_compiler::compile_request::ProductKind;
use verter_compiler::framework_common::ProductExecutionGrant;

fn forge() -> ProductExecutionGrant {
    ProductExecutionGrant {
        admitted: ProductKind::IdeCompanion,
    }
}

fn mint() -> ProductExecutionGrant {
    ProductExecutionGrant::mint(ProductKind::IdeCompanion)
}

fn main() {}
