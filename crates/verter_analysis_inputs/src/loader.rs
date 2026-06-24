//! The analysis-input config CONTENT is parsed here; the file READ lives outside.
//!
//! This crate is a filesystem-free neutral leaf: it parses config *content* a
//! caller hands it ([`crate::parse_config`]) and never touches disk. The actual
//! file read (from the `DX_HARNESS_EXTERNAL_CORPUS` env var or a fixed default
//! path) belongs to the consumer that owns an allow-listed disk boundary — the TS
//! dx-harness performs its own I/O (`packages/dx-harness/src/analysisConfig.ts`),
//! and the future Rust analysis runner (P1-IDE / P1-TSC) will read the file through
//! its own `WorkspaceAccess`/`NativeFs` path, then call [`crate::parse_config`] on
//! the bytes. Keeping the read out of this leaf preserves the NativeFs invariant
//! (this crate touches no OS file API) without an allow-list carve-out.

/// The env var an opt-in runner sets (to the config path) to load a local analysis
/// corpus. Defined here as a single source of truth shared with the TS side; the
/// consumer that owns the disk boundary reads it and feeds the bytes to
/// [`crate::parse_config`]. This crate never reads the var or the file itself.
pub const ANALYSIS_CORPUS_ENV: &str = "DX_HARNESS_EXTERNAL_CORPUS";
