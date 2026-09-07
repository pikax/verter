//! Compile-fail: production mints and the forcing authority exist, but remain
//! crate-private. External consumers can carry an operand but cannot forge or
//! evaluate one.

use verter_session::semantic_query::operand::{
    OperandLexicalScope, OperandSplitEnv, SemanticOperand, SemanticOperandForceRequest,
};

fn main() {
    let _ = OperandLexicalScope::for_locator(todo!());
    let _ = OperandSplitEnv::new(todo!(), todo!(), todo!(), todo!(), todo!(), todo!());
    let _ = SemanticOperand::node(0, 0, todo!(), todo!(), todo!());
    let _ = SemanticOperandForceRequest::new(todo!());
    let _ = SemanticOperandForceRequest::projecting(todo!(), todo!());
    let _ = SemanticOperandForceRequest::key_domain(todo!());

    use verter_session::project_semantic_dispatch::semantic_operand as _;

    let operand: SemanticOperand = todo!();
    let _ = operand;

    // The actual forcing/minting authority — `ProjectSemanticDispatch`'s own
    // `force_semantic_operand` / `mint_authored_semantic_operand` /
    // `mint_node_semantic_operand` — never `SemanticOperand` itself. Naming
    // the type at all is what must fail: the owning module is
    // `pub(crate)`, so no external crate can reach the authority methods
    // regardless of argument shape.
    let dispatch: verter_session::project_semantic_dispatch::ProjectSemanticDispatch = todo!();
    let _ = dispatch.force_semantic_operand(todo!(), todo!());
    let _ = dispatch.mint_authored_semantic_operand(todo!(), todo!());
    let _ = dispatch.mint_node_semantic_operand(todo!());
}
