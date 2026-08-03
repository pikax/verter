//! Inverse compile-fail fixture for the raw registered-carrier parser seal.
//!
//! This compiles while the raw carrier parser is publicly nameable. Sealing the
//! parser behind the registered producer turns it into a genuine compile
//! failure.

use verter_compiler::compile::compile;

fn main() {
    let _raw_registered_carrier_parser = compile;
}
