//! `#[derive(WitSerialize)]`: serde's `Serialize` for a type wasmtime's `bindgen!` makes of a WIT
//! record, variant or enum, which it is given as one of the bindgen's `additional_derives`. Each
//! field and case is serialized by its WIT name, which the bindgen writes on it as
//! `#[component(name = "…")]`: `dropped-before`, not `dropped_before`; `stdout`, not `Stdout`. So
//! JSON of a value reads as the WIT says it: a record an object of its fields, a case its name, or
//! an object of its name holding its payload, an enum's case its name; an option `null` or its
//! value, and a list or a tuple an array, as serde serializes them.
//!
//! The invariant: a name comes from the type's `component` attribute alone, never from its Rust
//! name, so what is serialized is the WIT's, as wasmtime lifts and lowers it.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::spanned::Spanned;
use syn::{Attribute, Data, DeriveInput, Fields, LitStr, parse_macro_input};

/// serde's `Serialize` for a record, variant or enum of wasmtime's bindgen, by its WIT names.
#[proc_macro_derive(WitSerialize, attributes(component))]
pub fn derive_wit_serialize(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    serialize(&input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn serialize(input: &DeriveInput) -> syn::Result<Tokens> {
    let body = match &input.data {
        Data::Struct(record) => serialize_record(input, &record.fields)?,
        Data::Enum(cases) => serialize_cases(input, cases)?,
        Data::Union(_) => {
            return Err(syn::Error::new(input.span(), "a WIT type is never a union"));
        }
    };
    let name = &input.ident;
    let (impl_generics, type_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::serde::Serialize for #name #type_generics #where_clause {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> ::core::result::Result<S::Ok, S::Error> {
                #body
            }
        }
    })
}

/// A record: a struct of its fields by their WIT names.
fn serialize_record(input: &DeriveInput, fields: &Fields) -> syn::Result<Tokens> {
    let Fields::Named(fields) = fields else {
        return Err(syn::Error::new(
            input.span(),
            "a WIT record has named fields",
        ));
    };
    let type_name = input.ident.to_string();
    let count = fields.named.len();
    let serialized = fields
        .named
        .iter()
        .map(|field| {
            let name = wit_name(&field.attrs, field.span())?;
            let ident = &field.ident;
            Ok(quote! { record.serialize_field(#name, &self.#ident)?; })
        })
        .collect::<syn::Result<Vec<Tokens>>>()?;
    Ok(quote! {
        use ::serde::ser::SerializeStruct as _;
        let mut record = serializer.serialize_struct(#type_name, #count)?;
        #(#serialized)*
        record.end()
    })
}

/// A variant or an enum: each case by its WIT name, with its payload where it has one.
fn serialize_cases(input: &DeriveInput, cases: &syn::DataEnum) -> syn::Result<Tokens> {
    let type_name = input.ident.to_string();
    let arms = cases
        .variants
        .iter()
        .enumerate()
        .map(|(index, case)| {
            let name = wit_name(&case.attrs, case.span())?;
            let index = u32::try_from(index).expect("a WIT type has fewer than 2^32 cases");
            let ident = &case.ident;
            match &case.fields {
                Fields::Unit => Ok(quote! {
                    Self::#ident => serializer.serialize_unit_variant(#type_name, #index, #name),
                }),
                Fields::Unnamed(payload) if payload.unnamed.len() == 1 => Ok(quote! {
                    Self::#ident(payload) => serializer.serialize_newtype_variant(#type_name, #index, #name, payload),
                }),
                _ => Err(syn::Error::new(
                    case.span(),
                    "a WIT case has one payload or none",
                )),
            }
        })
        .collect::<syn::Result<Vec<Tokens>>>()?;
    Ok(quote! {
        match self {
            #(#arms)*
        }
    })
}

/// The WIT name `#[component(name = "…")]` gives a field or a case.
fn wit_name(attrs: &[Attribute], span: proc_macro2::Span) -> syn::Result<LitStr> {
    let mut found = None;
    for attr in attrs
        .iter()
        .filter(|attr| attr.path().is_ident("component"))
    {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("name") {
                found = Some(meta.value()?.parse::<LitStr>()?);
            }
            Ok(())
        })?;
    }
    found.ok_or_else(|| {
        syn::Error::new(
            span,
            "a field or case of wasmtime's bindgen carries its WIT name as #[component(name = \"…\")]",
        )
    })
}
