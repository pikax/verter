//! Hover-provenance enrichment for LSP hover responses.
//!
//! Hosts `enrich_hover_with_provenance` — which appends a cached
//! provenance section to a hover body, spawning a background blocking
//! task to populate the enrichment cache on a miss — and its private
//! `append_markdown` hover-suffix helper.
//!
//! Extracted from `nav_features` to keep that source under the
//! file-size guard (`no_oversize_files`); `handle_hover` calls
//! `enrich_hover_with_provenance` through this sibling module.

use std::sync::Arc;

use tower_lsp_server::ls_types::*;

use super::VerterLanguageServer;

/// Post-process a hover response with provenance enrichment:
/// - If `hover.provenance` is disabled → return hover unchanged.
/// - If the enrichment cache has a payload for `(canonical_id,
///   position)` → append it to the hover body.
/// - On cache miss → spawn a background blocking task to compute
///   the payload via `VerterHost::get_component_meta_with_resolution`
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
    if !server.hover_provenance_enabled.load(Ordering::Relaxed) {
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
    // `footprint_capture` are off, `get_component_meta_with_resolution`
    // would run to completion but `take_audit_record` would
    // return None and the cache would stay empty. That means
    // every subsequent hover would spawn another futile task.
    // Short-circuit here so the user sees the legacy hover and
    // no blocking-pool slots are burned on a capture-disabled
    // host.
    let host = server.documents.host_arc();
    if !host.config().audit_enabled || !host.config().footprint_capture {
        return hover;
    }

    // Cache miss + capture enabled — return the legacy payload
    // immediately and spawn a background AuditedRequest to populate
    // the cache for the next hover.
    let cache = Arc::clone(&server.hover_provenance_cache);
    let canonical_for_task = canonical_id;
    tokio::task::spawn_blocking(move || {
        let Some((_analysis, resolution)) =
            host.get_component_meta_with_resolution(&canonical_for_task)
        else {
            return;
        };
        let Some(record) = host.take_audit_record(resolution.request_id) else {
            return;
        };
        let markdown = crate::features::hover_provenance::render_provenance_markdown(&record);
        cache.insert(
            key,
            crate::features::hover_provenance::HoverProvenancePayload { markdown },
        );
    });

    hover
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
