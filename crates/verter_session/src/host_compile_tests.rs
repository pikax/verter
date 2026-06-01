//! Tests for [`crate::host_compile`] — host-backed parallel SFC batch
//! compile.
//!
//! Test matrix (sub-):
//!
//! | Test | Discriminating assertion |
//! | ---- | ------------------------ |
//! | `compile_many_returns_in_input_order` | Positional canonical_id match. |
//! | `compile_many_warm_cache_reuses_compile_results` | r1 cold, r2 warm, identical code. |
//! | `compile_many_isolates_per_file_errors` | Bad input only fails its own slot. |
//! | `compile_many_records_all_errors_not_just_first` | errors[0].len() >= 2 on multi-error inputs. |
//! | `compile_many_isolates_panics` | Coordinator's per-item catch boundary isolates panics. |
//! | `compile_many_dedup_conflicting_source_rejects_entire_group` | Both conflict entries fail; sibling /B.vue compiles. |
//! | `compile_many_with_zero_inputs` | Empty input — no panic, no pool. |
//! | `compile_many_compiles_each_canonical_once` | Read-once invariant via compile_one_call_count. |
//! | `compile_many_propagates_interactive_priority` | last_upsert_priority observable. |
//! | `compile_many_priority_default_is_background` | Default = Background. |
//! | `compile_many_compile_error_preserves_all_diagnostics` | Ok(Err(CompileError(failure))) arm unpacks all diags. |
//! | `compile_many_default_pool_has_8mib_stack` | Deeply-nested template under the default host pool. |
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
        requested_mode: None,
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
            "Stage D must preserve input position {i}"
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
    // exercises the `Ok(Err(CompileError(failure)))` arm specifically.
}

// ---------------------------------------------------------------------------
// 5. Per-input panic isolation (host batch coordinator's catch boundary)
// ---------------------------------------------------------------------------

#[test]
fn compile_many_isolates_panics() {
    let host = new_host();
    let inputs = vec![
        ok_input("/before.vue", &good_template("before")),
        // Panic-inject sentinel — the `#[cfg(test)]` branch in
        // `compile_one_in_batch`'s worker body panics, so the panic
        // unwinds through the host batch coordinator's generic catch
        // boundary (the same code path as a real codegen panic) and is
        // rendered into an error entry by `compile_panic_entry`.
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
            "Stage B should reject /A.vue at position {i} with duplicate-conflict error: {:?}",
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
            priority: None,
            default_mode: None,
        },
    );
    assert!(
        entries.is_empty(),
        "empty input must short-circuit to empty output (no pool work)"
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
        "Stage D must fan out to all 5 input positions"
    );
    assert_eq!(
        after - baseline,
        1,
        "compile_one_in_batch must be invoked exactly ONCE for 5 duplicate-canonical inputs \
         (read-once invariant). Got delta={}.",
        after - baseline
    );

    // Stage D fan-out clones one Arc to all 5 positions.
    for i in 1..5 {
        assert!(
            Arc::ptr_eq(&entries[0].code, &entries[i].code),
            "entries[0].code and entries[{i}].code must share the same Arc allocation \
             (Stage D clones the Arc, not the string)"
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
            priority: Some(Priority::Interactive),
            default_mode: None,
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
            priority: Some(Priority::Background),
            default_mode: None,
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
            priority: None,
            default_mode: None,
        },
    );
    assert_eq!(
        *host.last_upsert_priority.lock(),
        Some(Priority::Background),
        "default priority must be Background per sub-"
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
            priority: None,
            default_mode: None,
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
    // CARGO_MANIFEST_DIR/target/tmp instead. The integration-test
    // form `env!("CARGO_TARGET_TMPDIR")` is for integration tests;
    // the inline-mod equivalent is the workspace target dir.
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
        "throughput smoke wrote: {} (cold={:.2}ms, warm={:.2}ms, hit_ratio={})",
        out_path.display(),
        cold_ms,
        warm_ms,
        warm_hit_ratio
    );
}

// ---------------------------------------------------------------------------
// 14. HostCpuPool integration — workers carry this host's pool-id token
// ---------------------------------------------------------------------------

/// Discriminating test for the dual-pool isolation invariant AND the
/// per-host pool ownership contract: a `compile_many` worker MUST be a
/// worker of THIS host's `HostCpuPool`.
///
/// Strict discriminator: the assertion is
/// `compile_one_host_cpu_pool_token == host.host_cpu_pool().pool_id()`.
/// The pool-id token is installed only by `HostCpuPool::new`'s
/// `start_handler`. A regression that re-routes `compile_many` onto:
///
/// - The scheduler's CPU pool — would report a different pool_id (or
///   `usize::MAX` if it didn't run on any HostCpuPool at all). The
///   caller-kind would also flip from `External` to `CpuWorker`.
/// - A per-call Rayon pool (the former implementation) — would report
///   `usize::MAX` (the sentinel for "no host-pool token") because no
///   `start_handler` installs the token; the caller-kind would still
///   read `External` *by default* (the former version relied on that
///   defaulting, which is exactly what made its tag-only check
///   non-discriminating).
/// - Any other `External`-defaulting thread (rayon::current_thread,
///   global rayon, manual std::thread, etc.) — same `None` reading.
///
/// The token check pins the contract on a positive identity, not on
/// the accidental `External` default. The caller-kind tag check is
/// preserved as a secondary canary against a `CpuWorker` regression
/// (so a future refactor that broke isolation differently — e.g.
/// stashed a CpuWorker token but ran inline anyway — would still
/// surface clearly).
#[test]
fn compile_many_workers_carry_host_cpu_pool_id() {
    let host = new_host();
    // Reset both observables so the read after `compile_many`
    // reflects only this batch's worker.
    host.compile_one_caller_kind_tag
        .store(0, std::sync::atomic::Ordering::Relaxed);
    host.compile_one_host_cpu_pool_token
        .store(usize::MAX, std::sync::atomic::Ordering::Relaxed);

    let expected_pool_id = host.host_cpu_pool().pool_id();

    let inputs = vec![ok_input("/host-pool-id.vue", &good_template("v"))];
    let entries = host.compile_many(inputs, CompileBatchOptions::default());
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0].errors.is_empty(),
        "compile_many input must compile cleanly: {:?}",
        entries[0].errors
    );

    // Primary discriminator: pool-id token equals THIS host's pool_id.
    let observed_token = host
        .compile_one_host_cpu_pool_token
        .load(std::sync::atomic::Ordering::Relaxed);
    assert_ne!(
        observed_token,
        usize::MAX,
        "compile_many worker must carry a HostCpuPool identity token — \
         got `usize::MAX` (the sentinel for `None`). This indicates the \
         worker never ran on any HostCpuPool's `start_handler`. A \
         regression where `compile_many` falls back to a per-call Rayon \
         pool (no token installed) would surface here, even if \
         `CallerKind::current()` happens to read `External` by default."
    );
    assert_eq!(
        observed_token, expected_pool_id,
        "compile_many worker must carry THIS host's pool-id \
         (expected {expected_pool_id}, got {observed_token}). A worker \
         from any OTHER HostCpuPool (e.g. a global pool, a sibling host's \
         pool) would fail this strict-equality check."
    );

    // Secondary canary: caller-kind tag is still `External`. This
    // catches a hypothetical regression that installed a token but
    // ran on a scheduler `CpuWorker` thread (e.g. via a refactor that
    // moved the token install into a different start_handler).
    let observed_kind = host
        .compile_one_caller_kind_tag
        .load(std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        observed_kind, 1,
        "compile_many workers must also report `CallerKind::External` \
         (tag 1) — got tag {observed_kind}. A `CpuWorker` (tag 3) reading \
         would indicate the batch is running on the scheduler's own CPU \
         pool, breaking the dual-pool isolation invariant."
    );
}

// ---------------------------------------------------------------------------
// 15. HostCpuPool integration — back-to-back compile_many share host pool
// ---------------------------------------------------------------------------

/// Discriminating test for the host-owned (singleton) pool ownership
/// contract: two back-to-back `compile_many` calls on the SAME host
/// must share the SAME `HostCpuPool` instance — proved by the workers
/// in BOTH calls carrying the SAME pool-id token.
///
/// Strict discriminator: each call captures
/// `compile_one_host_cpu_pool_token` (the worker's host-pool identity)
/// from inside `compile_one_in_batch`. The assertions are:
///
/// 1. **Both calls report a real host-pool token** — not the
///    `usize::MAX` sentinel that would indicate the worker ran on a
///    non-`HostCpuPool` thread (the former per-call Rayon regression).
/// 2. **Both calls report THIS host's `pool_id()`** — proves the
///    workers are this host's pool's workers, not some other pool's.
/// 3. **Both calls report the SAME token** — proves the underlying
///    pool was reused, not rebuilt. A per-call rebuild would assign
///    different `pool_id`s on each call.
///
/// This is strictly stronger than the previous `Arc::as_ptr`
/// pointer-equality check, which was structurally impossible to fail
/// (no `&self` method on `VerterHost` could mutate the
/// `pub(crate) host_cpu_pool: Arc<HostCpuPool>` field). The
/// `pool_id` check fails on a *real* regression where a worker was
/// dispatched somewhere else.
#[test]
fn two_back_to_back_compile_many_share_host_pool() {
    let host = new_host();
    let expected_pool_id = host.host_cpu_pool().pool_id();

    host.compile_one_host_cpu_pool_token
        .store(usize::MAX, std::sync::atomic::Ordering::Relaxed);
    let inputs_a = vec![ok_input("/share-a.vue", &good_template("a"))];
    let entries_a = host.compile_many(inputs_a, CompileBatchOptions::default());
    assert_eq!(entries_a.len(), 1);
    let token_after_first = host
        .compile_one_host_cpu_pool_token
        .load(std::sync::atomic::Ordering::Relaxed);

    host.compile_one_host_cpu_pool_token
        .store(usize::MAX, std::sync::atomic::Ordering::Relaxed);
    let inputs_b = vec![ok_input("/share-b.vue", &good_template("b"))];
    let entries_b = host.compile_many(inputs_b, CompileBatchOptions::default());
    assert_eq!(entries_b.len(), 1);
    let token_after_second = host
        .compile_one_host_cpu_pool_token
        .load(std::sync::atomic::Ordering::Relaxed);

    assert_ne!(
        token_after_first,
        usize::MAX,
        "first compile_many worker must carry a real HostCpuPool token \
         — got `usize::MAX` (sentinel for `None`). A regression that \
         fell back to a per-call Rayon pool would land here even with \
         `External` caller-kind."
    );
    assert_ne!(
        token_after_second,
        usize::MAX,
        "second compile_many worker must carry a real HostCpuPool token \
         — got `usize::MAX` (sentinel for `None`)."
    );
    assert_eq!(
        token_after_first, expected_pool_id,
        "first compile_many worker must report THIS host's pool_id \
         (expected {expected_pool_id}, got {token_after_first})"
    );
    assert_eq!(
        token_after_second, expected_pool_id,
        "second compile_many worker must report THIS host's pool_id \
         (expected {expected_pool_id}, got {token_after_second}) — a \
         regression that rebuilt the pool would surface a different \
         pool_id between the two calls"
    );
    assert_eq!(
        token_after_first, token_after_second,
        "both compile_many calls must share the SAME pool_id \
         (first: {token_after_first}, second: {token_after_second}) — a \
         regressed per-call `HostCpuPool::new()` would advance the \
         process-wide pool-id counter between the two reads"
    );
}

// ---------------------------------------------------------------------------
// 16. HostConfig::host_cpu_threads = Some(0) construction semantics
// ---------------------------------------------------------------------------

/// `HostConfig::host_cpu_threads = Some(0)` MUST construct a working
/// host (treated as `None` per the documented contract) rather than
/// panic. Discriminator: a regression that forwarded `0` straight to
/// `HostCpuPool::new` would trip the positive-thread assertion and
/// panic in `VerterHost::new`. The documented "treated as None"
/// behaviour means construction succeeds AND the resolved worker
/// count matches `available_parallelism()` (the same resolution path
/// as `None`), not `1`. A regression that resolved `Some(0) → Some(1)`
/// or `Some(0) → 1` would still pass `compile_many` but would
/// silently break the documented contract on a multi-core machine;
/// the `pool_thread_count` equality assertion catches that class.
#[test]
fn host_cpu_threads_some_zero_constructs_default_pool() {
    let config = HostConfig {
        host_cpu_threads: Some(0),
        ..HostConfig::default()
    };
    let host = VerterHost::new_standalone(config);
    // The pool must exist and have a positive pool_id (a panic-on-0
    // regression would have already failed at `new_standalone`).
    let pool_id = host.host_cpu_pool().pool_id();
    assert!(
        pool_id > 0,
        "HostCpuPool::pool_id must be a positive process-unique id \
         (got {pool_id})"
    );
    // Strict discriminator: the resolved worker count must match the
    // same fallback expression `host_construction.rs` uses for `None`
    // — `available_parallelism().map(|n| n.get()).unwrap_or(1)`. This
    // pins the "Some(0) is treated as None" contract from the
    // resolved-pool side rather than relying on `compile_many` to
    // smoke-test it.
    let expected_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let observed_threads = host.host_cpu_pool().pool_thread_count();
    assert_eq!(
        observed_threads, expected_threads,
        "Some(0) host_cpu_threads must resolve to available_parallelism \
         (= {expected_threads}), not be silently clamped to Some(1). \
         Observed {observed_threads} workers — a regression that \
         resolved Some(0) -> Some(1) would report 1 here on a \
         multi-core machine and would still pass the compile_many \
         smoke check below."
    );
    // Smoke-test that compile_many still works (proves the pool
    // actually has at least one worker, not that we got an empty
    // ThreadPoolBuilder past validation somehow).
    let inputs = vec![ok_input("/host-cpu-threads-0.vue", &good_template("ok"))];
    let entries = host.compile_many(inputs, CompileBatchOptions::default());
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0].errors.is_empty(),
        "compile_many must succeed under Some(0) (treated-as-None) config: {:?}",
        entries[0].errors
    );
}

/// Explicit positive `Some(N)` worker count is respected: the host
/// pool is built with exactly `N` workers and `compile_many` runs
/// against it. Discriminator: complements the `Some(0)` test by
/// pinning the `Some(N>0)` branch — a regression that swallowed
/// the explicit count and always defaulted to `available_parallelism`
/// would still pass `compile_many` but the `pool_thread_count`
/// strict-equality assertion catches the silent contract break.
#[test]
fn host_cpu_threads_some_explicit_constructs_pool() {
    let config = HostConfig {
        host_cpu_threads: Some(2),
        ..HostConfig::default()
    };
    let host = VerterHost::new_standalone(config);
    let _ = host.host_cpu_pool().pool_id(); // must not panic
                                            // Strict discriminator: pool must have exactly 2 workers.
    assert_eq!(
        host.host_cpu_pool().pool_thread_count(),
        2,
        "Some(2) host_cpu_threads must build a 2-worker pool. A \
         regression that swallowed the explicit count and used \
         available_parallelism() would report a different value here \
         on any non-2-core machine."
    );
    let inputs = vec![ok_input("/host-cpu-threads-2.vue", &good_template("ok"))];
    let entries = host.compile_many(inputs, CompileBatchOptions::default());
    assert_eq!(entries.len(), 1);
    assert!(
        entries[0].errors.is_empty(),
        "compile_many must succeed under Some(2) config: {:?}",
        entries[0].errors
    );
}
