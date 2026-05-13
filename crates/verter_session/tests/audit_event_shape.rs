//! Stage 6c — audit event shape arch guard.
//!
//! R23 scope-fence: new audit-event emissions on the route-surface /
//! augmentation-index call paths use typed `StructuredAuditEvent`
//! variants, not `Custom`. The two variants introduced in Stage 6c
//! (`ModuleAugmentationStitched`, `ModuleAugmentationIndexShape`)
//! MUST be reachable as constructions in the production source and
//! MUST NOT be emitted via `Custom`.

use std::fs;
use std::path::PathBuf;

fn crates_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .to_path_buf()
}

fn walk_rs_files(dir: &PathBuf) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return out;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.extend(walk_rs_files(&p));
        } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
            out.push(p);
        }
    }
    out
}

/// Both new audit-event variants exist on `StructuredAuditEvent`.
/// Compile-time check via type construction.
#[test]
fn stage_6c_audit_event_variants_exist() {
    use std::sync::Arc;
    use verter_audit::{AugmentationTargetKindTag, StructuredAuditEvent};

    // Construct one of each — if the variant or any of its fields
    // change shape, this test fails to compile.
    let stitched = StructuredAuditEvent::ModuleAugmentationStitched {
        target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
        external_specifier: Some(Arc::from("vue")),
        resolved_relative_canonical: None,
        wildcard_pattern: None,
        augmenter_count: 0,
        fingerprint: [0u8; 16],
    };
    let _ = format!("{}", stitched);

    let index_install = StructuredAuditEvent::ModuleAugmentationIndexShape {
        target_kind_tag: AugmentationTargetKindTag::ExternalSpecifier,
        external_specifier: Some(Arc::from("vue")),
        resolved_relative_canonical: None,
        wildcard_pattern: None,
        prev_fingerprint: None,
        new_fingerprint: [0u8; 16],
        augmenter_count: 0,
    };
    let _ = format!("{}", index_install);

    let index_refresh = StructuredAuditEvent::ModuleAugmentationIndexShape {
        target_kind_tag: AugmentationTargetKindTag::ResolvedRelativeCanonical,
        external_specifier: None,
        resolved_relative_canonical: Some(Arc::from("/local.ts")),
        wildcard_pattern: None,
        prev_fingerprint: Some([1u8; 16]),
        new_fingerprint: [2u8; 16],
        augmenter_count: 2,
    };
    let _ = format!("{}", index_refresh);
}

/// `AugmentationTargetKindTag` enumerates exactly the four archetypes
/// required by R29. Compile-time enumeration check via exhaustive
/// match.
#[test]
fn augmentation_target_kind_tag_covers_four_archetypes() {
    use verter_audit::AugmentationTargetKindTag;
    fn classify(t: AugmentationTargetKindTag) -> &'static str {
        match t {
            AugmentationTargetKindTag::ExternalSpecifier => "external",
            AugmentationTargetKindTag::ResolvedRelativeCanonical => "rel",
            AugmentationTargetKindTag::WildcardAmbient => "wild",
            AugmentationTargetKindTag::GlobalAugmentation => "global",
        }
    }
    assert_eq!(
        classify(AugmentationTargetKindTag::ExternalSpecifier),
        "external"
    );
    assert_eq!(
        classify(AugmentationTargetKindTag::ResolvedRelativeCanonical),
        "rel"
    );
    assert_eq!(classify(AugmentationTargetKindTag::WildcardAmbient), "wild");
    assert_eq!(
        classify(AugmentationTargetKindTag::GlobalAugmentation),
        "global"
    );
}

/// The Stage 6c production-source paths that emit augmentation
/// audit events MUST NOT use `StructuredAuditEvent::Custom { ... }`.
///
/// Source-grep over the route-db + file-artifact-store production
/// surface: any `Custom { ... }` construction site there is a
/// scope-fence violation. The general `Custom` arch guard in
/// `no_legacy_trace_surface.rs` requires every construction site to
/// carry a justified comment; this guard goes further on the
/// Stage 6c surface by banning the construction entirely.
#[test]
fn stage_6c_augmentation_paths_do_not_emit_custom() {
    let crates = crates_dir();
    let session_src = crates.join("verter_session").join("src");

    // Files whose production-emission sites are governed by Stage
    // 6c's R23 scope fence — these are exactly the augmentation +
    // route-db files.
    let stage_6c_files = vec![
        session_src.join("resolver_core").join("route_db.rs"),
        session_src.join("file_artifact_store.rs"),
    ];

    for file in &stage_6c_files {
        if !file.exists() {
            continue;
        }
        let text =
            fs::read_to_string(file).unwrap_or_else(|e| panic!("read {}: {}", file.display(), e));
        for (i, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") || trimmed.starts_with("*") {
                continue;
            }
            // Build the discriminator pattern at runtime so this
            // arch-guard source file does not itself contain the
            // literal `StructuredAuditEvent::Custom {` pattern (which
            // would trip the parallel `every_custom_variant_*`
            // justification-comment guard in `no_legacy_trace_surface.rs`
            // without contributing a real `Custom` emission site).
            let custom_pattern = ["StructuredAuditEvent", "::", "Custom", " {"].concat();
            let alias_pattern = ["Event", "::", "Custom", " {"].concat();
            let is_construct =
                line.contains(custom_pattern.as_str()) || line.contains(alias_pattern.as_str());
            // Allow pattern-match arms (rare here): they have `..`
            // inside the braces.
            let is_destructuring = line.contains("Custom { ..")
                || line.contains(" => ")
                || line.trim_end().ends_with("=>");
            if is_construct && !is_destructuring {
                let msg = format!(
                    "R23 scope-fence violation: {}:{} uses the \
                     `Custom` enum-construction syntax on a Stage-6c emission \
                     surface. Augmentation-index and route-db emissions MUST \
                     use the typed `ModuleAugmentationStitched` / \
                     `ModuleAugmentationIndexShape` variants instead.",
                    file.display(),
                    i + 1
                );
                panic!("{msg}");
            }
        }
    }
}

/// Both new audit-event variants are referenced from the Stage 6c
/// production emission sites. Sanity check that the emission helpers
/// exist + reference the typed variants.
#[test]
fn stage_6c_emission_sites_reference_typed_variants() {
    let crates = crates_dir();
    let session_src = crates.join("verter_session").join("src");

    let route_db = session_src.join("resolver_core").join("route_db.rs");
    let file_artifact_store = session_src.join("file_artifact_store.rs");

    let route_db_text = fs::read_to_string(&route_db).expect("route_db.rs");
    let fas_text = fs::read_to_string(&file_artifact_store).expect("file_artifact_store.rs");

    assert!(
        route_db_text.contains("ModuleAugmentationStitched"),
        "route_db.rs MUST reference `ModuleAugmentationStitched` audit event"
    );
    assert!(
        fas_text.contains("ModuleAugmentationIndexShape"),
        "file_artifact_store.rs MUST reference `ModuleAugmentationIndexShape` audit event"
    );
}

#[allow(dead_code)]
fn _suppress_unused_walk() {
    let _ = walk_rs_files;
}
