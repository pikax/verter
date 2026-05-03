//! Tests for `OwnedTypeResolutionContext`.
//!
//! Includes the Step 1A discriminating tests:
//! - `owned_type_resolution_context_is_send_sync_static`
//! - `owned_type_resolution_context_has_no_source_field` (D65)

use super::*;
use crate::owned_artifacts::eval_program;
use static_assertions::assert_impl_all;

// `OwnedTypeResolutionContext` MUST be `Send + Sync + 'static`. This is
// the load-bearing property that lets it sit in the host-owned typed
// `TypeResolutionContextDb` instead of in a thread-local cache (D45).
assert_impl_all!(OwnedTypeResolutionContext: Send, Sync);
assert_impl_all!(TypeDeclArena: Send, Sync);
assert_impl_all!(OwnedInterfaceEntry: Send, Sync);
assert_impl_all!(OwnedClassDecl: Send, Sync);
assert_impl_all!(OwnedTypeExpr: Send, Sync);

#[test]
fn owned_type_resolution_context_is_send_sync_static() {
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    assert_send_sync_static::<OwnedTypeResolutionContext>();
    assert_send_sync_static::<TypeDeclArena>();
    assert_send_sync_static::<SpanArena>();
    assert_send_sync_static::<OwnedTypeExpr>();
    assert_send_sync_static::<DeclarationFingerprint>();

    // Constructive guard: build a non-empty context, move it across a
    // thread boundary. If a borrowed lifetime ever sneaks in (e.g.,
    // someone re-introduces `source: &'ctx [u8]`), this fails to
    // compile.
    let ctx = make_minimal_context();
    let handle = std::thread::spawn(move || ctx.type_aliases.len());
    assert_eq!(handle.join().unwrap(), 1);
}

#[test]
fn owned_type_resolution_context_has_no_source_field() {
    // D65 — the `source: &'ctx [u8]` field is DROPPED entirely. The
    // owned form does identifier comparison via `InternedIdentifierId`
    // equality, never via byte-level reread.
    //
    // Discriminating predicate: parse the owned-artifact module's
    // source via `syn` and assert that
    // `pub struct OwnedTypeResolutionContext { … }` has NO field whose
    // name (`source`) and type (`& [u8]` / `&'ctx [u8]` / `&'_ [u8]`)
    // matches the dropped shape. A regression that re-introduces the
    // field would be caught here.
    let path = workspace_root()
        .join("crates/verter_session/src/owned_artifacts/type_resolution_context.rs");
    let body = std::fs::read_to_string(&path).expect("read type_resolution_context.rs");
    let parsed: syn::File = syn::parse_str(&body).expect("parse owned-artifact module");

    let mut found_struct = false;
    for item in &parsed.items {
        if let syn::Item::Struct(s) = item {
            if s.ident == "OwnedTypeResolutionContext" {
                found_struct = true;
                for field in &s.fields {
                    let name = field.ident.as_ref().map(syn::Ident::to_string);
                    if name.as_deref() == Some("source") {
                        let ty = &field.ty;
                        let ty_str = quote::quote!(#ty).to_string();
                        // Whether or not the field's type is &[u8],
                        // having a field literally named `source` on
                        // OwnedTypeResolutionContext violates D65 —
                        // the borrowed form's `source: &'ctx [u8]` was
                        // the discriminating defect.
                        panic!(
                            "OwnedTypeResolutionContext must NOT have a `source` field (D65). Found type `{ty_str}`"
                        );
                    }
                }
            }
        }
    }
    assert!(
        found_struct,
        "OwnedTypeResolutionContext struct not found in source — test self-broken"
    );
}

#[test]
fn declaration_fingerprint_table_present() {
    // Tier 1A introduces the `declaration_fingerprints` table empty;
    // Tier 1B consumes it. Discriminator: the field MUST be present
    // and addressable. A regression that drops the field would break
    // Tier 1B's TypeHandle resolution.
    let ctx = OwnedTypeResolutionContext::empty();
    assert!(ctx.declaration_fingerprints.is_empty());
    // Constructive: insert + lookup roundtrip.
    let mut ctx = ctx;
    let fp = DeclarationFingerprint::from_bytes([0; 16]);
    let id = DeclId::Alias(TypeAliasDeclId(0));
    ctx.declaration_fingerprints.insert(fp, id);
    assert_eq!(ctx.declaration_fingerprints.get(&fp), Some(&id));
}

#[test]
fn decl_arena_indexes_round_trip() {
    let mut arena = TypeDeclArena::new();
    let span = eval_program::SpanId::new(0, 5);
    let body_id = arena.push_expr(OwnedTypeExpr::Unsupported { span });
    let alias = OwnedAliasEntry {
        name: eval_program::InternedIdentifierId(0),
        body: body_id,
        type_params: Vec::new(),
        span,
    };
    let alias_id = arena.push_alias(alias);
    // Discriminator: the alias body MUST point back to the same expr.
    let recovered = arena.alias(alias_id).unwrap();
    assert_eq!(recovered.body, body_id);
    assert!(matches!(
        arena.expr(recovered.body),
        Some(OwnedTypeExpr::Unsupported { .. })
    ));
}

// ─────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn make_minimal_context() -> OwnedTypeResolutionContext {
    let mut ctx = OwnedTypeResolutionContext::empty();
    let alias_id = TypeAliasDeclId(0);
    ctx.type_aliases
        .insert(eval_program::InternedIdentifierId(0), (alias_id, None));
    ctx
}
