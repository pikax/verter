//! Manifest contract tests for the v5/process parity port.
//!
//! This module does not assert codegen behavior directly; it enforces the
//! parity manifest contract that maps every upstream v5/process case to a
//! Rust-side test target.

use std::collections::HashSet;

const MANIFEST: &str = include_str!("../../tests/parity/v5_process_manifest.toml");

#[derive(Debug, Default, Clone)]
struct ManifestEntry {
    id: String,
    kind: String,
    status: String,
    rust_test: String,
}

fn parse_manifest_entries(input: &str) -> Vec<ManifestEntry> {
    let mut entries = Vec::new();
    let mut current: Option<ManifestEntry> = None;

    for raw_line in input.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line == "[[entry]]" {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            current = Some(ManifestEntry::default());
            continue;
        }

        let Some(eq) = line.find('=') else {
            continue;
        };
        let key = line[..eq].trim();
        let raw_value = line[eq + 1..].trim();
        let value = raw_value
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .unwrap_or(raw_value)
            .replace("\\\"", "\"");

        let Some(entry) = current.as_mut() else {
            continue;
        };

        match key {
            "id" => entry.id = value,
            "kind" => entry.kind = value,
            "status" => entry.status = value,
            "rust_test" => entry.rust_test = value,
            _ => {}
        }
    }

    if let Some(entry) = current.take() {
        entries.push(entry);
    }

    entries
}

#[test]
fn v5_process_spec_manifest_contract() {
    let entries = parse_manifest_entries(MANIFEST);
    let spec_entries: Vec<_> = entries.iter().filter(|e| e.kind == "spec_case").collect();

    assert!(
        !spec_entries.is_empty(),
        "expected at least one spec_case entry in v5 parity manifest"
    );

    let mut ids = HashSet::new();
    for entry in spec_entries {
        assert!(!entry.id.is_empty(), "spec_case entry must have id");
        assert!(
            ids.insert(entry.id.clone()),
            "duplicate spec_case id in manifest: {}",
            entry.id
        );
        assert_eq!(
            entry.status, "ported",
            "spec_case entry must be marked as ported: {}",
            entry.id
        );
        assert!(
            !entry.rust_test.is_empty(),
            "spec_case entry must point to a rust_test: {}",
            entry.id
        );
    }
}

#[test]
fn v5_process_fixture_manifest_contract() {
    let entries = parse_manifest_entries(MANIFEST);
    let fixture_entries: Vec<_> = entries
        .iter()
        .filter(|e| e.kind == "fixture_case")
        .collect();

    assert!(
        !fixture_entries.is_empty(),
        "expected at least one fixture_case entry in v5 parity manifest"
    );

    let mut ids = HashSet::new();
    for entry in fixture_entries {
        assert!(!entry.id.is_empty(), "fixture_case entry must have id");
        assert!(
            ids.insert(entry.id.clone()),
            "duplicate fixture_case id in manifest: {}",
            entry.id
        );
        assert_eq!(
            entry.status, "ported",
            "fixture_case entry must be marked as ported: {}",
            entry.id
        );
        assert!(
            !entry.rust_test.is_empty(),
            "fixture_case entry must point to a rust_test: {}",
            entry.id
        );
    }
}
