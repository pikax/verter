//! A carrier parse artifact is an immutable stable unit: its identity is the
//! content/grammar/parse identity of the source, not the version-bearing
//! registered-source snapshot. Reusing one is a read, never a consuming take,
//! so a file that keeps arriving at new source generations with unchanged
//! bytes parses exactly once and every later generation adopts the retained
//! unit. Retention is bounded, and an evicted unit is re-parsed rather than
//! served stale.
//!
//! Regression boundary: a consuming reuse slot serves the FIRST repeat
//! generation and then silently re-parses every one after it, so an editor
//! that revisits the same bytes (undo/redo, save-no-change, re-scan) pays an
//! unbounded number of carrier parses for one immutable unit.

use std::sync::Arc;

use verter_language::carrier_grammar::{
    AcceptedRegisteredCarrierSource, CarrierGrammarAuthority, CarrierGrammarConfig,
    CarrierParserGrammarVersion, FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};
use verter_session::carrier_publication_store::{
    AuditRequestId, CarrierPublicationStore, PublicationRequestContext, PublicationSurface,
};

fn vue_grammar() -> CarrierGrammarConfig {
    CarrierGrammarConfig::vue("{{", "}}", std::iter::empty::<&str>()).unwrap()
}

fn authorities() -> (Arc<RegisteredSourceAuthority>, Arc<CarrierGrammarAuthority>) {
    let source = Arc::new(RegisteredSourceAuthority::new().expect("source authority"));
    let grammar = Arc::new(CarrierGrammarAuthority::new().expect("grammar authority"));
    grammar
        .register_carrier_grammar(
            verter_language::FileLanguage::vue(),
            FrameworkAdapterSemanticVersion::new(1).unwrap(),
            CarrierParserGrammarVersion::new(1).unwrap(),
            vue_grammar(),
        )
        .expect("register Vue grammar");
    (source, grammar)
}

fn accepted(
    source: &RegisteredSourceAuthority,
    grammar: &CarrierGrammarAuthority,
    generation: u64,
    bytes: &str,
) -> AcceptedRegisteredCarrierSource {
    let snapshot = source
        .register_source(
            CanonicalFileId::new("file:///workspace/App.vue"),
            FileIncarnation::new(7),
            SourceGeneration::new(generation),
            verter_language::FileLanguage::vue(),
            Arc::from(bytes),
        )
        .expect("register source");
    grammar
        .accept_registered_source(source, &snapshot, &vue_grammar())
        .expect("accept source")
}

fn request(id: u64, accepted: &AcceptedRegisteredCarrierSource) -> PublicationRequestContext {
    PublicationRequestContext::new(
        AuditRequestId::new(id),
        PublicationSurface::ProjectionHost,
        verter_scheduler::cancellation::CancellationToken::new(),
        accepted.source().snapshot_id().clone(),
    )
}

/// Three generations of the same bytes are three registered snapshots but ONE
/// stable unit. Only the first may run the parser; every later generation
/// adopts the retained unit.
#[test]
fn repeated_generations_of_one_content_parse_once_and_adopt_after() {
    let (source, grammar) = authorities();
    let store = CarrierPublicationStore::new(Arc::clone(&source), Arc::clone(&grammar));
    let bytes = "<template><p>stable</p></template>";

    for generation in 1..=3u64 {
        let accepted = accepted(&source, &grammar, generation, bytes);
        assert!(
            store
                .publish_or_get(&accepted, request(generation, &accepted))
                .into_envelope()
                .is_some(),
            "generation {generation} must publish an envelope"
        );
    }

    let audit = store.audit_snapshot();
    assert_eq!(
        audit.parser_started, 1,
        "one immutable stable unit must parse exactly once across repeated generations, got {audit:?}"
    );
    assert_eq!(
        audit.adopted, 2,
        "the two later generations must adopt the retained unit, got {audit:?}"
    );
}

/// An alternating A/B/A/B edit stream holds TWO stable units. Each parses
/// once; a consuming reuse slot re-parses both on their second revisit.
#[test]
fn alternating_contents_hold_one_stable_unit_each() {
    let (source, grammar) = authorities();
    let store = CarrierPublicationStore::new(Arc::clone(&source), Arc::clone(&grammar));
    let variants = [
        "<template><p>A</p></template>",
        "<template><p>B</p></template>",
    ];

    for generation in 1..=6u64 {
        let bytes = variants[(generation as usize - 1) % 2];
        let accepted = accepted(&source, &grammar, generation, bytes);
        assert!(
            store
                .publish_or_get(&accepted, request(generation, &accepted))
                .into_envelope()
                .is_some(),
            "generation {generation} must publish an envelope"
        );
    }

    let audit = store.audit_snapshot();
    assert_eq!(
        audit.parser_started, 2,
        "two distinct contents means exactly two parses, got {audit:?}"
    );
    assert_eq!(
        audit.adopted, 4,
        "the four revisits must adopt, got {audit:?}"
    );
}

/// Reuse never rewrites provenance: an adopted envelope reports the CALLER's
/// own registered snapshot, not the generation that originally parsed the
/// unit.
#[test]
fn an_adopted_envelope_reports_its_own_generation() {
    let (source, grammar) = authorities();
    let store = CarrierPublicationStore::new(Arc::clone(&source), Arc::clone(&grammar));
    let bytes = "<template><p>provenance</p></template>";

    let first = accepted(&source, &grammar, 1, bytes);
    let first_envelope = store
        .publish_or_get(&first, request(1, &first))
        .into_envelope()
        .expect("first envelope");
    let second = accepted(&source, &grammar, 2, bytes);
    let second_envelope = store
        .publish_or_get(&second, request(2, &second))
        .into_envelope()
        .expect("second envelope");

    assert_eq!(
        first_envelope.source().generation(),
        SourceGeneration::new(1)
    );
    assert_eq!(
        second_envelope.source().generation(),
        SourceGeneration::new(2)
    );
    assert_ne!(first_envelope.id(), second_envelope.id());
    assert_eq!(
        first_envelope.artifact().carrier_structure_hash(),
        second_envelope.artifact().carrier_structure_hash(),
        "one immutable unit reused across generations keeps identical geometry"
    );
}

/// Retention is bounded: once more distinct contents than the configured
/// retention have been published, the least recently used unit is gone and a
/// revisit re-parses it rather than being served from an unbounded store.
#[test]
fn stable_unit_retention_is_bounded_and_evicts_least_recently_used() {
    let (source, grammar) = authorities();
    let store = CarrierPublicationStore::with_stable_unit_retention(
        Arc::clone(&source),
        Arc::clone(&grammar),
        2,
    );

    let contents = [
        "<template><p>one</p></template>",
        "<template><p>two</p></template>",
        "<template><p>three</p></template>",
    ];
    for (index, bytes) in contents.iter().enumerate() {
        let generation = index as u64 + 1;
        let accepted = accepted(&source, &grammar, generation, bytes);
        store.publish_or_get(&accepted, request(generation, &accepted));
    }
    assert_eq!(store.audit_snapshot().parser_started, 3);

    // `one` was evicted when `three` was retained; `three` is still retained.
    let evicted = accepted(&source, &grammar, 4, contents[0]);
    store.publish_or_get(&evicted, request(4, &evicted));
    assert_eq!(
        store.audit_snapshot().parser_started,
        4,
        "an evicted unit must be re-parsed, never served stale"
    );

    let retained = accepted(&source, &grammar, 5, contents[2]);
    store.publish_or_get(&retained, request(5, &retained));
    let audit = store.audit_snapshot();
    assert_eq!(
        audit.parser_started, 4,
        "a retained unit must still adopt, got {audit:?}"
    );
    assert_eq!(audit.adopted, 1, "exactly the retained revisit adopted");
}
