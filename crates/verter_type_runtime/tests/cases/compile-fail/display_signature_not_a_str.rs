//! The brand implements no `Deref<Target = str>` / `AsRef<str>` /
//! `Into<String>` / `Display`, so it cannot leak implicitly into string-typed
//! (or semantic) APIs — the sole reader is the labelled `as_display_str`.

fn takes_str(_: &str) {}

fn main() {
    let hover = verter_type_runtime::protocol::HoverInfo::default();
    let signature = hover.display_signature.as_ref().unwrap();
    takes_str(signature);
}
