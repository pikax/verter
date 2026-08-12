//! Shared declarative macros used by [`crate::identity`] and
//! [`crate::profile`].

/// Declares a digest-backed identity newtype: a thin, non-interchangeable
/// wrapper over [`crate::canonical::Canonical`], constructed from any
/// owner-supplied [`crate::encoding::CanonicalEncode`] descriptor. Each
/// invocation produces a genuinely distinct Rust type — the macro exists to
/// keep the many structurally-identical declarations mechanically
/// consistent, not to erase their distinctness (there is no shared
/// non-marker base type any two of them coerce through).
#[macro_export]
macro_rules! digest_identity {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name($crate::canonical::Canonical);

        impl $name {
            /// Builds this identity from a canonical descriptor. The
            /// descriptor's [`crate::encoding::CanonicalEncode::DOMAIN_TAG`]
            /// is this identity's compatibility domain
            /// (identity-encoding.md §1).
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
