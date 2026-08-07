//! `DisplaySignatureWireWitness` is obtainable only through a `TypeProvider`
//! impl (`provider_wire_witness`). Its field is non-public, so an out-of-crate
//! struct literal — which would let arbitrary code reach the `pub`
//! `from_provider_wire` constructor — must fail to compile. Kept SEPARATE from
//! the tuple-constructor fixture so neither forge vector's error masks the
//! other.

fn main() {
    let _witness = verter_type_runtime::protocol::DisplaySignatureWireWitness { _private: () };
}
