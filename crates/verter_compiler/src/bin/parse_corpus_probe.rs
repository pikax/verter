use std::cmp::Ordering;
use std::io::{self, BufRead, BufWriter, Write};

use serde::{Deserialize, Serialize};
use verter_compiler::framework_common::registered_carrier_projection;
use verter_language::{
    compare_language_diagnostics, DiagnosticArg, FrameworkAdapterId, LanguageDiagnostic,
    LanguageDiagnosticSeverity, LanguageId, ParseKey, ParseOptions, SyntaxReject,
};

#[derive(Deserialize)]
struct Request {
    id: String,
    adapter: String,
    source: String,
    #[serde(default)]
    options: ProbeOptions,
}

#[derive(Default, Deserialize)]
struct ProbeOptions {
    delimiters: Option<(String, String)>,
    custom_elements: Option<Vec<String>>,
    #[serde(default)]
    svelte_loose: bool,
}

#[derive(Serialize)]
struct Response {
    id: String,
    outcome: &'static str,
    reject_variant: Option<&'static str>,
    diagnostics: Vec<Diagnostic>,
    validation: Validation,
}

#[derive(Serialize)]
struct Validation {
    spans_mapped: bool,
    diagnostics_sorted: bool,
}

#[derive(Serialize)]
struct Diagnostic {
    start: u32,
    end: u32,
    severity: &'static str,
    code: String,
    arguments: Vec<Argument>,
}

#[derive(Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
enum Argument {
    Bool(bool),
    Unsigned(u64),
    Signed(i64),
    Text(String),
    Span { start: u32, end: u32 },
}

fn diagnostic(value: &LanguageDiagnostic) -> Diagnostic {
    Diagnostic {
        start: value.span.start,
        end: value.span.end,
        severity: match value.severity {
            LanguageDiagnosticSeverity::Error => "error",
            LanguageDiagnosticSeverity::Warning => "warning",
            LanguageDiagnosticSeverity::Info => "info",
        },
        code: value.code.to_string(),
        arguments: value
            .arguments
            .iter()
            .map(|argument| match argument {
                DiagnosticArg::Bool(value) => Argument::Bool(*value),
                DiagnosticArg::Unsigned(value) => Argument::Unsigned(*value),
                DiagnosticArg::Signed(value) => Argument::Signed(*value),
                DiagnosticArg::Text(value) => Argument::Text(value.clone()),
                DiagnosticArg::Span { start, end } => Argument::Span {
                    start: *start,
                    end: *end,
                },
            })
            .collect(),
    }
}

fn spans_mapped(source: &str, diagnostics: &[&LanguageDiagnostic]) -> bool {
    diagnostics.iter().all(|diagnostic| {
        let start = diagnostic.span.start as usize;
        let end = diagnostic.span.end as usize;
        start <= end
            && end <= source.len()
            && source.is_char_boundary(start)
            && source.is_char_boundary(end)
    })
}

fn diagnostics_sorted(parse_key: &ParseKey, diagnostics: &[&LanguageDiagnostic]) -> bool {
    diagnostics.windows(2).all(|pair| {
        compare_language_diagnostics(parse_key, pair[0], parse_key, pair[1]) != Ordering::Greater
    })
}

fn reject_response(id: String, source: &str, reject: SyntaxReject) -> Response {
    let (variant, parse_key, values): (&'static str, &ParseKey, Vec<&LanguageDiagnostic>) =
        match &reject {
            SyntaxReject::UnsupportedProfile { parse_key, .. } => {
                ("unsupported_profile", parse_key, Vec::new())
            }
            SyntaxReject::RejectedSyntax {
                parse_key,
                primary,
                related,
                ..
            } => {
                let mut values = Vec::with_capacity(related.len() + 1);
                values.push(primary.as_ref());
                values.extend(related.iter());
                ("rejected_syntax", parse_key, values)
            }
            SyntaxReject::UnmappedDiagnostic { parse_key, .. } => {
                ("unmapped_diagnostic", parse_key, Vec::new())
            }
            SyntaxReject::InvalidCarrierGeometry { parse_key, .. } => {
                ("invalid_carrier_geometry", parse_key, Vec::new())
            }
        };
    Response {
        id,
        outcome: "reject",
        reject_variant: Some(variant),
        diagnostics: values.iter().map(|value| diagnostic(value)).collect(),
        validation: Validation {
            spans_mapped: spans_mapped(source, &values),
            diagnostics_sorted: diagnostics_sorted(parse_key, &values),
        },
    }
}

fn handle(request: Request) -> Result<Response, String> {
    let (adapter_id, language_id, options) = match request.adapter.as_str() {
        "vue" => {
            // The corpus caller sends `None` for "use Vue's own standard
            // delimiters" (this probe's callers never intend an actually-empty
            // delimiter pair) — resolve that explicitly here rather than
            // letting an absent value silently become an empty one.
            (
                FrameworkAdapterId::vue(),
                LanguageId::new("vue"),
                ParseOptions {
                    delimiters: request
                        .options
                        .delimiters
                        .unwrap_or_else(|| ParseOptions::vue_standard().delimiters),
                    custom_elements: request.options.custom_elements.unwrap_or_default(),
                    svelte_loose: false,
                },
            )
        }
        "svelte" => (
            FrameworkAdapterId::svelte(),
            LanguageId::new("svelte"),
            ParseOptions {
                svelte_loose: request.options.svelte_loose,
                ..ParseOptions::default()
            },
        ),
        other => (
            FrameworkAdapterId::new(other),
            LanguageId::new(other),
            ParseOptions {
                svelte_loose: request.options.svelte_loose,
                custom_elements: request.options.custom_elements.unwrap_or_default(),
                delimiters: request.options.delimiters.unwrap_or_default(),
            },
        ),
    };
    match registered_carrier_projection::parse_registered_frontend(
        &adapter_id,
        &language_id,
        &request.source,
        &options,
    ) {
        Ok(artifact) => {
            let values = artifact.diagnostics.iter().collect::<Vec<_>>();
            Ok(Response {
                id: request.id,
                outcome: "ok",
                reject_variant: None,
                diagnostics: values.iter().map(|value| diagnostic(value)).collect(),
                validation: Validation {
                    spans_mapped: spans_mapped(&request.source, &values),
                    diagnostics_sorted: diagnostics_sorted(&artifact.parse_key, &values),
                },
            })
        }
        Err(reject) => Ok(reject_response(request.id, &request.source, reject)),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdout = BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Request = serde_json::from_str(&line)?;
        let response = handle(request).map_err(io::Error::other)?;
        serde_json::to_writer(&mut stdout, &response)?;
        stdout.write_all(b"\n")?;
        stdout.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The concrete regression example: an unclosed `<template>` must surface
    /// a mapped diagnostic through the carrier's own `diagnostics` field —
    /// not an empty vector (the bug this probe's Vue branch used to hide by
    /// rebuilding diagnostics from the raw parsed carrier instead of reading
    /// the mapped channel `build_vue_parse_artifact` populates).
    #[test]
    fn vue_probe_diagnostics_come_from_the_carriers_own_mapped_channel() {
        let request = Request {
            id: "1".to_string(),
            adapter: "vue".to_string(),
            source: "<template><div></template>".to_string(),
            options: ProbeOptions::default(),
        };
        let response = handle(request).expect("probe handles the request");
        assert_eq!(response.outcome, "ok");
        assert!(
            !response.diagnostics.is_empty(),
            "an unclosed <div> inside <template> must surface a mapped diagnostic"
        );
        assert!(
            response.validation.spans_mapped,
            "spans must map into the source"
        );
        assert!(
            response.validation.diagnostics_sorted,
            "diagnostics must already be in normative order"
        );
    }

    /// Negative control: a well-formed template produces no diagnostics.
    #[test]
    fn vue_probe_reports_no_diagnostics_for_a_well_formed_template() {
        let request = Request {
            id: "2".to_string(),
            adapter: "vue".to_string(),
            source: "<template><div>ok</div></template>".to_string(),
            options: ProbeOptions::default(),
        };
        let response = handle(request).expect("probe handles the request");
        assert_eq!(response.outcome, "ok");
        assert!(
            response.diagnostics.is_empty(),
            "a well-formed template must not fabricate a diagnostic"
        );
    }
}
