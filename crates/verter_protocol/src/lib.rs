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
