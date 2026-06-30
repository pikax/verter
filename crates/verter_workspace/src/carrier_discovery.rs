//! Carrier discovery + tsconfig-virtualization planning.
//!
//! A framework CARRIER file (`.vue`, `.svelte`, …) is type-checked through its
//! generated companion TypeScript surface (`Foo.vue.tsx`, `Bar.svelte.tsx`).
//! For an external TypeScript engine to type-check that companion, the
//! companion must become a member (a root file) of the configured Program.
//!
//! Whether that happens for free depends ENTIRELY on the shape of the user's
//! `tsconfig.json` `include`/`files`:
//!
//! - When the user's membership ALREADY enumerates the companion's extension
//!   (the default include, a directory / bare-star glob like `src` or
//!   `src/**/*`, or an extension-specific glob that matches `.tsx`/`.ts`), the
//!   engine discovers the companion directly through the overlay's directory
//!   enumeration — NO virtual config is needed.
//! - When the user's membership does NOT enumerate the companion's extension (a
//!   carrier-specific include such as `src/**/*.vue`, a `files` list, or any
//!   glob whose extensions exclude the companion's `.tsx`/`.ts`), the companion
//!   can never be enumerated from the real config — the configured tsconfig must
//!   be VIRTUALIZED: served to the engine with the companion paths injected into
//!   `include`/`files`, Verter-computed and never written to user disk.
//!
//! This module owns the DECISION (which case applies) and the virtual-config
//! IDENTITY model (a distinct project identity per virtualized config, so a
//! virtualized config never aliases the non-virtualized config in any cache
//! slot). It is policy in `verter_workspace`. The overlay MATERIALIZATION
//! (computing and serving the augmented tsconfig bytes) lives in the sibling
//! `tsgo_virtual_config` module; the `verter_tsgo_api` overlay seam stays
//! policy-free.
//!
//! The decision REUSES the exact production membership-expansion primitives
//! (`membership_to_spec` → `expand_include_glob` → `StaticMembershipSpec`) and
//! the registry-backed companion-naming transform
//! (`carrier_ide_provider_path`). It never reimplements glob matching or the
//! TS extension rules, and it is framework-agnostic (the carrier extension and
//! its companion suffix come from the language registry, never hardcoded).

use crate::canonical_path::CanonicalPath;
use crate::resolver::{carrier_ide_provider_path, IdeProjectCompilerOptions, ProjectMembership};
use crate::snapshot_builder::{membership_to_spec, supported_extensions_for};

/// How a carrier's companion TypeScript surface becomes a member of a
/// configured Program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarrierDiscoveryMode {
    /// The user's `include`/`files` ALREADY enumerate the companion's
    /// extension; the engine discovers the companion directly through the
    /// overlay directory enumeration. No virtual config is needed.
    Enumerated,
    /// The companion is not reachable from the user's `include`/`files`; the
    /// configured tsconfig must be virtualized (companion paths injected).
    Virtualize,
}

/// Decide, for one carrier source under one configured project, whether the
/// carrier's companion surface is already an enumerable Program member or the
/// tsconfig must be virtualized to make it one.
///
/// `carrier_source` is the canonical carrier path (`d:/ws/src/Foo.vue`).
/// `is_jsx` selects the companion suffix (`.jsx` for a JS-flavoured carrier
/// surface, `.tsx` otherwise) — the same `is_jsx` the IDE provider-path
/// transform takes.
///
/// The decision is made against the COMPANION path, never the carrier path: a
/// `src/**/*.vue` include OWNS the `.vue` carrier yet does NOT enumerate the
/// `.vue.tsx` companion, so it requires virtualization.
pub fn decide_carrier_discovery(
    project_root: &CanonicalPath,
    membership: &ProjectMembership,
    compiler_options: &IdeProjectCompilerOptions,
    carrier_source: &str,
    is_jsx: bool,
) -> CarrierDiscoveryMode {
    // The companion surface the engine must type-check. Registry-derived, never
    // hardcoded — `.vue` → `Foo.vue.tsx`, `.svelte` → `Foo.svelte.tsx`, with the
    // `.jsx` variant when the carrier surface is JS-flavoured.
    let companion = CanonicalPath::new(&carrier_ide_provider_path(carrier_source, is_jsx));

    // Expand the user's membership through the EXACT production extension rule:
    // a directory / bare-star glob expands into one glob per supported extension
    // (so `.tsx`/`.jsx` are covered), an extension-specific glob is kept verbatim,
    // `files` entries are exact, `exclude` filters `include`.
    let supported = supported_extensions_for(compiler_options);
    let spec = membership_to_spec(project_root, membership, &supported);

    // The decision is made against the COMPANION, not the carrier: if the
    // expanded membership matches the companion path the engine discovers it
    // directly through the overlay enumeration; otherwise the config must be
    // virtualized to inject the companion.
    if spec.matches(&companion) {
        CarrierDiscoveryMode::Enumerated
    } else {
        CarrierDiscoveryMode::Virtualize
    }
}

#[cfg(test)]
#[path = "carrier_discovery_tests.rs"]
mod tests;
