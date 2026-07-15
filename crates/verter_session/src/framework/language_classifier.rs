//! Host-level language classification: static registry × project
//! capabilities.

use std::sync::Arc;

use verter_language::{CapabilityId, FileLanguage, LanguageRegistry, StaticClassification};

use super::project_capabilities::ProjectCapabilitySnapshot;
use crate::types::Hash16;

/// The single classification authority for SESSION-level consumers.
///
/// Composes [`LanguageRegistry::classify_static`] (the pure leaf entry)
/// with the [`ProjectCapabilitySnapshot`]: a gated candidate row
/// resolves to its candidate language when the gating capability bit is
/// derived ON, and to its ungated fallback otherwise.
///
/// FFI-time classification deliberately does NOT route through this
/// type: the FFI boundary is static-only (it cannot consult project
/// capabilities), so gated rows REQUIRE an explicit kind string there.
#[derive(Debug, Clone)]
pub struct HostLanguageClassifier {
    registry: Arc<LanguageRegistry>,
    capabilities: ProjectCapabilitySnapshot,
}

impl HostLanguageClassifier {
    /// Classifier over an explicit registry + capability snapshot.
    pub fn new(registry: Arc<LanguageRegistry>, capabilities: ProjectCapabilitySnapshot) -> Self {
        Self {
            registry,
            capabilities,
        }
    }

    /// Classifier over the built-in registry.
    pub fn with_built_in_registry(capabilities: ProjectCapabilitySnapshot) -> Self {
        Self::new(Arc::new(LanguageRegistry::built_in()), capabilities)
    }

    /// Resolve a path to its [`FileLanguage`] row.
    pub fn classify(&self, path: &str) -> FileLanguage {
        match self.registry.classify_static(path) {
            StaticClassification::Resolved(language) => language,
            StaticClassification::Gated(candidate) => {
                if self.capabilities.is_enabled(&candidate.capability) {
                    candidate.candidate
                } else {
                    candidate.fallback
                }
            }
            StaticClassification::Unknown => FileLanguage::script_ts(),
        }
    }

    /// The capability-snapshot hash — the classification cache key
    /// dimension (a capability flip changes it; raw config edits that
    /// flip no derived bit do not).
    pub fn capability_hash(&self) -> Hash16 {
        self.capabilities.hash()
    }

    /// Whether a derived capability bit is ON. The resolved-validation half of
    /// the framework script-fact seam consults this to gate a provider's
    /// resolved facts on a derived capability.
    pub fn capability_is_enabled(&self, capability: &CapabilityId) -> bool {
        self.capabilities.is_enabled(capability)
    }
}

impl Default for HostLanguageClassifier {
    fn default() -> Self {
        Self::with_built_in_registry(ProjectCapabilitySnapshot::empty())
    }
}

#[cfg(test)]
mod tests {
    use verter_language::{
        CapabilityId, FrameworkAdapterId, GatedCandidate, LanguageRow, ScriptSourceType,
    };

    use super::*;

    #[test]
    fn empty_snapshot_matches_static_resolution_for_built_in_rows() {
        let classifier = HostLanguageClassifier::default();
        for path in [
            "/src/App.vue",
            "/src/Box.svelte",
            "/src/a.ts",
            "/src/a.d.ts",
            "/src/a.jsx",
            "/src/unknown.css",
        ] {
            assert_eq!(
                classifier.classify(path),
                LanguageRegistry::global()
                    .classify_static(path)
                    .static_resolution(),
                "empty snapshot must match pure static resolution for {path}"
            );
        }
    }

    fn gated_registry() -> (Arc<LanguageRegistry>, CapabilityId, FileLanguage) {
        let capability = CapabilityId::new("fixture-capability");
        let candidate_language = FileLanguage::FrameworkTemplate {
            adapter_id: FrameworkAdapterId::new("fixture-framework"),
            owner_hint: None,
        };
        let registry = Arc::new(LanguageRegistry::new(vec![
            LanguageRow::fixed("vue", FileLanguage::vue()),
            LanguageRow::gated(
                "html",
                GatedCandidate {
                    capability: capability.clone(),
                    candidate: candidate_language.clone(),
                    fallback: FileLanguage::script(ScriptSourceType::Ts),
                },
            ),
        ]));
        (registry, capability, candidate_language)
    }

    #[test]
    fn gated_row_resolves_to_candidate_only_when_bit_is_on() {
        let (registry, capability, candidate_language) = gated_registry();

        let off =
            HostLanguageClassifier::new(Arc::clone(&registry), ProjectCapabilitySnapshot::empty());
        assert_eq!(
            off.classify("/src/page.html"),
            FileLanguage::script(ScriptSourceType::Ts),
            "capability OFF must resolve the gated row to its fallback"
        );

        let on = HostLanguageClassifier::new(
            registry,
            ProjectCapabilitySnapshot::from_capabilities([capability]),
        );
        assert_eq!(
            on.classify("/src/page.html"),
            candidate_language,
            "capability ON must resolve the gated row to its candidate"
        );
    }

    #[test]
    fn capability_hash_tracks_the_snapshot() {
        let (registry, capability, _) = gated_registry();
        let off =
            HostLanguageClassifier::new(Arc::clone(&registry), ProjectCapabilitySnapshot::empty());
        let on = HostLanguageClassifier::new(
            registry,
            ProjectCapabilitySnapshot::from_capabilities([capability]),
        );
        assert_ne!(
            off.capability_hash(),
            on.capability_hash(),
            "a capability flip must change the classification cache key dimension"
        );
    }
}
