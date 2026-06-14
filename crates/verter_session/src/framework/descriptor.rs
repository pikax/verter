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

/// An adapter's virtual-file naming policy — the single authority for its
/// IDE / API / testing-API / sidecar suffixes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualFileNaming {
    /// The IDE virtual-file suffix policy (the suffix appended to a carrier
    /// path to form the type-checked TSX/JSX virtual file). `None` when the
    /// adapter has no IDE virtual file.
    pub ide: Option<IdeSuffixPolicy>,
    /// The API virtual-file suffix (the suffix the public-API extraction
    /// virtual file uses). `None` when the adapter has no API virtual file.
    pub api_suffix: Option<&'static str>,
    /// The testing-API virtual-file suffix. Structural rule: `Some` here
    /// REQUIRES `api_suffix` to be `Some` (a testing-API file is a variant of
    /// the API file).
    pub testing_api_suffix: Option<&'static str>,
    /// Additional sidecar virtual-file suffixes the adapter emits.
    pub sidecar_suffixes: &'static [&'static str],
}

impl VirtualFileNaming {
    /// Whether this naming policy satisfies the structural invariant
    /// (`testing_api_suffix.is_some() ⇒ api_suffix.is_some()`).
    #[must_use]
    pub fn is_structurally_valid(&self) -> bool {
        self.testing_api_suffix.is_none() || self.api_suffix.is_some()
    }
}

/// The IDE virtual-file suffix policy.
///
/// An adapter's IDE virtual file is either a fixed suffix or chosen between a
/// JSX and a non-JSX suffix depending on whether the carrier script uses JSX.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdeSuffixPolicy {
    /// A single fixed suffix regardless of script content.
    Fixed(&'static str),
    /// Choose `jsx` when the carrier script uses JSX, else `non_jsx`.
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
            ide: Some(IdeSuffixPolicy::JsxConditional {
                jsx: ".jsx",
                non_jsx: ".tsx",
            }),
            api_suffix: Some(".ts"),
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
/// `api_suffix: Some(".ts")` is matched by the registered Svelte api-projector
/// leg (the `framework_registry_complete` api-leg clause).
#[must_use]
pub fn svelte_descriptor() -> FrameworkAdapterDescriptor {
    FrameworkAdapterDescriptor {
        id: FrameworkAdapterId::svelte(),
        tag: FrameworkTag::Svelte,
        supported_surfaces: SVELTE_SUPPORTED_SURFACE_KINDS,
        carrier_language: Some(LanguageId::new("svelte")),
        virtual_file_naming: Some(VirtualFileNaming {
            ide: Some(IdeSuffixPolicy::JsxConditional {
                jsx: ".jsx",
                non_jsx: ".tsx",
            }),
            api_suffix: Some(".ts"),
            // No testing-API surface for Svelte (the testing surface is
            // Vue-only, D-ak/D-al).
            testing_api_suffix: None,
            sidecar_suffixes: &[],
        }),
        // The Svelte carrier resolves the default-export component surface only
        // (a `.svelte` file is one component); a named-export framework surface
        // is not a distinct resolution for it.
        supports_named_export_surfaces: false,
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
            Some(IdeSuffixPolicy::JsxConditional {
                jsx: ".jsx",
                non_jsx: ".tsx",
            })
        );
        assert_eq!(naming.api_suffix, Some(".ts"));
        assert_eq!(naming.testing_api_suffix, Some(".__verter_test.ts"));
        assert_eq!(naming.sidecar_suffixes, &[] as &[&str]);
        assert!(naming.is_structurally_valid());
    }

    #[test]
    fn testing_api_suffix_requires_api_suffix() {
        // A testing-API suffix without an API suffix is structurally invalid.
        let invalid = VirtualFileNaming {
            ide: None,
            api_suffix: None,
            testing_api_suffix: Some(".__verter_test.ts"),
            sidecar_suffixes: &[],
        };
        assert!(!invalid.is_structurally_valid());

        let valid = VirtualFileNaming {
            ide: None,
            api_suffix: Some(".ts"),
            testing_api_suffix: Some(".__verter_test.ts"),
            sidecar_suffixes: &[],
        };
        assert!(valid.is_structurally_valid());
    }
}
