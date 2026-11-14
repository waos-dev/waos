//! `#[wasdk::main]` and `#[wasdk::worker]`: a program's entry and its workers' entries, which
//! generation cannot produce.
//!
//! `#[wasdk::main]` keeps the function as the program and adds the entries beside it: the export
//! of `wasi:cli/run@0.3.0`, the async entry the engine calls, and of `wasdk:platform/worker-entry`,
//! which WaOS calls in each worker and which runs the `#[wasdk::worker]` of the entry asked, an
//! entry the program does not have answered as unknown. So a program without workers writes
//! nothing for them. A `fn main` is still emitted, because the `wasm32-wasip2` target links a
//! Preview 2 command entry into every `bin` whether or not one is written, and it only says where
//! the real entry is (`../experiments/2026-09-20-dual-target-entry`).
//!
//! `#[wasdk::worker]` keeps the function as it is and registers it under its name, which a
//! constant beside it holds (`COMPILE_ENTRY` for `compile`), so the name a program starts a worker
//! at is checked when it builds. Each worker of a program registers itself, in the crate that
//! defines it, and the one export finds it there (`wasdk`'s `__entry`).
//!
//! Neither attribute takes options: what a window is called and how big it is are the program's
//! to ask through `views.request-view`, and a view the shell gives is the shell's to place.
//!
//! The invariant: this crate holds only the macros. `wasdk` re-exports them, so a program reaches
//! them as `wasdk::main` and `wasdk::worker`.

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{Ident, ItemFn, ReturnType, Visibility};

/// Marks the program's `async fn main`.
///
/// ```ignore
/// #[wasdk::main]
/// async fn main() {
///     for view in wasdk::views::views() {
///         // …
///     }
/// }
/// ```
///
/// The function may return `()` or a `Result<(), E>` where `E: Display`; an error is
/// printed and reported to the engine as the program's failure.
#[proc_macro_attribute]
pub fn main(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(Span::call_site(), "`#[wasdk::main]` takes no options")
            .to_compile_error()
            .into();
    }
    let mut program = syn::parse_macro_input!(item as ItemFn);
    if program.sig.asyncness.is_none() {
        return syn::Error::new_spanned(
            program.sig.fn_token,
            "`#[wasdk::main]` takes an `async fn`",
        )
        .to_compile_error()
        .into();
    }
    if !program.sig.inputs.is_empty() {
        return syn::Error::new_spanned(
            &program.sig.inputs,
            "the program's `main` takes no arguments",
        )
        .to_compile_error()
        .into();
    }
    if matches!(program.sig.output, ReturnType::Default) {
        program.sig.output = syn::parse_quote!(-> ());
    }
    let program_function = Ident::new("__wasdk_program", Span::call_site());
    program.sig.ident = program_function.clone();
    program.vis = Visibility::Inherited;

    quote! {
        #program

        struct __WasdkProgram;

        ::wasdk::__entry::export!(__WasdkProgram with_types_in ::wasdk::bindings);

        impl ::wasdk::__entry::Guest for __WasdkProgram {
            async fn run() -> ::core::result::Result<(), ()> {
                ::wasdk::__entry::Outcome::into_result(
                    #program_function().await,
                    ::core::env!("CARGO_PKG_NAME"),
                )
            }
        }

        impl ::wasdk::__entry::WorkerGuest for __WasdkProgram {
            async fn run(
                entry: ::std::string::String,
                inbox: ::wasdk::StreamReader<::std::vec::Vec<u8>>,
            ) -> ::core::result::Result<
                ::wasdk::StreamReader<::std::vec::Vec<u8>>,
                ::wasdk::workers::WorkerError,
            > {
                ::wasdk::__entry::run_worker(entry, inbox)
            }
        }

        // The Preview 2 command entry the target links in regardless; an engine that calls it
        // is not wasdk's.
        fn main() {
            ::wasdk::__entry::not_the_entry();
        }
    }
    .into()
}

/// Marks an `async fn` a worker runs, started at its name.
///
/// ```ignore
/// #[wasdk::worker]
/// async fn compile(mut inbox: StreamReader<Vec<u8>>, mut outbox: StreamWriter<Vec<u8>>) {
///     while let Some(source) = inbox.next().await {
///         outbox.write_one(render_pages(&source)).await;
///     }
/// }
///
/// // in the main instance:
/// let worker = workers.start(COMPILE_ENTRY.to_owned(), inbox).await?;
/// ```
///
/// The function takes the worker's inbox, what the program sends it, and its outbox, what it
/// sends back, and returns nothing; the worker ends once it returns and everything it spawned
/// has finished. Beside it, `<NAME>_ENTRY` is its name, with the function's visibility.
#[proc_macro_attribute]
pub fn worker(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(Span::call_site(), "`#[wasdk::worker]` takes no options")
            .to_compile_error()
            .into();
    }
    let worker = syn::parse_macro_input!(item as ItemFn);
    if let Err(error) = check_worker(&worker) {
        return error.to_compile_error().into();
    }
    let function = &worker.sig.ident;
    let entry = function.to_string();
    let entry_constant = Ident::new(&format!("{}_ENTRY", entry.to_uppercase()), function.span());
    let visibility = &worker.vis;
    let doc = format!("The entry the worker `{entry}` is started at, its name.");

    quote! {
        #worker

        #[doc = #doc]
        #[allow(dead_code)]
        #visibility const #entry_constant: &str = #entry;

        ::wasdk::__entry::submit! {
            ::wasdk::__entry::WorkerEntry {
                name: #entry,
                function: |inbox, outbox| ::std::boxed::Box::pin(#function(inbox, outbox)),
            }
        }
    }
    .into()
}

/// A worker's function is an `async fn` of an inbox and an outbox, returning nothing.
fn check_worker(worker: &ItemFn) -> syn::Result<()> {
    if worker.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            worker.sig.fn_token,
            "`#[wasdk::worker]` takes an `async fn`",
        ));
    }
    if worker.sig.inputs.len() != 2 {
        return Err(syn::Error::new_spanned(
            &worker.sig.inputs,
            "a worker takes its inbox, a `StreamReader<Vec<u8>>`, and its outbox, a `StreamWriter<Vec<u8>>`",
        ));
    }
    if !matches!(worker.sig.output, ReturnType::Default) {
        return Err(syn::Error::new_spanned(
            &worker.sig.output,
            "a worker returns nothing: it says what it has to say on its outbox",
        ));
    }
    Ok(())
}
