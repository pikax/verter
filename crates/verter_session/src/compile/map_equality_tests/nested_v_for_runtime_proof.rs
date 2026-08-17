//! Genuine runtime-execution proof that nested `v-for`/`v-if` structural
//! directives — a source-rewrite regression across `v-for`/`v-if`/`v-for`,
//! and a flush-ordering regression across nested/sibling `v-if` chains —
//! neither throw nor silently drop content.
//!
//! A `contains()` check on the generated string cannot see WHERE a matched
//! substring sits relative to the scope that defines it — a raw
//! `_for_item0.value.tags` reference is byte-identical whether it sits
//! inside `(_for_item0) => { ... }` (correct) or immediately outside it (a
//! `ReferenceError`) — and it cannot see an ABSENT construct at all: a
//! dropped inner `_createIf` leaves the string simply shorter, which no
//! substring check flags. This module compiles the exact fixture through
//! the real Vapor pipeline, assembles a full importable module, and MOUNTS
//! it through the pinned official with-vapor runtime in jsdom — the same
//! mechanism `packages/framework-conformance-harness/src/execute-vue-vapor.mjs`
//! provides for the BF2 runtime axis — so a scope defect fails as an actual
//! thrown error and a dropped construct fails as missing rendered content,
//! neither just a string match.
//!
//! A `<div>` root's own `_createFor`/`_createIf` is invoked EAGERLY during
//! `render()` regardless of whether the outer loop's own list is empty: a
//! misplaced `() => (_for_item0.value.show)` closure throws the moment
//! `_createIf` calls it to decide the initial branch, before any item is
//! ever iterated — so this proof does not depend on `items` being
//! non-empty to be meaningful.
//!
//! Gated behind `bf2-authoritative` like its siblings: it needs the same
//! provisioned oracle install (`ensureOracleDomain("vue")`) that a fresh
//! checkout does not have.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use oxc_allocator::Allocator;
use serde_json::Value;
use verter_compiler::framework_common::vue_bridge::VueCarrierCompiler;
use verter_compiler::framework_common::{CarrierCompiler, RuntimeCompileOptions};

use super::bf2_seed_matrix::run_bounded;
use super::{assemble_vue_main_module, CompileProfile, HmrStrategy};

/// Compile an INLINE `.vue` source (not a fixture file) through the real
/// Vue carrier and assemble a full runnable module — the same pipeline
/// [`super::compile_fixture`] drives, minus the file read, since this
/// proof's fixture is authored inline for readability next to the
/// assertion it backs.
fn assemble_inline_vapor_module(source: &str) -> String {
    let canonical_id = "fixtures/vue/nested-v-for-runtime-proof.vue".to_string();
    let provenance = crate::types::MetaProvenance::default();
    let (snapshot, artifact) = crate::parse::parse_vue_snapshot(
        &canonical_id,
        source,
        verter_semantic::analysis::AnalysisScope::LSP,
        &provenance,
    );

    let allocator = Allocator::new();
    let compiled = VueCarrierCompiler::default()
        .compile_bundle(
            source,
            &artifact,
            &RuntimeCompileOptions {
                filename: Some(canonical_id.clone()),
                source_map: false,
                ssr: false,
                is_production: false,
                inline: Some(false),
                force_js: true,
                force_vapor: true,
                ..RuntimeCompileOptions::default()
            },
            &allocator,
        )
        .expect("the Vue carrier produces a runtime bundle for this fixture")
        .into_produced()
        .expect("the Vue carrier produces a runtime surface; it never refuses one");

    let profile = CompileProfile {
        filename: Some(canonical_id.clone()),
        is_production: false,
        ssr: false,
        source_map: false,
        inline: Some(false),
        force_vapor: true,
        hmr_strategy: HmrStrategy::None,
        ..CompileProfile::default()
    };

    assemble_vue_main_module(&canonical_id, &compiled, &snapshot.meta, &profile)
        .unwrap_or_else(|failure| panic!("the assembler failed closed: {failure:?}"))
        .code
}

fn harness_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/framework-conformance-harness")
}

/// Mount `module_code` through the pinned official with-vapor runtime in
/// jsdom via a Node subprocess, returning `(ok, detail)`. `detail` is the
/// thrown error's message when `!ok`, else the mounted container's
/// `innerHTML`.
fn execute_through_official_vapor_runtime(module_code: &str) -> (bool, String) {
    // `run_bounded` always sets the child's stdin to `Stdio::null()` (its
    // sibling callers never need to feed a child anything), so the module
    // code is handed over via a temp file path argument instead of a pipe.
    //
    // The path must be unique per CALL, not merely per process: this
    // module's own tests all run inside the SAME `verter_session --lib`
    // test binary, so under ordinary multi-threaded test execution several
    // of them mount concurrently — a PID-only name collides across all of
    // them, and whichever call's `Drop`/cleanup runs first deletes the file
    // out from under a still-reading sibling (`ENOENT`) or silently swaps
    // in another call's module code. Same fix as `TempCandidate::write` in
    // `bf2_seed_matrix.rs`: PID plus a per-process monotonic counter.
    static CALL_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temp = std::env::temp_dir().join(format!(
        "verter-nested-vfor-runtime-proof-{}-{}.vue.mjs",
        std::process::id(),
        CALL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    ));
    std::fs::write(&temp, module_code).expect("write module code to a temp file");
    let temp_path = temp.to_string_lossy().into_owned();

    let script = r#"
import { readFileSync } from "node:fs";
import { executeVueVaporInterop, ensureVaporRuntimePreloaded, cleanupScratch } from "./src/execute-vue-vapor.mjs";
const moduleCode = readFileSync(process.argv[1], "utf8");
await ensureVaporRuntimePreloaded();
const result = await executeVueVaporInterop(moduleCode);
cleanupScratch();
process.stdout.write(JSON.stringify({ ok: result.ok, detail: result.ok ? (result.html ?? "") : (result.error ?? "unknown error") }));
"#;
    let mut command = Command::new("node");
    command
        .arg("--input-type=module")
        .arg("-e")
        .arg(script)
        .arg("--")
        .arg(&temp_path)
        .current_dir(harness_root());

    let finished = run_bounded(&mut command, Duration::from_secs(60));
    let _ = std::fs::remove_file(&temp);

    assert!(
        !finished.timed_out,
        "the vapor runtime mount did not finish within 60s — it was killed.\nstderr:\n{}",
        finished.stderr
    );
    assert_eq!(
        finished.code,
        Some(0),
        "the Node harness itself failed (not the mount under test).\nstdout:\n{}\nstderr:\n{}",
        finished.stdout,
        finished.stderr
    );
    let report: Value = serde_json::from_str(&finished.stdout).unwrap_or_else(|error| {
        panic!(
            "the mount harness emitted no JSON report ({error}).\nstdout:\n{}\nstderr:\n{}",
            finished.stdout, finished.stderr
        )
    });
    let ok = report
        .get("ok")
        .and_then(Value::as_bool)
        .expect("report carries `ok`");
    let detail = report
        .get("detail")
        .and_then(Value::as_str)
        .unwrap_or("<absent>")
        .to_string();
    (ok, detail)
}

/// The exact regression shape: `v-for` > `v-if` > `v-for`, where the inner
/// `v-for`'s source references the OUTER loop's own variable through a
/// member-access chain. Mounts genuinely and must not throw.
#[test]
fn nested_v_for_source_rewrite_module_mounts_without_reference_error() {
    let source = "<template><div><li v-for=\"item in items\"><p v-if=\"item.show\">\
                  <span v-for=\"tag in item.tags\">{{ tag }}</span></p></li></div></template>\
                  <script setup>const items = [{ show: true, tags: ['a', 'b'] }];</script>";
    let module_code = assemble_inline_vapor_module(source);
    assert!(
        module_code.contains("_for_item0.value.tags"),
        "sanity: the fixture must still exercise the rewrite this proof backs, got:\n{module_code}"
    );

    let (ok, detail) = execute_through_official_vapor_runtime(&module_code);
    assert!(
        ok,
        "the compiled module threw when mounted through the real with-vapor runtime — \
         a scope defect a `contains()` check cannot see. Error:\n{detail}\n\nmodule:\n{module_code}"
    );
    assert!(
        detail.contains("<li"),
        "the mount succeeded but rendered no <li> content — items was non-empty, so an \
         empty render would itself indicate a defect, got:\n{detail}"
    );
}

/// The primary regression shape for the flush-gate fix: a `v-if` element
/// (`<section v-if="outer">`) that is ITSELF the recorded DOM parent of a
/// DIFFERENT, deeper pending `v-if` chain (`<p v-if="inner">`, its only
/// child). Before the fix, `leave_element`'s early-flush gate skipped this
/// case because it tested `el.v_condition.is_none()` — true only for a
/// PLAIN parent — so a `v-if` parent's own state popped before the child
/// chain flushed, and `merge_into_stack_index` silently no-op'd against the
/// now-stale index: the inner `_createIf` and its `<p>` were dropped from
/// the generated module entirely, with no error. Expected HTML below is
/// confirmed byte-identical to the pinned rc.3 compiler's own mount of the
/// same fixture (reactive `ref` bindings — a literal `const` binding
/// triggers an unrelated ONCE-optimization that changes the skeleton).
#[test]
fn v_if_nested_in_v_if_mounts_and_renders_inner_content() {
    let source = "<template><div><section v-if=\"outer\"><p v-if=\"inner\">x</p></section></div></template>\
                  <script setup>import { ref } from 'vue'; const outer = ref(true); const inner = ref(true);</script>";
    let module_code = assemble_inline_vapor_module(source);
    assert!(
        module_code.matches("_createIf(").count() == 2,
        "sanity: both the outer and inner _createIf must be present in the generated module, got:\n{module_code}"
    );

    let (ok, detail) = execute_through_official_vapor_runtime(&module_code);
    assert!(
        ok,
        "the compiled module threw when mounted through the real with-vapor runtime. Error:\n{detail}\n\nmodule:\n{module_code}"
    );
    assert_eq!(
        detail, "<div><section><p>x</p><!--if--></section><!--if--></div>",
        "mounted HTML must match the pinned rc.3 compiler's own mount of this fixture exactly \
         (confirmed independently against the real oracle), got:\n{detail}\n\nmodule:\n{module_code}"
    );
}

/// THREE v-if elements nested in a row (`section > article > p`, each
/// carrying its own `v-if`): every intermediate element must flush its
/// deeper child's chain via the SAME index comparison, back to back. A
/// two-level chain alone cannot show the gate re-triggering correctly at
/// consecutive levels — this is the direct adversarial generalization
/// check. Confirmed byte-identical to the pinned rc.3 compiler's own mount.
#[test]
fn triple_nested_v_if_mounts_and_renders_deepest_content() {
    let source = "<template><div><section v-if=\"a\"><article v-if=\"b\"><p v-if=\"c\">deep</p></article></section></div></template>\
                  <script setup>import { ref } from 'vue'; const a = ref(true); const b = ref(true); const c = ref(true);</script>";
    let module_code = assemble_inline_vapor_module(source);
    assert!(
        module_code.matches("_createIf(").count() == 3,
        "sanity: all three nested _createIf calls must be present, got:\n{module_code}"
    );

    let (ok, detail) = execute_through_official_vapor_runtime(&module_code);
    assert!(
        ok,
        "the compiled module threw when mounted through the real with-vapor runtime. Error:\n{detail}\n\nmodule:\n{module_code}"
    );
    assert_eq!(
        detail, "<div><section><article><p>deep</p><!--if--></article><!--if--></section><!--if--></div>",
        "mounted HTML must match the pinned rc.3 compiler's own mount of this fixture exactly \
         (confirmed independently against the real oracle), got:\n{detail}\n\nmodule:\n{module_code}"
    );
}

/// A generalization one level further than [`v_if_nested_in_v_if_mounts_and_renders_inner_content`]:
/// `v-for > v-if > v-if > v-for`, where the flush-gate must fire for BOTH
/// conditional parents (`<p v-if="item.show">` flushing the `<section
/// v-if="item.deep">` chain, and — via the `v-for`-own merge path (a
/// separate, direct merge that never goes through `pending_vif_chain` at
/// all) — the outer `<li v-for>` flushing the `<p>` chain) without dropping
/// the innermost `v-for`'s own content either. Expected HTML confirmed
/// byte-identical to the pinned rc.3 compiler.
#[test]
fn v_for_v_if_v_if_v_for_mounts_and_renders_innermost_content() {
    let source = "<template><div><li v-for=\"item in items\"><p v-if=\"item.show\">\
                  <section v-if=\"item.deep\"><span v-for=\"tag in item.tags\">{{ tag }}</span></section>\
                  </p></li></div></template>\
                  <script setup>import { ref } from 'vue'; \
                  const items = ref([{ show: true, deep: true, tags: ['a', 'b'] }]);</script>";
    let module_code = assemble_inline_vapor_module(source);

    let (ok, detail) = execute_through_official_vapor_runtime(&module_code);
    assert!(
        ok,
        "the compiled module threw when mounted through the real with-vapor runtime. Error:\n{detail}\n\nmodule:\n{module_code}"
    );
    assert_eq!(
        detail,
        "<div><li><p><section><span>a</span><span>b</span><!--for--></section><!--if--></p><!--if--></li><!--for--></div>",
        "mounted HTML must match the pinned rc.3 compiler's own mount of this fixture exactly \
         (confirmed independently against the real oracle), got:\n{detail}\n\nmodule:\n{module_code}"
    );
}

/// A `v-if` chain PRECEDED by an unrelated, already-flushed independent
/// `v-if` chain as its sibling — the case `handle_v_if_chain`'s own `If`
/// arm safety-net flush (not the `leave_element` top-of-function gate this
/// fix changed) must still cover: two SEPARATE single-branch `v-if`
/// elements sharing one parent, neither continuing the other's chain.
/// Confirmed byte-identical to the pinned rc.3 compiler's own mount.
#[test]
fn independent_sibling_v_if_chains_both_mount_correctly() {
    let source = "<template><div><p v-if=\"a\">A</p><span v-if=\"b\">B</span></div></template>\
                  <script setup>import { ref } from 'vue'; const a = ref(true); const b = ref(true);</script>";
    let module_code = assemble_inline_vapor_module(source);

    let (ok, detail) = execute_through_official_vapor_runtime(&module_code);
    assert!(
        ok,
        "the compiled module threw when mounted through the real with-vapor runtime. Error:\n{detail}\n\nmodule:\n{module_code}"
    );
    assert!(
        detail.contains("<p>A</p>") && detail.contains("<span>B</span>"),
        "both independent sibling v-if branches must render, got:\n{detail}\n\nmodule:\n{module_code}"
    );
}

/// `v-if` flushed by a NON-IMMEDIATE later ancestor: the chain's own
/// structural parent (`<li>`) has a later PLAIN sibling (`<footer>`) that
/// leaves before `<li>` itself does. The chain must stay pending across
/// that plain sibling's own `leave_element` (its own index never matches
/// the chain's recorded `target_stack_index`) and flush only once `<li>`
/// itself is reached. Confirmed byte-identical to the pinned rc.3 compiler.
#[test]
fn v_if_flushed_by_later_plain_sibling_of_a_non_immediate_ancestor_mounts_correctly() {
    let source =
        "<template><div><li><p v-if=\"show\">A</p><footer>after</footer></li></div></template>\
                  <script setup>import { ref } from 'vue'; const show = ref(true);</script>";
    let module_code = assemble_inline_vapor_module(source);

    let (ok, detail) = execute_through_official_vapor_runtime(&module_code);
    assert!(
        ok,
        "the compiled module threw when mounted through the real with-vapor runtime. Error:\n{detail}\n\nmodule:\n{module_code}"
    );
    assert!(
        detail.contains("<p>A</p>") && detail.contains("<footer>after</footer>"),
        "both the v-if branch and the following plain sibling must render, got:\n{detail}\n\nmodule:\n{module_code}"
    );
}

/// **Known, confirmed, standing limitation, structurally distinct from the
/// flush-gate class this module's other tests cover.** `<template v-if>` is
/// a virtual/fragment wrapper with no real DOM node in every official form
/// (`v-slot`, `v-if`, `v-for`, bare); only `<template v-slot>` is currently
/// treated that way in `enter_element`'s `builds_open_tag` — a `<template
/// v-if>` still gets its own literal `_template("<template>")` hoist and
/// `_setInsertionState` call. Because a real `<template>` DOM element's
/// children live in `.content` (a detached `DocumentFragment`, invisible to
/// `innerHTML`), the inner `_createIf`'s content is inserted somewhere
/// structurally inert — reproduced here as a genuine runtime mount, not a
/// `contains()` guess. The two `_createIf` calls ARE both present and
/// correctly nested in the generated module (confirmed independently) —
/// this is purely the transparent-wrapper root-element question, not a
/// flush-ordering defect. Closing it requires `build_closure_body`/the v-if
/// branch root-element machinery to recognize a `<template>`-wrapped branch
/// body as OWNING NO template of its own, forwarding its single child's
/// construct directly — a materially larger change than a flush-gate
/// correction. Tracked as confirmed follow-up work, not silently dropped.
#[test]
#[ignore = "confirmed separate defect: <template v-if> is not a transparent \
            root element in Vapor codegen (needs build_closure_body support \
            for a template-wrapped branch with no DOM footprint of its \
            own) — distinct from, and not fixed by, the leave_element \
            flush-gate correction the rest of this module covers"]
fn template_v_if_wrapping_inner_v_if_mounts_and_renders_inner_content() {
    let source = "<template><div><template v-if=\"outer\"><p v-if=\"inner\">x</p></template></div></template>\
                  <script setup>import { ref } from 'vue'; const outer = ref(true); const inner = ref(true);</script>";
    let module_code = assemble_inline_vapor_module(source);

    let (ok, detail) = execute_through_official_vapor_runtime(&module_code);
    assert!(
        ok,
        "the compiled module threw when mounted through the real with-vapor runtime. Error:\n{detail}\n\nmodule:\n{module_code}"
    );
    assert_eq!(
        detail, "<div><p>x</p></div>",
        "mounted HTML must match the pinned rc.3 compiler's own mount of this fixture exactly \
         (confirmed independently against the real oracle), got:\n{detail}\n\nmodule:\n{module_code}"
    );
}
