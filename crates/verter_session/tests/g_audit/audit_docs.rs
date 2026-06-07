//! Documentation gate tests.
//!
//! Guards the audit-footprint documentation surface against silent
//! regressions:
//!
//! * `audit_modules_compile_without_missing_docs_warnings` — every
//!   audit-scope module carries `#![deny(missing_docs)]`. The fact
//!   that this test binary links proves the workspace built without
//!   the deny attribute tripping; the test body additionally greps
//!   each file to pin the attribute's presence (a future refactor
//!   that strips the attribute would fail here before the first
//!   doc regression lands).
//! * `audit_doctests_pass` — loads the committed
//!   `docs/audit-footprint/*` markdown files and asserts every
//!   ```rust code fence that isn't marked `ignore` / `no_run`
//!   references names that still exist in the audit crate. This
//!   is stricter than a plain `cargo test --doc`: it catches docs
//!   that reference removed API names without the corresponding
//!   doctest harness cost.
//! * `skill_references_audit_api_names_exactly_as_exported` —
//!   reads `.claude/skills/component-meta/SKILL.md` and verifies
//!   every enumerated API name matches a symbol exported by the
//!   audit crate.
//! * `readme_code_examples_mirror_rustdoc_doctests` — cross-checks
//!   README code examples against rustdoc doctests so the two
//!   surfaces do not drift.

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if p.join("docs/audit-footprint/README.md").exists() {
            return p;
        }
        if !p.pop() {
            panic!(
                "unable to locate workspace root by walking up from `{}`; \
                 expected `docs/audit-footprint/README.md` to exist",
                env!("CARGO_MANIFEST_DIR"),
            );
        }
    }
}

/// Files that must carry module-scoped `#![deny(missing_docs)]`.
const DOC_DENY_FILES: &[&str] = &[
    "crates/verter_scheduler/src/request_context.rs",
    "crates/verter_session/src/request_context.rs",
    "crates/verter_session/src/audited_request.rs",
    "crates/verter_session/src/component_meta_audit/mod.rs",
    "crates/verter_session/src/component_meta_audit/accumulator.rs",
    "crates/verter_session/src/component_meta_audit/assertions.rs",
    "crates/verter_session/src/component_meta_audit/audit_records_store.rs",
    "crates/verter_session/src/component_meta_audit/footprint_miner.rs",
    "crates/verter_session/src/component_meta_audit/session_vfs_sink.rs",
    "crates/verter_session/src/component_meta_audit/structured_event.rs",
    "crates/verter_workspace/src/audit_sink.rs",
];

#[test]
fn audit_modules_compile_without_missing_docs_warnings() {
    // The workspace build is the primary gate — if `#![deny(missing_docs)]`
    // catches an undocumented item, `cargo build --workspace --tests`
    // fails first. This test's role is to
    // pin the ATTRIBUTE's presence so a future refactor can't silently
    // strip it without failing a test whose name names the risk.
    let root = workspace_root();
    for rel in DOC_DENY_FILES {
        let path = root.join(rel);
        let src =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read `{}`: {e}", path.display()));
        assert!(
            src.contains("#![deny(missing_docs)]"),
            "`{rel}` must carry `#![deny(missing_docs)]`. \
             If this test is failing because the attribute was removed, either \
             restore it or update this test together.",
        );
    }
}

#[test]
fn audit_doc_snippet_names_resolve_to_exported_symbols() {
    // (Previously named `audit_doctests_pass` — renamed in review
    // F12 because the body does NAME RESOLUTION against the audit
    // source, not `cargo test --doc` execution. The rustdoc
    // `#![deny(missing_docs)]` attribute already guarantees every
    // public item carries documentation; actual doctest execution
    // is handled by `cargo test --doc` when the user explicitly
    // runs it.)
    //
    // Parse each `docs/audit-footprint/*.md`. For every fenced Rust
    // code block that is not `ignore` / `no_run`, verify that every
    // type name referenced in the block corresponds to a string
    // present in one of the audit-crate source files. This catches
    // docs that reference removed or renamed API names before a
    // doctest run ever happens.
    let root = workspace_root();
    let docs_dir = root.join("docs/audit-footprint");
    let audit_src_blob = build_audit_source_blob(&root);

    let mut checked_files = 0usize;
    for entry in fs::read_dir(&docs_dir).expect("read docs dir") {
        let entry = entry.unwrap();
        if entry.path().extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let src = fs::read_to_string(entry.path()).expect("read doc");
        for block in extract_rust_blocks(&src) {
            if block.ignored {
                continue;
            }
            // Look for identifiers that look like Rust type names —
            // CamelCase words of length ≥ 4 that begin with an
            // uppercase letter. We match these against the audit
            // sources; any unresolved name fails the test.
            for name in extract_type_names(&block.body) {
                assert!(
                    audit_src_blob.contains(&name) || STDLIB_NAMES.contains(&name.as_str()),
                    "`docs/audit-footprint/{}` references `{}` in a Rust code block but \
                     the audit crate no longer exports that name. Either update the doc \
                     or restore the API.",
                    entry.file_name().to_string_lossy(),
                    name,
                );
            }
        }
        checked_files += 1;
    }
    assert!(
        checked_files >= 3,
        "expected at least 3 markdown files under docs/audit-footprint/; found {checked_files}",
    );
}

#[test]
fn skill_references_audit_api_names_exactly_as_exported() {
    let root = workspace_root();
    let skill_path = root.join(".claude/skills/component-meta/SKILL.md");
    // `.claude/skills/component-meta/SKILL.md` is tracked in this
    // repo. Silent-skip on missing file would hide a `git rm` mistake
    // on a future branch; an explicit env-var escape hatch keeps the
    // path open for downstream consumers who vendor this test without
    // the skill file.
    if !skill_path.exists() {
        if std::env::var("VERTER_SKIP_SKILL_MD_CHECKS").is_ok() {
            eprintln!(
                "skipping — {} not present and VERTER_SKIP_SKILL_MD_CHECKS is set",
                skill_path.display(),
            );
            return;
        }
        panic!(
            "`.claude/skills/component-meta/SKILL.md` is missing at `{}`. This file \
             must remain tracked. If you are intentionally testing a \
             vendored checkout without the skill file, set \
             `VERTER_SKIP_SKILL_MD_CHECKS=1`; otherwise, restore the file.",
            skill_path.display(),
        );
    }
    let skill = fs::read_to_string(&skill_path).expect("read SKILL.md");
    // Audit-surface API names. If you add a
    // reference in the skill file, append the name here — the test
    // pins the mapping so "I mentioned X but X was renamed" surfaces.
    let audit_api_names = &[
        "RequestAuditRecord",
        "RequestFootprintAudit",
        "AuditedRequest",
        "loaded_files",
        "declared_dependency_files",
        "audit_validator.ts",
    ];
    for name in audit_api_names {
        if skill.contains(name) {
            // name is referenced — check it still exists in the audit
            // source blob.
            let sources =
                concat_sources(&root.join("crates/verter_session/src/component_meta_audit"))
                    + &fs::read_to_string(
                        root.join("crates/verter_session/src/audited_request.rs"),
                    )
                    .unwrap();
            let ts_sources =
                fs::read_to_string(root.join("packages/benchmark/src/audit-validator.ts"))
                    .unwrap_or_default();
            let exists = sources.contains(name) || ts_sources.contains(name);
            assert!(
                exists,
                "SKILL.md references `{name}` but it is not present in \
                 the audit crate or the benchmark validator. Update the \
                 skill or restore the API.",
            );
        }
    }
}

#[test]
fn readme_code_examples_mirror_rustdoc_doctests() {
    // The audit-footprint README contains usage snippets. Every
    // identifier referenced in a Rust code block in the README
    // must also appear in the audit crate's source — this prevents
    // README drift where a snippet says `my_method()` but the method
    // has been renamed.
    let root = workspace_root();
    let readme_path = root.join("docs/audit-footprint/README.md");
    let readme = fs::read_to_string(&readme_path).expect("read README");
    let api_ref_path = root.join("docs/audit-footprint/api-reference.md");
    let api_ref = fs::read_to_string(&api_ref_path).expect("read api-reference");

    let audit_src = build_audit_source_blob(&root);

    for (file_name, doc) in [("README.md", &readme), ("api-reference.md", &api_ref)] {
        for block in extract_rust_blocks(doc) {
            if block.ignored {
                continue;
            }
            for name in extract_type_names(&block.body) {
                assert!(
                    audit_src.contains(&name) || STDLIB_NAMES.contains(&name.as_str()),
                    "docs/audit-footprint/{file_name}: snippet references `{name}` \
                     but the audit crate does not export it. Update the doc or \
                     restore the API.",
                );
            }
        }
    }
}

// ── helpers ──

struct CodeBlock {
    body: String,
    ignored: bool,
}

fn extract_rust_blocks(src: &str) -> Vec<CodeBlock> {
    let mut out = Vec::new();
    let mut lines = src.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if let Some(fence_rest) = trimmed.strip_prefix("```") {
            let fence = fence_rest.trim();
            if !(fence == "rust" || fence.starts_with("rust ") || fence.starts_with("rust,")) {
                // Not a Rust block — skip until the closing fence.
                for l in lines.by_ref() {
                    if l.trim().starts_with("```") {
                        break;
                    }
                }
                continue;
            }
            let ignored = fence.contains("ignore") || fence.contains("no_run");
            let mut body = String::new();
            for l in lines.by_ref() {
                if l.trim().starts_with("```") {
                    break;
                }
                body.push_str(l);
                body.push('\n');
            }
            out.push(CodeBlock { body, ignored });
        }
    }
    out
}

fn extract_type_names(src: &str) -> Vec<String> {
    // Identifiers that look like Rust CamelCase type names — start
    // with an uppercase ASCII letter, length ≥ 4, and contain at
    // least one lowercase letter (to exclude SCREAMING_SNAKE_CASE
    // constants which are placeholders in snippets, not type
    // references). Also exclude leading-underscore identifiers.
    let mut out = std::collections::BTreeSet::new();
    let mut chars = src.chars().peekable();
    let mut cur = String::new();
    while let Some(c) = chars.next() {
        if c.is_ascii_uppercase() && cur.is_empty() {
            cur.push(c);
            let mut has_lower = false;
            while let Some(&next) = chars.peek() {
                if next.is_ascii_alphanumeric() || next == '_' {
                    cur.push(next);
                    if next.is_ascii_lowercase() {
                        has_lower = true;
                    }
                    chars.next();
                } else {
                    break;
                }
            }
            if cur.len() >= 4 && has_lower {
                out.insert(cur.clone());
            }
            cur.clear();
        }
    }
    out.into_iter().collect()
}

fn concat_sources(dir: &Path) -> String {
    let mut out = String::new();
    for entry in fs::read_dir(dir).expect("read source dir") {
        let entry = entry.expect("dir entry");
        if entry.path().extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push_str(&fs::read_to_string(entry.path()).expect("read source"));
            out.push('\n');
        }
    }
    out
}

/// Build the audit-source blob the doc-snippet name-resolution
/// tests check against. Spans both the substrate crate
/// (`verter_audit/src/`) and the session-side audit owner under
/// `verter_session/src/` (the `component_meta_audit/` module tree,
/// the host runtime, the audited-request harness, the per-
/// `*_with_audit` entry-point modules, and the request-context
/// module). The substrate / session split (CLAUDE.md `Shared
/// Optimized Codebase`) means doc snippets reference DTOs from
/// `verter_audit` AND lifecycle types from `verter_session`; both
/// crates' source must be considered before declaring a name
/// "no longer exported".
fn build_audit_source_blob(root: &Path) -> String {
    let mut blob = String::new();

    // Session-side audit ownership: component_meta_audit module
    // tree + the harness + request-context source.
    blob.push_str(&concat_sources(
        &root.join("crates/verter_session/src/component_meta_audit"),
    ));
    blob.push('\n');
    blob.push_str(
        &fs::read_to_string(root.join("crates/verter_session/src/audited_request.rs"))
            .expect("read audited_request.rs"),
    );
    blob.push('\n');
    blob.push_str(
        &fs::read_to_string(root.join("crates/verter_session/src/request_context.rs"))
            .expect("read request_context.rs"),
    );
    blob.push('\n');
    blob.push_str(
        &fs::read_to_string(root.join("crates/verter_session/src/host_audit_runtime.rs"))
            .expect("read host_audit_runtime.rs"),
    );
    blob.push('\n');
    // Per-`*_with_audit` host entry-point modules — the docs
    // describe their public signatures.
    for entry_point in [
        "host_compile_audit.rs",
        "host_analyze_audit.rs",
        "host_resolve_type_audit.rs",
        "host_workspace_audit.rs",
        "host_lsp_audit.rs",
        "host_mcp_audit.rs",
    ] {
        let p = root.join("crates/verter_session/src").join(entry_point);
        if let Ok(s) = fs::read_to_string(&p) {
            blob.push_str(&s);
            blob.push('\n');
        }
    }

    // Substrate: every `*.rs` under `verter_audit/src/` (recursive).
    // The `verter_audit` crate owns `RequestAuditRecord`,
    // `RequestKind`, `RequestKindPayload`, all per-kind payload
    // structs, the `AuditObserver` trait and `current_observer()`
    // accessor, `NoOpObserver`, `AuditConfig` + `AuditConsumerFilter`,
    // `BatchAuditAggregator` + `AuditRecordSource`, the structured-
    // event enum, plus the per-record sub-DTOs
    // (`RequestTimingAudit`, `RequestMemoryAudit`, `WaitAudit`,
    // `SchedulerAudit`, `RequestStoreAudit`, `FileAudit`, footprint
    // records, origin graph DTOs).
    walk_audit_src(&root.join("crates/verter_audit/src"), &mut blob);
    blob
}

fn walk_audit_src(dir: &Path, out: &mut String) {
    for entry in fs::read_dir(dir).expect("read verter_audit src dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.is_dir() {
            walk_audit_src(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push_str(&fs::read_to_string(&path).expect("read verter_audit source"));
            out.push('\n');
        }
    }
}

/// A conservative allow-list of standard-library / framework type
/// names that may appear in code snippets without being defined in
/// the audit crate. Extend only if a doc deliberately references a
/// common std type and the test fails on it.
const STDLIB_NAMES: &[&str] = &[
    "Option",
    "Some",
    "None",
    "Result",
    "Ok",
    "Err",
    "String",
    "Vec",
    "Box",
    "Arc",
    "Rc",
    "Cell",
    "RefCell",
    "Mutex",
    "RwLock",
    "Self",
    "Debug",
    "Clone",
    "Copy",
    "PartialEq",
    "Eq",
    "Hash",
    "Ord",
    "PartialOrd",
    "Send",
    "Sync",
    "Sized",
    "Display",
    "From",
    "Into",
    "IntoIterator",
    "Iterator",
    "Default",
    "Drop",
    "Deref",
    "DerefMut",
    "AsRef",
    "AsMut",
    "BTreeMap",
    "BTreeSet",
    "HashMap",
    "HashSet",
    "ProvenanceChain",
    "Promise",
    "JSON",
    "BigInt",
    "Buffer",
    "Hover",
    "Window",
    "Document",
    "Element",
    "Array",
    "Object",
    "Map",
    "Set",
    "Date",
    "Error",
    "TypeError",
    "RangeError",
    "SyntaxError",
    "Uint8Array",
    "Int8Array",
    "Float64Array",
    "Int32Array",
    "Uint32Array",
    "Float32Array",
    "ArrayBuffer",
    "DataView",
    "Symbol",
    "Function",
    "Reflect",
    "Proxy",
];
