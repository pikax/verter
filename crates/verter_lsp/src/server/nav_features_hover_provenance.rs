//! Hover-provenance enrichment for LSP hover responses.
//!
//! Hosts `enrich_hover_with_provenance` — which appends a cached
//! provenance section to a hover body, spawning a background blocking
//! task to populate the enrichment cache on a miss — and its private
//! `append_markdown` hover-suffix helper.
//!
//! `handle_hover` calls `enrich_hover_with_provenance` through this sibling
//! module; it lives apart from `nav_features` so both modules remain a readable
//! size.

#[cfg(feature = "semantic-observe")]
use std::sync::Arc;

use tower_lsp_server::ls_types::*;

use super::VerterLanguageServer;

/// Post-process a hover response with provenance enrichment:
/// - If `hover.provenance` is disabled → return hover unchanged.
/// - If the enrichment cache has a payload for `(canonical_id,
///   position)` → append it to the hover body.
/// - On cache miss → spawn a background blocking task to compute
///   the payload via `VerterHost::get_component_meta_with_audit`
///   and insert it into the cache. The current hover call returns
///   the legacy payload immediately.
///
/// The host must have `audit_enabled + footprint_capture` set for
/// the background task to produce a useful payload; otherwise the
/// task returns without populating the cache (graceful degradation).
pub(super) fn enrich_hover_with_provenance(
    server: &VerterLanguageServer,
    uri: &Uri,
    position: &Position,
    hover: Option<Hover>,
) -> Option<Hover> {
    use std::sync::atomic::Ordering;
    // The supplement is its own labelled inspection cell: a withdrawn
    // `hover.inspection` neither computes nor shows it.
    if !server.hover_provenance_enabled.load(Ordering::Relaxed)
        || !crate::participation::admits(
            &server.documents,
            verter_language::participation::CapabilityOperation::HoverInspection,
            uri,
            verter_language::participation::DemandPurpose::Interactive,
        )
    {
        return hover;
    }
    let Some(canonical_id) = server.documents.get_canonical_id(uri) else {
        return hover;
    };
    let key =
        crate::features::hover_provenance::HoverProvenanceKey::new(canonical_id.clone(), *position);

    if let Some(payload) = server.hover_provenance_cache.get(&key) {
        return Some(append_markdown(hover, &payload.markdown));
    }

    // Cache miss — check whether the host can actually produce a
    // useful payload BEFORE spawning. If `audit_enabled` or
    // `footprint_capture` are off, the audited entry would return no
    // captured record and the cache would stay empty. That means
    // every subsequent hover would spawn another futile task.
    // Short-circuit here so the user sees the legacy hover and
    // no blocking-pool slots are burned on a capture-disabled
    // host.
    // A server built without `semantic-observe` has no audit records to
    // read: the hover stays the legacy payload.
    #[cfg(feature = "semantic-observe")]
    {
        let host = server.documents.host_arc();
        if !host.as_ref().config().audit_enabled || !host.as_ref().config().footprint_capture {
            return hover;
        }

        // Cache miss + capture enabled — return the legacy payload
        // immediately and spawn a background AuditedRequest to populate
        // the cache for the next hover.
        let cache = Arc::clone(&server.hover_provenance_cache);
        let canonical_for_task = canonical_id;
        let participation = Arc::clone(server.documents.participation());
        let admitted_fence = participation.fence();
        tokio::task::spawn_blocking(move || {
            let Some(payload) = provenance_payload(host.as_ref(), &canonical_for_task) else {
                return;
            };
            // Work admitted under a superseded demand epoch is never warmed.
            if participation.fence() != admitted_fence {
                return;
            }
            cache.insert(key, payload);
        });
    }
    #[cfg(not(feature = "semantic-observe"))]
    let _ = (key, canonical_id);

    hover
}

#[cfg(feature = "semantic-observe")]
fn provenance_payload(
    host: &verter_session::VerterHost,
    canonical: &str,
) -> Option<crate::features::hover_provenance::HoverProvenancePayload> {
    let (Some(_analysis), Some(record)) = host.get_component_meta_with_audit(canonical) else {
        return None;
    };
    Some(crate::features::hover_provenance::HoverProvenancePayload {
        markdown: crate::features::hover_provenance::render_provenance_markdown(&record),
    })
}

// Reads optional audit capture: runs in the instrumentation lane.
#[cfg(all(test, feature = "semantic-observe"))]
mod tests {
    use super::*;

    #[test]
    fn hover_provenance_renders_its_own_request_record() {
        let host = verter_session::VerterHost::new_standalone(verter_session::HostConfig {
            audit_enabled: true,
            footprint_capture: true,
            ..Default::default()
        });
        let canonical = "/workspace/Flag.vue";
        let _ = host
            .upsert(verter_session::UpsertRequest {
                canonical_id: Some(canonical.into()),
                input_id: canonical.into(),
                source: Arc::from(
                    "<script setup lang=\"ts\">defineProps<{ on: boolean }>()</script>",
                ),
                file_language: host.language_classifier().classify(canonical),
                aliases: vec![],
            })
            .unwrap();
        let (_, stale) = host.get_component_meta_with_resolution(canonical).unwrap();
        let payload = provenance_payload(&host, canonical).expect("captured hover request");
        let rendered_id = payload
            .markdown
            .split("request_id = ")
            .nth(1)
            .unwrap()
            .split('`')
            .next()
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert!(rendered_id > stale.request_id);
        assert!(!payload
            .markdown
            .contains(&format!("request_id = {}`", stale.request_id)));
        assert!(host
            .host_audit_runtime()
            .take_record(stale.request_id)
            .is_some());
        assert!(host.host_audit_runtime().take_record(rendered_id).is_none());
    }
}

/// Append a markdown suffix to a hover body. Used by the hover
/// provenance enrichment to tack on the "Provenance" section below
/// the legacy hover content. Returns a constructed hover even if the
/// input was `None` (the enrichment alone counts as useful output).
fn append_markdown(hover: Option<Hover>, suffix: &str) -> Hover {
    match hover {
        Some(mut h) => {
            let combined = match h.contents {
                HoverContents::Markup(existing) => {
                    let mut value = existing.value;
                    value.push_str(suffix);
                    HoverContents::Markup(MarkupContent {
                        kind: existing.kind,
                        value,
                    })
                }
                HoverContents::Scalar(marked) => {
                    let value = match marked {
                        MarkedString::String(s) => s,
                        MarkedString::LanguageString(ls) => ls.value,
                    };
                    HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: format!("{value}{suffix}"),
                    })
                }
                HoverContents::Array(items) => {
                    let mut combined = String::new();
                    for item in items {
                        let part = match item {
                            MarkedString::String(s) => s,
                            MarkedString::LanguageString(ls) => ls.value,
                        };
                        if !combined.is_empty() {
                            combined.push_str("\n\n");
                        }
                        combined.push_str(&part);
                    }
                    combined.push_str(suffix);
                    HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: combined,
                    })
                }
            };
            h.contents = combined;
            h
        }
        None => Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: suffix.to_string(),
            }),
            range: None,
        },
    }
}
