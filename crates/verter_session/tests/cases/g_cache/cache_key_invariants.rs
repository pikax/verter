//! R5 / R6 — `FileArtifactKey` carries every dimension that meaningfully
//! changes the cached value.
//!
//! These tests assert the cache-key composition invariants:
//!
//! - **R5**: content-addressed caches carry `content_hash` AND
//!   `parse_env_hash` (and `parser_version` for `FileArtifactKey`).
//! - **R6**: cache keys NEVER include `fact_dep_signature` or other
//!   version-tracking payload — those live on the cached value.
//!   `FileArtifactKey` carries no fact-dep-signature, so this test
//!   characterises the LEGAL key shape: only the four documented
//!   dimensions are part of the key's identity.
//!
//! The invariant is: reordering or omitting any of the four key
//! dimensions changes hash equality. Omitting a dimension is structurally
//! impossible because Rust enforces the field set; but a different value
//! for any of the four MUST yield a non-equal key.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use verter_session::file_artifact_store::FileArtifactKey;

fn make_key(
    canonical: &str,
    content_hash: [u8; 16],
    parse_env_hash: [u8; 16],
    parser_version: u32,
) -> FileArtifactKey {
    FileArtifactKey {
        canonical: Arc::from(canonical),
        content_hash,
        parse_env_hash,
        parser_version,
        file_language_id: FileArtifactKey::derived_file_language_id(canonical),
    }
}

fn hash_of(k: &FileArtifactKey) -> u64 {
    let mut h = DefaultHasher::new();
    k.hash(&mut h);
    h.finish()
}

#[test]
fn canonical_change_breaks_key_equality() {
    let a = make_key("/a.ts", [1u8; 16], [2u8; 16], 1);
    let b = make_key("/b.ts", [1u8; 16], [2u8; 16], 1);
    assert_ne!(a, b);
    assert_ne!(hash_of(&a), hash_of(&b));
}

#[test]
fn content_hash_change_breaks_key_equality() {
    // R5: content_hash MUST be part of the key.
    let a = make_key("/a.ts", [1u8; 16], [2u8; 16], 1);
    let b = make_key("/a.ts", [99u8; 16], [2u8; 16], 1);
    assert_ne!(a, b, "R5: content_hash MUST be in the key");
    assert_ne!(hash_of(&a), hash_of(&b));
}

#[test]
fn parse_env_hash_change_breaks_key_equality() {
    // R5: parse_env_hash MUST be part of the key.
    let a = make_key("/a.ts", [1u8; 16], [2u8; 16], 1);
    let b = make_key("/a.ts", [1u8; 16], [99u8; 16], 1);
    assert_ne!(a, b, "R5: parse_env_hash MUST be in the key");
    assert_ne!(hash_of(&a), hash_of(&b));
}

#[test]
fn parser_version_change_breaks_key_equality() {
    let a = make_key("/a.ts", [1u8; 16], [2u8; 16], 1);
    let b = make_key("/a.ts", [1u8; 16], [2u8; 16], 2);
    assert_ne!(a, b);
    assert_ne!(hash_of(&a), hash_of(&b));
}

#[test]
fn identical_inputs_produce_equal_keys() {
    let a = make_key("/a.ts", [1u8; 16], [2u8; 16], 1);
    let b = make_key("/a.ts", [1u8; 16], [2u8; 16], 1);
    assert_eq!(a, b);
    assert_eq!(hash_of(&a), hash_of(&b));
}

#[test]
fn r6_key_struct_has_no_fact_dep_signature_field() {
    // R6 codification by structural assertion: the FileArtifactKey type
    // has exactly five fields (canonical, content_hash, parse_env_hash,
    // parser_version, file_language_id). If `fact_dep_signature` or
    // similar is added to the key, this test fails at compile time (the
    // destructuring pattern below would not match).
    let key = make_key("/a.ts", [1u8; 16], [2u8; 16], 1);
    let FileArtifactKey {
        canonical,
        content_hash,
        parse_env_hash,
        parser_version,
        file_language_id,
    } = key;
    assert_eq!(&*canonical, "/a.ts");
    assert_eq!(content_hash, [1u8; 16]);
    assert_eq!(parse_env_hash, [2u8; 16]);
    assert_eq!(parser_version, 1);
    assert_eq!(
        file_language_id,
        verter_language::FileLanguage::script(verter_language::ScriptSourceType::Ts)
    );
}
