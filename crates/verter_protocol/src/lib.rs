pub mod graph;

pub mod types;

pub mod component_meta;
pub mod schema;
pub mod typeinfo;

pub mod verter {
    pub mod v1 {
        // The generated code includes large oneof variants (the
        // `SemanticTypeGraph` envelope carries the entire typeinfo
        // graph payload as one variant of `TypeInfoGraphResponse`).
        // Boxing the variant would require boxing every read site on
        // the consumer side, and the wire-form types are not on a hot
        // allocation path. Suppress the clippy lint at the generated
        // boundary so the wire surface stays a literal mirror of the
        // proto schema.
        #![allow(clippy::large_enum_variant)]
        include!(concat!(env!("OUT_DIR"), "/verter.v1.rs"));
    }
}

/// The generated `verter.v1` prost Rust source that THIS build compiled and
/// linked, exposed verbatim for test-support.
///
/// `include_str!` reads the exact same `concat!(env!("OUT_DIR"),
/// "/verter.v1.rs")` path that the `verter::v1` `include!` above compiles, so
/// this string is the prost output of the current build *by construction* —
/// not a sibling artifact discovered by scanning `target/` (whose `read_dir`
/// can surface a stale `verter_protocol-<hash>/out/` dir from another
/// fingerprint, worktree, or branch sharing the same `CARGO_TARGET_DIR`).
/// Taxonomy guards assert the documented oneof carrier modules are present in
/// this source. The `const` carries no codegen footprint where unreferenced,
/// so production binaries that never touch it are unaffected.
pub const GENERATED_VERTER_V1_RS: &str = include_str!(concat!(env!("OUT_DIR"), "/verter.v1.rs"));
