use super::*;

#[test]
fn hex_tag_round_trips_configured() {
    let key = ProjectStableKey::Configured(compute_hash16(b"configured-fixture"));
    let tag = key.to_hex_tag();
    assert!(tag.starts_with('C'));
    assert_eq!(ProjectStableKey::parse_hex_tag(&tag), Some(key));
}

#[test]
fn hex_tag_round_trips_fallback() {
    let key = ProjectStableKey::Fallback(compute_hash16(b"fallback-fixture"));
    let tag = key.to_hex_tag();
    assert!(tag.starts_with('F'));
    assert_eq!(ProjectStableKey::parse_hex_tag(&tag), Some(key));
}

#[test]
fn parse_hex_tag_rejects_unknown_prefix() {
    let key = ProjectStableKey::Configured(compute_hash16(b"x"));
    let tag = key.to_hex_tag();
    let bad = format!("Z{}", &tag[1..]);
    assert_eq!(ProjectStableKey::parse_hex_tag(&bad), None);
}

#[test]
fn parse_hex_tag_rejects_short_input() {
    assert_eq!(ProjectStableKey::parse_hex_tag(""), None);
    assert_eq!(ProjectStableKey::parse_hex_tag("C"), None);
}

#[test]
fn parse_hex_tag_rejects_wrong_length_hex() {
    assert_eq!(ProjectStableKey::parse_hex_tag("Cabc"), None);
}

#[test]
fn configured_and_fallback_with_same_hash_are_distinct() {
    let hash = compute_hash16(b"shared-input");
    assert_ne!(
        ProjectStableKey::Configured(hash),
        ProjectStableKey::Fallback(hash)
    );
}

#[test]
fn compute_hash16_is_deterministic_and_input_sensitive() {
    assert_eq!(compute_hash16(b"same"), compute_hash16(b"same"));
    assert_ne!(compute_hash16(b"a"), compute_hash16(b"b"));
}
