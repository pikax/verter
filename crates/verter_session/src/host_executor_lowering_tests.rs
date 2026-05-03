//! Tier 1A — discriminating tests for the host_executor lowering
//! boundary (§3.2.4).
//!
//! Includes:
//! - `lowering_step_drops_oxc_arena_at_boundary`

use crate::owned_artifacts::eval_program::OwnedEvalProgram;
use crate::owned_artifacts::type_resolution_context::OwnedTypeResolutionContext;
use static_assertions::assert_impl_all;

#[test]
fn lowering_step_drops_oxc_arena_at_boundary() {
    // Tier 1A invariant: the post-lowering owned artifact MUST be
    // `Send + Sync + 'static`. The OXC parser arena (`oxc_allocator::Allocator`)
    // is `!Send` because the arena interior uses `Cell`-style mutability
    // and the AST nodes borrow from it via `&'a TSType<'a>` style. If
    // `OwnedEvalProgram` retained any reference to the arena (or any
    // `Rc` / `RefCell`), it would not satisfy `Send + Sync + 'static`
    // and this test would fail to compile.
    //
    // The test discriminates the lowering boundary contract: pre-1A
    // the borrowed `crate::ParsedEvalProgram` self-cell holds the OXC
    // arena alive across cache lifetimes; post-1A the lowering
    // produces `OwnedEvalProgram` and the arena drops at the boundary.
    fn assert_send_sync_static<T: Send + Sync + 'static>() {}
    assert_send_sync_static::<OwnedEvalProgram>();
    assert_send_sync_static::<OwnedTypeResolutionContext>();

    // Cross-thread move: build a non-empty owned artifact, send it
    // across a thread boundary. If a non-Send field were re-introduced
    // (e.g., an `Rc<oxc_allocator::Allocator>` field on
    // `OwnedEvalProgram`), this would fail to compile.
    let program = OwnedEvalProgram::empty();
    let handle = std::thread::spawn(move || program.statements.len());
    assert_eq!(handle.join().unwrap(), 0);

    // Negative discriminator: the borrowed-form `ParsedEvalProgram`
    // (which still exists in 1A as the lowering-input type) is
    // explicitly !Send because of the OXC arena. This is the property
    // the lowering boundary breaks. We assert this compile-time fact
    // for documentation: any future commit that makes
    // `ParsedEvalProgram` Send would be a behavior change worth
    // catching here.
    //
    // We can't directly negative-assert at the type level in Rust
    // without unstable features, but we CAN assert the structural
    // dichotomy: the owned form is Send; the borrowed form holds the
    // arena. The compile-time `assert_impl_all!` for the owned form
    // is the load-bearing check.
    assert_impl_all!(OwnedEvalProgram: Send, Sync);
}

#[test]
fn lowering_boundary_owned_artifact_has_no_arena_lifetime() {
    // Discriminating predicate: parse `eval_program.rs` via syn and
    // verify that `OwnedEvalProgram` carries NO lifetime parameter
    // (`<'a>`, `<'ctx>`, etc.). A borrowed-lifetime parameter would
    // mean the type still references the arena.
    let path = workspace_root().join("crates/verter_session/src/owned_artifacts/eval_program.rs");
    let body = std::fs::read_to_string(&path).expect("read eval_program.rs");
    let parsed: syn::File = syn::parse_str(&body).expect("parse owned-artifact module");

    let mut found = false;
    for item in &parsed.items {
        if let syn::Item::Struct(s) = item {
            if s.ident == "OwnedEvalProgram" {
                found = true;
                let has_lifetime = s.generics.lifetimes().next().is_some();
                assert!(
                    !has_lifetime,
                    "OwnedEvalProgram must have NO lifetime parameter — D44 lowering-boundary invariant"
                );
            }
        }
    }
    assert!(
        found,
        "OwnedEvalProgram struct not found in source — test self-broken"
    );
}

fn workspace_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}
