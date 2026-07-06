//! Tests for the rendezvous advertisement: round-trip, nonce verification,
//! portable on-disk name sanitization, stable hashing, and session-key
//! discovery.

use super::*;

fn sample(session_key: &str, pid: u32, nonce: &str) -> Advertisement {
    Advertisement {
        advertisement_version: ADVERTISEMENT_VERSION,
        protocol: crate::control::messages::PROTOCOL_VERSION,
        endpoint: if cfg!(windows) {
            format!(r"\\.\pipe\verter-relay-ctl-{pid}")
        } else {
            format!("/tmp/verter-relay-ctl-{pid}.sock")
        },
        nonce: nonce.to_string(),
        pid,
        session_key: session_key.to_string(),
        real_tsgo: "/some/node_modules/@typescript/tsc.exe".to_string(),
        real_tsgo_hash: stable_hash_str("/some/node_modules/@typescript/tsc.exe"),
        wire_pin: 0x1234,
        editor_session_generation: 99,
    }
}

fn scratch_dir(tag: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "verter-adv-test-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn advertisement_round_trips_through_disk() {
    let dir = scratch_dir("roundtrip");
    let adv = sample("proj-key", 4321, "nonce-xyz");
    let written = adv.write(&dir).expect("write advertisement");
    assert!(written.exists(), "the advertisement file must be written");
    let back = Advertisement::read_from_path(&written).expect("read advertisement");
    assert_eq!(back, adv, "the advertisement round-trips through disk");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn verify_nonce_accepts_matching_rejects_mismatch() {
    let adv = sample("k", 1, "the-real-nonce");
    assert!(adv.verify_nonce("the-real-nonce"));
    // Discriminating negative: a stale/spoofed nonce is refused.
    assert!(!adv.verify_nonce("stale-nonce"));
    assert!(!adv.verify_nonce(""));
}

#[test]
fn stable_hash_is_deterministic_and_discriminates() {
    // Deterministic across calls (and, by FNV-1a construction, across processes).
    assert_eq!(
        stable_hash_str("/a/b/tsc.exe"),
        stable_hash_str("/a/b/tsc.exe")
    );
    // Discriminates different inputs.
    assert_ne!(
        stable_hash_str("/a/b/tsc.exe"),
        stable_hash_str("/a/b/other.exe")
    );
}

#[test]
fn sanitize_component_strips_illegal_chars_and_separators() {
    // Path separators and NTFS-illegal characters are all replaced.
    let s = sanitize_component(r#"C:\dev\proj<a>:"|?*.vue"#);
    for bad in ['\\', '/', ':', '<', '>', '"', '|', '?', '*'] {
        assert!(
            !s.contains(bad),
            "sanitized name must not contain {bad:?}: {s:?}"
        );
    }
    assert!(!s.is_empty());
    // A non-empty sanitized name has no trailing dot or space.
    assert!(!s.ends_with('.'));
    assert!(!s.ends_with(' '));
}

#[test]
fn sanitize_component_guards_reserved_basenames_and_empty() {
    // Reserved device names (case-insensitive) are never produced bare.
    for reserved in ["CON", "nul", "Aux", "COM1", "lpt9"] {
        let s = sanitize_component(reserved);
        let stem = s.split('.').next().unwrap().to_ascii_lowercase();
        assert!(
            !["con", "prn", "aux", "nul", "com1", "lpt9"].contains(&stem.as_str()),
            "reserved basename {reserved:?} must be escaped, got {s:?}"
        );
    }
    // A string of only illegal characters never yields an empty component.
    assert!(!sanitize_component("///").is_empty());
    assert!(!sanitize_component("").is_empty());
}

#[test]
fn advertisement_file_name_is_portable() {
    let name = advertisement_file_name(r"C:\weird:name", 7);
    // No NTFS-illegal characters survive into the filename.
    for bad in ['\\', '/', ':', '<', '>', '"', '|', '?', '*'] {
        assert!(
            !name.contains(bad),
            "filename must not contain {bad:?}: {name:?}"
        );
    }
    assert!(name.ends_with(".json"));
    assert!(name.contains("-7."), "the pid keys the filename: {name:?}");
}

#[test]
fn find_for_session_key_returns_newest_matching_advertisement() {
    let dir = scratch_dir("find");
    // Two shims under the SAME session key (different pids): discovery returns
    // the newest, and an unrelated session key is not matched.
    let older = sample("workspace-A", 100, "nonce-old");
    older.write(&dir).unwrap();
    // Ensure a strictly later mtime for the newer advertisement.
    std::thread::sleep(std::time::Duration::from_millis(30));
    let newer = sample("workspace-A", 200, "nonce-new");
    newer.write(&dir).unwrap();
    // An unrelated session key must never be returned for "workspace-A".
    let unrelated = sample("workspace-B", 300, "nonce-b");
    unrelated.write(&dir).unwrap();

    let (_path, found) =
        Advertisement::find_for_session_key(&dir, "workspace-A").expect("discover by session key");
    assert_eq!(found.pid, 200, "the newest matching advertisement wins");
    assert_eq!(found.nonce, "nonce-new");
    assert_eq!(found.session_key, "workspace-A");

    // Discriminating negative: no advertisement for an unknown key.
    let missing = Advertisement::find_for_session_key(&dir, "workspace-Z");
    assert!(
        matches!(missing, Err(AdvertisementError::NotFound(_))),
        "an unknown session key fails closed with NotFound"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The sanitized-filename rendezvous-collision discriminator. Two DISTINCT
/// raw session keys (`a-b` and `a/b`) sanitize to the SAME on-disk name, so the
/// filename prefix is a lossy candidate FILTER, not the identity. Discovery must
/// match on the RAW (unsanitized) `session_key`, so `a-b` never picks up `a/b`'s
/// advertisement (whose nonce would then authenticate the WRONG endpoint).
///
/// RED before the fix: `find_for_session_key` selected the newest prefix-matching
/// file WITHOUT verifying the raw key, so the `a-b` query returned `a/b`'s
/// (newer) advertisement.
#[test]
fn distinct_session_keys_that_sanitize_alike_do_not_cross_match() {
    let dir = scratch_dir("collision");
    // The sanitized on-disk names collide — the filename cannot distinguish them.
    assert_eq!(sanitize_component("a/b"), sanitize_component("a-b"));

    // `a-b` (pid 100) first, then a strictly-newer `a/b` (pid 200) — so a
    // newest-prefix-match selection would wrongly return `a/b` for the `a-b` query.
    sample("a-b", 100, "nonce-dash").write(&dir).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    sample("a/b", 200, "nonce-slash").write(&dir).unwrap();

    // The `a-b` client gets EXACTLY the `a-b` advertisement — never `a/b`'s.
    let (_p, dash) = Advertisement::find_for_session_key(&dir, "a-b")
        .expect("a-b matches its own advertisement");
    assert_eq!(
        dash.session_key, "a-b",
        "the raw session key must match exactly"
    );
    assert_eq!(dash.pid, 100);
    assert_eq!(dash.nonce, "nonce-dash");

    // The `a/b` client gets EXACTLY the `a/b` advertisement.
    let (_p, slash) = Advertisement::find_for_session_key(&dir, "a/b")
        .expect("a/b matches its own advertisement");
    assert_eq!(slash.session_key, "a/b");
    assert_eq!(slash.pid, 200);
    assert_eq!(slash.nonce, "nonce-slash");

    let _ = std::fs::remove_dir_all(&dir);
}

/// An advertisement whose `advertisement_version` or `protocol` does not match
/// the client's is REJECTED at discovery (fail closed), so the nonce never
/// authenticates an incompatible endpoint.
#[test]
fn find_for_session_key_rejects_version_or_protocol_mismatch() {
    let dir = scratch_dir("versionmismatch");
    // A future/incompatible advertisement schema version.
    let mut stale_schema = sample("ws", 100, "nonce-schema");
    stale_schema.advertisement_version = ADVERTISEMENT_VERSION + 1;
    stale_schema.write(&dir).unwrap();
    // A mismatched control protocol version.
    let mut stale_proto = sample("ws", 101, "nonce-proto");
    stale_proto.protocol = crate::control::messages::PROTOCOL_VERSION + 1;
    stale_proto.write(&dir).unwrap();

    let found = Advertisement::find_for_session_key(&dir, "ws");
    assert!(
        matches!(found, Err(AdvertisementError::NotFound(_))),
        "a version/protocol-mismatched advertisement must be rejected (fail closed), got {found:?}"
    );

    // Discriminating positive: a matching advertisement on the same key is found.
    sample("ws", 102, "nonce-ok").write(&dir).unwrap();
    let (_p, ok) = Advertisement::find_for_session_key(&dir, "ws")
        .expect("a version-matched advertisement is found");
    assert_eq!(ok.nonce, "nonce-ok");
    let _ = std::fs::remove_dir_all(&dir);
}
