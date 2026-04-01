//! Generated query artifact contract and mapping helpers.
//!
//! tsserver/TSGO never receive arbitrary source slices. They query a generated
//! artifact produced from the SFC under an explicit profile. This module owns
//! the artifact structure and the SFC span → generated offset mapping.
//!
//! # Invariants
//!
//! - Each `(canonical_id, profile)` pair maps to a distinct generated file identity
//! - Different profiles for the same source file never share the same generated identity
//! - `QuerySpanMapping` entries are stably ordered by generated offset
//! - Mixed block origins (script, script_setup, template) remain unambiguous
//! - The artifact tracks the source revision used to build it

use verter_span::Span;

use crate::resolver_core::type_expansion::ExpansionProfile;

// ---------------------------------------------------------------------------
// Artifact Profile
// ---------------------------------------------------------------------------

/// Controls what the generated artifact contains.
/// Mirrors [`ExpansionProfile`] but is the artifact-layer representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactProfile {
    /// Minimal script for type expansion: imports + type decls + setup body.
    ComponentMeta,
    /// Full IDE artifact (existing LSP path).
    Lsp,
}

impl From<ExpansionProfile> for ArtifactProfile {
    fn from(profile: ExpansionProfile) -> Self {
        match profile {
            ExpansionProfile::ComponentMeta => Self::ComponentMeta,
            ExpansionProfile::Lsp => Self::Lsp,
        }
    }
}

/// Convert to the runtime crate's `ArtifactProfile` for backend sync.
impl From<ArtifactProfile> for verter_type_runtime::ArtifactProfile {
    fn from(profile: ArtifactProfile) -> Self {
        match profile {
            ArtifactProfile::ComponentMeta => Self::ComponentMeta,
            ArtifactProfile::Lsp => Self::Lsp,
        }
    }
}

// ---------------------------------------------------------------------------
// Generated File Identity
// ---------------------------------------------------------------------------

/// Semantic identity of a generated artifact at the resolver layer.
///
/// Uniquely identifies an artifact by `(canonical_id, profile)`.
/// The runtime layer adds a `runtime_key` when creating the full
/// `verter_type_runtime::GeneratedFileId` for session-scoped sync.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtifactId {
    /// Canonical file ID of the source SFC.
    pub canonical_id: String,
    /// Which profile produced this artifact.
    pub profile: ArtifactProfile,
}

impl ArtifactId {
    /// Create a new artifact identity.
    pub fn new(canonical_id: impl Into<String>, profile: ArtifactProfile) -> Self {
        Self {
            canonical_id: canonical_id.into(),
            profile,
        }
    }

    /// Compute the virtual path suffix for this artifact.
    ///
    /// Different profiles produce different paths so they never collide.
    /// For `.vue` files: `/src/Foo.vue` → `/src/Foo.vue.meta.ts` (component-meta)
    ///                                   → `/src/Foo.vue.tsx` (LSP)
    pub fn virtual_path(&self) -> String {
        let suffix = match self.profile {
            ArtifactProfile::ComponentMeta => ".meta.ts",
            ArtifactProfile::Lsp => ".tsx",
        };
        format!("{}{}", self.canonical_id, suffix)
    }
}

// ---------------------------------------------------------------------------
// Generated Query Artifact
// ---------------------------------------------------------------------------

/// A generated file produced from an SFC under a specific profile.
///
/// This artifact is the ONLY place where SFC span → generated offset
/// translation happens. Backend queries use generated offsets; the mapping
/// table converts back and forth.
#[derive(Debug, Clone)]
pub struct GeneratedQueryArtifact {
    /// Generated TypeScript/TSX source text.
    pub generated_source: String,
    /// Which profile produced this artifact.
    pub profile: ArtifactProfile,
    /// SFC span → generated offset mappings.
    /// Ordered by `generated_offset`.
    pub mappings: Vec<QuerySpanMapping>,
    /// Source revision used to build this artifact.
    pub source_revision: u64,
    /// Generated file identity for this artifact.
    pub artifact_id: ArtifactId,
}

/// Maps an SFC-absolute span to a position in the generated artifact.
#[derive(Debug, Clone, Copy)]
pub struct QuerySpanMapping {
    /// SFC-absolute span of the original source region.
    pub sfc_span: Span,
    /// Byte offset in the generated source where this region starts.
    pub generated_offset: u32,
    /// Byte length in the generated source.
    pub generated_len: u32,
}

impl GeneratedQueryArtifact {
    /// Look up the generated offset for an exact SFC span.
    ///
    /// Returns `None` if no mapping covers the requested span exactly.
    pub fn lookup_sfc_span(&self, sfc_span: Span) -> Option<&QuerySpanMapping> {
        self.mappings
            .iter()
            .find(|m| m.sfc_span.start == sfc_span.start && m.sfc_span.end == sfc_span.end)
    }

    /// Look up the generated offset for an SFC span that falls within
    /// a mapped region. Returns the mapping and the relative offset within it.
    pub fn lookup_containing(&self, sfc_offset: u32) -> Option<(&QuerySpanMapping, u32)> {
        for mapping in &self.mappings {
            if sfc_offset >= mapping.sfc_span.start && sfc_offset < mapping.sfc_span.end {
                let relative = sfc_offset - mapping.sfc_span.start;
                return Some((mapping, relative));
            }
        }
        None
    }

    /// Translate an SFC-absolute offset to a generated-artifact offset.
    ///
    /// Returns `None` if the offset doesn't fall within any mapped region.
    pub fn sfc_to_generated(&self, sfc_offset: u32) -> Option<u32> {
        self.lookup_containing(sfc_offset)
            .map(|(mapping, relative)| mapping.generated_offset + relative)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_artifact() -> GeneratedQueryArtifact {
        // Simulates an SFC with:
        //   <script> at SFC bytes 20..150 → generated bytes 0..130
        //   <script setup> at SFC bytes 170..300 → generated bytes 130..260
        GeneratedQueryArtifact {
            generated_source: "x".repeat(260),
            profile: ArtifactProfile::ComponentMeta,
            mappings: vec![
                QuerySpanMapping {
                    sfc_span: Span::new(20, 150),
                    generated_offset: 0,
                    generated_len: 130,
                },
                QuerySpanMapping {
                    sfc_span: Span::new(170, 300),
                    generated_offset: 130,
                    generated_len: 130,
                },
            ],
            source_revision: 1,
            artifact_id: ArtifactId::new("/src/Button.vue", ArtifactProfile::ComponentMeta),
        }
    }

    #[test]
    fn different_profiles_produce_different_virtual_paths() {
        let meta_id = ArtifactId::new("/src/Foo.vue", ArtifactProfile::ComponentMeta);
        let lsp_id = ArtifactId::new("/src/Foo.vue", ArtifactProfile::Lsp);
        assert_ne!(
            meta_id.virtual_path(),
            lsp_id.virtual_path(),
            "different profiles must produce different paths"
        );
    }

    #[test]
    fn virtual_path_includes_canonical_id_and_profile_suffix() {
        let id = ArtifactId::new("/src/Foo.vue", ArtifactProfile::ComponentMeta);
        assert_eq!(id.virtual_path(), "/src/Foo.vue.meta.ts");

        let id = ArtifactId::new("/src/Foo.vue", ArtifactProfile::Lsp);
        assert_eq!(id.virtual_path(), "/src/Foo.vue.tsx");
    }

    #[test]
    fn lookup_exact_sfc_span() {
        let artifact = make_artifact();
        let mapping = artifact.lookup_sfc_span(Span::new(20, 150));
        assert!(mapping.is_some(), "exact span should match");
        assert_eq!(mapping.unwrap().generated_offset, 0);
    }

    #[test]
    fn lookup_exact_sfc_span_no_match() {
        let artifact = make_artifact();
        // Span that doesn't exactly match any mapping
        let mapping = artifact.lookup_sfc_span(Span::new(25, 150));
        assert!(mapping.is_none(), "non-exact span should not match");
    }

    #[test]
    fn sfc_to_generated_offset_in_first_block() {
        let artifact = make_artifact();
        // SFC offset 50 is inside first block (20..150)
        // relative offset = 50 - 20 = 30
        // generated offset = 0 + 30 = 30
        assert_eq!(artifact.sfc_to_generated(50), Some(30));
    }

    #[test]
    fn sfc_to_generated_offset_in_second_block() {
        let artifact = make_artifact();
        // SFC offset 200 is inside second block (170..300)
        // relative offset = 200 - 170 = 30
        // generated offset = 130 + 30 = 160
        assert_eq!(artifact.sfc_to_generated(200), Some(160));
    }

    #[test]
    fn sfc_to_generated_offset_outside_any_block() {
        let artifact = make_artifact();
        // SFC offset 155 is between the two blocks (gap 150..170)
        assert_eq!(artifact.sfc_to_generated(155), None);
    }

    #[test]
    fn sfc_to_generated_offset_at_block_boundary() {
        let artifact = make_artifact();
        // Start of first block
        assert_eq!(artifact.sfc_to_generated(20), Some(0));
        // Start of second block
        assert_eq!(artifact.sfc_to_generated(170), Some(130));
    }

    #[test]
    fn sfc_to_generated_offset_at_block_end_is_exclusive() {
        let artifact = make_artifact();
        // End of first block (exclusive) — should NOT match
        assert_eq!(artifact.sfc_to_generated(150), None);
    }

    #[test]
    fn artifact_tracks_source_revision() {
        let artifact = make_artifact();
        assert_eq!(artifact.source_revision, 1);
    }

    #[test]
    fn mappings_ordered_by_generated_offset() {
        let artifact = make_artifact();
        for window in artifact.mappings.windows(2) {
            assert!(
                window[0].generated_offset < window[1].generated_offset,
                "mappings must be ordered by generated_offset"
            );
        }
    }

    #[test]
    fn artifact_id_equality_by_canonical_and_profile() {
        let a = ArtifactId::new("/src/A.vue", ArtifactProfile::ComponentMeta);
        let b = ArtifactId::new("/src/A.vue", ArtifactProfile::ComponentMeta);
        let c = ArtifactId::new("/src/A.vue", ArtifactProfile::Lsp);
        assert_eq!(a, b, "same canonical + profile should be equal");
        assert_ne!(a, c, "different profile should be different");
    }
}
