//! Discriminating tests for the SHARED inserted-import specifier-rewrite layer.
//!
//! Every test asserts the fail-closed contract explicitly: a carrier companion is
//! rewritten to the bare `.vue`/`.svelte`, a non-companion is left UNCHANGED, an
//! unmappable carrier specifier is DROPPED, and — the load-bearing negative — NO
//! `.vue.tsx` / `.svelte.tsx` / `.verter.ts` / `.d.<ext>.ts` companion specifier
//! ever survives into a user-facing edit. Vue AND Svelte.

use super::{rewrite_inserted_carrier_specifier, SpecifierRewrite, SpecifierRewriteCtx};

/// Build a context with the given user-facing edit target and source-exists probe.
fn ctx_with<'a>(
    edit_target_path: &'a str,
    sink: &'a dyn Fn(&str) -> bool,
) -> SpecifierRewriteCtx<'a> {
    SpecifierRewriteCtx {
        edit_target_path,
        carrier_source_exists: sink,
    }
}

/// A `carrier_source_exists` probe that returns `true` for any path in `set`
/// (compared forward-slashed).
fn exists_set(set: &'static [&'static str]) -> impl Fn(&str) -> bool {
    move |p: &str| {
        let q = p.replace('\\', "/");
        set.iter().any(|s| s.replace('\\', "/") == q)
    }
}

/// Assert no carrier-companion suffix leaked into a produced edit text — IDE
/// companion (`.tsx`/`.jsx`), API carrier (`.verter.ts`), OR declaration carrier
/// (`.d.<ext>.ts`).
fn assert_no_companion_leak(text: &str) {
    assert!(
        !text.contains(".vue.tsx")
            && !text.contains(".svelte.tsx")
            && !text.contains(".vue.jsx")
            && !text.contains(".svelte.jsx")
            && !text.contains(".verter.ts")
            && !text.contains(".d.vue.ts")
            && !text.contains(".d.svelte.ts"),
        "a carrier-companion specifier must NEVER leak into a user-facing edit; got: {text:?}"
    );
}

// ── (1) Vue IDE companion `.vue.tsx` → bare `.vue`, quote-preserving ──

#[test]
fn vue_ide_companion_rewrites_to_bare_carrier() {
    let probe = exists_set(&[]);
    let ctx = ctx_with("/ws/src/Consumer.ts", &probe);
    let out = rewrite_inserted_carrier_specifier("import Comp from \"./Comp.vue.tsx\";\n", &ctx);
    assert_eq!(
        out,
        SpecifierRewrite::Rewritten("import Comp from \"./Comp.vue\";\n".to_string()),
        "a `.vue.tsx` IDE-companion specifier must be rewritten to the bare `.vue`"
    );
    if let SpecifierRewrite::Rewritten(t) = &out {
        assert_no_companion_leak(t);
    }
    // Single-quote style preserved.
    let out_sq = rewrite_inserted_carrier_specifier("import Comp from './Comp.vue.tsx';\n", &ctx);
    assert_eq!(
        out_sq,
        SpecifierRewrite::Rewritten("import Comp from './Comp.vue';\n".to_string()),
        "single-quote style must be preserved"
    );
}

// ── (2) Svelte companion + `.jsx` IDE companion + `.verter.ts` API carrier ──

#[test]
fn svelte_companion_and_jsx_and_api_carrier_rewrite_to_bare() {
    let probe = exists_set(&[]);
    let ctx = ctx_with("/ws/src/Consumer.ts", &probe);

    let svelte =
        rewrite_inserted_carrier_specifier("import Widget from \"./Widget.svelte.tsx\";\n", &ctx);
    assert_eq!(
        svelte,
        SpecifierRewrite::Rewritten("import Widget from \"./Widget.svelte\";\n".to_string()),
        "a `.svelte.tsx` companion must be rewritten to the bare `.svelte`"
    );

    let jsx = rewrite_inserted_carrier_specifier("import Comp from \"./Comp.vue.jsx\";\n", &ctx);
    assert_eq!(
        jsx,
        SpecifierRewrite::Rewritten("import Comp from \"./Comp.vue\";\n".to_string()),
        "a `.vue.jsx` IDE-companion (JSX project) must be rewritten to the bare `.vue`"
    );

    let api =
        rewrite_inserted_carrier_specifier("import Comp from \"./Comp.vue.verter.ts\";\n", &ctx);
    assert_eq!(
        api,
        SpecifierRewrite::Rewritten("import Comp from \"./Comp.vue\";\n".to_string()),
        "a `.verter.ts` API-carrier specifier must be rewritten to the bare `.vue`"
    );
    let api_sv =
        rewrite_inserted_carrier_specifier("import W from \"./Widget.svelte.verter.ts\";\n", &ctx);
    assert_eq!(
        api_sv,
        SpecifierRewrite::Rewritten("import W from \"./Widget.svelte\";\n".to_string()),
        "a Svelte `.verter.ts` API-carrier specifier must be rewritten to the bare `.svelte`"
    );
}

// ── side-effect import + non-import text ──

#[test]
fn side_effect_import_rewrites_and_non_import_is_unchanged() {
    let probe = exists_set(&[]);
    let ctx = ctx_with("/ws/src/Consumer.ts", &probe);

    let se = rewrite_inserted_carrier_specifier("import \"./Comp.vue.tsx\";\n", &ctx);
    assert_eq!(
        se,
        SpecifierRewrite::Rewritten("import \"./Comp.vue\";\n".to_string()),
        "a bare side-effect import of a companion must be rewritten"
    );

    let non_import = rewrite_inserted_carrier_specifier("const x = 1;\n", &ctx);
    assert_eq!(
        non_import,
        SpecifierRewrite::Unchanged,
        "text with no import specifier carries nothing to rewrite → Unchanged"
    );
}

// ── (3) bare extension-less `./Comp` — resolve against the edit-target dir ──

#[test]
fn bare_specifier_resolves_to_unique_existing_carrier() {
    // Only `Comp.vue` exists next to the consumer → bare `./Comp` resolves to it.
    let probe = exists_set(&["/ws/src/Comp.vue"]);
    let ctx = ctx_with("/ws/src/Consumer.ts", &probe);
    let out = rewrite_inserted_carrier_specifier("import Comp from \"./Comp\";\n", &ctx);
    assert_eq!(
        out,
        SpecifierRewrite::Rewritten("import Comp from \"./Comp.vue\";\n".to_string()),
        "a bare `./Comp` resolving to a unique sibling `Comp.vue` must be rewritten to `./Comp.vue`"
    );
    if let SpecifierRewrite::Rewritten(t) = &out {
        assert_no_companion_leak(t);
    }

    // Svelte sibling.
    let probe_sv = exists_set(&["/ws/src/Widget.svelte"]);
    let ctx_sv = ctx_with("/ws/src/Consumer.ts", &probe_sv);
    let out_sv = rewrite_inserted_carrier_specifier("import Widget from \"./Widget\";\n", &ctx_sv);
    assert_eq!(
        out_sv,
        SpecifierRewrite::Rewritten("import Widget from \"./Widget.svelte\";\n".to_string()),
        "a bare `./Widget` resolving to a unique sibling `Widget.svelte` must be rewritten"
    );
}

#[test]
fn bare_specifier_ambiguous_between_two_carriers_fails_closed() {
    // BOTH `Comp.vue` and `Comp.svelte` exist → a bare `./Comp` is AMBIGUOUS.
    let probe = exists_set(&["/ws/src/Comp.vue", "/ws/src/Comp.svelte"]);
    let ctx = ctx_with("/ws/src/Consumer.ts", &probe);
    let out = rewrite_inserted_carrier_specifier("import Comp from \"./Comp\";\n", &ctx);
    assert_eq!(
        out,
        SpecifierRewrite::Drop,
        "a bare `./Comp` matching BOTH `Comp.vue` and `Comp.svelte` cannot be unambiguously \
         mapped → the whole action must be DROPPED (fail closed), never guessed"
    );
}

#[test]
fn bare_specifier_with_no_carrier_sibling_is_unchanged() {
    // Neither `Foo.vue` nor `Foo.svelte` exists — `./Foo` is a plain module import
    // (e.g. resolves to `Foo.ts`), NOT a carrier. It must be left UNCHANGED so a
    // real bare-`.ts` import is never mangled.
    let probe = exists_set(&["/ws/src/Foo.ts"]);
    let ctx = ctx_with("/ws/src/Consumer.ts", &probe);
    let out = rewrite_inserted_carrier_specifier("import { foo } from \"./Foo\";\n", &ctx);
    assert_eq!(
        out,
        SpecifierRewrite::Unchanged,
        "a bare `./Foo` with no carrier sibling is a plain module import → Unchanged"
    );
}

// ── non-companion / fail-closed-by-leaving-unchanged ──

#[test]
fn non_companion_specifiers_are_unchanged() {
    let probe = exists_set(&[]);
    let ctx = ctx_with("/ws/src/Consumer.ts", &probe);

    // A plain sibling import.
    assert_eq!(
        rewrite_inserted_carrier_specifier("import { formatCount } from \"./utils\";\n", &ctx),
        SpecifierRewrite::Unchanged,
        "a plain `./utils` import must be left UNCHANGED (not a carrier companion)"
    );

    // A Svelte RUNE module (`.svelte.ts`) is NOT a `.svelte.tsx`/`.verter.ts`
    // companion — leave it intact so a real rune import is never mangled.
    assert_eq!(
        rewrite_inserted_carrier_specifier("import { store } from \"./store.svelte.ts\";\n", &ctx),
        SpecifierRewrite::Unchanged,
        "a Svelte rune module `./store.svelte.ts` must NOT be rewritten (fail closed)"
    );

    // A `.tsx` whose stem is not a carrier.
    assert_eq!(
        rewrite_inserted_carrier_specifier("import x from \"./plain.tsx\";\n", &ctx),
        SpecifierRewrite::Unchanged,
        "a `.tsx` whose stem is not a carrier must be left UNCHANGED"
    );
}

// ── (S5 declaration carrier) `.d.<ext>.ts` → bare carrier ──

#[test]
fn declaration_carrier_companion_rewrites_to_bare_carrier() {
    let probe = exists_set(&[]);
    let ctx = ctx_with("/ws/src/Consumer.ts", &probe);

    // A bare `./Comp.vue` resolves to the `.d.vue.ts` declaration carrier, so an
    // engine-emitted auto-import may name `./Comp.d.vue.ts`; S5 must map it back
    // to the user-facing bare `./Comp.vue`.
    let vue = rewrite_inserted_carrier_specifier("import Comp from \"./Comp.d.vue.ts\";\n", &ctx);
    assert_eq!(
        vue,
        SpecifierRewrite::Rewritten("import Comp from \"./Comp.vue\";\n".to_string()),
        "a `.d.vue.ts` declaration-carrier specifier must be rewritten to the bare `.vue`"
    );
    if let SpecifierRewrite::Rewritten(t) = &vue {
        assert_no_companion_leak(t);
        assert!(
            !t.contains(".d.vue.ts"),
            "no declaration-carrier suffix may survive into the edit: {t:?}"
        );
    }

    // Svelte declaration carrier.
    let svelte =
        rewrite_inserted_carrier_specifier("import W from \"./Widget.d.svelte.ts\";\n", &ctx);
    assert_eq!(
        svelte,
        SpecifierRewrite::Rewritten("import W from \"./Widget.svelte\";\n".to_string()),
        "a `.d.svelte.ts` declaration-carrier specifier must be rewritten to the bare `.svelte`"
    );

    // Parent-relative + single-quote style preserved.
    let parent = rewrite_inserted_carrier_specifier("import Comp from '../Comp.d.vue.ts';\n", &ctx);
    assert_eq!(
        parent,
        SpecifierRewrite::Rewritten("import Comp from '../Comp.vue';\n".to_string()),
        "a parent-relative `../Comp.d.vue.ts` declaration carrier must rewrite to `../Comp.vue`"
    );
}

#[test]
fn non_carrier_d_ts_and_rune_module_are_unchanged() {
    let probe = exists_set(&[]);
    let ctx = ctx_with("/ws/src/Consumer.ts", &probe);

    // NEGATIVE: a plain `.d.ts` whose stem is NOT a carrier (`./types` is not a
    // `.vue`/`.svelte` carrier) must be left UNCHANGED — the `.d.<ext>.ts` arm
    // must require a CARRIER extension between `.d.` and `.ts`, never strip a
    // bare `.d.ts`.
    assert_eq!(
        rewrite_inserted_carrier_specifier("import type { T } from \"./types.d.ts\";\n", &ctx),
        SpecifierRewrite::Unchanged,
        "a non-carrier `./types.d.ts` declaration file must be left UNCHANGED (fail closed)"
    );

    // NEGATIVE: a Svelte RUNE module `.svelte.ts` (no `.d.` infix) is NOT a
    // declaration carrier — it must stay UNCHANGED so a real rune import is never
    // mangled.
    assert_eq!(
        rewrite_inserted_carrier_specifier("import { store } from \"./store.svelte.ts\";\n", &ctx),
        SpecifierRewrite::Unchanged,
        "a Svelte rune module `./store.svelte.ts` (no `.d.` infix) must NOT be rewritten"
    );
}

// ── (6) the marquee negative: a parent-relative companion still rewrites ──

#[test]
fn parent_relative_companion_rewrites_and_never_leaks() {
    let probe = exists_set(&[]);
    let ctx = ctx_with("/ws/src/nested/Consumer.ts", &probe);
    let out = rewrite_inserted_carrier_specifier("import Comp from \"../Comp.vue.tsx\";\n", &ctx);
    assert_eq!(
        out,
        SpecifierRewrite::Rewritten("import Comp from \"../Comp.vue\";\n".to_string()),
        "a parent-relative `../Comp.vue.tsx` companion must rewrite to `../Comp.vue`"
    );
    if let SpecifierRewrite::Rewritten(t) = &out {
        assert_no_companion_leak(t);
    }
}
