fn main() {
    // Out-of-module implementation path (including aliases/function pointers).
    let _implementation =
        verter_compiler::framework_common::registered_carrier_projection::project_registered_carrier;
    // Parent-module path: there is no production re-export outside `cfg(test)`.
    let _facade = verter_compiler::framework_common::project_registered_carrier;
}
