//! `DisplaySignature` is minted ONLY through `DisplaySignature::from_provider_wire`
//! with a provider-obtained witness. The inner field is non-public, so an
//! out-of-crate tuple-constructor forge — the vector a public field would
//! open — must fail to compile.

fn main() {
    let _forged = verter_type_runtime::protocol::DisplaySignature("forged".to_string());
}
