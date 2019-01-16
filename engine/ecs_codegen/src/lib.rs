//! This crate provides procedural macros for use with the moleengine_ecs crate
//! to generate Systems with minimal boilerplate code and a pretty API.

#![recursion_limit = "128"]

extern crate proc_macro;

use syn;

use proc_macro::TokenStream;
use quote::quote;

// this macro translates to something like the following test
// (unless I've changed it and forgotten to update this, which is not unlikely):
//
//#[derive(Debug)]
//pub struct Position {
//    pub x: f32,
//    pub y: f32,
//}
//pub struct Velocity {
//    pub x: f32,
//    pub y: f32,
//}
//
//#[derive(ComponentFilter)]
//pub struct PosVel<'a> {
//    #[id] id: IdType,
//    position: &'a mut Position,
//    velocity: &'a Velocity,
//}
//
//impl<'a> ComponentFilter<'a> for PosVel<'a> {
//    fn run(space: &Space, mut f: impl FnMut(&mut [Self])) -> Option<()> {
//        let position = space.try_open_container::<Position>()?;
//        let velocity = space.try_open_container::<Velocity>()?;
//        let mut position_access = position.write();
//        let velocity_access = velocity.read();
//
//        let alive = space.get_alive();
//        let position_users = position.get_users();
//        let velocity_users = velocity.get_users();
//
//        let and_set = hibitset::BitSetAll;
//        let and_set = hibitset::BitSetAnd(position_users, and_set);
//        let and_set = hibitset::BitSetAnd(velocity_users, and_set);
//        let and_set = hibitset::BitSetAnd(alive, and_set);
//
//        use hibitset::BitSetLike;
//        let iter = and_set.iter();
//        let mut items: Vec<_> = iter
//            .map(|id| unsafe {
//                PosVel {
//                    position: position_access.get_mut_raw(id as IdType).as_mut().unwrap(),
//                    velocity: velocity_access.get_raw(id as IdType).as_ref().unwrap(),
//                    id: id as IdType,
//                }
//            })
//            .collect();
//
//        f(items.as_mut_slice());
//
//        Some(())
//    }
//}

/// This attribute denotes a struct that gathers Components from a Space, primarily used by Systems
/// but available elsewhere as well.
/// Declaring a field as mutable gives write access to the corresponding component.
///
/// The struct must have exactly one lifetime parameter.
/// Fields must be named and they must all be reference types. Additionally, an optional IdType field
/// may be provided with the #[id] attribute if you need to identify objects.
/// #Example
/// ```
///#[derive(ComponentFilter)]
///pub struct PosVel<'a> {
///    #[id] id: IdType,
///    position: &'a mut Position,
///    velocity: &'a Velocity,
///}
///```
#[proc_macro_derive(ComponentFilter, attributes(id))]
pub fn system_item(item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::ItemStruct);
    let ident = &input.ident;
    let fields = &input.fields;

    let mut accesses = Vec::new();
    let mut users_idents = Vec::new();
    let mut field_getters = Vec::new();
    let mut id_field = None;
    for field in fields {
        if is_id(field) {
            id_field = field.ident.as_ref();
        } else {
            let ident = field.ident.as_ref().expect("Struct fields must be named");
            let access_ident = append_ident(ident, "_access");
            let users_ident = append_ident(ident, "_users");

            let field_type_ref = match &field.ty {
                syn::Type::Reference(r) => r,
                _ => panic!("Filter must only contain reference types (maybe you're missing a #[id] attribute?)"),
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

                    let getter = quote! {
                        #ident: #access_ident.get_mut_raw(id as IdType).as_mut().unwrap(),
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

                    let getter = quote! {
                        #ident: #access_ident.get_raw(id as IdType).as_ref().unwrap(),
                    };
                    field_getters.push(getter);
                }
            }
            users_idents.push(users_ident);
        }
    }

    let id_setter = id_field.map(|ident| {
        quote! {
            #ident: id as IdType,
        }
    });

    let runner = quote! {
        impl<'a> ComponentFilter<'a> for #ident<'a> {
            fn run(space: &Space, mut f: impl FnMut(&mut [Self])) -> Option<()> {
                #(#accesses)*

                let alive = space.get_alive();
                let and_set = hibitset::BitSetAll{};
                #(let and_set = hibitset::BitSetAnd(and_set, #users_idents);)*

                use hibitset::BitSetLike;
                let iter = and_set.iter();
                let mut items: Vec<_> = iter.map(|id| unsafe {
                    #ident {
                        #(#field_getters)*
                        #id_setter
                    }
                }).collect();

                f(items.as_mut_slice());

                Some(())
            }
        }
    };

    runner.into()
}

fn is_id(field: &syn::Field) -> bool {
    for attr in &field.attrs {
        let meta = attr.parse_meta().unwrap();
        match meta {
            syn::Meta::Word(ref ident) if ident == "id" => return true,
            _ => (),
        };
    }

    false
}

fn append_ident(ident: &syn::Ident, postfix: &str) -> syn::Ident {
    syn::Ident::new(
        format!("{}{}", ident.clone(), postfix).as_str(),
        ident.span(),
    )
}
