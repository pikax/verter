//! Alternate public compile facade: `CompileAttempt` cannot be constructed
//! by struct literal. The only entries are `enter_direct`, `enter_project`,
//! and the semantic-only pair.

use verter_compiler::compile_transaction::CompileAttempt;

fn main() {
    let _ = CompileAttempt {
        canonical_id: "",
        framework: "vue",
        is_production: false,
        force_js: false,
        source_digest: [0; 32],
        project: CompileAttempt::UNBOUND_PROJECT,
        vue_macro_semantics_staged: false,
        cancelled: false,
    };
}
