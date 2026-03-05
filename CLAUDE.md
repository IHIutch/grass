# grass - Sass compiler in Rust

## Build & Test
- `cargo test --features=macro` - run all tests
- `cargo clippy --features=macro -- -D warnings` - lint check
- `cargo build --release` - release build
- Rust MSRV: see rust-version in crates/*/Cargo.toml

## Project Structure
- `crates/compiler/` - core compiler (grass_compiler crate)
- `crates/lib/` - public library + CLI binary (grass crate)
- `crates/include_sass/` - proc macro crate
- `crates/lib/tests/` - integration tests organized by feature
- `sass-spec/` - git submodule of the official Sass spec tests

## Conventions
- Tests use a `test!` macro comparing Sass input to expected CSS output
- `#[ignore = "reason"]` marks known-failing tests with explanation
- Targets feature parity with dart-sass reference implementation
- Error message and span differences from dart-sass are acceptable
