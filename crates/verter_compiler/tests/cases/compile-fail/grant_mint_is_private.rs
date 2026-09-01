//! The consume-once execution grant's mint is crate-private: an external
//! caller cannot mint one directly — grant-minting authority never
//! leaves the crate.

use verter_compiler::compile_request::ProductKind;
use verter_compiler::framework_common::ProductExecutionGrant;

fn mint() -> ProductExecutionGrant {
    ProductExecutionGrant::mint(ProductKind::IdeCompanion)
}

fn main() {}
