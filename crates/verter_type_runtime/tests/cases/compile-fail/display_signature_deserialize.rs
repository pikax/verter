//! The brand derives `Serialize` but deliberately NOT `Deserialize`: a display
//! signature cannot be conjured off a wire — it exists only where a provider
//! normalized its own engine's response.

fn main() {
    let _: verter_type_runtime::protocol::DisplaySignature =
        serde_json::from_str("\"forged\"").unwrap();
}
