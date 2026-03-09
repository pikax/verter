use std::sync::Arc;

use verter_span::Span;

use crate::shared::read_lock;
use crate::{
    CompileErrorPolicy, CompileProfile, FileKind, HostConfig, HostDiagnostic, HostError,
    HostSeverity, UpsertRequest, VerterHost, VirtualNodeKind, VirtualQuery,
};

fn strict_host() -> VerterHost {
    VerterHost::new(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    })
}

fn profile() -> CompileProfile {
    CompileProfile::default()
}

fn upsert_vue(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::VueSfc,
            aliases: Vec::new(),
        })
        .unwrap();
}

fn upsert_non_sfc(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::NonSfc,
            aliases: Vec::new(),
        })
        .unwrap();
}

fn compile_main_error(host: &VerterHost, canonical_id: &str) -> crate::DiagnosticsSnapshot {
    match host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(canonical_id.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile(),
    }) {
        Err(HostError::CompileError { diagnostics }) => diagnostics,
        Err(other) => panic!("expected compile error, got {other:?}"),
        Ok(result) => panic!(
            "expected compile error, got successful response {}",
            result.id
        ),
    }
}

fn find_diag<'a>(diagnostics: &'a crate::DiagnosticsSnapshot, code: &str) -> &'a HostDiagnostic {
    diagnostics
        .diagnostics
        .iter()
        .find(|diag| diag.code == code)
        .unwrap_or_else(|| {
            panic!(
                "expected diagnostic {code}, got {:?}",
                diagnostics.diagnostics
            )
        })
}

fn assert_missing_src_compile_error(
    host: &VerterHost,
    canonical_id: &str,
    source: &str,
    specifier: &str,
    expected_tag: &str,
) {
    let diagnostics = compile_main_error(host, canonical_id);
    let missing = find_diag(&diagnostics, "HOST_MISSING_EXTERNAL_SOURCE");

    assert_eq!(missing.severity, HostSeverity::Error);
    assert!(
        missing.message.contains(specifier),
        "missing external source message should mention {specifier}: {}",
        missing.message
    );
    assert!(
        !missing.message.contains("HOST_MISSING_MACRO_TYPE_DEP"),
        "message should not mention macro type deps: {}",
        missing.message
    );

    let start = source.find(expected_tag).unwrap() as u32;
    let end = source[start as usize..]
        .find('>')
        .map(|offset| start + offset as u32 + 1)
        .unwrap();
    assert_eq!(
        missing.span,
        Some(Span::new(start, end)),
        "missing external source span should point at the owning tag"
    );

    {
        let files = read_lock(&host.files);
        let entry = files.get(canonical_id).unwrap();
        assert!(
            entry.compile_slots.is_empty(),
            "failed compile must not leave cached outputs behind"
        );
    }
    assert!(
        host.get_ide(canonical_id, &profile()).is_none(),
        "failed compile must not expose IDE output"
    );
}

#[test]
fn missing_script_src_produces_compile_error_and_no_outputs() {
    let host = strict_host();
    let source = "<script src=\"./missing.ts\"></script>\n<template><div/></template>";
    upsert_vue(&host, "/src/Comp.vue", source);

    assert_missing_src_compile_error(&host, "/src/Comp.vue", source, "./missing.ts", "<script");
}

#[test]
fn missing_template_src_produces_compile_error_and_no_outputs() {
    let host = strict_host();
    let source = "<template src=\"./missing.html\"></template>\n<script setup>const n = 1</script>";
    upsert_vue(&host, "/src/Comp.vue", source);

    assert_missing_src_compile_error(
        &host,
        "/src/Comp.vue",
        source,
        "./missing.html",
        "<template",
    );
}

#[test]
fn missing_style_src_produces_compile_error_and_no_outputs() {
    let host = strict_host();
    let source = "<template><div/></template>\n<style src=\"./missing.css\"></style>";
    upsert_vue(&host, "/src/Comp.vue", source);

    assert_missing_src_compile_error(&host, "/src/Comp.vue", source, "./missing.css", "<style");
}

#[test]
fn missing_macro_type_dependency_produces_compile_error_and_no_outputs() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport type { Props } from './types'\nconst props = defineProps<Props>()\n</script>\n<template><div/></template>";
    upsert_vue(&host, "/src/Comp.vue", source);

    let diagnostics = compile_main_error(&host, "/src/Comp.vue");
    let missing = find_diag(&diagnostics, "HOST_MISSING_MACRO_TYPE_DEP");

    assert_eq!(missing.severity, HostSeverity::Error);
    assert!(
        missing.message.contains("./types"),
        "macro type dep message should mention the missing import source: {}",
        missing.message
    );
    assert!(
        !missing.message.contains("HOST_MISSING_EXTERNAL_SOURCE"),
        "macro type dep message should not be reported as an external src failure: {}",
        missing.message
    );

    let import_start = source.find("import type").unwrap() as u32;
    let import_end = import_start + "import type { Props } from './types'".len() as u32;
    assert_eq!(
        missing.span,
        Some(Span::new(import_start, import_end)),
        "macro type dep span should point at the owning import"
    );

    {
        let files = read_lock(&host.files);
        let entry = files.get("/src/Comp.vue").unwrap();
        assert!(
            entry.compile_slots.is_empty(),
            "failed macro type dep compile must not cache outputs"
        );
    }
    assert!(
        host.get_ide("/src/Comp.vue", &profile()).is_none(),
        "failed macro type dep compile must not expose IDE output"
    );
}

#[test]
fn missing_template_src_retries_successfully_after_dependency_arrives() {
    let host = strict_host();
    let source =
        "<template src=\"./resolved.html\"></template>\n<script setup>const n = 1</script>";
    upsert_vue(&host, "/src/Comp.vue", source);

    let diagnostics = compile_main_error(&host, "/src/Comp.vue");
    assert!(
        diagnostics
            .diagnostics
            .iter()
            .any(|diag| diag.code == "HOST_MISSING_EXTERNAL_SOURCE"),
        "first compile should fail on the missing template source"
    );

    upsert_non_sfc(&host, "/src/resolved.html", "<div>resolved</div>");

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("compile should succeed once the external template source exists");

    assert!(
        !response.diagnostics.has_errors,
        "resolved compile should not keep the old missing-source error"
    );
    assert!(
        response.code.contains("render"),
        "resolved compile should produce code, got: {}",
        response.code
    );
    assert!(
        !response.code.contains("missing external source"),
        "resolved compile output must not contain the previous error text"
    );
}

#[test]
fn missing_macro_type_dependency_retries_successfully_after_dependency_arrives() {
    let host = strict_host();
    let source = "<script setup lang=\"ts\">\nimport type { Props } from './types'\nconst props = defineProps<Props>()\n</script>\n<template><div>{{ props.msg }}</div></template>";
    upsert_vue(&host, "/src/Comp.vue", source);

    let diagnostics = compile_main_error(&host, "/src/Comp.vue");
    assert!(
        diagnostics
            .diagnostics
            .iter()
            .any(|diag| diag.code == "HOST_MISSING_MACRO_TYPE_DEP"),
        "first compile should fail on the missing macro type dependency"
    );

    upsert_non_sfc(
        &host,
        "/src/types.ts",
        "export interface Props { msg: string }",
    );

    let response = host
        .get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some("/src/Comp.vue".to_string()),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile(),
        })
        .expect("compile should succeed once the macro type dependency exists");

    assert!(
        !response.diagnostics.has_errors,
        "resolved compile should not keep the old missing macro type error"
    );
    assert!(
        response.code.contains("export default"),
        "resolved compile should produce a main module, got: {}",
        response.code
    );
    assert!(
        !response.code.contains("HOST_MISSING_MACRO_TYPE_DEP"),
        "resolved compile output must not contain the previous missing-type error"
    );
}
