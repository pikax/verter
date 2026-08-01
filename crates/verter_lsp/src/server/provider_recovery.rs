//! Bounded transient-error recovery for provider-backed positional queries —
//! the ONE shared owner of the resync + retry-once protocol that hover,
//! definition, and type-definition all run (the no-silent-empty D7 contract).
//!
//! A provider `Err` (project bring-up on the tsserver router, a transient IPC
//! failure, an engine restart) must never surface as a silently dead tooltip or
//! CTRL+CLICK. The recovery is deliberately bounded and nonblocking:
//!
//! 1. Query once against the captured surface.
//! 2. On `Err`: resync the current file, recapture the surface, and retry
//!    EXACTLY once. A second failure fails closed to the caller's native
//!    result — never a fabrication and never a spin. Navigation stays
//!    forbidden from awaiting dependency publication; this heals transient
//!    faults, it is not an admission gate.
//!
//! ## The retry identity fence
//!
//! The request `Position` is interpreted against the carrier source the
//! INITIAL capture described. A concurrent edit landing between the two
//! attempts can put a DIFFERENT token at the same coordinates (`foo.bar` →
//! `other.baz`), and a retry against the fresh surface would then return a
//! confidently WRONG answer for the original request — the recomputed offset
//! validates only that the coordinate maps within the NEW surface, not that
//! its source identity still matches what was asked. So the retry is fenced on
//! source identity: the recaptured surface's carrier `source_hash` must equal
//! the initial capture's, otherwise the recovery fails closed. An empty answer
//! is a nuisance; a wrong jump is a defect.

use tower_lsp_server::ls_types::Position;

use crate::type_provider::merge;
use crate::type_provider::protocol::TypeProviderError;

use super::TypeProviderContext;

/// The outcome of a bounded-recovery provider query.
///
/// `value` is `None` when the provider leg yielded nothing usable (persistent
/// failure, no recapturable surface, identity-fence refusal, unmappable retry
/// position) — the caller then serves its native result. `ctx` is the surface
/// the value was produced against (on `None`, the last captured surface, so
/// hover-style callers keep a coherent context for their fallback branches).
pub(super) struct ProviderQueryOutcome<T> {
    pub(super) value: Option<T>,
    pub(super) ctx: TypeProviderContext,
}

/// Run `query` with the shared bounded transient-error recovery.
///
/// `query` performs the provider call for ONE attempt against the given
/// provider path + generated offset (callers pin any per-attempt state — e.g.
/// the foreign carrier IDE/API sets — inside it, so a retry re-pins under the
/// surface it actually queries). `resync` repairs the current file's provider
/// surface (production: `ensure_current_file_synced`); `recapture` captures
/// the post-resync surface (production: `type_provider_context`). Both are
/// injected so the protocol — including the identity fence — is directly
/// testable without a live provider route.
pub(super) async fn provider_query_with_bounded_recovery<T, QFut, RFut>(
    feature: &'static str,
    position: &Position,
    initial_ctx: TypeProviderContext,
    initial_offset: u32,
    mut query: impl FnMut(String, u32) -> QFut,
    resync: impl FnOnce() -> RFut,
    recapture: impl FnOnce() -> Option<TypeProviderContext>,
) -> ProviderQueryOutcome<T>
where
    QFut: std::future::Future<Output = Result<T, TypeProviderError>>,
    RFut: std::future::Future<Output = ()>,
{
    tracing::debug!("{feature}: querying type provider at tsx offset {initial_offset}");
    match query(initial_ctx.tsx_path.clone(), initial_offset).await {
        Ok(value) => {
            return ProviderQueryOutcome {
                value: Some(value),
                ctx: initial_ctx,
            };
        }
        Err(e) => {
            tracing::warn!("{feature} type provider error: {e} — resyncing and retrying once");
        }
    }

    resync().await;
    let Some(retry_ctx) = recapture() else {
        // No recapturable surface — fail closed to the native result.
        return ProviderQueryOutcome {
            value: None,
            ctx: initial_ctx,
        };
    };

    // IDENTITY FENCE: the retry may only answer the question that was asked.
    // See the module doc — a carrier-source change between attempts means the
    // same coordinates may now name a different token, so fail closed rather
    // than return a coherent-but-wrong result for the original request.
    if retry_ctx.snapshot.source_hash != initial_ctx.snapshot.source_hash {
        tracing::warn!(
            "{feature}: skipping provider retry — the carrier source changed between attempts, \
             so the retry would answer a different request (fail closed to the native result)"
        );
        return ProviderQueryOutcome {
            value: None,
            ctx: initial_ctx,
        };
    }

    let Some(retry_offset) = merge::carrier_position_to_tsx_offset_validated(
        position,
        &retry_ctx.carrier_line_index,
        &retry_ctx.mapper,
        &retry_ctx.tsx_line_index,
    ) else {
        return ProviderQueryOutcome {
            value: None,
            ctx: retry_ctx,
        };
    };

    tracing::debug!("{feature}: retrying type provider at tsx offset {retry_offset}");
    match query(retry_ctx.tsx_path.clone(), retry_offset).await {
        Ok(value) => ProviderQueryOutcome {
            value: Some(value),
            ctx: retry_ctx,
        },
        Err(e) => {
            // Fail-closed-on-persistent: after the bounded retry the caller
            // serves its native result — never invented content, never a spin.
            tracing::warn!("{feature} type provider retry failed: {e}");
            ProviderQueryOutcome {
                value: None,
                ctx: retry_ctx,
            }
        }
    }
}
