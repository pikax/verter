//! Guard: `carrier_never_shadows_real_user_file` (ownership / `Ambiguous` half).
//!
//! The carrier-companion path (`{name}.vue.tsx` / `{name}.vue.jsx` /
//! `{name}.svelte.tsx` / `{name}.svelte.jsx`) is in the USER namespace, not a
//! Verter-reserved one.
//! When a real user file already occupies that EXACT path, Verter must NEVER
//! overlay-shadow it: the source is downgraded to `Ambiguous` (no external-TS
//! binding) and Verter places no overlay. Asserted for `.vue` AND `.svelte`,
//! across EVERY companion family the descriptor authority enumerates — the IDE
//! carrier suffixes (`.tsx` and `.jsx` for each `JsxConditional` adapter),
//! the extension-middle DECLARATION carrier (`Foo.d.vue.ts`), the `.verter.ts`
//! import-surface API, the testing-API, and any sidecar — never only the IDE
//! companion.
//!
//! DISCRIMINATING: without the carrier-path conflict pass a clean owner would
//! resolve to a `ProjectBinding` even when a real file sits at the carrier path;
//! this guard asserts the `Ambiguous` downgrade instead. Against a hardcoded
//! `.tsx`-only conflict probe, the `.jsx` companion case below stays a
//! `ProjectBinding` (the bug) — the descriptor-derived probe is what makes it
//! `Ambiguous`. (The per-engine overlay-placement half of this guard lands with
//! the backends in a later block.)

use verter_session::external_ts::{AmbiguityCause, CarrierOwnershipResolution};
use verter_session::framework::descriptor::built_in_descriptors;
use verter_session::framework::VirtualFileNaming;

use super::shared::{carrier_exts, resolve_with};

const TS: &str = "d:/ws/tsconfig.json";

/// The descriptor-owned IDE carrier suffixes for a carrier extension (e.g.
/// `"vue"`), via the `VirtualFileNaming` authority. Vue and Svelte both yield
/// `[".tsx", ".jsx"]`. Never a hardcoded suffix list.
fn ide_suffixes_for(carrier_ext: &str) -> Vec<&'static str> {
    // The descriptor's carrier_language id is the bare carrier ext
    // (`"vue"`/`"svelte"`).
    built_in_descriptors()
        .iter()
        .filter(|d| {
            d.carrier_language
                .as_ref()
                .is_some_and(|id| id.as_str() == carrier_ext)
        })
        .filter_map(|d| d.virtual_file_naming.as_ref())
        .flat_map(VirtualFileNaming::ide_carrier_suffixes)
        .collect()
}

#[test]
fn carrier_never_shadows_real_user_file() {
    let exts = carrier_exts();
    assert!(
        exts.iter().any(|e| e == "vue") && exts.iter().any(|e| e == "svelte"),
        "this guard requires the built-in `.vue` AND `.svelte` carrier adapters; got {exts:?}"
    );

    for ext in &exts {
        let source = format!("d:/ws/src/Foo.{ext}");

        // Control: with NO real file at any carrier path, the source binds cleanly.
        let clean = resolve_with(
            &[
                (TS, r#"{ "include": ["src/**/*"] }"#),
                (source.as_str(), "// carrier"),
            ],
            &[TS],
            &source,
        );
        assert!(
            matches!(clean, CarrierOwnershipResolution::Bound(_)),
            "control: `Foo.{ext}` with no occupying file must bind cleanly (got {clean:?})"
        );

        // For EVERY descriptor-valid IDE carrier suffix (`.tsx`, and `.jsx` for a
        // `JsxConditional` adapter): a real user file at that exact
        // carrier path ⇒ Ambiguous, no shadowing.
        let suffixes = ide_suffixes_for(ext);
        assert!(
            suffixes.contains(&".tsx"),
            "every built-in carrier's IDE policy must yield the `.tsx` companion; \
             `.{ext}` yielded {suffixes:?}"
        );
        for suffix in &suffixes {
            let carrier_path = format!("d:/ws/src/Foo.{ext}{suffix}");
            let conflicted = resolve_with(
                &[
                    (TS, r#"{ "include": ["src/**/*"] }"#),
                    (source.as_str(), "// carrier"),
                    (carrier_path.as_str(), "export const realUserFile = 1;"),
                ],
                &[TS],
                &source,
            );
            assert_eq!(
                conflicted,
                CarrierOwnershipResolution::Ambiguous {
                    candidates: Vec::new(),
                    cause: AmbiguityCause::CarrierPathOccupiedByRealFile,
                },
                "a real file at the descriptor-valid carrier path `Foo.{ext}{suffix}` must \
                 downgrade `Foo.{ext}` to Ambiguous (Verter never overlay-shadows a real \
                 user file)"
            );
        }
    }

    // Each component adapter's `JsxConditional` policy MUST contribute a `.jsx`
    // companion — the
    // exact case the hardcoded-`.tsx` probe missed. Assert it explicitly so the
    // JSX coverage cannot silently regress to `.tsx`-only.
    assert!(
        ide_suffixes_for("vue").contains(&".jsx"),
        "Vue's JsxConditional IDE policy must yield a `.jsx` companion path"
    );
    assert!(
        ide_suffixes_for("svelte").contains(&".jsx"),
        "Svelte's JsxConditional IDE policy must yield a `.jsx` companion path"
    );
}

/// A real user file at ANY descriptor-owned companion path — not only the IDE
/// `.tsx`/`.jsx`, but the extension-middle DECLARATION carrier (`Foo.d.vue.ts` /
/// `Foo.d.svelte.ts`), the `.verter.ts` import-surface API, the testing-API, and any
/// sidecar — must downgrade the carrier source to
/// `Ambiguous(CarrierPathOccupiedByRealFile)`. The shared resolver owner enumerates
/// EVERY companion family through the descriptor authority
/// (`carrier_companion_identities_for_source`), so Verter never overlay-shadows a real
/// user file at any occupiable companion path.
///
/// DISCRIMINATING: an IDE-companion-only conflict probe never sees the declaration /
/// API / testing companions — a source binds cleanly (`ProjectBinding`) with a real
/// file at (e.g.) `Foo.d.vue.ts`. This guard asserts the `Ambiguous` downgrade the
/// full-family enumeration produces for EVERY companion the descriptor emits, and pins
/// the declaration family (the specific gap it closes) as covered.
#[test]
fn real_file_at_any_companion_path_downgrades_source_to_ambiguous() {
    use verter_session::framework::descriptor::{
        carrier_companion_identities_for_source, CarrierCompanionKind,
    };

    let exts = carrier_exts();
    assert!(
        exts.iter().any(|e| e == "vue") && exts.iter().any(|e| e == "svelte"),
        "this guard requires the built-in `.vue` AND `.svelte` carrier adapters; got {exts:?}"
    );

    let mut saw_declaration = false;
    let mut checked = 0usize;
    for ext in &exts {
        let source = format!("d:/ws/src/Foo.{ext}");
        let companions = carrier_companion_identities_for_source(&source);
        assert!(
            !companions.is_empty(),
            "carrier `Foo.{ext}` must project at least one descriptor-owned companion"
        );
        for companion in &companions {
            if companion.kind == CarrierCompanionKind::Declaration {
                saw_declaration = true;
            }
            let conflicted = resolve_with(
                &[
                    (TS, r#"{ "include": ["src/**/*"] }"#),
                    (source.as_str(), "// carrier"),
                    (companion.path.as_str(), "export const realUserFile = 1;"),
                ],
                &[TS],
                &source,
            );
            assert_eq!(
                conflicted,
                CarrierOwnershipResolution::Ambiguous {
                    candidates: Vec::new(),
                    cause: AmbiguityCause::CarrierPathOccupiedByRealFile,
                },
                "a real file at the descriptor-owned {:?} companion `{}` must downgrade \
                 `{source}` to Ambiguous (Verter never overlay-shadows a real user file at any \
                 occupiable companion path)",
                companion.kind,
                companion.path
            );
            checked += 1;
        }
    }
    assert!(
        saw_declaration,
        "the declaration companion family (the gap this closes) must be exercised"
    );
    assert!(
        checked >= 8,
        "expected the Vue (IDE ×2 + declaration + API + testing) and Svelte (IDE + \
         declaration + API) companion families covered; checked {checked}"
    );
}
