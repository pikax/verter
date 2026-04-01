pub mod graph;

pub mod types;

pub mod component_meta;
pub mod schema;

pub mod verter {
    pub mod v1 {
        include!(concat!(env!("OUT_DIR"), "/verter.v1.rs"));
    }
}
