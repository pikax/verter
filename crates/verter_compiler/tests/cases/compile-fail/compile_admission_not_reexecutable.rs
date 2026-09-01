//! One issued compile admission drives at most ONE execution: the host
//! backends' execution entries take the admission by VALUE, so a second
//! execution of the same issuance is a move-of-moved-value compile error
//! on both the multi-product and render entries, for both frameworks.

use verter_compiler::framework_common::{
    FrameworkParseArtifact, SvelteCompileAdmission, SvelteHostExecutionInputs,
    SvelteHostIntegrationBackend, VueCompileAdmission, VueHostExecutionInputs,
    VueHostIntegrationBackend,
};

fn vue_double_execute(
    backend: &VueHostIntegrationBackend,
    admission: VueCompileAdmission,
    artifact: &FrameworkParseArtifact,
    inputs: &VueHostExecutionInputs,
    alloc: &oxc_allocator::Allocator,
) {
    let _ = backend.compile_host_products(admission, artifact, inputs, alloc);
    let _ = backend.compile_host_products(admission, artifact, inputs, alloc);
}

fn svelte_double_render(
    backend: &SvelteHostIntegrationBackend,
    admission: SvelteCompileAdmission,
    artifact: &FrameworkParseArtifact,
    inputs: &SvelteHostExecutionInputs,
    alloc: &oxc_allocator::Allocator,
) {
    let _ = backend.compile_runtime_render(admission, artifact, inputs, alloc);
    let _ = backend.compile_runtime_render(admission, artifact, inputs, alloc);
}

fn main() {}
