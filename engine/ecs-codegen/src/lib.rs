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
// #[system_target]
// pub struct PositionIntegrator<'a> {
//     position: &'a mut Position,
//     velocity: &'a Velocity,
// }
//
// impl<'a> System for PositionIntegrator<'a> {
//     type Runner = PositionIntegratorRunner;
//     #[system_logic]
//     fn operate(item: Self) {
//         item.position.x += item.velocity.x;
//         item.position.y += item.velocity.y;
//         println!("position is {:?}", item.position);
//     }
// }
//
// pub struct PositionIntegratorRunner;
// impl SystemRunner for PositionIntegratorRunner {
//     fn run(space: &Space) {
//         let position = space.open::<Position>();
//         let velocity = space.open::<Velocity>();
//         let mut position_access = position.write();
//         let velocity_access = velocity.read();
//
//         let alive = space.get_alive();
//         let position_users = position.get_users();
//         let velocity_users = velocity.get_users();
//
//         let and_set = BitSetAnd(position_users, velocity_users);
//         let and_set = BitSetAnd(alive, and_set);
//         let iter = and_set.iter();
//
//         for id in iter {
//             let item = unsafe {
//                 PositionIntegrator {
//                     position: position_access.get_mut(id as IdType),
//                     velocity: velocity_access.get(id as IdType),
//                 }
//             };
//             PositionIntegrator::operate(item);
//         }
//     }
// }

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

        if let syn::Type::Reference(r) = &field.ty {
            let ty = &r.elem;
            match &r.mutability {
                Some(_) => {
                    let access = quote! {
                        let #ident = space.open::<#ty>();
                        let mut #access_ident = #ident.write();
                        let #users_ident = #ident.get_users();
                    };
                    accesses.push(access);

                    let getter = quote! {
                        #ident: #access_ident.get_mut(id as IdType),
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
        } else {
            panic!("System must only contain reference types");
        }
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

                for id in iter {
                    let item = unsafe {
                        #ident {
                            #(#field_getters)*
                        }
                    };
                    #ident::operate(item);
                }
            }
        }
    };

    runner.into()
}

const MSG_ARG_MISMATCH: &str = "System logic function must take a system data struct as argument";

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
    let arg_cap = match arg {
        syn::FnArg::Captured(a) => a,
        _ => panic!(MSG_ARG_MISMATCH),
    };
    let type_ident = match &arg_cap.ty {
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
            fn operate (#arg_ident: Self) {
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
