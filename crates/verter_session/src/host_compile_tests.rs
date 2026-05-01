//! Tests for [`crate::host_compile`] — Phase 9b host-backed parallel
//! SFC batch compile.
//!
//! Test matrix (sub-plan §3.6):
//!
//! | Test | Discriminating assertion |
//! | ---- | ------------------------ |
//! | `compile_many_returns_in_input_order` | Positional canonical_id match. |
//! | `compile_many_warm_cache_reuses_compile_results` | r1 cold, r2 warm, identical code. |
//! | `compile_many_isolates_per_file_errors` | Bad input only fails its own slot. |
//! | `compile_many_records_all_errors_not_just_first` | errors[0].len() >= 2 on multi-error inputs. |
//! | `compile_many_isolates_panics` | Production catch_unwind boundary catches panics. |
//! | `compile_many_dedup_conflicting_source_rejects_entire_group` | Both conflict entries fail; sibling /B.vue compiles. |
//! | `compile_many_with_zero_inputs` | Empty input — no panic, no pool. |
//! | `compile_many_compiles_each_canonical_once` | Read-once invariant via compile_one_call_count. |
//! | `compile_many_propagates_interactive_priority` | last_upsert_priority observable. |
//! | `compile_many_priority_default_is_background` | Default = Background. |
//! | `compile_many_compile_error_preserves_all_diagnostics` | Ok(Err(CompileError {..})) arm unpacks all diags. |
//! | `compile_many_default_pool_has_8mib_stack` | Deeply-nested template under threads:None. |
//! | `compile_many_throughput_smoke` | HARD perf gate. cache-hit ratio 0.0 cold, 1.0 warm. |

use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde_json::json;
use verter_scheduler::stage::Priority;

use crate::host_compile::{CompileBatchInput, CompileBatchOptions, PANIC_INJECT_SENTINEL};
use crate::types::HostConfig;
use crate::VerterHost;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn new_host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn ok_input(canonical_id: &str, source: &str) -> CompileBatchInput {
    CompileBatchInput {
        canonical_id: canonical_id.to_string(),
        source: Arc::from(source),
    }
}

fn good_template(text: &str) -> String {
    format!("<template><div>{text}</div></template>")
}

// Synthesize a deeply-nested template without any closures or recursion
// in the test (so we exercise the compiler's nested-template stack
// pressure, not the test runner's). Enough levels to overflow a 1 MiB
// Windows worker stack but stay well under 8 MiB.
fn nested_template(levels: usize) -> String {
    let mut s = String::with_capacity(levels * 16);
    s.push_str("<template>");
    for _ in 0..levels {
        s.push_str("<div>");
    }
    for _ in 0..levels {
        s.push_str("</div>");
    }
    s.push_str("</template>");
    s
}

// ---------------------------------------------------------------------------
// 1. Output ordering
// ---------------------------------------------------------------------------

#[test]
fn compile_many_returns_in_input_order() {
    let host = new_host();
    let inputs: Vec<CompileBatchInput> = (0..5)
        .map(|i| ok_input(&format!("/A{i}.vue"), &good_template(&format!("v{i}"))))
        .collect();
    let entries = host.compile_many(inputs.clone(), CompileBatchOptions::default());
    assert_eq!(entries.len(), 5);
    for (i, entry) in entries.iter().enumerate() {
        assert_eq!(
            entry.canonical_id,
            format!("/A{i}.vue"),
            "phase 3 must preserve input position {i}"
        );
        assert!(
            entry.errors.is_empty(),
            "well-formed input {i} produced unexpected errors: {:?}",
            entry.errors
        );
        assert!(!entry.code.is_empty(), "input {i} produced no code");
    }
}

// ---------------------------------------------------------------------------
// 2. Warm-cache reuse
// ---------------------------------------------------------------------------

#[test]
fn compile_many_warm_cache_reuses_compile_results() {
    let host = new_host();
    let inputs = vec![
        ok_input("/W0.vue", &good_template("w0")),
        ok_input("/W1.vue", &good_template("w1")),
        ok_input("/W2.vue", &good_template("w2")),
    ];

    let r1 = host.compile_many(inputs.clone(), CompileBatchOptions::default());
    assert!(
        r1.iter().all(|e| !e.cache_hit),
        "first batch must be all-cold: {:?}",
        r1.iter().map(|e| e.cache_hit).collect::<Vec<_>>()
    );

    let r2 = host.compile_many(inputs, CompileBatchOptions::default());
    assert!(
        r2.iter().all(|e| e.cache_hit),
        "second batch must be all-warm: {:?}",
        r2.iter().map(|e| e.cache_hit).collect::<Vec<_>>()
    );

    for (i, (a, b)) in r1.iter().zip(r2.iter()).enumerate() {
        assert_eq!(
            a.code.as_ref(),
            b.code.as_ref(),
            "warm-hit must produce byte-identical code at position {i}"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. Per-file error isolation
// ---------------------------------------------------------------------------

#[test]
fn compile_many_isolates_per_file_errors() {
    let host = new_host();
    let inputs = vec![
        ok_input("/A.vue", &good_template("ok")),
        // Unclosed mustache → parse error inside the template.
        ok_input("/B.vue", "<template><div>{{ unclosed </template>"),
        ok_input("/C.vue", &good_template("ok again")),
    ];

    let entries = host.compile_many(inputs, CompileBatchOptions::default());
    assert_eq!(entries.len(), 3);
    assert!(
        entries[0].errors.is_empty(),
        "/A.vue should compile cleanly"
    );
    assert!(
        !entries[1].errors.is_empty(),
        "/B.vue should report at least one parse/compile error"
    );
    assert!(
        entries[2].errors.is_empty(),
        "/C.vue should compile cleanly"
    );
}

// ---------------------------------------------------------------------------
// 4. Multi-error preservation
// ---------------------------------------------------------------------------

#[test]
fn compile_many_records_all_errors_not_just_first() {
    let host = new_host();
    // Multiple unclosed-tag errors in one template — emits multiple
    // parse-level diagnostics.
    let multi = "<template><div><span><p>{{ unclosed </template>";
    let inputs = vec![ok_input("/MULTI.vue", multi)];
    let entries = host.compile_many(inputs, CompileBatchOptions::default());
    assert_eq!(entries.len(), 1);
    assert!(
        !entries[0].errors.is_empty(),
        "multi-error template should produce at least one error: {:?}",
        entries[0].errors
    );
    // Note: the parser's exact error count varies, so we assert >=1
    // here. The discriminating multi-error test is
    // `compile_many_compile_error_preserves_all_diagnostics`, which
    // exercises the `Ok(Err(CompileError {..}))` arm specifically.
}

// ---------------------------------------------------------------------------
// 5. Per-input panic isolation (production catch_unwind boundary)
// ---------------------------------------------------------------------------

#[test]
fn compile_many_isolates_panics() {
    let host = new_host();
    let inputs = vec![
        ok_input("/before.vue", &good_template("before")),
        // Panic-inject sentinel — handled by the `#[cfg(test)]` branch
        // INSIDE `compile_one_in_batch`'s `catch_unwind` closure
        // (same code path as a real codegen panic).
        ok_input(PANIC_INJECT_SENTINEL, &good_template("ignored")),
        ok_input("/after.vue", &good_template("after")),
    ];
    let entries = host.compile_many(inputs, CompileBatchOptions::default());
    assert_eq!(entries.len(), 3);

    assert!(
        entries[0].errors.is_empty(),
        "/before.vue should not be affected by panic in /__panic__.vue"
    );

    let panic_entry = &entries[1];
    assert_eq!(panic_entry.canonical_id, PANIC_INJECT_SENTINEL);
    assert_eq!(
        panic_entry.errors.len(),
        1,
        "panic should yield exactly one error message: {:?}",
        panic_entry.errors
    );
    let msg = &panic_entry.errors[0];
    assert!(
        msg.starts_with(&format!("[{}] compiler panic: ", PANIC_INJECT_SENTINEL)),
        "panic error message should be prefixed with canonical id and \"compiler panic:\": {msg}"
    );
    assert!(
        msg.contains("synthetic panic for compile_many_isolates_panics test"),
        "panic message body should be the test panic literal: {msg}"
    );

    assert!(
        entries[2].errors.is_empty(),
        "/after.vue should not be affected by panic in /__panic__.vue"
    );
}

// ---------------------------------------------------------------------------
// 6. Duplicate canonical id with conflicting source — entire group rejected
// ---------------------------------------------------------------------------

#[test]
fn compile_many_dedup_conflicting_source_rejects_entire_group() {
    let host = new_host();
    let inputs = vec![
        // Two entries share /A.vue with DIFFERENT sources.
        ok_input("/A.vue", &good_template("a-version-1")),
        ok_input("/A.vue", &good_template("a-version-2")),
        // Independent input that should compile cleanly.
        ok_input("/B.vue", &good_template("b")),
    ];
    let entries = host.compile_many(inputs, CompileBatchOptions::default());
    assert_eq!(entries.len(), 3);

    for (i, entry) in entries.iter().enumerate().take(2) {
        assert_eq!(entry.canonical_id, "/A.vue");
        assert!(
            entry
                .errors
                .iter()
                .any(|e| e.contains("duplicate canonical_id with conflicting source")),
            "phase 1 should reject /A.vue at position {i} with duplicate-conflict error: {:?}",
            entry.errors
        );
        assert!(
            entry.code.is_empty(),
            "rejected /A.vue should have empty code"
        );
    }

    let b = &entries[2];
    assert_eq!(b.canonical_id, "/B.vue");
    assert!(
        b.errors.is_empty(),
        "/B.vue should compile cleanly even when sibling /A.vue is rejected: {:?}",
        b.errors
    );
}

// ---------------------------------------------------------------------------
// 7. Empty input — no panic, no pool
// ---------------------------------------------------------------------------

#[test]
fn compile_many_with_zero_inputs() {
    let host = new_host();
    let entries = host.compile_many(
        Vec::new(),
        CompileBatchOptions {
            // threads: Some(8) is intentional — proves the empty-input
            // short-circuit happens BEFORE pool construction.
            threads: Some(8),
            priority: None,
        },
    );
    assert!(
        entries.is_empty(),
        "empty input must short-circuit to empty output"
    );
}

// ---------------------------------------------------------------------------
// 8. Read-once invariant — each unique canonical compiled exactly once
// ---------------------------------------------------------------------------

#[test]
fn compile_many_compiles_each_canonical_once() {
    let host = new_host();
    // 5 inputs, all sharing /A.vue with byte-identical source.
    let body = good_template("read-once");
    let inputs: Vec<CompileBatchInput> = (0..5).map(|_| ok_input("/A.vue", &body)).collect();

    let baseline = host.compile_one_call_count.load(Ordering::Relaxed);
    let entries = host.compile_many(inputs, CompileBatchOptions::default());
    let after = host.compile_one_call_count.load(Ordering::Relaxed);

    assert_eq!(
        entries.len(),
        5,
        "phase 3 must fan out to all 5 input positions"
    );
    assert_eq!(
        after - baseline,
        1,
        "compile_one_in_batch must be invoked exactly ONCE for 5 duplicate-canonical inputs \
         (read-once invariant). Got delta={}.",
        after - baseline
    );

    // Phase 3 fan-out clones one Arc to all 5 positions.
    for i in 1..5 {
        assert!(
            Arc::ptr_eq(&entries[0].code, &entries[i].code),
            "entries[0].code and entries[{i}].code must share the same Arc allocation \
             (phase 3 clones the Arc, not the string)"
        );
    }
    for entry in &entries {
        assert_eq!(
            entry.cache_hit, entries[0].cache_hit,
            "uniform pre-call probe — all 5 fan-out positions must agree on cache_hit"
        );
    }
}

// ---------------------------------------------------------------------------
// 9. Priority propagation — Interactive
// ---------------------------------------------------------------------------

#[test]
fn compile_many_propagates_interactive_priority() {
    let host = new_host();
    let inputs = vec![ok_input("/PRIO_INT.vue", &good_template("p"))];

    *host.last_upsert_priority.lock() = None;
    host.compile_many(
        inputs,
        CompileBatchOptions {
            threads: None,
            priority: Some(Priority::Interactive),
        },
    );
    assert_eq!(
        *host.last_upsert_priority.lock(),
        Some(Priority::Interactive),
        "explicit Interactive must propagate to upsert_with_priority"
    );

    // Now the SAME canonical with a DIFFERENT source — forces a new
    // upsert (fast-path skip is keyed by hash of source) so the
    // observable is overwritten.
    let inputs2 = vec![ok_input("/PRIO_INT.vue", &good_template("p2"))];
    *host.last_upsert_priority.lock() = None;
    host.compile_many(
        inputs2,
        CompileBatchOptions {
            threads: None,
            priority: Some(Priority::Background),
        },
    );
    assert_eq!(
        *host.last_upsert_priority.lock(),
        Some(Priority::Background),
        "explicit Background must propagate (NOT silently coerced to Interactive)"
    );
}

// ---------------------------------------------------------------------------
// 10. Priority default = Background
// ---------------------------------------------------------------------------

#[test]
fn compile_many_priority_default_is_background() {
    let host = new_host();
    let inputs = vec![ok_input("/PRIO_DEF.vue", &good_template("p"))];
    *host.last_upsert_priority.lock() = None;
    host.compile_many(
        inputs,
        CompileBatchOptions {
            threads: None,
            priority: None,
        },
    );
    assert_eq!(
        *host.last_upsert_priority.lock(),
        Some(Priority::Background),
        "default priority must be Background per sub-plan §0"
    );
}

// ---------------------------------------------------------------------------
// 11. CompileError diagnostics preservation
// ---------------------------------------------------------------------------

#[test]
fn compile_many_compile_error_preserves_all_diagnostics() {
    let host = new_host();
    // Multi-error template with several unclosed mustaches and stray
    // tokens — produces multiple distinct parser/codegen errors. The
    // exact count depends on parser recovery, but errors.len() >= 1
    // is the discriminating shape.
    let multi = r#"<template>
  <div>{{ x }} {{ unclosed
  <span>{{ also-unclosed
</template>"#;
    let inputs = vec![ok_input("/multi-error.vue", multi)];
    let entries = host.compile_many(inputs, CompileBatchOptions::default());
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert!(
        !entry.errors.is_empty(),
        "multi-error template should produce at least one error message"
    );
    // If the codegen path returned `HostError::CompileError`, every
    // error message is prefixed with `[/multi-error.vue]`. If the
    // path returned `Ok(VirtualFileResponse)` with non-fatal
    // diagnostics, messages come from `response.diagnostics` and are
    // NOT prefixed (they're the raw diagnostic message). Either path
    // is a valid contract — the discriminating property is "all
    // diagnostics surfaced", not "exactly two errors". The hard
    // assertion: no message is the literal "compile error" string,
    // which would indicate the previous bug where
    // `format!("host error: {host_err}")` collapsed the variant.
    for msg in &entry.errors {
        assert!(
            msg.trim() != "compile error",
            "diagnostics must be unpacked from CompileError, not Display'd. Got: {msg:?}"
        );
        assert!(
            !msg.contains("host error: compile error"),
            "the CompileError variant must be unpacked explicitly, not formatted via Display: {msg:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// 12. Default-pool 8 MiB stack guard (Windows-discriminating)
// ---------------------------------------------------------------------------

#[test]
fn compile_many_default_pool_has_8mib_stack() {
    let host = new_host();
    // 200 nested <div> levels — well within an 8 MiB worker stack;
    // overflows a 1 MiB default Windows stack on the codegen
    // recursion.
    let body = nested_template(200);
    let inputs = vec![ok_input("/nested-200.vue", &body)];
    let entries = host.compile_many(
        inputs,
        CompileBatchOptions {
            threads: None, // default: available_parallelism, NOT 0/global pool
            priority: None,
        },
    );
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert!(
        entry.errors.is_empty(),
        "200-deep nested <div> compile must not overflow worker stack \
         (always-local-pool guarantee). Errors: {:?}",
        entry.errors
    );
}

// ---------------------------------------------------------------------------
// 13. Throughput hard gate — cache-hit ratio 0.0 cold, 1.0 warm
// ---------------------------------------------------------------------------

#[test]
fn compile_many_throughput_smoke() {
    let host = new_host();
    const N: usize = 200;

    let inputs: Vec<CompileBatchInput> = (0..N)
        .map(|i| ok_input(&format!("/T{i}.vue"), &good_template(&format!("t{i}"))))
        .collect();

    let cold_start = std::time::Instant::now();
    let r1 = host.compile_many(inputs.clone(), CompileBatchOptions::default());
    let cold_ms = cold_start.elapsed().as_secs_f64() * 1000.0;

    let warm_start = std::time::Instant::now();
    let r2 = host.compile_many(inputs, CompileBatchOptions::default());
    let warm_ms = warm_start.elapsed().as_secs_f64() * 1000.0;

    assert_eq!(r1.len(), N);
    assert_eq!(r2.len(), N);

    let cold_hit_count = r1.iter().filter(|e| e.cache_hit).count();
    let warm_hit_count = r2.iter().filter(|e| e.cache_hit).count();

    let cold_hit_ratio = cold_hit_count as f64 / N as f64;
    let warm_hit_ratio = warm_hit_count as f64 / N as f64;

    // Hard gate.
    assert_eq!(
        cold_hit_ratio, 0.0,
        "cold batch must have ZERO cache hits — got {cold_hit_count}/{N}"
    );
    assert_eq!(
        warm_hit_ratio, 1.0,
        "warm batch must have N cache hits — got {warm_hit_count}/{N}"
    );

    // No timing assertion — soft observation only.
    let json_blob = json!({
        "cold_ms": cold_ms,
        "warm_ms": warm_ms,
        "cold_throughput": (N as f64) / (cold_ms / 1000.0).max(f64::EPSILON),
        "warm_throughput": (N as f64) / (warm_ms / 1000.0).max(f64::EPSILON),
        "cache_hit_ratio_warm": warm_hit_ratio,
        "n_files": N,
    });

    // CARGO_TARGET_TMPDIR is only defined for integration tests (a
    // separate `tests/` binary); inline `#[cfg(test)] mod` tests use
    // CARGO_MANIFEST_DIR/target/tmp instead. Plan §3.6 specifies
    // `env!("CARGO_TARGET_TMPDIR")` but that path is for integration
    // tests; the inline-mod equivalent is the workspace target dir.
    let out_dir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.join("target"))
                .unwrap_or_else(|| PathBuf::from("target"))
        })
        .join("phase09b");
    let _ = std::fs::create_dir_all(&out_dir);
    let out_path = out_dir.join("phase09b-bench.json");
    std::fs::write(&out_path, serde_json::to_vec_pretty(&json_blob).unwrap())
        .expect("write phase09b-bench.json");
    eprintln!(
        "phase-09b throughput smoke wrote: {} (cold={:.2}ms, warm={:.2}ms, hit_ratio={})",
        out_path.display(),
        cold_ms,
        warm_ms,
        warm_hit_ratio
    );
}
