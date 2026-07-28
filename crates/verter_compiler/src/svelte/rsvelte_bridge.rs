//! Translation between rsvelte's policy-free compiler facade and Verter's
//! framework-neutral carrier bundle.
//!
//! This module is the only production code in Verter that names
//! `rsvelte_core`. Cache admission, scheduling, source ownership, IDE
//! projection, and type resolution stay on the Verter side of the boundary.

use std::sync::Arc;

use rsvelte_core::{
    compiler::{AnalysisError, CssMode},
    toolchain::{
        RuntimeTarget, Toolchain, FACTS_ABI_VERSION, RUNTIME_ABI_VERSION, TOOLCHAIN_ABI_VERSION,
    },
    CompileError, CompileOptions,
};
use verter_span::Span;

use crate::framework_common::{
    RuntimeCompileOptions, RuntimeCompileOutput, RuntimeDiagnostic, RuntimeDiagnosticSeverity,
    RuntimeStyleBlock,
};

const EXPECTED_TOOLCHAIN_ABI_VERSION: u32 = 1;
const EXPECTED_RUNTIME_ABI_VERSION: u32 = 1;
const EXPECTED_FACTS_ABI_VERSION: u32 = 2;
const EXPECTED_RSVELTE_VERSION: &str = "0.9.4";
const EXPECTED_SVELTE_VERSION: &str = "5.56.8";

const _: () = {
    assert!(TOOLCHAIN_ABI_VERSION == EXPECTED_TOOLCHAIN_ABI_VERSION);
    assert!(RUNTIME_ABI_VERSION == EXPECTED_RUNTIME_ABI_VERSION);
    assert!(FACTS_ABI_VERSION == EXPECTED_FACTS_ABI_VERSION);
};

/// Compile one runtime target through rsvelte and translate it into the
/// carrier-neutral bundle consumed by the Verter host.
pub(super) fn compile_runtime(source: &str, opts: &RuntimeCompileOptions) -> RuntimeCompileOutput {
    let toolchain = Toolchain::new();
    let fingerprint = toolchain.fingerprint();
    if fingerprint.rsvelte_version != EXPECTED_RSVELTE_VERSION
        || fingerprint.svelte_version != EXPECTED_SVELTE_VERSION
    {
        return incompatible_engine(fingerprint.rsvelte_version, fingerprint.svelte_version);
    }

    let mut compile_options = CompileOptions {
        filename: opts.filename.clone(),
        output_filename: opts.filename.clone(),
        css_output_filename: opts.filename.clone(),
        custom_element: opts.custom_element,
        css: CssMode::External,
        preserve_comments: opts.comments.unwrap_or(false),
        enable_sourcemap: opts.source_map,
        ..Default::default()
    };
    if let Some(scope_hash) = opts.svelte_css_hash_override.clone() {
        compile_options.css_hash = Some(Arc::new(move |_| scope_hash.clone()));
    }

    let target = if opts.ssr {
        RuntimeTarget::Server
    } else {
        RuntimeTarget::Client
    };
    let result = toolchain
        .prepare(source, compile_options)
        .and_then(|mut prepared| {
            let scope_hash = prepared.facts().css_scope_hash.clone();
            prepared
                .compile(target)
                .map(|compiled| (compiled, scope_hash))
        });

    match result {
        Ok((compiled, scope_hash)) => {
            let mut bundle = RuntimeCompileOutput::default();
            bundle.main.body_code = Some(compiled.js.code);
            bundle.main.source_map = compiled.js.map.unwrap_or_default();
            bundle.main.lang = Some("js".to_string());
            bundle.diagnostics = compiled
                .warnings
                .into_iter()
                .map(|warning| RuntimeDiagnostic {
                    severity: RuntimeDiagnosticSeverity::Warning,
                    code: warning.code,
                    message: warning.message,
                    span: warning_span(source, warning.start, warning.end),
                })
                .collect();

            if let Some(css) = compiled.css {
                bundle.styles.push(RuntimeStyleBlock {
                    code: css.code,
                    source_map: if opts.source_map { css.map } else { None },
                    lang: None,
                    scope_hash,
                    has_global: css.has_global,
                });
            }
            bundle
        }
        Err(error) => {
            let diagnostic = compile_error_diagnostic(&error);
            RuntimeCompileOutput {
                runtime_refusal: Some(diagnostic.clone()),
                diagnostics: vec![diagnostic],
                ..Default::default()
            }
        }
    }
}

fn incompatible_engine(rsvelte_version: &str, svelte_version: &str) -> RuntimeCompileOutput {
    let diagnostic = RuntimeDiagnostic {
        severity: RuntimeDiagnosticSeverity::Warning,
        code: "rsvelte-incompatible-engine".to_string(),
        message: format!(
            "Verter requires rsvelte {EXPECTED_RSVELTE_VERSION} targeting Svelte \
             {EXPECTED_SVELTE_VERSION}, but loaded rsvelte {rsvelte_version} targeting \
             Svelte {svelte_version}"
        ),
        span: None,
    };
    RuntimeCompileOutput {
        runtime_refusal: Some(diagnostic.clone()),
        diagnostics: vec![diagnostic],
        ..Default::default()
    }
}

fn compile_error_diagnostic(error: &CompileError) -> RuntimeDiagnostic {
    let (code, span) = match error {
        CompileError::Parse(parse) => {
            let code = match parse {
                rsvelte_core::error::ParseError::SvelteError { code, .. } => code.clone(),
                _ => "parse-error".to_string(),
            };
            let (start, end) = parse.span();
            (code, Some(span_from_usize(start, end)))
        }
        CompileError::Analysis(analysis) => {
            let code = match analysis {
                AnalysisError::ValidationWithCode { code, .. } => code.clone(),
                AnalysisError::Scope(_) => "scope-error".to_string(),
                AnalysisError::Validation(_) => "validation-error".to_string(),
                AnalysisError::Css(_) => "css-error".to_string(),
            };
            (code, None)
        }
        CompileError::Transform(_) => ("transform-error".to_string(), None),
    };
    RuntimeDiagnostic {
        severity: RuntimeDiagnosticSeverity::Warning,
        code: format!("rsvelte-{code}"),
        message: error.to_string(),
        span,
    }
}

fn warning_span(
    source: &str,
    start: Option<rsvelte_core::compiler::Position>,
    end: Option<rsvelte_core::compiler::Position>,
) -> Option<Span> {
    let start = start?;
    let end = end.unwrap_or_else(|| start.clone());
    Some(Span::new(
        utf16_offset_to_byte(source, start.character),
        utf16_offset_to_byte(source, end.character),
    ))
}

fn utf16_offset_to_byte(source: &str, wanted: usize) -> u32 {
    let mut utf16_offset = 0;
    for (byte_offset, character) in source.char_indices() {
        if utf16_offset >= wanted {
            return byte_offset as u32;
        }
        utf16_offset += character.len_utf16();
    }
    source.len() as u32
}

fn span_from_usize(start: usize, end: usize) -> Span {
    Span::new(
        u32::try_from(start).unwrap_or(u32::MAX),
        u32::try_from(end).unwrap_or(u32::MAX),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_css_hash_matches_the_pinned_svelte_engine() {
        let output = compile_runtime(
            "<style>.card { color: blue }</style><div class=\"card\">ok</div>",
            &RuntimeCompileOptions {
                filename: Some("App.svelte".to_string()),
                ..Default::default()
            },
        );

        assert!(
            !output.runtime_surface_refused(),
            "{:?}",
            output.diagnostics
        );
        assert_eq!(
            output.styles[0].scope_hash.as_deref(),
            Some("svelte-n50uah")
        );
    }

    #[test]
    fn resolved_css_hash_override_crosses_the_bridge_verbatim() {
        let output = compile_runtime(
            "<style>.card { color: blue }</style><div class=\"card\">ok</div>",
            &RuntimeCompileOptions {
                filename: Some("App.svelte".to_string()),
                svelte_css_hash_override: Some("scope-from-config".to_string()),
                ..Default::default()
            },
        );

        assert!(
            !output.runtime_surface_refused(),
            "{:?}",
            output.diagnostics
        );
        assert_eq!(
            output.styles[0].scope_hash.as_deref(),
            Some("scope-from-config")
        );
        assert!(
            output
                .main
                .body_code
                .as_deref()
                .is_some_and(|code| { code.contains("scope-from-config") }),
            "the runtime module and CSS artifact must share the resolved scope hash"
        );
    }
}
