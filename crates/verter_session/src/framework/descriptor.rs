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

use std::sync::OnceLock;

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
    /// (`.verter.ts`, the reserved redirect-reached infix) for a component
    /// carrier, or the self file for a rune module.
    pub import_surface: VirtualPathPolicy,
    /// The testing-API virtual-file suffix. Structural rule: `Some` here
    /// REQUIRES `import_surface` to append a distinct file (a testing-API file
    /// is a variant of the import-surface API file, not of a `SelfFile`).
    pub testing_api_suffix: Option<&'static str>,
    /// Additional sidecar virtual-file suffixes the adapter emits.
    pub sidecar_suffixes: &'static [&'static str],
    /// How the DECLARATION carrier surface (`.d.<ext>.ts`) is named relative to
    /// the carrier source — the dedicated BARE-IMPORT-PROBED declaration file a
    /// bare framework-carrier import (`import B from "./B.vue"`) resolves to.
    ///
    /// This is a DISTINCT column from [`Self::import_surface`]: the import
    /// surface is the redirect-reached `.verter.` API file, while the
    /// declaration carrier is the extension-MIDDLE `.d.<ext>.ts` file tsgo's
    /// basename-append probe reaches FIRST. A component carrier sets
    /// [`DeclarationSurface::ExtensionMiddleTs`]; a non-projecting adapter sets
    /// [`DeclarationSurface::None`].
    pub declaration_surface: DeclarationSurface,
}

/// How an adapter's DECLARATION carrier surface (`.d.<ext>.ts`) is named.
///
/// A component carrier emits a real `.d.<ext>.ts` declaration file the bare
/// framework-carrier import resolves to (tsgo's basename-append probe order is
/// `.d.vue.ts` -> `.vue.ts` -> `.vue.tsx`). The declaration carrier is the
/// EXTENSION-MIDDLE form — `Foo.vue` -> `Foo.d.vue.ts` — NOT the extension-last
/// `Foo.vue.d.ts`, and it NEVER carries the redirect-reached `.verter.` infix
/// (that lives on the [`VirtualFileNaming::import_surface`] API file).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationSurface {
    /// No declaration carrier surface (a non-component adapter / rune module).
    None,
    /// The extension-MIDDLE `.d.<ext>.ts` declaration file: insert `.d.` between
    /// the carrier source's stem and its carrier extension, then append `.ts`
    /// (`Foo.vue` -> `Foo.d.vue.ts`, `Foo.svelte` -> `Foo.d.svelte.ts`).
    ExtensionMiddleTs,
}

/// The structural role a generated companion file plays for its carrier source.
///
/// Every descriptor-owned companion path maps to exactly one kind. The kind is the
/// role the [`VirtualFileNaming`] column assigns the path, recovered by INVERTING the
/// same forward transform the column uses to compose it — a path is a declaration
/// companion because it is exactly what the declaration transform emits, never because
/// its name happens to contain `.d.`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarrierCompanionKind {
    /// The IDE (type-checked) carrier surface (`Foo.vue.tsx` / `Foo.vue.jsx`).
    Ide,
    /// The extension-middle declaration carrier (`Foo.d.vue.ts`) a bare
    /// framework-carrier import resolves to.
    Declaration,
    /// The redirect-reached import-resolution API surface (`Foo.vue.verter.ts`).
    ImportSurface,
    /// The testing-API surface (`Foo.vue.__verter_test.ts`).
    TestingApi,
    /// An additional sidecar surface the adapter emits.
    Sidecar,
}

/// A generated companion file paired with the carrier SOURCE it projects from and the
/// structural role it plays.
///
/// The single typed result of the descriptor companion-classification authority: the
/// forward enumeration ([`VirtualFileNaming::carrier_companion_identities`]) yields one
/// per companion a source projects, and the reverse map
/// ([`VirtualFileNaming::carrier_source_for_companion`]) recovers the `(source, path,
/// kind)` triple from a companion path, so a consumer never re-derives a source with an
/// ad-hoc suffix strip. Forward and reverse are exact inverses for any descriptor whose
/// companion-suffix families are DISJOINT (no family's stripped suffix is itself a suffix
/// of another's) — the built-in Vue/Svelte descriptors are, locked by
/// `carrier_companion_identities_round_trip_through_source_for_companion`. The reverse map
/// resolves families by the FIXED order documented on
/// [`VirtualFileNaming::carrier_source_for_companion`] (not by rejecting overlaps), so for
/// a hypothetical overlapping-family descriptor that order — not a round-trip guarantee —
/// decides the attributed role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarrierCompanion {
    /// The carrier source (`Foo.vue`) this companion projects from.
    pub source: String,
    /// The generated companion path (`Foo.vue.tsx`, `Foo.d.vue.ts`, …).
    pub path: String,
    /// The structural role this companion plays for its source.
    pub kind: CarrierCompanionKind,
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
    /// dual-file API surface (`.verter.ts`, the reserved redirect-reached
    /// infix). `None` for a `SelfFile`/`None`/`JsxConditional` import surface (a
    /// rune module serves its own file; a JSX-conditional surface has no single
    /// fixed API suffix).
    #[must_use]
    pub fn api_surface_suffix(&self) -> Option<&'static str> {
        match self.import_surface {
            VirtualPathPolicy::Suffix(s) => Some(s),
            _ => None,
        }
    }

    /// Every distinct-file IDE-carrier suffix this naming policy can produce,
    /// in a stable order. This is the descriptor AUTHORITY for "which companion
    /// path(s) could the IDE carrier live at" — a consumer that needs to detect
    /// a real-file collision against the IDE carrier path must enumerate THESE,
    /// never a hardcoded `.tsx`.
    ///
    /// - [`VirtualPathPolicy::Suffix`] → that one suffix (Svelte's `.tsx`).
    /// - [`VirtualPathPolicy::JsxConditional`] → BOTH the `non_jsx` and `jsx`
    ///   suffixes (Vue's `.tsx` AND `.jsx`), because the carrier lives at one or
    ///   the other depending on the SFC's script lang, which is not known at
    ///   ownership time.
    /// - [`VirtualPathPolicy::SelfFile`] / [`VirtualPathPolicy::None`] → empty
    ///   (no distinct companion file — the rune-module / no-projection case).
    #[must_use]
    pub fn ide_carrier_suffixes(&self) -> Vec<&'static str> {
        match self.ide {
            VirtualPathPolicy::Suffix(s) => vec![s],
            VirtualPathPolicy::JsxConditional { jsx, non_jsx } => vec![non_jsx, jsx],
            VirtualPathPolicy::SelfFile | VirtualPathPolicy::None => Vec::new(),
        }
    }

    /// Every descriptor-valid IDE carrier IDENTITY for a carrier `source`, in a
    /// stable order: the FULL carrier source with each IDE suffix appended
    /// (`Foo.vue` + `.tsx` → `Foo.vue.tsx`, `Foo.vue` + `.jsx` → `Foo.vue.jsx`).
    ///
    /// This is the descriptor AUTHORITY for the IDE carrier's companion identity.
    /// The suffix is APPENDED to the full carrier source: the carrier extension
    /// (`.vue`/`.svelte`) is PRESERVED and NO infix is inserted between the
    /// source and the suffix, so the produced identity is EXACTLY the path a tsgo
    /// basename-append probe for the bare carrier import reaches. The reserved
    /// `.verter.` infix lives only on the redirect-reached
    /// [`api_surface_suffix`](Self::api_surface_suffix), never here. A consumer
    /// composing an IDE carrier path MUST route through this — never `format!`-ing
    /// a suffix onto a source itself, which is how the carrier extension gets
    /// dropped or the `.verter.` infix leaks onto the bare-probed IDE surface.
    #[must_use]
    pub fn ide_carrier_identities(&self, source: &str) -> Vec<String> {
        self.ide_carrier_suffixes()
            .into_iter()
            .map(|suffix| format!("{source}{suffix}"))
            .collect()
    }

    /// The descriptor-valid DECLARATION carrier identity for a carrier `source`:
    /// the EXTENSION-MIDDLE `.d.<ext>.ts` path a bare framework-carrier import
    /// resolves to. `Foo.vue` -> `Foo.d.vue.ts`, `Foo.svelte` -> `Foo.d.svelte.ts`.
    ///
    /// The transform inserts `.d.` between the carrier source's stem and its
    /// carrier extension, then appends `.ts`: the carrier extension is PRESERVED
    /// in extension-MIDDLE form (`.d.vue.ts`, never the extension-last
    /// `.vue.d.ts` tsgo would not bare-resolve), and NO `.verter.` infix is
    /// inserted (that reserved infix lives only on the redirect-reached
    /// [`api_surface_suffix`](Self::api_surface_suffix)). `None` when this naming
    /// policy projects no declaration carrier ([`DeclarationSurface::None`]) or
    /// the `source` has no carrier extension to wrap.
    ///
    /// This is the descriptor AUTHORITY for the declaration carrier's identity:
    /// a consumer composing a `.d.<ext>.ts` path MUST route through this — never
    /// `format!`-ing a suffix onto a source itself, which is how the carrier
    /// extension lands extension-last or the `.verter.` infix leaks onto the
    /// bare-probed declaration surface.
    #[must_use]
    pub fn declaration_carrier_identity(
        &self,
        source: &str,
        carrier_extension: Option<&str>,
    ) -> Option<String> {
        match self.declaration_surface {
            DeclarationSurface::None => None,
            DeclarationSurface::ExtensionMiddleTs => {
                // The carrier extension the owning adapter declares (derived from
                // its `carrier_language`, e.g. `.vue` / `.svelte`) — NEVER a
                // hand-matched literal here (`single_language_classifier`). The
                // transform applies ONLY to a `source` carrying THIS extension:
                // a `.ts` / `.js` / foreign-extension / extension-less source is
                // not a carrier path and yields `None` (never a fabricated
                // `Foo.d.ts.ts`).
                let carrier_ext = carrier_extension?;
                let stem = source.strip_suffix(carrier_ext)?;
                // The extension must sit on a non-empty BASENAME stem: the char
                // immediately before the extension must be a real basename
                // character, not a path separator (a bare `/.vue` is not a
                // carrier path) and the stem must not itself be empty.
                let last = stem.chars().next_back()?;
                if last == '/' || last == '\\' {
                    return None;
                }
                // Insert `.d.` between the stem and the carrier extension, then
                // append `.ts`: `{stem}{.d}{carrier_ext}{.ts}` =
                // `Foo` + `.d` + `.vue` + `.ts` → `Foo.d.vue.ts`.
                Some(format!("{stem}.d{carrier_ext}.ts"))
            }
        }
    }

    /// Every descriptor-valid companion IDENTITY for a carrier `source`, across ALL
    /// companion families (IDE, declaration, import-surface API, testing-API,
    /// sidecar), in a stable order.
    ///
    /// This is the descriptor AUTHORITY for "which occupiable path(s) does this carrier
    /// project" — a consumer detecting a real-file collision at ANY companion path, or
    /// reverse-mapping a companion to its source, enumerates THESE rather than a
    /// hardcoded suffix list. `carrier_extension` is the owning adapter's carrier
    /// extension (`.vue`/`.svelte`); companions are produced ONLY for a `source` that
    /// carries it on a non-empty basename stem — a non-carrier source
    /// (`.ts`/`.js`/foreign/extension-less) yields an empty list. Each family routes
    /// through its existing forward composer, so the produced paths are byte-identical
    /// to the ones the per-family authorities emit.
    #[must_use]
    pub fn carrier_companion_identities(
        &self,
        source: &str,
        carrier_extension: Option<&str>,
    ) -> Vec<CarrierCompanion> {
        // Companions exist only for a source carrying the adapter's carrier extension
        // on a non-empty basename stem (the same gate the declaration authority
        // applies), so a non-carrier source produces nothing.
        let Some(carrier_ext) = carrier_extension else {
            return Vec::new();
        };
        let Some(stem) = source.strip_suffix(carrier_ext) else {
            return Vec::new();
        };
        match stem.chars().next_back() {
            Some(last) if last != '/' && last != '\\' => {}
            _ => return Vec::new(),
        }

        let mut companions = Vec::new();
        for path in self.ide_carrier_identities(source) {
            companions.push(CarrierCompanion {
                source: source.to_string(),
                path,
                kind: CarrierCompanionKind::Ide,
            });
        }
        if let Some(path) = self.declaration_carrier_identity(source, carrier_extension) {
            companions.push(CarrierCompanion {
                source: source.to_string(),
                path,
                kind: CarrierCompanionKind::Declaration,
            });
        }
        if let Some(suffix) = self.api_surface_suffix() {
            companions.push(CarrierCompanion {
                source: source.to_string(),
                path: format!("{source}{suffix}"),
                kind: CarrierCompanionKind::ImportSurface,
            });
        }
        if let Some(suffix) = self.testing_api_suffix {
            companions.push(CarrierCompanion {
                source: source.to_string(),
                path: format!("{source}{suffix}"),
                kind: CarrierCompanionKind::TestingApi,
            });
        }
        for suffix in self.sidecar_suffixes {
            companions.push(CarrierCompanion {
                source: source.to_string(),
                path: format!("{source}{suffix}"),
                kind: CarrierCompanionKind::Sidecar,
            });
        }
        companions
    }

    /// The carrier SOURCE + companion role a `companion_path` projects from, or `None`
    /// when it is not a descriptor-valid companion of a carrier carrying
    /// `carrier_extension`.
    ///
    /// The reverse of [`Self::carrier_companion_identities`]: each family's forward
    /// transform is inverted and the recovered source is validated to carry the adapter's
    /// carrier extension on a non-empty basename stem, so a companion maps to its TRUE
    /// carrier source (`Foo.d.vue.ts` -> `Foo.vue`), never an intermediate stem a generic
    /// trailing-`.segment` strip would land on (`Foo.d.vue`). Families are tried in a
    /// FIXED order — declaration, import-surface API, testing-API, sidecar, then IDE LAST
    /// (its `.tsx`/`.jsx` suffixes are the broadest, so a more-specific `.ts` family
    /// claims an overlapping path first). The carrier-extension validation is the real
    /// authority; the order only decides the attributed role when two families' suffixes
    /// overlap. This is a TRUE inverse of the forward enumeration only for a descriptor
    /// whose families are DISJOINT (no family's stripped suffix is itself a suffix of
    /// another's) — the built-in Vue/Svelte descriptors are, locked by
    /// `carrier_companion_identities_round_trip_through_source_for_companion`; the reverse
    /// map does NOT reject a hypothetical overlapping-family descriptor.
    #[must_use]
    pub fn carrier_source_for_companion(
        &self,
        companion_path: &str,
        carrier_extension: Option<&str>,
    ) -> Option<CarrierCompanion> {
        let carrier_ext = carrier_extension?;

        // Validate a BORROWED candidate `source` (it is a carrier source only when it
        // carries the carrier extension on a non-empty basename stem — the shape a real
        // carrier path has) and allocate the owned [`CarrierCompanion`] ONLY then, so a
        // family whose suffix strips but whose recovered source is NOT a carrier (a plain
        // `foo.tsx` stripping the IDE `.tsx`) allocates nothing on the miss.
        let recovered = |source: &str, kind: CarrierCompanionKind| -> Option<CarrierCompanion> {
            let stem = source.strip_suffix(carrier_ext)?;
            let last = stem.chars().next_back()?;
            if last == '/' || last == '\\' {
                return None;
            }
            Some(CarrierCompanion {
                source: source.to_string(),
                path: companion_path.to_string(),
                kind,
            })
        };

        // Declaration: `{stem}.d{carrier_ext}.ts` -> `{stem}{carrier_ext}`. The
        // three-segment suffix is peeled with chained `strip_suffix` on borrowed data (no
        // combined `.d<ext>.ts` String is built before the match); the owned source is
        // `format!`-ed only once all three segments strip AND `{stem}` ends in a non-empty
        // basename character (a bare `/.d.vue.ts` is not a carrier companion).
        if matches!(
            self.declaration_surface,
            DeclarationSurface::ExtensionMiddleTs
        ) {
            if let Some(stem) = companion_path
                .strip_suffix(".ts")
                .and_then(|rest| rest.strip_suffix(carrier_ext))
                .and_then(|rest| rest.strip_suffix(".d"))
            {
                if stem
                    .chars()
                    .next_back()
                    .is_some_and(|last| last != '/' && last != '\\')
                {
                    return Some(CarrierCompanion {
                        source: format!("{stem}{carrier_ext}"),
                        path: companion_path.to_string(),
                        kind: CarrierCompanionKind::Declaration,
                    });
                }
            }
        }
        // Import-surface API: `{source}{suffix}` -> `{source}`.
        if let Some(suffix) = self.api_surface_suffix() {
            if let Some(source) = companion_path.strip_suffix(suffix) {
                if let Some(c) = recovered(source, CarrierCompanionKind::ImportSurface) {
                    return Some(c);
                }
            }
        }
        // Testing-API: `{source}{suffix}` -> `{source}`.
        if let Some(suffix) = self.testing_api_suffix {
            if let Some(source) = companion_path.strip_suffix(suffix) {
                if let Some(c) = recovered(source, CarrierCompanionKind::TestingApi) {
                    return Some(c);
                }
            }
        }
        // Sidecar: `{source}{suffix}` -> `{source}`.
        for suffix in self.sidecar_suffixes {
            if let Some(source) = companion_path.strip_suffix(suffix) {
                if let Some(c) = recovered(source, CarrierCompanionKind::Sidecar) {
                    return Some(c);
                }
            }
        }
        // IDE: `{source}{suffix}` -> `{source}` (the broadest suffixes; checked LAST so a
        // more-specific `.ts` family claims an overlapping path first). The policy's
        // suffix(es) are read inline — no `Vec` is materialized; the owned source is
        // allocated only when a suffix strips AND the recovered source validates as a
        // carrier. Order matches `ide_carrier_suffixes()` (non-JSX before JSX).
        let ide_recovered = match &self.ide {
            VirtualPathPolicy::Suffix(suffix) => companion_path
                .strip_suffix(*suffix)
                .and_then(|source| recovered(source, CarrierCompanionKind::Ide)),
            VirtualPathPolicy::JsxConditional { jsx, non_jsx } => companion_path
                .strip_suffix(*non_jsx)
                .and_then(|source| recovered(source, CarrierCompanionKind::Ide))
                .or_else(|| {
                    companion_path
                        .strip_suffix(*jsx)
                        .and_then(|source| recovered(source, CarrierCompanionKind::Ide))
                }),
            VirtualPathPolicy::SelfFile | VirtualPathPolicy::None => None,
        };
        if let Some(c) = ide_recovered {
            return Some(c);
        }
        None
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
        virtual_file_naming: Some(VirtualFileNaming {
            ide: VirtualPathPolicy::JsxConditional {
                jsx: ".jsx",
                non_jsx: ".tsx",
            },
            // The public-API / import-resolution carrier is REDIRECT-reached
            // (never bare-import-probed), so it carries the Verter-reserved
            // `.verter.` infix uniformly across adapters. A bare `.svelte.ts`
            // API carrier would collide with a Svelte rune module (GATE 5: tsgo
            // probes `.svelte.ts` before `.svelte.tsx`); `.verter.ts` collides
            // with no real adapter source or rune-module extension.
            import_surface: VirtualPathPolicy::Suffix(".verter.ts"),
            // The testing-API surface stays `.__verter_test.ts`: it is itself a
            // redirect-reached, non-bare-probed surface, and `.svelte.__verter_test.ts`
            // is not a rune-module extension, so it is already collision-free.
            testing_api_suffix: Some(".__verter_test.ts"),
            sidecar_suffixes: &[],
            // The bare `import B from "./B.vue"` declaration carrier is the
            // extension-middle `B.d.vue.ts` — the path tsgo's basename-append
            // probe reaches first (`.d.vue.ts` -> `.vue.ts` -> `.vue.tsx`).
            declaration_surface: DeclarationSurface::ExtensionMiddleTs,
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

/// The process-wide cached built-in descriptor list, built ONCE. The registry-level
/// forward/reverse companion dispatch reads THIS rather than rebuilding the descriptor
/// rows (each row owns interned ids + a naming column) on every classification. The
/// completeness guard and tests still take the owned [`built_in_descriptors`]; this is
/// the hot-path reader.
fn built_in_descriptors_cached() -> &'static [FrameworkAdapterDescriptor] {
    static DESCRIPTORS: OnceLock<Vec<FrameworkAdapterDescriptor>> = OnceLock::new();
    DESCRIPTORS.get_or_init(built_in_descriptors)
}

/// The process-wide cached dotted carrier extensions (`Some(".vue")` / `Some(".svelte")`
/// / `None`) for the built-in descriptors, in the SAME order as
/// [`built_in_descriptors_cached`], each derived ONCE from its descriptor's
/// [`FrameworkAdapterDescriptor::carrier_extension`]. The registry-level reverse/forward
/// companion dispatch BORROWS these rather than deriving a fresh `String` per descriptor
/// probe, so — after this `OnceLock` is initialized once — a classify miss over a common
/// non-companion path (`util.ts`) allocates nothing per call. Still DERIVED from each
/// descriptor's `carrier_language`, never a hand-matched
/// extension literal. The owned [`FrameworkAdapterDescriptor::carrier_source_for_companion`]
/// / [`FrameworkAdapterDescriptor::carrier_companion_identities`] wrappers stay the
/// adapter-aware API (exercised by the round-trip test); this is the hot-path reader.
fn built_in_descriptor_carrier_extensions_cached() -> &'static [Option<String>] {
    static EXTENSIONS: OnceLock<Vec<Option<String>>> = OnceLock::new();
    EXTENSIONS.get_or_init(|| {
        built_in_descriptors_cached()
            .iter()
            .map(|descriptor| descriptor.carrier_extension())
            .collect()
    })
}

/// Classify a generated companion `path` to its carrier SOURCE + role by dispatching
/// across every built-in adapter descriptor — the registry-level reverse authority.
///
/// The reverse map runs on the BORROWED raw `path`: companion suffixes carry no path
/// separator, so the suffix strip and the basename-stem check are separator-invariant, and a
/// path that is not any adapter's companion (a plain `.ts`/`.tsx` baseline, a non-companion
/// file) — forward-slash OR backslash — yields `None`. After the one-time process-wide
/// descriptor/extension `OnceLock` caches are initialized, that miss borrows the raw
/// `path` and allocates nothing; an owned [`CarrierCompanion`] is allocated only on a
/// successful match. Each
/// descriptor's naming column is reverse-mapped through the shared authority
/// ([`VirtualFileNaming::carrier_source_for_companion`]), BORROWING the descriptor's
/// process-cached carrier extension; the FIRST match wins (companion paths are collision-free
/// across the built-in adapters' carrier extensions, so at most one descriptor matches).
/// Separator normalization (`\` → `/`) is applied to the recovered source/path ONLY after a
/// companion matches, so a backslash companion still reverse-maps to the same forward-slashed
/// identity a forward-slashed input would. Framework-agnostic: a new adapter participates the
/// moment its descriptor is registered.
#[must_use]
pub fn classify_carrier_companion(path: &str) -> Option<CarrierCompanion> {
    // Reverse-map the BORROWED raw path directly: companion suffixes carry no separator, so
    // matching is separator-invariant and, once the process-wide descriptor/extension
    // `OnceLock` caches are initialized, a non-companion path (forward-slash OR backslash)
    // allocates nothing on the miss.
    let mut companion = built_in_descriptors_cached()
        .iter()
        .zip(built_in_descriptor_carrier_extensions_cached())
        .find_map(|(descriptor, carrier_ext)| {
            descriptor
                .virtual_file_naming
                .as_ref()?
                .carrier_source_for_companion(path, carrier_ext.as_deref())
        })?;
    // Normalize the recovered identity to forward slashes ONLY after a match, so a backslash
    // companion yields the same source/path a forward-slashed input would (the suffix strip
    // that matched removed only separator-free trailing bytes, so the recovered form differs
    // from the normalized one by separators alone).
    if path.contains('\\') {
        companion.source = companion.source.replace('\\', "/");
        companion.path = companion.path.replace('\\', "/");
    }
    Some(companion)
}

/// Every descriptor-valid companion IDENTITY for a carrier `source` across every
/// built-in adapter — the registry-level forward authority. A source produces
/// companions only under the ONE adapter whose carrier extension it carries; a
/// non-carrier source yields an empty list. BORROWS each descriptor's process-cached
/// carrier extension (no per-source `String` derivation). Framework-agnostic: a new
/// adapter participates the moment its descriptor is registered.
#[must_use]
pub fn carrier_companion_identities_for_source(source: &str) -> Vec<CarrierCompanion> {
    built_in_descriptors_cached()
        .iter()
        .zip(built_in_descriptor_carrier_extensions_cached())
        .flat_map(|(descriptor, carrier_ext)| {
            descriptor
                .virtual_file_naming
                .as_ref()
                .map(|naming| naming.carrier_companion_identities(source, carrier_ext.as_deref()))
                .unwrap_or_default()
        })
        .collect()
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
        virtual_file_naming: Some(VirtualFileNaming {
            // A `.svelte` COMPONENT always projects a fixed `.tsx` IDE file:
            // the projection emits TS with the `@jsxImportSource`
            // pragma — it is never JSX-conditional the way a Vue
            // `<script lang="jsx">` carrier is, so there is no `.jsx`
            // alternative. Its import surface is the `.verter.ts` API file.
            ide: VirtualPathPolicy::Suffix(".tsx"),
            // The public-API / import-resolution carrier carries the reserved
            // `.verter.` infix: a bare `.svelte.ts` API carrier would collide
            // with a Svelte rune module (GATE 5 — `.svelte.ts` is probed before
            // `.svelte.tsx`). This carrier is redirect-reached, never
            // bare-probed, so the reserved infix breaks no probe.
            import_surface: VirtualPathPolicy::Suffix(".verter.ts"),
            // No testing-API surface for Svelte (the testing surface is
            // Vue-only).
            testing_api_suffix: None,
            sidecar_suffixes: &[],
            // The bare `import C from "./C.svelte"` declaration carrier is the
            // extension-middle `C.d.svelte.ts` — the path tsgo's basename-append
            // probe reaches first (`.d.svelte.ts` -> `.svelte.ts` -> `.svelte.tsx`).
            declaration_surface: DeclarationSurface::ExtensionMiddleTs,
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
        // A rune module is a script, not a component carrier — it projects no
        // declaration carrier (it serves its own `SelfFile` path).
        declaration_surface: DeclarationSurface::None,
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
        // The public-API carrier carries the reserved `.verter.` infix
        // (redirect-reached, never bare-probed — GATE 5).
        assert_eq!(
            naming.import_surface,
            VirtualPathPolicy::Suffix(".verter.ts")
        );
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
        // `.verter.ts` import surface, and has NO testing-API surface (Vue-only).
        assert_eq!(naming.ide, VirtualPathPolicy::Suffix(".tsx"));
        assert_ne!(
            naming.ide,
            VirtualPathPolicy::JsxConditional {
                jsx: ".jsx",
                non_jsx: ".tsx",
            },
            "Svelte is NOT JSX-conditional"
        );
        // The public-API carrier carries the reserved `.verter.` infix: a bare
        // `.svelte.ts` would collide with a rune module (GATE 5).
        assert_eq!(
            naming.import_surface,
            VirtualPathPolicy::Suffix(".verter.ts")
        );
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
    fn testing_api_suffix_requires_a_distinct_import_surface_file() {
        // A testing-API suffix requires a distinct suffix-appended import
        // surface (a `SelfFile`/`None` import surface has no API file to vary).
        let invalid = VirtualFileNaming {
            ide: VirtualPathPolicy::None,
            import_surface: VirtualPathPolicy::None,
            testing_api_suffix: Some(".__verter_test.ts"),
            sidecar_suffixes: &[],
            declaration_surface: DeclarationSurface::None,
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
        };
        assert!(!invalid_self.is_structurally_valid());

        let valid = VirtualFileNaming {
            ide: VirtualPathPolicy::None,
            import_surface: VirtualPathPolicy::Suffix(".ts"),
            testing_api_suffix: Some(".__verter_test.ts"),
            sidecar_suffixes: &[],
            declaration_surface: DeclarationSurface::None,
        };
        assert!(valid.is_structurally_valid());
    }

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
