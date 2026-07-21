// The carrier-membership / provider-sync paths must never hold a synchronous guard
// across an `.await` (the single-writer actor + reconciler lock discipline). Deny it
// crate-wide so a regression fails the build, matching `verter_type_runtime`.
#![deny(clippy::await_holding_lock)]

/// Max concurrent in-flight requests the tower-lsp-server serve loop dispatches
/// (`Server::concurrency_level`). tower-lsp-server 0.23 defaults to 4; a handful
/// of slow semantic handlers then occupy every slot, the framed-stdin forwarder
/// stalls, and the server stops reading client stdin entirely — so provider-free
/// control requests (`$/verter/getStatistics`, `$/cancelRequest`) are STARVED and
/// no client-side rescue can land. The always-on per-request deadline stops a
/// handler occupying a slot forever; this generous cap additionally guarantees
/// control requests get a slot immediately alongside a burst of semantic work.
pub const LSP_MAX_CONCURRENCY: usize = 64;

/// Stack size for the thread that runs the tokio runtime and therefore polls
/// `Server::serve`.
///
/// `tower-lsp-server` drives every request through `buffer_unordered`, so all
/// handler futures are polled INLINE on whichever thread called `block_on` —
/// not on runtime workers. Under `#[tokio::main]` that thread is the process
/// main thread, whose stack on Windows/MSVC is the linker default of **1 MiB**
/// (`SizeOfStackReserve` in the PE optional header); Rust does not raise it, and
/// `RUST_MIN_STACK` cannot — that variable only affects `std::thread`-spawned
/// threads.
///
/// Measured peak consumption of a healthy `textDocument/definition`, from the
/// serve thread's base to the deepest frame, and constant across a 10-line SFC,
/// a mid-sized component and a 1200-line component with a large import closure
/// (i.e. bounded by code shape, not by input):
///
/// | profile | handler entry | peak  |
/// |---------|---------------|-------|
/// | release | 115 KiB       | 117 KiB |
/// | debug   | 1817 KiB      | 1857 KiB |
///
/// Release therefore fits inside 1 MiB with room to spare. Debug does not: the
/// same request needs ~1.8 MiB because unoptimized `async fn` state machines
/// nest inline and their poll frames are not compacted, so a debug server on
/// Windows died on its first request regardless of what that request did.
///
/// 8 MiB is chosen as defence in depth, not as a correctness condition: ~70x
/// headroom over the release peak and ~4.4x over the debug peak, while staying
/// small enough to be an ordinary thread rather than a licence for unbounded
/// recursion. A runaway recursion must still be fixed at its source — no stack
/// size survives one.
pub const SERVE_THREAD_STACK_BYTES: usize = 8 * 1024 * 1024;

/// Run `body` on a thread with [`SERVE_THREAD_STACK_BYTES`] of stack and return
/// its value, propagating a panic to the caller.
///
/// This is the server's entry point: it exists so the thread that polls
/// `Server::serve` has an explicitly sized stack instead of inheriting the
/// platform's main-thread default.
pub fn run_on_serve_thread<F, T>(body: F) -> T
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    std::thread::Builder::new()
        .name("verter-lsp-serve".to_string())
        .stack_size(SERVE_THREAD_STACK_BYTES)
        .spawn(body)
        .expect("serve thread must spawn")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}

#[cfg(test)]
#[path = "serve_thread_tests.rs"]
mod serve_thread_tests;

pub mod analysis;
pub mod audit_harness;
pub mod capabilities;
pub mod carrier_cache;
pub mod carrier_registry;
pub mod config;
pub mod css;
pub mod documents;
pub mod editor_tsserver;
pub mod extension_provider;
pub mod external_ts;
pub mod external_ts_sync;
pub mod features;
pub mod project_resolver;
pub mod provider_surface_store;
pub mod provider_sync;
pub mod resync_singleflight;
pub mod server;
pub mod statistics;
pub mod svelte_assets;
pub mod sync_coordinator;
pub mod tsgo;
pub mod tsserver;
pub mod type_provider;
pub mod utils;
pub mod vue_assets;
pub mod workspace_scanner;
pub mod workspace_state;

mod resilient_provider;
mod uri;

#[cfg(test)]
mod hot_path_overhead_tests;
#[cfg(test)]
#[allow(
    unused_must_use,
    clippy::unused_enumerate_index,
    clippy::unnecessary_to_owned,
    clippy::redundant_iter_cloned
)]
mod integration_tests;
#[cfg(test)]
mod real_provider_tests;
#[cfg(test)]
mod resilient_provider_tests;
#[cfg(test)]
mod test_harness;
#[cfg(test)]
mod test_harness_gating;
#[cfg(test)]
mod test_utils;

use std::sync::Arc;
use verter_session::VerterHost;

use type_provider::traits::TypeProvider;

fn public_api_projection_subject_json(
    subject: verter_session::PublicApiProjectionSubject,
) -> serde_json::Value {
    match subject {
        verter_session::PublicApiProjectionSubject::Macro { syntax_index } => {
            serde_json::json!({ "kind": "macro", "syntaxIndex": syntax_index })
        }
        verter_session::PublicApiProjectionSubject::ScriptSetupAttrs { source_range } => {
            serde_json::json!({
                "kind": "scriptSetupAttrs",
                "sourceRange": { "start": source_range.start, "end": source_range.end },
            })
        }
    }
}

/// Emit every stable field of a public-API failure instead of collapsing it
/// into the ordinary `None` path.
pub(crate) fn report_public_api_projection_error(
    context: &'static str,
    canonical_id: &str,
    error: &verter_session::PublicApiProjectionError,
) {
    let unavailable_outcome = error.unavailable_outcome();
    tracing::error!(
        context,
        canonical_id,
        code = error.code(),
        detail_code = error.detail_code(),
        subject = ?error.subject(),
        declaration_shape_reason = ?error.declaration_shape_reason().map(|reason| reason.code()),
        member_ordinal = ?error.member_ordinal(),
        outcome_kind = ?unavailable_outcome.map(|outcome| outcome.kind_code()),
        outcome_reason = ?unavailable_outcome.map(|outcome| outcome.reason_code()),
        outcome_diagnostic = ?unavailable_outcome.and_then(|outcome| outcome.diagnostic()),
        "public API projection failed"
    );
}

/// Preserve a public-API projection failure on the JSON-RPC transport rail.
pub(crate) fn public_api_projection_jsonrpc_error(
    context: &'static str,
    canonical_id: &str,
    error: verter_session::PublicApiProjectionError,
) -> tower_lsp_server::jsonrpc::Error {
    report_public_api_projection_error(context, canonical_id, &error);
    let unavailable_outcome = error.unavailable_outcome();
    tower_lsp_server::jsonrpc::Error {
        code: tower_lsp_server::jsonrpc::ErrorCode::InternalError,
        message: std::borrow::Cow::Owned(format!("{context}: public API projection failed")),
        data: Some(serde_json::json!({
            "code": error.code(),
            "detailCode": error.detail_code(),
            "subject": public_api_projection_subject_json(error.subject()),
            "declarationShapeReason": error
                .declaration_shape_reason()
                .map(|reason| reason.code()),
            "memberOrdinal": error.member_ordinal(),
            "outcomeKind": unavailable_outcome.map(|outcome| outcome.kind_code()),
            "outcomeReason": unavailable_outcome.map(|outcome| outcome.reason_code()),
            "outcomeDiagnostic": unavailable_outcome.and_then(|outcome| outcome.diagnostic()),
        })),
    }
}

#[cfg(test)]
mod public_api_projection_transport_tests {
    use super::*;
    use verter_compiler::tsc::{
        TscFailureSubject, TscGenerationError, TscInvalidOutcome, TscUnavailableOutcome,
    };
    use verter_macro_dto::{
        MacroFailure, MacroInvalidReason, MacroPartialReason, UnresolvedReason, UnsupportedReason,
    };
    use verter_session::{FileLanguage, HostConfig, PublicApiMode, UpsertRequest};

    #[test]
    fn jsonrpc_projection_error_preserves_every_stable_field() {
        let host = VerterHost::new_standalone(HostConfig::default());
        let _update = host
            .upsert(UpsertRequest {
                canonical_id: Some("/src/UnsafeEnum.vue".to_string()),
                input_id: "/src/UnsafeEnum.vue".to_string(),
                source: Arc::from(
                    r#"<script setup lang="ts">
enum Unsafe { Value = Math.random() }
defineProps<{ value: Unsafe }>()
</script>"#,
                ),
                file_language: FileLanguage::vue(),
                aliases: Vec::new(),
            })
            .expect("upsert unsafe enum");
        let projection_error = host
            .get_public_api_with_mode("/src/UnsafeEnum.vue", PublicApiMode::Declaration, None)
            .expect_err("unsafe enum projection");
        let error = public_api_projection_jsonrpc_error(
            "getVirtualFiles",
            "/src/UnsafeEnum.vue",
            projection_error,
        );

        assert_eq!(
            error.code,
            tower_lsp_server::jsonrpc::ErrorCode::InternalError
        );
        assert_eq!(
            error.message,
            "getVirtualFiles: public API projection failed"
        );
        assert_eq!(
            error.data,
            Some(serde_json::json!({
                "code": "tsc-generation",
                "detailCode": "unsupported-declaration-shape",
                "subject": { "kind": "macro", "syntaxIndex": 0 },
                "declarationShapeReason": "unsupported-enum-shape",
                "memberOrdinal": null,
                "outcomeKind": null,
                "outcomeReason": null,
                "outcomeDiagnostic": null,
            }))
        );
    }

    #[test]
    fn jsonrpc_projection_error_preserves_all_unavailable_outcome_arms() {
        let cases = [
            (
                TscUnavailableOutcome::Partial(MacroFailure::new(
                    MacroPartialReason::IncompleteTraversal,
                    Some("partial detail".to_string()),
                )),
                "partial",
                "incomplete-traversal",
                "partial detail",
            ),
            (
                TscUnavailableOutcome::Unresolved(MacroFailure::new(
                    UnresolvedReason::AmbiguousReference,
                    Some("unresolved detail".to_string()),
                )),
                "unresolved",
                "ambiguous-reference",
                "unresolved detail",
            ),
            (
                TscUnavailableOutcome::Unsupported(MacroFailure::new(
                    UnsupportedReason::SemanticConstruct,
                    Some("unsupported detail".to_string()),
                )),
                "unsupported",
                "semantic-construct",
                "unsupported detail",
            ),
            (
                TscUnavailableOutcome::Invalid(TscInvalidOutcome::Macro(MacroFailure::new(
                    MacroInvalidReason::NonObjectRoot,
                    Some("invalid detail".to_string()),
                ))),
                "invalid",
                "non-object-root",
                "invalid detail",
            ),
        ];

        for (syntax_index, (outcome, kind, reason, diagnostic)) in cases.into_iter().enumerate() {
            let error = public_api_projection_jsonrpc_error(
                "hover",
                "/src/Unavailable.vue",
                TscGenerationError::UnavailableOutcome {
                    subject: TscFailureSubject::Macro {
                        syntax_index: syntax_index as u32,
                    },
                    outcome,
                }
                .into(),
            );

            assert_eq!(
                error.data,
                Some(serde_json::json!({
                    "code": "tsc-generation",
                    "detailCode": "unavailable-outcome",
                    "subject": { "kind": "macro", "syntaxIndex": syntax_index },
                    "declarationShapeReason": null,
                    "memberOrdinal": null,
                    "outcomeKind": kind,
                    "outcomeReason": reason,
                    "outcomeDiagnostic": diagnostic,
                }))
            );
        }
    }

    #[test]
    fn jsonrpc_projection_error_preserves_script_setup_attrs_subject() {
        let error = public_api_projection_jsonrpc_error(
            "hover",
            "/src/MalformedAttrs.vue",
            TscGenerationError::UnavailableOutcome {
                subject: TscFailureSubject::ScriptSetupAttrs {
                    source_range: verter_span::Span::new(31, 37),
                },
                outcome: TscUnavailableOutcome::Invalid(TscInvalidOutcome::AuthoredTypeSyntax(
                    verter_compiler::tsc::TscInvalidAuthoredTypeReason::MalformedOrRecoveredTypeSyntax,
                )),
            }
            .into(),
        );
        assert_eq!(
            error.data.expect("structured data")["subject"],
            serde_json::json!({
                "kind": "scriptSetupAttrs",
                "sourceRange": { "start": 31, "end": 37 },
            })
        );
    }
}

/// Which TypeScript type provider backend is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeProviderKind {
    /// TSGO (Go-based TypeScript server).
    Tsgo,
    /// tsserver (Node.js-based TypeScript server).
    Tsserver,
    /// The editor's own tsserver, extended by Verter's contributed plugin.
    EditorTsserver,
    /// No type provider — verter-only mode.
    None,
}

impl std::fmt::Display for TypeProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeProviderKind::Tsgo => write!(f, "TSGO"),
            TypeProviderKind::Tsserver => write!(f, "tsserver"),
            TypeProviderKind::EditorTsserver => write!(f, "editor-tsserver"),
            TypeProviderKind::None => write!(f, "none"),
        }
    }
}

/// Configuration for creating a verter LSP server instance.
pub struct LspConfig {
    /// The verter host instance (always required, shared via Arc for MCP embedding).
    pub host: Arc<VerterHost>,
    /// Optional in-process provider actor. This is `None` both in Verter-only mode and
    /// when the editor-owned tsserver/plugin is the attested semantic authority.
    pub type_provider: Option<Arc<dyn TypeProvider>>,
    /// How files are synced to the type provider.
    pub project_sync_mode: ProjectSyncMode,
    /// Which type provider backend is active.
    pub type_provider_kind: TypeProviderKind,
    /// Actual MCP HTTP port (already bound). `None` when MCP is disabled.
    /// The LSP sends a `$/verter/mcpReady` notification during `initialized()`.
    pub mcp_port: Option<u16>,
    /// Human-readable provenance for the selected provider, or the reason no provider
    /// could be started. Sent via `$/verter/typeProviderStatus` for editor status UI.
    pub type_provider_reason: Option<String>,
    /// TEST SEAM: when `true`, `did_open` does NOT eagerly prewarm an imported
    /// child carrier's `{carrier}.ts` PUBLIC-API surface. Production leaves this
    /// `false` (the prewarm makes hover/completion/go-to-def on `<ChildComponent>`
    /// work immediately). With suppression on, the ONLY sync of a closed child's
    /// API surface would come from INSIDE `handle_rename`'s own sync-before-query —
    /// so this seam drives the WOULD-BE discriminator for that path. That lane is
    /// currently `#[ignore]`'d: under tsserver the in-`handle_rename` sync opens the
    /// child too late to join the parent's program (the Block H-membership
    /// program-membership gap), so suppression does NOT prove `handle_rename`'s own
    /// sync closes the closed child today — it pins the seam against which Block
    /// H-membership is validated.
    pub suppress_imported_carrier_prewarm: bool,
}

/// Controls what data `verter_lsp` sends to the type provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectSyncMode {
    /// Send resolver-managed project files to the type provider.
    /// `.vue` files are exposed as `.vue.tsx` for IDE queries and `.vue.ts`
    /// for public API resolution; non-carrier files are synced as source files.
    #[default]
    FullProject,
}
