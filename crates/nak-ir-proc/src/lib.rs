// SPDX-License-Identifier: AGPL-3.0-only
//! Proc-macro derives for NAK IR types.
//!
//! Replaces Mesa's `nak_ir_proc` crate. Provides derive macros used by
//! the NAK IR definition in `nak/ir.rs`:
//!
//! - `SrcsAsSlice` — generates `AsSlice<Src>` impl for instruction op structs
//! - `DstsAsSlice` — generates `AsSlice<Dst>` impl for instruction op structs
//! - `DisplayOp` — generates `DisplayOp` impl for the `Op` enum
//! - `FromVariants` — generates `From<OpFoo>` for the `Op` enum

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    AngleBracketedGenericArguments, Data, DataEnum, DataStruct, DeriveInput, Expr, Field, Fields,
    FieldsUnnamed, GenericArgument, Ident, Meta, PathArguments, Type, TypePath, parse_macro_input,
};

#[proc_macro_derive(SrcsAsSlice, attributes(src_type))]
pub fn derive_srcs_as_slice(input: TokenStream) -> TokenStream {
    derive_as_slice(input, "Src", "src_type", "SrcType")
}

#[proc_macro_derive(DstsAsSlice, attributes(dst_type))]
pub fn derive_dsts_as_slice(input: TokenStream) -> TokenStream {
    derive_as_slice(input, "Dst", "dst_type", "DstType")
}

/// Core logic for generating `AsSlice<T>` implementations.
///
/// Walks the struct fields, finds those matching `elem_type_name` (single or
/// array), reads the `#[attr_name(Variant)]` attribute for each, and generates
/// an `impl AsSlice<T>` that uses `repr(C)` layout to return a contiguous
/// slice across all matching fields.
fn derive_as_slice(
    input: TokenStream,
    elem_type_name: &str,
    attr_name: &str,
    type_enum_name: &str,
) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    if let Data::Enum(ref e) = input.data {
        return derive_as_slice_enum(
            struct_name,
            e,
            elem_type_name,
            type_enum_name,
            &impl_generics,
            &ty_generics,
            where_clause,
        );
    }

    let fields = match &input.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(f),
            ..
        }) => &f.named,
        _ => panic!(
            "{} can only be derived for structs with named fields",
            attr_name
        ),
    };

    let elem_type = Ident::new(elem_type_name, Span::call_site());
    let type_enum = Ident::new(type_enum_name, Span::call_site());

    struct MatchedField {
        ident: Ident,
        count: usize,
        attr_variant: Option<Ident>,
    }

    let mut matched: Vec<MatchedField> = Vec::new();

    for field in fields {
        let field_ident = field.ident.as_ref().unwrap().clone();
        let ty = &field.ty;

        if is_type_named(ty, elem_type_name) {
            let variant = get_field_attr(field, attr_name);
            matched.push(MatchedField {
                ident: field_ident,
                count: 1,
                attr_variant: variant,
            });
        } else if let Some(n) = array_of_type(ty, elem_type_name) {
            let variant = get_field_attr(field, attr_name);
            matched.push(MatchedField {
                ident: field_ident,
                count: n,
                attr_variant: variant,
            });
        }
    }

    let total_count: usize = matched.iter().map(|m| m.count).sum();

    if matched.is_empty() {
        return TokenStream::from(quote! {
            impl #impl_generics coral_nak_stubs::as_slice::AsSlice<#elem_type>
                for #struct_name #ty_generics #where_clause
            {
                type Attr = #type_enum;
                fn as_slice(&self) -> &[#elem_type] { &[] }
                fn as_mut_slice(&mut self) -> &mut [#elem_type] { &mut [] }
                fn attrs(&self) -> coral_nak_stubs::as_slice::AttrList<#type_enum> {
                    coral_nak_stubs::as_slice::AttrList::List(Vec::new())
                }
            }
        });
    }

    let first_field = &matched[0].ident;

    let as_slice_body = if matched.len() == 1 && matched[0].count == 1 {
        quote! { std::slice::from_ref(&self.#first_field) }
    } else if matched.len() == 1 {
        quote! { &self.#first_field }
    } else {
        let total = total_count;
        quote! {
            // SAFETY: The struct has #[repr(C)] and all matched fields (Elem or [Elem; N])
            // are contiguous in memory with no other fields between them. The pointer
            // points to the first such field and the length is the sum of all element
            // counts, so the resulting slice is valid.
            unsafe {
                let ptr = &self.#first_field as *const #elem_type;
                std::slice::from_raw_parts(ptr, #total)
            }
        }
    };

    let as_mut_slice_body = if matched.len() == 1 && matched[0].count == 1 {
        quote! { std::slice::from_mut(&mut self.#first_field) }
    } else if matched.len() == 1 {
        quote! { &mut self.#first_field }
    } else {
        let total = total_count;
        quote! {
            // SAFETY: The struct has #[repr(C)] and all matched fields (Elem or [Elem; N])
            // are contiguous in memory with no other fields between them. The pointer
            // points to the first such field and the length is the sum of all element
            // counts, so the resulting slice is valid.
            unsafe {
                let ptr = &mut self.#first_field as *mut #elem_type;
                std::slice::from_raw_parts_mut(ptr, #total)
            }
        }
    };

    let default_variant = Ident::new("DEFAULT", Span::call_site());

    let attrs_body = if matched.len() == 1 {
        let variant = matched[0]
            .attr_variant
            .as_ref()
            .map(|v| quote! { #type_enum::#v })
            .unwrap_or_else(|| quote! { #type_enum::#default_variant });
        quote! {
            coral_nak_stubs::as_slice::AttrList::Uniform(#variant)
        }
    } else {
        let mut attr_entries = Vec::new();
        for m in &matched {
            let variant = m
                .attr_variant
                .as_ref()
                .map(|v| quote! { #type_enum::#v })
                .unwrap_or_else(|| quote! { #type_enum::#default_variant });
            for _ in 0..m.count {
                attr_entries.push(variant.clone());
            }
        }
        quote! {
            coral_nak_stubs::as_slice::AttrList::List(
                vec![#(#attr_entries),*]
            )
        }
    };

    let expanded = quote! {
        impl #impl_generics coral_nak_stubs::as_slice::AsSlice<#elem_type>
            for #struct_name #ty_generics #where_clause
        {
            type Attr = #type_enum;

            fn as_slice(&self) -> &[#elem_type] {
                #as_slice_body
            }

            fn as_mut_slice(&mut self) -> &mut [#elem_type] {
                #as_mut_slice_body
            }

            fn attrs(&self) -> coral_nak_stubs::as_slice::AttrList<#type_enum> {
                #attrs_body
            }
        }
    };

    TokenStream::from(expanded)
}

fn is_boxed_variant(v: &syn::Variant) -> bool {
    match &v.fields {
        Fields::Unnamed(FieldsUnnamed { unnamed, .. }) if unnamed.len() == 1 => {
            let ty = &unnamed.first().unwrap().ty;
            if let Type::Path(TypePath { path, .. }) = ty {
                if let Some(seg) = path.segments.last() {
                    return seg.ident == "Box";
                }
            }
            false
        }
        _ => false,
    }
}

fn derive_as_slice_enum(
    enum_name: &Ident,
    e: &DataEnum,
    elem_type_name: &str,
    type_enum_name: &str,
    impl_generics: &syn::ImplGenerics<'_>,
    ty_generics: &syn::TypeGenerics<'_>,
    where_clause: Option<&syn::WhereClause>,
) -> TokenStream {
    let elem_type = Ident::new(elem_type_name, Span::call_site());
    let type_enum = Ident::new(type_enum_name, Span::call_site());

    let mut slice_arms = TokenStream2::new();
    let mut mut_slice_arms = TokenStream2::new();
    let mut attrs_arms = TokenStream2::new();

    for v in &e.variants {
        let case = &v.ident;
        let boxed = is_boxed_variant(v);

        if boxed {
            slice_arms.extend(quote! {
                #enum_name::#case(x) => coral_nak_stubs::as_slice::AsSlice::<#elem_type>::as_slice(x.as_ref()),
            });
            mut_slice_arms.extend(quote! {
                #enum_name::#case(x) => coral_nak_stubs::as_slice::AsSlice::<#elem_type>::as_mut_slice(x.as_mut()),
            });
            attrs_arms.extend(quote! {
                #enum_name::#case(x) => coral_nak_stubs::as_slice::AsSlice::<#elem_type>::attrs(x.as_ref()),
            });
        } else {
            slice_arms.extend(quote! {
                #enum_name::#case(x) => coral_nak_stubs::as_slice::AsSlice::<#elem_type>::as_slice(x),
            });
            mut_slice_arms.extend(quote! {
                #enum_name::#case(x) => coral_nak_stubs::as_slice::AsSlice::<#elem_type>::as_mut_slice(x),
            });
            attrs_arms.extend(quote! {
                #enum_name::#case(x) => coral_nak_stubs::as_slice::AsSlice::<#elem_type>::attrs(x),
            });
        }
    }

    let expanded = quote! {
        impl #impl_generics coral_nak_stubs::as_slice::AsSlice<#elem_type>
            for #enum_name #ty_generics #where_clause
        {
            type Attr = #type_enum;

            fn as_slice(&self) -> &[#elem_type] {
                match self {
                    #slice_arms
                }
            }

            fn as_mut_slice(&mut self) -> &mut [#elem_type] {
                match self {
                    #mut_slice_arms
                }
            }

            fn attrs(&self) -> coral_nak_stubs::as_slice::AttrList<#type_enum> {
                match self {
                    #attrs_arms
                }
            }
        }
    };

    TokenStream::from(expanded)
}

/// Derive `DisplayOp` for an enum that delegates to each variant's `DisplayOp` impl.
#[proc_macro_derive(DisplayOp)]
pub fn enum_derive_display_op(input: TokenStream) -> TokenStream {
    let DeriveInput { ident, data, .. } = parse_macro_input!(input);

    if let Data::Enum(e) = data {
        let mut fmt_dsts_cases = TokenStream2::new();
        let mut fmt_op_cases = TokenStream2::new();
        for v in e.variants {
            let case = v.ident;
            fmt_dsts_cases.extend(quote! {
                #ident::#case(x) => x.fmt_dsts(f),
            });
            fmt_op_cases.extend(quote! {
                #ident::#case(x) => x.fmt_op(f),
            });
        }
        quote! {
            impl DisplayOp for #ident {
                fn fmt_dsts(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    match self {
                        #fmt_dsts_cases
                    }
                }

                fn fmt_op(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    match self {
                        #fmt_op_cases
                    }
                }
            }
        }
        .into()
    } else {
        panic!("DisplayOp can only be derived for enums");
    }
}

fn into_box_inner_type(from_type: &Type) -> Option<&Type> {
    let last = match from_type {
        Type::Path(TypePath { path, .. }) => path.segments.last()?,
        _ => return None,
    };

    if last.ident != "Box" {
        return None;
    }

    let PathArguments::AngleBracketed(AngleBracketedGenericArguments { args, .. }) =
        &last.arguments
    else {
        panic!("Expected Box<T> (with angle brackets)");
    };

    for arg in args {
        if let GenericArgument::Type(inner_type) = arg {
            return Some(inner_type);
        }
    }
    panic!("Expected Box to use a type argument");
}

/// Derive `From<VariantType>` for each variant of an enum.
#[proc_macro_derive(FromVariants)]
pub fn derive_from_variants(input: TokenStream) -> TokenStream {
    let DeriveInput { ident, data, .. } = parse_macro_input!(input);
    let enum_type = ident;

    let mut impls = TokenStream2::new();

    if let Data::Enum(e) = data {
        for v in e.variants {
            let var_ident = v.ident;
            let from_type = match v.fields {
                Fields::Unnamed(FieldsUnnamed { unnamed, .. }) => unnamed,
                _ => panic!("Expected Op(OpFoo)"),
            };

            assert!(from_type.len() == 1, "Expected Op(OpFoo)");
            let from_type = &from_type.first().unwrap().ty;

            impls.extend(quote! {
                impl From<#from_type> for #enum_type {
                    fn from(op: #from_type) -> #enum_type {
                        #enum_type::#var_ident(op)
                    }
                }
            });

            if let Some(inner_type) = into_box_inner_type(from_type) {
                impls.extend(quote! {
                    impl From<#inner_type> for #enum_type {
                        fn from(value: #inner_type) -> Self {
                            From::from(Box::new(value))
                        }
                    }
                });
            }
        }
    }

    impls.into()
}

/// Check if a type is named `name` (e.g. `Src`, `Dst`).
fn is_type_named(ty: &Type, name: &str) -> bool {
    if let Type::Path(TypePath { path, .. }) = ty {
        if let Some(seg) = path.segments.last() {
            return seg.ident == name && seg.arguments.is_empty();
        }
    }
    false
}

/// If the type is `[T; N]` where T matches `name`, return N.
fn array_of_type(ty: &Type, name: &str) -> Option<usize> {
    if let Type::Array(arr) = ty {
        if is_type_named(&arr.elem, name) {
            if let Expr::Lit(lit) = &arr.len {
                if let syn::Lit::Int(i) = &lit.lit {
                    return i.base10_parse().ok();
                }
            }
        }
    }
    None
}

/// Extract the variant identifier from `#[attr_name(Variant)]`.
fn get_field_attr(field: &Field, attr_name: &str) -> Option<Ident> {
    for attr in &field.attrs {
        if attr.path().is_ident(attr_name) {
            if let Meta::List(list) = &attr.meta {
                let tokens = list.tokens.clone();
                let ident: Ident = syn::parse2(tokens).ok()?;
                return Some(ident);
            }
        }
    }
    None
}
