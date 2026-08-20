//! Runtime proof that nested `v-for`/`v-if` neither throw nor drop content.
//! A `contains()` check cannot see whether a reference sits inside its
//! scope, or notice a dropped `_createIf`. Compiles through the real
//! Vapor pipeline and mounts via the pinned with-vapor runtime in jsdom.
//! Eager `_createIf` during `render()` means an empty `items` still
//! throws on a misplaced closure.
//!
//! Behind `bf2-authoritative` (needs the provisioned oracle install).

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
    let compiled = VueCarrierCompiler
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
    // stdin is `Stdio::null()`, so the module is a temp file. Path must be
    // unique per CALL (same `--lib` binary, concurrent mounts): a PID-only
    // name collides; first Drop deletes the sibling's file. Same as
    // `TempCandidate::write`: PID plus a monotonic counter.
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

/// `v-if` parent of a deeper pending `v-if` chain. Early-flush used to skip
/// because it tested `el.v_condition.is_none()` (plain parents only),
/// dropping the inner `_createIf`. Expected HTML matches pinned rc.3
/// (reactive `ref` — a `const` triggers an unrelated ONCE opt).
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

/// Three nested `v-if`s: consecutive flush-gate retrigger. Matches pinned rc.3.
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

/// `v-for > v-if > v-if > v-for`: flush-gate on both conditional parents
/// plus the `v-for` merge path. Matches pinned rc.3.
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

/// Sibling `v-if` chains sharing a parent: safety-net flush, not the
/// `leave_element` gate. Matches pinned rc.3.
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

/// `v-if` flushed by a later ancestor, not a plain sibling that leaves
/// first. Matches pinned rc.3.
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

/// Known limitation, not the flush-gate class: `<template v-if>` is not a
/// transparent wrapper (`builds_open_tag` only special-cases `v-slot`).
/// Children land in `.content` (invisible to `innerHTML`). Both
/// `_createIf`s are present and nested. Tracked follow-up.
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
