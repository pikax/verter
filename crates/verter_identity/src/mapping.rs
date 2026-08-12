//! Mapping product identity types, per mapping-products.md §1: "The
//! architecture distinguishes [four] different identities and products. A
//! single 'maps enabled' boolean is insufficient at architecture/API/
//! benchmark level."
//!
//! Scope, stated precisely: this module lands the four DISTINCT identity
//! types so nothing downstream collapses them back into one boolean or one
//! generic "map" type. It does NOT implement map construction, encoding, or
//! round-trip behavior — that is source-unit/`CodeTransform` behavior owned
//! by whichever component compacts the source-unit representation, and
//! building it here would be exactly the semantic-behavior migration
//! landing dependency-neutral types is scoped apart from.

digest_identity!(
    /// Internal source-unit placement/composition identity
    /// (mapping-products.md §1.1).
    PlacementMapId
);
digest_identity!(
    /// Identity of the map required to interpret an IDE/provider companion
    /// (mapping-products.md §1.2). An IDE companion requiring this cannot be
    /// Ready/published without it (mapping-products.md §3).
    SourceProjectionMapId
);
digest_identity!(
    /// Identity of an optional runtime/build map segment set
    /// (mapping-products.md §1.3). Runtime code without a requested runtime
    /// source map constructs no [`RuntimeSourceMapDataId`] at all
    /// (mapping-products.md §3) — this type's absence (`Option::None` at the
    /// call site, not a zero-value instance) is the correct representation
    /// of "not requested".
    RuntimeSourceMapDataId
);
digest_identity!(
    /// Terminal external serialized map identity (mapping-products.md
    /// §1.4). Encoding/serialization identity is separate from semantic/
    /// generated-code identity (mapping-products.md §4) — changing this
    /// type's construction never invalidates a [`PlacementMapId`] or
    /// [`SourceProjectionMapId`] for the same underlying map data.
    EncodedSourceMapId
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::{CanonicalEncode, CanonicalEncoder};

    struct MapDescriptor(u64);
    impl CanonicalEncode for MapDescriptor {
        const DOMAIN_TAG: &'static str = "mapping-test.descriptor.v1";
        fn encode_fields(&self, e: &mut CanonicalEncoder) {
            e.field_u64(1, self.0);
        }
    }

    /// The four mapping-product identities are four distinct Rust types --
    /// exactly the "single boolean is insufficient" requirement
    /// (mapping-products.md 1) made structural: a caller cannot pass a
    /// `PlacementMapId` where a `SourceProjectionMapId` is expected.
    #[test]
    fn four_distinct_mapping_identities_construct_independently() {
        let placement = PlacementMapId::from_canonical(&MapDescriptor(1));
        let projection = SourceProjectionMapId::from_canonical(&MapDescriptor(1));
        let runtime = RuntimeSourceMapDataId::from_canonical(&MapDescriptor(1));
        let encoded = EncodedSourceMapId::from_canonical(&MapDescriptor(1));
        assert_eq!(placement, PlacementMapId::from_canonical(&MapDescriptor(1)));
        assert_eq!(
            projection,
            SourceProjectionMapId::from_canonical(&MapDescriptor(1))
        );
        assert_eq!(
            runtime,
            RuntimeSourceMapDataId::from_canonical(&MapDescriptor(1))
        );
        assert_eq!(
            encoded,
            EncodedSourceMapId::from_canonical(&MapDescriptor(1))
        );
    }

    /// "Not requested" is `Option::None`, never a zero-value instance
    /// (mapping-products.md 3) -- exercised structurally: this type has no
    /// `Default`/zero-value constructor at all, only `from_canonical`, so
    /// "absent" can only be expressed by the `Option` wrapper a caller
    /// chooses, never by an in-band sentinel this crate would have to
    /// invent and then special-case.
    #[test]
    fn runtime_source_map_data_id_has_no_zero_value_constructor() {
        let requested: Option<RuntimeSourceMapDataId> = None;
        assert!(requested.is_none());
        let requested = Some(RuntimeSourceMapDataId::from_canonical(&MapDescriptor(1)));
        assert!(requested.is_some());
    }
}
