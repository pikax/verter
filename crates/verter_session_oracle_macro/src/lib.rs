//! `#[oracle_row]` — the self-keyed attribute proc-macro for lifted TS7
//! `TypeExpr`-projection rows (`docs/arch/u0-oracle-harness-design.md` §Q4).
//!
//! A lifted row body must call the shared registry driver keyed by the row's
//! OWN `(file, function)`. A hand-typed key string is FORBIDDEN: if a `foo_test`
//! body mistyped `oracle::run_row(file!(), "bar_test")`, every coverage /
//! biconditional / count guard would still pass (the row-ref it runs against is a
//! real, fully-covered registry key) while `foo_test` silently validates
//! `bar_test`'s snapshots — a wrong-row execution no downstream guard catches. A
//! no-arg declarative `oracle_row!()` macro CANNOT fix this either: a
//! body-position declarative macro is given no access to the enclosing `fn`
//! name.
//!
//! The mechanism is therefore an ATTRIBUTE proc-macro placed ON the test fn: it
//! receives the whole `ItemFn` (including its identifier `sig.ident`), reads the
//! fn's OWN name, and synthesizes the body
//! `oracle::run_row(file!(), "<sig.ident>")` with the key emitted FROM the
//! parsed identifier — never typed by the author. So a `foo_test` cannot name
//! `bar_test`'s key; the wrong-row-execution class is foreclosed at the macro
//! boundary.
//!
//! This crate is a DEV-dependency of `verter_session` (lifted bodies are
//! `#[cfg(test)]` unit tests), so it never enters the production build or the
//! resolver dep closure — preserving the `tsgo`-forbidden-at-runtime invariants.

use proc_macro::TokenStream;
use quote::quote;
use syn::ItemFn;

/// Replace a test fn's body with the self-keyed shared-driver call. The key is
/// the fn's OWN `sig.ident` — never a hand-typed string. `#[oracle_row]` is the
/// OUTER attribute, so it expands first and re-emits the fn carrying its other
/// attributes (notably `#[test]`), which the test harness then sees.
#[proc_macro_attribute]
pub fn oracle_row(_attr: TokenStream, item: TokenStream) -> TokenStream {
    match syn::parse::<ItemFn>(item) {
        Ok(item_fn) => synthesize_body(&item_fn).into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// The pure token transform (separated from the proc-macro ABI entry so it is
/// unit-testable directly on a parsed `ItemFn`). Preserves the fn's attributes,
/// visibility, and signature; REPLACES the body with the synthesized driver
/// call keyed by the fn's own identifier.
//
// (Named `synthesize_body` rather than the bare verb `expand` so it does not
//  collide with the `expand` symbol the guard
//  `expand_variant_and_expand_mode_absent_from_workspace` asserts is absent
//  from the workspace — an unrelated symbol.)
fn synthesize_body(item_fn: &ItemFn) -> proc_macro2::TokenStream {
    let attrs = &item_fn.attrs;
    let vis = &item_fn.vis;
    let sig = &item_fn.sig;
    // The SELF key — sourced from the fn's own identifier token, never authored.
    let fn_name = sig.ident.to_string();
    quote! {
        #(#attrs)*
        #vis #sig {
            oracle::run_row(file!(), #fn_name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::synthesize_body;

    fn expand_str(src: &str) -> String {
        let item_fn: syn::ItemFn = syn::parse_str(src).expect("parse fn");
        synthesize_body(&item_fn).to_string()
    }

    #[test]
    fn expand_emits_self_keyed_run_row() {
        let out = expand_str("fn composed_props_expands() {}");
        assert!(
            out.contains("run_row"),
            "must call the shared driver: {out}"
        );
        assert!(out.contains("file !"), "must pass file!(): {out}");
        assert!(
            out.contains("\"composed_props_expands\""),
            "key must be the fn's own ident: {out}"
        );
    }

    #[test]
    fn expand_preserves_test_attribute() {
        // `#[oracle_row]` is outer, so the `#[test]` it receives must be re-emitted
        // for the harness to see the test.
        let out = expand_str("#[test] fn foo_test() {}");
        assert!(out.contains("# [test]"), "must preserve #[test]: {out}");
        assert!(out.contains("\"foo_test\""));
    }

    #[test]
    fn expand_key_is_the_fn_ident_not_a_literal() {
        // Discriminating: the key is SOURCED from sig.ident, so two differently
        // named fns get two different keys — a `foo` body can never name `bar`.
        let foo = expand_str("fn foo() {}");
        let bar = expand_str("fn bar() {}");
        assert!(foo.contains("\"foo\"") && !foo.contains("\"bar\""));
        assert!(bar.contains("\"bar\"") && !bar.contains("\"foo\""));
    }
}
