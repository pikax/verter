//! The identity bindings a bundler plugin reads instead of keeping its own
//! copies: id canonicalisation, the registered content hash, the block
//! content token, package classification, the resolution request kind, the
//! request-id grammar, and the encoding of the document structure's ranges.

use std::sync::Arc;

use super::*;
use verter_workspace::{MemoryOptions, MemoryWorkspace};

fn standalone_host() -> NapiVerterHost {
    NapiVerterHost {
        inner: Arc::new(host::VerterHost::new_standalone(host::HostConfig::default())),
    }
}

fn upsert_request(input_id: &str, source: &str) -> NapiUpsertRequest {
    NapiUpsertRequest {
        canonicalId: None,
        inputId: input_id.to_owned(),
        source: Buffer::from(source.as_bytes().to_vec()),
        fileKind: None,
        aliases: None,
    }
}

#[test]
fn canonicalize_id_answers_the_host_canonical_form() {
    for (raw, expected) in [
        ("C:\\Proj\\src\\App.vue", "c:/Proj/src/App.vue"),
        ("/proj/App.vue?vue&type=script&lang.ts", "/proj/App.vue"),
        ("//?/C:/proj/App.vue", "c:/proj/App.vue"),
        ("//?/UNC/srv/share/App.vue", "//srv/share/App.vue"),
        ("\\\\?\\UNC\\srv\\share\\App.vue", "//srv/share/App.vue"),
    ] {
        assert_eq!(canonicalize_id(raw.to_owned()), expected, "raw id {raw:?}");
    }
}

#[test]
fn parse_request_id_answers_the_grammar_without_a_host() {
    let parsed = match parse_request_id("./Comp.vue?vue&type=i18n&index=1&lang.json".to_owned()) {
        Either::A(parsed) => parsed,
        Either::B(rejected) => panic!("a well-formed request parses: {}", rejected.rejected),
    };
    assert_eq!(parsed.nodeKind.kind, "custom");
    assert_eq!(parsed.nodeKind.index, Some(1));
    assert_eq!(parsed.marker.as_deref(), Some("vue"));
    assert_eq!(parsed.lang.as_deref(), Some("json"));
    assert_eq!(parsed.blockType.as_deref(), Some("i18n"));
    assert_eq!(parsed.rawPath, "./Comp.vue");
    assert!(!parsed.scoped);

    match parse_request_id("./Comp.vue?vue&lang.ts".to_owned()) {
        Either::B(rejected) => {
            assert_eq!(rejected.rejected, "marker-without-type");
            assert_eq!(rejected.rawPath, "./Comp.vue");
            assert_eq!(rejected.marker.as_deref(), Some("vue"));
        }
        Either::A(parsed) => panic!("a marker without type parsed: {}", parsed.rawPath),
    }
}

#[test]
fn render_bundler_id_writes_the_carrier_marker_and_language() {
    let napi_host = NapiVerterHost::new(Some(NapiHostConfig {
        carrierExtensions: Some(vec![NapiCarrierExtension {
            suffix: ".md".to_string(),
            framework: "vue".to_string(),
        }]),
        ..NapiHostConfig::default()
    }))
    .expect("md as a Vue carrier");
    let render = |canonical_id: &str, kind: &str, index: Option<u32>, lang: Option<&str>| {
        napi_host
            .render_bundler_id(
                canonical_id.to_string(),
                NapiVirtualNodeKind {
                    kind: kind.to_string(),
                    index,
                },
                lang.map(str::to_string),
            )
            .expect("a known node kind renders")
    };
    assert_eq!(
        render("/src/App.vue", "script", None, Some("ts")),
        "/src/App.vue?vue&type=script&lang.ts"
    );
    assert_eq!(
        render("/src/App.vue", "style", Some(0), Some("module.css")),
        "/src/App.vue?vue&type=style&index=0&lang.module.css"
    );
    assert_eq!(
        render("/src/Card.svelte", "style", Some(0), Some("css")),
        "/src/Card.svelte?verter&type=style&index=0&lang.css"
    );
    assert_eq!(
        render("/docs/Guide.md", "template", None, None),
        "/docs/Guide.md?vue&type=template"
    );
}

#[test]
fn upsert_reports_one_content_hash_per_content() {
    let napi_host = standalone_host();
    let first = napi_host
        .upsert(upsert_request("/src/a.ts", "export const a = 1;\n"))
        .expect("first upsert");
    let same_bytes = napi_host
        .upsert(upsert_request("/src/b.ts", "export const a = 1;\n"))
        .expect("second upsert");
    let other_bytes = napi_host
        .upsert(upsert_request("/src/c.ts", "export const c = 2;\n"))
        .expect("third upsert");

    assert_eq!(first.contentHash.len(), 32, "{}", first.contentHash);
    assert!(
        first
            .contentHash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{}",
        first.contentHash
    );
    assert_eq!(first.contentHash, same_bytes.contentHash);
    assert_ne!(first.contentHash, other_bytes.contentHash);
}

#[test]
fn hash_block_content_answers_the_host_block_token() {
    let code = ".a { color: red; }\n";
    let token = hash_block_content(Buffer::from(code.as_bytes().to_vec())).expect("utf-8 input");
    assert_eq!(token, host::hash_block_content(code).into_string());
    assert!(token.starts_with("sha256:"), "{token}");
}

#[test]
fn workspace_classifies_package_backed_files() {
    let workspace = NapiWorkspace::new(vec!["/proj".to_owned()]);
    let package_file = workspace.is_package_backed("/proj/node_modules/lodash/index.js".into());
    let source_file = workspace.is_package_backed("/proj/src/a.ts".into());
    assert!(package_file);
    assert!(!source_file);
}

#[test]
fn resolve_import_answers_the_declaration_for_a_type_request() {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let player = "export {};\n";
    let script = "export const v = 1;\n";
    let declaration = "export declare const v: number;\n";
    workspace.inject_file("/src/player.ts".to_owned(), Arc::from(player));
    workspace.inject_file("/src/video.js".to_owned(), Arc::from(script));
    workspace.inject_file("/src/video.d.ts".to_owned(), Arc::from(declaration));
    let napi_host = NapiVerterHost {
        inner: Arc::new(host::VerterHost::new(
            host::HostConfig::default(),
            workspace,
        )),
    };
    napi_host
        .upsert(upsert_request("/src/player.ts", player))
        .expect("importer upsert");

    assert_eq!(
        napi_host.resolve_import_napi(
            "/src/player.ts".to_owned(),
            "./video.js".to_owned(),
            None,
            Some("type".to_owned()),
        ),
        Some("/src/video.d.ts".to_owned()),
    );
    assert_eq!(
        napi_host.resolve_import_napi(
            "/src/player.ts".to_owned(),
            "./video.js".to_owned(),
            None,
            None,
        ),
        Some("/src/video.js".to_owned()),
    );
}

#[test]
fn resolve_projects_the_request_facets_and_typed_rejections() {
    let napi_host = standalone_host();
    let raw = "/src/App.svelte?verter&type=style&index=0&scoped&lang.module.css";
    let resolved = match napi_host.resolve(raw.to_owned()).expect("resolve") {
        Either::A(resolved) => resolved,
        Either::B(rejected) => panic!("a well-formed request resolves: {}", rejected.rejected),
    };
    assert_eq!(resolved.marker.as_deref(), Some("verter"));
    assert_eq!(resolved.lang.as_deref(), Some("module.css"));
    assert!(resolved.scoped);
    assert_eq!(resolved.rawPath, "/src/App.svelte");
    assert_eq!(
        resolved.bundlerId,
        "/src/App.svelte?verter&type=style&index=0&lang.module.css"
    );

    let rejected = napi_host
        .resolve("/src/App.vue?vue&verter&type=script".to_owned())
        .expect("resolve");
    match rejected {
        Either::B(rejected) => assert_eq!(rejected.rejected, "both-markers"),
        Either::A(resolved) => panic!("both markers resolved: {}", resolved.bundlerId),
    }
}

fn range_ends(structure: &serde_json::Value, encoding: &str, ends: &mut Vec<u64>) {
    match structure {
        serde_json::Value::Object(fields) => {
            if let (Some(end), Some(tag)) = (fields.get("end"), fields.get("encoding")) {
                assert_eq!(tag, encoding, "every range carries the requested encoding");
                ends.push(end.as_u64().expect("numeric range end"));
            }
            for value in fields.values() {
                range_ends(value, encoding, ends);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                range_ends(value, encoding, ends);
            }
        }
        _ => {}
    }
}

fn last_range_end(napi_host: &NapiVerterHost, encoding: Option<&str>, tag: &str) -> u64 {
    let json = napi_host
        .get_document_structure("/src/Wave.vue".to_owned(), encoding.map(str::to_owned))
        .expect("structure projection")
        .expect("registered structure");
    let structure: serde_json::Value = serde_json::from_str(&json).expect("structure JSON");
    let mut ends = Vec::new();
    range_ends(&structure, tag, &mut ends);
    ends.into_iter().max().expect("the structure has ranges")
}

#[test]
fn document_structure_ranges_follow_the_requested_encoding() {
    let source = "<template><p>\u{1F30A}</p></template>\n<script setup>\nconst a = 1\n</script>\n";
    let napi_host = standalone_host();
    napi_host
        .upsert(upsert_request("/src/Wave.vue", source))
        .expect("sfc upsert");
    let script_close = source.rfind("</script>").expect("closing tag") + "</script>".len();

    let utf8_end = last_range_end(&napi_host, None, "utf8.v1");
    assert!(utf8_end >= script_close as u64, "{utf8_end}");
    assert_eq!(
        last_range_end(&napi_host, Some("utf8.v1"), "utf8.v1"),
        utf8_end
    );
    // The wave is four UTF-8 bytes and one surrogate pair.
    assert_eq!(
        last_range_end(&napi_host, Some("utf16.v1"), "utf16.v1"),
        utf8_end - 2
    );

    let refused = napi_host
        .get_document_structure("/src/Wave.vue".to_owned(), Some("utf32".to_owned()))
        .expect_err("an unknown tag is refused");
    assert_eq!(refused.status, Status::InvalidArg);
}
