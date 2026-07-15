//! Guard: `carrier_companion_suffix_collision_free`.
//!
//! The two carrier-companion identities are PATH-collision-free against any
//! REAL adapter source / rune-module extension (§2.2 collision invariant,
//! GATE 5). Adapter-parameterized over `.vue` AND `.svelte` (incl. the
//! `.svelte.ts` / `.svelte.js` rune-module form):
//!
//! (a) the COMPONENT IDE carrier suffix(es) end in `.tsx` / `.jsx` and the
//!     full companion extension (`{carrier_ext}{ide_suffix}`, e.g.
//!     `.svelte.tsx`) equals NO real adapter source or rune-module extension
//!     (NOT `.svelte`, NOT `.svelte.ts`, NOT `.svelte.js`);
//! (b) the `.ts` API carrier carries the reserved `.verter.` infix
//!     (`.verter.ts`), and the full companion extension
//!     (`{carrier_ext}{api_suffix}`, e.g. `.svelte.verter.ts`) equals NO real
//!     adapter source or rune-module extension — precisely because a bare
//!     `.svelte.ts` API carrier WOULD collide (GATE 5: tsgo probes `.svelte.ts`
//!     before `.svelte.tsx`).
//!
//! The suffixes are DERIVED from the descriptor (`ide_carrier_suffixes()` /
//! `api_surface_suffix()`); the real source / rune-module extensions are
//! DERIVED from the language registry (`carrier_extensions()` /
//! `all_adapter_module_extensions()`). Never a hardcoded suffix list.
//!
//! DISCRIMINATING (self-test): a hypothetical `import_surface: Suffix(".ts")`
//! for Svelte (the PRE-change descriptor shape) WOULD collide — its full
//! companion extension `.svelte.ts` IS a real rune-module extension — so the
//! collision predicate FIRES on that bad input. This proves the guard is not
//! vacuous and would have failed the old descriptor.

use verter_language::LanguageRegistry;
use verter_session::framework::descriptor::{built_in_descriptors, VirtualFileNaming};

use super::shared::carrier_exts;

/// Every REAL adapter source / rune-module extension, WITH a leading dot
/// (`.vue`, `.svelte`, `.svelte.ts`, `.svelte.js`). A generated companion's
/// full trailing extension must equal none of these.
fn real_adapter_extensions() -> Vec<String> {
    let registry = LanguageRegistry::global();
    let mut exts: Vec<String> = registry
        .carrier_extensions()
        .iter()
        .map(|e| format!(".{e}"))
        .collect();
    for m in registry.all_adapter_module_extensions() {
        exts.push(format!(".{m}"));
    }
    exts
}

/// Whether the companion extension formed by appending `companion_suffix` to a
/// full carrier canonical of extension `carrier_ext` (`{carrier_ext}{suffix}`,
/// e.g. `.svelte` + `.ts` = `.svelte.ts`) collides with a REAL adapter source /
/// rune-module extension. This is the §2.2 path-collision predicate.
fn companion_collides(carrier_ext: &str, companion_suffix: &str, real: &[String]) -> bool {
    let full = format!(".{carrier_ext}{companion_suffix}");
    real.contains(&full)
}

#[test]
fn carrier_companion_suffix_collision_free() {
    let exts = carrier_exts();
    assert!(
        exts.iter().any(|e| e == "vue") && exts.iter().any(|e| e == "svelte"),
        "this guard requires the built-in `.vue` AND `.svelte` carrier adapters; got {exts:?}"
    );
    let real = real_adapter_extensions();
    // The registry must surface the Svelte rune-module extension — otherwise
    // the collision check below is vacuous (nothing for `.svelte.ts` to clash
    // with). This is the load-bearing input.
    assert!(
        real.iter().any(|r| r == ".svelte.ts") && real.iter().any(|r| r == ".svelte.js"),
        "the rune-module extensions must be in the real-extension set, got {real:?}"
    );

    let mut ide_checks = 0usize;
    let mut api_checks = 0usize;

    for ext in &exts {
        // The descriptor row whose carrier language id is this bare carrier ext.
        let naming: VirtualFileNaming = built_in_descriptors()
            .into_iter()
            .find(|d| {
                d.carrier_language
                    .as_ref()
                    .is_some_and(|id| id.as_str() == ext)
            })
            .and_then(|d| d.virtual_file_naming)
            .unwrap_or_else(|| panic!("carrier `.{ext}` must carry a virtual-file naming column"));

        // (a) Every IDE carrier suffix ends in `.tsx`/`.jsx` and its full
        //     companion extension collides with no real adapter extension.
        let ide_suffixes = naming.ide_carrier_suffixes();
        assert!(
            !ide_suffixes.is_empty(),
            "carrier `.{ext}` must project at least one IDE companion suffix"
        );
        for suffix in &ide_suffixes {
            assert!(
                *suffix == ".tsx" || *suffix == ".jsx",
                "the IDE companion suffix for `.{ext}` must be `.tsx`/`.jsx`, got `{suffix}`"
            );
            assert!(
                !companion_collides(ext, suffix, &real),
                "the IDE companion `.{ext}{suffix}` collides with a real adapter \
                 source/rune-module extension — the component carrier must be \
                 path-collision-free"
            );
            ide_checks += 1;
        }

        // (b) The `.ts` API carrier carries the reserved `.verter.` infix and
        //     its full companion extension collides with no real adapter
        //     extension.
        let api_suffix = naming
            .api_surface_suffix()
            .unwrap_or_else(|| panic!("carrier `.{ext}` must project a distinct API surface"));
        assert!(
            api_suffix.contains(".verter."),
            "the API carrier suffix for `.{ext}` must carry the reserved `.verter.` infix, \
             got `{api_suffix}`"
        );
        assert!(
            !companion_collides(ext, api_suffix, &real),
            "the API companion `.{ext}{api_suffix}` collides with a real adapter \
             source/rune-module extension — the reserved `.verter.` infix must keep it \
             collision-free"
        );
        api_checks += 1;
    }

    // Non-vacuity: we checked at least one IDE suffix per carrier (Vue
    // contributes two — `.tsx` + `.jsx`) and one API suffix per carrier.
    assert!(
        ide_checks >= exts.len() && api_checks == exts.len(),
        "every registered carrier must contribute IDE + API collision checks \
         (ide_checks={ide_checks}, api_checks={api_checks}, carriers={})",
        exts.len()
    );
}

/// Discriminating self-test: the PRE-change descriptor shape — a bare
/// `import_surface: Suffix(".ts")` for Svelte — WOULD collide. The collision
/// predicate MUST fire on that bad input (its full companion extension
/// `.svelte.ts` IS a real rune-module extension), and MUST NOT fire on the
/// shipped `.verter.ts` infix. This proves the guard would have failed the old
/// descriptor and is not vacuous.
#[test]
fn collision_predicate_fires_on_bare_ts_svelte_api_carrier() {
    let real = real_adapter_extensions();

    // The bad (pre-change) Svelte API carrier: bare `.ts` → `.svelte.ts` which
    // IS a real rune-module extension ⇒ collision DETECTED.
    assert!(
        companion_collides("svelte", ".ts", &real),
        "a bare `.ts` Svelte API carrier (`.svelte.ts`) MUST be detected as a collision \
         (it is a real rune-module extension) — the guard would catch the pre-change descriptor"
    );
    // The shipped reserved-infix API carrier: `.verter.ts` → `.svelte.verter.ts`
    // which is NOT a real extension ⇒ no collision.
    assert!(
        !companion_collides("svelte", ".verter.ts", &real),
        "the shipped `.verter.ts` Svelte API carrier (`.svelte.verter.ts`) must NOT collide"
    );
    // The IDE carrier `.tsx` → `.svelte.tsx` which is NOT a real extension ⇒ no
    // collision (GATE 5 proves `.svelte.tsx` and a real `.svelte.ts` coexist).
    assert!(
        !companion_collides("svelte", ".tsx", &real),
        "the `.svelte.tsx` IDE carrier must NOT collide with the `.svelte.ts` rune module"
    );
    // Vue has no rune-module family, so even a bare `.ts` would not collide for
    // Vue — confirming the collision is Svelte-rune-specific, not a blanket ban.
    assert!(
        !companion_collides("vue", ".ts", &real),
        "Vue has no rune-module family, so `.vue.ts` does not collide — the collision is \
         specific to the Svelte rune family"
    );
}
