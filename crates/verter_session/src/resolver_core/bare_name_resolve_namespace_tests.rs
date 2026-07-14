//! Isolation tests for the content-free namespace-sibling resolver hook
//! ([`super::resolve_namespace_sibling_in_scope`]).
//!
//! Each builds a SYNTHETIC [`ShallowFileState`] from source and drives the
//! hook with a [`LocalScopePayload::Namespace`] scope descriptor, asserting it
//! reconstructs the qualified sibling identity (`NS.Sib`) from the SHALLOW
//! type/value headers — NO sibling map on the carrier, NO body lowered. The
//! three origin cases mirror the eager `add_namespace_sibling_resolutions`
//! rules exactly; the hook is exercised WITHOUT changing that eager path.

use std::sync::Arc;

use super::resolve_namespace_sibling_in_scope;
use crate::resolver_core::ShallowFileState;
use crate::semantic_query::{LocalScopeOrigin, LocalScopePayload};

fn state_from(source: &str) -> Arc<ShallowFileState> {
    ShallowFileState::service_backed_for_test(source)
}

fn namespace(prefix: &str, origin: LocalScopeOrigin) -> LocalScopePayload {
    LocalScopePayload::Namespace {
        prefix: Arc::from(prefix),
        origin,
    }
}

#[test]
fn file_scope_namespace_binds_type_and_value_siblings() {
    // A file-scope `namespace M { ... }`: the shallow inventory indexes the
    // members under their qualified `M.<name>` names.
    let state = state_from(
        "export namespace M {\n\
         export type Inner = { a: 1 }\n\
         export type Outer = Inner\n\
         export const helper = 0\n\
         }\n",
    );

    // A TYPE sibling binds.
    let inner = resolve_namespace_sibling_in_scope(
        &namespace("M", LocalScopeOrigin::File),
        &state,
        "/src/nst.ts",
        "Inner",
    )
    .expect("the file-scope namespace must bind the direct TYPE sibling `Inner`");
    assert_eq!(
        (inner.canonical_id.as_str(), inner.symbol_name.as_str()),
        ("/src/nst.ts", "M.Inner"),
        "a bare `Inner` inside `namespace M` resolves to the qualified `M.Inner`",
    );

    // A VALUE sibling binds too (file-scope origin binds both rails).
    let helper = resolve_namespace_sibling_in_scope(
        &namespace("M", LocalScopeOrigin::File),
        &state,
        "/src/nst.ts",
        "helper",
    )
    .expect("the file-scope namespace must bind the direct VALUE sibling `helper`");
    assert_eq!(
        (helper.canonical_id.as_str(), helper.symbol_name.as_str()),
        ("/src/nst.ts", "M.helper"),
    );

    // DISCRIMINATING: a name that is NOT a member of `M` does NOT bind.
    assert!(
        resolve_namespace_sibling_in_scope(
            &namespace("M", LocalScopeOrigin::File),
            &state,
            "/src/nst.ts",
            "NotAMember",
        )
        .is_none(),
        "a non-member name must NOT bind to a fabricated sibling identity",
    );

    // DISCRIMINATING: a dotted name (a deeper `M.Sub.X`) is not reachable as a
    // bare name from `M`'s own members.
    assert!(
        resolve_namespace_sibling_in_scope(
            &namespace("M", LocalScopeOrigin::File),
            &state,
            "/src/nst.ts",
            "Sub.X",
        )
        .is_none(),
        "a dotted name is not a single-segment direct sibling",
    );
}

#[test]
fn global_augmentation_namespace_binds_global_type_siblings_only() {
    // A `declare global { namespace JSX { ... } }`: the inner type members are
    // retained under `(Global, "JSX.<name>")`, never in file-scope symbols.
    let state = state_from(
        "export {}\n\
         declare global {\n\
         namespace JSX {\n\
         interface Element { tag: string }\n\
         }\n\
         }\n",
    );

    let element = resolve_namespace_sibling_in_scope(
        &namespace("JSX", LocalScopeOrigin::Global),
        &state,
        "/src/jsx.ts",
        "Element",
    )
    .expect("the global-augmentation namespace must bind the global TYPE sibling `Element`");
    assert_eq!(
        (element.canonical_id.as_str(), element.symbol_name.as_str()),
        ("/src/jsx.ts", "JSX.Element"),
    );

    // DISCRIMINATING: the global sibling is NOT visible under File origin (the
    // members never enter file-scope symbols, only the Global augmentation
    // inventory) — proving the origin gate actually discriminates.
    assert!(
        resolve_namespace_sibling_in_scope(
            &namespace("JSX", LocalScopeOrigin::File),
            &state,
            "/src/jsx.ts",
            "Element",
        )
        .is_none(),
        "a File-origin lookup must NOT see a global-augmentation namespace sibling",
    );
}

#[test]
fn module_augmentation_namespace_binds_nothing() {
    // A `declare module "X" { namespace NS { ... } }`: no consumable
    // module-scope sibling is addressable today, so the hook binds nothing.
    let state = state_from(
        "export {}\n\
         declare module \"ext\" {\n\
         namespace NS {\n\
         interface Thing { v: number }\n\
         }\n\
         }\n",
    );

    assert!(
        resolve_namespace_sibling_in_scope(
            &namespace("NS", LocalScopeOrigin::Module),
            &state,
            "/src/mod.ts",
            "Thing",
        )
        .is_none(),
        "a Module-origin namespace binds NO sibling (no consumable module-scope slot today)",
    );
}
