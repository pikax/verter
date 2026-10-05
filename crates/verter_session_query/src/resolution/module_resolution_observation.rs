//! Observation DTOs for `ResolverObservation`'s three module-resolution
//! I/O primitives: `probe_path`, `real_path`, and `package_manifest`.
//! They correspond one-for-one with `InputKey::PathProbe`,
//! `InputKey::RealPath`, and `InputKey::PackageManifest`.
//!
//! `PathProbe` and `CanonicalId` are semantic-owned resolver vocabulary.
//! `ResolutionPackageManifest` is intentionally narrower than the
//! workspace manifest: resolution reads only `exports`, `imports`, `main`,
//! `module`, `types`, and `typings`, so identity metadata and raw source do
//! not cross the observation boundary.

/// Narrow projection of `verter_workspace::types::PackageManifest`,
/// carrying only the fields `resolver.rs`'s resolution algorithm actually
/// reads. Omits `name`/`version` (identity metadata, unused by resolution)
/// and `raw` (re-parse-from-source escape hatch — the kernel receives the
/// already-parsed projection and has no re-parse capability of its own).
#[derive(Debug, Clone, PartialEq)]
pub struct ResolutionPackageManifest {
    pub main: Option<String>,
    pub module: Option<String>,
    pub types: Option<String>,
    pub typings: Option<String>,
    pub exports: Option<serde_json::Value>,
    pub imports: Option<serde_json::Value>,
}
