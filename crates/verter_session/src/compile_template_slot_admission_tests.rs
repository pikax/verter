//! The compile-publish raw-template persist honours the slot's
//! admission semantics.
//!
//! The profileless canonical-keyed `derived_raw_cache().raw_template_analysis`
//! slot stores the DEFAULT-extraction template for the canonical's own
//! inline bytes — that is what every reader (`get_analysis` /
//! `get_raw_analysis_snapshot`) serves it as. Two compile shapes
//! therefore must never populate it:
//!
//! - a compile under parse-affecting profile options (`delimiters`,
//!   `custom_elements`): the extraction describes a DIFFERENT parse of
//!   the same bytes, yet the entry would carry a valid current
//!   generation stamp — cache poisoning the rail cannot reject;
//! - an external-src SFC: editing the external dep clears compile
//!   slots, not this slot, and the owner's node generation does not
//!   move, so the rail cannot reject the stale entry.
//!
//! Both rules are the SAME admission the lazy template-analysis writer
//! already enforces — one shared write authority, not a per-site gate.

use std::sync::Arc;

use crate::hash::compile_profile_hash;
use crate::types::{
    CompileCacheMode, CompileProfile, HostConfig, UpsertRequest, VirtualNodeKind, VirtualQuery,
};
use crate::VerterHost;

const OWNER: &str = "/proj/Owner.vue";

/// Inline SFC: one script binding interpolated with the DEFAULT
/// delimiters. Under custom delimiters the `{{ msg }}` text is plain
/// text, so the delimiter extraction carries NO `msg` binding
/// occurrence — the discriminating observable.
const INLINE_SOURCE: &str = "<script setup lang=\"ts\">\nconst msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>";

/// External-src SFC: the template body lives in `partial.html`.
const EXTERNAL_SRC_SOURCE: &str = "<template src=\"./partial.html\"></template>\n<script setup lang=\"ts\">\nconst ok = true\n</script>";

fn make_host() -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.inject_file(
        "/proj/partial.html".to_string(),
        Arc::from("<div>hello</div>"),
    );
    Arc::new(VerterHost::new(HostConfig::default(), workspace))
}

fn upsert(host: &VerterHost, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(OWNER.to_string()),
            input_id: OWNER.to_string(),
            source: Arc::from(source),
            file_language: verter_language::LanguageRegistry::global()
                .classify_static(OWNER)
                .static_resolution(),
            aliases: Vec::new(),
        })
        .expect("upsert must succeed");
}

fn compile_with(
    host: &VerterHost,
    profile: CompileProfile,
) -> Result<crate::types::VirtualFileResponse, crate::types::HostError> {
    host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(OWNER.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile,
    })
}

/// Whether the Session compile slot for `(OWNER, profile)` holds a
/// published entry — proves the compile's OWN admission was
/// `Cacheable`, so a missing template-slot persist is the slot
/// admission acting, not a refused compile.
fn session_slot_present(host: &VerterHost, profile: &CompileProfile) -> bool {
    let profile_hash = compile_profile_hash(profile);
    host.compile_cache()
        .get(OWNER)
        .map(|cc| {
            crate::compile_output_node::CompileOutputNodeFactValidatedSession::new()
                .peek_signature(&cc, profile_hash)
                .is_some()
        })
        .unwrap_or(false)
}

fn template_slot(
    host: &VerterHost,
) -> Option<
    verter_session_query::retention::heap_size::OwnedValue<
        verter_session_query::analysis::template::TemplateAnalysisSnapshot,
    >,
> {
    host.derived_raw_cache().get(OWNER).and_then(|cc| {
        cc.raw_template_analysis()
            .map(|entry| entry.template.clone())
    })
}

/// THE PIN (parse-affecting profile): a Session compile under custom
/// delimiters extracts a DIFFERENT template from the same bytes — it
/// must not populate the profileless default-extraction slot, and the
/// next default-lane read must serve the default extraction.
#[test]
fn non_default_parse_profile_compile_never_populates_the_default_template_slot() {
    let host = make_host();
    upsert(&host, INLINE_SOURCE);

    let profile = CompileProfile {
        delimiters: Some(("[[".to_string(), "]]".to_string())),
        ..CompileProfile::default()
    };
    let error = compile_with(&host, profile.clone())
        .expect_err("grammar mismatch must fail before compilation");
    assert!(matches!(error, crate::types::HostError::GrammarMismatch(_)));
    assert!(!session_slot_present(&host, &profile));
    assert!(template_slot(&host).is_none());
}

/// THE PIN (external src): an external-src SFC compile must not
/// populate the slot — an external dep edit clears compile slots, not
/// this slot, and the owner's node generation does not move, so the
/// rail could never reject the stale entry.
#[test]
fn external_src_compile_uses_the_session_slot_not_the_template_slot() {
    let host = make_host();
    upsert(&host, EXTERNAL_SRC_SOURCE);
    host.set_exact_resolutions(
        OWNER,
        vec![verter_workspace::ExactResolution {
            specifier: "./partial.html".to_string(),
            phase: verter_session_query::resolution::ResolvePhase::CodegenBlocker,
            kind: verter_session_query::resolution::ResolveRequestKind::EsmImport,
            resolved_canonical_id: Some("/proj/partial.html".to_string()),
            possible_canonical_ids: vec!["/proj/partial.html".to_string()],
        }],
    );

    let profile = CompileProfile::default();
    let output = compile_with(&host, profile.clone()).expect("external content lowering");
    assert!(output.code.contains("hello"));
    assert!(session_slot_present(&host, &profile));
    assert!(template_slot(&host).is_none());
}

/// No-over-decline negative control: a default-profile inline compile
/// still persists the slot, and the default lane warm-serves the SAME
/// Arc — the admission keys on parse-affecting options and src blocks,
/// not on declining every compile-lane persist.
#[test]
fn default_profile_inline_compile_still_persists_and_warm_serves() {
    let host = make_host();
    upsert(&host, INLINE_SOURCE);

    let profile = CompileProfile::default();
    let compiled = compile_with(&host, profile.clone()).expect("compile must serve");
    assert_eq!(compiled.actual_mode, CompileCacheMode::Session);
    assert!(
        session_slot_present(&host, &profile),
        "the default compile must publish its session slot",
    );

    let persisted =
        template_slot(&host).expect("a default-profile inline compile persists the template slot");
    let snapshot = host
        .get_raw_analysis_snapshot(OWNER)
        .expect("the warm read must serve");
    let served = snapshot
        .template
        .as_ref()
        .expect("the warm read carries a template");
    assert!(
        verter_session_query::retention::heap_size::OwnedValue::ptr_eq(served, &persisted),
        "the default lane warm-serves the compile-persisted value — the \
         admission must not decline the coherent default-extraction persist",
    );
    assert!(
        persisted
            .binding_occurrences
            .iter()
            .any(|occ| occ.name == "msg"),
        "the persisted default extraction carries the msg interpolation occurrence",
    );
}

#[test]
fn page_retention_refusal_evicts_compile_companions_and_serves_uncached() {
    use verter_session_query::facts::fact_cache::{FactVersionRef, ReadSetSignature};
    use verter_session_query::retention::{
        RetentionLimits, SemanticRetentionAccount, StoreAccount,
    };
    let profile = CompileProfile::default();
    for refused in [false, true] {
        let host = make_host();
        upsert(&host, INLINE_SOURCE);
        let facts: Vec<_> = (0..2 * verter_session_query::facts::fact_read_set::FACT_PAGE_WIDTH
            + 3)
            .map(|i| {
                let canonical_id = format!("/publish-evidence/{i}.ts");
                let _ = host
                    .upsert(UpsertRequest {
                        canonical_id: Some(canonical_id.clone()),
                        input_id: canonical_id.clone(),
                        source: Arc::from("export const value = 1;"),
                        file_language: verter_language::FileLanguage::script_ts(),
                        aliases: Vec::new(),
                    })
                    .unwrap();
                let source = host.scheduler.try_get_source(&canonical_id).unwrap();
                let hash = source
                    .downcast_data::<crate::host_executor::HostSourceData>()
                    .unwrap()
                    .parse
                    .whole_hash;
                FactVersionRef::FileWholeHash { canonical_id, hash }
            })
            .collect();
        let first = compile_with(&host, profile.clone()).unwrap();
        assert!(template_slot(&host).is_some());
        let profile_hash = compile_profile_hash(&profile);
        assert!(host
            .scheduler
            .try_get_artifact(OWNER, profile_hash)
            .is_some());
        // Force the same production route cold, leaving both companions as
        // witnesses that refusal must evict them rather than merely skip writes.
        {
            let mut state = host.compile_cache().get_mut(OWNER).unwrap();
            crate::compile_output_node::CompileOutputNodeFactValidatedSession::new()
                .remove(&mut state, profile_hash);
        }
        let account = SemanticRetentionAccount::new(RetentionLimits {
            max_entry_bytes: if refused { 1 } else { usize::MAX },
            ..RetentionLimits::defaults()
        });
        let read = || {
            let signature = ReadSetSignature::new(
                verter_session_query::facts::fact_read_set::seal_canonical_signature(facts.clone()),
            );
            crate::compile_output_node::with_session_publish_evidence_for_test(
                signature,
                StoreAccount::new(Arc::clone(&account)),
                || compile_with(&host, profile.clone()).unwrap(),
            )
        };
        assert_eq!(
            read().code,
            first.code,
            "retention never changes the product"
        );
        assert_eq!(session_slot_present(&host, &profile), !refused);
        assert_eq!(template_slot(&host).is_some(), !refused);
        assert_eq!(
            host.scheduler
                .try_get_artifact(OWNER, profile_hash)
                .is_some(),
            !refused
        );
        if refused {
            assert_eq!(read().code, first.code, "repeat delivers uncached output");
            assert!(!session_slot_present(&host, &profile));
            assert!(template_slot(&host).is_none());
            assert!(host
                .scheduler
                .try_get_artifact(OWNER, profile_hash)
                .is_none());
        } else {
            let signature = host
                .compile_cache()
                .get(OWNER)
                .and_then(|state| {
                    crate::compile_output_node::CompileOutputNodeFactValidatedSession::new()
                        .peek_signature(&state, profile_hash)
                })
                .unwrap();
            let view = host.resolver_store_view_read().into_owned_view();
            assert!(
                verter_session_query::facts::store_view::StoreView::validates_fact_signature(
                    &view,
                    &signature.facts
                ),
                "the admitted wide witness is current"
            );
            assert_eq!(
                compile_with(&host, profile.clone()).unwrap().code,
                first.code,
                "valid admitted warm output equals the cold product"
            );
        }
        drop(host);
        assert_eq!(
            account.snapshot().retained_bytes,
            0,
            "dropping companions drains pages"
        );
    }
}

/// An IDE artifact the retention account declines to keep is still served by
/// the compile that produced it: only the cache slot is missing, so a
/// consumer that takes the served artifact never loses the IDE surface of a
/// component whose output exceeds the per-entry ceiling.
#[test]
fn refused_ide_admission_still_serves_the_compiled_artifact() {
    use verter_session_query::facts::fact_cache::ReadSetSignature;
    use verter_session_query::retention::{
        RetentionLimits, SemanticRetentionAccount, StoreAccount,
    };
    let profile = CompileProfile::default();
    let mut admitted_code = None;
    for refused in [false, true] {
        let host = make_host();
        upsert(&host, INLINE_SOURCE);
        let account = SemanticRetentionAccount::new(RetentionLimits {
            max_entry_bytes: if refused { 1 } else { usize::MAX },
            ..RetentionLimits::defaults()
        });
        let ide = crate::compile_output_node::with_session_publish_evidence_for_test(
            ReadSetSignature::new(
                verter_session_query::facts::fact_read_set::seal_canonical_signature(Vec::new()),
            ),
            StoreAccount::new(Arc::clone(&account)),
            || host.compile_ide(OWNER, &profile),
        )
        .expect("the carrier compiles")
        .expect("a .vue carrier projects an IDE surface");
        assert!(
            ide.code.contains("msg"),
            "the served artifact is the compiled IDE surface"
        );
        assert_eq!(
            session_slot_present(&host, &ide_profile(&profile)),
            !refused,
            "only an admitted artifact is cached"
        );
        match &admitted_code {
            None => admitted_code = Some(ide.code),
            Some(code) => assert_eq!(&*ide.code, &**code, "retention never changes the artifact"),
        }
        drop(host);
        assert_eq!(account.snapshot().retained_bytes, 0);
    }
}

/// An IDE output the account refuses to retain is still delivered: its
/// compile hands it to the document as the surface the pure IDE read serves
/// — complete, byte-equal to the retained output — charged to its readers,
/// pinned, and never reused as a cache hit. The next compile the account
/// retains supersedes it and gives the surface's charge back. Without this an
/// oversized IDE surface was compiled and then lost, and the editor saw no
/// provider surface at all.
#[test]
fn a_refused_ide_output_is_still_delivered_complete_to_the_reader() {
    use crate::types::CompileTarget;
    use verter_session_query::facts::fact_cache::ReadSetSignature;
    use verter_session_query::retention::{
        ChargeClass, RetentionLimits, SemanticRetentionAccount, StoreAccount,
    };
    let host = make_host();
    upsert(&host, INLINE_SOURCE);
    let profile = CompileProfile {
        source_map: true,
        target: CompileTarget::IDE,
        ..CompileProfile::default()
    };
    let remove_slot = || {
        let mut state = host.compile_cache().get_mut(OWNER).unwrap();
        crate::compile_output_node::CompileOutputNodeFactValidatedSession::new()
            .remove(&mut state, compile_profile_hash(&ide_profile(&profile)));
    };
    assert!(host.ensure_ide_compiled(OWNER, &profile).unwrap());
    let retained = host
        .get_ide(OWNER, &profile)
        .expect("a retained IDE output is read");
    remove_slot();

    let account = SemanticRetentionAccount::new(RetentionLimits {
        max_entry_bytes: 1,
        ..RetentionLimits::defaults()
    });
    let delivered = crate::compile_output_node::with_session_publish_evidence_for_test(
        ReadSetSignature::new(
            verter_session_query::facts::fact_read_set::seal_canonical_signature(Vec::new()),
        ),
        StoreAccount::new(Arc::clone(&account)),
        || {
            assert!(host.ensure_ide_compiled(OWNER, &profile).unwrap());
            host.get_ide(OWNER, &profile)
        },
    )
    .expect("a refused IDE output is still delivered");
    assert!(
        !session_slot_present(&host, &ide_profile(&profile)),
        "nothing is retained"
    );
    assert_eq!(&*delivered.code, &*retained.code, "the output is complete");
    assert_eq!(
        delivered.source_map.as_deref(),
        retained.source_map.as_deref()
    );
    assert_eq!(delivered.code.charge_class(), Some(ChargeClass::Pinned));
    let again = host
        .get_ide(OWNER, &profile)
        .expect("the surface is the document's");
    assert!(
        verter_type_expr::OwnedStr::ptr_eq(&again.code, &delivered.code),
        "every read serves the one delivered surface"
    );
    let texts = delivered.code.charge_bytes().unwrap()
        + delivered
            .source_map
            .as_ref()
            .and_then(|map| map.charge_bytes())
            .unwrap_or(0);
    let snapshot = account.snapshot();
    assert_eq!(snapshot.retained_bytes, 0, "no cache retains it");
    assert_eq!(
        snapshot.pinned_bytes, texts,
        "its texts are charged to the readers"
    );
    drop((delivered, again));
    assert_eq!(
        account.snapshot().pinned_bytes,
        texts,
        "the document keeps its surface after the readers let go"
    );

    assert!(host.ensure_ide_compiled(OWNER, &profile).unwrap());
    assert!(
        session_slot_present(&host, &ide_profile(&profile)),
        "an unrestricted compile is retained"
    );
    assert_eq!(
        account.snapshot().pinned_bytes,
        0,
        "a retained slot supersedes the delivered surface and its charge goes"
    );
}

/// A compile whose only non-cacheable reads are import resolutions the
/// retention account declined to keep is the complete answer, uncached: the
/// document still holds its IDE surface, as for any other refused retention.
/// With the account admitting, the same compile is retained.
///
/// Mutation recipe: replay the prefetch's unretained resolution as a fenced
/// serve, or admit any non-cacheable compile as superseded. The compile is
/// then forgotten and the pure IDE read misses.
#[test]
fn a_compile_over_unretained_resolutions_still_delivers_its_ide_surface() {
    use crate::types::CompileTarget;
    struct RefusingResolutionAccount;
    impl verter_session_query::retention::resolution_charge::ResolutionRetentionAccount
        for RefusingResolutionAccount
    {
        fn reserve_retained(
            &self,
            _bytes: usize,
        ) -> Option<verter_session_query::retention::resolution_charge::ResolutionRetentionCharge>
        {
            None
        }
    }
    const IMPORTING_SOURCE: &str = "<script setup lang=\"ts\">\nimport type { Msg } from './dep'\nconst msg: Msg = 'hi'\n</script>\n<template><div>{{ msg }}</div></template>";
    let profile = CompileProfile {
        target: CompileTarget::IDE,
        ..CompileProfile::default()
    };
    let mut codes = Vec::new();
    for refused in [false, true] {
        let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        workspace.inject_file(
            "/proj/dep.ts".to_string(),
            Arc::from("export type Msg = string\n"),
        );
        let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
        if refused {
            host.ws()
                .install_resolution_retention(Arc::new(RefusingResolutionAccount));
        }
        upsert(&host, IMPORTING_SOURCE);
        let publication = host.resolve_type_dependency_canonical(OWNER, "./dep");
        assert_eq!(
            publication.delivered().flatten().map(String::as_str),
            Some("/proj/dep.ts"),
            "premise: the import resolves, got {publication:?}"
        );
        assert_eq!(
            matches!(
                publication,
                verter_workspace::ResolutionPublication::Unretained(_)
            ),
            refused,
            "premise: only the refusing account declines the resolution"
        );
        assert!(host.ensure_ide_compiled(OWNER, &profile).unwrap());
        assert_eq!(
            session_slot_present(&host, &ide_profile(&profile)),
            !refused,
            "only a compile over retained resolutions is cached"
        );
        let ide = host
            .get_ide(OWNER, &profile)
            .expect("the IDE surface is served with or without retention");
        codes.push(ide.code.to_string());
    }
    assert_eq!(
        codes[0], codes[1],
        "retention never changes the IDE surface"
    );
}

/// The IDE-normalized profile `ensure_ide_compiled` / `get_ide` address.
fn ide_profile(profile: &CompileProfile) -> CompileProfile {
    VerterHost::ide_normalized_profile(profile)
}

#[test]
fn stateless_generated_text_is_pinned_through_its_final_leaf() {
    use verter_session_query::retention::{ChargeClass, SemanticRetentionAccount};
    let host = make_host();
    upsert(&host, INLINE_SOURCE);
    let response = compile_with(
        &host,
        CompileProfile {
            requested_mode: CompileCacheMode::Stateless,
            source_map: true,
            ..CompileProfile::default()
        },
    )
    .expect("stateless compile produces complete output");
    assert_eq!(response.actual_mode, CompileCacheMode::Stateless);
    assert_eq!(response.code.charge_class(), Some(ChargeClass::Pinned));
    let map = response
        .source_map
        .clone()
        .expect("source maps were requested");
    assert_eq!(map.charge_class(), Some(ChargeClass::Pinned));
    let code = response.code.clone();
    let bytes = verter_type_expr::OwnedStr::charge_for_len(code.len());
    assert_eq!(code.charge_bytes(), Some(bytes));
    drop((response, host, map));
    assert_eq!(
        code.charge_bytes(),
        Some(bytes),
        "leaf outlives host and response"
    );
    let account = SemanticRetentionAccount::process_local();
    let before = account.snapshot().pinned_bytes;
    drop(code);
    assert_eq!(account.snapshot().pinned_bytes, before - bytes);
}
