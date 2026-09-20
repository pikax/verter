//! Compile-fail fixture: the capability catalog rows are SEALED — their
//! fields are private and identity composition resolves a `QueryFeature`
//! through `capability_row`, so a caller cannot copy the Hover row, retag
//! it, and mint a second certified query/result contract for Hover. If the
//! fields were widened to `pub`, this fixture would COMPILE and the
//! compile-contracts lane would turn red.

use verter_session::external_ts::QueryFeature;
use verter_session::semantic_capability::{SEMANTIC_CAPABILITY_CATALOG, SemanticCapabilityRow};

fn forge_row() -> SemanticCapabilityRow {
    let canonical = SEMANTIC_CAPABILITY_CATALOG
        .iter()
        .find(|row| row.feature() == QueryFeature::Hover)
        .expect("the catalog must close Hover");
    SemanticCapabilityRow {
        feature: canonical.feature(),
        query_kind_domain_tag: "verter.session.forged.query.hover.v1",
    }
}

fn main() {
    let _ = forge_row();
}
