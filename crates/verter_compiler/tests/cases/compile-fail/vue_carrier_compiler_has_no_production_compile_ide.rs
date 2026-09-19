//! Production `VueCarrierCompiler` has no registry IDE method.
//! Typed projection owns IDE emission.

use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;

fn main() {
    VueCarrierCompiler.compile_ide();
}
