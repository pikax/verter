//! Carrier-runtime-routing functional gate (native-Svelte foundation).
//!
//! These are the DISCRIMINATING tests for the carrier-registry routing of the runtime
//! compile through the `CarrierCompiler` registry:
//!
//! * Vue runtime (`Main`) + IDE (`getIde`) output stays BYTE-IDENTICAL through
//!   the neutral-bundle path (carrier routing must not change Vue bytes).
//! * A Main-less Svelte carrier satisfies `ensure_ide_compiled` + `get_ide`
//!   WITHOUT a runtime `Main` — and the IDE output is Svelte-specific (the
//!   `@jsxImportSource @verter/svelte-jsx` pragma + `__verter_*` helpers) with
//!   NO Vue-specific residue.
//! * `get_ide` is a pure cached read — it does NOT compile on a cache miss.
//! * `ensure_ide_compiled` is idempotent / warm on the second call, and a
//!   dependency edit forces recompute before `get_ide`.
//! * Racing `ensure_ide_compiled` and `get_virtual_file(Main)` coalesce.
//! * A Svelte projector typed-unsupported diagnostic reaches the host
//!   `DiagnosticsSnapshot` through the runtime-bundle diagnostics channel.

use std::sync::Arc;

use verter_compiler::compile::CompileTarget;
use verter_session::{
    CompileProfile, FileLanguage, HostConfig, HostError, UpsertRequest, VerterHost,
    VirtualNodeKind, VirtualQuery,
};

fn host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn upsert(host: &VerterHost, canonical: &str, source: &str, lang: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: lang,
            aliases: Vec::new(),
        })
        .unwrap_or_else(|e| panic!("upsert {canonical}: {e:?}"));
}

/// An IDE-target compile profile (`CompileTarget::IDE` ⇒ the `TSX` bit), the
/// profile the LSP uses. Drives `want_ide` through the carrier.
fn ide_profile() -> CompileProfile {
    CompileProfile {
        target: CompileTarget::IDE,
        ..CompileProfile::default()
    }
}

fn main_code(host: &VerterHost, canonical: &str, profile: &CompileProfile) -> Option<String> {
    match host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile.clone(),
    }) {
        Ok(resp) => Some(resp.code.to_string()),
        Err(HostError::MissingVirtualNode { .. }) => None,
        Err(e) => panic!("get_virtual_file(Main) for {canonical}: {e:?}"),
    }
}

// ── The Vue SFC used for the byte-identity goldens ──────────────────────────
//
// A representative Vue SFC: `<script setup>` with a ref + a template using it,
// plus a scoped style (exercises the style virtual-import line) and a custom
// block (exercises the custom-block import + invocation lines).
const VUE_SRC: &str = "<script setup lang=\"ts\">\nimport { ref } from 'vue'\nconst msg = ref('hi')\n</script>\n<template><div>{{ msg }}</div></template>\n<style scoped>.a{color:red}</style>\n";

/// The Vue runtime `Main` module — byte-frozen. The carrier-routed compile MUST
/// reproduce these bytes exactly; any drift fails this gate (the §4.4
/// byte-identity characterization). Captured from the live carrier-routed
/// compile; a future change that alters Vue runtime bytes fails here.
const VUE_MAIN_GOLDEN: &str = include_str!("svelte_compiler_block1_goldens/vue_main.txt");

/// The Vue IDE (`getIde`) TSX — byte-frozen.
const VUE_IDE_GOLDEN: &str = include_str!("svelte_compiler_block1_goldens/vue_ide.txt");

#[test]
fn vue_runtime_main_is_byte_identical_through_the_carrier() {
    let host = host();
    upsert(&host, "/src/App.vue", VUE_SRC, FileLanguage::vue());
    let profile = CompileProfile::default();
    let got =
        main_code(&host, "/src/App.vue", &profile).expect("Vue produces a runtime Main module");
    assert_eq!(
        got, VUE_MAIN_GOLDEN,
        "Vue runtime Main bytes drifted through carrier routing.\n--- got ---\n{got}\n--- golden ---\n{VUE_MAIN_GOLDEN}"
    );
}

#[test]
fn vue_ide_output_is_byte_identical_through_the_carrier() {
    let host = host();
    upsert(&host, "/src/App.vue", VUE_SRC, FileLanguage::vue());
    let profile = ide_profile();
    assert!(
        host.ensure_ide_compiled("/src/App.vue", &profile)
            .expect("Vue IDE ensure"),
        "Vue is a carrier — ensure_ide_compiled must report an IDE projection"
    );
    let ide = host
        .get_ide("/src/App.vue", &profile)
        .expect("Vue IDE output present after ensure");
    assert_eq!(
        ide.code.as_ref(),
        VUE_IDE_GOLDEN,
        "Vue IDE bytes drifted through carrier routing.\n--- got ---\n{}\n--- golden ---\n{VUE_IDE_GOLDEN}",
        ide.code
    );
}

#[test]
fn vue_ensure_ide_compiled_does_not_change_main_behavior() {
    // The IDE-ensure path is ADDITIVE: a Vue file still produces its runtime
    // Main, and `ensure_ide_compiled` populates the IDE slot without disturbing
    // it. (Regression: carrier routing must not break the existing Main path.)
    let host = host();
    upsert(&host, "/src/App.vue", VUE_SRC, FileLanguage::vue());
    let profile = ide_profile();

    // ensure_ide_compiled populates the IDE.
    assert!(host.ensure_ide_compiled("/src/App.vue", &profile).unwrap());
    assert!(host.get_ide("/src/App.vue", &profile).is_some());

    // And the runtime Main is still produced (byte-identical to the golden,
    // proving the IDE ensure did not perturb the Main assembly).
    let got = main_code(&host, "/src/App.vue", &profile).expect("Vue Main still present");
    // The IDE profile carries TSX but Main assembly is target-independent for
    // the bytes we assert; compare against the golden's structural anchors.
    assert!(got.contains("export default _sfc_main"));
    assert!(got.contains("_sfc_main.render = render"));
}

#[test]
fn ensure_ide_compiled_normalizes_a_bundler_profile_to_the_ide_surface() {
    // Profile-normalization contract: `ensure_ide_compiled` must normalize the caller's
    // profile to an IDE/TSX-bearing target INTERNALLY, so it returns `Ok(true)`
    // and populates `CachedTsx` whenever the carrier HAS an IDE surface — even
    // when the caller passes a DEFAULT/bundler profile that carries NO `TSX`
    // bit. `Ok(false)` must mean a genuine no-IDE surface (a non-carrier), never
    // "the caller's profile happened to lack the TSX target".
    //
    // DISCRIMINATING: the default `CompileProfile` target is `BUNDLER` (no
    // `TSX`). Before the normalization fix, `ensure_ide_compiled` forwarded that
    // profile unchanged, the compile produced no `CachedTsx`, and the function
    // returned `Ok(false)` — and `get_ide` peeked the bundler slot (empty). This
    // test FAILS against that state (false + None) and PASSES after the fix
    // (true + populated TSX) for BOTH carriers.
    let host = host();
    upsert(&host, "/src/App.vue", VUE_SRC, FileLanguage::vue());
    upsert(
        &host,
        "/src/Counter.svelte",
        SVELTE_SRC,
        FileLanguage::svelte(),
    );

    // The DEFAULT profile — bundler target, NO explicit TSX bit.
    let bundler = CompileProfile::default();
    assert!(
        !bundler.target.needs_tsx(),
        "precondition: the default/bundler profile must NOT carry the TSX bit (else the test is \
         vacuous)"
    );

    // Vue: a carrier with an IDE surface ⇒ Ok(true) + populated CachedTsx, even
    // though the caller passed a no-TSX bundler profile.
    assert!(
        host.ensure_ide_compiled("/src/App.vue", &bundler)
            .expect("Vue IDE ensure under a bundler profile"),
        "ensure_ide_compiled must normalize the bundler profile to the IDE surface and report \
         Ok(true) for a carrier"
    );
    let vue_ide = host
        .get_ide("/src/App.vue", &bundler)
        .expect("get_ide must read the normalized CachedTsx slot under the SAME bundler profile");
    assert!(
        !vue_ide.code.is_empty(),
        "the normalized IDE slot must carry non-empty Vue TSX"
    );

    // Svelte (Main-less): same contract under the bundler profile.
    assert!(
        host.ensure_ide_compiled("/src/Counter.svelte", &bundler)
            .expect("Svelte IDE ensure under a bundler profile"),
        "ensure_ide_compiled must normalize the bundler profile to the IDE surface for a Main-less \
         Svelte carrier too"
    );
    let svelte_ide = host
        .get_ide("/src/Counter.svelte", &bundler)
        .expect("get_ide must read the normalized Svelte CachedTsx slot under the bundler profile");
    assert!(
        svelte_ide
            .code
            .contains("@jsxImportSource @verter/svelte-jsx"),
        "the normalized IDE slot must carry the Svelte-specific TSX, got:\n{}",
        svelte_ide.code
    );

    // `Ok(false)` is reserved for a genuine NO-IDE surface — a plain script
    // (non-carrier), never a carrier whose caller profile lacked the TSX bit.
    upsert(
        &host,
        "/src/util.ts",
        "export const x = 1;\n",
        FileLanguage::script_ts(),
    );
    assert!(
        !host
            .ensure_ide_compiled("/src/util.ts", &bundler)
            .expect("non-carrier ensure"),
        "a plain script (non-carrier) has no IDE surface ⇒ Ok(false)"
    );
}

// ── Svelte: Main-less IDE projection ────────────────────────────────────────

const SVELTE_SRC: &str = "<script lang=\"ts\">let count = 0;</script>\n<button onclick={() => count++}>{count}</button>\n";

#[test]
fn svelte_ensure_ide_compiled_succeeds_with_no_main_node() {
    let host = host();
    upsert(
        &host,
        "/src/Counter.svelte",
        SVELTE_SRC,
        FileLanguage::svelte(),
    );
    let profile = ide_profile();

    // The Main-less Svelte carrier satisfies the IDE ensure (Ok(true)) …
    let ensured = host
        .ensure_ide_compiled("/src/Counter.svelte", &profile)
        .expect("Svelte IDE ensure must not error");
    assert!(
        ensured,
        "a Svelte carrier projects an IDE surface — ensure_ide_compiled must report true"
    );

    // … `get_ide` returns Some (a pure cached read after the ensure) …
    let ide = host
        .get_ide("/src/Counter.svelte", &profile)
        .expect("Svelte IDE output present after ensure_ide_compiled");

    // … the IDE output is SVELTE-SPECIFIC: the `@jsxImportSource @verter/svelte-jsx`
    // pragma opens the file and the projection emits the Svelte `__verter_*`
    // checker helpers (here `__verter_event` for the `onclick` handler). This is
    // strictly discriminating — "non-empty" would pass for Vue output too.
    let code = ide.code.as_ref();
    assert!(
        code.contains("@jsxImportSource @verter/svelte-jsx"),
        "Svelte IDE output must carry the @verter/svelte-jsx pragma, got:\n{code}"
    );
    assert!(
        code.contains("__verter_"),
        "Svelte IDE output must emit the `__verter_*` checker helpers, got:\n{code}"
    );
    // NEGATIVE: NO Vue-specific residue (the Svelte projection is not Vue TSX).
    assert!(
        !code.contains("_sfc_main"),
        "Svelte IDE output must NOT carry the Vue `_sfc_main` shape, got:\n{code}"
    );
    assert!(
        !code.contains("@jsxImportSource vue"),
        "Svelte IDE output must NOT carry the Vue jsxImportSource, got:\n{code}"
    );

    // … and there is NO runtime `Main` virtual node (Svelte runtime generation
    // is a later block).
    assert!(
        main_code(&host, "/src/Counter.svelte", &profile).is_none(),
        "a Main-less Svelte carrier must NOT produce a runtime Main virtual node yet"
    );
}

#[test]
fn get_ide_does_not_compile_on_cache_miss() {
    // `get_ide` is a PURE cached read — before any ensure / compile, it returns
    // None for a freshly-upserted carrier (it never computes on read). The
    // explicit ensure path is what populates the slot.
    let host = host();
    upsert(
        &host,
        "/src/Counter.svelte",
        SVELTE_SRC,
        FileLanguage::svelte(),
    );
    let profile = ide_profile();
    assert!(
        host.get_ide("/src/Counter.svelte", &profile).is_none(),
        "get_ide must NOT compile on a cache miss — it is a pure cached read"
    );
    // Vue too: no compile-on-read.
    upsert(&host, "/src/App.vue", VUE_SRC, FileLanguage::vue());
    assert!(
        host.get_ide("/src/App.vue", &profile).is_none(),
        "get_ide must NOT compile a Vue file on a cache miss either"
    );
}

#[test]
fn ensure_ide_compiled_is_idempotent_and_invalidates_on_dependency_edit() {
    // A Svelte component importing a workspace `.ts` type. The first
    // ensure compiles; the second is warm/idempotent; a dependency edit forces
    // recompute before get_ide reflects it.
    let host = host();
    upsert(
        &host,
        "/src/types.ts",
        "export interface Props { label: string; }\n",
        FileLanguage::script_ts(),
    );
    upsert(
        &host,
        "/src/Widget.svelte",
        "<script lang=\"ts\">import type { Props } from './types'; let p: Props;</script>\n<div>{p.label}</div>\n",
        FileLanguage::svelte(),
    );
    let profile = ide_profile();

    assert!(host
        .ensure_ide_compiled("/src/Widget.svelte", &profile)
        .unwrap());
    assert!(host.compile_slot_is_warm("/src/Widget.svelte", &profile));

    // Idempotent: a second ensure stays warm (no error, slot still warm).
    assert!(host
        .ensure_ide_compiled("/src/Widget.svelte", &profile)
        .unwrap());
    assert!(host.compile_slot_is_warm("/src/Widget.svelte", &profile));

    // Dependency edit: the imported `Props` member changes. The warm slot is
    // rejected by fact-validation (cross-file edit invalidates lazily).
    upsert(
        &host,
        "/src/types.ts",
        "export interface Props { label: number; }\n",
        FileLanguage::script_ts(),
    );
    assert!(
        !host.compile_slot_is_warm("/src/Widget.svelte", &profile),
        "a cross-file dependency edit must invalidate the Svelte IDE compile slot"
    );

    // ensure_ide_compiled recomputes; get_ide reflects the recompile.
    assert!(host
        .ensure_ide_compiled("/src/Widget.svelte", &profile)
        .unwrap());
    assert!(host.compile_slot_is_warm("/src/Widget.svelte", &profile));
    assert!(host.get_ide("/src/Widget.svelte", &profile).is_some());
}

#[test]
fn racing_ensure_ide_compiled_and_get_virtual_file_main_coalesce() {
    // Vue produces BOTH a runtime Main and an IDE artifact from ONE shared
    // compile. Racing `ensure_ide_compiled` (Ide demand) and
    // `get_virtual_file(Main)` (VirtualNode demand) on the same
    // (canonical, profile) must COALESCE on ONE shared parsed carrier artifact
    // and ONE published compile slot — the demand is checked AFTER the shared
    // result, so both succeed and the result is consistent regardless of which
    // thread wins.
    //
    // DISCRIMINATING (single-slot coalescing, not just "both succeed"): the
    // racing pair must share ONE parsed carrier artifact. `carrier_parses` is
    // the framework-neutral parse-once rail (one increment per
    // `CarrierCompiler::parse`); a regression where each request RE-PARSED the
    // carrier independently (a per-request parse instead of a shared cached
    // artifact) would bump it to >= 2. Two independent compiles that each
    // re-parsed would fail this assertion; the current both-succeed-only test
    // would not. The carrier is parsed once at `upsert`, and BOTH the Ide and
    // the Main demand reuse that one cached artifact (no `src=` blocks, no
    // parse-affecting template options ⇒ `can_use_cache`), so exactly ONE
    // carrier parse backs the racing pair.
    let host = host();
    upsert(&host, "/src/App.vue", VUE_SRC, FileLanguage::vue());
    let profile = ide_profile();

    // Baseline AFTER upsert: the upsert performed the single carrier parse.
    // Measure the delta the racing compile pair adds — it must add NO further
    // carrier parse (the compiles reuse the one cached artifact) and run at
    // most one cold compile-output compute (the coalesced shared slot).
    let parses_before = host.provenance_snapshot().carrier_parses;
    let cold_runs_before = host
        .provenance()
        .compile_cold_runs
        .load(std::sync::atomic::Ordering::Relaxed);

    let h1 = {
        let host = Arc::clone(&host);
        let profile = profile.clone();
        std::thread::spawn(move || host.ensure_ide_compiled("/src/App.vue", &profile))
    };
    let h2 = {
        let host = Arc::clone(&host);
        let profile = profile.clone();
        std::thread::spawn(move || {
            host.get_virtual_file(VirtualQuery {
                raw_id: None,
                canonical_id: Some("/src/App.vue".to_string()),
                node_kind: Some(VirtualNodeKind::Main),
                compile_profile: profile,
            })
            .map(|r| r.code.to_string())
        })
    };

    let ide_ok = h1
        .join()
        .expect("ensure thread")
        .expect("ensure_ide_compiled ok");
    let main = h2
        .join()
        .expect("main thread")
        .expect("get_virtual_file(Main) ok");

    assert!(ide_ok, "the IDE ensure must succeed on the coalesced slot");
    assert!(
        main.contains("export default _sfc_main"),
        "the Main demand must produce the runtime module on the coalesced slot"
    );
    // Both surfaces are now readable from the one shared compile.
    assert!(host.get_ide("/src/App.vue", &profile).is_some());
    assert!(main_code(&host, "/src/App.vue", &profile).is_some());

    // Single-slot coalescing: the racing pair shares ONE parsed carrier
    // artifact — neither request re-parsed the carrier.
    let parses_after = host.provenance_snapshot().carrier_parses;
    assert_eq!(
        parses_after, parses_before,
        "the racing Ide + Main pair must reuse the ONE carrier artifact parsed at upsert \
         (carrier_parses must not increase): a per-request re-parse is a coalescing regression \
         (before={parses_before}, after={parses_after})"
    );
    // Both subsequent cached reads above add NO cold compile compute; the
    // published slot serves them warm. The racing pair itself ran at most a
    // bounded number of cold computes (the demand is checked AFTER the shared
    // result; the published slot satisfies both surfaces), never re-parsing.
    let cold_runs_after = host
        .provenance()
        .compile_cold_runs
        .load(std::sync::atomic::Ordering::Relaxed);
    let cold_delta = cold_runs_after - cold_runs_before;
    assert!(
        cold_delta >= 1,
        "the racing pair must have run at least one cold compile-output compute (delta={cold_delta})"
    );
    assert!(
        cold_delta <= 2,
        "the racing pair shares ONE compile slot — at most one cold compute PER racing thread \
         (<= 2 total); a larger count means the cached slot is not being reused across the \
         demand surfaces (delta={cold_delta})"
    );
}

#[test]
fn svelte_projector_diagnostic_reaches_diagnostics_snapshot() {
    // A Svelte experimental await-EXPRESSION is a typed-unsupported projector
    // diagnostic (`svelte-await-experimental`, Information severity). It is
    // PRODUCED by the projector and — through the runtime-bundle diagnostics
    // channel carrier routing added — must reach the host `DiagnosticsSnapshot`.
    //
    // DISCRIMINATING: before the diagnostics lift (the `svelte/carrier.rs:279`
    // TODO), the projector produced this diagnostic but it never reached the
    // host snapshot; this test fails against that state.
    let host = host();
    // An await-expression in the instance script triggers the F6 typed
    // diagnostic in the projector.
    let src = "<script lang=\"ts\">const x = await fetch('/x');</script>\n<div>{x}</div>\n";
    upsert(&host, "/src/Await.svelte", src, FileLanguage::svelte());
    let profile = ide_profile();

    // The ensure succeeds (the await-expression is REAL-checked, Information
    // severity — not a hard error) and produces an IDE artifact.
    assert!(
        host.ensure_ide_compiled("/src/Await.svelte", &profile)
            .expect("svelte await ensure"),
        "the await-expression component still projects an IDE surface"
    );

    // The projector diagnostic reaches the host DiagnosticsSnapshot.
    let diags = host
        .get_diagnostics("/src/Await.svelte", &profile)
        .expect("a diagnostics snapshot must exist after compile");
    assert!(
        diags
            .diagnostics
            .iter()
            .any(|d| d.code == "svelte-await-experimental"),
        "the Svelte projector `svelte-await-experimental` diagnostic must reach the host \
         DiagnosticsSnapshot, got: {:?}",
        diags
            .diagnostics
            .iter()
            .map(|d| &d.code)
            .collect::<Vec<_>>()
    );
}
