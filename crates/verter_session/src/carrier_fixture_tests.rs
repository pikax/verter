#![cfg(test)]

use std::sync::Arc;

pub(crate) fn publish_carrier_fixture(
    canonical_id: &str,
    source: &str,
    file_language: &verter_language::FileLanguage,
    provenance: &crate::types::MetaProvenance,
) -> Option<Arc<verter_language::FrameworkParseArtifact>> {
    use verter_language::carrier_grammar::{
        CarrierGrammarAuthority, CarrierGrammarConfig, CarrierParserGrammarVersion,
        FrameworkAdapterSemanticVersion,
    };
    use verter_language::registered_source_authority::{
        CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
    };
    let source_authority = Arc::new(RegisteredSourceAuthority::new().ok()?);
    let grammar_authority = Arc::new(CarrierGrammarAuthority::new().ok()?);
    let config = if file_language.adapter_id().is_some_and(|id| id.is_vue()) {
        CarrierGrammarConfig::vue("{{", "}}", std::iter::empty::<&str>()).ok()?
    } else {
        CarrierGrammarConfig::Svelte
    };
    grammar_authority
        .register_carrier_grammar(
            file_language.clone(),
            FrameworkAdapterSemanticVersion::new(1)?,
            CarrierParserGrammarVersion::new(1)?,
            config.clone(),
        )
        .ok()?;
    let snapshot = source_authority
        .register_source(
            CanonicalFileId::new(canonical_id),
            FileIncarnation::new(1),
            SourceGeneration::new(1),
            file_language.clone(),
            Arc::from(source),
        )
        .ok()?;
    let accepted = grammar_authority
        .accept_registered_source(&source_authority, &snapshot, &config)
        .ok()?;
    let store = crate::carrier_publication_store::CarrierPublicationStore::new(
        source_authority,
        grammar_authority,
    );
    let envelope = store
        .publish_or_get(
            &accepted,
            crate::carrier_publication_store::PublicationRequestContext::new(
                crate::carrier_publication_store::AuditRequestId::new(1),
                crate::carrier_publication_store::PublicationSurface::ProjectionHost,
                verter_scheduler::cancellation::CancellationToken::new(),
                snapshot.snapshot_id().clone(),
            ),
        )
        .into_envelope()?;
    use std::sync::atomic::Ordering::Relaxed;
    provenance.carrier_parses.fetch_add(1, Relaxed);
    if file_language.adapter_id().is_some_and(|id| id.is_vue()) {
        provenance.sfc_parses.fetch_add(1, Relaxed);
    }
    Some(Arc::clone(envelope.artifact()))
}
