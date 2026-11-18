#![allow(dead_code, unused_variables)]
//! Target-neutral side tables over the template topology: per-node facts,
//! flat attribute and directive tables, interned names, and resolved child
//! lists. Nothing here is specific to one target.

mod tables;
mod walk;

pub use tables::{
    build_compile_structure, dump_structure, verify_structure, VueAttr, VueAttrKey, VueAttrKind,
    VueBuiltinDirective, VueChainRole, VueChildKey, VueChildKind, VueChildRecord, VueCompileNodeId,
    VueCompileNodeKey, VueCompileStructure, VueNodeFacts, VueNodeFlags, VueStructureRefusal,
};
pub use walk::{walk_template, VueTemplateVisitor, VueWalkControl};

#[cfg(test)]
mod tests;
