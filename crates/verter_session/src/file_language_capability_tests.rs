//! Why a framework-capability flip cannot move an artifact key's
//! `file_language_id` — and the guard that fails the moment it can.
//!
//! ## The question
//!
//! The SourceEnv compaction domain claims to cover `parse_env_hash`,
//! `parser_version` AND `file_language_id`, but its counter
//! (`source_env_generation`) is advanced only by env-table republication:
//! `rebuild_and_publish`, `publish_snapshot`, and
//! `WorkspaceChange::ConfigChanged`. A framework-capability flip that
//! reclassified a file's `FileLanguage` would move `file_language_id`
//! WITHOUT any of those — and a SourceEnv aggregate would then validate
//! across a language reclassification — a stale serve, and exactly the
//! class domain-wise compaction must never introduce.
//!
//! ## Why it cannot happen, and it is NOT "no gated rows exist"
//!
//! The artifact key's producer is
//! `FileArtifactKey::derived_file_language_id`, which is
//! `LanguageRegistry::global().classify_static(path).static_resolution()`.
//! `StaticClassification::static_resolution` maps
//! `Gated(candidate) => candidate.fallback` — it deliberately DISCARDS
//! the capability dimension, because consumers below the host seam never
//! see project-gated candidates. So the key's `file_language_id` is
//! capability-independent BY CONSTRUCTION, regardless of what the
//! registry contains: adding the first `Gated` row tomorrow would not
//! change it, because the producer takes the ungated fallback either way.
//!
//! That is a stronger mechanism than the absence of gated rows (which is
//! also true today — `built_in()` registers only `fixed`/`carrier` rows —
//! but is a contingent fact, not a structural one).
//!
//! ## The consequence, stated so it is not lost
//!
//! Because the producer discards capabilities, the per-file
//! classification column does NOT currently track a capability flip. The
//! capability machinery is built but unarmed (`ProjectCapabilitySnapshot`
//! is constructed empty at host construction and has no production
//! producer), so nothing observable depends on it today. SourceEnv's
//! producer set is therefore COMPLETE as it stands.
//!
//! It stops being complete if `derived_file_language_id` is ever routed
//! through the host-level `HostLanguageClassifier` instead of
//! `static_resolution` — the change that would make the per-file column
//! actually capability-sensitive. At that moment SourceEnv needs a
//! producer on the capability-flip path, and this test is what says so.
//!
//! Mutation recipe: change `StaticClassification::static_resolution`'s
//! `Gated` arm to return `candidate.candidate` instead of
//! `candidate.fallback`. The first assertion fails, naming the
//! consequence.

use verter_language::{CapabilityId, FileLanguage, GatedCandidate, StaticClassification};

/// A gated candidate resolves to its UNGATED FALLBACK below the host
/// seam. This is the mechanism that keeps the artifact key's
/// `file_language_id` capability-independent.
#[test]
fn static_resolution_discards_the_capability_dimension() {
    let fallback = FileLanguage::script_ts();
    let candidate = FileLanguage::script(verter_language::ScriptSourceType::jsx());
    assert_ne!(
        fallback, candidate,
        "fixture invariant: the gated candidate must differ from its fallback, or this \
         test cannot tell which one `static_resolution` returned"
    );

    let gated = StaticClassification::Gated(GatedCandidate {
        capability: CapabilityId::new("verter.test.capability"),
        candidate: candidate.clone(),
        fallback: fallback.clone(),
    });

    assert_eq!(
        gated.static_resolution(),
        fallback,
        "`static_resolution` must yield the UNGATED FALLBACK for a gated row. This is \
         what makes `FileArtifactKey::derived_file_language_id` — and therefore the \
         `FileSourceEnv` fact's `file_language_id` — capability-independent by \
         construction. If this ever returns the candidate instead, a framework-capability \
         flip moves `file_language_id` with NO content bump and NO env republication, and \
         the SourceEnv compaction domain's producer set (rebuild_and_publish / \
         publish_snapshot / ConfigChanged) no longer covers it — a SourceEnv aggregate \
         would validate across a language reclassification"
    );
}

/// The producer really is the capability-discarding path: the same
/// canonical classifies identically no matter which host asks.
#[test]
fn derived_file_language_id_is_a_pure_function_of_the_path() {
    let paths = ["/ws/a.ts", "/ws/b.vue", "/ws/c.d.ts", "/ws/d.mjs"];
    for path in paths {
        let first = verter_language::LanguageRegistry::global()
            .classify_static(path)
            .static_resolution();
        let second = verter_language::LanguageRegistry::global()
            .classify_static(path)
            .static_resolution();
        assert_eq!(
            first, second,
            "{path}: classification must be a pure function of the path — the registry is \
             a `OnceLock` with no setter and rows are static data, so nothing a running \
             host does can reclassify a file"
        );
    }
}
