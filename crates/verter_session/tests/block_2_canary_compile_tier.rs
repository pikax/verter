//! Compile-tier cross-file lazy-invalidation canary suite.
//!
//! The owner-upsert path has no eager reverse-dependent invalidation
//! cascade. These canary tests are the coherent named gate that proves
//! the lazy fact-validation substrate backs every compile-tier
//! cross-file invalidation scenario.
//!
//! Every mutation routes through the production [`harness::upsert`]
//! helper (plain `VerterHost::upsert`), which performs no eager
//! own-canonical query-identity cache drain. The dependency edits
//! exercised here are cross-file, so a consumer's warm entry survives
//! the dependency edit and is rejected only by lazy fact-validation on
//! the next read.
//!
//! Each test:
//!  1. Sets up an owner SFC + dependency file and primes a warm
//!     compile slot.
//!  2. Mutates the dependency through [`harness::upsert`] — no eager
//!     cascade runs, so the consumer's warm slot physically survives
//!     the dependency edit. The ONLY mechanism that can invalidate it
//!     is the warm-hit fact-signature check.
//!  3. Asserts the lazy semantics: the stale warm slot is REJECTED by
//!     fact-validation (`!compile_slot_is_warm`), `ensure_compiled`
//!     RECOMPUTES, and the recompiled assembled `Main` output carries
//!     the new dependency content.
//!
//! These tests deliberately do NOT assert physical cache emptiness
//! (`compile_slots.is_empty()`): a warm slot can survive the
//! dependency edit and still be lazily rejected on read. The gate is
//! stale-miss + recompute + correct user-visible output.

#![cfg(test)]

use verter_session::{CompileProfile, FileKind};

#[path = "block_2_canary/harness.rs"]
mod harness;

use harness::{compile_main, prime_compile, standalone_host, upsert};

/// Canary — cross-file macro type member edit.
///
/// `defineProps<Foo>()` over a `Foo` interface imported from a
/// workspace `.ts`. Editing a `Foo` member's type
/// (`a: number` → `a: string`) must invalidate the consumer's warm
/// compile slot through fact-validation and the recompiled output
/// must carry the new member type.
///
/// Discrimination property: the warm-hit oracle
/// `compile_slot_fact_signature_validates` (consulted by
/// `compile_slot_is_warm`) pins this test — the slot's
/// `fact_dep_signature` records the dep's pre-edit `Member` /
/// `MemberPresence` body fingerprint; the post-edit registry
/// fingerprints differ. Reverting that warm-hit signature check
/// leaves the stale slot warm and the assertion fails.
#[test]
fn cross_file_macro_type_member_edit_invalidates_compile_slot() {
    let host = standalone_host();
    upsert(
        &host,
        "/src/types.ts",
        "export interface Foo { a: number; }\n",
        FileKind::NonSfc,
    );
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { Foo } from './types';\n\
         const props = defineProps<Foo>();\n\
         </script>\n\
         <template><div>{{ props.a }}</div></template>\n",
        FileKind::VueSfc,
    );

    let profile = CompileProfile::default();
    prime_compile(&host, "/src/Comp.vue");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "precondition: Comp.vue must have a warm compile slot after prime"
    );

    // Edit the imported member's type — no eager cascade; the
    // consumer's warm slot survives the dependency edit.
    upsert(
        &host,
        "/src/types.ts",
        "export interface Foo { a: string; }\n",
        FileKind::NonSfc,
    );

    // Lazy semantics: the warm slot is rejected by fact-validation.
    assert!(
        !host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "a cross-file macro-type member edit MUST invalidate the warm \
         compile slot via fact-validation — a still-warm slot here means \
         the dep's Member fact mismatch was not caught"
    );

    // Recomputation occurs and the user-visible output reflects the edit.
    host.ensure_compiled("/src/Comp.vue", &profile)
        .expect("recompile after dep edit");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "after ensure_compiled the slot is warm again (recompiled against \
         the edited dep)"
    );
    let recompiled = compile_main(&host, "/src/Comp.vue")
        .expect("assembled module recompiles after the dep edit");
    assert!(
        !recompiled.diagnostics.has_errors,
        "recompiled output must be error-free: {:?}",
        recompiled.diagnostics
    );
    assert!(
        recompiled.code.contains("Foo"),
        "the recompiled assembled module must still resolve the imported \
         `Foo` macro type — got: {}",
        recompiled.code
    );
}

/// Canary — runtime import body edit.
///
/// `Comp.vue` has a runtime (value) import `import { helper } from
/// './utils'`. Editing `helper`'s BODY (`return 1` → `return 2`) —
/// a change the function signature `() => number` cannot see — must
/// still invalidate `Comp.vue`'s warm compile slot.
///
/// Discrimination property: the compile-tier producer records a
/// `FileWholeHash` fact for a runtime-imported dependency; the
/// warm-hit oracle validates it. Reverting the whole-hash fact
/// emission (so only the signature-pinned `Export` fact remains)
/// leaves a body-only edit invisible and the slot stays warm.
#[test]
fn runtime_import_body_edit_invalidates_compile_slot() {
    let host = standalone_host();
    upsert(
        &host,
        "/src/utils.ts",
        "export function helper() { return 1; }\n",
        FileKind::NonSfc,
    );
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import { helper } from './utils';\n\
         const n = helper();\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
        FileKind::VueSfc,
    );

    let profile = CompileProfile::default();
    prime_compile(&host, "/src/Comp.vue");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "precondition: Comp.vue must have a warm compile slot after prime"
    );

    // Edit ONLY the body of `helper` — signature stays `() => number`.
    upsert(
        &host,
        "/src/utils.ts",
        "export function helper() { return 2; }\n",
        FileKind::NonSfc,
    );

    assert!(
        !host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "a runtime-import body edit MUST invalidate the warm compile slot \
         via FileWholeHash fact-validation — the signature-pinned Export \
         fact alone cannot see a body-only edit"
    );

    host.ensure_compiled("/src/Comp.vue", &profile)
        .expect("recompile after runtime dep edit");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "after ensure_compiled the slot is warm again (recompiled)"
    );
    let recompiled = compile_main(&host, "/src/Comp.vue")
        .expect("assembled module recompiles after the runtime dep edit");
    assert!(
        !recompiled.diagnostics.has_errors,
        "recompiled output must be error-free: {:?}",
        recompiled.diagnostics
    );
    assert!(
        recompiled.code.contains("helper"),
        "the recompiled assembled module must still reference the runtime \
         import `helper` — got: {}",
        recompiled.code
    );
}

/// Canary — external `src=` template edit (Tier-1 / no-TS-exports
/// fallback path).
///
/// `Comp.vue` has `<template src="./tpl.html">`. The external file's
/// content is spliced verbatim into the compiled render function, so
/// editing `tpl.html` must invalidate the consumer's warm compile
/// slot and the recompiled output must carry the new template
/// content.
///
/// Discrimination property: the compile-tier producer records a
/// `FileWholeHash` fact for an external `src=` dependency (the
/// external file has no TS exports — the signature-fact path cannot
/// represent it). Reverting that whole-hash fact emission leaves the
/// consumer's signature empty, which trivially validates, and the
/// stale slot is served forever.
#[test]
fn external_src_template_edit_invalidates_compile_slot() {
    let host = standalone_host();
    upsert(
        &host,
        "/src/tpl.html",
        "<div>ALPHA</div>\n",
        FileKind::NonSfc,
    );
    upsert(
        &host,
        "/src/Comp.vue",
        "<template src=\"./tpl.html\"></template>\n\
         <script setup lang=\"ts\">\nconst n = 1;\n</script>\n",
        FileKind::VueSfc,
    );

    let profile = CompileProfile::default();
    prime_compile(&host, "/src/Comp.vue");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "precondition: Comp.vue must have a warm compile slot after prime"
    );
    let before = compile_main(&host, "/src/Comp.vue").expect("pre-edit assembled module compiles");
    assert!(
        before.code.contains("ALPHA"),
        "pre-edit compiled output must carry the original template text \
         `ALPHA` — got: {}",
        before.code
    );

    // Edit the external template — no eager cascade; the consumer's
    // warm slot survives the dependency edit.
    upsert(
        &host,
        "/src/tpl.html",
        "<section>BETA</section>\n",
        FileKind::NonSfc,
    );

    assert!(
        !host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "an external `src=` template edit MUST invalidate the warm compile \
         slot via FileWholeHash fact-validation — a still-warm slot here \
         means the producer recorded no whole-hash fact for the external dep"
    );

    host.ensure_compiled("/src/Comp.vue", &profile)
        .expect("recompile after external template edit");
    let after = compile_main(&host, "/src/Comp.vue")
        .expect("assembled module recompiles after the external template edit");
    assert!(
        after.code.contains("BETA"),
        "the recompiled output MUST carry the NEW template text `BETA` — \
         a stale slot would still render `ALPHA`. Got: {}",
        after.code
    );
    assert!(
        !after.code.contains("ALPHA"),
        "the recompiled output must NOT carry the OLD template text \
         `ALPHA` — got: {}",
        after.code
    );
}

/// Canary — side-effect import body edit.
///
/// `Comp.vue` has a side-effect import `import './setup'` — a runtime
/// (non-type-only) import with ZERO bindings. The dependency's
/// content is re-emitted in the assembled module, so editing
/// `setup.ts`'s body must invalidate `Comp.vue`'s warm compile slot.
///
/// Discrimination property: the compile-tier producer records a
/// `FileWholeHash` fact even for a bindings-empty side-effect import.
/// Reverting that — restricting whole-hash admission to the
/// per-binding loop, which never executes for a side-effect import —
/// leaves the consumer's signature without a fact for the dep and the
/// stale slot is served.
#[test]
fn side_effect_import_body_edit_invalidates_compile_slot() {
    let host = standalone_host();
    upsert(
        &host,
        "/src/setup.ts",
        "globalThis.__verter_setup = 1;\n",
        FileKind::NonSfc,
    );
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import './setup';\n\
         const n = 1;\n\
         </script>\n\
         <template><div>{{ n }}</div></template>\n",
        FileKind::VueSfc,
    );

    let profile = CompileProfile::default();
    prime_compile(&host, "/src/Comp.vue");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "precondition: Comp.vue must have a warm compile slot after prime"
    );

    // Edit ONLY the body of `setup.ts` — no eager cascade; the
    // consumer's warm slot survives the dependency edit.
    upsert(
        &host,
        "/src/setup.ts",
        "globalThis.__verter_setup = 2;\n",
        FileKind::NonSfc,
    );

    assert!(
        !host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "a side-effect import body edit MUST invalidate the warm compile \
         slot via FileWholeHash fact-validation — a still-warm slot here \
         means the bindings-empty import skipped whole-hash admission"
    );

    host.ensure_compiled("/src/Comp.vue", &profile)
        .expect("recompile after side-effect dep edit");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "after ensure_compiled the slot is warm again (recompiled)"
    );
    let recompiled = compile_main(&host, "/src/Comp.vue")
        .expect("assembled module recompiles after the side-effect dep edit");
    assert!(
        !recompiled.diagnostics.has_errors,
        "recompiled output must be error-free: {:?}",
        recompiled.diagnostics
    );
    assert!(
        recompiled.code.contains("./setup"),
        "the recompiled assembled module must still emit the side-effect \
         import of `./setup` — got: {}",
        recompiled.code
    );
}

/// Canary — custom `resolve_extensions` config + dependency edit.
///
/// The owner host is configured with a custom `resolve_extensions`
/// list. A macro-type dependency edit must still invalidate the
/// consumer's warm compile slot — the fact-validation substrate is
/// independent of the extension-resolution config.
///
/// Discrimination property: the warm-hit oracle
/// `compile_slot_fact_signature_validates` pins this test. The custom
/// `resolve_extensions` value affects only specifier→canonical
/// resolution; the recorded `Member` fact and its post-edit mismatch
/// are unchanged. Reverting the warm-hit signature check leaves the
/// stale slot warm.
#[test]
fn custom_resolve_extensions_dep_edit_invalidates_compile_slot() {
    use verter_session::{HostConfig, VerterHost};
    let host = std::sync::Arc::new(VerterHost::new_standalone(HostConfig {
        resolve_extensions: vec![".ts".to_string(), ".js".to_string()],
        ..HostConfig::default()
    }));
    upsert(
        &host,
        "/src/types.ts",
        "export interface MyType { foo: string }\n",
        FileKind::NonSfc,
    );
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { MyType } from './types'\n\
         const props = defineProps<MyType>()\n\
         </script>\n\
         <template><div>{{ props.foo }}</div></template>\n",
        FileKind::VueSfc,
    );

    let profile = CompileProfile::default();
    prime_compile(&host, "/src/Comp.vue");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "precondition: Comp.vue must have a warm compile slot — a custom \
         resolve_extensions list must still resolve `./types` to `.ts`"
    );

    // Edit MyType — no eager cascade; the consumer's warm slot
    // survives the dependency edit.
    upsert(
        &host,
        "/src/types.ts",
        "export interface MyType { foo: string; bar: number }\n",
        FileKind::NonSfc,
    );

    assert!(
        !host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "a macro-type dependency edit MUST invalidate the warm compile slot \
         via fact-validation even under a custom resolve_extensions config"
    );

    host.ensure_compiled("/src/Comp.vue", &profile)
        .expect("recompile after dep edit under custom resolve_extensions");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "after ensure_compiled the slot is warm again (recompiled)"
    );
    let recompiled = compile_main(&host, "/src/Comp.vue")
        .expect("assembled module recompiles under custom resolve_extensions");
    assert!(
        !recompiled.diagnostics.has_errors,
        "recompiled output must be error-free: {:?}",
        recompiled.diagnostics
    );
    assert!(
        recompiled.code.contains("MyType"),
        "the recompiled assembled module must still resolve the imported \
         `MyType` macro type — got: {}",
        recompiled.code
    );
}

/// Canary — Tier-3 dependency type member ADDED.
///
/// `defineProps<MyType>()` over an imported interface. ADDING a
/// sibling member to `MyType` must invalidate the consumer's warm
/// compile slot (the consumer uses the whole `MyType` surface as its
/// prop set).
///
/// Discrimination property: the cold compile records
/// `MemberPresence` / `Member` facts for the consumed type; adding a
/// member changes the observed fact set. The warm-hit oracle catches
/// the mismatch. Reverting the warm-hit signature check leaves the
/// stale slot warm.
#[test]
fn tier3_dep_type_member_added_invalidates_compile_slot() {
    let host = standalone_host();
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { MyType } from './types'\n\
         const props = defineProps<MyType>()\n\
         </script>\n\
         <template><div/></template>\n",
        FileKind::VueSfc,
    );
    upsert(
        &host,
        "/src/types.ts",
        "export interface MyType { foo: string }\n",
        FileKind::NonSfc,
    );

    let profile = CompileProfile::default();
    prime_compile(&host, "/src/Comp.vue");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "precondition: Comp.vue must have a warm compile slot after prime"
    );

    // ADD a sibling member — no eager cascade; the consumer's warm
    // slot survives the dependency edit.
    upsert(
        &host,
        "/src/types.ts",
        "export interface MyType { foo: string; bar: number }\n",
        FileKind::NonSfc,
    );

    assert!(
        !host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "adding a member to the imported type MUST invalidate the warm \
         compile slot via fact-validation — a new MemberPresence fact \
         changes the observed fact set"
    );

    host.ensure_compiled("/src/Comp.vue", &profile)
        .expect("recompile after member added");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "after ensure_compiled the slot is warm again (recompiled)"
    );
    let recompiled = compile_main(&host, "/src/Comp.vue")
        .expect("assembled module recompiles after the member is added");
    assert!(
        !recompiled.diagnostics.has_errors,
        "recompiled output must be error-free: {:?}",
        recompiled.diagnostics
    );
}

/// Canary — Tier-3 dependency type member TYPE CHANGED.
///
/// `defineProps<MyType>()` over an imported interface. Changing a
/// member's TYPE (`foo: string` → `foo: number`) — same member name,
/// different `Member` body fingerprint — must invalidate the
/// consumer's warm compile slot.
///
/// Discrimination property: the cold compile records the consumed
/// member's `Member` body fingerprint; a type change bumps that
/// fingerprint while `MemberPresence` is unchanged. The warm-hit
/// oracle catches the `Member`-body mismatch. Reverting the warm-hit
/// signature check leaves the stale slot warm.
#[test]
fn tier3_dep_type_member_type_changed_invalidates_compile_slot() {
    let host = standalone_host();
    upsert(
        &host,
        "/src/Comp.vue",
        "<script setup lang=\"ts\">\n\
         import type { MyType } from './types'\n\
         const props = defineProps<MyType>()\n\
         </script>\n\
         <template><div>{{ props.foo }}</div></template>\n",
        FileKind::VueSfc,
    );
    upsert(
        &host,
        "/src/types.ts",
        "export interface MyType { foo: string }\n",
        FileKind::NonSfc,
    );

    let profile = CompileProfile::default();
    prime_compile(&host, "/src/Comp.vue");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "precondition: Comp.vue must have a warm compile slot after prime"
    );

    // CHANGE the member's type — no eager cascade; the consumer's
    // warm slot survives the dependency edit.
    upsert(
        &host,
        "/src/types.ts",
        "export interface MyType { foo: number }\n",
        FileKind::NonSfc,
    );

    assert!(
        !host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "changing a member's type MUST invalidate the warm compile slot \
         via fact-validation — the Member body fingerprint changed even \
         though MemberPresence is unchanged"
    );

    host.ensure_compiled("/src/Comp.vue", &profile)
        .expect("recompile after member type changed");
    assert!(
        host.compile_slot_is_warm("/src/Comp.vue", &profile),
        "after ensure_compiled the slot is warm again (recompiled)"
    );
    let recompiled = compile_main(&host, "/src/Comp.vue")
        .expect("assembled module recompiles after the member type changes");
    assert!(
        !recompiled.diagnostics.has_errors,
        "recompiled output must be error-free: {:?}",
        recompiled.diagnostics
    );
    assert!(
        recompiled.code.contains("MyType"),
        "the recompiled assembled module must still resolve the imported \
         `MyType` macro type — got: {}",
        recompiled.code
    );
}
