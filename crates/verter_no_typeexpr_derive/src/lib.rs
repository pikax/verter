//! `#[derive(NoTypeExpr)]` — the field-recursive witness emitter for the
//! `verter_no_typeexpr::NoTypeExpr` marker trait.
//!
//! # Why this crate exists
//!
//! The hot prepared-declaration carriers (`crates/verter_session/src/
//! resolver_core/hot_prepared.rs`) must own NO transitive `verter_type_expr::
//! TypeExpr`: every type-body position is an arena handle, never the symbolic
//! IR. The first guard for that invariant was a `syn` SOURCE SCANNER that
//! allow-listed field-type SPELLINGS. It was empirically launderable: planting
//! `use verter_type_expr::TypeExpr as HotBody; field: HotBody` passes the
//! scanner, because it judges the written spelling, not the resolved type.
//!
//! The sound replacement is a COMPILER-checked marker trait. The compiler
//! resolves the actual field type, so an aliased `TypeExpr`-owner (or a nested
//! owner like `ValueRef` / `TupleElement`) FAILS the bound. This derive emits
//! an impl of the HIDDEN witness supertrait `__private::NoTypeExprWitness`
//! (never the public `NoTypeExpr` trait) with a `: NoTypeExpr` bound on every
//! field's type — so the impl only holds when every field is, recursively,
//! `NoTypeExpr`. Emitting the hidden witness (not the public trait) is what
//! keeps the public trait non-hand-implementable downstream: `NoTypeExpr` is
//! blanket-impl'd over every witness, so a hand-written `impl NoTypeExpr for X`
//! would overlap the blanket and be rejected.
//!
//! There is deliberately NO syntactic special-casing for type aliases — that is
//! the whole point: the compiler resolves the real field type, closing the
//! alias / re-export / module-shadow hole the spelling scanner could not.

use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Type};

/// Derive the hidden `NoTypeExpr` witness for a `struct` or `enum`, bounding
/// every field's type on the public `NoTypeExpr` trait. Rejects `union`.
#[proc_macro_derive(NoTypeExpr)]
pub fn derive_no_type_expr(item: TokenStream) -> TokenStream {
    match syn::parse::<DeriveInput>(item) {
        Ok(input) => expand_witness(&input).into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// The pure token transform (separated from the proc-macro ABI entry so it is
/// unit-testable directly on a parsed `DeriveInput`). Emits the hidden-witness
/// impl with a per-field `: ::verter_no_typeexpr::NoTypeExpr` bound, or a
/// `compile_error!` for a `union`.
fn expand_witness(input: &DeriveInput) -> proc_macro2::TokenStream {
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let field_types = match &input.data {
        Data::Struct(data) => fields_types(&data.fields),
        Data::Enum(data) => data
            .variants
            .iter()
            .flat_map(|variant| fields_types(&variant.fields))
            .collect(),
        Data::Union(_) => {
            return quote! {
                ::core::compile_error!("NoTypeExpr cannot be derived for unions");
            };
        }
    };

    // Re-emit the existing where-predicates (if any) and append one
    // `<field-ty>: ::verter_no_typeexpr::NoTypeExpr` bound per field. The public
    // trait is the bound a field author reasons about; the blanket bridge in
    // `verter_no_typeexpr` makes `NoTypeExpr` equivalent to the hidden witness,
    // so bounding on the public trait still drives the recursion.
    let existing_predicates = where_clause.map(|w| {
        let preds = &w.predicates;
        quote! { #preds }
    });
    let field_bounds = field_types.iter().map(|ty| {
        quote! { #ty: ::verter_no_typeexpr::NoTypeExpr }
    });

    quote! {
        #[automatically_derived]
        impl #impl_generics ::verter_no_typeexpr::__private::NoTypeExprWitness
            for #ident #ty_generics
        where
            #existing_predicates
            #(#field_bounds,)*
        {}
    }
}

/// Every field type of a struct / enum-variant field set. Named and unnamed
/// (tuple) fields both contribute their declared type; a unit field set
/// contributes nothing (no per-field bound).
fn fields_types(fields: &Fields) -> Vec<Type> {
    match fields {
        Fields::Named(named) => named.named.iter().map(|f| f.ty.clone()).collect(),
        Fields::Unnamed(unnamed) => unnamed.unnamed.iter().map(|f| f.ty.clone()).collect(),
        Fields::Unit => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::expand_witness;
    use syn::DeriveInput;

    fn expand_str(src: &str) -> String {
        let input: DeriveInput = syn::parse_str(src).expect("parse derive input");
        expand_witness(&input).to_string()
    }

    #[test]
    fn emits_hidden_witness_not_public_trait() {
        let out = expand_str("struct S { a: u32 }");
        // The emitted impl targets the HIDDEN witness — emitting the public
        // `NoTypeExpr` directly would collide with the blanket impl and would
        // let a hand-written impl masquerade as the derive.
        assert!(
            out.contains("NoTypeExprWitness"),
            "must impl the hidden witness, not the public trait: {out}"
        );
        assert!(
            out.contains(":: verter_no_typeexpr :: __private :: NoTypeExprWitness"),
            "must reference the __private witness path: {out}"
        );
    }

    #[test]
    fn named_struct_bounds_every_field_on_public_trait() {
        let out = expand_str("struct S { a: u32, b: String }");
        // Discriminating: BOTH field types must appear bounded on the PUBLIC
        // trait — the bound is what propagates the recursion.
        assert!(
            out.contains("u32 : :: verter_no_typeexpr :: NoTypeExpr"),
            "field `a: u32` must be bounded: {out}"
        );
        assert!(
            out.contains("String : :: verter_no_typeexpr :: NoTypeExpr"),
            "field `b: String` must be bounded: {out}"
        );
    }

    #[test]
    fn tuple_struct_bounds_every_field() {
        let out = expand_str("struct T(u64, bool);");
        assert!(out.contains("u64 : :: verter_no_typeexpr :: NoTypeExpr"));
        assert!(out.contains("bool : :: verter_no_typeexpr :: NoTypeExpr"));
    }

    #[test]
    fn enum_bounds_every_variant_field() {
        // Discriminating: a field hidden inside ANY variant must still gain a
        // bound — otherwise an enum could smuggle a `TypeExpr` through a variant.
        let out = expand_str("enum E { A(u8), B { x: char }, C }");
        assert!(
            out.contains("u8 : :: verter_no_typeexpr :: NoTypeExpr"),
            "tuple-variant field `A(u8)` must be bounded: {out}"
        );
        assert!(
            out.contains("char : :: verter_no_typeexpr :: NoTypeExpr"),
            "named-variant field `B {{ x: char }}` must be bounded: {out}"
        );
    }

    #[test]
    fn unit_struct_emits_impl_with_no_field_bounds() {
        let out = expand_str("struct Unit;");
        assert!(out.contains("NoTypeExprWitness"));
        // A fieldless type is unconditionally a witness — no per-field bound.
        assert!(
            !out.contains(":: verter_no_typeexpr :: NoTypeExpr"),
            "a unit struct must add no `: NoTypeExpr` field bound: {out}"
        );
    }

    #[test]
    fn generic_struct_carries_its_params_and_field_bound() {
        let out = expand_str("struct G<T> { inner: Vec<T> }");
        assert!(
            out.contains("impl < T >"),
            "must carry generic params: {out}"
        );
        assert!(
            out.contains("Vec < T > : :: verter_no_typeexpr :: NoTypeExpr"),
            "the generic field type must be bounded: {out}"
        );
    }

    #[test]
    fn union_is_rejected_with_compile_error() {
        // Discriminating: a union must NOT receive a witness impl — its overlap
        // semantics make field-recursive soundness impossible.
        let out = expand_str("union U { a: u32, b: f32 }");
        assert!(
            out.contains("compile_error"),
            "a union must expand to compile_error!, not a witness impl: {out}"
        );
        assert!(
            !out.contains("NoTypeExprWitness"),
            "a union must NOT receive a witness impl: {out}"
        );
    }
}
