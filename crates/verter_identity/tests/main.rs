//! Single consolidated integration-test binary for `verter_identity`.
//!
//! Every case lives as a private submodule under [`cases`]; this binary
//! compiles and runs them all in one process, per the one-binary
//! integration-test layout (`CLAUDE.md` → Anti-Binary-Growth
//! Integration-Test Layout).

mod cases;
