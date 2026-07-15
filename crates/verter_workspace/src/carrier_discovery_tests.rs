//! Tests for the carrier-discovery decision.
//!
//! Adapter-parameterized over every registered carrier extension (`.vue`,
//! `.svelte`, …). Built on a `MemoryWorkspace` driven through the PRODUCTION
//! `load_project_membership` / `load_compiler_options` parse chain, so the
//! decision is exercised against the real membership-expansion semantics, not a
//! reimplementation.
//!
//! DISCRIMINATING: the decision is made against the COMPANION surface
//! (`Foo.vue.tsx`), not the carrier (`Foo.vue`). A model that keyed on
//! carrier-ownership would wrongly classify `src/**/*.vue` as `Enumerated`
//! (it OWNS the `.vue` carrier) — but the `.vue.tsx` companion is NOT a member,
//! so the correct answer is `Virtualize`. The `vue_specific_include_*` case
//! below fails RED against any carrier-keyed model.

use std::sync::Arc;

use crate::canonical_path::CanonicalPath;
use crate::config::{load_compiler_options, load_project_membership};
use crate::memory::{MemoryOptions, MemoryWorkspace};
use crate::resolver::carrier_ide_provider_path;

use super::{decide_carrier_discovery, CarrierDiscoveryMode};

const WORKSPACE_ROOT: &str = "d:/ws";
const TSCONFIG: &str = "d:/ws/tsconfig.json";

/// The carrier extensions the live registry registers (`vue`, `svelte`, …),
/// WITHOUT a leading dot. Tests are adapter-parameterized over these.
fn carrier_exts() -> Vec<String> {
    verter_language::LanguageRegistry::global()
        .carrier_extensions()
        .iter()
        .map(|e| (*e).to_string())
        .collect()
}

fn workspace_with(files: &[(&str, &str)]) -> MemoryWorkspace {
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![WORKSPACE_ROOT.to_string()],
        default_resolve_extensions: None,
    });
    for (path, content) in files {
        ws.inject_file((*path).to_string(), Arc::<str>::from(*content));
    }
    ws
}

/// Decide discovery for a carrier `src/Foo.<ext>` under the given tsconfig body.
fn decide(tsconfig_body: &str, ext: &str) -> CarrierDiscoveryMode {
    let carrier = format!("d:/ws/src/Foo.{ext}");
    let ws = workspace_with(&[(TSCONFIG, tsconfig_body), (carrier.as_str(), "// carrier")]);
    let membership = load_project_membership(&ws, TSCONFIG);
    let compiler_options = load_compiler_options(&ws, TSCONFIG);
    let root = CanonicalPath::new("d:/ws");
    decide_carrier_discovery(&root, &membership, &compiler_options, &carrier, false)
}

#[test]
fn three_discovery_cases_per_carrier_adapter() {
    let exts = carrier_exts();
    assert!(
        exts.iter().any(|e| e == "vue") && exts.iter().any(|e| e == "svelte"),
        "this test requires the built-in `.vue` AND `.svelte` carrier adapters; got {exts:?}"
    );

    for ext in &exts {
        // ── ENUMERATED (no virtualization): the companion `.tsx` is already a
        //    member through the user's include. ──────────────────────────────

        // Directory glob `["src"]` expands to per-extension globs including
        // `src/**/*.tsx`, which matches the `.vue.tsx` companion.
        assert_eq!(
            decide(r#"{ "include": ["src"] }"#, ext),
            CarrierDiscoveryMode::Enumerated,
            "directory glob [\"src\"] enumerates the companion ⇒ no virtual config (ext={ext})"
        );

        // Bare-star glob `["src/**/*"]` expands the same way.
        assert_eq!(
            decide(r#"{ "include": ["src/**/*"] }"#, ext),
            CarrierDiscoveryMode::Enumerated,
            "bare-star [\"src/**/*\"] enumerates the companion ⇒ no virtual config (ext={ext})"
        );

        // An extension-specific glob that DOES match the companion's `.tsx`.
        assert_eq!(
            decide(r#"{ "include": ["src/**/*.tsx"] }"#, ext),
            CarrierDiscoveryMode::Enumerated,
            "[\"src/**/*.tsx\"] matches the `.tsx` companion ⇒ no virtual config (ext={ext})"
        );

        // Default include (no `files`/`include`) covers the companion.
        assert_eq!(
            decide(r#"{ "compilerOptions": {} }"#, ext),
            CarrierDiscoveryMode::Enumerated,
            "default include enumerates the companion ⇒ no virtual config (ext={ext})"
        );

        // ── VIRTUALIZE (needs injection): the companion `.tsx` is NOT reachable
        //    from the user's include/files. ───────────────────────────────────

        // GOLD discrimination: a carrier-specific include OWNS the `.vue`
        // carrier but the `.vue.tsx` companion is NOT a member. A carrier-keyed
        // model wrongly returns Enumerated here.
        let vue_specific = format!(r#"{{ "include": ["src/**/*.{ext}"] }}"#);
        assert_eq!(
            decide(&vue_specific, ext),
            CarrierDiscoveryMode::Virtualize,
            "carrier-specific include [\"src/**/*.{ext}\"] does NOT enumerate the `.{ext}.tsx` \
             companion ⇒ virtualize (this fails RED against any carrier-ownership-keyed model)"
        );

        // An extension-specific `.ts` glob (`.ts` ≠ the companion's `.tsx`).
        assert_eq!(
            decide(r#"{ "include": ["src/**/*.ts"] }"#, ext),
            CarrierDiscoveryMode::Virtualize,
            "[\"src/**/*.ts\"] does NOT match the `.tsx` companion ⇒ virtualize (ext={ext})"
        );

        // A `files` list (exact paths) that lists the carrier but NOT the
        // companion — the companion can never be enumerated from a `files` list.
        let files_only = format!(r#"{{ "files": ["src/Foo.{ext}"] }}"#);
        assert_eq!(
            decide(&files_only, ext),
            CarrierDiscoveryMode::Virtualize,
            "a `files` list naming only the carrier does NOT enumerate the companion ⇒ virtualize \
             (ext={ext})"
        );
    }
}

/// NEGATIVE/structural: the decision keys on the companion suffix the registry
/// transform produces, not a hardcoded `.tsx`. Confirm the companion path the
/// decision reasons about is exactly the registry-derived one.
#[test]
fn companion_path_is_registry_derived_not_hardcoded() {
    for ext in carrier_exts() {
        let carrier = format!("d:/ws/src/Foo.{ext}");
        let companion = carrier_ide_provider_path(&carrier, false);
        assert_eq!(
            companion,
            format!("d:/ws/src/Foo.{ext}.tsx"),
            "companion for a non-jsx carrier is `{{source}}.tsx` (ext={ext})"
        );
        // The jsx variant differs — the decision must reason about whichever
        // `is_jsx` the caller passes (a Vue `<script lang=\"jsx\">` carrier).
        assert_eq!(
            carrier_ide_provider_path(&carrier, true),
            format!("d:/ws/src/Foo.{ext}.jsx"),
            "jsx companion is `{{source}}.jsx` (ext={ext})"
        );
    }
}

/// DISCRIMINATING for the jsx axis: a `*.jsx`-only include enumerates the jsx
/// companion but NOT the default `.tsx` companion (and vice-versa). Proves the
/// decision honors the `is_jsx` selector rather than always probing `.tsx`.
#[test]
fn jsx_selector_changes_the_probed_companion() {
    let ext = "vue";
    let carrier = format!("d:/ws/src/Foo.{ext}");
    let body = r#"{ "include": ["src/**/*.jsx"] }"#;
    let ws = workspace_with(&[(TSCONFIG, body), (carrier.as_str(), "// carrier")]);
    let membership = load_project_membership(&ws, TSCONFIG);
    let compiler_options = load_compiler_options(&ws, TSCONFIG);
    let root = CanonicalPath::new("d:/ws");

    // A `*.jsx` include enumerates the jsx companion …
    assert_eq!(
        decide_carrier_discovery(&root, &membership, &compiler_options, &carrier, true),
        CarrierDiscoveryMode::Enumerated,
        "[\"src/**/*.jsx\"] enumerates the `.vue.jsx` companion when is_jsx=true"
    );
    // … but NOT the default `.tsx` companion.
    assert_eq!(
        decide_carrier_discovery(&root, &membership, &compiler_options, &carrier, false),
        CarrierDiscoveryMode::Virtualize,
        "[\"src/**/*.jsx\"] does NOT enumerate the `.vue.tsx` companion when is_jsx=false"
    );
}
