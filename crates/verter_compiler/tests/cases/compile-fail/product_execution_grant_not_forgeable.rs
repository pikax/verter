//! A product backend cannot be driven without admission evidence: the
//! consume-once execution grant has a private field and a crate-private
//! mint, so an external caller can neither forge one by struct literal,
//! mint one directly, nor reach any test-only constructor — the only
//! out-of-crate sources are the admission carves on the host-integration
//! backends. The backends themselves are equally sealed: no external
//! construction, no `Default`.

use verter_compiler::compile_request::ProductKind;
use verter_compiler::framework_common::{
    ProductExecutionGrant, SvelteHostIntegrationBackend, VueHostIntegrationBackend,
};

fn forge() -> ProductExecutionGrant {
    ProductExecutionGrant {
        admitted: ProductKind::IdeCompanion,
    }
}

fn mint() -> ProductExecutionGrant {
    ProductExecutionGrant::mint(ProductKind::IdeCompanion)
}

fn test_mint() -> ProductExecutionGrant {
    ProductExecutionGrant::mint_for_tests(ProductKind::IdeCompanion)
}

fn build_vue_backend() -> VueHostIntegrationBackend {
    VueHostIntegrationBackend { _registered: () }
}

fn default_vue_backend() -> VueHostIntegrationBackend {
    VueHostIntegrationBackend::default()
}

fn new_svelte_backend() -> SvelteHostIntegrationBackend {
    SvelteHostIntegrationBackend::new()
}

fn main() {}
