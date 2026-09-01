//! The consume-once execution grant cannot be forged by struct literal:
//! its `admitted` field is private, so the only sources are the
//! admission carve and the crate-private mint.

use verter_compiler::compile_request::ProductKind;
use verter_compiler::framework_common::ProductExecutionGrant;

fn forge() -> ProductExecutionGrant {
    ProductExecutionGrant {
        admitted: ProductKind::IdeCompanion,
    }
}

fn main() {}
