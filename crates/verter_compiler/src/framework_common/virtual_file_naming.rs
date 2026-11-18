//! Descriptor-owned companion naming shared by compiler and session.

use std::sync::OnceLock;

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
    /// How the CHECKING module of a carrier is named: the one module a
    /// carrier's script and template compose into for whole-program type
    /// checking, served beside the carrier so the engine's own extension
    /// probing resolves the authored `./Foo.vue` specifier to it.
    pub checking_surface: CheckingSurface,
}

/// How an adapter names the CHECKING module of a carrier.
///
/// The name is the full carrier source with the module's dialect suffix
/// appended (`Foo.vue` checked as TypeScript is `Foo.vue.ts`). The suffix
/// follows the module's script dialect, because the engine parses the file
/// by its extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckingSurface {
    /// The adapter composes no checking module.
    None,
    /// One suffix per checking dialect, each with its leading dot
    /// (`.ts`, `.tsx`, `.js`, `.jsx`).
    DialectSuffixes(&'static [&'static str]),
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
    /// The IDE carrier surface (`Foo.vue.tsx`). Shared checking suffixes
    /// retain this role and recover the same carrier.
    Ide,
    /// A checking carrier surface not already owned by the IDE policy.
    Checking,
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
/// ad-hoc suffix strip. Checking and IDE suffixes may overlap when both
/// recover the same source; the IDE role wins and enumeration includes that
/// path once. Families whose recovered carriers differ must be disjoint for
/// a source round-trip. The reverse map uses the fixed precedence documented
/// on [`VirtualFileNaming::carrier_source_for_companion`].
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

    /// The suffix, with its leading dot, a checking module in the dialect
    /// whose extension is `dialect_extension` (`"ts"`, `"tsx"`, …) carries;
    /// `None` when this naming projects no checking module or does not name
    /// that dialect.
    #[must_use]
    pub fn checking_module_suffix(&self, dialect_extension: &str) -> Option<&'static str> {
        let CheckingSurface::DialectSuffixes(suffixes) = self.checking_surface else {
            return None;
        };
        suffixes
            .iter()
            .copied()
            .find(|suffix| suffix.strip_prefix('.') == Some(dialect_extension))
    }

    /// The CHECKING module identity of a carrier `source` checked in the
    /// dialect whose extension is `dialect_extension` (`"ts"`, `"tsx"`, …,
    /// without a dot): the full source with that dialect's suffix appended
    /// (`Foo.vue` + `.ts` -> `Foo.vue.ts`).
    ///
    /// `None` when this naming projects no checking module, when the dialect
    /// is not one the column names, or when `source` does not carry the
    /// adapter's carrier extension on a non-empty basename stem, or when a
    /// more-specific companion family owns the identity for another carrier. A consumer
    /// naming a checking module routes through this, never `format!`-ing an
    /// extension onto a source itself.
    #[must_use]
    pub fn checking_module_identity(
        &self,
        source: &str,
        carrier_extension: Option<&str>,
        dialect_extension: &str,
    ) -> Option<String> {
        let suffix = self.checking_module_suffix(dialect_extension)?;
        let stem = source.strip_suffix(carrier_extension?)?;
        let last = stem.chars().next_back()?;
        if last == '/' || last == '\\' {
            return None;
        }
        let path = format!("{source}{suffix}");
        let companion = self.carrier_source_for_companion(&path, carrier_extension)?;
        (companion.source == source).then_some(path)
    }

    /// The declaration a carrier `source` is published under: the file the
    /// engine emits for the carrier's TypeScript checking module (`Foo.vue`,
    /// checked as `Foo.vue.ts`, publishes `Foo.vue.d.ts`). A consumer's
    /// `./Foo.vue` import reaches it through TypeScript's own extension
    /// probe. `None` when this naming checks no TypeScript module for
    /// `source`.
    #[must_use]
    pub fn published_declaration_identity(
        &self,
        source: &str,
        carrier_extension: Option<&str>,
    ) -> Option<String> {
        let suffix = self.checking_module_suffix("ts")?;
        self.checking_module_identity(source, carrier_extension, "ts")?;
        Some(format!("{source}.d{suffix}"))
    }

    /// Every descriptor-valid companion IDENTITY for a carrier `source`, across ALL
    /// companion families (IDE, declaration, import-surface API, testing-API,
    /// sidecar, checking), in a stable order. Checking identities already
    /// occupied by another family are included once, with that family's role.
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
        if let CheckingSurface::DialectSuffixes(suffixes) = self.checking_surface {
            for suffix in suffixes {
                let Some(dialect) = suffix.strip_prefix('.') else {
                    continue;
                };
                if let Some(path) =
                    self.checking_module_identity(source, carrier_extension, dialect)
                {
                    if !companions.iter().any(|companion| companion.path == path) {
                        companions.push(CarrierCompanion {
                            source: source.to_string(),
                            path,
                            kind: CarrierCompanionKind::Checking,
                        });
                    }
                }
            }
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
    /// FIXED order — declaration, import-surface API, testing-API, sidecar, then IDE
    /// (its `.tsx`/`.jsx` suffixes are the broadest, so a more-specific `.ts` family
    /// claims an overlapping path first), followed by checking dialects. The
    /// carrier-extension validation is the real
    /// authority; the order only decides the attributed role when two families' suffixes
    /// overlap. Checking and IDE suffixes can share a path when they recover
    /// the same carrier, retaining the IDE role. Families whose carriers
    /// differ require disjoint paths to guarantee a source round-trip.
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
        // Checking dialects can include surfaces the IDE policy does not emit.
        // Keep the existing families' precedence when the policies overlap.
        if let CheckingSurface::DialectSuffixes(suffixes) = self.checking_surface {
            for suffix in suffixes {
                if let Some(source) = companion_path.strip_suffix(suffix) {
                    if let Some(companion) = recovered(source, CarrierCompanionKind::Checking) {
                        return Some(companion);
                    }
                }
            }
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

/// The module STEM of the virtualized `@verter/types` sidecar: the segment the
/// producer appends to a carrier's IDE-companion BASENAME to form the relative
/// import specifier a rewritten `@verter/types` import resolves through
/// (`./App.vue.tsx.__verter_types`). The on-disk overlay adds the `.d.ts`
/// declaration extension on top ([`VERTER_TYPES_SIDECAR_EXT`]) — TypeScript's
/// extensionless probe resolves the specifier to that overlay.
pub const VERTER_TYPES_SIDECAR_MODULE_STEM: &str = ".__verter_types";

/// The on-disk extension of the virtualized `@verter/types` sidecar overlay,
/// appended to a carrier's FULL IDE-companion path
/// (`{ide_companion}.__verter_types.d.ts`). Exactly
/// [`VERTER_TYPES_SIDECAR_MODULE_STEM`] + `.d.ts` — pinned by
/// `verter_types_sidecar_column_derives_from_the_module_stem`.
pub const VERTER_TYPES_SIDECAR_EXT: &str = ".__verter_types.d.ts";

/// The adjacent `@verter/types` fallback overlay the LSP publishes BESIDE a
/// carrier's IDE companion when the owning project cannot resolve the
/// `@verter/types` package (`ProjectSync::publish_tsx` →
/// `prepare_carrier_provider_surface`): `{ide_companion}.__verter_types.d.ts`,
/// one form per IDE suffix (`.tsx` / `.jsx`). Relative to the carrier SOURCE the
/// sidecar is therefore `{source}{ide_suffix}.__verter_types.d.ts`.
///
/// Naming the sidecar in the descriptor column is what lets
/// [`classify_carrier_companion`] reverse-map it to its carrier source, so every
/// companion-family consumer treats it as part of the carrier family — above all
/// the SHARED tsgo overlay, whose record/inject scope keys on
/// `carrier_source_of` and would otherwise DROP the sidecar: the editor-owned
/// Program then fails the rewritten `__verter_types` import (TS2307), the JSX
/// helper surface collapses, template hover degrades to `any`, and template
/// completions return null while the managed lane (which receives the sidecar
/// through its own lifecycle verbs) keeps serving typed answers.
///
/// Each element is `{ide_suffix}{VERTER_TYPES_SIDECAR_EXT}` (literal because the
/// column is `&'static`; the derivation is pinned by
/// `verter_types_sidecar_column_derives_from_the_module_stem`). The producer
/// never spells these strings itself: creation, the import specifier, and
/// cleanup all route through [`verter_types_sidecar_path`] /
/// [`verter_types_import_specifier`], so a rename here and the published paths
/// cannot drift apart.
const VERTER_TYPES_SIDECAR_SUFFIXES: &[&str] =
    &[".tsx.__verter_types.d.ts", ".jsx.__verter_types.d.ts"];

/// The final path segment of an IDE-companion path, or `None` when the path
/// names no file (empty, or separator-terminated — `/ws/src/`, `C:\ws\`,
/// `\\?\C:\ws\`). Separator-neutral (`/` and `\`), so a Windows provider path
/// answers the same as a normalized one. The ONE basename predicate BOTH
/// sidecar derivations below share, so the overlay path and its import
/// specifier refuse on exactly the same inputs and can never strand each other.
fn ide_companion_basename(ide_companion_path: &str) -> Option<&str> {
    ide_companion_path
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())
}

/// The virtualized `@verter/types` sidecar overlay PATH for a carrier's IDE
/// companion — the ONE forward derivation the producer
/// (`prepare_carrier_provider_surface`), the cleanup path
/// (`ProjectSync::close_virtual_verter_types`), and tests consume. Appending to
/// the FULL IDE-companion path is what keeps the result classifiable: the
/// companion is `{source}{ide_suffix}`, so the sidecar is
/// `{source}{ide_suffix}{VERTER_TYPES_SIDECAR_EXT}` — exactly the
/// `sidecar_suffixes` column entry for that IDE suffix.
///
/// `None` for a path with no basename ([`ide_companion_basename`]): a
/// basename-less input names no file to sit beside, and fabricating
/// `{directory}{VERTER_TYPES_SIDECAR_EXT}` would mint a path the classifier
/// does not recognize — the exact drift class this API exists to prevent.
/// Fail-closed on the SAME predicate as [`verter_types_import_specifier`].
#[must_use]
pub fn verter_types_sidecar_path(ide_companion_path: &str) -> Option<String> {
    ide_companion_basename(ide_companion_path)
        .map(|_| format!("{ide_companion_path}{VERTER_TYPES_SIDECAR_EXT}"))
}

/// The RELATIVE import specifier a rewritten `@verter/types` import resolves the
/// sidecar overlay through: `./{ide_companion_basename}.__verter_types`
/// (extensionless — TypeScript's declaration probe appends `.d.ts` and lands on
/// [`verter_types_sidecar_path`]'s overlay). `None` for a path with no basename
/// ([`ide_companion_basename`] — the same predicate the overlay-path derivation
/// refuses on), mirroring the producer's fail-closed handling. Separator-neutral
/// (`/` and `\`), so a Windows provider path derives the same specifier a
/// normalized one would.
#[must_use]
pub fn verter_types_import_specifier(ide_companion_path: &str) -> Option<String> {
    let basename = ide_companion_basename(ide_companion_path)?;
    Some(format!("./{basename}{VERTER_TYPES_SIDECAR_MODULE_STEM}"))
}

/// The Vue carrier's authoritative virtual naming policy.
pub const VUE_VIRTUAL_FILE_NAMING: VirtualFileNaming = VirtualFileNaming {
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
    sidecar_suffixes: VERTER_TYPES_SIDECAR_SUFFIXES,
    // The bare `import B from "./B.vue"` declaration carrier is the
    // extension-middle `B.d.vue.ts` — the path tsgo's basename-append
    // probe reaches first (`.d.vue.ts` -> `.vue.ts` -> `.vue.tsx`).
    declaration_surface: DeclarationSurface::ExtensionMiddleTs,
    // The checking module sits at the carrier path plus its dialect
    // suffix, the path the engine's extension probe reaches for the
    // authored `./Foo.vue` specifier.
    checking_surface: CheckingSurface::DialectSuffixes(&[".ts", ".tsx", ".js", ".jsx"]),
};

/// The Svelte carrier's authoritative virtual naming policy.
pub const SVELTE_VIRTUAL_FILE_NAMING: VirtualFileNaming = VirtualFileNaming {
    // A `.svelte` COMPONENT always projects a fixed `.tsx` IDE file:
    // the projection emits TS with the `@jsxImportSource`
    // pragma. A no-lang JavaScript component is published as `.jsx`
    // with JSDoc while a TypeScript component is published as `.tsx`.
    // Its import surface remains the `.verter.ts` API file.
    ide: VirtualPathPolicy::JsxConditional {
        jsx: ".jsx",
        non_jsx: ".tsx",
    },
    // The public-API / import-resolution carrier carries the reserved
    // `.verter.` infix: a bare `.svelte.ts` API carrier would collide
    // with a Svelte rune module (GATE 5 — `.svelte.ts` is probed before
    // `.svelte.tsx`). This carrier is redirect-reached, never
    // bare-probed, so the reserved infix breaks no probe.
    import_surface: VirtualPathPolicy::Suffix(".verter.ts"),
    // No testing-API surface for Svelte (the testing surface is
    // Vue-only).
    testing_api_suffix: None,
    sidecar_suffixes: VERTER_TYPES_SIDECAR_SUFFIXES,
    // The bare `import C from "./C.svelte"` declaration carrier is the
    // extension-middle `C.d.svelte.ts` — the path tsgo's basename-append
    // probe reaches first (`.d.svelte.ts` -> `.svelte.ts` -> `.svelte.tsx`).
    declaration_surface: DeclarationSurface::ExtensionMiddleTs,
    // No checking module is composed for a Svelte component.
    checking_surface: CheckingSurface::None,
};

/// The authoritative built-in carrier naming rows, with extensions derived
/// once from the same profile manifests the carrier frontends register.
fn built_in_namings_cached() -> &'static [(VirtualFileNaming, String)] {
    static NAMINGS: OnceLock<Vec<(VirtualFileNaming, String)>> = OnceLock::new();
    NAMINGS.get_or_init(|| {
        [
            (
                &super::vue_carrier_frontend::VUE_PROFILE_MANIFEST,
                &VUE_VIRTUAL_FILE_NAMING,
            ),
            (
                &crate::svelte::carrier_frontend::SVELTE_PROFILE_MANIFEST,
                &SVELTE_VIRTUAL_FILE_NAMING,
            ),
        ]
        .into_iter()
        .flat_map(|(profile, naming)| {
            profile
                .claimed_carriers
                .iter()
                .map(move |language| (naming.clone(), format!(".{language}")))
        })
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
/// moment its profile and naming policy join the built-in rows.
#[must_use]
pub fn classify_carrier_companion(path: &str) -> Option<CarrierCompanion> {
    // Reverse-map the BORROWED raw path directly: companion suffixes carry no separator, so
    // matching is separator-invariant and, once the process-wide descriptor/extension
    // `OnceLock` caches are initialized, a non-companion path (forward-slash OR backslash)
    // allocates nothing on the miss.
    let mut companion = built_in_namings_cached()
        .iter()
        .find_map(|(naming, carrier_ext)| {
            naming.carrier_source_for_companion(path, Some(carrier_ext))
        })?;
    // Normalize the recovered identity to forward slashes ONLY after a match, so a backslash
    // companion yields the same source/path a forward-slashed input would (the suffix strip
    // that matched removed only separator-free trailing bytes, so the recovered form differs
    // from the normalized one by separators alone).
    if path.contains('\\') {
        companion.source =
            verter_span::path::normalize_path_separators(&companion.source).into_owned();
        companion.path = verter_span::path::normalize_path_separators(&companion.path).into_owned();
    }
    Some(companion)
}

/// Classify a module reference, including the extensionless spellings emitted
/// for private API and helper companions. Bare carrier references remain bare:
/// only reserved families supply an extensionless spelling.
pub(crate) fn classify_carrier_companion_reference(reference: &str) -> Option<CarrierCompanion> {
    if let Some(companion) = classify_carrier_companion(reference) {
        return Some(companion);
    }
    static SUFFIXES: OnceLock<Vec<(String, &'static str)>> = OnceLock::new();
    let suffixes = SUFFIXES.get_or_init(|| {
        let mut suffixes = Vec::new();
        for (naming, _) in built_in_namings_cached() {
            for suffix in naming
                .api_surface_suffix()
                .into_iter()
                .chain(naming.testing_api_suffix)
            {
                if let Some((logical, _)) = suffix.rsplit_once('.') {
                    if !logical.is_empty() {
                        let pair = (logical.to_owned(), suffix);
                        if !suffixes.contains(&pair) {
                            suffixes.push(pair);
                        }
                    }
                }
            }
            for &suffix in naming.sidecar_suffixes {
                if let Some(prefix) = suffix.strip_suffix(VERTER_TYPES_SIDECAR_EXT) {
                    let pair = (
                        format!("{prefix}{VERTER_TYPES_SIDECAR_MODULE_STEM}"),
                        suffix,
                    );
                    if !suffixes.contains(&pair) {
                        suffixes.push(pair);
                    }
                }
            }
        }
        suffixes
    });
    suffixes.iter().find_map(|(logical, physical)| {
        let source = reference.strip_suffix(logical.as_str())?;
        classify_carrier_companion(&format!("{source}{physical}"))
    })
}

/// Every descriptor-valid companion IDENTITY for a carrier `source` across every
/// built-in adapter — the registry-level forward authority. A source produces
/// companions only under the ONE adapter whose carrier extension it carries; a
/// non-carrier source yields an empty list. BORROWS each descriptor's process-cached
/// carrier extension (no per-source `String` derivation). Framework-agnostic: a new
/// adapter participates when its profile and naming policy join the built-in rows.
#[must_use]
pub fn carrier_companion_identities_for_source(source: &str) -> Vec<CarrierCompanion> {
    built_in_namings_cached()
        .iter()
        .flat_map(|(naming, carrier_ext)| {
            naming.carrier_companion_identities(source, Some(carrier_ext))
        })
        .collect()
}

/// The checking module of a carrier `source` in the dialect whose extension
/// is `dialect_extension`, under the ONE built-in adapter whose carrier
/// extension it carries ([`VirtualFileNaming::checking_module_identity`]).
#[must_use]
pub fn checking_module_identity_for_source(
    source: &str,
    dialect_extension: &str,
) -> Option<String> {
    built_in_namings_cached()
        .iter()
        .find_map(|(naming, carrier_ext)| {
            naming.checking_module_identity(source, Some(carrier_ext), dialect_extension)
        })
}

/// The declaration a carrier `source` is published under, across every
/// built-in adapter ([`VirtualFileNaming::published_declaration_identity`]).
#[must_use]
pub fn published_declaration_identity_for_source(source: &str) -> Option<String> {
    built_in_namings_cached()
        .iter()
        .find_map(|(naming, carrier_ext)| {
            naming.published_declaration_identity(source, Some(carrier_ext))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_carrier_publishes_the_declaration_its_typescript_checking_module_emits() {
        assert_eq!(
            checking_module_identity_for_source("/ws/src/Foo.vue", "ts").as_deref(),
            Some("/ws/src/Foo.vue.ts")
        );
        assert_eq!(
            published_declaration_identity_for_source("/ws/src/Foo.vue").as_deref(),
            Some("/ws/src/Foo.vue.d.ts")
        );
        // The published name is not the bare-probed declaration carrier.
        assert_ne!(
            published_declaration_identity_for_source("/ws/src/Foo.vue"),
            VUE_VIRTUAL_FILE_NAMING.declaration_carrier_identity("/ws/src/Foo.vue", Some(".vue"))
        );
        // No TypeScript checking module, no published declaration.
        for source in ["/ws/src/Foo.svelte", "/ws/src/Foo.ts", "/ws/src/"] {
            assert_eq!(published_declaration_identity_for_source(source), None);
        }
    }

    #[test]
    fn private_module_references_resolve_through_the_physical_naming_owner() {
        for (naming, extension) in built_in_namings_cached() {
            let source = format!("./App{extension}");
            for suffix in naming
                .api_surface_suffix()
                .into_iter()
                .chain(naming.testing_api_suffix)
            {
                let logical = suffix.rsplit_once('.').unwrap().0;
                assert_eq!(
                    classify_carrier_companion_reference(&format!("{source}{logical}")),
                    classify_carrier_companion(&format!("{source}{suffix}"))
                );
            }
            for ide in naming.ide_carrier_identities(&source) {
                let reference = verter_types_import_specifier(&ide).unwrap();
                let physical = verter_types_sidecar_path(&ide).unwrap();
                assert_eq!(
                    classify_carrier_companion_reference(&reference),
                    classify_carrier_companion(&physical)
                );
            }
            assert!(classify_carrier_companion_reference(&source).is_none());
        }
        for source in ["./module.svelte.ts", "./module.svelte.js", "./.vue.verter"] {
            assert!(classify_carrier_companion_reference(source).is_none());
        }
    }
}
