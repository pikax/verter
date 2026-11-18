#![deny(missing_docs)]
//! Static description of one framework adapter.
//!
//! A [`FrameworkAdapterDescriptor`] is the single authority for an adapter's
//! identity, the surface kinds it can produce, its carrier language (if any),
//! and its virtual-file naming policy. The descriptor REUSES the prost-generated
//! wire enums ([`FrameworkTag`], [`FrameworkSurfaceKind`]) rather than mirroring
//! them in a parallel host taxonomy — a mirror would be a second closed enum to
//! keep in sync for zero gain (the session already depends on
//! `verter_protocol`).
//!
//! The [`VirtualFileNaming`] column is the single authority for an adapter's
//! IDE / API / testing-API / sidecar virtual-file suffixes. The column is the
//! producer; the LSP / ts-plugin naming derivations are its consumers. A
//! characterization test pins the column ↔ live derivation equivalence so the
//! two cannot drift.

pub use verter_compiler::framework_common::virtual_file_naming::{
    carrier_companion_identities_for_source, classify_carrier_companion,
    verter_types_import_specifier, verter_types_sidecar_path, CarrierCompanion,
    CarrierCompanionKind, CheckingSurface, DeclarationSurface, VirtualFileNaming,
    VirtualPathPolicy, VERTER_TYPES_SIDECAR_EXT, VERTER_TYPES_SIDECAR_MODULE_STEM,
};
use verter_compiler::framework_common::virtual_file_naming::{
    SVELTE_VIRTUAL_FILE_NAMING, VUE_VIRTUAL_FILE_NAMING,
};

#[cfg(test)]
const VERTER_TYPES_SIDECAR_SUFFIXES: &[&str] = VUE_VIRTUAL_FILE_NAMING.sidecar_suffixes;

use verter_language::{FrameworkAdapterId, LanguageId};
use verter_protocol::typeinfo::graph::{FrameworkSurfaceKind, FrameworkTag};

pub use verter_compiler::framework_common::profile::{
    carrier_syntax_schema, framework_profile_descriptor_schema, FrameworkProfileManifest,
    CARRIER_SYNTAX_SCHEMA_FAMILY, FRAMEWORK_PROFILE_DESCRIPTOR_SCHEMA_EPOCH,
    FRAMEWORK_PROFILE_DESCRIPTOR_SCHEMA_FAMILY,
};
pub use verter_compiler::framework_common::vue_carrier_frontend::VUE_PROFILE_MANIFEST;
pub use verter_compiler::svelte::carrier_frontend::SVELTE_PROFILE_MANIFEST;

/// Every framework-surface kind the wire taxonomy defines, in tag order.
///
/// The framework-surface executor's requested set is ALWAYS this full set —
/// the response carries exactly one entry per known kind, supported kinds
/// materialized and unsupported kinds filled structurally. Request-narrowing,
/// if added, rides a dedicated wire field; this is the static requested set.
pub const ALL_FRAMEWORK_SURFACE_KINDS: &[FrameworkSurfaceKind] = &[
    FrameworkSurfaceKind::Props,
    FrameworkSurfaceKind::Emits,
    FrameworkSurfaceKind::Slots,
    FrameworkSurfaceKind::Options,
    FrameworkSurfaceKind::Expose,
    FrameworkSurfaceKind::Model,
];

/// The framework-surface kinds the Svelte adapter supports (§9).
///
/// OPTIONS is OMITTED — Svelte has no options surface, so the executor fills
/// OPTIONS structurally UNSUPPORTED. Every other kind maps to a Svelte source
/// family.
pub const SVELTE_SUPPORTED_SURFACE_KINDS: &[FrameworkSurfaceKind] = &[
    FrameworkSurfaceKind::Props,
    FrameworkSurfaceKind::Emits,
    FrameworkSurfaceKind::Slots,
    FrameworkSurfaceKind::Expose,
    FrameworkSurfaceKind::Model,
];

/// Static description of one framework adapter's identity + capabilities.
///
/// The descriptor is the registry row's immutable identity half: it names the
/// interned adapter id, the wire [`FrameworkTag`] the adapter answers to, the
/// surface kinds it can produce, its optional carrier language, and its
/// optional virtual-file naming policy.
#[derive(Debug, Clone)]
pub struct FrameworkAdapterDescriptor {
    /// The interned `verter_language` adapter id (e.g. the `.vue` adapter id).
    pub id: FrameworkAdapterId,
    /// The wire framework tag this adapter's responses carry: the closed
    /// display taxonomy entry, never a selector. A TypeInfo `Component`
    /// selector names the registration's admitted profile (minted from
    /// [`Self::profile_manifest`]) instead.
    pub tag: FrameworkTag,
    /// The framework-surface kinds this adapter can produce. REUSES the wire
    /// enum (no parallel host enum). A kind absent from this slice is filled
    /// structurally as `UNSUPPORTED` by the executor.
    pub supported_surfaces: &'static [FrameworkSurfaceKind],
    /// The carrier language whose parse artifact this adapter consumes, when
    /// the adapter is carrier-backed (`Some("vue")` for Vue). `None` for
    /// carrier-less adapters (e.g. an extracted program model). A `None`
    /// carrier language means [`crate::framework::FrameworkAdapterCtx::carrier_for`]
    /// returns `None` cleanly.
    pub carrier_language: Option<LanguageId>,
    /// The adapter's virtual-file naming policy, when it produces virtual files
    /// for the IDE / API surface. `None` for adapters with no virtual-file
    /// projection.
    pub virtual_file_naming: Option<VirtualFileNaming>,
    /// Whether this adapter resolves framework surfaces for a NAMED export
    /// (`{ export_name }`), not only the default-export component.
    ///
    /// The framework-NEUTRAL executor reads this REGISTRY-DATA capability to
    /// decide whether a named-export framework-surface request is supported —
    /// it does NOT branch on a framework identity (`is_vue()`). `false` means a
    /// named-export framework surface for this adapter is a typed
    /// `MalformedPayload` (the export must be the default component). The Vue
    /// keystone adapter resolves the SFC's default-export component surface only,
    /// so it sets this `false`; an adapter that synthesizes per-export component
    /// surfaces sets it `true`.
    pub supports_named_export_surfaces: bool,
    /// Whether this adapter's carrier carries a `<script setup>` block whose
    /// generic type parameters must be surfaced as typed declarations in the
    /// prepared-declaration bundle (the Vue SFC
    /// `<script setup generic="T, U">` contract).
    ///
    /// The framework-NEUTRAL prepared-declaration materializer reads this
    /// REGISTRY-DATA capability to decide whether to collect script-setup
    /// generic type-parameter bindings — it does NOT branch on a framework
    /// identity (`is_vue()`). The Vue descriptor declares the surface `true`;
    /// an adapter whose carrier has no script-setup generics surface declares
    /// it `false` and its bundles carry no bindings.
    pub declares_script_setup_type_bindings: bool,
    /// Whether this adapter's carrier analysis snapshot is served from its
    /// parse artifact — the carrier's own structure/script parse products
    /// threaded into the snapshot — together with template-analysis
    /// inputs for the carrier's template surface (the SFC template).
    ///
    /// The framework-NEUTRAL snapshot builder and the narrowed-scope
    /// serve read this REGISTRY-DATA capability to decide which snapshot
    /// path a carrier takes and whether template inputs ride alongside —
    /// they do NOT branch on a framework identity (`is_vue()`). The Vue
    /// descriptor declares it `true` (an SFC snapshot serves from the
    /// framework parse artifact and carries template inputs); an adapter
    /// whose carrier has no template surface declares it `false` and its
    /// snapshot rebuilds through the plain-script parse.
    pub carries_template_analysis_inputs: bool,
    /// Whether this adapter's carrier anchors its export spans at the
    /// script-setup bindings and macros of its script analysis —
    /// script-setup top-level bindings ARE the authored exports (the
    /// `<script setup>` auto-export contract) — rather than at authored
    /// export-signature statements.
    ///
    /// The framework-NEUTRAL export-span lookup reads this REGISTRY-DATA
    /// capability to decide where a carrier's export anchors live — it
    /// does NOT branch on a framework identity (`is_vue()`). The Vue
    /// descriptor declares it `true`; an adapter whose exports are
    /// ordinary signature statements declares it `false`.
    pub anchors_exports_at_script_setup: bool,
    /// Whether this adapter's carrier analysis classifies its component API
    /// calls (lifecycle hooks, `provide`/`inject`, `defineComponent`) and its
    /// store definitions and uses, so a query over those classified facts is
    /// complete for the carrier.
    ///
    /// The framework-NEUTRAL MCP analysis queries read this REGISTRY-DATA
    /// capability as their catalog support for a carrier's profile — they do
    /// NOT branch on a framework identity (`is_vue()`). The Vue descriptor
    /// declares it `true`; an adapter whose analysis does not classify those
    /// calls declares it `false`, and the queries refuse its carriers as an
    /// unsupported capability rather than reporting them clean.
    pub classifies_component_api_calls: bool,
    /// This carrier's rank in the extension-probe priority for
    /// extension-based dependency resolution: when one bare specifier
    /// matches admitted carriers of several adapters (a same-stem
    /// `.vue`/`.svelte` collision), the resolver probes extensions in
    /// ASCENDING rank order and takes the first match.
    ///
    /// The rank is an explicit REGISTRATION DECISION a new adapter declares
    /// next to the existing ones — deliberately NOT derived from the
    /// classifier's carrier order (a longest-suffix-first MATCHING order)
    /// nor from the capability catalog's identity sort (a lexicographic
    /// accident), either of which would silently flip same-stem resolution.
    /// Equal ranks keep the catalog's deterministic sorted-identity order.
    pub carrier_probe_rank: u32,
    /// The semantic-profile manifest this registration is the meaning of.
    /// The registry mints its [`FrameworkProfileId`](verter_identity::successor::FrameworkProfileId) ONCE at construction;
    /// the TypeInfo `Component` selector names that profile, never
    /// the adapter id or the display tag.
    pub profile_manifest: FrameworkProfileManifest,
}

impl FrameworkAdapterDescriptor {
    /// The carrier extension this adapter owns, in leading-dot form
    /// (`.{carrier_language}`, e.g. `.vue` / `.svelte`), or `None` for a
    /// carrier-less adapter. DERIVED from [`Self::carrier_language`] — never a
    /// hand-matched extension literal (`single_language_classifier`).
    #[must_use]
    pub fn carrier_extension(&self) -> Option<String> {
        self.carrier_language
            .as_ref()
            .map(|lang| format!(".{}", lang.as_str()))
    }

    /// The descriptor-valid DECLARATION carrier identity for a carrier `source`
    /// (`Foo.vue` -> `Foo.d.vue.ts`), or `None` when this adapter projects no
    /// declaration carrier OR `source` does not carry THIS adapter's carrier
    /// extension. This is the descriptor AUTHORITY for the `.d.<ext>.ts`
    /// identity: it supplies the registry-derived carrier extension to
    /// [`VirtualFileNaming::declaration_carrier_identity`], so a non-carrier
    /// source (`Foo.ts` / `Foo.js` / a foreign extension) never produces a
    /// fabricated `Foo.d.ts.ts`.
    #[must_use]
    pub fn declaration_carrier_identity(&self, source: &str) -> Option<String> {
        let naming = self.virtual_file_naming.as_ref()?;
        naming.declaration_carrier_identity(source, self.carrier_extension().as_deref())
    }

    /// Every descriptor-valid companion IDENTITY for a carrier `source` across ALL
    /// families, or an empty list when this adapter projects no virtual files OR
    /// `source` does not carry THIS adapter's carrier extension. Supplies the
    /// registry-derived carrier extension to
    /// [`VirtualFileNaming::carrier_companion_identities`].
    #[must_use]
    pub fn carrier_companion_identities(&self, source: &str) -> Vec<CarrierCompanion> {
        let Some(naming) = self.virtual_file_naming.as_ref() else {
            return Vec::new();
        };
        naming.carrier_companion_identities(source, self.carrier_extension().as_deref())
    }

    /// The carrier SOURCE + companion role a `companion_path` projects from under THIS
    /// adapter, or `None` when it is not one of this adapter's companions. Supplies the
    /// registry-derived carrier extension to
    /// [`VirtualFileNaming::carrier_source_for_companion`].
    #[must_use]
    pub fn carrier_source_for_companion(&self, companion_path: &str) -> Option<CarrierCompanion> {
        let naming = self.virtual_file_naming.as_ref()?;
        naming.carrier_source_for_companion(companion_path, self.carrier_extension().as_deref())
    }
}

/// The Vue adapter descriptor row.
///
/// The single source of truth for the Vue adapter's identity, surface kinds,
/// carrier language, and virtual-file naming. The naming column is
/// characterization-tested against the live production derivations
/// (`verter_workspace`/`provider_sync`/ts-plugin) so the column and the live
/// derivations cannot drift while both coexist.
#[must_use]
pub fn vue_descriptor() -> FrameworkAdapterDescriptor {
    FrameworkAdapterDescriptor {
        id: FrameworkAdapterId::vue(),
        tag: FrameworkTag::Vue,
        supported_surfaces: ALL_FRAMEWORK_SURFACE_KINDS,
        carrier_language: Some(LanguageId::new("vue")),
        virtual_file_naming: Some(VUE_VIRTUAL_FILE_NAMING.clone()),
        // The Vue adapter resolves the SFC's default-export component surface
        // only; a named-export framework surface is not yet a distinct
        // resolution.
        supports_named_export_surfaces: false,
        // The Vue SFC `<script setup generic="T, U">` surface: the
        // prepared-declaration materializer collects its type-parameter
        // bindings as typed declarations.
        declares_script_setup_type_bindings: true,
        // An SFC snapshot serves from the framework parse artifact and
        // carries template-analysis inputs for the template surface.
        carries_template_analysis_inputs: true,
        // `<script setup>` top-level bindings are the authored exports,
        // so export spans anchor at the script-setup bindings/macros.
        anchors_exports_at_script_setup: true,
        // The SFC script analysis classifies Vue API calls and store facts.
        classifies_component_api_calls: true,
        // The established same-stem probe winner: a `./Widget` collision
        // with a `.svelte` twin resolves to the `.vue` carrier first.
        carrier_probe_rank: 0,
        profile_manifest: VUE_PROFILE_MANIFEST,
    }
}

/// Every built-in framework-adapter descriptor, in a stable order.
///
/// The single enumeration the compiler-completeness guard
/// (`carrier_descriptors_have_compilers`) iterates: it filters the
/// carrier-bearing rows (`carrier_language.is_some()`) and asserts each has a
/// registered compile/eval/IDE compiler. Source-stage parse is catalog-frontend
/// owned; this list still covers compile-side completeness. A new carrier
/// vertical adds its descriptor here and the guard automatically covers it.
#[must_use]
pub fn built_in_descriptors() -> Vec<FrameworkAdapterDescriptor> {
    vec![vue_descriptor(), svelte_descriptor()]
}

/// The Svelte adapter descriptor row.
///
/// The single source of truth for the Svelte carrier's identity, carrier
/// language, and virtual-file naming. `supported_surfaces` is the §9 Svelte set
/// ([`SVELTE_SUPPORTED_SURFACE_KINDS`] — OPTIONS omitted), so the executor
/// reports OPTIONS structurally UNSUPPORTED and resolves every other kind
/// through the registered `SvelteFrameworkAdapter`. The `.ts` import-surface
/// API file is matched by the registered Svelte api-projector leg (the
/// `framework_registry_complete` api-leg clause).
#[must_use]
pub fn svelte_descriptor() -> FrameworkAdapterDescriptor {
    FrameworkAdapterDescriptor {
        id: FrameworkAdapterId::svelte(),
        tag: FrameworkTag::Svelte,
        supported_surfaces: SVELTE_SUPPORTED_SURFACE_KINDS,
        carrier_language: Some(LanguageId::new("svelte")),
        virtual_file_naming: Some(SVELTE_VIRTUAL_FILE_NAMING.clone()),
        // The Svelte carrier resolves the default-export component surface only
        // (a `.svelte` file is one component); a named-export framework surface
        // is not a distinct resolution for it.
        supports_named_export_surfaces: false,
        // A `.svelte` script has no script-setup generics surface; its bundles
        // carry no script-setup type bindings.
        declares_script_setup_type_bindings: false,
        // A `.svelte` snapshot rebuilds through the plain-script parse and
        // carries no template-analysis inputs.
        carries_template_analysis_inputs: false,
        // A `.svelte` script's exports are ordinary signature statements,
        // not script-setup auto-exports.
        anchors_exports_at_script_setup: false,
        // A `.svelte` analysis does not classify Svelte lifecycle, context or
        // store calls into the component-API facts.
        classifies_component_api_calls: false,
        // Probed after `.vue` on a same-stem collision.
        carrier_probe_rank: 1,
        profile_manifest: SVELTE_PROFILE_MANIFEST,
    }
}

/// The Svelte standalone rune-module (`.svelte.ts`/`.svelte.js`) virtual-file
/// naming.
///
/// A rune module is NOT a component carrier — it has no dual-file model. Its
/// IDE surface and its import-resolution surface are the SAME `SelfFile`: the
/// module serves its own canonical path with prelude-augmented content
/// (Channel B). It exposes NO component API and NO testing surface. This is a
/// distinct column from any carrier descriptor (a rune module is a script, not
/// a `FrameworkAdapterDescriptor` carrier row), recorded so the TS mirror and
/// the LSP naming derivations share one authority.
#[must_use]
pub fn svelte_rune_module_naming() -> VirtualFileNaming {
    VirtualFileNaming {
        ide: VirtualPathPolicy::SelfFile,
        import_surface: VirtualPathPolicy::SelfFile,
        testing_api_suffix: None,
        sidecar_suffixes: &[],
        // A rune module is a script, not a component carrier — it projects no
        // declaration carrier (it serves its own `SelfFile` path).
        declaration_surface: DeclarationSurface::None,
        // A rune module is checked as itself.
        checking_surface: CheckingSurface::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_and_compiler_paths_share_the_naming_owner() {
        use verter_compiler::framework_common::virtual_file_naming as compiler;
        let naming: compiler::VirtualFileNaming = vue_descriptor().virtual_file_naming.unwrap();
        let session_naming: VirtualFileNaming = naming;
        let path = session_naming
            .checking_module_identity("/ws/App.vue", Some(".vue"), "ts")
            .unwrap();
        let from_session: Option<compiler::CarrierCompanion> = classify_carrier_companion(&path);
        let from_compiler: Option<CarrierCompanion> = compiler::classify_carrier_companion(&path);
        assert_eq!(from_session, from_compiler);
        assert!(std::ptr::fn_addr_eq(
            classify_carrier_companion as fn(&str) -> Option<CarrierCompanion>,
            compiler::classify_carrier_companion as fn(&str) -> Option<CarrierCompanion>,
        ));
    }

    #[test]
    fn checking_identity_refuses_a_path_owned_by_another_carrier() {
        let descriptor = vue_descriptor();
        let naming = descriptor.virtual_file_naming.as_ref().unwrap();
        let extension = descriptor.carrier_extension();
        let declaration = descriptor
            .declaration_carrier_identity("/ws/Foo.vue")
            .unwrap();
        assert_eq!(
            classify_carrier_companion(&declaration).unwrap().source,
            "/ws/Foo.vue"
        );
        assert!(naming
            .checking_module_identity("/ws/Foo.d.vue", extension.as_deref(), "ts")
            .is_none());
        assert!(naming
            .checking_module_identity("/ws/Foo.d.vue", extension.as_deref(), "tsx")
            .is_some());
        for companion in descriptor.carrier_companion_identities("/ws/Foo.d.vue") {
            assert_eq!(
                classify_carrier_companion(&companion.path).unwrap(),
                companion
            );
        }
    }

    #[test]
    fn checking_module_identities_classify_back_to_the_carrier() {
        let descriptor = vue_descriptor();
        let naming = descriptor.virtual_file_naming.as_ref().unwrap();
        let extension = descriptor.carrier_extension();
        for dialect in ["ts", "js", "tsx", "jsx"] {
            let path = naming
                .checking_module_identity("/ws/App.vue", extension.as_deref(), dialect)
                .unwrap();
            let companion = classify_carrier_companion(&path)
                .unwrap_or_else(|| panic!("checking module {path} must classify as a companion"));
            assert_eq!(companion.source, "/ws/App.vue");
            assert_eq!(companion.path, path);
            assert_eq!(
                companion.kind,
                if ["tsx", "jsx"].contains(&dialect) {
                    CarrierCompanionKind::Ide
                } else {
                    CarrierCompanionKind::Checking
                }
            );
            assert!(descriptor
                .carrier_companion_identities("/ws/App.vue")
                .contains(&companion));
        }
        // The registered checking policy, rather than an IDE suffix or a
        // framework-specific literal, determines the additional paths.
        let mut custom = naming.clone();
        custom.ide = VirtualPathPolicy::None;
        custom.checking_surface = CheckingSurface::DialectSuffixes(&[".check"]);
        let path = custom
            .checking_module_identity("/ws/App.vue", extension.as_deref(), "check")
            .unwrap();
        assert_eq!(
            custom
                .carrier_source_for_companion(&path, extension.as_deref())
                .unwrap()
                .source,
            "/ws/App.vue"
        );
        assert!(custom
            .carrier_source_for_companion("/ws/App.ts.check", extension.as_deref())
            .is_none());
        assert!(custom
            .carrier_source_for_companion("/ws/.vue.check", extension.as_deref())
            .is_none());
    }

    #[test]
    fn all_framework_surface_kinds_covers_every_wire_kind() {
        // The static requested set must enumerate every wire kind exactly once,
        // in tag order — the executor relies on it to fill one entry per kind.
        assert_eq!(ALL_FRAMEWORK_SURFACE_KINDS.len(), 6);
        assert_eq!(
            ALL_FRAMEWORK_SURFACE_KINDS,
            &[
                FrameworkSurfaceKind::Props,
                FrameworkSurfaceKind::Emits,
                FrameworkSurfaceKind::Slots,
                FrameworkSurfaceKind::Options,
                FrameworkSurfaceKind::Expose,
                FrameworkSurfaceKind::Model,
            ]
        );
    }

    #[test]
    fn vue_descriptor_matches_live_virtual_file_derivations() {
        let d = vue_descriptor();
        assert!(d.id.is_vue());
        assert_eq!(d.tag, FrameworkTag::Vue);
        assert_eq!(d.supported_surfaces, ALL_FRAMEWORK_SURFACE_KINDS);
        assert_eq!(d.carrier_language, Some(LanguageId::new("vue")));

        let naming = d
            .virtual_file_naming
            .as_ref()
            .expect("the Vue descriptor carries a virtual-file naming column");
        // Pin the column against the live production derivations the LSP /
        // ts-plugin still own. A drift here means the column and the live
        // derivation disagree — a real defect.
        assert_eq!(
            naming.ide,
            VirtualPathPolicy::JsxConditional {
                jsx: ".jsx",
                non_jsx: ".tsx",
            }
        );
        // The public-API carrier carries the reserved `.verter.` infix
        // (redirect-reached, never bare-probed — GATE 5).
        assert_eq!(
            naming.import_surface,
            VirtualPathPolicy::Suffix(".verter.ts")
        );
        assert_eq!(naming.testing_api_suffix, Some(".__verter_test.ts"));
        // The adjacent `@verter/types` fallback overlay is a named sidecar of
        // the carrier family — one form per IDE suffix — so the SHARED overlay
        // records + injects it with its carrier instead of dropping it.
        assert_eq!(naming.sidecar_suffixes, VERTER_TYPES_SIDECAR_SUFFIXES);
        let sidecar = classify_carrier_companion("/ws/src/App.vue.tsx.__verter_types.d.ts")
            .expect("the __verter_types sidecar classifies as a Vue carrier companion");
        assert_eq!(sidecar.source, "/ws/src/App.vue");
        assert_eq!(sidecar.kind, CarrierCompanionKind::Sidecar);
        assert!(naming.is_structurally_valid());
    }

    #[test]
    fn svelte_descriptor_projects_language_specific_ide_files_with_no_testing_surface() {
        let d = svelte_descriptor();
        assert_eq!(d.id, FrameworkAdapterId::svelte());
        assert_eq!(d.tag, FrameworkTag::Svelte);
        assert_eq!(d.carrier_language, Some(LanguageId::new("svelte")));

        let naming = d
            .virtual_file_naming
            .as_ref()
            .expect("the Svelte descriptor carries a virtual-file naming column");
        // A `.svelte` COMPONENT projects `.tsx` for TypeScript source and
        // `.jsx` for JavaScript source, plus the fixed `.verter.ts` import
        // surface. It has NO testing-API surface (Vue-only).
        assert_eq!(
            naming.ide,
            VirtualPathPolicy::JsxConditional {
                jsx: ".jsx",
                non_jsx: ".tsx",
            }
        );
        // The public-API carrier carries the reserved `.verter.` infix: a bare
        // `.svelte.ts` would collide with a rune module (GATE 5).
        assert_eq!(
            naming.import_surface,
            VirtualPathPolicy::Suffix(".verter.ts")
        );
        assert_eq!(naming.testing_api_suffix, None);
        // Svelte carriers are first-class on the same virtualized-`@verter/types`
        // sidecar rail as Vue: the `.jsx` form covers a no-lang JS component's
        // IDE companion.
        assert_eq!(naming.sidecar_suffixes, VERTER_TYPES_SIDECAR_SUFFIXES);
        let sidecar = classify_carrier_companion("/ws/src/Card.svelte.jsx.__verter_types.d.ts")
            .expect("the __verter_types sidecar classifies as a Svelte carrier companion");
        assert_eq!(sidecar.source, "/ws/src/Card.svelte");
        assert_eq!(sidecar.kind, CarrierCompanionKind::Sidecar);
        assert!(naming.is_structurally_valid());
    }

    #[test]
    fn verter_types_sidecar_column_derives_from_the_module_stem() {
        // The overlay extension IS the module stem plus the `.d.ts` declaration
        // extension — the extensionless specifier probe must land on the overlay.
        assert_eq!(
            VERTER_TYPES_SIDECAR_EXT,
            format!("{VERTER_TYPES_SIDECAR_MODULE_STEM}.d.ts")
        );
        for descriptor in built_in_descriptors() {
            let naming = descriptor
                .virtual_file_naming
                .as_ref()
                .expect("built-in carrier descriptors carry a naming column");
            let VirtualPathPolicy::JsxConditional { jsx, non_jsx } = naming.ide else {
                panic!("built-in carrier descriptors use the JsxConditional IDE policy");
            };
            // The column is EXACTLY the IDE suffixes with the sidecar extension
            // appended — no extra entry, no missing IDE form.
            let derived: std::collections::BTreeSet<String> = [non_jsx, jsx]
                .iter()
                .map(|ide_suffix| format!("{ide_suffix}{VERTER_TYPES_SIDECAR_EXT}"))
                .collect();
            let column: std::collections::BTreeSet<String> = naming
                .sidecar_suffixes
                .iter()
                .map(|s| (*s).to_string())
                .collect();
            assert_eq!(
                column, derived,
                "{:?}: sidecar_suffixes must be the IDE suffixes + VERTER_TYPES_SIDECAR_EXT",
                descriptor.id
            );
            // FORWARD → CLASSIFIER round-trip: the path the producer derives for
            // any IDE companion classifies back to that companion's carrier
            // source as a Sidecar — the invariant whose absence dropped the
            // sidecar from the SHARED overlay.
            let carrier_ext = descriptor
                .carrier_extension()
                .expect("built-in carrier descriptors have a carrier extension");
            for ide_suffix in [non_jsx, jsx] {
                let source = format!("/ws/src/Comp{carrier_ext}");
                let ide_companion = format!("{source}{ide_suffix}");
                let sidecar = verter_types_sidecar_path(&ide_companion)
                    .expect("an IDE companion with a basename derives a sidecar path");
                let classified = classify_carrier_companion(&sidecar).unwrap_or_else(|| {
                    panic!("derived sidecar {sidecar} must classify as a carrier companion")
                });
                assert_eq!(classified.source, source);
                assert_eq!(classified.kind, CarrierCompanionKind::Sidecar);
                // The specifier derives from the companion BASENAME with the
                // module stem — the exact spelling the producer rewrites imports to.
                assert_eq!(
                    verter_types_import_specifier(&ide_companion).as_deref(),
                    Some(
                        format!(
                            "./Comp{carrier_ext}{ide_suffix}{VERTER_TYPES_SIDECAR_MODULE_STEM}"
                        )
                        .as_str()
                    )
                );
            }
        }
        // Fail-closed: a path with no basename derives NEITHER a sidecar path
        // NOR a specifier — the helpers refuse to fabricate
        // `.__verter_types.d.ts` at a bare directory. Both refusals fire on the
        // same predicate, so the pair cannot drift: a specifier without an
        // overlay (or an overlay no specifier reaches) would be a stranded
        // publication. Windows spellings included (Cross-Platform Portability):
        // a trailing `\` or a verbatim `\\?\` drive root is just as
        // basename-less as a POSIX directory path.
        for no_basename in ["", "/ws/src/", "C:\\ws\\", "\\\\?\\C:\\ws\\"] {
            assert_eq!(
                verter_types_sidecar_path(no_basename),
                None,
                "{no_basename:?} names no file, so no sidecar path may be fabricated"
            );
            assert_eq!(
                verter_types_import_specifier(no_basename),
                None,
                "{no_basename:?} names no file, so no specifier may be derived"
            );
        }
    }

    #[test]
    fn svelte_rune_module_naming_is_same_file_with_no_component_surface() {
        // A standalone rune module uses the SAME-FILE model (NOT the
        // component dual-file model). Its IDE and import surfaces are BOTH
        // `SelfFile` (it serves its own canonical path), and it has NO testing
        // surface. This is the discriminating contrast with the component
        // carrier's conditional `.tsx`/`.jsx` IDE plus `.verter.ts` API model.
        let naming = svelte_rune_module_naming();
        assert_eq!(naming.ide, VirtualPathPolicy::SelfFile);
        assert_eq!(naming.import_surface, VirtualPathPolicy::SelfFile);
        assert_ne!(
            naming.ide,
            VirtualPathPolicy::Suffix(".tsx"),
            "a rune module is NOT the component dual-file model"
        );
        assert_eq!(naming.testing_api_suffix, None);
        assert_eq!(naming.sidecar_suffixes, &[] as &[&str]);
        assert!(naming.is_structurally_valid());
    }

    #[test]
    fn declaration_carrier_identity_inserts_d_infix_in_extension_middle_form() {
        // The declaration carrier identity is the EXTENSION-MIDDLE `.d.<ext>.ts`
        // form: take `.../Foo.<ext>`, insert `.d.` between the stem and the
        // carrier extension, then append `.ts`. This is the path tsgo's bare
        // framework-carrier import basename-append probe reaches FIRST
        // (`.d.vue.ts` -> `.vue.ts` -> `.vue.tsx`). Exercised through the
        // descriptor-level entry, which derives the carrier extension from the
        // adapter's `carrier_language` (no hand-matched literal).
        assert_eq!(
            vue_descriptor().declaration_carrier_identity("/ws/src/Foo.vue"),
            Some("/ws/src/Foo.d.vue.ts".to_string()),
            "Vue declaration carrier is the extension-middle `.d.vue.ts`"
        );
        assert_eq!(
            svelte_descriptor().declaration_carrier_identity("/ws/src/Foo.svelte"),
            Some("/ws/src/Foo.d.svelte.ts".to_string()),
            "Svelte declaration carrier is the extension-middle `.d.svelte.ts`"
        );
    }

    #[test]
    fn declaration_carrier_identity_is_extension_middle_never_extension_last() {
        // NEGATIVE: the declaration carrier PRESERVES the carrier extension in
        // extension-MIDDLE form. It is NEVER the extension-LAST `.vue.d.ts`
        // (which tsgo would not resolve the bare `.vue` import to), and it
        // NEVER carries the redirect-reached `.verter.` infix.
        let identity = vue_descriptor()
            .declaration_carrier_identity("/ws/src/Foo.vue")
            .expect("vue produces a declaration carrier");
        assert!(
            identity.ends_with(".d.vue.ts"),
            "extension-middle form: `{identity}` must end with `.d.vue.ts`"
        );
        assert!(
            !identity.contains(".vue.d.ts"),
            "must NOT be extension-last `.vue.d.ts`: `{identity}`"
        );
        assert!(
            !identity.contains(".verter."),
            "the declaration carrier is bare-probed, never `.verter.`: `{identity}`"
        );
    }

    #[test]
    fn declaration_carrier_identity_none_for_extension_less_source() {
        // A source with no extension has no carrier extension to wrap, so the
        // declaration-carrier transform returns None (it is not a carrier path).
        assert_eq!(
            vue_descriptor().declaration_carrier_identity("/ws/src/Foo"),
            None
        );
    }

    #[test]
    fn declaration_carrier_identity_rejects_non_carrier_extensions() {
        // The declaration carrier identity is a CARRIER-EXTENSION transform: it
        // returns `Some(..)` ONLY when the source's extension matches THIS
        // adapter's (registry-derived) carrier extension. A `.ts` / `.js` /
        // foreign-extension source is NOT a carrier path, so the transform
        // returns `None` — it must NOT fabricate a `Foo.d.ts.ts` by blindly
        // inserting `.d.` at the final basename dot.
        let vue = vue_descriptor();

        // A `.ts` source under the Vue descriptor is NOT a carrier: it must NOT
        // become `/ws/src/Foo.d.ts.ts`.
        assert_eq!(
            vue.declaration_carrier_identity("/ws/src/Foo.ts"),
            None,
            "a `.ts` source is not a Vue carrier — no `.d.ts.ts`"
        );
        // NEGATIVE: explicitly assert the bad output is never produced.
        assert_ne!(
            vue.declaration_carrier_identity("/ws/src/Foo.ts"),
            Some("/ws/src/Foo.d.ts.ts".to_string()),
            "the `.ts` source must NEVER yield the fabricated `Foo.d.ts.ts`"
        );
        assert_eq!(
            vue.declaration_carrier_identity("/ws/src/Foo.js"),
            None,
            "a `.js` source is not a Vue carrier"
        );
        // A foreign carrier extension (`.svelte` under the Vue descriptor) is
        // also not THIS descriptor's carrier.
        assert_eq!(
            vue.declaration_carrier_identity("/ws/src/Foo.svelte"),
            None,
            "a `.svelte` source is not a Vue carrier under the Vue descriptor"
        );

        // The Svelte descriptor is the mirror image: it accepts `.svelte`, not
        // `.vue`.
        assert_eq!(
            svelte_descriptor().declaration_carrier_identity("/ws/src/Foo.vue"),
            None,
            "a `.vue` source is not a Svelte carrier under the Svelte descriptor"
        );
    }

    #[test]
    fn declaration_carrier_identity_accepts_only_the_matching_carrier_extension() {
        // POSITIVE control (non-vacuity): the matching carrier extension still
        // produces the extension-middle `.d.<ext>.ts` identity for each
        // descriptor.
        assert_eq!(
            vue_descriptor().declaration_carrier_identity("/ws/src/Foo.vue"),
            Some("/ws/src/Foo.d.vue.ts".to_string()),
        );
        assert_eq!(
            svelte_descriptor().declaration_carrier_identity("/ws/src/Foo.svelte"),
            Some("/ws/src/Foo.d.svelte.ts".to_string()),
        );
    }

    #[test]
    fn a_checking_module_is_named_by_the_carrier_and_its_dialect() {
        let vue = vue_descriptor();
        let naming = vue.virtual_file_naming.as_ref().expect("vue naming");
        let ext = vue.carrier_extension();
        let named = |source: &str, dialect: &str| {
            naming.checking_module_identity(source, ext.as_deref(), dialect)
        };
        assert_eq!(
            named("/ws/src/Foo.vue", "ts").as_deref(),
            Some("/ws/src/Foo.vue.ts")
        );
        assert_eq!(
            named("/ws/src/Foo.vue", "tsx").as_deref(),
            Some("/ws/src/Foo.vue.tsx")
        );
        assert_eq!(
            named("/ws/src/Foo.vue", "js").as_deref(),
            Some("/ws/src/Foo.vue.js")
        );
        // A dialect the column does not name, and a source that is not a
        // carrier, have no checking module.
        assert_eq!(named("/ws/src/Foo.vue", "mts"), None);
        assert_eq!(named("/ws/src/Foo.ts", "ts"), None);
        assert_eq!(named("/ws/src/.vue", "ts"), None);

        let svelte = svelte_descriptor();
        let svelte_naming = svelte.virtual_file_naming.as_ref().expect("svelte naming");
        assert_eq!(
            svelte_naming.checking_module_identity(
                "/ws/src/Card.svelte",
                svelte.carrier_extension().as_deref(),
                "ts"
            ),
            None
        );
    }

    #[test]
    fn testing_api_suffix_requires_a_distinct_import_surface_file() {
        // A testing-API suffix requires a distinct suffix-appended import
        // surface (a `SelfFile`/`None` import surface has no API file to vary).
        let invalid = VirtualFileNaming {
            ide: VirtualPathPolicy::None,
            import_surface: VirtualPathPolicy::None,
            testing_api_suffix: Some(".__verter_test.ts"),
            sidecar_suffixes: &[],
            declaration_surface: DeclarationSurface::None,
            checking_surface: CheckingSurface::None,
        };
        assert!(!invalid.is_structurally_valid());

        // A `SelfFile` import surface (rune module) also cannot carry a
        // testing-API suffix — there is no distinct API file.
        let invalid_self = VirtualFileNaming {
            ide: VirtualPathPolicy::SelfFile,
            import_surface: VirtualPathPolicy::SelfFile,
            testing_api_suffix: Some(".__verter_test.ts"),
            sidecar_suffixes: &[],
            declaration_surface: DeclarationSurface::None,
            checking_surface: CheckingSurface::None,
        };
        assert!(!invalid_self.is_structurally_valid());

        let valid = VirtualFileNaming {
            ide: VirtualPathPolicy::None,
            import_surface: VirtualPathPolicy::Suffix(".ts"),
            testing_api_suffix: Some(".__verter_test.ts"),
            sidecar_suffixes: &[],
            declaration_surface: DeclarationSurface::None,
            checking_surface: CheckingSurface::None,
        };
        assert!(valid.is_structurally_valid());
    }

    /// Shared checking and IDE suffixes retain IDE classification and recover
    /// the same carrier. Disjointness matters only when carriers differ.
    #[test]
    fn carrier_companion_identities_round_trip_through_source_for_companion() {
        // The forward enumeration and the reverse classification are inverses: for
        // every built-in carrier descriptor and every companion identity it composes,
        // the reverse map recovers the EXACT (source, kind) pair — so a companion never
        // mis-derives its source (`Foo.d.vue.ts` -> `Foo.vue`, never `Foo.d.vue`) or its
        // role.
        let source_stem = "/ws/src/Foo";
        let mut seen_kinds: Vec<CarrierCompanionKind> = Vec::new();
        let mut total = 0usize;
        for descriptor in built_in_descriptors() {
            let Some(carrier_ext) = descriptor.carrier_extension() else {
                continue;
            };
            let source = format!("{source_stem}{carrier_ext}");
            let companions = descriptor.carrier_companion_identities(&source);
            let naming = descriptor.virtual_file_naming.as_ref().unwrap();
            if let CheckingSurface::DialectSuffixes(checking) = naming.checking_surface {
                for suffix in naming.ide_carrier_suffixes() {
                    if checking.contains(&suffix) {
                        let ide_path = naming
                            .ide_carrier_identities(&source)
                            .into_iter()
                            .find(|path| path == &format!("{source}{suffix}"))
                            .unwrap();
                        let checking_path = naming
                            .checking_module_identity(
                                &source,
                                Some(&carrier_ext),
                                suffix.strip_prefix('.').unwrap(),
                            )
                            .unwrap();
                        let ide = descriptor.carrier_source_for_companion(&ide_path).unwrap();
                        let checked = descriptor
                            .carrier_source_for_companion(&checking_path)
                            .unwrap();
                        assert_eq!(ide.source, source);
                        assert_eq!(checked.source, ide.source);
                        assert_eq!(checked.kind, CarrierCompanionKind::Ide);
                    }
                }
            }
            assert!(
                !companions.is_empty(),
                "a carrier descriptor for `{carrier_ext}` must project at least one companion"
            );
            for companion in &companions {
                assert_eq!(
                    companion.source, source,
                    "the forward enumeration tags each companion with its own source"
                );
                let recovered = descriptor
                    .carrier_source_for_companion(&companion.path)
                    .unwrap_or_else(|| {
                        panic!(
                            "companion `{}` ({:?}) must reverse-map to a carrier source",
                            companion.path, companion.kind
                        )
                    });
                assert_eq!(
                    recovered.source, source,
                    "companion `{}` ({:?}) must reverse-map to its source `{source}`, got `{}`",
                    companion.path, companion.kind, recovered.source
                );
                assert_eq!(
                    recovered.kind, companion.kind,
                    "companion `{}` must reverse-map to the same role it was enumerated as",
                    companion.path
                );
                assert_eq!(
                    recovered.path, companion.path,
                    "the reverse map echoes the companion path unchanged"
                );
                if !seen_kinds.contains(&companion.kind) {
                    seen_kinds.push(companion.kind);
                }
                total += 1;
            }
        }

        // Non-vacuity: the IDE, declaration, import-surface, and testing-API families
        // must all be exercised across the built-in adapters (Vue emits all four; the
        // declaration family's reverse map recovers `Foo.vue` from `Foo.d.vue.ts`, not the
        // intermediate `Foo.d.vue` stem).
        for kind in [
            CarrierCompanionKind::Ide,
            CarrierCompanionKind::Declaration,
            CarrierCompanionKind::ImportSurface,
            CarrierCompanionKind::TestingApi,
            // The checking-only dialects are a separate role; shared paths
            // were checked above against the IDE precedence rule.
            CarrierCompanionKind::Checking,
        ] {
            assert!(
                seen_kinds.contains(&kind),
                "the round-trip must cover the {kind:?} companion family; saw {seen_kinds:?}"
            );
        }
        assert!(
            total >= 6,
            "expected the Vue and Svelte companion families to round-trip; got {total}"
        );
    }
}
