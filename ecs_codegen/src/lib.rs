//! This crate provides procedural macros for use with the moleengine_ecs crate
//! to generate Systems with minimal boilerplate code and a pretty API.

#![recursion_limit = "128"]

extern crate proc_macro;

use syn;

use proc_macro::TokenStream;
use quote::quote;

/// This attribute denotes a struct that gathers Components from a Space, primarily used by Systems
/// but available elsewhere as well.
/// Declaring a field as mutable gives write access to the corresponding component.
///
/// The struct must have exactly one lifetime parameter.
/// Fields must be named and they must all be reference types or Options containing reference types.
/// Options, instead of filtering out an object if it does not have a given component, are set to None.
/// Additionally, two optional annotated fields can be included to gain extra information:
/// an IdType field with the #[id] attribute to identify objects,
/// and a bool field with the #[enabled] attribute to identify the enabled/disabled state of an object.
/// If an #[enabled] field is not provided, disabled objects will be filtered out.
/// #Example
/// ```
///#[derive(ComponentFilter)]
///pub struct Filter<'a> {
///    #[id] id: IdType,
///    #[enabled] is_enabled: bool,
///    mutable_thing: &'a mut Thing,
///    immutable_thing: &'a OtherThing,
///    optional_thing: Option<&'a Something>,
///}
///```
#[proc_macro_derive(ComponentFilter, attributes(id, enabled))]
pub fn system_item(item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::ItemStruct);
    let ident = &input.ident;
    let fields = &input.fields;
    let generics = &input.generics.params;
    let where_clause = &input.generics.where_clause;

    let mut lifetime = None;
    let mut generics_idents = Vec::new();
    for param in generics {
        match param {
            syn::GenericParam::Lifetime(def) => match lifetime {
                Some(_) => panic!("Filter must have exactly one lifetime parameter"),
                None => lifetime = Some(def),
            },
            syn::GenericParam::Type(t) => generics_idents.push(&t.ident),
            syn::GenericParam::Const(_) => {
                panic!("Const type parameters aren't supported (yet?) on ComponentFilters")
            }
        }
    }
    let lifetime = lifetime.expect("Filter must have exactly one lifetime parameter");
    let lifetime_ident = &lifetime.lifetime;

    let mut container_vars = Vec::new();
    let mut accesses = Vec::new();
    let mut users_idents = Vec::new();
    let mut field_getters = Vec::new();
    let mut id_field = None;
    let mut enabled_field = None;
    for field in fields {
        match field_type(field) {
            FieldType::Id => id_field = field.ident.as_ref(),
            FieldType::Enabled => enabled_field = field.ident.as_ref(),
            FieldType::Accessor => {
                let ident = field.ident.as_ref().expect("Struct fields must be named");
                container_vars.push(ident);
                let access_ident = append_ident(ident, "_access");
                let users_ident = append_ident(ident, "_users");

                let (field_type_ref, field_is_optional) = match &field.ty {
                    syn::Type::Reference(r) => (r, false),
                    syn::Type::Path(p) => {
                        let seg = p.path.segments.last().unwrap().into_value();
                        if seg.ident == "Option" {
                            match &seg.arguments {
                                syn::PathArguments::AngleBracketed(args) => {
                                    match args.args.first().unwrap().into_value() {
                                        syn::GenericArgument::Type(syn::Type::Reference(r)) => (r, true),
                                        _ => panic!("Option must contain a reference type"),
                                    }
                                }
                                _ => panic!("Option must have angle bracketed arguments"),
                            }
                        } else {
                            panic!("Filter must only contain reference and Option types (maybe you're missing a #[id] attribute?)")
                        }
                    }
                    _ => panic!("Filter must only contain reference and Option types (maybe you're missing a #[id] attribute?)"),
                };
                let ty = &field_type_ref.elem;

                match field_type_ref.mutability {
                    Some(_) => {
                        let access = quote! {
                            let #ident = space.try_open_container::<#ty>()?;
                            let mut #access_ident = #ident.write();
                            let #users_ident = #ident.get_users();
                        };
                        accesses.push(access);

                        let getter = if !field_is_optional {
                            quote! {
                                #ident: #access_ident.get_mut_raw(id).as_mut().unwrap(),
                            }
                        } else {
                            quote! {
                                #ident: if #users_ident.contains(id as u32) {
                                    Some(#access_ident.get_mut_raw(id).as_mut().unwrap())
                                } else {
                                    None
                                },
                            }
                        };
                        field_getters.push(getter);
                    }
                    None => {
                        let access = quote! {
                            let #ident = space.try_open_container::<#ty>()?;
                            let #access_ident = #ident.read();
                            let #users_ident = #ident.get_users();
                        };
                        accesses.push(access);

                        let getter = if !field_is_optional {
                            quote! {
                                #ident: #access_ident.get_raw(id).as_ref().unwrap(),
                            }
                        } else {
                            quote! {
                                #ident: if #users_ident.contains(id as u32) {
                                    Some(#access_ident.get_raw(id).as_ref().unwrap())
                                } else {
                                    None
                                },
                            }
                        };
                        field_getters.push(getter);
                    }
                }
                if !field_is_optional {
                    users_idents.push(users_ident);
                }
            }
        }
    }

    let id_setter = id_field.map(|ident| {
        quote! {
            #ident: id,
        }
    });

    let enabled_setter = enabled_field.map(|ident| {
        quote! {
            #ident: space.get_enabled().contains(id as u32),
        }
    });

    let disabled_filter = match enabled_field {
        Some(_) => None,
        None => Some(quote! {
            let and_set = hibitset::BitSetAnd(and_set, space.get_enabled());
        }),
    };

    let result = quote! {
        impl<#generics> ComponentFilter<#lifetime> for #ident<#lifetime_ident, #(#generics_idents,)*>
        #where_clause
        {
            fn run_filter(space: &moleengine::ecs::Space, mut f: impl FnOnce(&mut [Self])) -> Option<()> {
                #(#accesses)*

                let and_set = space.get_alive();
                #(let and_set = hibitset::BitSetAnd(and_set, #users_idents);)*

                #disabled_filter

                use hibitset::BitSetLike;
                let iter = and_set.iter();
                let mut items: Vec<_> = iter
                    .map(|id| id as moleengine::ecs::IdType)
                    .filter(|id| {
                        let space_gen = space.get_gen(*id);
                        space_gen <= 1
                        || (#(space_gen == #container_vars.get_gen(*id) && )* true)
                    })
                    .map(|id| unsafe {
                    #ident {
                        #(#field_getters)*
                        #id_setter
                        #enabled_setter
                    }
                }).collect();

                f(items.as_mut_slice());

                Some(())
            }
        }
    };

    result.into()
}

enum FieldType {
    Id,
    Enabled,
    Accessor,
}

fn field_type(field: &syn::Field) -> FieldType {
    for attr in &field.attrs {
        let meta = attr.parse_meta().unwrap();
        match meta {
            syn::Meta::Word(ref ident) if ident == "id" => return FieldType::Id,
            syn::Meta::Word(ref ident) if ident == "enabled" => return FieldType::Enabled,
            _ => (),
        };
    }

    FieldType::Accessor
}

fn append_ident(ident: &syn::Ident, postfix: &str) -> syn::Ident {
    syn::Ident::new(
        format!("{}{}", ident.clone(), postfix).as_str(),
        ident.span(),
    )
}
