//! `#[derive(HeapSize)]` — the exhaustive heap measurement of a retained data
//! tree, for `verter_retention::heap_size::HeapSize`.
//!
//! Every field of every variant is destructured and measured, so a field
//! added to a measured type is measured by construction: it compiles only
//! once its type measures itself. Two field attributes cover the rest:
//!
//! * `#[heap_size(inline)]` — the field owns no heap. The generated code
//!   requires the field's type to be `Copy`, which no heap owner is, so a
//!   heap-owning field cannot be declared inline.
//! * `#[heap_size(with = "path")]` — the field is measured by
//!   `path(&field, meter) -> usize`, for a foreign type this crate cannot
//!   implement the trait for.
//!
//! Type parameters gain a `HeapSize` bound; field types gain none, so a
//! recursive type (an enum holding `Arc<Self>`) derives without the trait
//! solver recursing.
//!
//! `#[derive(Unshared)]` — a deep copy that shares no raw allocation with
//! its source, for `verter_retention::unshared::Unshared`. Every field of
//! every variant is copied through `Unshared`; `#[unshared(clone)]` copies a
//! field whose `Clone` already shares no raw allocation (a span, a plain
//! enum, a self-charged owned handle).

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_quote, Data, DeriveInput, Field, Fields, LitStr};

/// Derive `HeapSize` for a `struct` or `enum`. Rejects `union`.
#[proc_macro_derive(HeapSize, attributes(heap_size))]
pub fn derive_heap_size(item: TokenStream) -> TokenStream {
    match syn::parse::<DeriveInput>(item) {
        Ok(input) => heap_size_impl(&input)
            .unwrap_or_else(|err| err.to_compile_error())
            .into(),
        Err(err) => err.to_compile_error().into(),
    }
}

enum FieldMode {
    Measured,
    Inline,
    With(syn::Path),
}

fn field_mode(field: &Field) -> syn::Result<FieldMode> {
    let mut mode = FieldMode::Measured;
    for attr in &field.attrs {
        if !attr.path().is_ident("heap_size") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("inline") {
                mode = FieldMode::Inline;
                Ok(())
            } else if meta.path.is_ident("with") {
                let path: LitStr = meta.value()?.parse()?;
                mode = FieldMode::With(path.parse()?);
                Ok(())
            } else {
                Err(meta.error("expected `inline` or `with = \"path\"`"))
            }
        })?;
    }
    Ok(mode)
}

/// The sum of every field's heap, each bound to `binding`.
fn fields_sum(fields: &[(&Field, proc_macro2::Ident)]) -> syn::Result<proc_macro2::TokenStream> {
    let mut terms = Vec::with_capacity(fields.len());
    for (field, binding) in fields {
        terms.push(match field_mode(field)? {
            FieldMode::Measured => quote! {
                ::verter_retention::heap_size::HeapSize::heap_size(#binding, meter)
            },
            FieldMode::Inline => quote! {
                ::verter_retention::heap_size::inline_copy(#binding)
            },
            FieldMode::With(path) => quote! { #path(#binding, meter) },
        });
    }
    Ok(quote! { 0usize #(+ #terms)* })
}

/// The destructuring pattern of `fields` and their bindings.
fn pattern(fields: &Fields) -> (proc_macro2::TokenStream, Vec<(&Field, proc_macro2::Ident)>) {
    match fields {
        Fields::Named(named) => {
            let bound: Vec<_> = named
                .named
                .iter()
                .map(|field| (field, field.ident.clone().expect("a named field")))
                .collect();
            let names = bound.iter().map(|(_, name)| name);
            (quote! { { #(#names),* } }, bound)
        }
        Fields::Unnamed(unnamed) => {
            let bound: Vec<_> = unnamed
                .unnamed
                .iter()
                .enumerate()
                .map(|(index, field)| (field, format_ident!("field_{index}")))
                .collect();
            let names = bound.iter().map(|(_, name)| name);
            (quote! { ( #(#names),* ) }, bound)
        }
        Fields::Unit => (quote! {}, Vec::new()),
    }
}

fn heap_size_impl(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let ident = &input.ident;
    let mut generics = input.generics.clone();
    for param in generics.type_params_mut() {
        param
            .bounds
            .push(parse_quote!(::verter_retention::heap_size::HeapSize));
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let body = match &input.data {
        Data::Struct(data) => {
            let (pattern, bound) = pattern(&data.fields);
            let sum = fields_sum(&bound)?;
            quote! {
                let Self #pattern = self;
                #sum
            }
        }
        Data::Enum(data) => {
            let mut arms = Vec::with_capacity(data.variants.len());
            for variant in &data.variants {
                let name = &variant.ident;
                let (pattern, bound) = pattern(&variant.fields);
                let sum = fields_sum(&bound)?;
                arms.push(quote! { Self::#name #pattern => #sum, });
            }
            if arms.is_empty() {
                quote! { match *self {} }
            } else {
                quote! { match self { #(#arms)* } }
            }
        }
        Data::Union(data) => {
            return Err(syn::Error::new_spanned(
                data.union_token,
                "HeapSize cannot be derived for a union",
            ))
        }
    };
    if iterative(&input.attrs)? {
        return Ok(quote! {
            impl #impl_generics ::verter_retention::heap_size::IterativeNode for #ident #ty_generics
                #where_clause
            {
                #[allow(unused_variables, clippy::let_and_return)]
                fn node_heap_size<'t>(
                    &'t self,
                    meter: &mut ::verter_retention::heap_size::HeapMeter<'_, 't>,
                ) -> usize {
                    #body
                }
            }

            impl #impl_generics ::verter_retention::heap_size::HeapSize for #ident #ty_generics
                #where_clause
            {
                fn heap_size<'t>(
                    &'t self,
                    meter: &mut ::verter_retention::heap_size::HeapMeter<'_, 't>,
                ) -> usize {
                    meter.iterate(self)
                }
            }
        });
    }
    Ok(quote! {
        impl #impl_generics ::verter_retention::heap_size::HeapSize for #ident #ty_generics
            #where_clause
        {
            #[allow(unused_variables, clippy::let_and_return)]
            fn heap_size<'t>(
                &'t self,
                meter: &mut ::verter_retention::heap_size::HeapMeter<'_, 't>,
            ) -> usize {
                #body
            }
        }
    })
}

/// Whether the container carries `#[heap_size(iterative)]`: a recursive
/// tree measured from the meter's work-list, never the call stack.
fn iterative(attrs: &[syn::Attribute]) -> syn::Result<bool> {
    let mut iterative = false;
    for attr in attrs {
        if !attr.path().is_ident("heap_size") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("iterative") {
                iterative = true;
                Ok(())
            } else {
                Err(meta.error("expected `iterative`"))
            }
        })?;
    }
    Ok(iterative)
}

/// Derive `Unshared` for a `struct` or `enum`. Rejects `union`.
#[proc_macro_derive(Unshared, attributes(unshared))]
pub fn derive_unshared(item: TokenStream) -> TokenStream {
    match syn::parse::<DeriveInput>(item) {
        Ok(input) => unshared_impl(&input)
            .unwrap_or_else(|err| err.to_compile_error())
            .into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// Whether `field` carries `#[unshared(clone)]`.
fn unshared_by_clone(field: &Field) -> syn::Result<bool> {
    let mut clone = false;
    for attr in &field.attrs {
        if !attr.path().is_ident("unshared") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("clone") {
                clone = true;
                Ok(())
            } else {
                Err(meta.error("expected `clone`"))
            }
        })?;
    }
    Ok(clone)
}

/// The construction of `fields` from their unshared copies, each bound to
/// its pattern binding.
fn unshared_fields(
    fields: &Fields,
    bound: &[(&Field, proc_macro2::Ident)],
) -> syn::Result<proc_macro2::TokenStream> {
    let mut copies = Vec::with_capacity(bound.len());
    for (field, binding) in bound {
        copies.push(if unshared_by_clone(field)? {
            quote! { ::core::clone::Clone::clone(#binding) }
        } else {
            quote! { ::verter_retention::unshared::Unshared::unshared(#binding) }
        });
    }
    Ok(match fields {
        Fields::Named(_) => {
            let names = bound.iter().map(|(_, name)| name);
            quote! { { #(#names: #copies),* } }
        }
        Fields::Unnamed(_) => quote! { ( #(#copies),* ) },
        Fields::Unit => quote! {},
    })
}

fn unshared_impl(input: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let ident = &input.ident;
    let mut generics = input.generics.clone();
    for param in generics.type_params_mut() {
        param
            .bounds
            .push(parse_quote!(::verter_retention::unshared::Unshared));
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    let body = match &input.data {
        Data::Struct(data) => {
            let (pattern, bound) = pattern(&data.fields);
            let build = unshared_fields(&data.fields, &bound)?;
            quote! {
                let Self #pattern = self;
                Self #build
            }
        }
        Data::Enum(data) => {
            let mut arms = Vec::with_capacity(data.variants.len());
            for variant in &data.variants {
                let name = &variant.ident;
                let (pattern, bound) = pattern(&variant.fields);
                let build = unshared_fields(&variant.fields, &bound)?;
                arms.push(quote! { Self::#name #pattern => Self::#name #build, });
            }
            if arms.is_empty() {
                quote! { match *self {} }
            } else {
                quote! { match self { #(#arms)* } }
            }
        }
        Data::Union(data) => {
            return Err(syn::Error::new_spanned(
                data.union_token,
                "Unshared cannot be derived for a union",
            ))
        }
    };
    Ok(quote! {
        impl #impl_generics ::verter_retention::unshared::Unshared for #ident #ty_generics
            #where_clause
        {
            fn unshared(&self) -> Self {
                #body
            }
        }
    })
}
