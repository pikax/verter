#![deny(missing_docs)]
//! The framework adapter registry — the single hub binding each framework's
//! descriptor, carrier leg, synthesis leg, public-API projector, script-fact
//! providers, and surface-resolution disposition.
//!
//! The registry is built ONCE at `VerterHost` construction and is the executor's
//! lookup authority: the wire `framework_adapter_id` a client selects interns to
//! a [`FrameworkAdapterId`], the registry resolves it to a
//! [`FrameworkRegistration`], and the executor drives that registration's legs.
//!
//! Completeness is a closed-set invariant ([`framework_registry_complete`]):
//! every wire [`FrameworkTag`] maps to a registered adapter OR an explicit
//! [`TagDisposition`] row (a deferred vertical or an out-of-scope framework).
//! An unregistered tag is NOT a fabricated registration — the disposition table
//! records the absence explicitly so a new wire tag cannot slip in unhandled.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_language::{CarrierAccessToken, FrameworkAdapterId, LanguageId};
use verter_protocol::typeinfo::graph::FrameworkTag;

use crate::framework::api_projector::ComponentApiProjector;
use crate::framework::surface_store::ErasedFrameworkSurfaceStore;
use crate::framework::synth::ComponentDefaultSynth;
use crate::typeinfo::framework_surface::FrameworkSurfaceAdapter;
use verter_semantic::analysis::framework_facts::{ScriptFactProvider, ScriptFactSyntaxGate};

/// One framework's carrier leg — the received carrier registration proof.
///
/// The token is RECEIVED from `verter_language`'s carrier-row registration
/// channel, never minted here (`verter_language` is the sole minting
/// authority). A carrier-less adapter has no leg (`carrier: None`).
#[derive(Debug, Clone)]
pub struct CarrierLeg {
    /// The adapter's carrier registration proof.
    pub token: CarrierAccessToken,
}

/// How an adapter resolves its component surfaces.
///
/// CLOSED two-arm taxonomy: an adapter EITHER ships a plan/normalize
/// [`FrameworkSurfaceAdapter`] ([`Self::Adapter`]) OR registers as
/// [`Self::Deferred`] — a framework whose adapter id is registered but whose
/// surface resolution is not yet implemented (the executor answers every kind
/// structurally UNSUPPORTED for a `Deferred` row). No third arm exists.
pub enum SurfaceRegistration {
    /// A plan/normalize adapter resolves this framework's surfaces.
    Adapter(Arc<dyn FrameworkSurfaceAdapter>),
    /// The adapter id is registered but surface resolution is not yet wired.
    Deferred,
}

impl std::fmt::Debug for SurfaceRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SurfaceRegistration::Adapter(_) => f.write_str("SurfaceRegistration::Adapter"),
            SurfaceRegistration::Deferred => f.write_str("SurfaceRegistration::Deferred"),
        }
    }
}

/// One framework adapter's full registration row.
///
/// Binds the adapter's descriptor to its optional carrier leg, its optional
/// synthesis leg, its optional public-API projector, its script-fact providers,
/// and its surface disposition. Every leg is `Option` (a framework need not
/// supply every capability); `script_fact_providers` is empty for Vue (its macro
/// analysis stays in the shallow pass) and carries one provider for Svelte.
pub struct FrameworkRegistration {
    /// The adapter's static descriptor row.
    pub descriptor: crate::framework::descriptor::FrameworkAdapterDescriptor,
    /// The carrier leg, when the adapter is carrier-backed.
    pub carrier: Option<CarrierLeg>,
    /// The synthesized-default leg, when the adapter synthesizes a `default`.
    pub synth: Option<Arc<dyn ComponentDefaultSynth>>,
    /// The public-API projector leg, when the adapter projects a public-API
    /// virtual file.
    pub api_projector: Option<Arc<dyn ComponentApiProjector>>,
    /// The adapter's syntax-capture script-fact providers (empty for Vue; the
    /// Svelte carrier registers one).
    pub script_fact_providers: Vec<Arc<dyn ScriptFactProvider>>,
    /// How the adapter resolves its component surfaces.
    pub surface: SurfaceRegistration,
    /// The adapter's erased surface-DTO store (one downcast at acquisition by
    /// the owning adapter's executor delegate).
    pub surface_store: Arc<dyn ErasedFrameworkSurfaceStore>,
}

impl std::fmt::Debug for FrameworkRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameworkRegistration")
            .field("descriptor", &self.descriptor)
            .field("carrier", &self.carrier)
            .field("synth", &self.synth.is_some())
            .field("api_projector", &self.api_projector.is_some())
            .field("script_fact_providers", &self.script_fact_providers.len())
            .field("surface", &self.surface)
            .finish_non_exhaustive()
    }
}

/// The disposition of a wire [`FrameworkTag`] the registry walks for
/// completeness.
///
/// A tag is EITHER backed by a registered adapter ([`Self::Registered`]) OR
/// explicitly absent — a [`Self::DeferredVertical`] (its adapter id registers in
/// a later framework vertical) or an [`Self::OutOfScope`] framework (no adapter
/// is planned). The structural non-tags (`NONE` / `OPEN_CANONICAL`) are handled
/// by the completeness guard directly, not through this table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagDisposition {
    /// The tag is backed by a registered adapter id.
    Registered(FrameworkAdapterId),
    /// The tag's adapter is a deferred vertical (registers later).
    DeferredVertical,
    /// The tag's framework is out of scope (no adapter planned).
    OutOfScope,
}

/// The session-side active-provider index — the resolved-validation half's
/// gate-keyed lookup of which script-fact providers are active for a file.
///
/// Rebuilt ONCE per registry construction (a registry rebuild on a
/// capability-snapshot change re-derives it). The two maps mirror the closed
/// [`ScriptFactSyntaxGate`] arms: a file's active set is the union of the
/// providers whose carrier-language gate matches the file's carrier language
/// and the providers whose import-specifier gate matches one of the file's
/// imports.
///
/// EMPTY (when no carrier registers a provider) is a zero-cost fast path:
/// [`Self::is_empty`] short-circuits before any per-file lookup. The Svelte
/// carrier registers one provider (carrier-language gated on `svelte`), so the
/// index is non-empty — but a NON-Svelte file (e.g. a `.vue`) still selects zero
/// providers, keeping its path byte-identical.
#[derive(Default, Clone)]
pub struct ActiveProviderIndex {
    by_carrier_language: FxHashMap<LanguageId, Vec<Arc<dyn ScriptFactProvider>>>,
    by_import_specifier: FxHashMap<&'static str, Vec<Arc<dyn ScriptFactProvider>>>,
}

impl std::fmt::Debug for ActiveProviderIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActiveProviderIndex")
            .field("by_carrier_language", &self.by_carrier_language.len())
            .field("by_import_specifier", &self.by_import_specifier.len())
            .finish()
    }
}

impl ActiveProviderIndex {
    /// Build the index from every registration's script-fact providers.
    ///
    /// Each provider is filed under its exact-valued [`ScriptFactSyntaxGate`].
    /// With no provider registered the index is empty (the zero-cost path).
    #[must_use]
    pub fn from_registry(registry: &FrameworkAdapterRegistry) -> Self {
        let mut by_carrier_language: FxHashMap<LanguageId, Vec<Arc<dyn ScriptFactProvider>>> =
            FxHashMap::default();
        let mut by_import_specifier: FxHashMap<&'static str, Vec<Arc<dyn ScriptFactProvider>>> =
            FxHashMap::default();
        for registration in registry.registrations.values() {
            for provider in &registration.script_fact_providers {
                match provider.syntax_gate() {
                    ScriptFactSyntaxGate::CarrierLanguage(language) => {
                        by_carrier_language
                            .entry(language)
                            .or_default()
                            .push(Arc::clone(provider));
                    }
                    ScriptFactSyntaxGate::ImportSpecifier(specifier) => {
                        by_import_specifier
                            .entry(specifier)
                            .or_default()
                            .push(Arc::clone(provider));
                    }
                }
            }
        }
        Self {
            by_carrier_language,
            by_import_specifier,
        }
    }

    /// Whether no provider is indexed — the zero-cost fast path. When true the
    /// resolved-validation half does no per-file work at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_carrier_language.is_empty() && self.by_import_specifier.is_empty()
    }

    /// The providers active for a file with carrier language `carrier_language`
    /// and importing `import_specifiers`.
    ///
    /// A provider gated on a carrier language is active when the file's carrier
    /// language matches; a provider gated on an import specifier is active when
    /// the file imports that specifier. A provider already matched by carrier
    /// language is not double-counted by an import-specifier match.
    #[must_use]
    pub fn active_for<'s, I>(
        &self,
        carrier_language: Option<&LanguageId>,
        import_specifiers: I,
    ) -> Vec<Arc<dyn ScriptFactProvider>>
    where
        I: IntoIterator<Item = &'s str>,
    {
        if self.is_empty() {
            return Vec::new();
        }
        let mut active: Vec<Arc<dyn ScriptFactProvider>> = Vec::new();
        let mut seen: Vec<FrameworkAdapterId> = Vec::new();
        if let Some(language) = carrier_language {
            if let Some(providers) = self.by_carrier_language.get(language) {
                for provider in providers {
                    seen.push(provider.adapter_id());
                    active.push(Arc::clone(provider));
                }
            }
        }
        for specifier in import_specifiers {
            if let Some(providers) = self.by_import_specifier.get(specifier) {
                for provider in providers {
                    let id = provider.adapter_id();
                    if seen.contains(&id) {
                        continue;
                    }
                    seen.push(id);
                    active.push(Arc::clone(provider));
                }
            }
        }
        active
    }

    /// Whether a provider's exact-valued [`ScriptFactSyntaxGate`] is active for
    /// a file with carrier language `carrier_language` importing
    /// `import_specifiers`.
    ///
    /// This is the SHARED gate-matching authority: [`Self::active_for`] selects
    /// through the gate-keyed maps it is built from, and the resolved-validation
    /// half's per-registration selection applies this exact predicate over a
    /// registration's own providers — the two agree by construction.
    #[must_use]
    pub fn gate_matches<'s, I>(
        gate: &ScriptFactSyntaxGate,
        carrier_language: Option<&LanguageId>,
        import_specifiers: I,
    ) -> bool
    where
        I: IntoIterator<Item = &'s str>,
    {
        match gate {
            ScriptFactSyntaxGate::CarrierLanguage(language) => carrier_language == Some(language),
            ScriptFactSyntaxGate::ImportSpecifier(specifier) => {
                import_specifiers.into_iter().any(|s| s == *specifier)
            }
        }
    }
}

/// The framework adapter registry.
///
/// Owns one [`FrameworkRegistration`] per registered adapter id. Built once at
/// host construction; immutable thereafter (a normalizer change is a registry
/// rebuild, not an in-place mutation).
#[derive(Debug)]
pub struct FrameworkAdapterRegistry {
    registrations: FxHashMap<FrameworkAdapterId, FrameworkRegistration>,
    active_provider_index: ActiveProviderIndex,
}

impl FrameworkAdapterRegistry {
    /// Build the registry with the production adapter rows.
    ///
    /// The carrier tokens are the carrier registration proofs RECEIVED from
    /// `verter_language`'s carrier-row channel (cloned from the blessed carrier
    /// accessors' held tokens — the same minted values, never a second mint).
    #[must_use]
    pub fn built_in(
        vue_carrier_token: CarrierAccessToken,
        svelte_carrier_token: CarrierAccessToken,
    ) -> Self {
        let mut registrations = FxHashMap::default();
        registrations.insert(
            FrameworkAdapterId::vue(),
            vue_registration(vue_carrier_token),
        );
        registrations.insert(
            FrameworkAdapterId::svelte(),
            svelte_registration(svelte_carrier_token),
        );
        Self::finish(registrations)
    }

    /// Build a registry from explicit registration rows. Used by the in-tree
    /// fixture registration the completeness/deferred/script-fact tests
    /// exercise.
    #[must_use]
    pub fn from_registrations(
        registrations: impl IntoIterator<Item = (FrameworkAdapterId, FrameworkRegistration)>,
    ) -> Self {
        Self::finish(registrations.into_iter().collect())
    }

    /// Seal a registration map into a registry, deriving the active-provider
    /// index once. The index is the ONLY derived state — registrations are
    /// immutable thereafter.
    fn finish(registrations: FxHashMap<FrameworkAdapterId, FrameworkRegistration>) -> Self {
        let mut registry = Self {
            registrations,
            active_provider_index: ActiveProviderIndex::default(),
        };
        registry.active_provider_index = ActiveProviderIndex::from_registry(&registry);
        registry
    }

    /// The active-provider index derived from this registry's script-fact
    /// providers (the resolved-validation half's per-file lookup). The Svelte
    /// carrier contributes one carrier-language-gated provider; Vue contributes
    /// none (its macro analysis stays in the shallow pass).
    #[must_use]
    pub fn active_provider_index(&self) -> &ActiveProviderIndex {
        &self.active_provider_index
    }

    /// The registration for `adapter_id`, if one is registered.
    #[must_use]
    pub fn get(&self, adapter_id: &FrameworkAdapterId) -> Option<&FrameworkRegistration> {
        self.registrations.get(adapter_id)
    }

    /// Whether `adapter_id` is registered.
    #[must_use]
    pub fn contains(&self, adapter_id: &FrameworkAdapterId) -> bool {
        self.registrations.contains_key(adapter_id)
    }

    /// Whether ANY registration carries a syntax-capture script-fact provider —
    /// the registry-wide oracle the [`ActiveProviderIndex`] emptiness mirrors
    /// (the index is empty IFF this is `false`).
    #[must_use]
    pub fn any_provider_registered(&self) -> bool {
        self.registrations
            .values()
            .any(|r| !r.script_fact_providers.is_empty())
    }

    /// Every registered adapter's descriptor, in adapter-id order. The
    /// compiler-completeness guard iterates these to assert every
    /// carrier-bearing descriptor has a registered `CarrierCompiler`.
    #[must_use]
    pub fn descriptors(&self) -> Vec<crate::framework::descriptor::FrameworkAdapterDescriptor> {
        let mut rows: Vec<_> = self
            .registrations
            .values()
            .map(|r| r.descriptor.clone())
            .collect();
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        rows
    }

    /// The disposition of a wire framework tag — the completeness oracle.
    ///
    /// A registered tag resolves to [`TagDisposition::Registered`] with its
    /// adapter id; the deferred/out-of-scope tags resolve to their explicit
    /// disposition. The structural non-tags (`NONE` / `OPEN_CANONICAL`) have no
    /// disposition (they are not framework-adapter tags) and return `None`.
    #[must_use]
    pub fn tag_disposition(&self, tag: FrameworkTag) -> Option<TagDisposition> {
        match tag {
            FrameworkTag::Vue => {
                let id = FrameworkAdapterId::vue();
                if self.contains(&id) {
                    Some(TagDisposition::Registered(id))
                } else {
                    // Vue is the keystone adapter; its absence is a build defect,
                    // not a deferred vertical.
                    None
                }
            }
            // Svelte's adapter id is registered with all legs, including the
            // real `SvelteFrameworkAdapter` surface arm; the completeness oracle
            // keys on registration.
            FrameworkTag::Svelte => {
                let id = FrameworkAdapterId::svelte();
                if self.contains(&id) {
                    Some(TagDisposition::Registered(id))
                } else {
                    Some(TagDisposition::DeferredVertical)
                }
            }
            // React / Solid are out of scope (no adapter planned).
            FrameworkTag::React | FrameworkTag::Solid => Some(TagDisposition::OutOfScope),
            // The structural non-tags are not framework-adapter tags.
            FrameworkTag::None | FrameworkTag::OpenCanonical => None,
        }
    }
}

/// The synthesis leg the host's neutral synth-injection selector reaches.
///
/// Selects the registered adapter for `adapter_id` and hands back its
/// [`ComponentDefaultSynth`] leg, or `None` when the adapter has no synth leg.
impl FrameworkAdapterRegistry {
    /// The synthesized-default leg for `adapter_id`, if the adapter registers
    /// one.
    #[must_use]
    pub fn synth_for(
        &self,
        adapter_id: &FrameworkAdapterId,
    ) -> Option<&Arc<dyn ComponentDefaultSynth>> {
        self.get(adapter_id).and_then(|r| r.synth.as_ref())
    }

    /// The public-API projector leg for `adapter_id`, if the adapter registers
    /// one.
    #[must_use]
    pub fn api_projector_for(
        &self,
        adapter_id: &FrameworkAdapterId,
    ) -> Option<&Arc<dyn ComponentApiProjector>> {
        self.get(adapter_id).and_then(|r| r.api_projector.as_ref())
    }

    /// The adapter id of the unique registered adapter that SYNTHESIZES a
    /// `default` component value (carries a [`ComponentDefaultSynth`] leg).
    ///
    /// Exactly one adapter registers a synth leg when Vue is the only carrier;
    /// `None` when none does. Deterministic: when more than one registers, the
    /// lowest adapter id wins so the selection is map-order-independent. The
    /// scratch-injection path uses [`Self::scratch_synthesizing_adapter_id`]
    /// instead — it needs the SPECIFIC carrier-macro inliner, not an arbitrary
    /// `.min()` across every synth-bearing adapter.
    #[must_use]
    pub fn synthesizing_adapter_id(&self) -> Option<FrameworkAdapterId> {
        self.registrations
            .iter()
            .filter(|(_, registration)| registration.synth.is_some())
            .map(|(id, _)| id.clone())
            .min()
    }

    /// The adapter id whose synth leg fabricates the `default` for a typeinfo
    /// EVALUATION SCRATCH (`verter://typeinfo/…`).
    ///
    /// A scratch inlines a `.vue` scope's eval-source as a `.vue`-MACRO prelude
    /// and classifies by its own `.ts` suffix — it has NO resolved framework
    /// language. The macro surface it inlines is Vue's, so the scratch routes to
    /// the VUE synth leg specifically (the carrier-MACRO inliner). This is
    /// REGISTRY DATA (the registered Vue adapter id), not a hardcoded literal,
    /// and not an arbitrary `.min()` over every synth adapter — a `.min()` would
    /// (mis)route the Vue-macro scratch to Svelte once Svelte registers a synth
    /// leg (`"svelte" < "vue"`). `None` when Vue registers no synth leg.
    #[must_use]
    pub fn scratch_synthesizing_adapter_id(&self) -> Option<FrameworkAdapterId> {
        let vue = FrameworkAdapterId::vue();
        self.synth_for(&vue).map(|_| vue)
    }
}

/// The Vue adapter registration row.
fn vue_registration(carrier_token: CarrierAccessToken) -> FrameworkRegistration {
    let store: Arc<dyn ErasedFrameworkSurfaceStore> =
        Arc::new(crate::framework::surface_store::FrameworkSurfaceStore::<
            crate::typeinfo::framework_surface::VueSurfaceKey,
            crate::typeinfo::framework_surface::MacroSurfaceDtos,
        >::new());
    FrameworkRegistration {
        descriptor: crate::framework::descriptor::vue_descriptor(),
        carrier: Some(CarrierLeg {
            token: carrier_token,
        }),
        synth: Some(Arc::new(crate::framework::synth::VueComponentDefaultSynth)),
        api_projector: Some(Arc::new(
            crate::framework::api_projectors::VueComponentApiProjector,
        )),
        script_fact_providers: Vec::new(),
        surface: SurfaceRegistration::Adapter(Arc::new(
            crate::typeinfo::adapters::vue::adapter::VueFrameworkAdapter::default(),
        )),
        surface_store: store,
    }
}

/// The Svelte adapter registration row.
///
/// Registers ALL legs — carrier + synth + script-fact provider + api projector +
/// the real `SvelteFrameworkAdapter` SURFACE leg. The surface store is keyed by
/// the Svelte adapter remainder ([`SvelteSurfaceKey`](crate::typeinfo::framework_surface::SvelteSurfaceKey)
/// — one source family per row).
fn svelte_registration(carrier_token: CarrierAccessToken) -> FrameworkRegistration {
    let store: Arc<dyn ErasedFrameworkSurfaceStore> =
        Arc::new(crate::framework::surface_store::FrameworkSurfaceStore::<
            crate::typeinfo::framework_surface::SvelteSurfaceKey,
            crate::typeinfo::framework_surface::MacroSurfaceDtos,
        >::new());
    FrameworkRegistration {
        descriptor: crate::framework::descriptor::svelte_descriptor(),
        carrier: Some(CarrierLeg {
            token: carrier_token,
        }),
        synth: Some(Arc::new(
            crate::framework::synth::SvelteComponentDefaultSynth,
        )),
        api_projector: Some(Arc::new(
            crate::framework::api_projectors::SvelteComponentApiProjector,
        )),
        script_fact_providers: vec![Arc::new(
            verter_semantic::analysis::framework_facts::svelte::SvelteScriptProvider,
        )],
        surface: SurfaceRegistration::Adapter(Arc::new(
            crate::typeinfo::adapters::svelte::adapter::SvelteFrameworkAdapter::default(),
        )),
        surface_store: store,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn built_in() -> FrameworkAdapterRegistry {
        FrameworkAdapterRegistry::built_in(
            crate::typeinfo::adapters::vue::vue_carrier_token_clone(),
            crate::typeinfo::adapters::svelte::svelte_carrier_token_clone(),
        )
    }

    /// COMPLETENESS GUARD: every wire framework tag maps to a registered
    /// adapter OR an explicit deferred/out-of-scope disposition; the structural
    /// non-tags are handled explicitly. A new wire tag with no disposition fails
    /// this (the closed-set invariant).
    #[test]
    fn framework_registry_complete() {
        let registry = built_in();
        // Walk EVERY wire tag — adding a tag forces a disposition decision here.
        for tag in [
            FrameworkTag::None,
            FrameworkTag::Vue,
            FrameworkTag::Svelte,
            FrameworkTag::React,
            FrameworkTag::Solid,
            FrameworkTag::OpenCanonical,
        ] {
            let disposition = registry.tag_disposition(tag);
            match tag {
                // Vue is registered.
                FrameworkTag::Vue => {
                    assert_eq!(
                        disposition,
                        Some(TagDisposition::Registered(FrameworkAdapterId::vue())),
                        "Vue must resolve to a registered adapter"
                    );
                }
                // Svelte's adapter id is REGISTERED with all legs, including the
                // real surface adapter. The completeness oracle keys on
                // registration.
                FrameworkTag::Svelte => {
                    assert_eq!(
                        disposition,
                        Some(TagDisposition::Registered(FrameworkAdapterId::svelte())),
                        "Svelte's adapter id is registered with all legs, including its \
                         surface adapter"
                    );
                }
                // React / Solid are out of scope.
                FrameworkTag::React | FrameworkTag::Solid => {
                    assert_eq!(disposition, Some(TagDisposition::OutOfScope));
                }
                // The structural non-tags have no disposition.
                FrameworkTag::None | FrameworkTag::OpenCanonical => {
                    assert_eq!(
                        disposition, None,
                        "{tag:?} is a structural non-tag, not a framework-adapter tag"
                    );
                }
            }
        }
    }

    /// API-LEG CLAUSE: a descriptor whose import surface is a distinct
    /// suffix-appended API file (it projects a public-API virtual file) MUST
    /// register a public-API projector leg.
    #[test]
    fn descriptor_with_api_suffix_has_a_projector_leg() {
        let registry = built_in();
        for registration in registry.registrations.values() {
            let projects_api = registration
                .descriptor
                .virtual_file_naming
                .as_ref()
                .and_then(|n| n.api_surface_suffix())
                .is_some();
            if projects_api {
                assert!(
                    registration.api_projector.is_some(),
                    "adapter {} projects an API virtual file (api_suffix Some) so it MUST \
                     register an api_projector leg",
                    registration.descriptor.id
                );
            }
        }
    }

    #[test]
    fn vue_registration_carries_every_leg() {
        let registry = built_in();
        let vue = registry
            .get(&FrameworkAdapterId::vue())
            .expect("the Vue adapter is registered");
        assert!(vue.carrier.is_some(), "Vue is carrier-backed");
        assert!(vue.synth.is_some(), "Vue synthesizes a default");
        assert!(vue.api_projector.is_some(), "Vue projects a public API");
        assert!(
            matches!(vue.surface, SurfaceRegistration::Adapter(_)),
            "Vue ships a plan/normalize adapter"
        );
        assert!(
            vue.script_fact_providers.is_empty(),
            "Vue registers no script-fact provider (its macro analysis stays in the shallow pass)"
        );
    }

    #[test]
    fn synth_for_selects_the_vue_leg() {
        let registry = built_in();
        assert!(
            registry.synth_for(&FrameworkAdapterId::vue()).is_some(),
            "the Vue synth leg is reachable by adapter id"
        );
        assert!(
            registry
                .synth_for(&FrameworkAdapterId::new("unregistered"))
                .is_none(),
            "an unregistered adapter id has no synth leg"
        );
    }

    #[test]
    fn scratch_synthesizing_adapter_id_is_vue_the_macro_inliner() {
        // The host's typeinfo-scratch default-injection routes a no-language
        // scratch canonical to the framework whose MACRO surface it inlines —
        // Vue (a scratch inlines a `.vue` macro prelude). That id is
        // REGISTRY-DERIVED (the registered Vue adapter id), NOT a `.min()` over
        // every synth adapter: with Svelte now ALSO registering a synth leg, a
        // `.min()` would (mis)route the Vue-macro scratch to Svelte
        // (`"svelte" < "vue"`).
        let registry = built_in();
        assert_eq!(
            registry.scratch_synthesizing_adapter_id(),
            Some(FrameworkAdapterId::vue()),
            "the typeinfo scratch inlines a Vue macro prelude, so it routes to \
             the Vue synth leg specifically"
        );
        // The generic `.min()` selector now returns Svelte (the lowest synth
        // adapter id) — DISCRIMINATING: it proves the scratch path uses the
        // dedicated selector, not `.min()`, since the two now differ.
        assert_eq!(
            registry.synthesizing_adapter_id(),
            Some(FrameworkAdapterId::svelte()),
            "with two synth adapters the `.min()` selector is Svelte, distinct \
             from the scratch path's Vue routing"
        );
    }

    #[test]
    fn synthesizing_adapter_id_is_none_without_a_synth_leg() {
        // A registry whose adapters carry no synth leg has no synthesizing
        // adapter — the scratch injection no-ops rather than fabricating an id.
        let registry = FrameworkAdapterRegistry::from_registrations([(
            crate::framework::script_facts::fixtures::fixture_adapter_id(),
            crate::framework::script_facts::fixtures::carrier_gated_fixture_registration(),
        )]);
        assert_eq!(
            registry.synthesizing_adapter_id(),
            None,
            "the fixture adapter registers no synth leg, so there is no \
             synthesizing adapter"
        );
    }

    #[test]
    fn built_in_active_provider_index_gates_svelte_only() {
        // The Svelte carrier registers a syntax-capture script-fact provider
        // (carrier-language gated on `svelte`); Vue registers none (its macro
        // analysis stays in the shallow pass). So the index is NON-empty but a
        // Vue file selects ZERO providers — the Vue path stays byte-identical
        // zero-cost.
        let registry = built_in();
        assert!(
            !registry.active_provider_index().is_empty(),
            "the Svelte carrier registers a script-fact provider"
        );
        // A `.vue` file's carrier language selects no provider (Vue is
        // provider-less).
        assert!(
            registry
                .active_provider_index()
                .active_for(Some(&LanguageId::new("vue")), std::iter::empty())
                .is_empty(),
            "a Vue file selects no provider — the Vue path is unchanged"
        );
        // A `.svelte` file's carrier language selects the Svelte provider.
        let active = registry
            .active_provider_index()
            .active_for(Some(&LanguageId::new("svelte")), std::iter::empty());
        assert_eq!(
            active.len(),
            1,
            "a Svelte file selects its one syntax-capture provider"
        );
        assert_eq!(active[0].adapter_id(), FrameworkAdapterId::svelte());
    }

    #[test]
    fn svelte_registration_carries_all_legs_and_a_real_surface_adapter() {
        // The Svelte carrier registers carrier + synth + script-fact provider +
        // api-projector legs PLUS the real `SvelteFrameworkAdapter` SURFACE arm
        // (the executor resolves Svelte surfaces, no longer a Deferred stub).
        let registry = built_in();
        let svelte = registry
            .get(&FrameworkAdapterId::svelte())
            .expect("Svelte is registered");
        assert!(svelte.carrier.is_some(), "Svelte is carrier-backed");
        assert!(svelte.synth.is_some(), "Svelte synthesizes a default");
        assert!(
            svelte.api_projector.is_some(),
            "Svelte projects a public API"
        );
        assert_eq!(
            svelte.script_fact_providers.len(),
            1,
            "Svelte registers its one syntax-capture provider"
        );
        assert!(
            matches!(svelte.surface, SurfaceRegistration::Adapter(_)),
            "Svelte registers a real surface adapter (the Deferred arm is superseded)"
        );
        // The api-leg clause holds: import surface is a distinct `.ts` API
        // file -> api_projector Some.
        assert_eq!(
            svelte
                .descriptor
                .virtual_file_naming
                .as_ref()
                .unwrap()
                .api_surface_suffix(),
            Some(".ts")
        );
    }

    #[test]
    fn active_provider_index_selects_by_carrier_language_gate() {
        let registry = FrameworkAdapterRegistry::from_registrations([(
            crate::framework::script_facts::fixtures::fixture_adapter_id(),
            crate::framework::script_facts::fixtures::carrier_gated_fixture_registration(),
        )]);
        let index = registry.active_provider_index();
        assert!(
            !index.is_empty(),
            "a registered provider populates the index"
        );
        // The fixture provider's carrier-language gate matches its language.
        let active = index.active_for(
            Some(&crate::framework::script_facts::fixtures::fixture_language()),
            std::iter::empty(),
        );
        assert_eq!(active.len(), 1, "the carrier-language gate selects it");
        // A different carrier language does NOT select it.
        let inactive = index.active_for(Some(&LanguageId::new("other")), std::iter::empty());
        assert!(
            inactive.is_empty(),
            "a non-matching carrier language is inert"
        );
    }

    #[test]
    fn active_provider_index_selects_by_import_specifier_gate() {
        let registry = FrameworkAdapterRegistry::from_registrations([(
            crate::framework::script_facts::fixtures::fixture_adapter_id(),
            crate::framework::script_facts::fixtures::import_gated_fixture_registration(),
        )]);
        let index = registry.active_provider_index();
        // Importing the gated specifier selects the provider.
        let active = index.active_for(
            None,
            [crate::framework::script_facts::fixtures::FIXTURE_IMPORT_SPECIFIER],
        );
        assert_eq!(active.len(), 1);
        // Importing something else does not.
        let inactive = index.active_for(None, ["vue"]);
        assert!(inactive.is_empty());
    }
}
