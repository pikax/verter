//! `#[derive(NoTypeExpr)]` — the field-recursive witness emitter for the
//! `verter_no_typeexpr::NoTypeExpr` marker trait.
//!
//! # Why this crate exists
//!
//! NoTypeExpr-witnessed carriers (e.g. the session's lazily-served
//! `LoweredValueDecl` in `crates/verter_session/src/decl_body_memo.rs`) must
//! own NO transitive `verter_type_expr::TypeExpr`: every type-body position
//! is a narrowed fact or content-free locator, never the symbolic IR. The first guard for that invariant was a `syn` SOURCE SCANNER that
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
use syn::punctuated::Punctuated;
use syn::token::Comma;
use syn::{parse_quote, Data, DeriveInput, Fields, Type, WherePredicate};

/// Derive the hidden `NoTypeExpr` witness for a `struct` or `enum`, bounding
/// every field's type on the public `NoTypeExpr` trait. Rejects `union`.
///
/// Accepts an opt-in container attribute `#[no_typeexpr(recursive_self)]` — see
/// [`expand_witness`] — for an approved owned self-container.
#[proc_macro_derive(NoTypeExpr, attributes(no_typeexpr))]
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
///
/// # Recursive-self escape
///
/// A container annotated `#[no_typeexpr(recursive_self)]` may own the ONE
/// approved fixed-point self-container shape `Arc<[Self]>` (a named closed
/// composition arm — e.g. `ClosednessRecipe::IntersectionAllArms(Arc<[Self]>)`).
/// A plain per-field `Arc<[Self]>: NoTypeExpr` bound would ask the trait solver
/// to prove `Arc<[Self]>: NoTypeExpr` while proving `Self: NoTypeExpr` — an
/// overflow (E0275). Rather than merely OMITTING the bound (a syntactic pick the
/// compiler never verifies, launderable by a bare re-import / local shadow /
/// custom `Arc` that owns a `TypeExpr`), the derive emits a COMPILER-RESOLVED
/// PROOF-BOUND `#field_ty: __private::RecursiveSelfArc<Self>`. That trait is
/// implemented ONLY for the genuine `::std::sync::Arc<[Self]>`, so the compiler
/// RESOLVES the field type and rejects any wrapper that is not the real std
/// `Arc` — while resolving NON-recursively (it does not require
/// `Self: NoTypeExpr`, so no E0275). EVERY non-recursive field/arm payload still
/// gets its per-field witness bound, so a NEW non-recursive arm carrying a
/// `TypeExpr` still FAILS the derive. The syntactic heuristic only PICKS
/// candidate fields; the proof-bound VERIFIES them — a wrong pick fails
/// `RecursiveSelfArc`, and a missed real `Arc<[Self]>` field keeps the recursive
/// witness bound and fails with E0275. The attribute is REJECTED unless at least
/// one `Arc<[Self]>` field exists, so it can never be abused to skip a
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
                ::core::compile_error!("NoTypeExpr cannot be derived for unions");
            };
        }
    };

    // Partition the fields: `Arc<[Self]>` fields are the approved recursive-self
    // candidates (only under the opt-in attribute) that get the compiler-resolved
    // `RecursiveSelfArc<Self>` PROOF-BOUND instead of the recursive witness bound;
    // every other field keeps its per-field witness bound.
    let mut bounded: Vec<Type> = Vec::new();
    let mut recursive_fields: Vec<Type> = Vec::new();
    for ty in field_types {
        if recursive_self && is_arc_slice_of_self(&ty, ident) {
            recursive_fields.push(ty);
            continue;
        }
        bounded.push(ty);
    }
    if recursive_self && recursive_fields.is_empty() {
        return quote! {
            ::core::compile_error!(
                "#[no_typeexpr(recursive_self)] requires at least one `Arc<[Self]>` field; \
                 it must not be used to skip a non-recursive bound"
            );
        };
    }

    // Build ONE combined predicate list: the input's existing where-predicates
    // (if any) ANDed with one `<field-ty>: ::verter_no_typeexpr::NoTypeExpr`
    // bound per bounded field. Folding both into a single `Punctuated<_, Comma>`
    // makes the separators the punctuation's responsibility, so the concatenation
    // is always well-formed regardless of whether the source where-clause carried
    // a trailing comma — re-emitting `#predicates` followed directly by the field
    // bounds would otherwise splice the last existing predicate into the first
    // field bound (`where T: Clone t: NoTypeExpr`, an unparsable token stream).
    //
    // The public trait is the bound a field author reasons about; the blanket
    // bridge in `verter_no_typeexpr` makes `NoTypeExpr` equivalent to the hidden
    // witness, so bounding on the public trait still drives the recursion.
    let mut predicates: Punctuated<WherePredicate, Comma> = Punctuated::new();
    if let Some(w) = where_clause {
        predicates.extend(w.predicates.iter().cloned());
    }
    for ty in &bounded {
        predicates.push(parse_quote! { #ty: ::verter_no_typeexpr::NoTypeExpr });
    }
    // The recursive-self field(s): emit the COMPILER-RESOLVED proof-bound instead
    // of omitting the bound. Only the genuine `::std::sync::Arc<[Self]>` satisfies
    // `RecursiveSelfArc<Self>`, so a bare re-import / local shadow / custom `Arc`
    // that could own a `TypeExpr` FAILS here. The bound resolves NON-recursively
    // (it does not require `Self: NoTypeExpr`), so it proves the shape WITHOUT the
    // `Arc<[Self]>: NoTypeExpr` overflow (E0275) the plain witness bound triggers.
    // `Self` in the emitted `where` clause is the impl's own target type.
    for ty in &recursive_fields {
        predicates
            .push(parse_quote! { #ty: ::verter_no_typeexpr::__private::RecursiveSelfArc<Self> });
    }

    quote! {
        #[automatically_derived]
        impl #impl_generics ::verter_no_typeexpr::__private::NoTypeExprWitness
            for #ident #ty_generics
        where
            #predicates
        {}
    }
}

/// Parse an optional container-level `#[no_typeexpr(recursive_self)]`. Returns
/// `Ok(true)` when present, `Ok(false)` when absent, or `Err(compile_error)` for
/// any other `no_typeexpr(...)` option (the only supported option is
/// `recursive_self`).
fn parse_recursive_self(attrs: &[syn::Attribute]) -> Result<bool, proc_macro2::TokenStream> {
    let mut found = false;
    for attr in attrs {
        if !attr.path().is_ident("no_typeexpr") {
            continue;
        }
        if let Err(err) = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("recursive_self") {
                found = true;
                Ok(())
            } else {
                Err(meta.error("unknown `no_typeexpr` option; only `recursive_self` is supported"))
            }
        }) {
            return Err(err.to_compile_error());
        }
    }
    Ok(found)
}

/// Whether `ty` is a CANDIDATE `Arc<[Self]>` — the container's OWN type wrapped
/// in the standard `Arc<[..]>` (the approved fixed-point self-container).
///
/// This is a candidate PICK only. Soundness rests on the compiler-resolved
/// `__private::RecursiveSelfArc<Self>` proof-bound the derive emits for the
/// picked field, NOT on this syntactic match: a wrong pick (a non-`Arc<[Self]>`
/// field) fails the proof-bound, and a bare re-import / local shadow / custom
/// `Arc` this check happens to accept ALSO fails it, because the resolved field
/// type is not `::std::sync::Arc<[Self]>`. The match stays TIGHT anyway (fewer
/// false picks, clearer errors):
///
/// - The wrapper must be [`is_std_arc_path`] — bare `Arc`, `std::sync::Arc`, or
///   `alloc::sync::Arc`. A multi-segment custom `foo::Arc` (which could OWN a
///   `TypeExpr`) is rejected, so its witness bound is kept.
/// - The slice element must be the container's own type: a bare SINGLE-SEGMENT
///   path (no qualified `<X as Y>::T`, no `::`-anchored, no `foo::T`) whose sole
///   segment ident is the container ident or the literal `Self`, with NO generic
///   args. This rejects `Arc<[some_mod::Recipe]>` — a DIFFERENT type sharing the
///   container's LAST-segment name that owns a `TypeExpr` — whose bound is then
///   KEPT. A same-module single-segment name cannot be a foreign shadow (a
///   name-collision error prevents it), so the ident-equality check is sound.
fn is_arc_slice_of_self(ty: &Type, self_ident: &syn::Ident) -> bool {
    // The wrapper must be a bare `Arc<..>` path — reject a qualified `<X as
    // Y>::Arc` and any shadowed/custom `foo::Arc`.
    let Type::Path(type_path) = ty else {
        return false;
    };
    if type_path.qself.is_some() || !is_std_arc_path(&type_path.path) {
        return false;
    }
    let Some(seg) = type_path.path.segments.last() else {
        return false;
    };
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
    // The slice element must be the container's OWN single-segment type — reject
    // qualified / `::`-anchored / multi-segment / generic-arg-bearing elements.
    if elem_path.qself.is_some() || elem_path.path.leading_colon.is_some() {
        return false;
    }
    if elem_path.path.segments.len() != 1 {
        return false;
    }
    let elem_seg = &elem_path.path.segments[0];
    if !matches!(elem_seg.arguments, syn::PathArguments::None) {
        return false;
    }
    elem_seg.ident == *self_ident || elem_seg.ident == "Self"
}

/// Whether `path` names the standard `Arc` wrapper: bare single-segment `Arc`,
/// or the full `std::sync::Arc` / `alloc::sync::Arc` path (a leading `::` on the
/// full path is fine). A multi-segment custom `foo::Arc` (which could OWN a
/// `TypeExpr`) is NOT matched, so its witness bound is KEPT. A BARE single-segment
/// `Arc` IS matched even when it is a local re-import of a custom `Arc`
/// (`use shadow::Arc;`) — that residual case is closed downstream by the
/// `RecursiveSelfArc` proof-bound, which resolves the field type and rejects any
/// wrapper that is not the real `::std::sync::Arc`.
fn is_std_arc_path(path: &syn::Path) -> bool {
    let segs: Vec<String> = path.segments.iter().map(|s| s.ident.to_string()).collect();
    match segs.len() {
        1 => segs[0] == "Arc" && path.leading_colon.is_none(),
        3 => segs[1] == "sync" && segs[2] == "Arc" && (segs[0] == "std" || segs[0] == "alloc"),
        _ => false,
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
    fn generic_struct_with_existing_where_clause_emits_parseable_impl() {
        // REGRESSION (where-clause concatenation): a generic input that already
        // carries a where-clause WITHOUT a trailing comma must still expand to a
        // PARSEABLE `impl`. The pre-fix emission re-emitted `#predicates`
        // followed directly by the field bounds, so `where T: Clone` spliced
        // into the first field bound — `where T : Clone t : … NoTypeExpr` — which
        // does NOT parse as an `ItemImpl` (the proc-macro would fail with
        // "produced unparsable tokens"). Discriminating: this `syn::parse_str`
        // FAILS against the pre-fix emitter and PASSES once the existing
        // predicates are comma-separated from the field bounds.
        let out = expand_str("struct G<T> where T: Clone { t: T }");
        syn::parse_str::<syn::ItemImpl>(&out).unwrap_or_else(|e| {
            panic!("where-bearing derive output must parse as an `impl`: {e}\n---\n{out}")
        });
        // Both the existing predicate and the field bound must survive, joined by
        // a comma (the separator the bug dropped).
        assert!(
            out.contains("T : Clone , T : :: verter_no_typeexpr :: NoTypeExpr"),
            "the existing `T: Clone` predicate must be comma-separated from the field bound: {out}"
        );
    }

    #[test]
    fn generic_struct_with_trailing_comma_where_clause_does_not_double_comma() {
        // The combined-`Punctuated` fix must NOT double-comma when the source
        // where-clause ALREADY ends in a trailing comma. `syn`'s `Punctuated`
        // owns the separators, so the output parses and contains exactly one
        // comma between the existing predicate and the first field bound.
        let out = expand_str("struct G<T> where T: Clone, { t: T }");
        syn::parse_str::<syn::ItemImpl>(&out).unwrap_or_else(|e| {
            panic!("trailing-comma where-bearing derive output must parse: {e}\n---\n{out}")
        });
        assert!(
            !out.contains(", ,") && !out.contains(",,"),
            "a trailing-comma source where-clause must not produce a double comma: {out}"
        );
        assert!(
            out.contains("T : Clone , T : :: verter_no_typeexpr :: NoTypeExpr"),
            "exactly one comma must join the existing predicate and the field bound: {out}"
        );
    }

    #[test]
    fn no_where_struct_still_emits_parseable_impl() {
        // The empty-existing-predicates path (no source where-clause) must also
        // expand to a parseable `impl` — the combined-`Punctuated` fix carries
        // only the field bounds in that case.
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
            !out.contains("NoTypeExprWitness"),
            "a union must NOT receive a witness impl: {out}"
        );
    }

    #[test]
    fn recursive_self_emits_proof_bound_for_self_arc_and_keeps_other_fields() {
        // Discriminating: `#[no_typeexpr(recursive_self)]` emits the compiler-
        // resolved `RecursiveSelfArc<Self>` PROOF-BOUND on the `Arc<[R]>` self
        // field (NOT a plain `NoTypeExpr` witness bound, which would overflow the
        // solver — E0275) and STILL bounds the non-recursive `u32` arm on the
        // witness — so a future `TypeExpr` arm would still fail. The pre-fix escape
        // OMITTED the recursive field entirely, so `RecursiveSelfArc` was ABSENT.
        let out = expand_str(
            "#[no_typeexpr(recursive_self)] enum R { Leaf(u32), Rec(std::sync::Arc<[R]>) }",
        );
        assert!(
            out.contains("u32 : :: verter_no_typeexpr :: NoTypeExpr"),
            "the non-recursive arm must keep its witness bound: {out}"
        );
        assert!(
            out.contains(":: verter_no_typeexpr :: __private :: RecursiveSelfArc < Self >"),
            "the `Arc<[Self]>` field must gain the RecursiveSelfArc proof-bound: {out}"
        );
        assert!(
            out.contains("NoTypeExprWitness"),
            "the recursive carrier must still receive a witness impl: {out}"
        );
    }

    #[test]
    fn recursive_self_matches_literal_self_element() {
        // The slice element may be spelled `Self` rather than the container name;
        // it still gains the `RecursiveSelfArc<Self>` proof-bound.
        let out =
            expand_str("#[no_typeexpr(recursive_self)] enum R { Rec(std::sync::Arc<[Self]>) }");
        assert!(
            out.contains(":: verter_no_typeexpr :: __private :: RecursiveSelfArc < Self >"),
            "the `Arc<[Self]>` field must gain the RecursiveSelfArc proof-bound: {out}"
        );
        assert!(out.contains("NoTypeExprWitness"), "{out}");
    }

    #[test]
    fn recursive_self_rejected_without_self_container_field() {
        // Abuse guard: the attribute on a type with NO `Arc<[Self]>` field is a
        // compile_error — it must not be usable to skip a non-recursive bound.
        let out = expand_str("#[no_typeexpr(recursive_self)] struct S { a: u32 }");
        assert!(
            out.contains("compile_error"),
            "the attr on a non-self-recursive shape must be rejected: {out}"
        );
        assert!(
            !out.contains("NoTypeExprWitness"),
            "a rejected attr must NOT emit a witness impl: {out}"
        );
    }

    #[test]
    fn recursive_self_rejects_non_self_arc_slice() {
        // `Arc<[Other]>` is NOT the container's own type, so it is NOT a
        // recursive-self field: its bound is kept, and with no real `Arc<[Self]>`
        // field the attr is rejected.
        let out =
            expand_str("#[no_typeexpr(recursive_self)] enum R { Rec(std::sync::Arc<[Other]>) }");
        assert!(
            out.contains("compile_error"),
            "`Arc<[Other]>` must not satisfy the recursive-self requirement: {out}"
        );
    }

    #[test]
    fn unknown_no_typeexpr_option_is_compile_error() {
        let out = expand_str("#[no_typeexpr(bogus)] struct S { a: u32 }");
        assert!(
            out.contains("compile_error"),
            "an unknown `no_typeexpr` option must be rejected: {out}"
        );
    }

    #[test]
    fn without_attr_arc_slice_self_still_gets_bound() {
        // WITHOUT the opt-in attribute, an `Arc<[Self]>` field is bounded like any
        // other (the escape is opt-in only). This proves the proof-bound
        // substitution is gated.
        let out = expand_str("enum R { Rec(std::sync::Arc<[R]>) }");
        assert!(
            out.contains("Arc"),
            "without the attr, `Arc<[Self]>` keeps its bound: {out}"
        );
    }

    #[test]
    fn recursive_self_keeps_bound_on_multi_segment_arc_slice_element() {
        // TIGHTENED matcher: `Arc<[foo::R]>` is a DIFFERENT type that merely shares
        // the container's LAST-segment name — its bound MUST be KEPT. Discriminating:
        // the pre-fix last-segment matcher treated `foo::R` as the self type and
        // OMITTED the bound, so `foo :: R` would be absent from the output.
        let out = expand_str(
            "#[no_typeexpr(recursive_self)] enum R { Rec(std::sync::Arc<[R]>), \
             Foreign(std::sync::Arc<[foo::R]>) }",
        );
        assert!(
            out.contains("foo :: R"),
            "a multi-segment slice element (foo::R) must KEEP its witness bound: {out}"
        );
    }

    #[test]
    fn recursive_self_keeps_bound_on_custom_arc_wrapper() {
        // TIGHTENED matcher: a shadowed/custom `foo::Arc` wrapper (which could OWN a
        // `TypeExpr`) is NOT the standard `Arc`, so its bound MUST be KEPT.
        // Discriminating: the pre-fix last-segment `== "Arc"` check matched
        // `foo::Arc` and OMITTED the bound.
        let out = expand_str(
            "#[no_typeexpr(recursive_self)] enum R { Rec(std::sync::Arc<[R]>), \
             Custom(foo::Arc<[R]>) }",
        );
        assert!(
            out.contains("foo :: Arc"),
            "a custom `foo::Arc` wrapper must KEEP its witness bound: {out}"
        );
    }

    #[test]
    fn recursive_self_rejects_generic_arg_slice_element() {
        // TIGHTENED matcher: a generic-arg-bearing element (`R<u8>`) is NOT the bare
        // fixed-point self type, so its bound MUST be KEPT. Discriminating: the
        // pre-fix matcher ignored element generic args and OMITTED the bound.
        let out = expand_str(
            "#[no_typeexpr(recursive_self)] enum R { Rec(std::sync::Arc<[R]>), \
             Gen(std::sync::Arc<[R<u8>]>) }",
        );
        assert!(
            out.contains("R < u8 >"),
            "a generic-arg slice element (R<u8>) must KEEP its witness bound: {out}"
        );
    }
}
