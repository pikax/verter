//! `CarrierAccessToken` is minted ONLY inside `verter_language` during
//! `LanguageRegistry` carrier-row construction. The `_private`
//! field is non-public, so an out-of-crate struct literal — the forging
//! vector a public constructor would open — must fail to compile.

fn main() {
    let _token = verter_language::CarrierAccessToken {
        adapter_id: verter_language::FrameworkAdapterId::new("forged"),
        _private: (),
    };
}
