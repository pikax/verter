//! A served function program entry and its keyed-lookup witness cannot be
//! assembled outside their owning crate: discovery data enters only through
//! `FunctionProgramIndex::from_discovery`, and an entry leaves an index only
//! through a keyed lookup.

use verter_session_query::function_program::{
    FunctionProgramEntry, FunctionProgramIndex, FunctionProgramMatch,
};

fn forge(entry: &FunctionProgramEntry) -> FunctionProgramEntry {
    FunctionProgramEntry {
        flow_body_exact_hash: None,
        ..entry.clone()
    }
}

fn mint(index: &FunctionProgramIndex) -> FunctionProgramMatch<'_> {
    FunctionProgramMatch { index, ordinal: 0 }
}

fn main() {
    let _ = (forge, mint);
}
