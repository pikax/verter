//! Audit event shape arch guard.
//!
//! R23 scope-fence: new audit-event emissions on the cache-subsystem
//! call paths (route-surface, augmentation-index, file-artifact-cache,
//! fact-registry, validation summary, route resolution) use typed
//! `StructuredAuditEvent` variants, NOT `Custom`. All variants
//! enumerated on the cache subsystem MUST be reachable as
//! constructions in the production source and MUST NOT be emitted
//! via `Custom`.

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

/// Stage 6d — `FactSignatureAdmissionRefused` and
/// `FactSignatureOverflow` are both reachable as construction sites.
/// Compile-time check via type construction. R23 scope-fence: the
/// Stage 6d emissions on the admission-guard call paths use typed
/// variants, not `Custom`.
#[test]
fn stage_6d_admission_guard_audit_event_variants_exist() {
    use std::sync::Arc;
    use verter_audit::{AdmissionRefusalReason, StructuredAuditEvent};

    let refused = StructuredAuditEvent::FactSignatureAdmissionRefused {
        cache_kind: Arc::from("validated_fact_cache_generic"),
        reason: AdmissionRefusalReason::EmptySignature,
    };
    let s = format!("{}", refused);
    assert!(
        s.contains("FactSignatureAdmissionRefused"),
        "Display arm must render the variant name, got {s}"
    );
    assert!(
        s.contains("EmptySignature"),
        "Display arm must render the refusal reason, got {s}"
    );

    let overflow = StructuredAuditEvent::FactSignatureOverflow {
        candidate_size: 2048,
        cap: 1024,
    };
    let s = format!("{}", overflow);
    assert!(
        s.contains("FactSignatureOverflow"),
        "Display arm must render the variant name, got {s}"
    );
}

/// `AdmissionRefusalReason` enumerates both refusal reasons documented
/// by the Stage 6d admission-guard contract. Compile-time enumeration
/// check via exhaustive match.
#[test]
fn admission_refusal_reason_covers_documented_reasons() {
    use verter_audit::AdmissionRefusalReason;
    fn classify(r: AdmissionRefusalReason) -> &'static str {
        match r {
            AdmissionRefusalReason::EmptySignature => "empty",
            AdmissionRefusalReason::NonCacheableKind => "non-cacheable",
        }
    }
    assert_eq!(classify(AdmissionRefusalReason::EmptySignature), "empty");
    assert_eq!(
        classify(AdmissionRefusalReason::NonCacheableKind),
        "non-cacheable"
    );
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

/// R23 scope-fence: the four cache-subsystem audit-event variants
/// (`FileArtifactCache`, `FactRegistryWrite`, `FactValidationSummary`,
/// `ExportRouteResolved`) MUST exist on `StructuredAuditEvent`.
/// Compile-time check via type construction — if a variant or any
/// of its fields change shape, this test fails to compile.
#[test]
fn r23_cache_subsystem_audit_event_variants_exist() {
    use std::sync::Arc;
    use verter_audit::{
        FactKeyKindTag, FactLaneTag, FileArtifactCacheAction, StructuredAuditEvent,
    };

    let admit = StructuredAuditEvent::FileArtifactCache {
        canonical_id: Arc::from("/w/a.ts"),
        action: FileArtifactCacheAction::Admit,
        content_hash: [0u8; 16],
        parse_env_hash: [0u8; 16],
        entry_count_after: 0,
    };
    let _ = format!("{admit}");

    let evict = StructuredAuditEvent::FileArtifactCache {
        canonical_id: Arc::from("/w/a.ts"),
        action: FileArtifactCacheAction::Evict,
        content_hash: [0u8; 16],
        parse_env_hash: [0u8; 16],
        entry_count_after: 0,
    };
    let _ = format!("{evict}");

    let write = StructuredAuditEvent::FactRegistryWrite {
        canonical_id: Arc::from("/w/a.ts"),
        fact_key_kind: FactKeyKindTag::Export,
        lane: FactLaneTag::Semantic,
        semantic_hash: [0u8; 16],
        display_hash: [0u8; 16],
    };
    let _ = format!("{write}");

    let summary = StructuredAuditEvent::FactValidationSummary {
        request_id: 1,
        cache_kind: Arc::from("validated_fact_cache_generic"),
        validations_attempted: 0,
        warm_hits: 0,
        stale_misses: 0,
        archive_checks: 0,
    };
    let _ = format!("{summary}");

    let route = StructuredAuditEvent::ExportRouteResolved {
        provider_canonical: Arc::from("/w/idx.ts"),
        exported_name: Arc::from("Foo"),
        resolved_canonical: Arc::from("/w/lib.ts"),
        resolved_source_name: Arc::from("Foo"),
        augmented: false,
    };
    let _ = format!("{route}");
}

/// `FactKeyKindTag` enumerates the 12 parse-domain `FactKey`
/// structural shapes. Compile-time enumeration check via
/// exhaustive match.
#[test]
fn fact_key_kind_tag_covers_parse_domain_factkey_shapes() {
    use verter_audit::FactKeyKindTag;
    fn classify(t: FactKeyKindTag) -> &'static str {
        match t {
            FactKeyKindTag::Export => "Export",
            FactKeyKindTag::ExportAlias => "ExportAlias",
            FactKeyKindTag::SyntacticExportSet => "SyntacticExportSet",
            FactKeyKindTag::LocalDecl => "LocalDecl",
            FactKeyKindTag::Member => "Member",
            FactKeyKindTag::MemberPresence => "MemberPresence",
            FactKeyKindTag::MemberShape => "MemberShape",
            FactKeyKindTag::MacroSurface => "MacroSurface",
            FactKeyKindTag::TemplateRoot => "TemplateRoot",
            FactKeyKindTag::ImportRef => "ImportRef",
            FactKeyKindTag::SyntacticReexportRef => "SyntacticReexportRef",
            FactKeyKindTag::ModuleAugmentation => "ModuleAugmentation",
        }
    }
    // Touch every variant so the match is exhaustive in both
    // directions; if a future `FactKeyKindTag` variant is added
    // without updating the match, the compile fails.
    assert_eq!(classify(FactKeyKindTag::Export), "Export");
    assert_eq!(classify(FactKeyKindTag::ExportAlias), "ExportAlias");
    assert_eq!(
        classify(FactKeyKindTag::SyntacticExportSet),
        "SyntacticExportSet"
    );
    assert_eq!(classify(FactKeyKindTag::LocalDecl), "LocalDecl");
    assert_eq!(classify(FactKeyKindTag::Member), "Member");
    assert_eq!(classify(FactKeyKindTag::MemberPresence), "MemberPresence");
    assert_eq!(classify(FactKeyKindTag::MemberShape), "MemberShape");
    assert_eq!(classify(FactKeyKindTag::MacroSurface), "MacroSurface");
    assert_eq!(classify(FactKeyKindTag::TemplateRoot), "TemplateRoot");
    assert_eq!(classify(FactKeyKindTag::ImportRef), "ImportRef");
    assert_eq!(
        classify(FactKeyKindTag::SyntacticReexportRef),
        "SyntacticReexportRef"
    );
    assert_eq!(
        classify(FactKeyKindTag::ModuleAugmentation),
        "ModuleAugmentation"
    );
}

/// `FactLaneTag` and `FileArtifactCacheAction` enumerate their
/// documented shapes. Compile-time enumeration check.
#[test]
fn fact_lane_tag_and_file_artifact_cache_action_cover_documented_variants() {
    use verter_audit::{FactLaneTag, FileArtifactCacheAction};
    fn classify_lane(t: FactLaneTag) -> &'static str {
        match t {
            FactLaneTag::Semantic => "Semantic",
            FactLaneTag::Display => "Display",
        }
    }
    fn classify_action(t: FileArtifactCacheAction) -> &'static str {
        match t {
            FileArtifactCacheAction::Admit => "Admit",
            FileArtifactCacheAction::Evict => "Evict",
        }
    }
    assert_eq!(classify_lane(FactLaneTag::Semantic), "Semantic");
    assert_eq!(classify_lane(FactLaneTag::Display), "Display");
    assert_eq!(classify_action(FileArtifactCacheAction::Admit), "Admit");
    assert_eq!(classify_action(FileArtifactCacheAction::Evict), "Evict");
}

/// R23 scope-fence — the production-source paths that admit cache-
/// subsystem entries MUST NOT use `StructuredAuditEvent::Custom`.
/// Mirrors `stage_6c_augmentation_paths_do_not_emit_custom` but
/// covers the four additional emission surfaces.
#[test]
fn cache_subsystem_paths_do_not_emit_custom() {
    let crates = crates_dir();
    let session_src = crates.join("verter_session").join("src");

    // The post-stage cache-subsystem emission surfaces. Any
    // `StructuredAuditEvent::Custom { ... }` construction site on
    // these files is a scope-fence violation.
    let cache_subsystem_files = vec![
        session_src.join("resolver_core").join("mod.rs"),
        session_src.join("resolver_core").join("route_db.rs"),
        session_src.join("file_artifact_store.rs"),
    ];

    for file in &cache_subsystem_files {
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
            // arch-guard source file does not contain the literal
            // construct it forbids elsewhere.
            let custom_pattern = ["StructuredAuditEvent", "::", "Custom", " {"].concat();
            let alias_pattern = ["Event", "::", "Custom", " {"].concat();
            let is_construct =
                line.contains(custom_pattern.as_str()) || line.contains(alias_pattern.as_str());
            let is_destructuring = line.contains("Custom { ..")
                || line.contains(" => ")
                || line.trim_end().ends_with("=>");
            if is_construct && !is_destructuring {
                panic!(
                    "R23 scope-fence violation: {}:{} uses the \
                     `Custom` enum-construction syntax on a cache-subsystem \
                     emission surface. Cache subsystem emissions MUST use \
                     the typed variants `FileArtifactCache`, `FactRegistryWrite`, \
                     `FactValidationSummary`, or `ExportRouteResolved` instead.",
                    file.display(),
                    i + 1,
                );
            }
        }
    }
}

/// Shadow-scaffold sanity check: `Candidate.legacy_dep_signature`
/// field is present on the public type signature. Stage 7's
/// revert deletes this field; the test compiles only on the
/// integration branch tree.
#[test]
fn shadow_scaffold_candidate_carries_legacy_dep_signature_field() {
    use std::sync::Arc;
    use verter_session::resolver_core::{Candidate, FactVersionRef, LegacyDepSignature};
    // Construct a Candidate directly to force the field to be
    // referenceable. If the field is renamed or removed, this
    // test fails to compile.
    let _c: Candidate<u32> = Candidate {
        signature_fingerprint: [0u8; 16],
        value: Arc::new(0u32),
        fact_dep_signature: Arc::<[FactVersionRef]>::from(Vec::<FactVersionRef>::new()),
        legacy_dep_signature: Some(LegacyDepSignature {
            opaque: Arc::<[u8]>::from(Vec::<u8>::new()),
        }),
    };
}

/// Shadow-scaffold sanity check: the parity-counter accessor exists
/// on `ValidatedFactCache`. Stage 7's revert deletes the accessor.
#[test]
fn shadow_scaffold_parity_mismatch_count_accessor_exists() {
    use verter_session::resolver_core::ValidatedFactCache;
    let cache: ValidatedFactCache<&'static str, u32> = ValidatedFactCache::default();
    let n = cache.parity_mismatch_count();
    assert_eq!(n, 0, "empty cache has zero parity mismatches");
}

#[allow(dead_code)]
fn _suppress_unused_walk() {
    let _ = walk_rs_files;
}
