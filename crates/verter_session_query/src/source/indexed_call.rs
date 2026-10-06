//! A call lowered from an indexed expression together with the read roots of its arguments
//! and receiver.

/// Transient call lowering and the exact source origins used to evaluate its
/// argument/receiver types. Value reads and authored type queries retain
/// distinct roles from the same AST borrow as `call`; the memo retains neither
/// this IR nor another body index.
pub struct IndexedFlowCallExpression {
    pub call: verter_type_expr::IndexedValueCall,
    pub argument_roots: Box<[crate::analysis::indexed_value::IndexedValueReadRoot]>,
    pub receiver_root: Option<crate::analysis::indexed_value::IndexedValueReadRoot>,
}
