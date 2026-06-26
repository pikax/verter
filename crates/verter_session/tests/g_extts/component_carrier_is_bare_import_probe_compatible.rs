//! Guard: `component_carrier_is_bare_import_probe_compatible`.
//!
//! GATE 5 (companion-identity P0): the COMPONENT IDE carrier identity must be a
//! path a tsgo `.tsx` / `.ts` basename-append probe reaches for a BARE
//! `.vue` / `.svelte` import, and the reserved `.verter.` infix must be
//! API-ONLY (never on the component IDE carrier). Adapter-parameterized over
//! `.vue` AND `.svelte`. This locks two facts about the PRODUCTION IDE-carrier
//! composer — `VirtualFileNaming::ide_carrier_identities`, the descriptor naming
//! authority the resolver's companion-path gate derives from:
//!
//! (a) the composer appends the descriptor's IDE suffix to the FULL carrier
//!     source (`Foo.vue` + `.tsx` = `Foo.vue.tsx`), PRESERVING the
//!     `.vue`/`.svelte` carrier extension — so the IDE carrier identity is
//!     EXACTLY a basename-append probe target (bare-import-probe-reachable);
//! (b) the composer NEVER produces a `.verter.`-infixed COMPONENT identity
//!     (`Foo.vue.verter.tsx`); the `.verter.` infix lives only on the
//!     redirect-reached API surface (`api_surface_suffix()`) — a
//!     `.verter.`-infixed component path does NOT satisfy the bare `.vue` import
//!     (GATE 5(a): TS2307).
//!
//! The IDE suffix is DERIVED from the descriptor (`ide_carrier_suffixes()`); the
//! carrier extensions from the language registry. Never a hardcoded suffix.
//!
//! DISCRIMINATING: the asserts compare the PRODUCTION composer's output against
//! the independently-specified bare-probe target. Stripping the carrier
//! extension (so the composer emits `Foo.tsx`) or inserting a `.verter.` infix
//! (so it emits `Foo.vue.verter.tsx`) turns this guard RED — it is not
//! satisfiable by a self-constructed string.

use verter_session::framework::descriptor::built_in_descriptors;

use crate::shared::carrier_exts;

#[test]
fn component_carrier_is_bare_import_probe_compatible() {
    let exts = carrier_exts();
    assert!(
        exts.iter().any(|e| e == "vue") && exts.iter().any(|e| e == "svelte"),
        "this guard requires the built-in `.vue` AND `.svelte` carrier adapters; got {exts:?}"
    );

    let mut checks = 0usize;
    for ext in &exts {
        let carrier_source = format!("/ws/src/Foo.{ext}");

        let naming = built_in_descriptors()
            .into_iter()
            .find(|d| {
                d.carrier_language
                    .as_ref()
                    .is_some_and(|id| id.as_str() == ext)
            })
            .and_then(|d| d.virtual_file_naming)
            .unwrap_or_else(|| panic!("carrier `.{ext}` must carry a virtual-file naming column"));

        let ide_suffixes = naming.ide_carrier_suffixes();
        assert!(
            ide_suffixes.contains(&".tsx"),
            "every component carrier's IDE policy must yield a `.tsx` companion (the \
             bare-import-probe target); `.{ext}` yielded {ide_suffixes:?}"
        );

        // Exercise the PRODUCTION IDE-carrier composer (the descriptor naming
        // authority the resolver's companion-path gate derives from), NOT a
        // self-constructed string. The asserts below compare ITS output against
        // the independently-specified probe target.
        let ide_identities = naming.ide_carrier_identities(&carrier_source);
        assert_eq!(
            ide_identities.len(),
            ide_suffixes.len(),
            "the production composer must yield exactly one IDE carrier identity per IDE \
             suffix for `.{ext}`; suffixes={ide_suffixes:?}, identities={ide_identities:?}"
        );

        for (ide_carrier, suffix) in ide_identities.iter().zip(ide_suffixes.iter()) {
            // (a) The production IDE carrier identity is EXACTLY the bare carrier
            //     source with the descriptor's IDE suffix appended — the precise
            //     path TS's bare-import probe for `./Foo.{ext}` reaches
            //     (`./Foo.{ext}` + candidate extension). A composer that stripped
            //     the `.{ext}` (yielding `Foo{suffix}`) or inserted ANY infix
            //     between the source and the suffix would NOT be reachable by the
            //     bare probe — and this equality fails.
            let probe_target = format!("{carrier_source}{suffix}");
            assert_eq!(
                ide_carrier, &probe_target,
                "the production IDE carrier identity for `.{ext}` + `{suffix}` must be the \
                 bare carrier source `{carrier_source}` with the IDE suffix appended (the \
                 bare-import-probe target): the composer must NOT strip the `.{ext}` carrier \
                 extension nor insert any infix. Got `{ide_carrier}`, expected `{probe_target}`"
            );

            // The carrier extension is PRESERVED: the production identity still
            // begins with the FULL source `…/Foo.{ext}` (not a stripped `…/Foo`).
            assert!(
                ide_carrier.starts_with(&carrier_source),
                "the production IDE carrier identity `{ide_carrier}` must preserve the full \
                 carrier source `{carrier_source}` (the `.{ext}` extension must NOT be \
                 stripped to `Foo`)"
            );

            // (b) The production IDE carrier identity does NOT carry the reserved
            //     `.verter.` infix — that infix is API-only and would be
            //     unreachable by the bare probe (GATE 5(a)).
            assert!(
                !ide_carrier.contains(".verter."),
                "the production component IDE carrier `{ide_carrier}` must NOT carry the \
                 reserved `.verter.` infix (it is API-only; a `.verter.`-infixed component \
                 path is not bare-import-probe-reachable — GATE 5(a))"
            );
            checks += 1;
        }
    }

    // Non-vacuity: Vue contributes `.tsx` + `.jsx`, Svelte contributes `.tsx`,
    // so at minimum one check per carrier plus Vue's extra `.jsx`.
    assert!(
        checks > exts.len(),
        "expected more IDE-carrier checks than carriers (Vue's JsxConditional adds `.jsx`); \
         got checks={checks}, carriers={}",
        exts.len()
    );
}

/// Discriminating: the PRODUCTION IDE-carrier composer yields the bare-probe
/// `.tsx` target and never a `.verter.`-infixed component identity, while the
/// reserved `.verter.` infix DOES live on the redirect-reached API surface
/// (`api_surface_suffix()`). This proves the IDE-vs-API split: the IDE carrier
/// is bare-import-probe-reachable; the `.verter.`-infixed surface is
/// redirect-only (the GATE-5(a) rejection).
#[test]
fn ide_carrier_is_bare_probe_target_not_verter_infixed_api_surface() {
    let naming = built_in_descriptors()
        .into_iter()
        .find(|d| {
            d.carrier_language
                .as_ref()
                .is_some_and(|id| id.as_str() == "svelte")
        })
        .and_then(|d| d.virtual_file_naming)
        .expect("the `.svelte` adapter must carry a virtual-file naming column");

    let carrier_source = "/ws/src/Foo.svelte";

    // The production IDE carrier identity IS the basename-append probe target.
    let ide_identities = naming.ide_carrier_identities(carrier_source);
    assert!(
        ide_identities.contains(&"/ws/src/Foo.svelte.tsx".to_string()),
        "the production Svelte IDE carrier identity must be the bare-import-probe target \
         `/ws/src/Foo.svelte.tsx`; got {ide_identities:?}"
    );

    // No production IDE carrier identity carries the reserved `.verter.` infix —
    // a `.verter.`-infixed component identity is NOT probe-reachable (GATE 5(a)).
    assert!(
        ide_identities.iter().all(|id| !id.contains(".verter.")),
        "no production IDE carrier identity may carry the reserved `.verter.` infix (it is \
         redirect-only, not bare-import-probe-reachable); got {ide_identities:?}"
    );

    // The `.verter.` infix DOES live on the redirect-reached API (import-
    // resolution) surface — proving the split: the IDE carrier is
    // bare-probe-reachable, the API surface is redirect-only.
    let api_suffix = naming
        .api_surface_suffix()
        .expect("the Svelte component carrier has a distinct-file API surface");
    assert!(
        api_suffix.contains(".verter."),
        "the API (import-resolution) surface suffix must carry the reserved `.verter.` infix \
         (the redirect-only surface), distinguishing it from the bare-probe-reachable IDE \
         carrier; got `{api_suffix}`"
    );
}
