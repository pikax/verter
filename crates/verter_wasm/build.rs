//! Link settings for the WebAssembly module.
//!
//! wasm32's linker reserves 1 MiB of linear memory for the module's own
//! stack unless told otherwise, and the host runs every stage inline on
//! that stack: the parse, the analysis and the flow evaluation of a deeply
//! nested source take more (2.6 MB optimized for 256 nested arrow
//! functions). The module reserves 8 MiB, the stack of the native host's
//! workers.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        println!("cargo:rustc-link-arg-cdylib=-zstack-size=8388608");
    }
}
