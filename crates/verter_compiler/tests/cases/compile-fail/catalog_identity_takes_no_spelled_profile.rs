//! A catalog identity is minted only by the typed registration
//! constructors, from an epoch type's manifest: no spelled adapter,
//! carrier or profile string constructs one, and the identity has no
//! public fields to fill in.

use verter_compiler::framework_common::{CatalogCapability, CatalogIdentity};
use verter_language::{FrameworkAdapterId, LanguageId};

fn main() {
    let _ = CatalogIdentity::ordinary(
        FrameworkAdapterId::new("vue"),
        LanguageId::new("vue"),
        CatalogCapability::Frontend,
    );
    let _ = CatalogIdentity {
        adapter_id: FrameworkAdapterId::new("vue"),
        carrier_language_id: LanguageId::new("vue"),
        framework_family: "vue",
        capability: CatalogCapability::Frontend,
    };
}
