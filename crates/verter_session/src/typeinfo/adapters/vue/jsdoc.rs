#![deny(missing_docs)]
//! Span-based JSDoc slicing for the `.vue` macro-surface normalizers.
//!
//! The typeinfo surface carries JSDoc as SPANS (description span + tag spans)
//! located on the surface members / signatures; these helpers slice the
//! declaring file's cache-owned raw source at those spans and normalize the
//! comment decoration into the published `(description, tags)` display pair.
//! Slicing only — no re-location, no reparse, no lazy name-search path.

use std::sync::Arc;

use verter_semantic::analysis::types::JsdocTag;

use crate::typeinfo::surface::{CanonicalSpan, TypeInfoSurfaceMember};
use crate::VerterHost;

/// Slice a member's leading-JSDoc DESCRIPTION + TAG spans into owned text for
/// the published DTO. The spans are already located on the surface (by
/// `with_member_jsdoc_spans`); this reads the declaring file's cache-owned
/// source and slices — it does NOT re-locate the comment block and does NOT
/// take the lazy `member_display_jsdoc` name-search path.
///
/// Returns `(None, empty)` when the member carries no JSDoc spans or the
/// declaring file's source is unavailable.
/// Slice a [`CanonicalSpan`]'s byte range out of its file's cache-owned RAW
/// source (`IndexedReady.raw_source`). [`CanonicalSpan`] offsets are
/// SFC-absolute (the eval source is position-preserving, so OXC stamps spans
/// in raw-file coordinates), so the slice indexes the raw source directly.
/// `None` when the file is not loaded or the byte range is out of bounds (a
/// stale / synthetic span). This is the single source-slicing primitive the
/// normalizers use to materialize display text from a span at the consumer
/// boundary — it does NOT re-resolve or re-parse.
pub(super) fn slice_canonical_span(host: &VerterHost, cspan: &CanonicalSpan) -> Option<String> {
    let indexed = host
        .ensure_indexed_ready_serve(cspan.file.as_ref())?
        .indexed;
    let source = Arc::clone(&indexed.raw_source);
    let start = cspan.span.start as usize;
    let end = cspan.span.end as usize;
    source.get(start..end).map(|s| s.to_string())
}

/// Normalize a multi-line JSDoc description/tag body sliced from a span.
///
/// A description/tag span is a contiguous `[start, end)` region whose FIRST line
/// already had its leading `/**`-decoration stripped (the span starts at the
/// content), but whose CONTINUATION lines still carry the `   * ` JSDoc
/// decoration verbatim (the span is contiguous source text). The published
/// `description` is DISPLAY text, not comment syntax, so strip each
/// continuation line's leading whitespace + optional single `*` decoration —
/// matching `verter_semantic::analysis::jsdoc`'s per-line stripping — and rejoin
/// with `\n`. A single-line body is returned trimmed.
pub(crate) fn normalize_jsdoc_body(raw: &str) -> String {
    let mut lines = raw.lines();
    let mut out = String::new();
    if let Some(first) = lines.next() {
        out.push_str(first.trim_end());
    }
    for line in lines {
        out.push('\n');
        // Strip leading whitespace, then a single `*` decoration, then the
        // whitespace after it.
        let trimmed = line.trim_start();
        let stripped = trimmed
            .strip_prefix('*')
            .map(|rest| rest.trim_start())
            .unwrap_or(trimmed);
        out.push_str(stripped.trim_end());
    }
    out.trim().to_string()
}

/// Slice a leading-JSDoc description span + tag spans into the published
/// `(description, tags)` display pair. Shared by the member path
/// ([`member_jsdoc_from_spans`]) and the call-signature emit path
/// ([`signature_jsdoc_from_spans`]) — both anchor JSDoc on the typeinfo
/// surface's spans, never a reparse.
fn jsdoc_from_spans(
    host: &VerterHost,
    description_span: Option<&CanonicalSpan>,
    tag_spans: &[crate::typeinfo::surface::JsdocTagSpan],
) -> (Option<String>, Vec<JsdocTag>) {
    let slice = |cspan: &CanonicalSpan| -> Option<String> { slice_canonical_span(host, cspan) };

    let description = description_span
        .and_then(&slice)
        .map(|text| normalize_jsdoc_body(&text))
        .filter(|text| !text.is_empty());

    let tags: Vec<JsdocTag> = tag_spans
        .iter()
        .filter_map(|tag| {
            let name = slice(&tag.name_span)?.trim().to_string();
            if name.is_empty() {
                return None;
            }
            let text = tag
                .text_span
                .as_ref()
                .and_then(&slice)
                .map(|t| normalize_jsdoc_body(&t))
                .filter(|t| !t.is_empty());
            Some(JsdocTag { name, text })
        })
        .collect();

    (description, tags)
}

/// Slice a surface member's leading-JSDoc spans into the published
/// `(description, tags)` display pair.
pub(super) fn member_jsdoc_from_spans(
    host: &VerterHost,
    member: &TypeInfoSurfaceMember,
) -> (Option<String>, Vec<JsdocTag>) {
    jsdoc_from_spans(
        host,
        member.jsdoc_description_span.as_ref(),
        &member.jsdoc_tag_spans,
    )
}

/// Slice a call/construct signature's leading-JSDoc into `(description, tags)`.
/// A call-signature emit (`(e: 'change', v: T): void`) documents the event via
/// the JSDoc on the signature itself — extracted here from the signature's
/// typeinfo JSDoc spans (symmetric with [`member_jsdoc_from_spans`]).
pub(super) fn signature_jsdoc_from_spans(
    host: &VerterHost,
    sig: &crate::typeinfo::surface::TypeInfoSurfaceSignature,
) -> (Option<String>, Vec<JsdocTag>) {
    jsdoc_from_spans(
        host,
        sig.jsdoc_description_span.as_ref(),
        &sig.jsdoc_tag_spans,
    )
}
