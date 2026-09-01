use std::sync::Arc;
use std::sync::Barrier;
use std::thread;

use verter_language::carrier_grammar::{
    CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
    FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};

use crate::carrier_publication_store::{
    AuditRequestId, CarrierPublicationStore, PublicationOutcome, PublicationRequestContext,
    PublicationSurface,
};

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

fn vue_grammar() -> CarrierGrammarConfig {
    CarrierGrammarConfig::vue("{{", "}}", std::iter::empty::<&str>()).unwrap()
}

fn svelte_authorities() -> (Arc<RegisteredSourceAuthority>, Arc<CarrierGrammarAuthority>) {
    let source = Arc::new(RegisteredSourceAuthority::new().expect("source authority"));
    let grammar = Arc::new(CarrierGrammarAuthority::new().expect("grammar authority"));
    grammar
        .register_carrier_grammar(
            verter_language::FileLanguage::svelte(),
            FrameworkAdapterSemanticVersion::new(1).unwrap(),
            CarrierParserGrammarVersion::new(1).unwrap(),
            CarrierGrammarConfig::Svelte,
        )
        .expect("register Svelte grammar");
    (source, grammar)
}

fn accepted(
    source: &RegisteredSourceAuthority,
    grammar: &CarrierGrammarAuthority,
    generation: u64,
    bytes: &str,
) -> verter_language::carrier_grammar::AcceptedRegisteredCarrierSource {
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

fn request(
    id: u64,
    accepted: &verter_language::carrier_grammar::AcceptedRegisteredCarrierSource,
) -> PublicationRequestContext {
    PublicationRequestContext::new(
        AuditRequestId::new(id),
        PublicationSurface::ProjectionHost,
        verter_scheduler::cancellation::CancellationToken::new(),
        accepted.source().snapshot_id().clone(),
    )
}

/// A strict-parse Svelte defect (unterminated trailing comment) stays
/// publishable: the tokenizer is infallible and always yields a usable
/// tree. The client-runtime `official_reject_gate` is a compile-time
/// contract, not a reason to refuse carrier publication. IDE features
/// need a usable artifact for a mid-typing document.
#[test]
fn strict_syntax_defect_still_publishes_a_recoverable_product_family() {
    use crate::carrier_publication_store::PublicationAuditKind;

    let strict_source = "<script>let x = 1;</script><div>{x}</div><style>.x{color:red}</style><!--";
    let (source, grammar) = svelte_authorities();
    let snapshot = source
        .register_source(
            CanonicalFileId::new("file:///workspace/App.svelte"),
            FileIncarnation::new(7),
            SourceGeneration::new(1),
            verter_language::FileLanguage::svelte(),
            Arc::from(strict_source),
        )
        .expect("register source");
    let accepted = grammar
        .accept_registered_source(&source, &snapshot, &CarrierGrammarConfig::Svelte)
        .expect("accept source");
    let store = CarrierPublicationStore::new(source, grammar);

    let outcome = store.publish_or_get(&accepted, request(1, &accepted));
    // Unterminated `<!--` is `Error` with `blocks_compile: false`, so it
    // does not set `has_errors`. Artifact still publishes.
    match outcome.clone().into_envelope() {
        Some(_) => {}
        None => panic!("a recoverable strict-parse defect must still publish, got {outcome:?}"),
    };

    let events = store.audit_events();
    assert!(events
        .iter()
        .any(|event| event.kind == PublicationAuditKind::ParserStarted));
    assert!(events
        .iter()
        .any(|event| event.kind == PublicationAuditKind::ParserFinished));
    assert!(events
        .iter()
        .any(|event| event.kind == PublicationAuditKind::PublishFencePassed));
    assert!(events
        .iter()
        .any(|event| event.kind == PublicationAuditKind::Published));

    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = crate::VerterHost::new(crate::HostConfig::default(), workspace);
    let update = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some("/Strict.svelte".into()),
            input_id: "/Strict.svelte".into(),
            source: Arc::from(strict_source),
            file_language: verter_language::FileLanguage::svelte(),
            aliases: Vec::new(),
        })
        .expect("a recoverable strict-parse defect must still admit the document");
    // Upsert parse diagnostics surface official `unexpected_eof` for the
    // empty `<!--` — admitted and parsed, sole diagnostic, `has_errors`
    // stays off (`blocks_compile: false`).
    assert!(
        update
            .diagnostics
            .diagnostics
            .iter()
            .any(|d| d.code == "unexpected_eof"),
        "the admitted document's own diagnostics must carry the parse defect: {:?}",
        update.diagnostics
    );
}

/// A recoverable strict-parse defect (`expected_attribute_value`) must
/// not fail-close compile for the whole file. The diagnostic is `Error`
/// but must not set `has_errors`. Same class as
/// `strict_syntax_defect_still_publishes_a_recoverable_product_family`,
/// one layer up.
#[test]
fn strict_syntax_defect_does_not_fail_close_ide_compile_for_the_whole_file() {
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    let source = "<script>let count = 1;</script>\n<div class= ></div>\n<p>{count}</p>\n";
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = crate::VerterHost::new(crate::HostConfig::default(), workspace);
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some("/StrictCompile.svelte".into()),
            input_id: "/StrictCompile.svelte".into(),
            source: Arc::from(source),
            file_language: verter_language::FileLanguage::svelte(),
            aliases: Vec::new(),
        })
        .expect("a recoverable strict-parse defect must still admit the document");

    let profile = crate::CompileProfile {
        target: crate::CompileTarget::IDE,
        ..crate::CompileProfile::default()
    };
    let ensured = host.ensure_ide_compiled("/StrictCompile.svelte", &profile);
    assert!(
        ensured.is_ok(),
        "a single recoverable strict-parse defect ({:?}) must not fail-close IDE \
         compilation for the whole file, got: {ensured:?}",
        "expected_attribute_value",
    );
    assert!(
        host.get_ide("/StrictCompile.svelte", &profile).is_some(),
        "the IDE product must still be published for a file with only a \
         recoverable strict-parse defect"
    );
}

#[test]
fn concurrent_publication_has_one_parser_start_and_one_terminal_arc() {
    let (source, grammar) = authorities();
    let accepted = accepted(&source, &grammar, 1, "<template><p>one</p></template>");
    let store = Arc::new(CarrierPublicationStore::new(source, grammar));
    let mut workers = Vec::new();
    for id in 1..=8 {
        let store = Arc::clone(&store);
        let accepted = accepted.clone();
        workers.push(thread::spawn(move || {
            store.publish_or_get(&accepted, request(id, &accepted))
        }));
    }
    let envelopes: Vec<_> = workers
        .into_iter()
        .map(|worker| match worker.join().expect("publication worker") {
            PublicationOutcome::Published(envelope) => envelope,
            other => panic!("unexpected publication outcome: {other:?}"),
        })
        .collect();
    assert!(envelopes
        .iter()
        .all(|envelope| Arc::ptr_eq(envelope, &envelopes[0])));
    let audit = store.audit_snapshot();
    assert_eq!(audit.parser_started, 1);
    assert_eq!(audit.leaders, 1);
    assert_eq!(
        audit.waiters + audit.live_hits,
        7,
        "every non-leader must either join Producing or observe the installed terminal",
    );
}

#[test]
fn source_generation_a_b_a_never_reuses_an_old_terminal() {
    let (source, grammar) = authorities();
    let store = CarrierPublicationStore::new(Arc::clone(&source), Arc::clone(&grammar));
    let first = accepted(&source, &grammar, 1, "<template>A</template>");
    let first_envelope = store
        .publish_or_get(&first, request(1, &first))
        .into_envelope()
        .expect("first envelope");
    let middle = accepted(&source, &grammar, 2, "<template>B</template>");
    let middle_envelope = store
        .publish_or_get(&middle, request(2, &middle))
        .into_envelope()
        .expect("middle envelope");
    let last = accepted(&source, &grammar, 3, "<template>A</template>");
    let last_envelope = store
        .publish_or_get(&last, request(3, &last))
        .into_envelope()
        .expect("last envelope");
    assert_ne!(first_envelope.id(), middle_envelope.id());
    assert_ne!(first_envelope.id(), last_envelope.id());
    assert!(!Arc::ptr_eq(&first_envelope, &last_envelope));
}

#[test]
fn registered_file_structure_is_the_envelope_owner() {
    let (source, grammar) = authorities();
    let store = CarrierPublicationStore::new(Arc::clone(&source), Arc::clone(&grammar));
    let accepted = accepted(&source, &grammar, 1, "<script setup>const x = 1</script>");
    let envelope = store
        .publish_or_get(&accepted, request(1, &accepted))
        .into_envelope()
        .expect("envelope");
    let structure = crate::carrier_publication_store::RegisteredFileStructure::new(envelope);
    assert!(Arc::ptr_eq(
        structure.envelope().artifact(),
        structure.artifact()
    ));
    assert!(Arc::ptr_eq(
        structure.artifact().inventory(),
        structure.envelope().inventory()
    ));
}

#[test]
fn registered_structure_views_resolve_noncanonical_alias_spelling() {
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = crate::VerterHost::new(crate::HostConfig::default(), workspace);
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some("C:/src/App.vue".to_string()),
            input_id: "C:/src/App.vue".to_string(),
            source: Arc::from("<script setup>const x = 1</script>"),
            file_language: verter_language::FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("carrier upsert");

    assert!(host.registered_file_structure("C:\\src\\App.vue").is_some());
    assert!(host
        .registered_file_structure_snapshot("C:\\src\\App.vue")
        .is_some());
    assert!(host
        .registered_source_revision_token("C:\\src\\App.vue")
        .is_some());
}

#[test]
fn exact_cohort_adopts_across_authority_lifetimes_without_parser_start() {
    let persistence = Arc::new(
        crate::carrier_publication_store::persistence::InMemoryCarrierPersistence::default(),
    );
    let (first_source, first_grammar) = authorities();
    let first = accepted(
        &first_source,
        &first_grammar,
        1,
        "<template><p>persisted</p></template>",
    );
    let first_store = CarrierPublicationStore::with_dependencies(
        first_source,
        first_grammar,
        persistence.clone(),
        Arc::new(crate::types::MetaProvenance::default()),
    );
    assert!(matches!(
        first_store.publish_or_get(&first, request(1, &first)),
        PublicationOutcome::Published(_)
    ));

    let (second_source, second_grammar) = authorities();
    let second = accepted(
        &second_source,
        &second_grammar,
        1,
        "<template><p>persisted</p></template>",
    );
    let second_store = CarrierPublicationStore::with_dependencies(
        second_source,
        second_grammar,
        persistence,
        Arc::new(crate::types::MetaProvenance::default()),
    );
    let adopted = second_store.publish_or_get(&second, request(2, &second));
    assert!(matches!(adopted, PublicationOutcome::Adopted(_)));
    assert_eq!(
        second_store.audit_snapshot(),
        crate::carrier_publication_store::PublicationAuditSnapshot {
            leaders: 1,
            adopted: 1,
            ..Default::default()
        }
    );
}

struct BlockingPersistence {
    entered: Arc<Barrier>,
    release: Arc<Barrier>,
}

impl crate::carrier_publication_store::persistence::CarrierPersistence for BlockingPersistence {
    fn take_candidate(
        &self,
        _id: &crate::carrier_publication_store::FrameworkArtifactId,
        _accepted: &verter_language::carrier_grammar::AcceptedRegisteredCarrierSource,
    ) -> Option<crate::carrier_publication_store::persistence::PersistedCarrierCandidate> {
        self.entered.wait();
        self.release.wait();
        None
    }

    fn store_success(
        &self,
        _id: &crate::carrier_publication_store::FrameworkArtifactId,
        _accepted: &verter_language::carrier_grammar::AcceptedRegisteredCarrierSource,
        _artifact: &Arc<verter_compiler::framework_common::FrameworkParseArtifact>,
        _cohort: crate::carrier_artifact_cohort::PersistedCarrierArtifactCohort,
    ) {
    }
}

#[test]
fn waiter_cancellation_detaches_without_cancelling_authority_owned_leader() {
    let (source, grammar) = authorities();
    let accepted = accepted(&source, &grammar, 1, "<template>leader</template>");
    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let persistence = Arc::new(BlockingPersistence {
        entered: Arc::clone(&entered),
        release: Arc::clone(&release),
    });
    let store = Arc::new(CarrierPublicationStore::with_dependencies(
        source,
        grammar,
        persistence,
        Arc::new(crate::types::MetaProvenance::default()),
    ));
    let leader_store = Arc::clone(&store);
    let leader_accepted = accepted.clone();
    let leader = thread::spawn(move || {
        leader_store.publish_or_get(&leader_accepted, request(1, &leader_accepted))
    });
    entered.wait();

    let cancellation = verter_scheduler::cancellation::CancellationToken::new();
    let waiter_store = Arc::clone(&store);
    let waiter_accepted = accepted.clone();
    let waiter_cancellation = cancellation.clone();
    let waiter = thread::spawn(move || {
        waiter_store.publish_or_get(
            &waiter_accepted,
            PublicationRequestContext::new(
                AuditRequestId::new(2),
                PublicationSurface::ProjectionHost,
                waiter_cancellation,
                waiter_accepted.source().snapshot_id().clone(),
            ),
        )
    });
    while store.audit_snapshot().waiters == 0 {
        thread::yield_now();
    }
    cancellation.cancel();
    assert!(matches!(
        waiter.join().unwrap(),
        PublicationOutcome::Cancelled
    ));
    release.wait();
    assert!(matches!(
        leader.join().unwrap(),
        PublicationOutcome::Published(_)
    ));
    assert_eq!(store.audit_snapshot().parser_started, 1);
}

#[test]
fn scheduled_semantic_host_ingests_the_identical_registered_artifact_arc() {
    use std::sync::atomic::Ordering;

    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    fn host() -> Arc<crate::VerterHost> {
        let workspace: Arc<dyn WorkspaceAccess> =
            Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
        Arc::new(crate::VerterHost::new(
            crate::HostConfig::default(),
            workspace,
        ))
    }

    let projection = host();
    let semantic = host();
    let source: Arc<str> = Arc::from("<template><p>shared</p></template>");
    let request = crate::UpsertRequest {
        canonical_id: Some("/App.vue".to_string()),
        input_id: "/App.vue".to_string(),
        source: Arc::clone(&source),
        file_language: verter_language::FileLanguage::vue(),
        aliases: Vec::new(),
    };
    let _ = projection
        .upsert(request.clone())
        .expect("projection upsert");
    let structure = projection
        .registered_file_structure("/App.vue")
        .expect("projection structure");

    let _ = semantic
        .upsert_registered_envelope(request, structure.clone())
        .expect("semantic envelope ingestion");
    let semantic_structure = semantic
        .registered_file_structure("/App.vue")
        .expect("semantic structure");

    assert!(Arc::ptr_eq(
        structure.artifact(),
        semantic_structure.artifact()
    ));
    assert_eq!(
        semantic.provenance.carrier_parses.load(Ordering::Relaxed),
        0,
        "semantic ingestion must not start a parser"
    );
}

#[test]
fn equal_compile_grammar_reuses_registered_artifact_and_mismatch_is_typed() {
    use crate::CompileTarget;
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = crate::VerterHost::new(crate::HostConfig::default(), workspace);
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some("/Grammar.vue".to_string()),
            input_id: "/Grammar.vue".to_string(),
            source: Arc::from("<template>{{ value }}</template>"),
            file_language: verter_language::FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("registered upsert");
    let before = host
        .registered_file_structure("/Grammar.vue")
        .expect("registered structure");

    let equal = crate::CompileProfile {
        target: CompileTarget::BUNDLER,
        delimiters: Some(("{{".to_string(), "}}".to_string())),
        ..crate::CompileProfile::default()
    };
    let _ = host
        .get_virtual_file(crate::VirtualQuery {
            raw_id: None,
            canonical_id: Some("/Grammar.vue".to_string()),
            node_kind: Some(crate::VirtualNodeKind::Main),
            compile_profile: equal,
        })
        .expect("equal grammar compiles");
    let after_equal = host
        .registered_file_structure("/Grammar.vue")
        .expect("structure after equal compile");
    assert!(Arc::ptr_eq(before.artifact(), after_equal.artifact()));

    let mismatch = crate::CompileProfile {
        target: CompileTarget::BUNDLER,
        delimiters: Some(("[[".to_string(), "]]".to_string())),
        ..crate::CompileProfile::default()
    };
    let outcome = host.get_virtual_file(crate::VirtualQuery {
        raw_id: None,
        canonical_id: Some("/Grammar.vue".to_string()),
        node_kind: Some(crate::VirtualNodeKind::Main),
        compile_profile: mismatch,
    });
    assert!(matches!(outcome, Err(crate::HostError::GrammarMismatch(_))));
    let after_mismatch = host
        .registered_file_structure("/Grammar.vue")
        .expect("structure after mismatch");
    assert!(Arc::ptr_eq(before.artifact(), after_mismatch.artifact()));
}

#[test]
fn external_src_compile_is_typed_and_fail_closed() {
    use crate::CompileTarget;
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = crate::VerterHost::new(crate::HostConfig::default(), workspace);
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some("/External.vue".to_string()),
            input_id: "/External.vue".to_string(),
            source: Arc::from("<template src=\"./External.html\"></template>"),
            file_language: verter_language::FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("registered external-src upsert");

    let outcome = host.get_virtual_file(crate::VirtualQuery {
        raw_id: None,
        canonical_id: Some("/External.vue".to_string()),
        node_kind: Some(crate::VirtualNodeKind::Main),
        compile_profile: crate::CompileProfile {
            target: CompileTarget::BUNDLER,
            ..crate::CompileProfile::default()
        },
    });
    assert!(matches!(
        outcome,
        Err(crate::HostError::BlockContentRefused(
            crate::BlockContentRefusal::Unavailable {
                availability: crate::BlockContentAvailability::Missing,
                ..
            }
        ))
    ));
}

#[test]
fn grammar_revision_keys_a_new_publication_lane() {
    let (source, grammar) = authorities();
    let store = CarrierPublicationStore::new(Arc::clone(&source), Arc::clone(&grammar));
    let first = accepted(&source, &grammar, 1, "<template>{{ value }}</template>");
    let first_envelope = store
        .publish_or_get(&first, request(1, &first))
        .into_envelope()
        .expect("first grammar publication");

    let revised_config =
        CarrierGrammarConfig::vue("[[", "]]", std::iter::empty::<&str>()).expect("revised grammar");
    grammar
        .register_carrier_grammar(
            verter_language::FileLanguage::vue(),
            FrameworkAdapterSemanticVersion::new(1).unwrap(),
            CarrierParserGrammarVersion::new(2).unwrap(),
            revised_config.clone(),
        )
        .expect("register grammar revision");
    let revised = grammar
        .accept_registered_source(&source, first.source(), &revised_config)
        .expect("accept revised grammar");
    let revised_envelope = store
        .publish_or_get(&revised, request(2, &revised))
        .into_envelope()
        .expect("revised grammar publication");

    assert!(!Arc::ptr_eq(&first_envelope, &revised_envelope));
    assert_eq!(store.audit_snapshot().parser_started, 2);
}

#[derive(Default)]
struct CorruptingPersistence {
    inner: crate::carrier_publication_store::persistence::InMemoryCarrierPersistence,
    corrupt_next: std::sync::atomic::AtomicBool,
}

#[derive(Default)]
struct ProducerDriftPersistence {
    inner: crate::carrier_publication_store::persistence::InMemoryCarrierPersistence,
    replacement:
        std::sync::Mutex<Option<Arc<verter_compiler::framework_common::FrameworkParseArtifact>>>,
}

impl crate::carrier_publication_store::persistence::CarrierPersistence
    for ProducerDriftPersistence
{
    fn take_candidate(
        &self,
        id: &crate::carrier_publication_store::FrameworkArtifactId,
        accepted: &verter_language::carrier_grammar::AcceptedRegisteredCarrierSource,
    ) -> Option<crate::carrier_publication_store::persistence::PersistedCarrierCandidate> {
        let mut candidate =
            crate::carrier_publication_store::persistence::CarrierPersistence::take_candidate(
                &self.inner,
                id,
                accepted,
            )?;
        if let Some(replacement) = self.replacement.lock().unwrap().take() {
            candidate.replace_artifact_for_test(replacement);
        }
        Some(candidate)
    }

    fn store_success(
        &self,
        id: &crate::carrier_publication_store::FrameworkArtifactId,
        accepted: &verter_language::carrier_grammar::AcceptedRegisteredCarrierSource,
        artifact: &Arc<verter_compiler::framework_common::FrameworkParseArtifact>,
        cohort: crate::carrier_artifact_cohort::PersistedCarrierArtifactCohort,
    ) {
        crate::carrier_publication_store::persistence::CarrierPersistence::store_success(
            &self.inner,
            id,
            accepted,
            artifact,
            cohort,
        );
    }
}

#[test]
fn persisted_payload_with_producer_parse_drift_is_refused_before_adoption() {
    let (wrong_source, wrong_grammar) = authorities();
    let wrong = accepted(
        &wrong_source,
        &wrong_grammar,
        1,
        "<template><p>wrong</p></template>",
    );
    let wrong_store = CarrierPublicationStore::new(wrong_source, wrong_grammar);
    let wrong_artifact = wrong_store
        .publish_or_get(&wrong, request(1, &wrong))
        .into_envelope()
        .expect("wrong fixture publishes")
        .artifact()
        .clone();

    let persistence = Arc::new(ProducerDriftPersistence::default());
    let source_text = "<template><p>right</p></template>";
    assert_eq!(
        wrong.source().bytes().len(),
        source_text.len(),
        "fixture must defeat length-only adoption"
    );
    let (first_source, first_grammar) = authorities();
    let first = accepted(&first_source, &first_grammar, 1, source_text);
    let first_store = CarrierPublicationStore::with_dependencies(
        first_source,
        first_grammar,
        persistence.clone(),
        Arc::new(crate::types::MetaProvenance::default()),
    );
    assert!(matches!(
        first_store.publish_or_get(&first, request(2, &first)),
        PublicationOutcome::Published(_)
    ));
    *persistence.replacement.lock().unwrap() = Some(wrong_artifact);

    let (second_source, second_grammar) = authorities();
    let second = accepted(&second_source, &second_grammar, 1, source_text);
    let second_store = CarrierPublicationStore::with_dependencies(
        second_source,
        second_grammar,
        persistence,
        Arc::new(crate::types::MetaProvenance::default()),
    );
    assert!(matches!(
        second_store.publish_or_get(&second, request(3, &second)),
        PublicationOutcome::Published(_)
    ));
    assert_eq!(second_store.audit_snapshot().rejected_candidates, 1);
    assert_eq!(second_store.audit_snapshot().parser_started, 1);
    assert!(second_store.audit_events().iter().any(|event| matches!(
        event.kind,
        crate::carrier_publication_store::PublicationAuditKind::PersistentAdoptionRejected(
            crate::carrier_publication_store::PersistentAdoptionRejection::ParserValidationFailed
        )
    )));
}

impl crate::carrier_publication_store::persistence::CarrierPersistence for CorruptingPersistence {
    fn take_candidate(
        &self,
        id: &crate::carrier_publication_store::FrameworkArtifactId,
        accepted: &verter_language::carrier_grammar::AcceptedRegisteredCarrierSource,
    ) -> Option<crate::carrier_publication_store::persistence::PersistedCarrierCandidate> {
        use std::sync::atomic::Ordering;

        let mut candidate =
            crate::carrier_publication_store::persistence::CarrierPersistence::take_candidate(
                &self.inner,
                id,
                accepted,
            )?;
        if self.corrupt_next.swap(false, Ordering::AcqRel) {
            candidate.corrupt_checksum_for_test();
        }
        Some(candidate)
    }

    fn store_success(
        &self,
        id: &crate::carrier_publication_store::FrameworkArtifactId,
        accepted: &verter_language::carrier_grammar::AcceptedRegisteredCarrierSource,
        artifact: &Arc<verter_compiler::framework_common::FrameworkParseArtifact>,
        cohort: crate::carrier_artifact_cohort::PersistedCarrierArtifactCohort,
    ) {
        crate::carrier_publication_store::persistence::CarrierPersistence::store_success(
            &self.inner,
            id,
            accepted,
            artifact,
            cohort,
        );
    }
}

#[test]
fn rejected_persistent_candidate_is_discarded_then_parsed_in_the_same_lane() {
    use std::sync::atomic::Ordering;

    let persistence = Arc::new(CorruptingPersistence::default());
    let (source, grammar) = authorities();
    let first = accepted(&source, &grammar, 1, "<template>candidate</template>");
    let first_store = CarrierPublicationStore::with_dependencies(
        source,
        grammar,
        persistence.clone(),
        Arc::new(crate::types::MetaProvenance::default()),
    );
    assert!(matches!(
        first_store.publish_or_get(&first, request(1, &first)),
        PublicationOutcome::Published(_)
    ));

    persistence.corrupt_next.store(true, Ordering::Release);
    let (source, grammar) = authorities();
    let second = accepted(&source, &grammar, 1, "<template>candidate</template>");
    let second_store = CarrierPublicationStore::with_dependencies(
        source,
        grammar,
        persistence,
        Arc::new(crate::types::MetaProvenance::default()),
    );
    assert!(matches!(
        second_store.publish_or_get(&second, request(2, &second)),
        PublicationOutcome::Published(_)
    ));
    let audit = second_store.audit_snapshot();
    assert_eq!(audit.rejected_candidates, 1);
    assert_eq!(audit.parser_started, 1);
    let kinds: Vec<_> = second_store
        .audit_events()
        .into_iter()
        .map(|event| event.kind)
        .collect();
    assert!(kinds.iter().any(|kind| matches!(
        kind,
        crate::carrier_publication_store::PublicationAuditKind::PersistentAdoptionRejected(
            crate::carrier_publication_store::PersistentAdoptionRejection::ChecksumMismatch
        )
    )));
    assert!(kinds.contains(
        &crate::carrier_publication_store::PublicationAuditKind::PersistentCandidateDiscarded
    ));
}

struct PanickingPersistence;

impl crate::carrier_publication_store::persistence::CarrierPersistence for PanickingPersistence {
    fn take_candidate(
        &self,
        _id: &crate::carrier_publication_store::FrameworkArtifactId,
        _accepted: &verter_language::carrier_grammar::AcceptedRegisteredCarrierSource,
    ) -> Option<crate::carrier_publication_store::persistence::PersistedCarrierCandidate> {
        panic!("injected persistence panic")
    }

    fn store_success(
        &self,
        _id: &crate::carrier_publication_store::FrameworkArtifactId,
        _accepted: &verter_language::carrier_grammar::AcceptedRegisteredCarrierSource,
        _artifact: &Arc<verter_compiler::framework_common::FrameworkParseArtifact>,
        _cohort: crate::carrier_artifact_cohort::PersistedCarrierArtifactCohort,
    ) {
    }
}

#[test]
fn leader_panic_publishes_one_typed_terminal_and_audit_failure() {
    let (source, grammar) = authorities();
    let accepted = accepted(&source, &grammar, 1, "<template>panic</template>");
    let store = CarrierPublicationStore::with_dependencies(
        source,
        grammar,
        Arc::new(PanickingPersistence),
        Arc::new(crate::types::MetaProvenance::default()),
    );
    assert!(matches!(
        store.publish_or_get(&accepted, request(1, &accepted)),
        PublicationOutcome::WinnerPanicked
    ));
    assert!(matches!(
        store.publish_or_get(&accepted, request(2, &accepted)),
        PublicationOutcome::WinnerPanicked
    ));
    let events = store.audit_events();
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event.kind,
                crate::carrier_publication_store::PublicationAuditKind::TerminalFailure
            ))
            .count(),
        1
    );
}

#[test]
fn cancelled_request_never_enters_a_publication_lane() {
    let (source, grammar) = authorities();
    let accepted = accepted(&source, &grammar, 1, "<template>cancel</template>");
    let provenance = Arc::new(crate::types::MetaProvenance::default());
    let store = CarrierPublicationStore::with_dependencies(
        source,
        grammar,
        Arc::new(
            crate::carrier_publication_store::persistence::InMemoryCarrierPersistence::default(),
        ),
        Arc::clone(&provenance),
    );
    let cancellation = verter_scheduler::cancellation::CancellationToken::new();
    cancellation.cancel();
    let outcome = store.publish_or_get(
        &accepted,
        PublicationRequestContext::new(
            AuditRequestId::new(1),
            PublicationSurface::ProjectionHost,
            cancellation,
            accepted.source().snapshot_id().clone(),
        ),
    );
    assert!(matches!(outcome, PublicationOutcome::Cancelled));
    assert_eq!(store.audit_snapshot(), Default::default());
    assert_eq!(
        provenance
            .carrier_parses
            .load(std::sync::atomic::Ordering::Relaxed),
        0,
        "a cancelled request must not parse"
    );
}

#[test]
fn elected_publication_parses_once_and_warm_get_does_not_reparse() {
    let (source, grammar) = authorities();
    let accepted = accepted(&source, &grammar, 1, "<template><p>once</p></template>");
    let provenance = Arc::new(crate::types::MetaProvenance::default());
    let store = CarrierPublicationStore::with_dependencies(
        source,
        grammar,
        Arc::new(
            crate::carrier_publication_store::persistence::InMemoryCarrierPersistence::default(),
        ),
        Arc::clone(&provenance),
    );
    let first = store.publish_or_get(&accepted, request(1, &accepted));
    let second = store.publish_or_get(&accepted, request(2, &accepted));
    assert!(matches!(first, PublicationOutcome::Published(_)));
    assert!(second.into_envelope().is_some());
    assert_eq!(
        provenance
            .carrier_parses
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
    assert_eq!(
        provenance
            .sfc_parses
            .load(std::sync::atomic::Ordering::Relaxed),
        1
    );
}

#[test]
fn registered_structure_seals_local_refs_and_public_token_domains() {
    use verter_language::parse_artifact::carrier_inventory::{
        AttributeId, BlockId, MarkupNodeId, SourceSpaceId,
    };

    let workspace: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::MemoryWorkspace::new(verter_workspace::MemoryOptions::default()),
    );
    let host = crate::VerterHost::new(crate::HostConfig::default(), workspace);
    let canonical = "/sealed-structure.vue";
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: std::sync::Arc::from(
                "<template><button disabled>ok</button></template><style>.x{}</style>",
            ),
            file_language: crate::FileLanguage::vue(),
            aliases: vec![],
        })
        .expect("upsert");

    let (structure, _) = host
        .registered_file_structure_snapshot(canonical)
        .expect("registered structure");
    let block_ref = structure.block_ref(BlockId(0)).expect("block ref");
    let node_ref = structure.node_ref(MarkupNodeId(0)).expect("node ref");
    let attribute_ref = structure
        .attribute_ref(AttributeId(0))
        .expect("attribute ref");

    assert_eq!(block_ref.artifact_id(), structure.artifact_id());
    assert_eq!(block_ref.block_id(), BlockId(0));
    assert_eq!(node_ref.artifact_id(), structure.artifact_id());
    assert_eq!(node_ref.node_id(), MarkupNodeId(0));
    assert_eq!(attribute_ref.artifact_id(), structure.artifact_id());
    assert_eq!(attribute_ref.attribute_id(), AttributeId(0));

    let artifact = structure.public_artifact_token();
    let block = structure
        .public_block_token(&block_ref)
        .expect("block token");
    let node = structure.public_node_token(&node_ref).expect("node token");
    let attribute = structure
        .public_attribute_token(&attribute_ref)
        .expect("attribute token");
    let source_space = structure
        .public_source_space_token(SourceSpaceId(0))
        .expect("source-space token");
    for token in [
        artifact.as_str(),
        block.as_str(),
        node.as_str(),
        attribute.as_str(),
        source_space.as_str(),
    ] {
        assert_eq!(token.len(), 43, "canonical unpadded base64url token");
        assert!(token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'));
    }
    assert_eq!(
        std::collections::HashSet::from([
            artifact.as_str(),
            block.as_str(),
            node.as_str(),
            attribute.as_str(),
            source_space.as_str(),
        ])
        .len(),
        5,
        "token domains must not alias even when local ids are all zero"
    );
}

/// Negative control: a Svelte `parse_reject_facts` duplicate attribute
/// (`blocks_compile: true`) still fails `compile_entry` closed. Only
/// `strict_parse_errors` is advisory.
#[test]
fn preexisting_parse_reject_fact_still_fails_close_ide_compile() {
    use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

    let source =
        "<script>let count = 1;</script>\n<div class=\"a\" class=\"b\"></div>\n<p>{count}</p>\n";
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = crate::VerterHost::new(crate::HostConfig::default(), workspace);
    let _ = host.upsert(crate::UpsertRequest {
        canonical_id: Some("/ProbeDup.svelte".into()),
        input_id: "/ProbeDup.svelte".into(),
        source: Arc::from(source),
        file_language: verter_language::FileLanguage::svelte(),
        aliases: Vec::new(),
    });
    let profile = crate::CompileProfile {
        target: crate::CompileTarget::IDE,
        ..crate::CompileProfile::default()
    };
    assert!(
        host.ensure_ide_compiled("/ProbeDup.svelte", &profile)
            .is_err(),
        "a duplicate-attribute parse_reject_fact must still fail-close IDE compile \
         (unchanged pre-existing behavior, out of this fix's scope)"
    );
}

/// Negative control: Vue parse-time `Error` (`TemplateFunctionalUnsupported`)
/// still fails `compile_entry` closed. Vue's snapshot path uses
/// `from_vec` (severity only) and never reads `blocks_compile`.
#[test]
fn vue_parse_time_error_diagnostic_still_fails_close_ide_compile() {
    let workspace: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::MemoryWorkspace::new(verter_workspace::MemoryOptions::default()),
    );
    let host = crate::VerterHost::new(crate::HostConfig::default(), workspace);
    let source =
        "<template functional><div>{{ x }}</div></template>\n<script setup>const x = 1</script>\n";
    let _ = host.upsert(crate::UpsertRequest {
        canonical_id: Some("/ProbeVue.vue".into()),
        input_id: "/ProbeVue.vue".into(),
        source: Arc::from(source),
        file_language: verter_language::FileLanguage::vue(),
        aliases: Vec::new(),
    });
    let profile = crate::CompileProfile {
        target: crate::CompileTarget::IDE,
        ..crate::CompileProfile::default()
    };
    assert!(
        host.ensure_ide_compiled("/ProbeVue.vue", &profile).is_err(),
        "TemplateFunctionalUnsupported must still fail-close IDE compile \
         (unchanged pre-existing Vue behavior, out of this fix's scope)"
    );
}
