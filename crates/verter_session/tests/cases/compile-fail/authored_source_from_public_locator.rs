use verter_type_expr::{OwnedSlice, OwnedStr};

use verter_type_expr::locators::{
    AuthoredAnchor, AuthoredBodyLocator, LocatorSymbolSpace, TypeBodySlot,
};
use verter_type_expr::{AuthoredTypeSource, TopLevelOwnerId};

fn main() {
    let locator = AuthoredBodyLocator::DeclBody(TypeBodySlot {
        anchor: AuthoredAnchor {
            canonical_id: OwnedStr::from("/forged.ts"),
            owner: TopLevelOwnerId::ordinary_file(),
            symbol: OwnedStr::from("Forged"),
            space: LocatorSymbolSpace::Type,
        },
        path: OwnedSlice::default(),
    });

    let _forged = AuthoredTypeSource::from_authored_body(&locator);
}
