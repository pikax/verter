//! Production `VueCarrierCompiler` has no registry bundle method.
//! Typed host/runtime backends own emission.

use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;

fn main() {
    VueCarrierCompiler.compile_bundle();
}
