//! Architecture guard — the legacy `compile_many` Stage-B per-file
//! upsert fan-out is fully retired.
//!
//! `compile_many`'s Stage-B per-file submit→wait path (an outer
//! `HostBatchCoordinator::run_batch` over
//! `canonical_to_upsert`, each worker issuing one
//! `Scheduler::submit_request` + one `wait_or_drive` buried inside
//! `upsert_via_scheduler_with_priority`) is replaced by a SINGLE
//! `Scheduler::submit_batch_atomic` + one `wait_batch` driven from the
//! one shared upsert engine (`upsert_many_with_priority`). The single-
//! file public `upsert` collapsed onto the same engine as a 1-element
//! batch.
//!
//! This guard asserts none of the retired construct's names or call
//! shapes survive in `crates/verter_session/src/**`:
//!
//!  - `upsert_via_scheduler_with_priority` — the deleted per-file
//!    submit→wait→post-commit method (its post-commit body now lives in
//!    `finish_upsert_post_commit`, which the single engine drives once
//!    per canonical AFTER the one `wait_batch`).
//!  - `upsert_with_priority_for_batch` — the deleted batch-side wrapper
//!    that `compile_many` called per file through the coordinator.
//!  - `compile_many_upsert` — the deleted Stage-B `BatchPolicy::label`
//!    naming the retired upsert fan-out.
//!  - `run_batch(&canonical_to_upsert` — the deleted Stage-B
//!    coordinator fan-out call shape.
//!
//! The scanner follows the architecture-guard discipline: it scans only
//! production source under `crates/verter_session/src/**/*.rs`, skips
//! `_tests.rs`/`tests.rs`/`tests/` files, and strips line/block/inline-
//! `#[cfg(test)] mod` content before matching so doc comments and test
//! scaffolding never trip the gate.

use std::path::{Path, PathBuf};

/// The retired identifiers / call shapes. The first three are matched
/// as substrings of the preprocessed source (they are unique enough
/// that substring matching is unambiguous — `compile_many_upsert` is a
/// label literal, the two `upsert_*` names are unique fn idents). The
/// fourth is a call-expression shape, intentionally matched as a
/// substring of the (whitespace-normalised) source.
const RETIRED_NEEDLES: &[&str] = &[
    "upsert_via_scheduler_with_priority",
    "upsert_with_priority_for_batch",
    "compile_many_upsert",
];

/// The retired Stage-B coordinator fan-out call shape. Matched after
/// whitespace normalisation so `run_batch ( & canonical_to_upsert`
/// (the `quote`-style spacing) and `run_batch(&canonical_to_upsert`
/// (the source spacing) both hit.
const RETIRED_RUN_BATCH_FANOUT: &str = "run_batch(&canonical_to_upsert";

fn session_src_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// `true` for files that are test scaffolding, not production source:
/// `_tests.rs` / `tests.rs` siblings or any file under a `tests/`
/// segment.
fn is_test_file(path: &Path) -> bool {
    let p = path.to_string_lossy().replace('\\', "/");
    if p.split('/').any(|seg| seg == "tests") {
        return true;
    }
    match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name.ends_with("_tests.rs") || name == "tests.rs",
        None => false,
    }
}

fn collect_production_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    for entry in std::fs::read_dir(dir).expect("read dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_production_rs(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") && !is_test_file(&path) {
            out.push(path);
        }
    }
}

/// Strip `//` line comments, `///`/`//!` doc comments, `/* */` block
/// comments, and inline `#[cfg(test)] mod ... { ... }` bodies so a
/// retired name appearing only in documentation or test scaffolding
/// does not trip the gate. Conservative brace-matching for the cfg-test
/// module body (sufficient for this crate's source shape).
fn preprocess(src: &str) -> String {
    // 1. Remove block comments.
    let mut no_block = String::with_capacity(src.len());
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            i += 2;
            while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
            continue;
        }
        no_block.push(bytes[i] as char);
        i += 1;
    }

    // 2. Remove line/doc comments line-by-line, AND strip inline
    //    `#[cfg(test)]`-gated `mod ... { ... }` bodies.
    let mut out = String::with_capacity(no_block.len());
    let mut lines = no_block.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        // Detect a `#[cfg(test)]` attribute that gates the following
        // `mod ... {`. Skip until brace depth returns to zero.
        if trimmed.starts_with("#[cfg(test)]") {
            // Consume following lines until we open the module brace.
            let mut depth: i32 = 0;
            let mut opened = false;
            // The attribute line itself may also carry the `mod {`.
            let mut pending = vec![line.to_string()];
            for next in lines.by_ref() {
                let has_brace = next.contains('{');
                pending.push(next.to_string());
                if has_brace {
                    opened = true;
                    break;
                }
                // If the gated item is not a module (e.g. a gated fn on
                // one line with no brace yet), keep consuming until a
                // brace appears.
                if pending.len() > 64 {
                    break;
                }
            }
            if !opened {
                // Couldn't find an opening brace — emit nothing for the
                // attribute (conservative: drop it).
                continue;
            }
            // Count braces across the joined pending text, then keep
            // consuming until depth returns to zero.
            let joined = pending.join("\n");
            for ch in joined.chars() {
                match ch {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    _ => {}
                }
            }
            while depth > 0 {
                match lines.next() {
                    Some(body_line) => {
                        for ch in body_line.chars() {
                            match ch {
                                '{' => depth += 1,
                                '}' => depth -= 1,
                                _ => {}
                            }
                        }
                    }
                    None => break,
                }
            }
            continue;
        }
        // Strip a trailing `//` line comment (doc or normal). Naive but
        // sufficient: there are no `//` inside string literals on the
        // retired-needle lines in this crate.
        let code = match line.find("//") {
            Some(idx) => &line[..idx],
            None => line,
        };
        out.push_str(code);
        out.push('\n');
    }
    out
}

/// Collapse all ASCII whitespace runs to a single space so call shapes
/// match regardless of formatting.
fn normalize_ws(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut last_space = false;
    for ch in src.chars() {
        if ch.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    out
}

#[test]
fn no_legacy_compile_many_upsert_fanout() {
    let mut files = Vec::new();
    collect_production_rs(&session_src_root(), &mut files);
    assert!(
        !files.is_empty(),
        "guard found no production source files under {} — scanner misconfigured",
        session_src_root().display()
    );

    let mut hits: Vec<String> = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let processed = preprocess(&text);
        for needle in RETIRED_NEEDLES {
            if processed.contains(needle) {
                hits.push(format!("{}: retired symbol `{needle}`", file.display()));
            }
        }
        // The fan-out call shape is matched after whitespace
        // normalisation so both `run_batch(&canonical_to_upsert, ...)`
        // and any reformatted spacing collapse to the same needle.
        let ws = normalize_ws(&processed.replace(' ', ""));
        if ws.contains(RETIRED_RUN_BATCH_FANOUT) {
            hits.push(format!(
                "{}: retired Stage-B fan-out call `{RETIRED_RUN_BATCH_FANOUT}...)`",
                file.display()
            ));
        }
    }

    assert!(
        hits.is_empty(),
        "compile_many Stage-B fan-out guard: the legacy per-file upsert \
         fan-out must be fully retired. The single shared upsert engine is \
         `upsert_many_with_priority` (one `submit_batch_atomic` + one \
         `wait_batch`); per-canonical post-commit runs through \
         `finish_upsert_post_commit`. Re-introducing any retired name or \
         call shape resurrects the deleted dual path.\nHits:\n{hits:#?}"
    );
}

// ---------------------------------------------------------------------------
// Discriminating-property fixtures: the scanner must FLAG a live
// production reference and IGNORE a doc/test-only one.
// ---------------------------------------------------------------------------

#[test]
fn scanner_flags_live_reference_and_ignores_doc_and_test() {
    // (a) Live production reference — must be detected.
    let live = "fn f(&self) { self.upsert_via_scheduler_with_priority(req, p); }\n";
    assert!(
        preprocess(live).contains("upsert_via_scheduler_with_priority"),
        "scanner must detect a live retired-fn reference"
    );

    // (b) Live Stage-B fan-out call shape — must be detected.
    let live_fanout = "let r = coordinator.run_batch(&canonical_to_upsert, &policy, f);\n";
    let ws = normalize_ws(&preprocess(live_fanout).replace(' ', ""));
    assert!(
        ws.contains(RETIRED_RUN_BATCH_FANOUT),
        "scanner must detect the retired `run_batch(&canonical_to_upsert` call shape"
    );

    // (c) Doc-comment reference — must be ignored.
    let doc = "/// see upsert_with_priority_for_batch for the old wrapper\nfn g() {}\n";
    assert!(
        !preprocess(doc).contains("upsert_with_priority_for_batch"),
        "scanner must erase doc-comment references to retired names"
    );

    // (d) Inline `#[cfg(test)] mod` reference — must be ignored.
    let test_mod = "fn live() {}\n#[cfg(test)]\nmod tests {\n    fn t() { let _ = compile_many_upsert(); }\n}\n";
    assert!(
        !preprocess(test_mod).contains("compile_many_upsert"),
        "scanner must erase #[cfg(test)] mod bodies"
    );
}
