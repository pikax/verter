//! The published TypeScript declaration of the host compile request is
//! byte-pinned to the Rust declarations that decode it.
//!
//! `packages/native/host-compile-request.generated.ts` is rendered from
//! the framework-tagged and product-tagged request unions and the DTOs
//! their arms carry. Nothing binds a hand-written declaration to a schema
//! it sits beside: deleting a field, mis-casing a key, adding a slot the
//! decoder has no room for, or turning a required slot optional all type
//! check and then refuse at run time. This pin is that binding.
//!
//! It renders IN MEMORY and byte-compares. It never writes: a check that
//! regenerates the file it is checking can only ever pass. Regenerate
//! deliberately, with `pnpm gen:host-request-ts`.
//!
//! The decode round-trips below are the other half. The render names each
//! arm's tag and each struct arm's wire keys; those names are only true if
//! the decoder answers to them, so each arm is decoded from a payload
//! built out of the rendered metadata and matched against the variant it
//! claims. A tag or key that drifts from serde's own fails here rather
//! than shipping a declaration the decoder refuses.

use std::path::PathBuf;

use serde_json::{json, Map, Value};

use verter_napi::host_compile_request_ts::{
    render_host_compile_request_ts, ArmShape, TaggedUnion, HOST_COMPILE_REQUEST_TS_PATH,
};
use verter_napi::{decode_host_compile_request, NapiHostCompileRequest, NapiRequestedProduct};

/// The workspace root, from this crate's manifest dir
/// (`<workspace>/crates/verter_napi`).
fn workspace_root() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .expect("CARGO_MANIFEST_DIR is `<workspace>/crates/verter_napi`")
        .to_path_buf()
}

#[test]
fn generated_declaration_is_byte_equal_to_the_rendered_rust_schema() {
    let rendered = render_host_compile_request_ts();
    let committed_path = workspace_root().join(HOST_COMPILE_REQUEST_TS_PATH);
    let committed = std::fs::read_to_string(&committed_path).unwrap_or_else(|error| {
        panic!(
            "`{}` must be readable: {error}. Generate it with \
             `pnpm gen:host-request-ts`.",
            committed_path.display()
        )
    });

    assert!(
        committed == rendered,
        "`{HOST_COMPILE_REQUEST_TS_PATH}` no longer matches the Rust declarations that decode \
         the host compile request. Regenerate it with `pnpm gen:host-request-ts` and commit \
         the result.\n{}",
        first_difference(&committed, &rendered),
    );
}

/// Report the first differing line rather than two whole files: the pin
/// fails on one structural change at a time, and naming it is what makes
/// the failure actionable.
fn first_difference(committed: &str, rendered: &str) -> String {
    let mut committed_lines = committed.lines();
    let mut rendered_lines = rendered.lines();
    let mut line = 1;
    loop {
        match (committed_lines.next(), rendered_lines.next()) {
            (None, None) => return "the two renders are equal line for line".to_string(),
            (left, right) if left == right => line += 1,
            (left, right) => {
                return format!(
                    "first difference at line {line}:\n  committed: {}\n  rendered:  {}",
                    left.unwrap_or("<end of file>"),
                    right.unwrap_or("<end of file>"),
                )
            }
        }
    }
}

#[test]
fn rendering_is_deterministic() {
    assert_eq!(
        render_host_compile_request_ts(),
        render_host_compile_request_ts(),
        "the render must not depend on environment or iteration order",
    );
}

// ── the rendered metadata answers to the decoder ─────────────────────────

/// The product tag each variant answers to, matched exhaustively so a new
/// product cannot be added without deciding what it is called here.
fn product_variant_tag(product: &NapiRequestedProduct) -> &'static str {
    match product {
        NapiRequestedProduct::RuntimeClient(_) => "runtimeClient",
        NapiRequestedProduct::RuntimeServer(_) => "runtimeServer",
        NapiRequestedProduct::IdeCompanion(_) => "ideCompanion",
        NapiRequestedProduct::PublicApi {} => "publicApi",
        NapiRequestedProduct::Declarations {} => "declarations",
        NapiRequestedProduct::Analysis(_) => "analysis",
    }
}

/// The framework tag each variant answers to, matched exhaustively.
fn request_variant_tag(request: &NapiHostCompileRequest) -> &'static str {
    match request {
        NapiHostCompileRequest::Vue { .. } => "vue",
        NapiHostCompileRequest::Svelte { .. } => "svelte",
    }
}

/// A value the named projected type accepts, for building a decode probe
/// out of the rendered metadata rather than out of a second hand-written
/// shape.
fn sample_for(ts_type: &str) -> Value {
    match ts_type {
        "boolean" => json!(true),
        "string" => json!("x"),
        "HostRuntimeProductOptions" => json!({ "runtimeSourceMap": true }),
        "HostIdeProductOptions" => json!({
            "wantSourceMap": true,
            "embedAmbientTypes": false,
            "conditionalRootNarrowing": false,
            "strictSlots": false,
            "ideChunkBoundaries": false,
        }),
        "HostAnalysisProductOptions" => {
            json!({ "wantScriptBindings": true, "wantTemplateData": false })
        }
        "HostCompileIdentity" => json!({ "isProduction": false, "forceJs": false }),
        "Array<HostRequestedProduct>" => json!([{ "kind": "publicApi" }]),
        "HostVueCompileOptions" => json!({
            "backend": "inferred",
            "ssr": false,
            "isCustomElement": [],
            "babelParserPlugins": [],
        }),
        "HostSvelteCompileOptions" => json!({}),
        other => panic!("the probe has no sample for the projected type `{other}`"),
    }
}

/// Build the JSON payload one rendered arm describes: its tag under the
/// union's tag key, plus whatever its shape says it carries.
fn probe_payload(union: &TaggedUnion, arm_index: usize) -> Value {
    let arm = &union.arms[arm_index];
    let mut object = Map::new();
    object.insert(union.tag_key.to_string(), json!(arm.tag_value));
    match &arm.shape {
        ArmShape::Payload(payload) => {
            let Value::Object(fields) = sample_for(payload) else {
                panic!("an intersected payload must project as an object");
            };
            object.extend(fields);
        }
        ArmShape::Fields(fields) => {
            for field in fields {
                object.insert(field.name.to_string(), sample_for(&field.ts_type));
            }
        }
    }
    Value::Object(object)
}

#[test]
fn every_rendered_product_arm_decodes_to_the_variant_it_names() {
    let cfg = ts_rs::Config::new();
    let union = NapiRequestedProduct::ts_union(&cfg);
    assert!(!union.arms.is_empty(), "the product union must have arms");

    for index in 0..union.arms.len() {
        let payload = probe_payload(&union, index);
        let request = json!({
            "framework": "vue",
            "identity": sample_for("HostCompileIdentity"),
            "products": [payload.clone()],
            "options": sample_for("HostVueCompileOptions"),
        });
        let decoded = decode_host_compile_request(request).unwrap_or_else(|error| {
            panic!(
                "the rendered arm `{}` describes a payload the decoder refuses: {error}\n{payload}",
                union.arms[index].ts_name
            )
        });
        let NapiHostCompileRequest::Vue { products, .. } = decoded else {
            panic!("a `vue` request must decode to the Vue arm");
        };
        assert_eq!(
            product_variant_tag(&products[0]),
            union.arms[index].tag_value,
            "the rendered arm `{}` names a tag the decoder routes elsewhere",
            union.arms[index].ts_name,
        );
    }
}

#[test]
fn every_rendered_request_arm_decodes_to_the_variant_it_names() {
    let cfg = ts_rs::Config::new();
    let union = NapiHostCompileRequest::ts_union(&cfg);
    assert!(!union.arms.is_empty(), "the request union must have arms");

    for index in 0..union.arms.len() {
        let payload = probe_payload(&union, index);
        let decoded = decode_host_compile_request(payload.clone()).unwrap_or_else(|error| {
            panic!(
                "the rendered arm `{}` describes a payload the decoder refuses: {error}\n{payload}",
                union.arms[index].ts_name
            )
        });
        assert_eq!(
            request_variant_tag(&decoded),
            union.arms[index].tag_value,
            "the rendered arm `{}` names a tag the decoder routes elsewhere",
            union.arms[index].ts_name,
        );
    }
}

#[test]
fn a_rendered_wire_key_the_decoder_does_not_know_is_refused() {
    // The round-trips above pass because the rendered keys are the
    // decoder's. This is the other direction: a key that is NOT one of
    // them is refused, so those probes are proving something.
    let cfg = ts_rs::Config::new();
    let union = NapiHostCompileRequest::ts_union(&cfg);
    let Value::Object(mut payload) = probe_payload(&union, 0) else {
        panic!("a request probe is an object");
    };
    let renamed = payload
        .remove("identity")
        .expect("the Vue arm has identity");
    payload.insert("ident".to_string(), renamed);

    let error = decode_host_compile_request(Value::Object(payload))
        .expect_err("a wire key the decoder does not declare must be refused");
    assert!(
        error.reason.contains("ident"),
        "the refusal must name the key it refused, got: {}",
        error.reason,
    );
}
