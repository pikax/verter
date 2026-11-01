//! Shared declarative macros used by [`crate::identity`],
//! [`crate::profile`], [`crate::mapping`] and [`crate::successor`].

/// Shared members of every digest identity newtype: the struct itself and
/// the read-only accessors. Written once; [`macro@digest_identity`] and
/// [`macro@sealed_digest_identity`] layer their construction boundary on
/// top of it.
#[macro_export]
macro_rules! digest_identity_shared {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(verter_retention::heap_size::HeapSize, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name($crate::canonical::Canonical);

        impl $name {
            /// The compact digest, for hot-path hashing/comparison.
            pub fn digest(&self) -> $crate::encoding::CanonicalDigest {
                self.0.digest()
            }

            /// The retained canonical bytes, for full-equality verification
            /// on a suspected collision.
            pub fn canonical_bytes(&self) -> &[u8] {
                self.0.bytes()
            }

            /// The one allocation every clone shares, for a measurement
            /// that counts each distinct allocation once.
            pub fn canonical_shared_bytes(&self) -> &std::sync::Arc<[u8]> {
                self.0.shared_bytes()
            }
        }

        impl core::fmt::Debug for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, concat!(stringify!($name), "({:?})"), self.0)
            }
        }
    };
}

/// Digest-backed identity newtype over [`crate::canonical::Canonical`]
/// whose descriptor is caller-owned: the caller defines the fields, so
/// [`Self::from_canonical`] is the construction path. Each invocation is a
/// distinct Rust type — the macro does not introduce a shared base they can
/// coerce through.
#[macro_export]
macro_rules! digest_identity {
    ($(#[$meta:meta])* $name:ident) => {
        $crate::digest_identity_shared!($(#[$meta])* $name);

        impl $name {
            /// Build from a canonical descriptor (`DOMAIN_TAG` is the
            /// compatibility domain).
            pub fn from_canonical<T: $crate::encoding::CanonicalEncode>(value: &T) -> Self {
                Self($crate::canonical::Canonical::from_encodable(value))
            }
        }
    };
}

/// Digest-backed identity newtype whose derivation this crate fixes. It
/// has **no** `from_canonical`: minting from an arbitrary canonical
/// descriptor would admit inputs the type's law rejects (a floating tag
/// where only decoded exact coordinates are allowed, a profile without
/// its release, an epoch-less identity), so construction happens only
/// through the type's own named constructors — the same boundary
/// `SourceUnitId` holds in [`crate::identity`]. Accessors and distinctness
/// are shared with [`macro@digest_identity`].
#[macro_export]
macro_rules! sealed_digest_identity {
    ($(#[$meta:meta])* $name:ident) => {
        $crate::digest_identity_shared!($(#[$meta])* $name);
    };
}
