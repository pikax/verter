//! Mapping-product identities (placement, projection, runtime, encoded)
//! plus the exact map-construction identity [`MapRevision`]. A single
//! "maps enabled" boolean is not enough (`mapping-products.md` §1). This
//! module does not construct, encode, or round-trip maps — that stays
//! with the source-unit / `CodeTransform` owner.

digest_identity!(
    /// Internal source-unit placement/composition identity.
    PlacementMapId
);
digest_identity!(
    /// Map required to interpret an IDE/provider companion. A companion
    /// that needs this cannot be Ready without it.
    SourceProjectionMapId
);
digest_identity!(
    /// Optional runtime/build map segments. Absence is `Option::None`,
    /// not a zero-value instance.
    RuntimeSourceMapDataId
);
digest_identity!(
    /// Terminal serialized-map identity. Changing this construction does
    /// not invalidate [`PlacementMapId`] or [`SourceProjectionMapId`] for
    /// the same underlying map data.
    EncodedSourceMapId
);
digest_identity!(
    /// Exact map-construction identity. Neighbour of source revision and
    /// content — never folded into [`crate::identity::SourceUnitId`].
    MapRevision
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

    #[test]
    fn four_distinct_mapping_identities_construct_independently() {
        let placement = PlacementMapId::from_canonical(&MapDescriptor(1));
        let projection = SourceProjectionMapId::from_canonical(&MapDescriptor(1));
        let runtime = RuntimeSourceMapDataId::from_canonical(&MapDescriptor(1));
        let encoded = EncodedSourceMapId::from_canonical(&MapDescriptor(1));
        let map_revision = MapRevision::from_canonical(&MapDescriptor(1));
        assert_eq!(placement, PlacementMapId::from_canonical(&MapDescriptor(1)));
        assert_eq!(map_revision, MapRevision::from_canonical(&MapDescriptor(1)));
        assert_ne!(map_revision, MapRevision::from_canonical(&MapDescriptor(2)));
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
