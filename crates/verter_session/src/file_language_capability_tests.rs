//! Static language classification is a fallback, not artifact-key authority.
//!
//! Scheduler/runtime state owns the language of every loaded canonical and
//! `IndexedReady` retains that exact row. Static classification remains useful
//! only before runtime authority exists, including genuinely overlay-only
//! canonicals and synthetic test keys.

use verter_language::{CapabilityId, FileLanguage, GatedCandidate, StaticClassification};

#[test]
fn static_resolution_discards_the_capability_dimension_for_fallbacks() {
    let fallback = FileLanguage::script_ts();
    let candidate = FileLanguage::script(verter_language::ScriptSourceType::jsx());
    assert_ne!(fallback, candidate, "fixture languages must discriminate");

    let gated = StaticClassification::Gated(GatedCandidate {
        capability: CapabilityId::new("verter.test.capability"),
        candidate,
        fallback: fallback.clone(),
    });

    assert_eq!(
        gated.static_resolution(),
        fallback,
        "pre-runtime static fallback must not guess a capability-gated language"
    );
}

#[test]
fn static_path_classification_is_a_deterministic_fallback() {
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
            "{path}: static fallback must be deterministic"
        );
    }
}
