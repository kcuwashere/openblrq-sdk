//! Bindings for the `openblrq:engine` WIT package.
//!
//! Enable the `server` feature in a server component and the `client` feature in a client
//! component. Each module re-exports the generated imports and an export macro:
//!
//! ```ignore
//! use openblrq::server::exports::openblrq::engine::server_events::Guest;
//!
//! struct Game;
//!
//! impl Guest for Game {
//!     // ...
//! }
//!
//! openblrq::export_server!(Game);
//! ```
//!
//! Build with `cargo build --release --target wasm32-wasip2`. The output in
//! `target/wasm32-wasip2/release/` is already a component.

/// Interface version these bindings were generated from.
pub const INTERFACE_VERSION: &str = "0.1.0";

#[cfg(feature = "server")]
pub mod server {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "openblrq:engine/server",
        pub_export_macro: true,
        export_macro_name: "export_server",
        default_bindings_module: "openblrq::server",
    });
}

#[cfg(feature = "server")]
pub use server::export_server;

#[cfg(feature = "client")]
pub mod client {
    wit_bindgen::generate!({
        path: "../../wit",
        world: "openblrq:engine/client",
        pub_export_macro: true,
        export_macro_name: "export_client",
        default_bindings_module: "openblrq::client",
    });
}

#[cfg(feature = "client")]
pub use client::export_client;
