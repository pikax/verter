//! Static contract guards for the typeinfo audit surface.
//!
//! Two invariants:
//! - Graph-publication diagnostics / degradation accounting lives ONLY
//!   on `TypeInfoGraphPayload`. No other `RequestKindPayload` arm carries
//!   the graph degradation fields.
//! - The `AuditedResult<T, E>` carrier lives in `verter_audit` (NOT
//!   `verter_protocol`, which is protobuf-authoritative and ts-rs-banned)
//!   and exports through `audit.generated.ts` via ts-rs.

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    crate_root
        .parent()
        .and_then(|p| p.parent())
        .expect("CARGO_MANIFEST_DIR must be `<workspace>/crates/verter_session`")
        .to_path_buf()
}

fn read_workspace_file(rel: &str) -> String {
    let path = workspace_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("file {} should be readable: {err}", path.display()))
}

/// Collect every `*.rs` file directly under `crates/verter_audit/src/payloads`.
fn audit_payload_files() -> Vec<PathBuf> {
    let dir = workspace_root().join("crates/verter_audit/src/payloads");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("payloads dir must be readable") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    out.sort();
    out
}

#[test]
fn diagnostics_only_on_typeinfo_graph_payload() {
    // Graph-publication degradation accounting — `degraded: bool` and
    // `degradation_reasons: Vec<TypeInfoDegradationReasonTag>` — is the
    // typeinfo-graph diagnostics surface. It must live ONLY on
    // `TypeInfoGraphPayload`; no other RequestKindPayload arm may carry
    // the graph degradation fields (their domain diagnostics — macro
    // expansion, walker — are a separate, non-graph concern). A field
    // declaration leaking these into another payload would let a
    // non-graph request masquerade as a degraded graph publication.
    //
    // Discriminating: declare `pub degraded: bool` on any other payload
    // struct and this guard fails naming that file.
    let graph_field_markers = ["pub degraded:", "pub degradation_reasons:"];
    let mut offenders: Vec<String> = Vec::new();
    let mut typeinfo_carries_them = false;

    for file in audit_payload_files() {
        let name = file
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        let contents = std::fs::read_to_string(&file).expect("read payload file");
        // Strip comment lines so doc-comment mentions of "degraded" do
        // not count as field declarations.
        let code_only: String = contents
            .lines()
            .filter(|line| {
                let t = line.trim_start();
                !t.starts_with("//!") && !t.starts_with("///") && !t.starts_with("//")
            })
            .collect::<Vec<_>>()
            .join("\n");
        let carries_graph_fields = graph_field_markers.iter().any(|m| code_only.contains(m));
        if name == "typeinfo_graph.rs" {
            typeinfo_carries_them = carries_graph_fields;
        } else if carries_graph_fields {
            offenders.push(name);
        }
    }

    assert!(
        typeinfo_carries_them,
        "TypeInfoGraphPayload (payloads/typeinfo_graph.rs) must declare the graph \
         degradation fields (`degraded` + `degradation_reasons`) — they are the \
         typeinfo-graph diagnostics surface",
    );
    assert!(
        offenders.is_empty(),
        "graph degradation fields (`degraded` / `degradation_reasons`) must live ONLY \
         on TypeInfoGraphPayload — found them on other payload(s): {offenders:?}. \
         A non-graph request must not carry graph-publication degradation accounting.",
    );

    // The degradation-reason taxonomy is also typeinfo-graph-specific —
    // `TypeInfoDegradationReasonTag` is declared in the typeinfo payload
    // module, not shared into another payload.
    let typeinfo_src = read_workspace_file("crates/verter_audit/src/payloads/typeinfo_graph.rs");
    assert!(
        typeinfo_src.contains("pub enum TypeInfoDegradationReasonTag"),
        "TypeInfoDegradationReasonTag must be declared in the typeinfo graph payload module",
    );
}

#[test]
fn audited_result_lives_in_audit_and_exports_through_generated_ts() {
    // The AuditedResult<T, E> carrier MUST live in verter_audit (the
    // crate that owns ts-rs + audit.generated.ts) and MUST NOT live in
    // verter_protocol (which is protobuf-authoritative and ts-rs-banned).
    // It exports through audit.generated.ts as a generic TS type.
    //
    // Discriminating: move the carrier to verter_protocol, or drop it from
    // audit.generated.ts, and this guard fails.

    // (1) The carrier is declared in verter_audit.
    let audit_src = read_workspace_file("crates/verter_audit/src/audited_result.rs");
    assert!(
        audit_src.contains("pub enum AuditedResult<T, E>"),
        "AuditedResult<T, E> must be declared in crates/verter_audit/src/audited_result.rs",
    );
    // It rides the ts-rs export path (the same `audit.generated.ts` target
    // the rest of the audit substrate uses), NOT a bare `#[ts(export)]`.
    assert!(
        audit_src.contains("ts_rs::TS")
            && audit_src.contains("#[ts(export_to = \"audit.generated.ts\")]"),
        "AuditedResult must derive ts_rs::TS with `#[ts(export_to = \"audit.generated.ts\")]`",
    );
    assert!(
        !audit_src.contains("#[ts(export)]") && !audit_src.contains("#[ts(export,"),
        "AuditedResult must NOT use the bare `#[ts(export)]` flag (it resurrects the \
         concurrent-truncation bug); use `#[ts(export_to = ...)]` only",
    );

    // (2) The carrier is exported from the verter_audit crate root.
    let audit_lib = read_workspace_file("crates/verter_audit/src/lib.rs");
    assert!(
        audit_lib.contains("pub mod audited_result;")
            && audit_lib.contains("pub use audited_result::AuditedResult;"),
        "verter_audit must declare and re-export the audited_result module",
    );

    // (3) It is NOT declared in verter_protocol (the protobuf-authoritative
    // crate is ts-rs-banned; a generic carrier embedding RequestAuditRecord
    // cannot live there).
    let protocol_typeinfo_dir = workspace_root().join("crates/verter_protocol/src/typeinfo");
    let bad_path = protocol_typeinfo_dir.join("audited_result.rs");
    assert!(
        !bad_path.exists(),
        "AuditedResult must NOT live at crates/verter_protocol/src/typeinfo/audited_result.rs — \
         verter_protocol is protobuf-authoritative and ts-rs-banned; the carrier belongs in \
         verter_audit",
    );
    // Belt-and-suspenders: no `AuditedResult` declaration anywhere under
    // verter_protocol/src.
    let protocol_src = workspace_root().join("crates/verter_protocol/src");
    let mut protocol_files = Vec::new();
    collect_rs_files(&protocol_src, &mut protocol_files);
    for file in protocol_files {
        let contents = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(
            !contents.contains("enum AuditedResult") && !contents.contains("struct AuditedResult"),
            "AuditedResult must NOT be declared under verter_protocol/src ({}) — it lives in \
             verter_audit",
            file.display(),
        );
    }

    // (4) It exports through audit.generated.ts as a generic TS type.
    let generated = read_workspace_file("packages/types/audit.generated.ts");
    assert!(
        generated.contains("export type AuditedResult<T, E> ="),
        "AuditedResult<T, E> must export as a generic type into \
         packages/types/audit.generated.ts (regenerate via \
         `VERTER_UPDATE_TS_BINDINGS=1 cargo test -p verter_session --test g_misc1 \
         audit_ts_bindings_are_in_sync` after a schema change)",
    );
    // The generated carrier references the audit record (its embedded
    // field), proving it rode the audit ts-rs graph rather than a
    // hand-authored mirror.
    assert!(
        generated.contains("audit: RequestAuditRecord"),
        "the generated AuditedResult must carry `audit: RequestAuditRecord` — it must \
         ride the audit ts-rs export graph, not a hand-written TS mirror",
    );
}

/// Recursively collect `*.rs` files under `dir`.
fn collect_rs_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}
