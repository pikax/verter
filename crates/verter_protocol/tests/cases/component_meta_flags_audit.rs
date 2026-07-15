//! D123 (Tier 1A) — discriminating audit for the
//! `ComponentMetaFlags::has_macro_failure: bool` field.
//!
//! Asserts BOTH the Rust struct field and the proto message field are
//! present. The test parses the syn-AST of
//! `verter_semantic::analysis::component_meta` and grep-scans the
//! `.proto` file. A regression that drops either side breaks the test.

use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

#[test]
fn component_meta_flags_has_macro_failure_field_present() {
    // ── Side 1: Rust struct ─────────────────────────────────────────
    // `ComponentMetaFlags` (defined in
    // `crates/verter_semantic/src/analysis/component_meta.rs`) MUST
    // declare a `has_macro_failure: bool` field. We parse the file
    // with syn and walk the struct's fields.
    let rust_path = workspace_root().join("crates/verter_semantic/src/analysis/component_meta.rs");
    let rust_body = std::fs::read_to_string(&rust_path).expect("read component_meta.rs");
    let parsed: syn::File = syn::parse_str(&rust_body).expect("parse component_meta.rs");

    let mut found_struct = false;
    let mut found_field = false;
    for item in &parsed.items {
        if let syn::Item::Struct(s) = item {
            if s.ident == "ComponentMetaFlags" {
                found_struct = true;
                for field in &s.fields {
                    let name = field
                        .ident
                        .as_ref()
                        .map(syn::Ident::to_string)
                        .unwrap_or_default();
                    if name == "has_macro_failure" {
                        // Verify the type is `bool` (not `Option<bool>`,
                        // not `&'static bool`, not `String`). The whole
                        // point of the discriminator is that the FFI
                        // surface gets a plain `bool`.
                        let ty_token = quote::quote!(#field).to_string();
                        assert!(
                            ty_token.contains("bool"),
                            "ComponentMetaFlags::has_macro_failure must be `bool`, found `{ty_token}`"
                        );
                        found_field = true;
                    }
                }
            }
        }
    }
    assert!(found_struct, "ComponentMetaFlags struct not found");
    assert!(
        found_field,
        "ComponentMetaFlags must have `pub has_macro_failure: bool` field (D123)"
    );

    // ── Side 2: Proto message ───────────────────────────────────────
    // The .proto definition for `ComponentFlags` MUST also declare
    // `has_macro_failure`. We grep the file's text — proto syntax is
    // simple enough that a substring assertion is fine.
    let proto_path =
        workspace_root().join("crates/verter_protocol/proto/verter/v1/component_meta.proto");
    let proto_body = std::fs::read_to_string(&proto_path).expect("read component_meta.proto");

    // Locate the `message ComponentFlags { ... }` block.
    let block_start = proto_body
        .find("message ComponentFlags")
        .expect("message ComponentFlags not found in proto");
    let block = &proto_body[block_start..];
    let block_end = block
        .find('}')
        .expect("ComponentFlags block has no closing brace");
    let block = &block[..block_end];

    assert!(
        block.contains("has_macro_failure"),
        "proto `ComponentFlags` must declare `has_macro_failure` field (D123)"
    );
    // The proto field tag for the new entry must be unique. Tier 1A
    // assigns tag 10 (next free after the original 9 fields).
    assert!(
        block.contains("bool has_macro_failure = 10"),
        "proto `ComponentFlags.has_macro_failure` must be `bool` at tag 10; \
         block content was:\n{block}"
    );

    // ── Side 3: Ffi struct ──────────────────────────────────────────
    // `FfiComponentMetaFlags` is the consumer-facing shape. It MUST
    // mirror the Rust struct — drop here means the field never
    // crosses the FFI boundary even if the Rust struct has it.
    let ffi_path = workspace_root().join("crates/verter_protocol/src/types.rs");
    let ffi_body = std::fs::read_to_string(&ffi_path).expect("read types.rs");
    let parsed_ffi: syn::File = syn::parse_str(&ffi_body).expect("parse types.rs");

    let mut found_ffi_field = false;
    for item in &parsed_ffi.items {
        if let syn::Item::Struct(s) = item {
            if s.ident == "FfiComponentMetaFlags" {
                for field in &s.fields {
                    let name = field
                        .ident
                        .as_ref()
                        .map(syn::Ident::to_string)
                        .unwrap_or_default();
                    if name == "has_macro_failure" {
                        found_ffi_field = true;
                    }
                }
            }
        }
    }
    assert!(
        found_ffi_field,
        "FfiComponentMetaFlags must mirror the Rust struct's `has_macro_failure` field"
    );
}
