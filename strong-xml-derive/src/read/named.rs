use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, LitStr};

use crate::types::{Field, Type};

pub fn read(
    prefix: &Option<LitStr>,
    local: &LitStr,
    ele_name: TokenStream,
    fields: &[Field],
) -> TokenStream {
    let init_fields = fields.iter().map(|field| match field {
        Field::Attribute { bind, ty, .. }
        | Field::Child { bind, ty, .. }
        | Field::FlattenText { bind, ty, .. } => init_value(bind, ty),
        Field::Text { bind, .. } => quote! { let #bind; },
    });

    let return_fields = fields.iter().map(|field| match field {
        Field::Attribute {
            name,
            bind,
            ty,
            default,
            ..
        }
        | Field::Child {
            name,
            bind,
            ty,
            default,
            ..
        }
        | Field::FlattenText {
            name,
            bind,
            ty,
            default,
            ..
        } => return_value(name, bind, ty, *default, &ele_name),
        Field::Text { name, bind, ty, .. } => return_value(name, bind, ty, false, &ele_name),
    });

    let read_attr_fields = fields.iter().filter_map(|field| match field {
        Field::Attribute {
            bind,
            ty,
            prefix,
            tag,
            name,
            ..
        } => Some(read_attrs(&prefix, &tag, &bind, &name, &ty, &ele_name)),
        _ => None,
    });

    let read_child_fields = fields.iter().filter_map(|field| match field {
        Field::Child {
            bind,
            ty,
            tags,
            name,
            ..
        } => {
            let tags: Vec<_> = tags.iter().map(|tag|{
                if tag.value().matches(":").count() > 1 {
                    panic!("child cannot have more than one colon in name.")
                } 
                match tag.value().split_once(':') {
                    Some(("", local)) => (None, local.to_owned()),
                    Some((prefix, local)) => (Some(prefix.to_owned()), local.to_owned()),
                    None => (None, tag.value())
                }
            }).collect();

            Some(read_children(&tags[..], bind, name, ty, &ele_name))
        }
        _ => None,
    });

    let read_flatten_text_fields = fields.iter().filter_map(|field| match field {
        Field::FlattenText {
            bind,
            ty,
            prefix,
            tag,
            name,
            ..
        } => Some(read_flatten_text(prefix, tag, bind, name, ty, &ele_name)),
        _ => None,
    });

    let read_text_fields = fields.iter().filter_map(|field| match field {
        Field::Text { bind, ty, name, .. } => {
            Some(read_text(&prefix, &local, bind, name, ty, &ele_name))
        }
        _ => None,
    });

    let is_text_element = fields
        .iter()
        .any(|field| matches!(field, Field::Text { .. }));

    let return_fields = quote! {
        let __res = #ele_name {
            #( #return_fields, )*
        };

        strong_xml::log_finish_reading!(#ele_name);

        return Ok(__res);
    };

    let prefix = if let Some(lit) = prefix {
        quote!(#lit)
    } else {
        quote!("")
    };

    let read_content = if is_text_element {
        quote! {
            #( #read_text_fields )*
            #return_fields
        }
    } else {
        quote! {
            if let Token::ElementEnd { end: ElementEnd::Empty, .. } = reader.next().unwrap()? {
                #return_fields
            }

            while let Some((__prefix, __local)) = reader.find_element_start(Some((#prefix, #local)))? {
                match (__prefix, __local) {
                    #( #read_child_fields, )*
                    #( #read_flatten_text_fields, )*
                    (prefix, local) => {
                        strong_xml::log_skip_element!(#ele_name, prefix, local);
                        // skip the start tag
                        reader.next();
                        reader.read_to_end(prefix, local)?;
                    },
                }
            }

            #return_fields
        }
    };

    quote! {
        strong_xml::log_start_reading!(#ele_name);

        #( #init_fields )*

        reader.read_till_element_start(#prefix, #local)?;

        while let Some((__prefix, __key, __value)) = reader.find_attribute()? {
            match (__prefix, __key) {
                #( #read_attr_fields, )*
                (prefix, key) => {
                    strong_xml::log_skip_attribute!(#ele_name, prefix, key);
                },
            }
        }

        #read_content
    }
}

fn init_value(name: &Ident, ty: &Type) -> TokenStream {
    if ty.is_vec() {
        quote! { let mut #name = Vec::new(); }
    } else {
        quote! { let mut #name = None; }
    }
}

fn return_value(
    name: &TokenStream,
    bind: &Ident,
    ty: &Type,
    default: bool,
    ele_name: &TokenStream,
) -> TokenStream {
    if ty.is_vec() || ty.is_option() {
        quote! { #name: #bind }
    } else if default {
        quote! { #name: #bind.unwrap_or_default() }
    } else {
        quote! {
            #name: #bind.ok_or(XmlError::MissingField {
                name: stringify!(#ele_name).to_owned(),
                field: stringify!(#name).to_owned(),
            })?
        }
    }
}

fn read_attrs(
    prefix: &Option<LitStr>,
    local: &LitStr,
    bind: &Ident,
    name: &TokenStream,
    ty: &Type,
    ele_name: &TokenStream,
) -> TokenStream {
    let from_str = from_str(ty);

    let prefix = if let Some(lit) = prefix {
        quote!(#lit)
    } else {
        quote!("")
    };

    if ty.is_vec() {
        panic!("`attr` attribute doesn't support Vec.");
    } else {
        quote! {
            (#prefix, #local) => {
                strong_xml::log_start_reading_field!(#ele_name, #name);

                #bind = Some(#from_str);

                strong_xml::log_finish_reading_field!(#ele_name, #name);
            }
        }
    }
}

fn read_text(
    prefix: &Option<LitStr>,
    local: &LitStr,
    bind: &Ident,
    name: &TokenStream,
    ty: &Type,
    ele_name: &TokenStream,
) -> TokenStream {
    let from_str = from_str(ty);
    
    let prefix = if let Some(lit) = prefix {
        quote!(#lit)
    } else {
        quote!("")
    };

    if ty.is_vec() {
        panic!("`text` attribute doesn't support Vec.");
    } else {
        quote! {
            strong_xml::log_start_reading_field!(#ele_name, #name);

            let __value = reader.read_text(#prefix, #local)?;
            #bind = Some(#from_str);

            strong_xml::log_finish_reading_field!(#ele_name, #name);
        }
    }
}

fn read_children(
    tags: &[(Option<String>, String)],
    bind: &Ident,
    name: &TokenStream,
    ty: &Type,
    ele_name: &TokenStream,
) -> TokenStream {
    let from_reader = match &ty {
        Type::VecT(ty) => quote! {
            #bind.push(<#ty as strong_xml::XmlRead>::from_reader(reader)?);
        },
        Type::OptionT(ty) | Type::T(ty) => quote! {
            #bind = Some(<#ty as strong_xml::XmlRead>::from_reader(reader)?);
        },
        _ => panic!("`child` attribute only supports Vec<T>, Option<T> and T."),
    };

    let (prefixes, locals): (Vec<_>, Vec<_>) = tags.iter().cloned().unzip();
    let prefixes = prefixes.iter().map(|prefix| if let Some(lit) = prefix {
        quote!(#lit)
    } else {
        quote!("")
    });
    quote! {
        #( (#prefixes, #locals) )|* => {
            strong_xml::log_start_reading_field!(#ele_name, #name);

            #from_reader

            strong_xml::log_finish_reading_field!(#ele_name, #name);
        }
    }
}

fn read_flatten_text(
    prefix: &Option<LitStr>,
    local: &LitStr,
    bind: &Ident,
    name: &TokenStream,
    ty: &Type,
    ele_name: &TokenStream,
) -> TokenStream {
    let from_str = from_str(ty);

    let prefix = if let Some(lit) = prefix {
        quote!(#lit)
    } else {
        quote!("")
    };

    let read_text = if ty.is_vec() {
        quote! {
            let __value = reader.read_text(#prefix, #local)?;
            #bind.push(#from_str);
        }
    } else {
        quote! {
            let __value = reader.read_text(#prefix, #local)?;
            #bind = Some(#from_str);
        }
    };

    quote! {
        (#prefix, #local) => {
            // skip element start
            reader.next();

            strong_xml::log_start_reading_field!(#ele_name, #name);

            #read_text

            strong_xml::log_finish_reading_field!(#ele_name, #name);
        }
    }
}

fn from_str(ty: &Type) -> TokenStream {
    match &ty {
        Type::CowStr | Type::OptionCowStr | Type::VecCowStr => quote! { __value },
        Type::Bool | Type::OptionBool | Type::VecBool => quote! {
            match &*__value {
                "t" | "true" | "y" | "yes" | "on" | "1" => true,
                "f" | "false" | "n" | "no" | "off" | "0" => false,
                _ => <bool as std::str::FromStr>::from_str(&__value).map_err(|e| XmlError::FromStr(e.into()))?
            }
        },
        Type::T(ty) | Type::OptionT(ty) | Type::VecT(ty) => quote! {
            <#ty as std::str::FromStr>::from_str(&__value).map_err(|e| XmlError::FromStr(e.into()))?
        },
    }
}
