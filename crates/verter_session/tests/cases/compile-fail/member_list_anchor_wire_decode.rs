//! Deserialization half of the member-list anchor seal: decoding serialized
//! analysis must not produce the edit capability the language server's macro
//! edit paths accept. The anchor implements no `Deserialize`; serialized
//! anchors decode to plain `MemberListAnchorData` instead, so a payload such
//! as `{"insertOffset":4,"isEmpty":false}` cannot become an editable position.

fn main() {
    let _forged: verter_session_query::analysis::types::MemberListAnchor =
        serde_json::from_str(r#"{"insertOffset":4,"isEmpty":false}"#).unwrap();
}
