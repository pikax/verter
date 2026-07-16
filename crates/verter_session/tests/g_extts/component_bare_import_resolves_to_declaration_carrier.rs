//! Guard: `component_bare_import_resolves_to_declaration_carrier`.
//!
//! GATE 5 (companion-identity P0): a BARE framework-carrier import
//! (`import X from "./Foo.vue"` / `"./Foo.svelte"`) RESOLVES to the
//! EXTENSION-MIDDLE DECLARATION carrier `Foo.d.vue.ts` / `Foo.d.svelte.ts` —
//! the path a tsgo basename-append probe reaches FIRST (probe order
//! `.d.<ext>.ts` -> `.<ext>.ts` -> `.<ext>.tsx`; the declaration WINS). The
//! reserved `.verter.` infix stays API-ONLY (never on a bare-probed carrier
//! identity). Adapter-parameterized over `.vue` AND `.svelte`. This locks the
//! resolution-target facts about the PRODUCTION descriptor naming authority —
//! `VirtualFileNaming::declaration_carrier_identity` /
//! `FrameworkAdapterDescriptor::declaration_carrier_identity`, the column the
//! resolver's companion-path gate derives the declaration target from:
//!
//! (a) the declaration carrier identity is the bare carrier source with `.d.`
//!     inserted between the stem and the carrier extension and `.ts` appended
//!     (`Foo.vue` -> `Foo.d.vue.ts`), PRESERVING the `.vue`/`.svelte` carrier
//!     extension in EXTENSION-MIDDLE form — so it is EXACTLY a basename-append
//!     probe target for the bare `./Foo.<ext>` import;
//! (b) the declaration target is NEVER the extension-LAST `Foo.vue.d.ts` (which
//!     tsgo would not bare-resolve to) and NEVER carries the reserved
//!     `.verter.` infix; that infix lives only on the redirect-reached API
//!     surface (`api_surface_suffix()`).
//!
//! The IDE carrier (`Foo.vue.tsx`/`.svelte.tsx`) is STILL composed (it is the
//! self-diagnostics surface verter_lsp owns), but it is NOT what the bare import
//! resolves to under the declaration-carrier scheme — so this guard pins the
//! DECLARATION target, and only separately asserts the IDE carrier is still
//! composed (without claiming it is the bare-import-probe target).
//!
//! The carrier extensions come from the language registry; the declaration /
//! IDE / API identities are DERIVED from the descriptor naming authority. Never
//! a hardcoded suffix.
//!
//! DISCRIMINATING: the asserts compare the PRODUCTION composer's output against
//! the independently-specified declaration-probe target. A composer that left
//! the bare-probe target as the IDE `.vue.tsx`, dropped the carrier extension,
//! emitted the extension-LAST `Foo.vue.d.ts`, or inserted a `.verter.` infix
//! turns this guard RED — it is not satisfiable by a self-constructed string.

use verter_session::framework::descriptor::built_in_descriptors;

use super::shared::carrier_exts;

/// Locate the production descriptor that owns the carrier extension `ext`.
fn descriptor_for_ext(
    ext: &str,
) -> verter_session::framework::descriptor::FrameworkAdapterDescriptor {
    built_in_descriptors()
        .into_iter()
        .find(|d| {
            d.carrier_language
                .as_ref()
                .is_some_and(|id| id.as_str() == ext)
        })
        .unwrap_or_else(|| panic!("carrier `.{ext}` must have a built-in adapter descriptor"))
}

#[test]
fn component_bare_import_resolves_to_declaration_carrier() {
    let exts = carrier_exts();
    assert!(
        exts.iter().any(|e| e == "vue") && exts.iter().any(|e| e == "svelte"),
        "this guard requires the built-in `.vue` AND `.svelte` carrier adapters; got {exts:?}"
    );

    let mut checks = 0usize;
    for ext in &exts {
        let carrier_source = format!("/ws/src/Foo.{ext}");
        let descriptor = descriptor_for_ext(ext);

        // ── The resolution target: the EXTENSION-MIDDLE `.d.<ext>.ts`
        //    declaration carrier the bare `./Foo.<ext>` import resolves to. ──
        // Exercise the PRODUCTION descriptor naming authority (which derives the
        // carrier extension from the adapter's `carrier_language`), NOT a
        // self-constructed string. The independently-specified probe target is
        // `Foo` + `.d.` + `.<ext>` + `.ts`.
        let decl_target = format!("/ws/src/Foo.d.{ext}.ts");
        let decl_identity = descriptor
            .declaration_carrier_identity(&carrier_source)
            .unwrap_or_else(|| {
                panic!(
                    "carrier `.{ext}` must project a declaration carrier identity for the bare \
                     import resolution target; got None"
                )
            });
        assert_eq!(
            decl_identity, decl_target,
            "(a) the production declaration carrier identity for `.{ext}` must be the bare \
             carrier source `{carrier_source}` with `.d.` inserted before the carrier extension \
             and `.ts` appended (the bare-import-probe target tsgo reaches FIRST). Got \
             `{decl_identity}`, expected `{decl_target}`"
        );

        // The carrier extension is PRESERVED in EXTENSION-MIDDLE form: the
        // identity ends with `.d.<ext>.ts` and begins with the full stem.
        assert!(
            decl_identity.ends_with(&format!(".d.{ext}.ts")),
            "the production declaration carrier identity `{decl_identity}` must be \
             extension-MIDDLE (`.d.{ext}.ts`)"
        );
        assert!(
            decl_identity.starts_with("/ws/src/Foo."),
            "the production declaration carrier identity `{decl_identity}` must preserve the \
             carrier basename stem `/ws/src/Foo`"
        );

        // (b) NEGATIVE: never the extension-LAST `Foo.<ext>.d.ts` (which tsgo
        //     would not bare-resolve the `./Foo.<ext>` import to).
        assert!(
            !decl_identity.contains(&format!(".{ext}.d.ts")),
            "the production declaration carrier identity `{decl_identity}` must NOT be the \
             extension-LAST `Foo.{ext}.d.ts` (tsgo's bare `./Foo.{ext}` probe would not reach it)"
        );
        // (b) NEGATIVE: never the reserved `.verter.` infix — that is API-only
        //     and would be unreachable by the bare basename-append probe.
        assert!(
            !decl_identity.contains(".verter."),
            "the production declaration carrier `{decl_identity}` must NOT carry the reserved \
             `.verter.` infix (it is API-only; a `.verter.`-infixed path is not \
             basename-append-probe-reachable)"
        );

        checks += 1;
    }

    assert_eq!(
        checks,
        exts.len(),
        "expected exactly one declaration-target check per carrier extension; got checks={checks}, \
         carriers={}",
        exts.len()
    );
}

/// Resolve the production `VirtualFileNaming` column for the carrier adapter that
/// owns extension `ext` (no leading dot — e.g. `"vue"` / `"svelte"`).
fn naming_for_ext(ext: &str) -> verter_session::framework::descriptor::VirtualFileNaming {
    descriptor_for_ext(ext)
        .virtual_file_naming
        .unwrap_or_else(|| panic!("the `.{ext}` adapter must carry a virtual-file naming column"))
}

/// The IDE carrier (`Foo.vue.tsx`/`Foo.vue.jsx` and
/// `Foo.svelte.tsx`/`Foo.svelte.jsx`) is STILL composed — it
/// is the self-diagnostics surface verter_lsp owns — but it is NOT what a bare
/// framework-carrier import resolves to (the declaration carrier wins the probe,
/// asserted above). This test pins that the IDE carrier composition survives,
/// stays DISTINCT from the declaration target, and stays `.verter.`-free, WITHOUT
/// claiming it is the bare-import-probe target — for BOTH Vue and Svelte.
///
/// Both IDE carriers are DUAL (`.tsx` AND `.jsx`) because each descriptor's
/// `ide` column is `VirtualPathPolicy::JsxConditional`. The IDE identities are
/// composed from the PRODUCTION descriptor naming authority
/// (`VirtualFileNaming::ide_carrier_identities`), NOT a self-constructed string.
///
/// DISCRIMINATING: a composer that stopped emitting `.vue.tsx`, dropped Vue's
/// `.vue.jsx` JSX-conditional identity, served the declaration `.d.vue.ts` as an
/// IDE identity, or stamped a `.verter.`-infixed *component* identity turns this
/// guard RED — none of the asserts is satisfiable by a self-constructed string.
#[test]
fn ide_carrier_still_composed_distinct_from_declaration_target_and_verter_infix_is_api_only() {
    // ── Vue: the `JsxConditional` IDE policy composes BOTH `.vue.tsx` AND
    //    `.vue.jsx`, both distinct from the `.d.vue.ts` declaration target. ──
    let vue_naming = naming_for_ext("vue");
    let vue_source = "/ws/src/Foo.vue";
    let vue_ide_identities = vue_naming.ide_carrier_identities(vue_source);

    // BOTH IDE identities of the Vue `JsxConditional` policy are STILL composed
    // (the self-diagnostics surface) — `.vue.tsx` AND the JSX-conditional
    // `.vue.jsx`.
    assert!(
        vue_ide_identities.contains(&"/ws/src/Foo.vue.tsx".to_string()),
        "the production Vue IDE carrier identity `/ws/src/Foo.vue.tsx` must STILL be composed \
         (the self-diagnostics surface); got {vue_ide_identities:?}"
    );
    assert!(
        vue_ide_identities.contains(&"/ws/src/Foo.vue.jsx".to_string()),
        "the production Vue IDE carrier identity `/ws/src/Foo.vue.jsx` (the `JsxConditional` \
         policy's JSX arm) must STILL be composed; got {vue_ide_identities:?}"
    );

    // The Vue DECLARATION carrier — the bare-import resolution target — is DISTINCT
    // from EVERY Vue IDE identity and carries the extension-MIDDLE `.d.` infix.
    let vue_decl_identity = vue_naming
        .declaration_carrier_identity(vue_source, Some(".vue"))
        .expect("the Vue component carrier projects a declaration carrier");
    assert_eq!(
        vue_decl_identity, "/ws/src/Foo.d.vue.ts",
        "the Vue declaration (bare-import resolution) target must be `/ws/src/Foo.d.vue.ts`"
    );
    assert!(
        !vue_ide_identities.contains(&vue_decl_identity),
        "the bare-import resolution target (the Vue declaration carrier `{vue_decl_identity}`) \
         must be DISTINCT from the Vue IDE carrier identities {vue_ide_identities:?} (the IDE \
         carrier is the self-diagnostics surface, NOT the bare-import-probe target)"
    );

    // No production Vue IDE carrier identity carries the reserved `.verter.` infix
    // (it is API-only) — asserted across BOTH the `.tsx` and `.jsx` arms.
    assert!(
        vue_ide_identities.iter().all(|id| !id.contains(".verter.")),
        "no production Vue IDE carrier identity may carry the reserved `.verter.` infix; got \
         {vue_ide_identities:?}"
    );

    // ── Svelte: the conditional IDE policy composes `.svelte.tsx` and
    //    `.svelte.jsx`, distinct from the declaration target. ──
    let naming = naming_for_ext("svelte");
    let carrier_source = "/ws/src/Foo.svelte";

    // The IDE carrier identity is STILL composed (the self-diagnostics surface).
    let ide_identities = naming.ide_carrier_identities(carrier_source);
    assert!(
        ide_identities.contains(&"/ws/src/Foo.svelte.tsx".to_string()),
        "the production Svelte IDE carrier identity `/ws/src/Foo.svelte.tsx` must STILL be \
         composed (the self-diagnostics surface); got {ide_identities:?}"
    );
    assert!(
        ide_identities.contains(&"/ws/src/Foo.svelte.jsx".to_string()),
        "the production Svelte JavaScript IDE carrier identity `/ws/src/Foo.svelte.jsx` must \
         STILL be composed; got {ide_identities:?}"
    );

    // The DECLARATION carrier — the bare-import resolution target — is DISTINCT
    // from the IDE carrier and carries the `.d.` infix.
    let decl_identity = naming
        .declaration_carrier_identity(carrier_source, Some(".svelte"))
        .expect("the Svelte component carrier projects a declaration carrier");
    assert_eq!(
        decl_identity, "/ws/src/Foo.d.svelte.ts",
        "the Svelte declaration (bare-import resolution) target must be `/ws/src/Foo.d.svelte.ts`"
    );
    assert!(
        !ide_identities.contains(&decl_identity),
        "the bare-import resolution target (the declaration carrier `{decl_identity}`) must be \
         DISTINCT from the IDE carrier identities {ide_identities:?} (the IDE carrier is the \
         self-diagnostics surface, NOT the bare-import-probe target)"
    );

    // No production IDE carrier identity carries the reserved `.verter.` infix.
    assert!(
        ide_identities.iter().all(|id| !id.contains(".verter.")),
        "no production IDE carrier identity may carry the reserved `.verter.` infix; got \
         {ide_identities:?}"
    );

    // The `.verter.` infix DOES live on the redirect-reached API (import-
    // resolution) surface — proving the split: the bare-probed carrier surfaces
    // (declaration + IDE) are `.verter.`-free; the API surface is redirect-only.
    // Asserted for BOTH adapters (Vue + Svelte) — neither leaks `.verter.` onto a
    // bare-probed surface, and both carry it on the redirect-only API surface.
    for (ext, naming) in [("vue", &vue_naming), ("svelte", &naming)] {
        let api_suffix = naming.api_surface_suffix().unwrap_or_else(|| {
            panic!("the `.{ext}` component carrier has a distinct-file API surface")
        });
        assert!(
            api_suffix.contains(".verter."),
            "the `.{ext}` API (import-resolution) surface suffix must carry the reserved \
             `.verter.` infix (the redirect-only surface), distinguishing it from the \
             bare-probe-reachable declaration/IDE carriers; got `{api_suffix}`"
        );
    }
}
