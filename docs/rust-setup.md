# Rust in Verter

This project uses Rust for performance-critical components. All Rust crates are located in the `crates/` directory and managed through a Cargo workspace.

## Project Structure

```
crates/
├── Cargo.toml          # Workspace root
└── <crate-name>/       # Individual crate
    ├── Cargo.toml
    └── src/
        ├── lib.rs      # Library entry point
        └── main.rs     # (optional) Binary entry point
```

## Setup & Development

### Prerequisites

- Rust 1.70+ ([Install Rust](https://rustup.rs/))

### Building

```bash
# Build all crates
cargo build

# Build in release mode
cargo build --release

# Build a specific crate
cargo build -p crate-name
```

### Testing

```bash
# Run all tests
cargo test

# Run tests for a specific crate
cargo test -p crate-name

# Run tests with output
cargo test -- --nocapture
```

### Formatting & Linting

```bash
# Format code
cargo fmt

# Check formatting
cargo fmt --check

# Run clippy linter
cargo clippy

# Run clippy with all warnings
cargo clippy -- -W clippy::all
```

### Documentation

```bash
# Generate and open documentation
cargo doc --open
```

## Creating a New Crate

```bash
cd crates/
cargo new --lib crate-name
# or for a binary crate
cargo new crate-name
```

Then update `Cargo.toml` in the new crate to include workspace settings:

```toml
[package]
name = "crate-name"
version.workspace = true
edition.workspace = true
license.workspace = true

[dependencies]
```

## Workspace Configuration

All crates inherit shared settings from the root `Cargo.toml`:

- `version`: 0.1.0
- `edition`: 2021
- `license`: ISC

Release builds are optimized with LTO and single-threaded compilation for maximum performance.

## Integration with Node.js

For Rust crates that need to be called from Node.js/TypeScript:

- Use [napi-rs](https://napi.rs/) for N-API bindings
- Use [wasm-bindgen](https://github.com/rustwasm/wasm-bindgen) for WebAssembly bindings
- Consider [cargo-build-scripts](https://doc.rust-lang.org/cargo/build-scripts.html) for native module compilation

## CI/CD

Rust code should be included in CI/CD pipelines:

- Format check: `cargo fmt --check`
- Linting: `cargo clippy -- -D warnings`
- Tests: `cargo test`
- Build: `cargo build --release`
