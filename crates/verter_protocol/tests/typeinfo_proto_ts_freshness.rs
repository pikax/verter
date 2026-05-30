//! Freshness guard for the generated TypeScript typeinfo bindings.
//!
//! The TypeScript surface at `packages/proto/src/gen/verter/v1/typeinfo_pb.ts`
//! is produced from the proto schema at
//! `crates/verter_protocol/proto/verter/v1/typeinfo.proto` through
//! the `pnpm proto:gen` pipeline (`buf generate && oxfmt`). Whenever
//! the proto changes the generated file must be regenerated and
//! committed in the same change; this guard pins the contract with
//! TWO discriminators:
//!
//! 1. `typeinfo_ts_bindings_reference_every_proto_message` — every
//!    `message` name in the proto schema appears as a genmessage
//!    descriptor in the TS file, every `enum` name appears as a TS
//!    enum descriptor, and the generated file records the proto
//!    source path header. Adding a new message / enum without
//!    regenerating fails this structural test.
//! 2. `typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output`
//!    — when `buf` is available on PATH (locally, in CI, or via the
//!    `@bufbuild/buf` devDependency), regenerate into a tempdir, run
//!    `oxfmt` on the temp output, and byte-compare against the
//!    committed file. Any drift — schema change without regen, hand
//!    edit, formatter mismatch — surfaces as a named diff.
//!
//! The byte-equality test gracefully skips when the `buf` binary is
//! absent (running cargo on a machine without node tooling); the
//! structural test always runs.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR must be `<workspace>/crates/verter_protocol`")
        .to_path_buf()
}

fn read_workspace_file(rel: &str) -> String {
    let path = workspace_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|err| {
        panic!(
            "freshness check should be able to read `{}`: {err}",
            path.display()
        )
    })
}

#[test]
fn typeinfo_ts_bindings_reference_every_proto_message() {
    let proto = read_workspace_file("crates/verter_protocol/proto/verter/v1/typeinfo.proto");
    let ts = read_workspace_file("packages/proto/src/gen/verter/v1/typeinfo_pb.ts");

    let mut messages: BTreeSet<String> = BTreeSet::new();
    let mut enums: BTreeSet<String> = BTreeSet::new();

    // Lightweight scan; the proto schema in this file is hand-authored
    // and uses the documented format `message <Name> {` / `enum <Name> {`.
    for raw_line in proto.lines() {
        let line = raw_line.trim_start();
        if let Some(rest) = line.strip_prefix("message ") {
            if let Some(name) = rest.split_whitespace().next() {
                let name = name.trim_end_matches('{').trim();
                if !name.is_empty() {
                    messages.insert(name.to_string());
                }
            }
        } else if let Some(rest) = line.strip_prefix("enum ") {
            if let Some(name) = rest.split_whitespace().next() {
                let name = name.trim_end_matches('{').trim();
                if !name.is_empty() {
                    enums.insert(name.to_string());
                }
            }
        }
    }

    assert!(
        !messages.is_empty(),
        "freshness scan should have found at least one message in typeinfo.proto",
    );

    // Every message must appear as `MessageSchema: GenMessage<` in the
    // generated TS surface (the protoc-gen-es target=ts shape).
    let mut missing_messages: Vec<String> = Vec::new();
    for name in &messages {
        let needle = format!("export const {name}Schema");
        if !ts.contains(&needle) {
            missing_messages.push(name.clone());
        }
    }

    let mut missing_enums: Vec<String> = Vec::new();
    for name in &enums {
        let needle_enum = format!("export enum {name}");
        let needle_desc = format!("export const {name}Schema");
        if !ts.contains(&needle_enum) && !ts.contains(&needle_desc) {
            missing_enums.push(name.clone());
        }
    }

    if !missing_messages.is_empty() || !missing_enums.is_empty() {
        panic!(
            "typeinfo_pb.ts is stale w.r.t. typeinfo.proto.\n\
             Missing message schemas: {missing_messages:?}\n\
             Missing enums: {missing_enums:?}\n\
             Run `buf generate` from the repository root to regenerate \
             `packages/proto/src/gen/verter/v1/typeinfo_pb.ts`, then commit \
             the regenerated file alongside the proto change."
        );
    }
}

#[test]
fn typeinfo_ts_bindings_record_the_proto_file_path() {
    let ts = read_workspace_file("packages/proto/src/gen/verter/v1/typeinfo_pb.ts");
    // The protoc-gen-es header records the source proto file path,
    // so a TS surface that was hand-edited (or moved to another proto
    // input) loses this header and trips the guard.
    assert!(
        ts.contains("@generated from file verter/v1/typeinfo.proto"),
        "typeinfo_pb.ts must record the proto source path in its @generated header",
    );
}

/// Locate the `buf` binary the workspace ships:
///
/// 1. Prefer `node_modules/.bin/buf` (the locked `@bufbuild/buf`
///    devDependency) so version drift is impossible.
/// 2. Fall back to `buf` on PATH.
/// 3. Return `None` when neither resolves — the test then skips
///    gracefully (running on a node-free machine).
fn locate_buf_binary(workspace_root: &Path) -> Option<PathBuf> {
    let workspace_buf = workspace_root.join("node_modules").join(".bin").join("buf");
    if workspace_buf.is_file() {
        return Some(workspace_buf);
    }
    // PATH lookup — `which buf` style.
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("buf");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Locate the `oxfmt` binary the workspace ships at root devDeps.
fn locate_oxfmt_binary(workspace_root: &Path) -> Option<PathBuf> {
    let workspace_oxfmt = workspace_root
        .join("node_modules")
        .join(".bin")
        .join("oxfmt");
    if workspace_oxfmt.is_file() {
        return Some(workspace_oxfmt);
    }
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join("oxfmt");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Byte-equality freshness discriminator: regenerate the typeinfo
/// TS bindings into a tempdir via `pnpm proto:gen`'s underlying
/// pipeline (`buf generate` followed by `oxfmt`), then byte-compare
/// the regenerated output against the committed file. Any drift —
/// schema edit without regen, hand-edit, formatter version mismatch
/// — surfaces as a named failure with a concrete remediation hint.
#[test]
fn typeinfo_ts_bindings_are_byte_equal_to_regenerated_buf_output() {
    let root = workspace_root();
    let Some(buf_bin) = locate_buf_binary(&root) else {
        // Skip gracefully when the buf CLI isn't installed (e.g.
        // running `cargo test` on a node-free machine). The
        // structural test above still runs.
        eprintln!(
            "skipping byte-equality freshness check: `buf` not found in `node_modules/.bin/` \
             or on `PATH`. Run `pnpm install` (or `pnpm proto:gen`) to populate it."
        );
        return;
    };

    let tempdir = tempfile::tempdir().expect("create tempdir for buf regen");
    let out_dir = tempdir.path().to_path_buf();

    // Run `buf generate` directing protoc-gen-es output into the
    // tempdir. The plugin honours the `--template` override on
    // `buf.gen.yaml`; we point it at a small generated template
    // that swaps the `out:` field to our tempdir.
    let template_path = tempdir.path().join("buf.gen.tempdir.yaml");
    let template_body = format!(
        "version: v2\n\
         plugins:\n  - local:\n      - node\n      - node_modules/@bufbuild/protoc-gen-es/bin/protoc-gen-es\n    \
         opt: target=ts\n    out: {}\ninputs:\n  - directory: crates/verter_protocol/proto\n",
        out_dir.display()
    );
    std::fs::write(&template_path, template_body).expect("write temp buf template");

    let status = Command::new(&buf_bin)
        .arg("generate")
        .arg("--template")
        .arg(&template_path)
        .current_dir(&root)
        .status()
        .unwrap_or_else(|err| panic!("invoke `buf generate`: {err}"));
    assert!(status.success(), "`buf generate` exited non-zero: {status}",);

    // Format the regenerated output via the same oxfmt the
    // `pnpm proto:gen` script runs, so the byte-compare is against
    // an oxfmt-normalised baseline.
    let regen_typeinfo_path = out_dir.join("verter").join("v1").join("typeinfo_pb.ts");
    assert!(
        regen_typeinfo_path.is_file(),
        "buf regen did not produce typeinfo_pb.ts under {}",
        out_dir.display(),
    );
    if let Some(oxfmt_bin) = locate_oxfmt_binary(&root) {
        let status = Command::new(&oxfmt_bin)
            .arg(&regen_typeinfo_path)
            .status()
            .unwrap_or_else(|err| panic!("invoke `oxfmt`: {err}"));
        assert!(status.success(), "`oxfmt` exited non-zero: {status}");
    }

    let regenerated =
        std::fs::read_to_string(&regen_typeinfo_path).expect("read regenerated typeinfo_pb.ts");
    let committed = read_workspace_file("packages/proto/src/gen/verter/v1/typeinfo_pb.ts");

    if regenerated != committed {
        // Show a small diff prefix to help diagnose; the full file is
        // available on disk via the tempdir path printed above.
        let regenerated_lines: Vec<&str> = regenerated.lines().collect();
        let committed_lines: Vec<&str> = committed.lines().collect();
        let mut first_diff = None;
        let limit = regenerated_lines.len().min(committed_lines.len());
        for i in 0..limit {
            if regenerated_lines[i] != committed_lines[i] {
                first_diff = Some(i);
                break;
            }
        }
        let preview = if let Some(idx) = first_diff {
            format!(
                "first divergence at line {}:\n  committed:    {}\n  regenerated:  {}",
                idx + 1,
                committed_lines[idx],
                regenerated_lines[idx],
            )
        } else if regenerated_lines.len() != committed_lines.len() {
            format!(
                "line counts differ: committed={}, regenerated={}",
                committed_lines.len(),
                regenerated_lines.len(),
            )
        } else {
            "files differ but no per-line divergence found (likely trailing whitespace or EOF)"
                .to_string()
        };
        panic!(
            "`packages/proto/src/gen/verter/v1/typeinfo_pb.ts` is out of sync with the \
             proto schema or the committed bindings drifted from the canonical \
             `buf generate` + `oxfmt` output. Run `pnpm proto:gen` to update bindings. \n{preview}",
        );
    }
}
