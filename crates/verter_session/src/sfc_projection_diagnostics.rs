//! Attribution of engine diagnostics to authored source.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_compiler::code_transform::{MappingSpan, ProjectedClass};
use verter_compiler::framework_common::vue_projection_backend::{ObligationFamily, UseHost};
use verter_identity::encoding::CanonicalEncoder;

use crate::content_mapper::{CarrierAnswer, ContentMapper, Refusal};
use crate::framework::descriptor::classify_carrier_companion;
use crate::sfc_projection_mapper::MapperTransformResult;

/// A diagnostic as the engine reported it, positions in the virtual file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineDiagnostic {
    pub file: Arc<str>,
    /// UTF-8 byte offset.
    pub start: u32,
    pub length: u32,
    pub code: u32,
    pub category: DiagnosticCategory,
    pub message: String,
    pub related: Vec<EngineRelated>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticCategory {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineRelated {
    pub file: Arc<str>,
    pub start: u32,
    pub length: u32,
    pub message: String,
}

/// Where an attributed diagnostic points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticAnchor {
    /// Authored bytes of the file.
    Authored { start: u32, end: u32 },
    /// No authored preimage. Reported against the carrier with the generated
    /// snippet in the message. Never dropped, never given a made-up position.
    Generated { snippet: String },
}

/// One diagnostic attributed to its obligation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionDiagnosticAttribution {
    /// The carrier or authored file path, never a generated virtual file.
    /// Empty for an unlocated engine diagnostic.
    pub carrier: Arc<str>,
    pub anchor: DiagnosticAnchor,
    pub family: Option<ObligationFamily>,
    pub host: Option<UseHost>,
    pub code: u32,
    pub category: DiagnosticCategory,
    /// The engine's message. Only carrier virtual paths are rewritten, to the
    /// carrier path.
    pub message: String,
    pub related: Vec<RelatedSourceInformation>,
    pub dedup: DiagnosticDeduplicationKey,
}

/// Related information mapped through the target file's own result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelatedSourceInformation {
    pub file: Arc<str>,
    pub anchor: DiagnosticAnchor,
    pub message: String,
}

/// Two diagnostics are one only when all fields agree.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiagnosticDeduplicationKey {
    pub carrier: Arc<str>,
    pub start: u32,
    pub end: u32,
    pub code: u32,
    pub message_digest: [u8; 16],
}

/// Looks up the transform result of a virtual file.
pub trait TransformLookup {
    fn result_for_virtual(&self, virtual_file: &str) -> Option<Arc<MapperTransformResult>>;

    /// Explicit source provenance takes precedence over companion naming.
    /// Return true for real authored files, including a real `Foo.vue.ts`.
    /// The default retains companion attribution for existing mapper callers.
    fn is_authored(&self, _file: &str) -> bool {
        false
    }
}

/// Attribute the diagnostics of one project. A diagnostic in a file that is
/// not a carrier passes through as `Authored` in its own file. A diagnostic
/// in a replayed copy that repeats one at the mapped original collapses into
/// it. Nothing else is merged. The caller keeps the raw engine output beside
/// this.
pub fn attribute_diagnostics(
    raw: &[EngineDiagnostic],
    lookup: &dyn TransformLookup,
) -> Vec<ProjectionDiagnosticAttribution> {
    let mut attributed: Vec<ProjectionDiagnosticAttribution> = Vec::with_capacity(raw.len());
    let mut seen: FxHashMap<(DiagnosticDeduplicationKey, Option<Arc<str>>), Vec<usize>> =
        FxHashMap::default();
    // The lookup can be host-backed. Pin each target once for the whole batch,
    // including related information, so every position sees the same result.
    let mut results = FxHashMap::default();
    for diagnostic in raw {
        let result = results
            .entry(diagnostic.file.clone())
            .or_insert_with(|| {
                (!lookup.is_authored(&diagnostic.file))
                    .then(|| lookup.result_for_virtual(&diagnostic.file))
                    .flatten()
            })
            .clone();
        let (carrier, anchor) = locate(
            &diagnostic.file,
            diagnostic.start,
            diagnostic.length,
            result.as_deref(),
            lookup.is_authored(&diagnostic.file),
        );
        let obligation = diagnostic
            .start
            .checked_add(diagnostic.length)
            .and_then(|end| {
                result
                    .as_ref()?
                    .module
                    .as_ref()?
                    .obligation_for_span(MappingSpan {
                        start: diagnostic.start,
                        end,
                    })
            });
        let message = rewrite_companion_paths(&diagnostic.message, lookup);
        let related = diagnostic
            .related
            .iter()
            .map(|related| {
                let result = results.entry(related.file.clone()).or_insert_with(|| {
                    (!lookup.is_authored(&related.file))
                        .then(|| lookup.result_for_virtual(&related.file))
                        .flatten()
                });
                let (file, anchor) = locate(
                    &related.file,
                    related.start,
                    related.length,
                    result.as_deref(),
                    lookup.is_authored(&related.file),
                );
                RelatedSourceInformation {
                    file,
                    anchor,
                    message: rewrite_companion_paths(&related.message, lookup),
                }
            })
            .collect();
        let (start, end) = match &anchor {
            DiagnosticAnchor::Authored { start, end } => (*start, *end),
            // Generated failures have no authored position. Their original
            // checking coordinates keep distinct generated occurrences apart.
            DiagnosticAnchor::Generated { .. } => (
                diagnostic.start,
                diagnostic
                    .start
                    .checked_add(diagnostic.length)
                    .unwrap_or(diagnostic.start),
            ),
        };
        let mut encoder = CanonicalEncoder::new("verter.projection-diagnostic-message.v1");
        encoder.field_str(1, &message);
        let mut message_digest = [0; 16];
        message_digest.copy_from_slice(&encoder.digest().as_bytes()[..16]);
        let dedup = DiagnosticDeduplicationKey {
            carrier: carrier.clone(),
            start,
            end,
            code: diagnostic.code,
            message_digest,
        };
        let error = ProjectionDiagnosticAttribution {
            carrier,
            anchor,
            family: obligation.map(|span| span.family),
            host: obligation.and_then(|span| span.host.clone()),
            code: diagnostic.code,
            category: diagnostic.category,
            message,
            related,
            dedup: dedup.clone(),
        };
        // Generated coordinates belong to their original virtual file,
        // even when several companions recover the same carrier.
        let generated_file = matches!(&error.anchor, DiagnosticAnchor::Generated { .. })
            .then(|| diagnostic.file.clone());
        let bucket = seen.entry((dedup, generated_file)).or_default();
        // A digest is only an index. Compare exact payloads too, preserving
        // severity and related information; replay may have another owner.
        if bucket.iter().any(|&index| {
            let previous = &attributed[index];
            previous.anchor == error.anchor
                && previous.category == error.category
                && previous.message == error.message
                && previous.related == error.related
        }) {
            continue;
        }
        bucket.push(attributed.len());
        attributed.push(error);
    }
    attributed
}

fn locate(
    file: &Arc<str>,
    start: u32,
    length: u32,
    result: Option<&MapperTransformResult>,
    authored: bool,
) -> (Arc<str>, DiagnosticAnchor) {
    let end = start.checked_add(length);
    if file.is_empty() {
        return (
            file.clone(),
            DiagnosticAnchor::Generated {
                snippet: String::new(),
            },
        );
    }
    let Some(companion) = classify_carrier_companion(file).filter(|_| !authored) else {
        return (
            file.clone(),
            match end {
                Some(end) => DiagnosticAnchor::Authored { start, end },
                None => DiagnosticAnchor::Generated {
                    snippet: String::new(),
                },
            },
        );
    };
    let anchor = match end.and_then(|end| {
        result?
            .plane
            .as_ref()
            .map(|plane| authored_answer(plane, MappingSpan { start, end }))
    }) {
        Some(CarrierAnswer::Point(at)) => DiagnosticAnchor::Authored { start: at, end: at },
        Some(CarrierAnswer::Span(span)) => DiagnosticAnchor::Authored {
            start: span.start,
            end: span.end,
        },
        Some(CarrierAnswer::Refused(_)) | None => DiagnosticAnchor::Generated {
            snippet: end
                .and_then(|end| result?.text.get(start as usize..end as usize))
                .unwrap_or_default()
                .to_string(),
        },
    };
    (Arc::from(companion.source), anchor)
}

/// Preserve engine prose and replace only references recognized by the
/// naming owner. Quoted paths may contain spaces; punctuation stays intact.
fn rewrite_companion_paths(message: &str, lookup: &dyn TransformLookup) -> String {
    fn unquoted(text: &str, out: &mut String, lookup: &dyn TransformLookup) {
        for part in text.split_inclusive(|ch: char| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | '<' | '>'
                )
        }) {
            let path = part.trim_end_matches(|ch: char| {
                ch.is_whitespace()
                    || matches!(
                        ch,
                        '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | '<' | '>' | '.' | ':'
                    )
            });
            if let Some(companion) =
                classify_carrier_companion(path).filter(|_| !lookup.is_authored(path))
            {
                out.push_str(&companion.source);
            } else {
                out.push_str(path);
            }
            out.push_str(&part[path.len()..]);
        }
    }
    let mut out = String::with_capacity(message.len());
    let mut rest = message;
    while let Some((at, quote)) = rest
        .char_indices()
        .find(|(_, ch)| matches!(ch, '\'' | '"' | '`'))
    {
        unquoted(&rest[..at], &mut out, lookup);
        out.push(quote);
        rest = &rest[at + quote.len_utf8()..];
        let Some(end) = rest.find(quote) else {
            unquoted(rest, &mut out, lookup);
            return out;
        };
        let quoted = &rest[..end];
        if lookup.is_authored(quoted) {
            out.push_str(quoted);
        } else if let Some(companion) = classify_carrier_companion(quoted) {
            out.push_str(&companion.source);
        } else {
            unquoted(quoted, &mut out, lookup);
        }
        out.push(quote);
        rest = &rest[end + quote.len_utf8()..];
    }
    unquoted(rest, &mut out, lookup);
    out
}

/// The carrier answer for a diagnostic's checking span. Scaffolding the span
/// covers has no authored position of its own, so the diagnostic is reported
/// from where its first authored region maps to where its last one ends, as
/// vue-tsc maps a diagnostic's start and end: `__VerterCtx.msg` lands on
/// `msg`, and `__VerterCtx.a + __VerterCtx.b` on `a + b`. Part of a
/// rewritten region answers for the whole region, its only correspondence.
/// A span of scaffolding alone keeps the plane's refusal.
fn authored_answer(plane: &ContentMapper, span: MappingSpan) -> CarrierAnswer {
    let refused = plane.to_carrier(span);
    if refused != CarrierAnswer::Refused(Refusal::NoCarrierPreimage) {
        return refused;
    }
    let product = plane.product();
    let mut first = None;
    let mut last = None;
    let mut at = span.start;
    while at < span.end {
        let Some(region) = product.projected_at(at) else {
            break;
        };
        let end = region.generated.end.min(span.end);
        if region.class != ProjectedClass::Synthesized {
            let piece = (MappingSpan { start: at, end }, region.generated);
            first = first.or(Some(piece));
            last = Some(piece);
        }
        at = end;
    }
    let bounds = |(piece, whole): (MappingSpan, MappingSpan)| {
        let answer = match plane.to_carrier(piece) {
            CarrierAnswer::Refused(_) => plane.to_carrier(whole),
            answer => answer,
        };
        match answer {
            CarrierAnswer::Point(point) => Some((point, point)),
            CarrierAnswer::Span(authored) => Some((authored.start, authored.end)),
            CarrierAnswer::Refused(_) => None,
        }
    };
    let (Some((start, first_end)), Some((_, last_end))) =
        (first.and_then(bounds), last.and_then(bounds))
    else {
        return refused;
    };
    let end = first_end.max(last_end);
    if end == start {
        CarrierAnswer::Point(start)
    } else {
        CarrierAnswer::Span(MappingSpan { start, end })
    }
}
