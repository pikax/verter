//! Shared declarative macros used by [`crate::identity`] and
//! [`crate::profile`].

/// Digest-backed identity newtype over [`crate::canonical::Canonical`].
/// Each invocation is a distinct Rust type — the macro does not introduce
/// a shared base they can coerce through.
#[macro_export]
macro_rules! digest_identity {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name($crate::canonical::Canonical);

        impl $name {
            /// Build from a canonical descriptor (`DOMAIN_TAG` is the
            /// compatibility domain).
            pub fn from_canonical<T: $crate::encoding::CanonicalEncode>(value: &T) -> Self {
                Self($crate::canonical::Canonical::from_encodable(value))
            }

            /// The compact digest, for hot-path hashing/comparison.
            pub fn digest(&self) -> $crate::encoding::CanonicalDigest {
                self.0.digest()
            }

            /// The retained canonical bytes, for full-equality verification
            /// on a suspected collision.
            pub fn canonical_bytes(&self) -> &[u8] {
                self.0.bytes()
            }
        }

        impl core::fmt::Debug for $name {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                write!(f, concat!(stringify!($name), "({:?})"), self.0)
            }
        }
    };
}
