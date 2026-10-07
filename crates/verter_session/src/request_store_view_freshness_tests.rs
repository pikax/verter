//! A request store view owns its base's captured content generation in the
//! workspace freshness history.
//!
//! The artifact-only whole-hash leg clamps every transition answer to the
//! generation the base captured. If retiring history raised the transition
//! floor past that generation mid-request, every artifact the request reads
//! through the base would look stale.

use std::sync::Arc;

use crate::resolver_core::{CanonicalCompletionOverlay, RequestStoreView};
use crate::{HostConfig, VerterHost};

fn churn(host: &VerterHost, tag: &str) {
    let records: Vec<(String, Arc<str>)> = (0..verter_workspace::freshness::DEFAULT_RETIRE_TRIGGER
        + 64)
        .map(|index| (format!("/churn/{tag}/{index}.ts"), Arc::from("export {};")))
        .collect();
    host.ws().notify_upsert_many(&records);
}

#[test]
fn a_request_view_caps_freshness_retirement_at_its_captured_generation() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let base = host.resolver_store_view_read().into_owned_view();
    let captured = host.ws().content_generation();
    let leases_before = host.ws().resource_snapshot().freshness_history.view_leases;

    let view = RequestStoreView::new(&base, Arc::new(CanonicalCompletionOverlay::new()));
    assert_eq!(
        host.ws().resource_snapshot().freshness_history.view_leases,
        leases_before + 1
    );
    churn(&host, "during");
    assert!(
        host.ws()
            .last_content_transition_generation("/untouched.ts")
            <= captured,
        "retirement must not pass the live request's captured generation"
    );

    drop(view);
    assert_eq!(
        host.ws().resource_snapshot().freshness_history.view_leases,
        leases_before
    );
    assert!(
        host.ws()
            .last_content_transition_generation("/untouched.ts")
            > captured,
        "once the request leaves, retirement may raise the floor"
    );
}
