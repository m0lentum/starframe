//! This crate provides procedural macros for use with the moleengine_ecs crate
//! to generate Systems with minimal boilerplate code and a pretty API.

#![recursion_limit = "128"]

extern crate proc_macro;

use syn;

use proc_macro::TokenStream;
use quote::quote;

// the macros here translate to something like the following test
// (unless I've changed them and forgotten to update this, which is not unlikely):
//
// #[derive(Debug)]
// pub struct Position {
//     pub x: f32,
//     pub y: f32,
// }
// pub struct Velocity {
//     pub x: f32,
//     pub y: f32,
// }
//
// //#[system_target]
// pub struct PositionIntegrator<'a> {
//     position: &'a mut Position,
//     velocity: &'a Velocity,
// }
//
//pub struct MoverRunner;
//impl SystemRunner for MoverRunner {
//    fn run(space: &Space) {
//        let position = space.open::<Position>();
//        let velocity = space.open::<Velocity>();
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
//        let iter = and_set.iter();
//        let mut items: Vec<_> = iter
//            .map(|id| unsafe {
//                Mover {
//                    position: position_access.get_mut_raw(id as IdType).as_mut().unwrap(),
//                    velocity: velocity_access.get(id as IdType),
//                }
//            })
//            .collect();
//
//        Mover::operate(items.as_mut_slice());
//    }
//}
//
//impl<'a> System for PositionIntegrator<'a> {
//    type Runner = PositionIntegratorRunner;
//    //#[system_logic]
//    fn operate(items: &mut [Self]) {
//        for item in items {
//             item.position.x += item.velocity.x;
//             item.position.y += item.velocity.y;
//        }
//    }
//}

/// This attribute denotes a struct that gathers Components for a System to use.
/// Declaring a field as mutable gives write access to the corresponding Component.
///
/// Fields must be named and they must all be reference types.
/// This must be used in conjunction with a function with the #[system_logic] attribute
/// to operate on the data this attribute provides; otherwise you will get a compiler error.
///
/// See system_logic for usage information.
#[proc_macro_attribute]
pub fn system_item(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(item as syn::ItemStruct);
    let vis = &input.vis;
    let ident = &input.ident;
    let fields = &input.fields;

    let runner_ident = append_ident(ident, "Runner");

    let mut accesses = Vec::new();
    let mut users_idents = Vec::new();
    let mut field_getters = Vec::new();
    for field in fields {
        let ident = field.ident.as_ref().expect("Struct fields must be named");
        let access_ident = append_ident(ident, "_access");
        let users_ident = append_ident(ident, "_users");

        let field_type_ref = match &field.ty {
            syn::Type::Reference(r) => r,
            _ => panic!("System must only contain reference types"),
        };
        let ty = &field_type_ref.elem;

        match field_type_ref.mutability {
            Some(_) => {
                let access = quote! {
                    let #ident = space.open::<#ty>();
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
                    let #ident = space.open::<#ty>();
                    let #access_ident = #ident.read();
                    let #users_ident = #ident.get_users();
                };
                accesses.push(access);

                let getter = quote! {
                    #ident: #access_ident.get(id as IdType),
                };
                field_getters.push(getter);
            }
        }
        users_idents.push(users_ident);
    }

    let runner = quote! {
        #input

        #vis struct #runner_ident;
        impl SystemRunner for #runner_ident {
            fn run(space: &Space) {
                #(#accesses)*

                let alive = space.get_alive();
                let and_set = hibitset::BitSetAll{};
                #(let and_set = hibitset::BitSetAnd(and_set, #users_idents);)*
                let iter = and_set.iter();

                let mut items: Vec<_> = iter.map(|id| unsafe {
                    #ident {
                        #(#field_getters)*
                    }
                }).collect();

                #ident::operate(items.as_mut_slice());
            }
        }
    };

    runner.into()
}

const MSG_ARG_MISMATCH: &str =
    "System logic function must take a mutable slice of a system data struct as argument";

/// This attribute denotes a function that operates on a struct marked with #[system_item]
///
/// The function must take as its only argument a mutable slice of a system_item.
///
/// # Example
/// ```
/// #[system_item]
/// pub struct Mover<'a> {
///     position: &'a mut Position,
///     velocity: &'a Velocity,
/// }
///
/// #[system_logic]
/// fn do_move(items: &mut [Mover<'_>]) {
///     for item in items {
///         item.position.x += item.velocity.x;
///         item.position.y += item.velocity.y;
///     }
/// }
/// ```
/// Running this with `Space::run_system::<Mover>()` gives do_move the position (read/write)
/// and the velocity (read only) of every Entity in the Space that has both of them associated with it.
#[proc_macro_attribute]
pub fn system_logic(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input! {item as syn::ItemFn};
    let block = &input.block;
    let arg = input
        .decl
        .inputs
        .first()
        .expect("System logic function must have one argument")
        .into_value();
    // the argument is a Captured Reference to a Slice of the desired type.
    // this gets a little cumbersome to write
    let arg_cap = match arg {
        syn::FnArg::Captured(a) => a,
        _ => panic!(MSG_ARG_MISMATCH),
    };
    let arg_ref = match &arg_cap.ty {
        syn::Type::Reference(r) => r,
        _ => panic!(MSG_ARG_MISMATCH),
    };
    arg_ref.mutability.expect(MSG_ARG_MISMATCH);
    let arg_slice = match &*arg_ref.elem {
        syn::Type::Slice(s) => s,
        _ => panic!(MSG_ARG_MISMATCH),
    };
    let type_ident = match &*arg_slice.elem {
        syn::Type::Path(p) => {
            &p.path
                .segments
                .last()
                .expect(MSG_ARG_MISMATCH)
                .into_value()
                .ident
        }
        _ => panic!(MSG_ARG_MISMATCH),
    };
    let runner_ident = append_ident(type_ident, "Runner");

    let arg_ident = match &arg_cap.pat {
        syn::Pat::Ident(i) => &i.ident,
        _ => panic!("Invalid function argument identifier"),
    };

    let result = quote! {
        impl System for #type_ident<'_> {
            type Runner = #runner_ident;
            fn operate (#arg_ident: &mut [Self]) {
                #block
            }
        }
    };

    result.into()
}

fn append_ident(ident: &syn::Ident, postfix: &str) -> syn::Ident {
    syn::Ident::new(
        format!("{}{}", ident.clone(), postfix).as_str(),
        ident.span(),
    )
}
