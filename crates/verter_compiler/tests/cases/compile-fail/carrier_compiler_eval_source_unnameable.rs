use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
use verter_compiler::framework_common::{CarrierCompiler, FrameworkParseArtifact};
use verter_compiler::svelte::SvelteCarrierCompiler;

fn generic_carrier_compiler<C: CarrierCompiler>(
    compiler: &C,
    source: &str,
    artifact: &FrameworkParseArtifact,
) {
    let _ = compiler.eval_source(source, artifact);
}

fn vue_carrier_compiler(source: &str, artifact: &FrameworkParseArtifact) {
    let _ = VueCarrierCompiler.eval_source(source, artifact);
}

fn svelte_carrier_compiler(source: &str, artifact: &FrameworkParseArtifact) {
    let _ = SvelteCarrierCompiler.eval_source(source, artifact);
}

fn main() {
    let _ = generic_carrier_compiler::<VueCarrierCompiler>;
    let _ = vue_carrier_compiler;
    let _ = svelte_carrier_compiler;
}
