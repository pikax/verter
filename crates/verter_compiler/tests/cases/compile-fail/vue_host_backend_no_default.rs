//! The Vue host-integration backend has no `Default`: a defaulted value
//! would be an out-of-crate construction path around the registered
//! `'static` instance.

use verter_compiler::framework_common::VueHostIntegrationBackend;

fn default() -> VueHostIntegrationBackend {
    VueHostIntegrationBackend::default()
}

fn main() {}
