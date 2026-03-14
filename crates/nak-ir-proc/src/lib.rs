// SPDX-License-Identifier: AGPL-3.0-only
#![deny(unsafe_code)]
#![warn(missing_docs)]
//! Proc-macro derives for codegen IR types.
//!
//! Provides derive macros used by the codegen IR definition in
//! `codegen/ir/`:
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
    FieldsUnnamed, GenericArgument, Ident, LitStr, Meta, PathArguments, Type, TypePath,
    parse_macro_input,
};

struct MatchedField {
    ident: Ident,
    count: usize,
    attr_variant: Option<Ident>,
    per_element_attrs: Option<Vec<Ident>>,
    names: Option<Vec<Ident>>,
}

/// Derive `AsSlice<Src>` for instruction op structs.
///
/// Reads `#[src_type(Variant)]` attributes on `Src` fields to generate
/// a contiguous slice view over the struct's source operands.
#[proc_macro_derive(SrcsAsSlice, attributes(src_type, src_types, src_names))]
pub fn derive_srcs_as_slice(input: TokenStream) -> TokenStream {
    derive_as_slice(input, "Src", "src_type", "SrcType")
}

/// Derive `AsSlice<Dst>` for instruction op structs.
///
/// Reads `#[dst_type(Variant)]` attributes on `Dst` fields to generate
/// a contiguous slice view over the struct's destination operands.
#[proc_macro_derive(DstsAsSlice, attributes(dst_type, dst_types, dst_names))]
pub fn derive_dsts_as_slice(input: TokenStream) -> TokenStream {
    derive_as_slice(input, "Dst", "dst_type", "DstType")
}

/// Core logic for generating `AsSlice<T>` implementations.
///
/// Walks the struct fields, finds those matching `elem_type_name` (single or
/// array), reads the `#[attr_name(Variant)]` attribute for each, and generates
/// an `impl AsSlice<T>` that uses `repr(C)` layout to return a contiguous
/// slice across all matching fields.
///
/// # Panics
///
/// Panics if applied to a type that is not a struct with named fields or an enum.
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

    let fields = if let Data::Struct(DataStruct {
        fields: Fields::Named(f),
        ..
    }) = &input.data
    {
        &f.named
    } else {
        let msg = format!("{attr_name} can only be derived for structs with named fields");
        let lit = LitStr::new(&msg, Span::call_site());
        return TokenStream::from(quote! { compile_error!(#lit); });
    };

    let elem_type = Ident::new(elem_type_name, Span::call_site());
    let type_enum = Ident::new(type_enum_name, Span::call_site());

    let matched = collect_matched_fields(fields, elem_type_name, attr_name);
    let total_count: usize = matched.iter().map(|m| m.count).sum();

    if matched.is_empty() {
        return generate_empty_as_slice(
            struct_name,
            &elem_type,
            &type_enum,
            &impl_generics,
            &ty_generics,
            where_clause,
        );
    }

    let first_field = &matched[0].ident;
    let as_slice_body =
        generate_as_slice_body(struct_name, &elem_type, &matched, first_field, total_count);
    let as_mut_slice_body =
        generate_as_mut_slice_body(&elem_type, &matched, first_field, total_count);
    let attrs_body = generate_attrs_body(&type_enum, &matched);

    let accessors = generate_named_accessors(struct_name, &elem_type, &matched);

    let expanded = quote! {
        impl #impl_generics coral_reef_stubs::as_slice::AsSlice<#elem_type>
            for #struct_name #ty_generics #where_clause
        {
            type Attr = #type_enum;

            fn as_slice(&self) -> &[#elem_type] {
                #as_slice_body
            }

            fn as_mut_slice(&mut self) -> &mut [#elem_type] {
                #as_mut_slice_body
            }

            fn attrs(&self) -> coral_reef_stubs::as_slice::AttrList<#type_enum> {
                #attrs_body
            }
        }

        #accessors
    };

    TokenStream::from(expanded)
}

/// Generate named accessor methods for array fields with `#[src_names(...)]` or `#[dst_names(...)]`.
///
/// For `#[src_names(a, b, c)]` on `srcs: [Src; 3]`, generates:
/// ```text
/// impl OpFoo {
///     pub fn a(&self) -> &Src { &self.srcs[0] }
///     pub fn a_mut(&mut self) -> &mut Src { &mut self.srcs[0] }
///     // ...etc for b, c
/// }
/// ```
fn generate_named_accessors(
    struct_name: &Ident,
    elem_type: &Ident,
    matched: &[MatchedField],
) -> TokenStream2 {
    let mut methods = Vec::new();
    for m in matched {
        let Some(names) = &m.names else { continue };
        let field = &m.ident;
        for (i, name) in names.iter().enumerate() {
            let getter_mut = Ident::new(&format!("{name}_mut"), name.span());
            methods.push(quote! {
                #[inline]
                pub fn #name(&self) -> &#elem_type { &self.#field[#i] }
                #[inline]
                pub fn #getter_mut(&mut self) -> &mut #elem_type { &mut self.#field[#i] }
            });
        }
    }
    if methods.is_empty() {
        quote! {}
    } else {
        quote! {
            impl #struct_name {
                #(#methods)*
            }
        }
    }
}

fn collect_matched_fields(
    fields: &syn::punctuated::Punctuated<Field, syn::Token![,]>,
    elem_type_name: &str,
    attr_name: &str,
) -> Vec<MatchedField> {
    let plural_attr = format!("{attr_name}s");
    let names_attr = format!(
        "{}names",
        attr_name.strip_suffix("type").unwrap_or(attr_name)
    );
    let mut matched = Vec::new();
    for field in fields {
        let Some(field_ident) = field.ident.clone() else {
            continue;
        };
        let ty = &field.ty;

        if is_type_named(ty, elem_type_name) {
            let variant = get_field_attr(field, attr_name);
            matched.push(MatchedField {
                ident: field_ident,
                count: 1,
                attr_variant: variant,
                per_element_attrs: None,
                names: None,
            });
        } else if let Some(n) = array_of_type(ty, elem_type_name) {
            let variant = get_field_attr(field, attr_name);
            let per_elem = get_field_attr_list(field, &plural_attr);
            let names = get_field_attr_list(field, &names_attr);
            matched.push(MatchedField {
                ident: field_ident,
                count: n,
                attr_variant: variant,
                per_element_attrs: per_elem,
                names,
            });
        }
    }
    matched
}

fn generate_empty_as_slice(
    struct_name: &Ident,
    elem_type: &Ident,
    type_enum: &Ident,
    impl_generics: &syn::ImplGenerics<'_>,
    ty_generics: &syn::TypeGenerics<'_>,
    where_clause: Option<&syn::WhereClause>,
) -> TokenStream {
    TokenStream::from(quote! {
        impl #impl_generics coral_reef_stubs::as_slice::AsSlice<#elem_type>
            for #struct_name #ty_generics #where_clause
        {
            type Attr = #type_enum;
            fn as_slice(&self) -> &[#elem_type] { &[] }
            fn as_mut_slice(&mut self) -> &mut [#elem_type] { &mut [] }
            fn attrs(&self) -> coral_reef_stubs::as_slice::AttrList<#type_enum> {
                coral_reef_stubs::as_slice::AttrList::List(Vec::new())
            }
        }
    })
}

fn generate_as_slice_body(
    struct_name: &Ident,
    _elem_type: &Ident,
    matched: &[MatchedField],
    first_field: &Ident,
    _total_count: usize,
) -> TokenStream2 {
    if matched.len() == 1 && matched[0].count == 1 {
        quote! { std::slice::from_ref(&self.#first_field) }
    } else if matched.len() == 1 {
        quote! { &self.#first_field }
    } else {
        let field_names: Vec<String> = matched.iter().map(|m| m.ident.to_string()).collect();
        let msg = format!(
            "AsSlice: `{}` has {} separate fields of the same type ({}). \
             Merge them into a single array field to avoid unsafe from_raw_parts.",
            struct_name,
            matched.len(),
            field_names.join(", "),
        );
        let lit = LitStr::new(&msg, Span::call_site());
        quote! { compile_error!(#lit); }
    }
}

fn generate_as_mut_slice_body(
    _elem_type: &Ident,
    matched: &[MatchedField],
    first_field: &Ident,
    _total_count: usize,
) -> TokenStream2 {
    if matched.len() == 1 && matched[0].count == 1 {
        quote! { std::slice::from_mut(&mut self.#first_field) }
    } else if matched.len() == 1 {
        quote! { &mut self.#first_field }
    } else {
        quote! { compile_error!("AsSlice: multi-field as_mut_slice requires array field migration"); }
    }
}

fn generate_attrs_body(type_enum: &Ident, matched: &[MatchedField]) -> TokenStream2 {
    let default_variant = Ident::new("DEFAULT", Span::call_site());

    if matched.len() == 1 && matched[0].per_element_attrs.is_none() {
        let variant = matched[0].attr_variant.as_ref().map_or_else(
            || quote! { #type_enum::#default_variant },
            |v| quote! { #type_enum::#v },
        );
        quote! {
            coral_reef_stubs::as_slice::AttrList::Uniform(#variant)
        }
    } else {
        let mut attr_entries = Vec::new();
        for m in matched {
            if let Some(per_elem) = &m.per_element_attrs {
                for v in per_elem {
                    attr_entries.push(quote! { #type_enum::#v });
                }
            } else {
                let variant = m.attr_variant.as_ref().map_or_else(
                    || quote! { #type_enum::#default_variant },
                    |v| quote! { #type_enum::#v },
                );
                for _ in 0..m.count {
                    attr_entries.push(variant.clone());
                }
            }
        }
        quote! {
            coral_reef_stubs::as_slice::AttrList::List(
                vec![#(#attr_entries),*]
            )
        }
    }
}

fn is_boxed_variant(v: &syn::Variant) -> bool {
    match &v.fields {
        Fields::Unnamed(FieldsUnnamed { unnamed, .. }) if unnamed.len() == 1 => {
            let Some(first) = unnamed.first() else {
                return false;
            };
            let ty = &first.ty;
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
                #enum_name::#case(x) => coral_reef_stubs::as_slice::AsSlice::<#elem_type>::as_slice(x.as_ref()),
            });
            mut_slice_arms.extend(quote! {
                #enum_name::#case(x) => coral_reef_stubs::as_slice::AsSlice::<#elem_type>::as_mut_slice(x.as_mut()),
            });
            attrs_arms.extend(quote! {
                #enum_name::#case(x) => coral_reef_stubs::as_slice::AsSlice::<#elem_type>::attrs(x.as_ref()),
            });
        } else {
            slice_arms.extend(quote! {
                #enum_name::#case(x) => coral_reef_stubs::as_slice::AsSlice::<#elem_type>::as_slice(x),
            });
            mut_slice_arms.extend(quote! {
                #enum_name::#case(x) => coral_reef_stubs::as_slice::AsSlice::<#elem_type>::as_mut_slice(x),
            });
            attrs_arms.extend(quote! {
                #enum_name::#case(x) => coral_reef_stubs::as_slice::AsSlice::<#elem_type>::attrs(x),
            });
        }
    }

    let expanded = quote! {
        impl #impl_generics coral_reef_stubs::as_slice::AsSlice<#elem_type>
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

            fn attrs(&self) -> coral_reef_stubs::as_slice::AttrList<#type_enum> {
                match self {
                    #attrs_arms
                }
            }
        }
    };

    TokenStream::from(expanded)
}

/// Derive `DisplayOp` for an enum that delegates to each variant's `DisplayOp` impl.
///
/// # Panics
///
/// Panics if applied to a non-enum type.
#[proc_macro_derive(DisplayOp)]
pub fn enum_derive_display_op(input: TokenStream) -> TokenStream {
    let DeriveInput { ident, data, .. } = parse_macro_input!(input);

    let Data::Enum(e) = data else {
        return TokenStream::from(quote! {
            compile_error!("DisplayOp can only be derived for enums");
        });
    };

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
        return None;
    };

    args.iter().find_map(|arg| {
        if let GenericArgument::Type(inner_type) = arg {
            Some(inner_type)
        } else {
            None
        }
    })
}

/// Derive `From<VariantType>` for each variant of an enum.
///
/// # Panics
///
/// Panics if applied to a non-enum or an enum variant without exactly one unnamed field.
#[proc_macro_derive(FromVariants)]
pub fn derive_from_variants(input: TokenStream) -> TokenStream {
    let DeriveInput { ident, data, .. } = parse_macro_input!(input);
    let enum_type = ident;

    let mut impls = TokenStream2::new();

    if let Data::Enum(e) = data {
        for v in e.variants {
            let var_ident = v.ident;
            let Fields::Unnamed(FieldsUnnamed {
                unnamed: from_type, ..
            }) = v.fields
            else {
                let msg = format!(
                    "FromVariants: variant `{var_ident}` must have exactly one unnamed field"
                );
                let lit = LitStr::new(&msg, Span::call_site());
                return TokenStream::from(quote! { compile_error!(#lit); });
            };

            if from_type.len() != 1 {
                let msg =
                    format!("FromVariants: variant `{var_ident}` must have exactly one field");
                let lit = LitStr::new(&msg, Span::call_site());
                return TokenStream::from(quote! { compile_error!(#lit); });
            }
            let Some(first) = from_type.first() else {
                continue;
            };
            let from_type = &first.ty;

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

/// Extract a comma-separated list of identifiers from `#[attr_name(A, B, C)]`.
fn get_field_attr_list(field: &Field, attr_name: &str) -> Option<Vec<Ident>> {
    for attr in &field.attrs {
        if attr.path().is_ident(attr_name) {
            if let Meta::List(list) = &attr.meta {
                let tokens = list.tokens.to_string();
                let idents: Vec<Ident> = tokens
                    .split(',')
                    .map(|s| Ident::new(s.trim(), Span::call_site()))
                    .collect();
                if !idents.is_empty() {
                    return Some(idents);
                }
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// #[derive(Encode)] — type-safe instruction encoding via TypedBitField
// ---------------------------------------------------------------------------

/// Derive macro for generating an `encode()` method on AMD instruction structs.
///
/// Fields annotated with `#[enc(offset = N, width = M)]` produce typed
/// bitfield writes. The struct must also have `#[encoding(FORMAT)]` on the
/// struct level to select the word count.
///
/// # Panics
///
/// Panics if a named field in `#[derive(Encode)]` has no identifier
/// (structurally impossible for named fields, but guarded defensively).
///
/// # Example (conceptual — requires `bitview::TypedBitField` in scope)
///
/// ```text
/// #[derive(Encode)]
/// #[encoding(VOP3)] // 64-bit → 2 words
/// struct EncodedVop3Add {
///     #[enc(offset = 26, width = 6)]
///     prefix: u32,
///     #[enc(offset = 16, width = 10)]
///     opcode: u16,
///     #[enc(offset = 0, width = 8)]
///     vdst: u8,
///     #[enc(offset = 32, width = 9)]  // word 1
///     src0: u16,
///     #[enc(offset = 41, width = 9)]
///     src1: u16,
///     #[enc(offset = 50, width = 9)]
///     src2: u16,
/// }
/// ```
#[proc_macro_derive(Encode, attributes(enc, encoding))]
pub fn derive_encode(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let struct_name = &input.ident;

    let word_count = get_encoding_word_count(&input);

    let fields = match &input.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(named),
            ..
        }) => &named.named,
        _ => {
            return syn::Error::new(
                Span::call_site(),
                "Encode only supports structs with named fields",
            )
            .to_compile_error()
            .into();
        }
    };

    let mut set_statements = Vec::new();
    for field in fields {
        if let Some((offset, width)) = get_enc_attr(field) {
            let field_name = field
                .ident
                .as_ref()
                .expect("named field in #[derive(Encode)]");
            set_statements.push(quote! {
                {
                    let range = (#offset as usize)..((#offset + #width) as usize);
                    let val: u64 = bitview::BitCastU64::as_bits(self.#field_name);
                    buf.set_bit_range_u64(range, val);
                }
            });
        }
    }

    let expanded = quote! {
        impl #struct_name {
            /// Encode this instruction into `u32` words.
            pub fn encode(&self) -> [u32; #word_count] {
                use bitview::BitMutViewable;
                let mut buf = [0u32; #word_count];
                #(#set_statements)*
                buf
            }
        }
    };

    expanded.into()
}

fn get_encoding_word_count(input: &DeriveInput) -> usize {
    for attr in &input.attrs {
        if attr.path().is_ident("encoding") {
            if let Meta::List(list) = &attr.meta {
                let tokens = list.tokens.to_string();
                let format = tokens.trim();
                return match format {
                    "SOP1" | "SOP2" | "SOPC" | "SOPK" | "SOPP" | "VOP1" | "VOP2" | "VOPC" => 1,
                    _ => 2,
                };
            }
        }
    }
    2
}

fn get_enc_attr(field: &Field) -> Option<(u32, u32)> {
    for attr in &field.attrs {
        if attr.path().is_ident("enc") {
            if let Meta::List(list) = &attr.meta {
                let content = list.tokens.to_string();
                let mut offset = None;
                let mut width = None;
                for part in content.split(',') {
                    let part = part.trim();
                    if let Some(val) = part.strip_prefix("offset") {
                        let val = val.trim().trim_start_matches('=').trim();
                        offset = val.parse::<u32>().ok();
                    } else if let Some(val) = part.strip_prefix("width") {
                        let val = val.trim().trim_start_matches('=').trim();
                        width = val.parse::<u32>().ok();
                    }
                }
                if let (Some(o), Some(w)) = (offset, width) {
                    return Some((o, w));
                }
            }
        }
    }
    None
}
