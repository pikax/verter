//! The compiler-side carrier-compiler registry.
//!
//! The host's carrier dispatch (`host_executor`) interns a file's
//! resolved [`FrameworkAdapterId`] and looks the carrier compiler up
//! here. The registry is built ONCE and is immutable thereafter — the
//! compiler-domain mirror of the session-side `FrameworkAdapterRegistry`
//! (which owns the carrier ACCESS token + the semantic legs); this one
//! owns the carrier COMPILER (parse / eval / IDE / template) per adapter.
//!
//! Completeness is the split-out `carrier_descriptors_have_compilers`
//! leg of B5's `framework_registry_complete`: every carrier-bearing
//! session descriptor MUST have a registered compiler here (Vue-through-
//! the-bridge satisfies it). The session-side guard asserts this against
//! the live registry.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_language::FrameworkAdapterId;

use super::carrier_compiler::CarrierCompiler;
use super::vue_bridge::VueCarrierCompiler;
use crate::svelte::SvelteCarrierCompiler;

/// The compiler-side carrier-compiler registry.
///
/// Owns one [`CarrierCompiler`] per registered adapter id. Built once via
/// [`Self::built_in`]; immutable thereafter.
#[derive(Clone)]
pub struct CarrierCompilerRegistry {
    compilers: FxHashMap<FrameworkAdapterId, Arc<dyn CarrierCompiler>>,
}

impl std::fmt::Debug for CarrierCompilerRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CarrierCompilerRegistry")
            .field("compilers", &self.compilers.len())
            .finish()
    }
}

impl CarrierCompilerRegistry {
    /// Build the registry with the production carrier compilers.
    ///
    /// Vue is the keystone carrier: its compiler is the bridge around the
    /// existing Vue pipeline. Svelte is the second carrier (its parser produces
    /// the framework-neutral artifact through the Svelte bridge). A new carrier
    /// vertical adds its row here.
    #[must_use]
    pub fn built_in() -> Self {
        let mut compilers: FxHashMap<FrameworkAdapterId, Arc<dyn CarrierCompiler>> =
            FxHashMap::default();
        Self::register(&mut compilers, Arc::new(VueCarrierCompiler::default()));
        Self::register(&mut compilers, Arc::new(SvelteCarrierCompiler::default()));
        Self { compilers }
    }

    /// Build a registry from explicit compiler rows. Used by the in-tree
    /// `CarrierCompiler` contract tests (a fixture compiler).
    #[must_use]
    pub fn from_compilers(compilers: impl IntoIterator<Item = Arc<dyn CarrierCompiler>>) -> Self {
        let mut map: FxHashMap<FrameworkAdapterId, Arc<dyn CarrierCompiler>> = FxHashMap::default();
        for compiler in compilers {
            Self::register(&mut map, compiler);
        }
        Self { compilers: map }
    }

    /// Insert a compiler under its own adapter id (its registration key).
    fn register(
        map: &mut FxHashMap<FrameworkAdapterId, Arc<dyn CarrierCompiler>>,
        compiler: Arc<dyn CarrierCompiler>,
    ) {
        let id = compiler.adapter_id();
        map.insert(id, compiler);
    }

    /// The carrier compiler for `adapter_id`, if one is registered.
    #[must_use]
    pub fn get(&self, adapter_id: &FrameworkAdapterId) -> Option<&Arc<dyn CarrierCompiler>> {
        self.compilers.get(adapter_id)
    }

    /// Whether `adapter_id` has a registered carrier compiler.
    #[must_use]
    pub fn contains(&self, adapter_id: &FrameworkAdapterId) -> bool {
        self.compilers.contains_key(adapter_id)
    }

    /// The carrier compiler that serves the carrier row `(adapter_id,
    /// carrier_language_id)`, or `None`.
    ///
    /// Returns `Some` ONLY when `adapter_id` is registered AND the
    /// registered compiler's [`CarrierCompiler::carrier_language_id`]
    /// equals `carrier_language_id`. A same-adapter NON-carrier row (an
    /// external template, a second adapter language) resolves to `None` —
    /// the row is the typed unsupported-language state, never routed
    /// through the SFC parse path by adapter id alone.
    #[must_use]
    pub fn compiler_for_carrier_language(
        &self,
        adapter_id: &FrameworkAdapterId,
        carrier_language_id: &verter_language::LanguageId,
    ) -> Option<&Arc<dyn CarrierCompiler>> {
        let compiler = self.compilers.get(adapter_id)?;
        (compiler.carrier_language_id() == *carrier_language_id).then_some(compiler)
    }

    /// Every registered adapter id, sorted for deterministic iteration.
    #[must_use]
    pub fn registered_adapter_ids(&self) -> Vec<FrameworkAdapterId> {
        let mut ids: Vec<FrameworkAdapterId> = self.compilers.keys().cloned().collect();
        ids.sort();
        ids
    }
}

impl Default for CarrierCompilerRegistry {
    fn default() -> Self {
        Self::built_in()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_registry_registers_the_vue_carrier_compiler() {
        let registry = CarrierCompilerRegistry::built_in();
        assert!(
            registry.contains(&FrameworkAdapterId::vue()),
            "the built-in registry must register the Vue carrier compiler (the bridge)"
        );
        let vue = registry
            .get(&FrameworkAdapterId::vue())
            .expect("Vue compiler registered");
        assert!(vue.adapter_id().is_vue());
    }

    #[test]
    fn unregistered_adapter_has_no_compiler() {
        let registry = CarrierCompilerRegistry::built_in();
        assert!(
            registry
                .get(&FrameworkAdapterId::new("not-a-framework"))
                .is_none(),
            "an unregistered adapter id resolves to no compiler"
        );
    }

    #[test]
    fn registered_adapter_ids_are_sorted_and_contain_vue() {
        let registry = CarrierCompilerRegistry::built_in();
        let ids = registry.registered_adapter_ids();
        assert!(ids.contains(&FrameworkAdapterId::vue()));
        // Sorted: a window comparison holds for any future multi-row build.
        assert!(ids.windows(2).all(|w| w[0] <= w[1]), "ids must be sorted");
    }

    #[test]
    fn compiler_for_carrier_language_matches_only_the_carrier_language() {
        use verter_language::LanguageId;
        let registry = CarrierCompilerRegistry::built_in();
        let vue = FrameworkAdapterId::vue();

        // The Vue carrier language (`vue`) resolves to the Vue compiler.
        assert!(
            registry
                .compiler_for_carrier_language(&vue, &LanguageId::new("vue"))
                .is_some(),
            "the vue carrier language resolves to the Vue compiler"
        );

        // A SAME-ADAPTER non-carrier language (e.g. an external Vue
        // template) does NOT resolve to the compiler — dispatch is keyed on
        // the FULL (adapter, carrier language) row, never adapter id alone.
        assert!(
            registry
                .compiler_for_carrier_language(&vue, &LanguageId::new("vue_template"))
                .is_none(),
            "a same-adapter non-carrier language must NOT dispatch through the SFC parse path"
        );

        // The Svelte carrier language (`svelte`) resolves to the Svelte
        // compiler — the second registered carrier.
        assert!(
            registry
                .compiler_for_carrier_language(
                    &FrameworkAdapterId::svelte(),
                    &LanguageId::new("svelte")
                )
                .is_some(),
            "the svelte carrier language resolves to the Svelte compiler"
        );

        // A truly unregistered adapter resolves to nothing.
        assert!(
            registry
                .compiler_for_carrier_language(
                    &FrameworkAdapterId::new("not-a-framework"),
                    &LanguageId::new("not-a-framework")
                )
                .is_none(),
            "an unregistered adapter has no carrier compiler"
        );
    }
}
