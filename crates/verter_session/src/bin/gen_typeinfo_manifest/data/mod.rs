//! Static ledger data tables the generator derives and emits from. The
//! tables are typed Rust `const`s (they carry Rust enum variant names and
//! emitted Rust fragments) so the compiler checks them.

pub(crate) mod block_maps;
pub(crate) mod lifted_overrides;
pub(crate) mod row_maps;

pub(crate) use block_maps::*;
pub(crate) use lifted_overrides::*;
pub(crate) use row_maps::*;
