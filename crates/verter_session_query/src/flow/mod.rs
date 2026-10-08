//! The parser-free flow substrate: the function-body skeleton, the flow graph built from
//! it, the demand planner, the slice hash and the slice IR.

pub mod binding;
pub mod bundle;
pub mod completion;
pub mod flow_graph;
pub mod flow_ir;
pub mod frame_span;
pub mod hashing;
pub mod lower;
pub mod peeker;
pub mod policy;
pub mod skeleton;
pub mod slice;
pub mod span_index;
