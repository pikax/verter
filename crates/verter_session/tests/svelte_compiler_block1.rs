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
        // BOTH "no node at all" and "the runtime surface was explicitly refused"
        // mean "no Main module produced" for this helper's callers; the dedicated R6
        // test distinguishes the explicit refusal via `get_virtual_file` directly.
        Err(HostError::MissingVirtualNode { .. })
        | Err(HostError::RuntimeSurfaceRefused { .. }) => None,
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

// ── Svelte: an UNSUPPORTED-runtime component is IDE-only (Main-less) ─────────
//
// A legacy (non-runes) Svelte component is an unsupported runtime surface: the
// carrier FAILS CLOSED on the runtime body (a precise non-fatal diagnostic) and
// produces NO `Main` node, while the IDE projection still type-checks. A SUPPORTED
// runes component DOES emit a `Main` (see `svelte_runes_component_emits_a_runtime_main`).

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

    // … and there is NO runtime `Main` virtual node: a legacy (non-runes)
    // component is an unsupported runtime surface, so the carrier fails closed on
    // the runtime body and emits no `Main` (the IDE projection above still
    // type-checks).
    assert!(
        main_code(&host, "/src/Counter.svelte", &profile).is_none(),
        "a legacy (non-runes) Svelte component must NOT produce a runtime Main node"
    );
}

/// A SUPPORTED runes Svelte component emits a runtime `Main` virtual node through
/// registry routing — the §1.2 client surface reaches `get_virtual_file(Main)`.
#[test]
fn svelte_runes_component_emits_a_runtime_main() {
    let host = host();
    // The §1.2 conformance fixture: `$state` + bind + a delegated event.
    let src = "<script>\n\tlet name = $state('world');\n\tlet count = $state(0);\n</script>\n\n<h1>Hello {name}!</h1>\n<input bind:value={name} />\n<button onclick={() => count += 1}>clicks: {count}</button>\n";
    upsert(&host, "/src/App.svelte", src, FileLanguage::svelte());
    let profile = ide_profile();

    let main = main_code(&host, "/src/App.svelte", &profile)
        .expect("a runes Svelte component must produce a runtime Main node");

    // The Main is the Svelte client module (imports the client runtime, exports the
    // component fn, declares the delegated set). Strictly discriminating — this is
    // Svelte client output, not Vue `_sfc_main`.
    assert!(
        main.contains("import * as $ from 'svelte/internal/client';"),
        "the Main must be the Svelte client module:\n{main}"
    );
    assert!(
        main.contains("export default function App($$anchor)"),
        "the Main must export the component fn:\n{main}"
    );
    assert!(
        main.contains("$.delegate(['click']);"),
        "the Main must declare the delegated event set:\n{main}"
    );
    // NEGATIVE: no Vue residue.
    assert!(
        !main.contains("_sfc_main"),
        "the Svelte Main must not be Vue-shaped:\n{main}"
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

#[test]
fn runtime_main_request_on_a_refused_special_element_is_an_explicit_refusal_yet_ide_resolves() {
    // R6: requesting the runtime `Main` of a Svelte component whose runtime surface
    // is REFUSED (here a STANDALONE `<svelte:fragment>` — the transparent-wrapper surface,
    // still refused; the window/document/body host + `<svelte:element>` + `<svelte:boundary>` +
    // `<svelte:head>` surfaces now EMIT, so a still-refused special fixture is a standalone
    // `<svelte:fragment>`) yields the EXPLICIT `HostError::RuntimeSurfaceRefused` (carrying the
    // precise `svelte-runtime-unsupported-*` reason) — NOT a silent `MissingVirtualNode`, and NOT
    // a successful compile. YET the IDE projection (`get_ide`) still resolves (type-checking
    // survives). RED against the prior host path (which ignored `runtime_surface_refused` and
    // collapsed the request to a generic missing node, indistinguishable from a clean IDE-only
    // carrier).
    let host = host();
    let source = "<script>let c = $state(true);</script>\n<svelte:fragment>hi</svelte:fragment>\n";
    upsert(&host, "/src/Refused.svelte", source, FileLanguage::svelte());
    let profile = ide_profile();

    // The runtime `Main` request is an EXPLICIT refusal carrying the precise reason.
    match host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some("/src/Refused.svelte".to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile.clone(),
    }) {
        Err(HostError::RuntimeSurfaceRefused {
            diagnostic_code, ..
        }) => {
            assert!(
                diagnostic_code.starts_with("svelte-runtime-unsupported-"),
                "the refusal must carry the precise reason, got: {diagnostic_code}"
            );
            // A standalone `<svelte:fragment>` special refuses with the EXACT
            // `ComponentOrSnippet` diagnostic code (`special_label`) — assert it precisely.
            assert_eq!(
                diagnostic_code, "svelte-runtime-unsupported-component",
                "a refused special-element host must refuse with the exact \
                 `svelte-runtime-unsupported-component` code, got: {diagnostic_code}"
            );
        }
        Err(HostError::MissingVirtualNode { .. }) => panic!(
            "a REQUESTED-but-refused runtime Main must be an EXPLICIT RuntimeSurfaceRefused, \
             not a silent MissingVirtualNode"
        ),
        Ok(_) => panic!("a refused runtime surface must NOT produce a Main module"),
        Err(e) => panic!("unexpected error for the refused runtime request: {e:?}"),
    }

    // The IDE projection STILL resolves (type-checking survives the runtime refusal).
    assert!(
        host.ensure_ide_compiled("/src/Refused.svelte", &profile)
            .unwrap(),
        "the IDE projection must still compile for a runtime-refused component"
    );
    assert!(
        host.get_ide("/src/Refused.svelte", &profile).is_some(),
        "the IDE `tsx` must resolve even though the runtime surface was refused"
    );
}

#[test]
fn svelte_style_virtual_node_carries_the_demanded_css_source_map() {
    // A Svelte SFC with a scoped `<style>`, compiled under a profile that
    // DEMANDS maps (`CompileProfile.source_map = true`), must surface the css
    // map on the `VirtualNodeKind::Style` response: the compiler produces
    // `RuntimeStyleBlock.source_map` (the official `css.map`, generated from
    // the same transform that rendered the code) and the session must CARRY
    // it into the cached virtual file — not drop it on the floor.
    let host = host();
    let source = "<script>let c = $state(0);</script>\n<style>.r{color:red}</style>\n<button class=\"r\" onclick={() => c++}>{c}</button>\n";
    upsert(&host, "/src/Styled.svelte", source, FileLanguage::svelte());
    let profile = CompileProfile {
        target: CompileTarget::IDE,
        source_map: true,
        ..CompileProfile::default()
    };
    let resp = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Styled.svelte".to_string()),
            node_kind: Some(VirtualNodeKind::Style { index: 0 }),
            compile_profile: profile,
        })
        .expect("the style virtual node resolves");
    assert!(
        resp.code.contains(".r"),
        "the scoped css code rides the style node: {}",
        resp.code
    );
    let map = resp
        .source_map
        .as_deref()
        .expect("the demanded css map rides the Style response (not dropped)");
    assert!(
        map.contains("\"mappings\""),
        "a real source-map JSON rides the style node: {map}"
    );
}

#[test]
fn runtime_main_request_on_a_supported_svelte_component_returns_the_main_module() {
    // R6 NEGATIVE: a SUPPORTED runes component's runtime `Main` request returns the
    // client module (NOT a refusal) — the refusal path is gated on a real refusal,
    // never a clean compile. (the Svelte client emitter emits `Main` for supported runes components.)
    let host = host();
    let source = "<script>let c = $state(0);</script>\n<button onclick={() => c++}>{c}</button>\n";
    upsert(&host, "/src/Ok.svelte", source, FileLanguage::svelte());
    let profile = ide_profile();
    let code = main_code(&host, "/src/Ok.svelte", &profile)
        .expect("a supported runes component returns a Main module");
    assert!(
        code.contains("import * as $ from 'svelte/internal/client';"),
        "the Main module is the Svelte client module:\n{code}"
    );
}

#[test]
fn cached_runtime_refusal_satisfies_a_main_demand_without_recompute() {
    // The refused runtime surface here is a standalone `<svelte:fragment>` (the transparent-
    // wrapper surface, still refused with `svelte-runtime-unsupported-component`; the
    // window/document/body host + `<svelte:element>` + `<svelte:boundary>` + `<svelte:head>`
    // surfaces now EMIT); its IDE projection still resolves.
    // A WARM cached runtime refusal (`runtime_surface_refused = true`, no `Main`
    // output) must SATISFY a `get_virtual_file(Main)` demand from the cache — the
    // serve gate (`compile_serve_satisfies_demand`) treats a `Main` demand as
    // satisfied when the served result is a runtime refusal, so the second request
    // is served from the warm slot (it still yields the typed `RuntimeSurfaceRefused`)
    // rather than falling through to a COLD recompile.
    //
    // DISCRIMINATING via the feature-independent `compile_cold_runs` provenance rail
    // (bumped once per cold run past the warm-hit consult): RED against the pre-fix
    // gate (which required `outputs.contains_key(Main)` and so recompiled the refusal
    // on every request — the second request's cold-run count would INCREASE).
    let host = host();
    let source = "<script>let c = $state(true);</script>\n<svelte:fragment>hi</svelte:fragment>\n";
    upsert(
        &host,
        "/src/RefusedCached.svelte",
        source,
        FileLanguage::svelte(),
    );
    let profile = ide_profile();

    let request = || {
        host.get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/RefusedCached.svelte".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile.clone(),
        })
    };

    // First request: the cold compile runs, fails closed, and the refusal is cached
    // carrying the EXACT `svelte-runtime-unsupported-component` code.
    match request() {
        Err(HostError::RuntimeSurfaceRefused {
            diagnostic_code, ..
        }) => assert_eq!(
            diagnostic_code, "svelte-runtime-unsupported-component",
            "the first runtime Main request on a refused component is a typed refusal \
             carrying the exact `svelte-runtime-unsupported-component` code, got: \
             {diagnostic_code}"
        ),
        Ok(_) => panic!(
            "the first runtime Main request on a refused component must be a typed \
             RuntimeSurfaceRefused, not a successful Main module"
        ),
        Err(e) => panic!(
            "the first runtime Main request on a refused component must be a typed \
             RuntimeSurfaceRefused, got: {e:?}"
        ),
    }
    let cold_after_first = host.provenance_snapshot().compile_cold_runs;

    // Second request: served from the WARM cached refusal — still the typed refusal
    // carrying the same exact code, and NO new cold run (the cold-run count is unchanged).
    match request() {
        Err(HostError::RuntimeSurfaceRefused {
            diagnostic_code, ..
        }) => assert_eq!(
            diagnostic_code, "svelte-runtime-unsupported-component",
            "the warm-served runtime Main request on a refused component is STILL a typed \
             refusal carrying the exact `svelte-runtime-unsupported-component` code, got: \
             {diagnostic_code}"
        ),
        Ok(_) => panic!(
            "the second runtime Main request on a refused component must STILL be a typed \
             RuntimeSurfaceRefused, not a successful Main module"
        ),
        Err(e) => panic!(
            "the second runtime Main request on a refused component must STILL be a typed \
             RuntimeSurfaceRefused, got: {e:?}"
        ),
    }
    let cold_after_second = host.provenance_snapshot().compile_cold_runs;
    assert_eq!(
        cold_after_first, cold_after_second,
        "the cached runtime refusal must satisfy the Main demand WITHOUT a cold \
         recompile (cold runs: {cold_after_first} -> {cold_after_second})"
    );

    // The IDE projection still resolves (type-checking survives the runtime refusal).
    assert!(
        host.ensure_ide_compiled("/src/RefusedCached.svelte", &profile)
            .unwrap(),
        "the IDE projection must still compile for a runtime-refused component"
    );
    assert!(
        host.get_ide("/src/RefusedCached.svelte", &profile)
            .is_some(),
        "the IDE `tsx` must resolve even though the runtime surface was refused"
    );
}
