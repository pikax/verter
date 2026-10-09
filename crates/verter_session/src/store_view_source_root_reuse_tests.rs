//! A cached base view must not outlive a byte-identical re-upsert's source
//! re-capture.
//!
//! The scheduler re-captures every submitted source at a fresh generation in
//! two halves: the generation bump records the file `Absent`, and the Source
//! commit records it `Present` again. A view captured between the two reads
//! the file as source-less. A byte-identical re-upsert advances no token
//! dimension (it is a deliberate host no-op), so unless that path retires
//! the cached view, every cache entry that read the file keeps failing
//! validation against it and its owners stay cold for good.

use std::sync::Arc;

use crate::types::FileLanguage;
use crate::{HostConfig, UpsertRequest, VerterHost};

const DEPENDENCY: &str = "/p/dep.ts";
const SOURCE: &str = "export const value = 1\n";

fn upsert(host: &VerterHost) {
    let _update = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: DEPENDENCY.to_string(),
            source: Arc::from(SOURCE),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
}

#[test]
fn a_view_captured_inside_a_source_recapture_is_not_served_after_an_unchanged_upsert() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(&host);
    let committed = host
        .scheduler()
        .try_get_source(DEPENDENCY)
        .expect("the upsert committed a source");

    // Stand in for a reader that captured its view inside a re-capture: the
    // first half of the transition has landed, the commit has not.
    let canonical: Arc<str> = Arc::from(DEPENDENCY);
    host.scheduler()
        .source_directory()
        .publish_transition(|publication| {
            publication.absent(&canonical, committed.incarnation, committed.generation);
        });
    let inside = host.resolver_store_view_read().into_owned_view();
    assert_eq!(
        inside.whole_hash(DEPENDENCY),
        None,
        "precondition: a view captured inside the window reads the file as \
         source-less"
    );

    let token_before = host.current_validation_token();
    upsert(&host);
    assert_eq!(
        host.current_validation_token(),
        token_before,
        "precondition: the byte-identical re-upsert advances no token dimension"
    );

    let after = host.resolver_store_view_read().into_owned_view();
    assert_eq!(
        after.whole_hash(DEPENDENCY),
        Some(committed.whole_hash),
        "after the re-upsert committed, the view must answer the committed \
         source rather than the cached view captured inside the window"
    );
}
