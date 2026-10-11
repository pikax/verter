use verter_type_engine::request_context::ExecutionValidityScope;

fn main() {
    // A fabricated guard could pop an enclosing execution's abort state.
    let _forged = ExecutionValidityScope { _private: Default::default() };
}
