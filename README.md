# meio

[![Crates.io][crates-badge]][crates-url]
[![Released API docs][docs-badge]][docs-url]

[crates-badge]: https://img.shields.io/crates/v/meio.svg
[crates-url]: https://crates.io/crates/meio
[docs-badge]: https://docs.rs/meio/badge.svg
[docs-url]: https://docs.rs/meio

Async actor framework for Rust. The benefits:

- Safe and reliable
- Has lifecycle events: `StartedBy`, `InterruptedBy`, etc.
- Oneshot actions
- Interacions with results
- Instant actions that deliver in high-priority
- Streams can be attached to actors as actions
- Support lite tasks with results
- Sequence of termination for sub-actors and tasks
- Scheluled actions
- Signals (can listen for `CtrlC`)
- `tokio` compatible
- `WASM` compatible (in progress)

This framework strongly inspired by `Erlang OTP` and `actix`, but my goal was
to make it more convenient and tight integrated with `async` capabilities of Rust.

This crate used by `rillrate` products.
