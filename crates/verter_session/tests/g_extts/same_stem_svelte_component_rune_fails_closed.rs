//! Guard: `same_stem_svelte_component_rune_fails_closed` (ownership half).
//!
//! Because the engine probes `.svelte.ts` BEFORE `.svelte.tsx`, a real same-stem
//! `Foo.svelte.ts` rune module beside a `Foo.svelte` component makes the bare
//! `import "./Foo.svelte"` AMBIGUOUS — the rune shadows the carrier. Verter
//! DETECTS this same-stem collision and FAILS CLOSED (the source is `Ambiguous`,
//! no external-TS result, no silently-wrong edge).
//!
//! DISCRIMINATING: without the same-stem-rune probe the `Foo.svelte` component
//! would bind cleanly; the control (a DIFFERENT-stem rune nearby) must still bind
//! cleanly, so the guard pins the same-stem detection specifically. The rune
//! extensions are sourced from `all_adapter_module_extensions()` — never
//! hardcoded — and matched to the carrier's own family, so a `.vue` source (no
//! `vue.*` rune family) is never tripped.

use verter_session::external_ts::{AmbiguityCause, ProjectResolution};

use crate::shared::resolve_with;

const TS: &str = "d:/ws/tsconfig.json";

#[test]
fn same_stem_svelte_component_rune_fails_closed() {
    // Sanity: the svelte rune extensions are registered (the data this guard
    // keys on). Without them the same-stem case is vacuous.
    let rune_exts = verter_session::LanguageRegistry::global().all_adapter_module_extensions();
    assert!(
        rune_exts.contains(&"svelte.ts"),
        "the svelte adapter must register the `svelte.ts` rune-module extension; got {rune_exts:?}"
    );

    // A `Foo.svelte` component beside a real same-stem `Foo.svelte.ts` rune ⇒
    // DETECTED ambiguity ⇒ Ambiguous (fail closed).
    let res = resolve_with(
        &[
            (TS, r#"{ "include": ["src/**/*"] }"#),
            ("d:/ws/src/Foo.svelte", "<script></script>"),
            ("d:/ws/src/Foo.svelte.ts", "export const rune = 1;"),
        ],
        &[TS],
        "d:/ws/src/Foo.svelte",
    );
    assert_eq!(
        res,
        ProjectResolution::Ambiguous(AmbiguityCause::SameStemRuneModule),
        "a same-stem `Foo.svelte.ts` rune beside `Foo.svelte` must fail closed (Ambiguous)"
    );

    // Control: a DIFFERENT-stem rune nearby must NOT trip the same-stem check —
    // the component binds cleanly. (Discriminates same-stem detection from a
    // blanket "any rune nearby" downgrade.)
    let control = resolve_with(
        &[
            (TS, r#"{ "include": ["src/**/*"] }"#),
            ("d:/ws/src/Clean.svelte", "<script></script>"),
            ("d:/ws/src/state.svelte.ts", "export const s = 1;"),
        ],
        &[TS],
        "d:/ws/src/Clean.svelte",
    );
    assert!(
        matches!(control, ProjectResolution::ProjectBinding(_)),
        "control: a DIFFERENT-stem rune (`state.svelte.ts`) must not downgrade \
         `Clean.svelte` — it binds cleanly (got {control:?})"
    );
}
