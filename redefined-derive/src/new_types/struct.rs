use std::collections::VecDeque;

use proc_macro2::TokenStream;
use quote::{quote, ToTokens};
use syn::{self, parse::Parse, spanned::Spanned, Attribute, DataStruct, Field, Fields, Generics, Ident, Type, Visibility};

use super::parse_attributes;
use crate::attributes::{
    primitives::is_simple_primitive,
    symbol::{USE_DEFAULT_FIELD, USE_FIELD, USE_SAME_FIELD_VALUE},
    ContainerAttributes,
};

pub fn parse_new_struct(
    data_struct: &DataStruct,
    struct_name: &Ident,
    new_struct_name: &Ident,
    generics: &Generics,
    visibility: &Visibility,
    attributes: &[Attribute],
    is_remote: bool,
) -> syn::Result<TokenStream> {
    let fields = match &data_struct.fields {
        Fields::Named(fields_named) => &fields_named.named,
        Fields::Unnamed(fields_unnamed) => &fields_unnamed.unnamed,
        _ => return Err(syn::Error::new_spanned(&data_struct.fields, "Expected a struct with named/unnamed fields")),
    };

    let (derive_attrs, container_attrs) = parse_attributes(attributes, struct_name.span())?;

    let struct_fields = fields
        .iter()
        .map(|field| parse_field(field, is_remote))
        .collect::<syn::Result<Vec<_>>>()?;

    let tokens = if let Some(semi_token) = data_struct.semi_token {
        quote! {
            #[derive(#(#derive_attrs),*)]
            #[redefined(#struct_name)]
            #(#container_attrs)*
            #visibility struct #new_struct_name #generics (#(#struct_fields),*)#semi_token
        }
    } else {
        quote! {
            #[derive(#(#derive_attrs),*)]
            #[redefined(#struct_name)]
            #(#container_attrs)*
            #visibility struct #new_struct_name #generics {
                #(#struct_fields),*
            }
        }
    };

    Ok(tokens)
}

pub fn parse_field(field: &Field, is_remote: bool) -> syn::Result<TokenStream> {
    let ident = &field.ident;
    let _mutability = &field.mutability;
    let colon_token = field.colon_token;
    let vis = &field.vis;
    let mut ty = field.ty.clone();
    let mut copied_field_attrs = Vec::new();
    let mut field_attrs = Vec::new();

    for attr in &field.attrs {
        if attr.path().is_ident("redefined") {
            field_attrs = attr.parse_args_with(ContainerAttributes::parse)?.0;
        } else {
            copied_field_attrs.push(attr)
        }
    }

    if let Some(attr) = field_attrs.iter().find(|s| s.symbol == USE_FIELD).cloned() {
        let mut attr_types = attr.list_tuple_idents.unwrap().into();

        /*
        panic!("#[redefined(field(...)) must either have 0 (default redefined type) or 1 (custom redefined type) in 'field(...)'")
         */

        ty = parse_type_to_redefined(&ty, &mut attr_types, false);

        if !attr_types.is_empty() {
            panic!("#[redefined(field(...))' must have the same length as the number of types - remaining: {:?}", attr_types)
        }
    } else if is_remote {
        ty = parse_type_to_redefined(&ty, &mut VecDeque::new(), true);
    }

    let tokens = quote! {
        #(#copied_field_attrs)*
        #vis #ident #colon_token #ty
    };

    Ok(tokens)
}

pub fn parse_type_to_redefined(src_type: &Type, new_type_names: &mut VecDeque<(Ident, Ident)>, is_remote: bool) -> Type {
    /*
    match &src_type {
        Type::BareFn(_) => unimplemented!(),
        Type::Group(_) => unimplemented!(),
        Type::ImplTrait(_) => unimplemented!(),
        Type::Infer(_) => unimplemented!(),
        Type::Macro(_) => unimplemented!(),
        Type::Never(_) => unimplemented!(),
        Type::Paren(_) => unimplemented!(),
        Type::Ptr(_) => unimplemented!(),

        Type::TraitObject(t) => unimplemented!(),
        Type::Tuple(t) => unimplemented!(), // add this 1
        Type::Verbatim(_) => unimplemented!(),
    }
    */

    match src_type {
        Type::Array(a) => {
            let mut array = a.clone();
            let new_type = parse_type_to_redefined(&a.elem, new_type_names, is_remote);
            array.elem = Box::new(new_type);
            Type::Array(array)
        }
        Type::Reference(r) => {
            let mut refer = r.clone();
            let new_type = parse_type_to_redefined(&r.elem, new_type_names, is_remote);
            refer.elem = Box::new(new_type);
            Type::Reference(refer)
        }
        Type::Slice(s) => {
            let mut slice = s.clone();
            let new_type = parse_type_to_redefined(&s.elem, new_type_names, is_remote);
            slice.elem = Box::new(new_type);
            Type::Slice(slice)
        }
        Type::Path(p) => {
            let mut path = p.clone();
            //panic!("TOOOO\n {:?}\n", p.path.get_ident());
            path.path.segments.iter_mut().for_each(|seg| {
                //panic!("TOOOO\n {}\n{}", seg.ident, Primitive::is_primitive(&seg.ident));
                //panic!("TOOOO\n {}\n{:?}\n{}", seg.ident, new_type_names.pop_front(),
                // new_type_names.len(), seg.arguments);

                if let Some((source, target)) = new_type_names
                    .clone()
                    .iter()
                    .find(|(source, _)| source == &seg.ident)
                {
                    if &seg.ident == source {
                        if target == USE_DEFAULT_FIELD {
                            //panic!("TOOOO\n {:?}\n", seg.ident);
                            seg.ident = Ident::new(&format!("{}Redefined", seg.ident), seg.span())
                        } else if target == USE_SAME_FIELD_VALUE {
                            ()
                        } else {
                            seg.ident = target.clone()
                        }
                    }
                    new_type_names.retain(|(s, t)| s != source && t != target);
                } else {
                    match &mut seg.arguments {
                        syn::PathArguments::None => {
                            //panic!("TOOOO\n {}\n{}", seg.ident, Primitive::is_primitive(&seg.ident));
                            if let Some((source, target)) = new_type_names.pop_front() {
                                if seg.ident == source {
                                    if target == USE_DEFAULT_FIELD {
                                        seg.ident = Ident::new(&format!("{}Redefined", seg.ident), seg.span())
                                    } else {
                                        seg.ident = target
                                    }
                                }
                            } else if is_remote {
                                if !is_simple_primitive(&seg.ident.to_string()) {
                                    seg.ident = Ident::new(&format!("{}Redefined", seg.ident), seg.span())
                                }
                            }
                        }

                        syn::PathArguments::AngleBracketed(a) => a.args.iter_mut().for_each(|arg| match arg {
                            syn::GenericArgument::Type(t) => *t = parse_type_to_redefined(&t, new_type_names, is_remote),
                            _ => (),
                        }),
                        syn::PathArguments::Parenthesized(p) => p
                            .inputs
                            .iter_mut()
                            .for_each(|t| *t = parse_type_to_redefined(&t, new_type_names, is_remote)),
                    }
                }
            });

            Type::Path(path)
        }
        _ => panic!("FIELD IS OF TYPE: {}", src_type.to_token_stream()),
    }
}
