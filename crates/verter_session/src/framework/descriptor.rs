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

use verter_language::{FrameworkAdapterId, LanguageId};
use verter_protocol::typeinfo::graph::{FrameworkSurfaceKind, FrameworkTag};

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
    /// The wire framework tag this adapter answers to. The tag is the closed
    /// wire taxonomy entry a client selects the adapter by; the host interns
    /// the wire `framework_adapter_id` string at receive time and the executor
    /// maps it to this descriptor.
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
}

/// An adapter's virtual-file naming policy — the single authority for how its
/// files map to the IDE / import-resolution / testing-API / sidecar virtual
/// surfaces the provider sees.
///
/// The `ide` and `import_surface` columns are explicit [`VirtualPathPolicy`]
/// values. A COMPONENT carrier (`.vue`/`.svelte`) uses the dual-file model —
/// a distinct `.tsx`/`.ts` virtual file ([`VirtualPathPolicy::Suffix`]). A
/// standalone rune MODULE (`.svelte.ts`/`.svelte.js`) uses the same-file model:
/// its ide and import surfaces are BOTH [`VirtualPathPolicy::SelfFile`] (it
/// serves its own canonical path with prelude-augmented content; there is no
/// distinct virtual file).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualFileNaming {
    /// How the IDE (type-checked) virtual surface is named relative to the
    /// file: a suffix-appended distinct file (`.tsx`), the self file, a
    /// JSX-conditional suffix, or none.
    pub ide: VirtualPathPolicy,
    /// How the IMPORT-RESOLUTION virtual surface (what a CONSUMING module
    /// resolves the file to) is named: a suffix-appended distinct API file
    /// (`.ts`) for a component carrier, or the self file for a rune module.
    pub import_surface: VirtualPathPolicy,
    /// The testing-API virtual-file suffix. Structural rule: `Some` here
    /// REQUIRES `import_surface` to append a distinct file (a testing-API file
    /// is a variant of the import-surface API file, not of a `SelfFile`).
    pub testing_api_suffix: Option<&'static str>,
    /// Additional sidecar virtual-file suffixes the adapter emits.
    pub sidecar_suffixes: &'static [&'static str],
}

impl VirtualFileNaming {
    /// Whether this naming policy satisfies the structural invariant: a
    /// `testing_api_suffix` requires the `import_surface` to be a distinct
    /// suffix-appended file (a testing-API file is a variant of THAT file; a
    /// `SelfFile`/`None` import surface has no API file to vary).
    #[must_use]
    pub fn is_structurally_valid(&self) -> bool {
        self.testing_api_suffix.is_none() || self.api_surface_suffix().is_some()
    }

    /// The fixed API-file suffix the import surface appends, when the import
    /// surface is a DISTINCT-file (`Suffix`) policy — the component-carrier
    /// dual-file API surface (`.ts`). `None` for a `SelfFile`/`None`/
    /// `JsxConditional` import surface (a rune module serves its own file; a
    /// JSX-conditional surface has no single fixed API suffix).
    #[must_use]
    pub fn api_surface_suffix(&self) -> Option<&'static str> {
        match self.import_surface {
            VirtualPathPolicy::Suffix(s) => Some(s),
            _ => None,
        }
    }
}

/// How a virtual surface (IDE or import-resolution) is named relative to a
/// file. Explicit path policies: a component carrier appends a
/// distinct-file suffix; a standalone rune module serves its own file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VirtualPathPolicy {
    /// No virtual surface of this kind.
    None,
    /// The file IS its own virtual surface — no suffix, no distinct file. The
    /// rune-module model: the same canonical path serves both the IDE and
    /// import surfaces (with prelude-augmented content).
    SelfFile,
    /// A single fixed suffix appended to the canonical path forms a DISTINCT
    /// virtual file (`App.vue` + `.ts` ⇒ `App.vue.ts`). The component-carrier
    /// dual-file model.
    Suffix(&'static str),
    /// Choose `jsx` when the carrier script uses JSX, else `non_jsx` (the Vue
    /// `<script lang="jsx">` IDE-file case). A distinct virtual file either way.
    JsxConditional {
        /// The suffix used when the carrier script uses JSX.
        jsx: &'static str,
        /// The suffix used when the carrier script does not use JSX.
        non_jsx: &'static str,
    },
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
        virtual_file_naming: Some(VirtualFileNaming {
            ide: VirtualPathPolicy::JsxConditional {
                jsx: ".jsx",
                non_jsx: ".tsx",
            },
            import_surface: VirtualPathPolicy::Suffix(".ts"),
            testing_api_suffix: Some(".__verter_test.ts"),
            sidecar_suffixes: &[],
        }),
        // The Vue adapter resolves the SFC's default-export component surface
        // only; a named-export framework surface is not yet a distinct
        // resolution.
        supports_named_export_surfaces: false,
    }
}

/// Every built-in framework-adapter descriptor, in a stable order.
///
/// The single enumeration the compiler-completeness guard
/// (`carrier_descriptors_have_compilers`) iterates: it filters the
/// carrier-bearing rows (`carrier_language.is_some()`) and asserts each has a
/// registered `CarrierCompiler`. A new carrier vertical adds its descriptor here
/// and the guard automatically covers it.
#[must_use]
pub fn built_in_descriptors() -> Vec<FrameworkAdapterDescriptor> {
    vec![vue_descriptor(), svelte_descriptor()]
}

/// The Svelte adapter descriptor row.
///
/// The single source of truth for the Svelte carrier's identity, carrier
/// language, and virtual-file naming. `supported_surfaces` is the §9 Svelte set
/// ([`SVELTE_SUPPORTED_SURFACE_KINDS`] — OPTIONS omitted), so the executor fills
/// OPTIONS structurally UNSUPPORTED and every other kind supported-empty-or-
/// resolved once the real `SvelteFrameworkAdapter` is registered.
/// the `.ts` import-surface API file is matched by the registered Svelte api-projector
/// leg (the `framework_registry_complete` api-leg clause).
#[must_use]
pub fn svelte_descriptor() -> FrameworkAdapterDescriptor {
    FrameworkAdapterDescriptor {
        id: FrameworkAdapterId::svelte(),
        tag: FrameworkTag::Svelte,
        supported_surfaces: SVELTE_SUPPORTED_SURFACE_KINDS,
        carrier_language: Some(LanguageId::new("svelte")),
        virtual_file_naming: Some(VirtualFileNaming {
            // A `.svelte` COMPONENT always projects a fixed `.tsx` IDE file:
            // the projection emits TS with the `@jsxImportSource`
            // pragma — it is never JSX-conditional the way a Vue
            // `<script lang="jsx">` carrier is, so there is no `.jsx`
            // alternative. Its import surface is the `.ts` API file.
            ide: VirtualPathPolicy::Suffix(".tsx"),
            import_surface: VirtualPathPolicy::Suffix(".ts"),
            // No testing-API surface for Svelte (the testing surface is
            // Vue-only).
            testing_api_suffix: None,
            sidecar_suffixes: &[],
        }),
        // The Svelte carrier resolves the default-export component surface only
        // (a `.svelte` file is one component); a named-export framework surface
        // is not a distinct resolution for it.
        supports_named_export_surfaces: false,
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(naming.import_surface, VirtualPathPolicy::Suffix(".ts"));
        assert_eq!(naming.testing_api_suffix, Some(".__verter_test.ts"));
        assert_eq!(naming.sidecar_suffixes, &[] as &[&str]);
        assert!(naming.is_structurally_valid());
    }

    #[test]
    fn svelte_descriptor_projects_a_fixed_tsx_ide_file_with_no_testing_surface() {
        let d = svelte_descriptor();
        assert_eq!(d.id, FrameworkAdapterId::svelte());
        assert_eq!(d.tag, FrameworkTag::Svelte);
        assert_eq!(d.carrier_language, Some(LanguageId::new("svelte")));

        let naming = d
            .virtual_file_naming
            .as_ref()
            .expect("the Svelte descriptor carries a virtual-file naming column");
        // A `.svelte` COMPONENT projects a fixed `.tsx` IDE file and a
        // `.ts` import surface, and has NO testing-API surface (Vue-only).
        assert_eq!(naming.ide, VirtualPathPolicy::Suffix(".tsx"));
        assert_ne!(
            naming.ide,
            VirtualPathPolicy::JsxConditional {
                jsx: ".jsx",
                non_jsx: ".tsx",
            },
            "Svelte is NOT JSX-conditional"
        );
        assert_eq!(naming.import_surface, VirtualPathPolicy::Suffix(".ts"));
        assert_eq!(naming.testing_api_suffix, None);
        assert_eq!(naming.sidecar_suffixes, &[] as &[&str]);
        assert!(naming.is_structurally_valid());
    }

    #[test]
    fn svelte_rune_module_naming_is_same_file_with_no_component_surface() {
        // A standalone rune module uses the SAME-FILE model (NOT the
        // component dual-file model). Its IDE and import surfaces are BOTH
        // `SelfFile` (it serves its own canonical path), and it has NO testing
        // surface. This is the discriminating contrast with the component
        // carrier's `Suffix(".tsx")`/`Suffix(".ts")` dual-file model.
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
    fn testing_api_suffix_requires_a_distinct_import_surface_file() {
        // A testing-API suffix requires a distinct suffix-appended import
        // surface (a `SelfFile`/`None` import surface has no API file to vary).
        let invalid = VirtualFileNaming {
            ide: VirtualPathPolicy::None,
            import_surface: VirtualPathPolicy::None,
            testing_api_suffix: Some(".__verter_test.ts"),
            sidecar_suffixes: &[],
        };
        assert!(!invalid.is_structurally_valid());

        // A `SelfFile` import surface (rune module) also cannot carry a
        // testing-API suffix — there is no distinct API file.
        let invalid_self = VirtualFileNaming {
            ide: VirtualPathPolicy::SelfFile,
            import_surface: VirtualPathPolicy::SelfFile,
            testing_api_suffix: Some(".__verter_test.ts"),
            sidecar_suffixes: &[],
        };
        assert!(!invalid_self.is_structurally_valid());

        let valid = VirtualFileNaming {
            ide: VirtualPathPolicy::None,
            import_surface: VirtualPathPolicy::Suffix(".ts"),
            testing_api_suffix: Some(".__verter_test.ts"),
            sidecar_suffixes: &[],
        };
        assert!(valid.is_structurally_valid());
    }
}
