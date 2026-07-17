//! Tests for the plain-script self-file projection and the JSX-dialect
//! fallback (`DocumentRegistry::is_jsx` / `is_jsx_for_canonical`).

use super::*;

/// A plain TS-family script is NOT a carrier either: did_open must build a
/// SELF-FILE projection whose zero-prelude mapper is the identity (its
/// provider buffer is the source bytes verbatim). Without the projection,
/// every provider-backed feature (hover/definition/completion/diagnostics)
/// fails closed for plain scripts.
#[test]
fn did_open_plain_script_builds_self_file_projection_with_identity_mapping() {
    use provider_projection::DocumentProviderProjection;
    use verter_span::{LspPosition, TsPosition};

    let host = Arc::new(verter_session::VerterHost::new_standalone(
        verter_session::HostConfig::default(),
    ));
    let registry = DocumentRegistry::new(Arc::clone(&host));

    let uri: Uri = "file:///x/plain-control.ts".parse().expect("uri");
    let _ = registry.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "typescript".to_string(),
        version: 1,
        text: "export const plainControlNumber = 1;\nplainControlNumber.toFixed(0);\n".to_string(),
    });

    let projection = registry
        .get_projection(&uri)
        .expect("a plain .ts script must build a self-file provider projection");
    let mapper = match &projection {
        DocumentProviderProjection::SelfFile { mapper } => mapper.clone(),
        DocumentProviderProjection::CarrierIde { .. } => {
            panic!("a plain .ts script is NOT a carrier — must be a SelfFile projection")
        }
    };
    assert_eq!(
        mapper.prelude_line_count(),
        0,
        "a plain script's provider buffer is verbatim — no prelude offset"
    );

    // Identity mapping in both directions (zero prelude, no rewrites).
    let prov = registry
        .get_position_mapper(&uri)
        .expect("unified mapper")
        .carrier_to_tsx(LspPosition::new(1, 3))
        .expect("source maps to provider");
    assert_eq!(prov.pos, TsPosition::new(1, 3));
    let back = registry
        .get_position_mapper(&uri)
        .expect("unified mapper")
        .tsx_to_carrier(TsPosition::new(1, 3))
        .expect("provider maps back");
    assert_eq!(back.pos, LspPosition::new(1, 3));
}

/// An unknown extension (no registered language row) must NOT build a
/// provider projection: never serve a non-script document to the
/// TypeScript provider.
#[test]
fn did_open_unknown_extension_builds_no_projection() {
    let host = Arc::new(verter_session::VerterHost::new_standalone(
        verter_session::HostConfig::default(),
    ));
    let registry = DocumentRegistry::new(Arc::clone(&host));

    let uri: Uri = "file:///x/notes.md".parse().expect("uri");
    let _ = registry.did_open(&TextDocumentItem {
        uri: uri.clone(),
        language_id: "markdown".to_string(),
        version: 1,
        text: "# notes\n".to_string(),
    });

    assert!(
        registry.get_projection(&uri).is_none(),
        "an unknown-extension document must not get a provider projection"
    );
}

/// With no compiled IDE output (a cold caller), `is_jsx_for_canonical`
/// must fall back to the parse-level script dialect — a JS carrier is
/// `.jsx` from the start, never a `.tsx` guess that flips later.
#[test]
fn is_jsx_for_canonical_falls_back_to_parse_dialect_without_ide_compile() {
    let host = Arc::new(verter_session::VerterHost::new_standalone(
        verter_session::HostConfig::default(),
    ));
    let registry = DocumentRegistry::new(Arc::clone(&host));

    // A no-lang Svelte component: the parse reports the JS dialect.
    let js_svelte = "<script>\nlet msg = 'hi';\n</script>\n<p>{msg}</p>";
    let _ = host.upsert(verter_session::UpsertRequest {
        canonical_id: Some("/x/JsComp.svelte".to_string()),
        input_id: "/x/JsComp.svelte".to_string(),
        source: Arc::from(js_svelte),
        file_language: verter_session::FileLanguage::svelte(),
        aliases: vec![],
    });
    let ts_vue = "<script setup lang=\"ts\">\nconst msg: string = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>";
    let _ = host.upsert(verter_session::UpsertRequest {
        canonical_id: Some("/x/TsComp.vue".to_string()),
        input_id: "/x/TsComp.vue".to_string(),
        source: Arc::from(ts_vue),
        file_language: verter_session::FileLanguage::vue(),
        aliases: vec![],
    });

    // No IDE compile ran — the parse-level dialect decides.
    let analysis = host.get_analysis("/x/JsComp.svelte");
    assert!(
        analysis.is_some(),
        "analysis must exist for the fallback to consult"
    );
    assert!(
        !analysis.unwrap().is_typescript,
        "a no-lang Svelte script is not TypeScript"
    );
    assert!(
        registry.is_jsx_for_canonical("/x/JsComp.svelte"),
        "a no-lang (JS) Svelte carrier is .jsx without an IDE compile"
    );
    assert!(
        !registry.is_jsx_for_canonical("/x/TsComp.vue"),
        "a lang=ts Vue carrier is .tsx without an IDE compile"
    );
}
