//! Compile-fail fixture: the engine's output authority cannot be duplicated or
//! default-constructed. A borrower can never turn a borrow into an owned
//! authority of its own.

use verter_type_engine::project_semantic_dispatch::engine_resources::OutputAuthority;

fn default_construct() -> OutputAuthority {
    OutputAuthority::default()
}

fn duplicate(authority: &OutputAuthority) -> OutputAuthority {
    authority.clone()
}

fn main() {
    let _ = (default_construct, duplicate);
}
