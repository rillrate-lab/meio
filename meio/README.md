# meio

Lightweight async actor framework for Rust.

[![Crates.io][crates-badge]][crates-url]
[![Released API docs][docs-badge]][docs-url]

[crates-badge]: https://img.shields.io/crates/v/meio.svg
[crates-url]: https://crates.io/crates/meio
[docs-badge]: https://docs.rs/meio/badge.svg
[docs-url]: https://docs.rs/meio

## WASM

It has experimental WASM support. To activate use:

```toml
meio = { default-features = false, feautres = ["wasm"] }
```

## Usage

Check tests in `lib.rs` file to see how it works.

## Guideline

You should prefer to have `Supervisor` actor that catches termination signals and
creates other actors, because it the supoervisor will call caonstructors of others
it can provider all necesary information and actors will have addresses of other parts
in constructors and you don't have to wrap that fields with `Option`.
