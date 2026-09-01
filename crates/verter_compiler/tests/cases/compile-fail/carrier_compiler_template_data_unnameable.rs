use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
use verter_compiler::framework_common::{CarrierCompiler, FrameworkParseArtifact};
use verter_compiler::svelte::SvelteCarrierCompiler;

fn generic_carrier_compiler<C: CarrierCompiler>(
    compiler: &C,
    source: &str,
    artifact: &FrameworkParseArtifact,
) {
    let _ = compiler.template_data(source, artifact);
}

fn vue_carrier_compiler(source: &str, artifact: &FrameworkParseArtifact) {
    let _ = VueCarrierCompiler.template_data(source, artifact);
}

fn svelte_carrier_compiler(source: &str, artifact: &FrameworkParseArtifact) {
    let _ = SvelteCarrierCompiler.template_data(source, artifact);
}

fn main() {
    let _ = generic_carrier_compiler::<VueCarrierCompiler>;
    let _ = vue_carrier_compiler;
    let _ = svelte_carrier_compiler;
}
