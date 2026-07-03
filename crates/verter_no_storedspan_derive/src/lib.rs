//! `#[derive(NoStoredSpan)]` — the field-recursive witness emitter for the
//! `verter_no_storedspan::NoStoredSpan` marker trait.
//!
//! # Why this crate exists
//!
//! The closed semantic fact families store no `verter_span::Span` (nor any of
//! the `*Spans` carriers that own spans): a fact is content-free, and a
//! recovered span is reconstructed via a producer-emitted origin locator BEFORE
//! any `Eq`/`Hash`/interning, never stored as a fact field. The sibling
//! `NoTypeExpr` marker cannot enforce that on its own — `Span` IS `NoTypeExpr`,
//! so a stored span slips past it.
//!
//! `NoStoredSpan` is the separate marker that forbids a stored span. It is the
//! deliberate inverse of `NoTypeExpr`: the SAME non-hand-implementable
//! machinery, minus the `verter_span::Span` leaf witness. Because no leaf impl
//! makes `Span` a witness, any type transitively OWNING a span-shaped field
//! fails the bound, while `String` / `bool` / `u32` / `Arc<str>` and their
//! containers pass.
//!
//! This derive emits an impl of the HIDDEN witness supertrait
//! `__private::NoStoredSpanWitness` (never the public `NoStoredSpan` trait) with
//! a `: NoStoredSpan` bound on every field's type — so the impl only holds when
//! every field is, recursively, `NoStoredSpan`. Emitting the hidden witness (not
//! the public trait) is what keeps the public trait non-hand-implementable
//! downstream: `NoStoredSpan` is blanket-impl'd over every witness, so a
//! hand-written `impl NoStoredSpan for X` would overlap the blanket and be
//! rejected.
//!
//! There is deliberately NO syntactic special-casing for type aliases — the
//! compiler resolves the real field type, closing the alias / re-export /
//! module-shadow hole a spelling scanner could not.

use proc_macro::TokenStream;
use quote::quote;
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::{parse_quote, Data, DeriveInput, Fields, Type, WherePredicate};

/// Derive the hidden `NoStoredSpan` witness for a `struct` or `enum`, bounding
/// every field's type on the public `NoStoredSpan` trait. Rejects `union`.
///
/// Accepts an opt-in container attribute `#[no_storedspan(recursive_self)]` —
/// see [`expand_witness`] — for an approved owned self-container.
#[proc_macro_derive(NoStoredSpan, attributes(no_storedspan))]
pub fn derive_no_stored_span(item: TokenStream) -> TokenStream {
    match syn::parse::<DeriveInput>(item) {
        Ok(input) => expand_witness(&input).into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// The pure token transform (separated from the proc-macro ABI entry so it is
/// unit-testable directly on a parsed `DeriveInput`). Emits the hidden-witness
/// impl with a per-field `: ::verter_no_storedspan::NoStoredSpan` bound, or a
/// `compile_error!` for a `union`.
///
/// # Recursive-self escape
///
/// A container annotated `#[no_storedspan(recursive_self)]` may own the ONE
/// approved fixed-point self-container shape `Arc<[Self]>` (a named closed
/// composition arm — e.g. `ClosednessRecipe::IntersectionAllArms(Arc<[Self]>)`).
/// For such a field the derive OMITS only that self-bound (a bound would ask the
/// trait solver to prove `Arc<[Self]>: NoStoredSpan` while proving
/// `Self: NoStoredSpan` — an overflow, E0275). EVERY non-recursive field/arm
/// payload still gets its per-field witness bound, so a NEW non-recursive arm
/// carrying a `Span` still FAILS the derive. The attribute is REJECTED unless at
/// least one `Arc<[Self]>` field exists, so it can never be abused to skip a
/// non-recursive bound.
fn expand_witness(input: &DeriveInput) -> proc_macro2::TokenStream {
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let recursive_self = match parse_recursive_self(&input.attrs) {
        Ok(flag) => flag,
        Err(err) => return err,
    };

    let field_types = match &input.data {
        Data::Struct(data) => fields_types(&data.fields),
        Data::Enum(data) => data
            .variants
            .iter()
            .flat_map(|variant| fields_types(&variant.fields))
            .collect(),
        Data::Union(_) => {
            return quote! {
                ::core::compile_error!("NoStoredSpan cannot be derived for unions");
            };
        }
    };

    // Partition the fields: `Arc<[Self]>` fields are the approved recursive-self
    // fields whose bound is OMITTED (only under the opt-in attribute); every
    // other field keeps its per-field witness bound.
    let mut bounded: Vec<Type> = Vec::new();
    let mut self_recursive_fields = 0usize;
    for ty in field_types {
        if recursive_self && is_arc_slice_of_self(&ty, ident) {
            self_recursive_fields += 1;
            continue;
        }
        bounded.push(ty);
    }
    if recursive_self && self_recursive_fields == 0 {
        return quote! {
            ::core::compile_error!(
                "#[no_storedspan(recursive_self)] requires at least one `Arc<[Self]>` field; \
                 it must not be used to skip a non-recursive bound"
            );
        };
    }

    // Build ONE combined predicate list: the input's existing where-predicates
    // (if any) ANDed with one `<field-ty>: ::verter_no_storedspan::NoStoredSpan`
    // bound per bounded field. Folding both into a single `Punctuated<_, Comma>`
    // makes the separators the punctuation's responsibility, so the concatenation
    // is always well-formed regardless of whether the source where-clause carried
    // a trailing comma.
    //
    // The public trait is the bound a field author reasons about; the blanket
    // bridge in `verter_no_storedspan` makes `NoStoredSpan` equivalent to the
    // hidden witness, so bounding on the public trait still drives the recursion.
    let mut predicates: Punctuated<WherePredicate, Comma> = Punctuated::new();
    if let Some(w) = where_clause {
        predicates.extend(w.predicates.iter().cloned());
    }
    for ty in &bounded {
        predicates.push(parse_quote! { #ty: ::verter_no_storedspan::NoStoredSpan });
    }

    quote! {
        #[automatically_derived]
        impl #impl_generics ::verter_no_storedspan::__private::NoStoredSpanWitness
            for #ident #ty_generics
        where
            #predicates
        {}
    }
}

/// Parse an optional container-level `#[no_storedspan(recursive_self)]`. Returns
/// `Ok(true)` when present, `Ok(false)` when absent, or `Err(compile_error)` for
/// any other `no_storedspan(...)` option (the only supported option is
/// `recursive_self`).
fn parse_recursive_self(attrs: &[syn::Attribute]) -> Result<bool, proc_macro2::TokenStream> {
    let mut found = false;
    for attr in attrs {
        if !attr.path().is_ident("no_storedspan") {
            continue;
        }
        if let Err(err) = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("recursive_self") {
                found = true;
                Ok(())
            } else {
                Err(meta
                    .error("unknown `no_storedspan` option; only `recursive_self` is supported"))
            }
        }) {
            return Err(err.to_compile_error());
        }
    }
    Ok(found)
}

/// Whether `ty` is exactly `Arc<[Self]>` — the container's own type wrapped in
/// `Arc<[..]>` (the approved fixed-point self-container). Matches on the LAST
/// path segment being `Arc` (so `std::sync::Arc<[..]>` and `Arc<[..]>` both
/// match) whose single argument is a slice of a path whose last segment is the
/// container ident or the literal `Self`.
fn is_arc_slice_of_self(ty: &Type, self_ident: &syn::Ident) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    let Some(seg) = type_path.path.segments.last() else {
        return false;
    };
    if seg.ident != "Arc" {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return false;
    };
    if args.args.len() != 1 {
        return false;
    }
    let syn::GenericArgument::Type(Type::Slice(slice)) = &args.args[0] else {
        return false;
    };
    let Type::Path(elem_path) = slice.elem.as_ref() else {
        return false;
    };
    match elem_path.path.segments.last() {
        Some(elem_seg) => elem_seg.ident == *self_ident || elem_seg.ident == "Self",
        None => false,
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
        // `NoStoredSpan` directly would collide with the blanket impl and would
        // let a hand-written impl masquerade as the derive.
        assert!(
            out.contains("NoStoredSpanWitness"),
            "must impl the hidden witness, not the public trait: {out}"
        );
        assert!(
            out.contains(":: verter_no_storedspan :: __private :: NoStoredSpanWitness"),
            "must reference the __private witness path: {out}"
        );
    }

    #[test]
    fn named_struct_bounds_every_field_on_public_trait() {
        let out = expand_str("struct S { a: u32, b: String }");
        // Discriminating: BOTH field types must appear bounded on the PUBLIC
        // trait — the bound is what propagates the recursion.
        assert!(
            out.contains("u32 : :: verter_no_storedspan :: NoStoredSpan"),
            "field `a: u32` must be bounded: {out}"
        );
        assert!(
            out.contains("String : :: verter_no_storedspan :: NoStoredSpan"),
            "field `b: String` must be bounded: {out}"
        );
    }

    #[test]
    fn tuple_struct_bounds_every_field() {
        let out = expand_str("struct T(u64, bool);");
        assert!(out.contains("u64 : :: verter_no_storedspan :: NoStoredSpan"));
        assert!(out.contains("bool : :: verter_no_storedspan :: NoStoredSpan"));
    }

    #[test]
    fn enum_bounds_every_variant_field() {
        // Discriminating: a field hidden inside ANY variant must still gain a
        // bound — otherwise an enum could smuggle a stored span through a variant.
        let out = expand_str("enum E { A(u8), B { x: char }, C }");
        assert!(
            out.contains("u8 : :: verter_no_storedspan :: NoStoredSpan"),
            "tuple-variant field `A(u8)` must be bounded: {out}"
        );
        assert!(
            out.contains("char : :: verter_no_storedspan :: NoStoredSpan"),
            "named-variant field `B {{ x: char }}` must be bounded: {out}"
        );
    }

    #[test]
    fn unit_struct_emits_impl_with_no_field_bounds() {
        let out = expand_str("struct Unit;");
        assert!(out.contains("NoStoredSpanWitness"));
        // A fieldless type is unconditionally a witness — no per-field bound.
        assert!(
            !out.contains(":: verter_no_storedspan :: NoStoredSpan"),
            "a unit struct must add no `: NoStoredSpan` field bound: {out}"
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
            out.contains("Vec < T > : :: verter_no_storedspan :: NoStoredSpan"),
            "the generic field type must be bounded: {out}"
        );
    }

    #[test]
    fn generic_struct_with_existing_where_clause_emits_parseable_impl() {
        // REGRESSION (where-clause concatenation): a generic input that already
        // carries a where-clause WITHOUT a trailing comma must still expand to a
        // PARSEABLE `impl`.
        let out = expand_str("struct G<T> where T: Clone { t: T }");
        syn::parse_str::<syn::ItemImpl>(&out).unwrap_or_else(|e| {
            panic!("where-bearing derive output must parse as an `impl`: {e}\n---\n{out}")
        });
        assert!(
            out.contains("T : Clone , T : :: verter_no_storedspan :: NoStoredSpan"),
            "the existing `T: Clone` predicate must be comma-separated from the field bound: {out}"
        );
    }

    #[test]
    fn generic_struct_with_trailing_comma_where_clause_does_not_double_comma() {
        let out = expand_str("struct G<T> where T: Clone, { t: T }");
        syn::parse_str::<syn::ItemImpl>(&out).unwrap_or_else(|e| {
            panic!("trailing-comma where-bearing derive output must parse: {e}\n---\n{out}")
        });
        assert!(
            !out.contains(", ,") && !out.contains(",,"),
            "a trailing-comma source where-clause must not produce a double comma: {out}"
        );
        assert!(
            out.contains("T : Clone , T : :: verter_no_storedspan :: NoStoredSpan"),
            "exactly one comma must join the existing predicate and the field bound: {out}"
        );
    }

    #[test]
    fn no_where_struct_still_emits_parseable_impl() {
        let out = expand_str("struct S { a: u32, b: String }");
        syn::parse_str::<syn::ItemImpl>(&out).unwrap_or_else(|e| {
            panic!("no-where derive output must parse as an `impl`: {e}\n---\n{out}")
        });
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
            !out.contains("NoStoredSpanWitness"),
            "a union must NOT receive a witness impl: {out}"
        );
    }

    #[test]
    fn recursive_self_omits_arc_slice_self_bound_but_keeps_other_fields() {
        // Discriminating: `#[no_storedspan(recursive_self)]` OMITS the `Arc<[R]>`
        // self-bound (which would overflow the solver) but STILL bounds the
        // non-recursive `u32` arm — so a future `Span` arm would still fail.
        let out = expand_str(
            "#[no_storedspan(recursive_self)] enum R { Leaf(u32), Rec(std::sync::Arc<[R]>) }",
        );
        assert!(
            out.contains("u32 : :: verter_no_storedspan :: NoStoredSpan"),
            "the non-recursive arm must keep its witness bound: {out}"
        );
        assert!(
            !out.contains("Arc"),
            "the `Arc<[Self]>` self-bound must be OMITTED (no Arc bound emitted): {out}"
        );
        assert!(
            out.contains("NoStoredSpanWitness"),
            "the recursive carrier must still receive a witness impl: {out}"
        );
    }

    #[test]
    fn recursive_self_matches_literal_self_element() {
        let out =
            expand_str("#[no_storedspan(recursive_self)] enum R { Rec(std::sync::Arc<[Self]>) }");
        assert!(
            !out.contains("Arc"),
            "the `Arc<[Self]>` bound must be omitted: {out}"
        );
        assert!(out.contains("NoStoredSpanWitness"), "{out}");
    }

    #[test]
    fn recursive_self_rejected_without_self_container_field() {
        // Abuse guard: the attribute on a type with NO `Arc<[Self]>` field is a
        // compile_error — it must not be usable to skip a non-recursive bound.
        let out = expand_str("#[no_storedspan(recursive_self)] struct S { a: u32 }");
        assert!(
            out.contains("compile_error"),
            "the attr on a non-self-recursive shape must be rejected: {out}"
        );
        assert!(
            !out.contains("NoStoredSpanWitness"),
            "a rejected attr must NOT emit a witness impl: {out}"
        );
    }

    #[test]
    fn recursive_self_rejects_non_self_arc_slice() {
        let out =
            expand_str("#[no_storedspan(recursive_self)] enum R { Rec(std::sync::Arc<[Other]>) }");
        assert!(
            out.contains("compile_error"),
            "`Arc<[Other]>` must not satisfy the recursive-self requirement: {out}"
        );
    }

    #[test]
    fn unknown_no_storedspan_option_is_compile_error() {
        let out = expand_str("#[no_storedspan(bogus)] struct S { a: u32 }");
        assert!(
            out.contains("compile_error"),
            "an unknown `no_storedspan` option must be rejected: {out}"
        );
    }

    #[test]
    fn without_attr_arc_slice_self_still_gets_bound() {
        let out = expand_str("enum R { Rec(std::sync::Arc<[R]>) }");
        assert!(
            out.contains("Arc"),
            "without the attr, `Arc<[Self]>` keeps its bound: {out}"
        );
    }
}
