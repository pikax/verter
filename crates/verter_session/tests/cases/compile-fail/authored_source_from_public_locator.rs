use std::sync::Arc;

use verter_type_expr::locators::{
    AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, TypeBodySlot,
};
use verter_type_expr::{AuthoredTypeSource, TopLevelOwnerId};

fn main() {
    let locator = AuthoredBodyLocator::DeclBody(TypeBodySlot {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from("/forged.ts"),
            owner: TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from("Forged"),
            space: LocatorSymbolSpace::Type,
        },
        path: Arc::from([]),
    });

    let _forged = AuthoredTypeSource::from_authored_body(&locator);
}
