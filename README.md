# OpenBLRQ SDK

Interfaces and tools for building games ("experiences") on OpenBLRQ.

An experience is a WebAssembly component that runs on our servers, plus optional components that run
in the player's client. They talk to the engine only through the interfaces in [`wit/`](wit), so this
repository is the full contract between your code and the platform. The engine itself is not open
source.

The platform isn't open to creators yet. You can build and check experiences locally with what's here,
but you can't publish them. Creator documentation and policies are in
[openblrq-docs](https://github.com/kcuwashere/openblrq-docs).

## Contents

| Path | What it is |
|---|---|
| `wit/` | The `openblrq:engine@0.1.0` WIT package, with a `server` world and a `client` world |
| `crates/openblrq` | Rust bindings for both worlds (`server` and `client` features) |
| `crates/openblrq-manifest` | Parser and checks for `openblrq.toml` |
| `cli/` | `obq`, the command-line tool |
| `examples/hello-server` | A minimal server component |

## Building the example

You need Rust 1.99 or newer and the `wasm32-wasip2` target. The toolchain file in this repo installs
it for you if you use rustup.

```sh
cd examples/hello-server
cargo build --release --target wasm32-wasip2
cargo run -p obq --manifest-path ../../Cargo.toml -- check .
```

The output, `target/wasm32-wasip2/release/hello_server.wasm`, is already a component. To see what it
imports:

```sh
wasm-tools component wit target/wasm32-wasip2/release/hello_server.wasm
```

## What your code can do

A server component can create and change entities, read and kick players, send messages to its own
client components, set timers, and, if the experience asks for them, use physics and a per-experience
key-value store. A client component can read the replicated scene, create local entities from the
experience's own assets, receive input through the actions declared in `openblrq.toml`, control the
camera, describe UI, play sounds, and message its server.

Neither side gets files, outbound network access, the clipboard, the player's account details, or
anything belonging to another experience. Rust's standard library compiles to calls that need some of
those (reading a file, for example); they exist but always fail. The limits on memory, CPU time and
message rates are listed in the creator docs.

## Versioning

The WIT package is versioned on its own. A published version doesn't change. The platform supports the
current and the previous minor version, and announces at least 90 days before it stops supporting one.

## Licence

MIT or Apache 2.0, at your option. See `LICENSE-MIT` and `LICENSE-APACHE`. This covers the SDK only.
Using the platform is covered by the Terms of Service and the Creator Agreement in openblrq-docs.

## Security

Please don't open public issues for security problems. See
[SECURITY.md](https://github.com/kcuwashere/openblrq-docs/blob/main/security/SECURITY.md) in
openblrq-docs.
