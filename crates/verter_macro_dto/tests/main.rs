//! Single consolidated integration-test binary for `verter_macro_dto`.
//!
//! Every case lives as a private submodule under [`cases`]; this binary
//! compiles and runs them all in one process, per the one-binary
//! integration-test layout.

mod cases;
