//! Host-level language classification: static registry × project
//! capabilities × framework admission.

use std::sync::Arc;

use verter_language::{CapabilityId, FileLanguage, LanguageRegistry, StaticClassification};

use super::options::FrameworkOptions;
use super::project_capabilities::ProjectCapabilitySnapshot;
use crate::types::Hash16;

/// The single classification authority for SESSION-level consumers.
///
/// Composes [`LanguageRegistry::classify_static`] (the pure leaf entry)
/// with the [`ProjectCapabilitySnapshot`]: a gated candidate row
/// resolves to its candidate language when the gating capability bit is
/// derived ON, and to its ungated fallback otherwise.
///
/// The third composition input is the host's [`FrameworkOptions`]: a
/// framework vertical the construction did not admit is invisible to
/// classification — its carrier extension falls through to the
/// plain-script catch-all and its adapter-module extensions classify as
/// plain scripts of the same dialect — and its rows leave the
/// carrier/adapter-module surface accessors, so the watch surface
/// (built from those accessors) cannot watch a framework the host
/// cannot serve.
///
/// FFI-time classification deliberately does NOT route through this
/// type: the FFI boundary is static-only (it cannot consult project
/// capabilities), so gated rows REQUIRE an explicit kind string there.
#[derive(Debug, Clone)]
pub struct HostLanguageClassifier {
    registry: Arc<LanguageRegistry>,
    capabilities: ProjectCapabilitySnapshot,
    framework: FrameworkOptions,
}

impl HostLanguageClassifier {
    /// Classifier over an explicit registry + capability snapshot.
    pub fn new(registry: Arc<LanguageRegistry>, capabilities: ProjectCapabilitySnapshot) -> Self {
        Self {
            registry,
            capabilities,
            framework: FrameworkOptions::default(),
        }
    }

    /// Classifier over an explicit registry + capability snapshot + typed
    /// framework options (the host-construction composition).
    pub fn with_options(
        registry: Arc<LanguageRegistry>,
        capabilities: ProjectCapabilitySnapshot,
        framework: FrameworkOptions,
    ) -> Self {
        Self {
            registry,
            capabilities,
            framework,
        }
    }

    /// Classifier over the built-in registry.
    pub fn with_built_in_registry(capabilities: ProjectCapabilitySnapshot) -> Self {
        Self::new(Arc::new(LanguageRegistry::built_in()), capabilities)
    }

    /// Classifier over the built-in registry under `options` — the
    /// composition host construction uses, so the classifier and the
    /// composed framework services admit the SAME verticals.
    pub fn with_built_in_registry_and_options(
        capabilities: ProjectCapabilitySnapshot,
        options: &FrameworkOptions,
    ) -> Self {
        Self::with_options(
            Arc::new(LanguageRegistry::built_in()),
            capabilities,
            options.clone(),
        )
    }

    /// Resolve a path to its [`FileLanguage`] row.
    pub fn classify(&self, path: &str) -> FileLanguage {
        let language = match self.registry.classify_static(path) {
            StaticClassification::Resolved(language) => language,
            StaticClassification::Gated(candidate) => {
                if self.capabilities.is_enabled(&candidate.capability) {
                    candidate.candidate
                } else {
                    candidate.fallback
                }
            }
            StaticClassification::Unknown => FileLanguage::script_ts(),
        };
        self.admission_resolved(language)
    }

    /// The admission projection of a classified row: a framework carrier
    /// or adapter module whose adapter is not admitted degrades to the
    /// routing an unregistered extension gets — the plain-script
    /// catch-all for a carrier, the same-dialect plain script for an
    /// adapter module. An unadmitted vertical never reaches a
    /// framework-aware path through classification.
    fn admission_resolved(&self, language: FileLanguage) -> FileLanguage {
        if let Some((adapter_id, _)) = language.adapter_script_language() {
            if !self.framework.admits(adapter_id) {
                return FileLanguage::script(
                    language
                        .script_source_type()
                        .expect("an adapter module carries a script source type"),
                );
            }
            return language;
        }
        if let Some(adapter_id) = language.adapter_id() {
            if !self.framework.admits(adapter_id) {
                return FileLanguage::script_ts();
            }
        }
        language
    }

    /// The ADMITTED framework-carrier row an editor `languageId` names
    /// (`"vue"` → the Vue carrier row), `None` for a non-carrier id or a
    /// carrier whose vertical this host does not admit.
    ///
    /// Editor ingress resolves a document's carrier here — never from a
    /// process-global registry — so a document whose client names an
    /// unadmitted carrier falls back to path classification, which routes
    /// it exactly like an unregistered extension.
    #[must_use]
    pub fn carrier_for_editor_language_id(&self, language_id: &str) -> Option<FileLanguage> {
        self.registry
            .carrier_for_editor_language_id(language_id)
            .filter(|language| {
                language
                    .adapter_id()
                    .is_none_or(|adapter_id| self.framework.admits(adapter_id))
            })
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

    /// The framework-carrier extensions THIS host's registry classifies AND
    /// admits, in registry order (longest suffix first).
    ///
    /// The host's own composition is the classification authority below the
    /// host seam: a watcher that needs the carrier surface reads it here, not
    /// from a process-global registry it never composed. An unadmitted
    /// vertical's extension is not a carrier this host claims — the watch
    /// surface narrows with the admission.
    #[must_use]
    pub fn carrier_extensions(&self) -> Vec<&str> {
        self.carrier_rows()
            .into_iter()
            .map(|(extension, _)| extension)
            .collect()
    }

    /// Every framework-carrier row THIS host's registry classifies AND
    /// admits, as `(extension, FileLanguage)` pairs, in registry order.
    ///
    /// The identity-bearing half of [`Self::carrier_extensions`]: a consumer
    /// that must know WHICH carrier an extension resolves to — not only which
    /// extensions are carriers — reads the rows here, so the watch surface can
    /// be checked against a composed framework catalog row by row.
    #[must_use]
    pub fn carrier_rows(&self) -> Vec<(&str, FileLanguage)> {
        self.registry
            .carrier_rows()
            .into_iter()
            .filter(|(_, language)| {
                language
                    .adapter_id()
                    .is_none_or(|adapter_id| self.framework.admits(adapter_id))
            })
            .collect()
    }

    /// The adapter-module extensions THIS host's registry classifies across
    /// every ADMITTED adapter, in registry order.
    #[must_use]
    pub fn adapter_module_extensions(&self) -> Vec<&str> {
        self.registry
            .all_adapter_module_extensions()
            .into_iter()
            .filter(|extension| {
                // Classification is the authority: a synthetic probe that
                // still resolves to an adapter module proves the owning
                // adapter is admitted; an unadmitted one degrades to a plain
                // script and is filtered out.
                let probe = format!("probe.{extension}");
                matches!(
                    self.classify(&probe),
                    FileLanguage::Script {
                        flavor: verter_language::ScriptFlavor::AdapterModule { .. },
                        ..
                    }
                )
            })
            .collect()
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
                LanguageRegistry::built_in()
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

    /// An unadmitted vertical is invisible to classification: its carrier
    /// extension falls through to the plain-script catch-all, its
    /// adapter-module extensions classify as same-dialect plain scripts,
    /// and both leave the carrier/adapter-module surface accessors — so
    /// the watch surface cannot watch a framework the host cannot serve.
    #[test]
    fn an_unadmitted_vertical_is_invisible_to_classification() {
        let vue_only =
            FrameworkOptions::admitting_names(["vue"]).expect("the Vue vertical is composed");
        let classifier = HostLanguageClassifier::with_built_in_registry_and_options(
            ProjectCapabilitySnapshot::empty(),
            &vue_only,
        );
        // The carrier falls through to the plain-script catch-all — the
        // same routing an extension with no row gets.
        assert_eq!(
            classifier.classify("/src/Box.svelte"),
            FileLanguage::script_ts()
        );
        // The adapter-module row degrades to a PLAIN script of the same
        // dialect — never a rune module of an unadmitted adapter.
        assert_eq!(
            classifier.classify("/src/store.svelte.ts"),
            FileLanguage::script(verter_language::ScriptSourceType::Ts),
            "an unadmitted adapter module classifies as a plain Ts script"
        );
        assert_eq!(
            classifier.classify("/src/store.svelte.js"),
            FileLanguage::script(verter_language::ScriptSourceType::js()),
            "an unadmitted adapter module classifies as a plain js script"
        );
        // An editor `languageId` naming the unadmitted carrier resolves to
        // no carrier row, so editor ingress falls back to path
        // classification instead of reaching the unadmitted carrier.
        assert_eq!(classifier.carrier_for_editor_language_id("svelte"), None);
        assert_eq!(
            classifier.carrier_for_editor_language_id("vue"),
            Some(FileLanguage::vue())
        );
        // The surface accessors narrow with the admission.
        assert_eq!(classifier.carrier_extensions(), vec!["vue"]);
        assert!(
            classifier.adapter_module_extensions().is_empty(),
            "an unadmitted adapter contributes no adapter-module extension"
        );
        // The admitted vertical keeps its rows.
        assert_eq!(classifier.classify("/src/App.vue"), FileLanguage::vue());
    }

    /// The default (admit-all) classifier keeps the historical surface:
    /// every built-in carrier and adapter-module row stays visible.
    #[test]
    fn the_default_classifier_keeps_every_built_in_row() {
        let classifier =
            HostLanguageClassifier::with_built_in_registry(ProjectCapabilitySnapshot::empty());
        assert_eq!(
            classifier.classify("/src/Box.svelte"),
            FileLanguage::svelte()
        );
        assert_eq!(
            classifier.classify("/src/store.svelte.ts"),
            FileLanguage::adapter_module(
                verter_language::ScriptSourceType::Ts,
                verter_language::FrameworkAdapterId::svelte(),
                verter_language::LanguageId::new(verter_language::SVELTE_RUNE_MODULE_LANGUAGE_ID),
            ),
            "an admitted adapter module keeps its rune-module flavor"
        );
        assert_eq!(
            classifier.carrier_for_editor_language_id("svelte"),
            Some(FileLanguage::svelte())
        );
        assert_eq!(
            classifier.carrier_for_editor_language_id("typescript"),
            None
        );
        let mut extensions = classifier.carrier_extensions();
        extensions.sort_unstable();
        assert_eq!(extensions, vec!["svelte", "vue"]);
        let mut modules = classifier.adapter_module_extensions();
        modules.sort_unstable();
        assert_eq!(modules, vec!["svelte.js", "svelte.ts"]);
    }

    /// A gated row whose CANDIDATE belongs to an unadmitted adapter
    /// resolves to its fallback even with the capability derived ON —
    /// the admission projection applies after the capability resolution,
    /// so an unadmitted template cannot reach a framework-aware path.
    #[test]
    fn an_unadmitted_gated_candidate_resolves_to_its_fallback() {
        let (registry, capability, _) = gated_registry();
        let fixture_only =
            FrameworkOptions::admitting_names(["vue"]).expect("the Vue vertical is composed");
        let classifier = HostLanguageClassifier::with_options(
            registry,
            ProjectCapabilitySnapshot::from_capabilities([capability]),
            fixture_only,
        );
        assert_eq!(
            classifier.classify("/src/page.html"),
            FileLanguage::script_ts(),
            "the gated candidate's unadmitted adapter degrades to the fallback routing"
        );
    }
}
