//! Identity of session-owned in-memory artifact shapes.

/// Fingerprint of private build-toolchain and in-memory DTO shapes.
///
/// This is deliberately separate from [`verter_identity::identity::ParseKey`]:
/// it invalidates derived session values whose representation changed without
/// claiming that identical source bytes parse differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BuildToolchainFingerprint([u8; 32]);

impl BuildToolchainFingerprint {
    /// Constructs a fingerprint from its canonical digest bytes.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the canonical digest bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

const CURRENT_BUILD_TOOLCHAIN_FINGERPRINT_BYTES: [u8; 32] = [
    0x4a, 0x35, 0x4a, 0xd2, 0x39, 0x2a, 0x48, 0xea, 0x8f, 0x71, 0x98, 0x3b, 0x44, 0xce, 0x1a, 0xc7,
    0x50, 0x09, 0x25, 0x52, 0xd1, 0xa5, 0x15, 0xbd, 0xec, 0x15, 0x1f, 0x97, 0xa1, 0xc9, 0x82, 0x8d,
];

/// Returns the one session-owned build-toolchain fingerprint.
#[must_use]
pub const fn current_build_toolchain_fingerprint() -> BuildToolchainFingerprint {
    BuildToolchainFingerprint(CURRENT_BUILD_TOOLCHAIN_FINGERPRINT_BYTES)
}

/// Mints a distinct, real parse identity for cache-key tests.
#[cfg(any(test, feature = "test-support"))]
pub fn parse_key_for_test(canonical: &str, marker: u8) -> verter_language::ParseKey {
    let language = verter_language::LanguageRegistry::global()
        .classify_static(canonical)
        .static_resolution();
    let source = format!("/* test parse identity {marker} */");
    verter_language::default_parse_identity_for(&source, &language)
        .expect("test canonical has a supported parse identity")
        .1
}

/// Mints a distinct private-shape fingerprint for invalidation tests.
#[cfg(any(test, feature = "test-support"))]
pub const fn fingerprint_for_test(marker: u8) -> BuildToolchainFingerprint {
    BuildToolchainFingerprint([marker; 32])
}
