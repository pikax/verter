//! Compile-fail fixture: the engine's output authority cannot be forged. Its
//! field is private and it has no public constructor, so the only authority
//! that exists is the one minted with an engine's stores — not even another
//! authority's engine identity can be re-wrapped into a new one.

use verter_session::for_tests::OutputAuthority;

fn forge(other: &OutputAuthority) -> OutputAuthority {
    OutputAuthority {
        engine: other.engine.clone(),
    }
}

fn main() {
    let _ = forge;
}
