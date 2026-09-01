//! The Vue host-integration backend cannot be built outside its crate:
//! the field is private, so every consumer holds the registered
//! `'static` instance instead of a freshly minted service value.

use verter_compiler::framework_common::VueHostIntegrationBackend;

fn build() -> VueHostIntegrationBackend {
    VueHostIntegrationBackend { _registered: () }
}

fn main() {}
