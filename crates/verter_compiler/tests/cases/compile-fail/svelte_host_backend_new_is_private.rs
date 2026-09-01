//! The Svelte host-integration backend's constructor is crate-private:
//! every consumer holds the registered `'static` instance instead of a
//! freshly minted service value.

use verter_compiler::framework_common::SvelteHostIntegrationBackend;

fn new() -> SvelteHostIntegrationBackend {
    SvelteHostIntegrationBackend::new()
}

fn main() {}
