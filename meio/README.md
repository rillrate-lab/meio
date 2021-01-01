# meio

Lightweight async actor framework for Rust.

## WASM

It has experimental WASM support. To activate use:

meio = { default-features = false, feautres = ["wasm"] }

## Usage

Check tests in `lib.rs` file to see how it works.

## Guideline

You should prefer to have `Supervisor` actor that catches termination signals and
creates other actors, because it the supoervisor will call caonstructors of others
it can provider all necesary information and actors will have addresses of other parts
in constructors and you don't have to wrap that fields with `Option`.
