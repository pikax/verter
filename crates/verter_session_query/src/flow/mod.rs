//! The parser-free flow substrate: the function-body skeleton, the flow graph built from
//! it, the demand planner, the slice hash and the slice IR.

pub mod binding;
pub mod flow_graph;
pub mod flow_ir;
pub mod frame_span;
pub mod hashing;
pub mod lower;
pub mod peeker;
pub mod skeleton;
