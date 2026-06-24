//! Architecture guard (the single-store invariant): the `ProviderSurfaceStore` is the SOLE
//! content-addressed record of provider content / source-maps / ownership in the
//! LSP layer. No SECOND content-addressed surface store may exist.
//!
//! The extended-store work added to `ProviderSurfaceStore` with the project-owner column,
//! `map_hash`, `import_signature_hash`/recheck state, and wired the
//! previously-reserved roles — so the temptation to spin up a parallel "carrier
//! cache" / "tsx cache" / second surface store is real. This guard makes a second
//! store mechanically visible: it source-scans `crates/verter_lsp/src/**` and
//! FAILS if any production struct OTHER THAN the single allow-listed
//! `ProviderSurfaceStore` (and its private `StoreInner`) is a content-addressed
//! surface store — identified structurally as a type whose name reads as a
//! surface/carrier/tsx content store, OR a struct that holds a `DashMap`/`HashMap`
//! keyed by a content hash storing provider/carrier snapshot content.
//!
//! Discriminates: introducing `struct CarrierTsxCache { entries: DashMap<ContentHash, ...> }`
//! (a second content-addressed surface store) FAILS this guard; folding that data
//! into `ProviderSurfaceStore` PASSES. It does NOT flag the new split-cache value
//! types (`RegenKey`/`EngineRecheckState`) — those are key/value structs carried
//! BY the single store, not a second store (they hold no content-addressed map).

use std::path::{Path, PathBuf};

/// The ONE allowed content-addressed surface-store type (plus its private inner).
const ALLOWED_SURFACE_STORE_TYPES: &[&str] = &["ProviderSurfaceStore", "StoreInner"];

/// Name fragments that mark a type as a (forbidden, if not allow-listed)
/// content-addressed surface/carrier store.
const SURFACE_STORE_NAME_MARKERS: &[&str] = &[
    "SurfaceStore",
    "CarrierStore",
    "CarrierCache",
    "TsxCache",
    "TsxStore",
    "ProviderContentStore",
    "ProviderCache",
];

fn src_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Recursively collect every `.rs` file under `dir`.
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Strip the trailing inline `#[cfg(test)]` module so the scan sees production
/// source only (test fixtures legitimately build ad-hoc maps).
fn production_only(src: &str) -> String {
    match src.find("\n#[cfg(test)]") {
        Some(idx) => src[..idx].to_string(),
        None => src.to_string(),
    }
}

/// Extract the type name from a `struct <Name>` / `pub struct <Name>` /
/// `pub(crate) struct <Name>` line, if present.
fn extract_struct_name(line: &str) -> Option<String> {
    let t = line.trim_start();
    let after = t.strip_prefix("pub ").unwrap_or(t);
    // Skip a `pub(...)` visibility qualifier.
    let after = match after.find("struct ") {
        Some(idx) => &after[idx..],
        None => return None,
    };
    let rest = after.strip_prefix("struct ")?;
    let name: String = rest
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

#[test]
fn no_second_content_addressed_surface_store_by_name() {
    let mut files = Vec::new();
    collect_rs_files(&src_root(), &mut files);
    assert!(!files.is_empty(), "expected to scan verter_lsp/src");

    let mut offenders: Vec<String> = Vec::new();
    for file in &files {
        let raw = std::fs::read_to_string(file).expect("read src file");
        let src = production_only(&raw);
        for line in src.lines() {
            let Some(name) = extract_struct_name(line) else {
                continue;
            };
            if ALLOWED_SURFACE_STORE_TYPES.contains(&name.as_str()) {
                continue;
            }
            if SURFACE_STORE_NAME_MARKERS.iter().any(|m| name.contains(m)) {
                offenders.push(format!(
                    "{}: `struct {name}` reads as a content-addressed surface store",
                    file.display()
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "a SECOND content-addressed surface store is forbidden (the single-store invariant — \
         ProviderSurfaceStore is the sole record of provider content/maps/ownership). \
         Fold the data into ProviderSurfaceStore instead. Offenders:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn provider_surface_store_is_defined_exactly_once() {
    let mut files = Vec::new();
    collect_rs_files(&src_root(), &mut files);

    let mut definitions: Vec<String> = Vec::new();
    for file in &files {
        let raw = std::fs::read_to_string(file).expect("read src file");
        let src = production_only(&raw);
        for line in src.lines() {
            if extract_struct_name(line).as_deref() == Some("ProviderSurfaceStore") {
                definitions.push(file.display().to_string());
            }
        }
    }

    assert_eq!(
        definitions.len(),
        1,
        "ProviderSurfaceStore must be defined EXACTLY once (the single store). \
         Found in: {definitions:?}"
    );
    assert!(
        definitions[0].ends_with("provider_surface_store.rs"),
        "ProviderSurfaceStore must live in provider_surface_store.rs, found in {}",
        definitions[0]
    );
}

/// The split-cache value types (`RegenKey` / `EngineRecheckState`) are NOT a
/// second store — they are key/value structs CARRIED BY the single store. This
/// asserts they carry no content-addressed map themselves (no `DashMap` /
/// `HashMap` field), so they can never become a parallel content store.
#[test]
fn split_cache_value_types_hold_no_content_addressed_map() {
    let carrier_cache = std::fs::read_to_string(src_root().join("carrier_cache.rs"))
        .expect("read carrier_cache.rs");
    let production = production_only(&carrier_cache);
    assert!(
        !production.contains("DashMap") && !production.contains("HashMap"),
        "carrier_cache.rs must define only pure key/value/predicate types (the split \
         cache logic carried BY ProviderSurfaceStore) — a map field here would be a \
         second content store"
    );
}

/// Markers that a map's KEY type is a content hash (the defining trait of a
/// content-addressed store).
const CONTENT_HASH_KEY_MARKERS: &[&str] = &["ContentHash", "Hash16", "ContentHash)", "blake3"];

/// Markers that a map's VALUE type stores provider/carrier surface content.
const SURFACE_VALUE_MARKERS: &[&str] = &[
    "Snapshot",
    "ProviderSurface",
    "Carrier",
    "Tsx",
    "provider_content",
];

/// Split a production source into `(struct_name, struct_body)` pairs for every
/// `struct <Name> { ... }` declaration, by brace-depth tracking. Tuple structs
/// and unit structs (no `{`) yield an empty body. Good enough to find a
/// content-addressed map FIELD inside a struct body.
fn struct_bodies(src: &str) -> Vec<(String, String)> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = src[search_from..].find("struct ") {
        let decl_start = search_from + rel;
        // Confirm this is a real struct declaration (line-leading `struct`/`pub
        // struct`/`pub(..) struct`), not a substring.
        let line_start = src[..decl_start].rfind('\n').map_or(0, |i| i + 1);
        let prefix = src[line_start..decl_start].trim();
        let is_decl = prefix.is_empty()
            || prefix == "pub"
            || prefix.ends_with(')') // pub(crate)/pub(super)
            || prefix == "pub ";
        // Extract the name following `struct `.
        let after = &src[decl_start + "struct ".len()..];
        let name: String = after
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        // Find the body, if any, before the next `;` or `{`.
        let rest = &src[decl_start..];
        let brace = rest.find('{');
        let semi = rest.find(';');
        let body = match (brace, semi) {
            (Some(b), semi_opt) if semi_opt.is_none_or(|s| b < s) => {
                // Brace-depth scan from the opening brace.
                let body_abs = decl_start + b;
                let mut depth = 0i32;
                let mut end = body_abs;
                for (i, &c) in bytes[body_abs..].iter().enumerate() {
                    match c {
                        b'{' => depth += 1,
                        b'}' => {
                            depth -= 1;
                            if depth == 0 {
                                end = body_abs + i;
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                src[body_abs + 1..end].to_string()
            }
            _ => String::new(),
        };
        if is_decl && !name.is_empty() {
            out.push((name, body));
        }
        search_from = decl_start + "struct ".len();
    }
    out
}

/// The STRUCTURAL arm: a second content-addressed surface store does not need an
/// obvious name — it is structurally a struct holding a `DashMap`/`HashMap` keyed
/// by a content hash whose value stores provider/carrier surface content. This
/// scans every production struct body and FAILS if any (other than the
/// allow-listed `StoreInner` of the single store) holds such a map. This closes
/// the name-scan's gap: a second store named `EngineMirror` / `SnapshotLedger`
/// with `DashMap<ContentHash, Arc<ProviderSurfaceSnapshot>>` would be caught here
/// even though its name carries no marker.
#[test]
fn no_second_content_addressed_surface_store_by_structure() {
    let mut files = Vec::new();
    collect_rs_files(&src_root(), &mut files);

    let mut offenders: Vec<String> = Vec::new();
    for file in &files {
        let raw = std::fs::read_to_string(file).expect("read src file");
        let src = production_only(&raw);
        for (name, body) in struct_bodies(&src) {
            if ALLOWED_SURFACE_STORE_TYPES.contains(&name.as_str()) {
                continue;
            }
            // A content-addressed map FIELD: a `DashMap`/`HashMap` whose generic
            // arguments name a content-hash KEY and a surface-content VALUE.
            for field_line in body.lines() {
                let has_map = field_line.contains("DashMap<") || field_line.contains("HashMap<");
                if !has_map {
                    continue;
                }
                let key_is_content_hash = CONTENT_HASH_KEY_MARKERS
                    .iter()
                    .any(|m| field_line.contains(m));
                let value_is_surface = SURFACE_VALUE_MARKERS.iter().any(|m| field_line.contains(m));
                if key_is_content_hash && value_is_surface {
                    offenders.push(format!(
                        "{}: `struct {name}` holds a content-addressed surface map: {}",
                        file.display(),
                        field_line.trim()
                    ));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "a SECOND content-addressed surface store is forbidden by STRUCTURE (the \
         single-store invariant — ProviderSurfaceStore is the sole record of provider \
         content/maps/ownership). A struct holding a DashMap/HashMap keyed by a \
         content hash storing provider/carrier content is a second store regardless \
         of its name; fold it into ProviderSurfaceStore. Offenders:\n{}",
        offenders.join("\n")
    );
}

/// The structural scanner must POSITIVELY recognize the shape it guards against —
/// otherwise it is a non-discriminating check. This feeds it a fabricated second
/// store and asserts the field-level predicate fires (and that a benign map does
/// not), so the guard above cannot silently degrade to always-pass.
#[test]
fn structural_scanner_recognizes_a_fabricated_second_store() {
    let fabricated = r#"
        pub struct EngineMirror {
            entries: DashMap<ContentHash, Arc<ProviderSurfaceSnapshot>>,
        }
    "#;
    let bodies = struct_bodies(fabricated);
    let (name, body) = bodies
        .iter()
        .find(|(n, _)| n == "EngineMirror")
        .expect("parser must find the fabricated struct");
    assert_eq!(name, "EngineMirror");
    let flagged = body.lines().any(|field_line| {
        (field_line.contains("DashMap<") || field_line.contains("HashMap<"))
            && CONTENT_HASH_KEY_MARKERS
                .iter()
                .any(|m| field_line.contains(m))
            && SURFACE_VALUE_MARKERS.iter().any(|m| field_line.contains(m))
    });
    assert!(
        flagged,
        "the structural predicate must FLAG a DashMap<ContentHash, ProviderSurfaceSnapshot> field"
    );

    // A benign map (not content-addressed, not surface-valued) must NOT be flagged.
    let benign = r#"
        pub struct RequestCounter {
            counts: HashMap<String, u64>,
        }
    "#;
    let benign_bodies = struct_bodies(benign);
    let (_, benign_body) = &benign_bodies[0];
    let benign_flagged = benign_body.lines().any(|field_line| {
        (field_line.contains("DashMap<") || field_line.contains("HashMap<"))
            && CONTENT_HASH_KEY_MARKERS
                .iter()
                .any(|m| field_line.contains(m))
            && SURFACE_VALUE_MARKERS.iter().any(|m| field_line.contains(m))
    });
    assert!(
        !benign_flagged,
        "a benign HashMap<String, u64> must NOT be flagged as a content store"
    );
}
