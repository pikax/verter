//! Compile-fail: every externally nameable carrier keeps private fields, so
//! scope, binder, environment, operand, and request identity cannot be forged.

use std::sync::Arc;
use verter_session::semantic_query::operand::{
    OperandBinderIdentity, OperandLexicalScope, OperandSplitEnv, SemanticOperand,
    SemanticOperandForceRequest,
};
use verter_session::semantic_query::{ProjectionMode, ProjectionReductionContext};
use verter_type_expr::TopLevelOwnerId;

fn main() {
    let canonical: Arc<str> = Arc::from("/x.ts");
    let _ = OperandLexicalScope {
        canonical_id: canonical,
        owner: TopLevelOwnerId::ordinary_file(),
    };
    let _ = OperandBinderIdentity {
        visibility: todo!(),
    };
    let _ = OperandSplitEnv {
        parse_env_hash: todo!(),
        resolve_env_hash: todo!(),
        type_env_hash: todo!(),
        lib_env_hash: todo!(),
        project_identity: todo!(),
    };
    let _ = SemanticOperand {
        kind: todo!(),
    };
    // Both private fields are named: rustc's cannot-construct error for a
    // partially-provided private-field literal suppresses the per-field
    // E0451 reports of the OTHER literals in this fixture on the pinned
    // toolchain, so this literal provides every field and keeps the
    // per-field privacy evidence for all five carriers.
    let _ = SemanticOperandForceRequest {
        context: ProjectionReductionContext::published(ProjectionMode::Shallow),
        demand: todo!(),
    };
}
