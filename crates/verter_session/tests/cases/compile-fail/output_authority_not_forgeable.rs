//! Compile-fail fixture: the engine's output authority cannot be forged. Its
//! field is private and it has no public constructor, so the only authority
//! that exists is the one minted with an engine's stores.

use verter_session::for_tests::OutputAuthority;

fn forge() -> OutputAuthority {
    OutputAuthority {
        engine: std::sync::Weak::new(),
    }
}

fn main() {
    let _ = forge;
}
