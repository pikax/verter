//! TSGO-backed `TypeExpander` implementation.
//!
//! Follows the same flow as the tsserver expander but communicates via
//! LSP JSON-RPC (TSGO protocol). Shares `parse_hover_to_expansion` and
//! `build_minimal_artifact` with the tsserver module.
//!
//! For component-meta in `Tsgo` mode, this is a first-class expansion path —
//! the result comes directly from TSGO, not routed through `VerterTypeExpander`.

#[cfg(feature = "type-runtime")]
use std::sync::Arc;

#[cfg(feature = "type-runtime")]
use verter_type_runtime::TypeProvider;

#[cfg(feature = "type-runtime")]
use crate::type_expansion::{
    ExpanderFuture, TypeExpander, TypeExpansionError, TypeExpansionRequest, TypeExpansionResult,
};
#[cfg(feature = "type-runtime")]
use crate::type_expansion_host::TypeExpansionHost;
#[cfg(feature = "type-runtime")]
use crate::type_expansion_tsserver::{build_minimal_artifact, parse_hover_to_expansion};

/// TSGO-backed `TypeExpander`.
///
/// Uses hover at generated offsets to resolve types via TSGO LSP JSON-RPC.
#[cfg(feature = "type-runtime")]
pub struct TsgoTypeExpander<H: TypeExpansionHost> {
    host: Arc<H>,
    provider: Arc<dyn TypeProvider>,
}

#[cfg(feature = "type-runtime")]
impl<H: TypeExpansionHost> TsgoTypeExpander<H> {
    pub fn new(host: Arc<H>, provider: Arc<dyn TypeProvider>) -> Self {
        Self { host, provider }
    }
}

#[cfg(feature = "type-runtime")]
impl<H: TypeExpansionHost + Send + Sync + 'static> TypeExpander for TsgoTypeExpander<H> {
    fn expand_type<'a>(
        &'a self,
        request: &'a TypeExpansionRequest,
    ) -> ExpanderFuture<'a, TypeExpansionResult> {
        Box::pin(async move {
            let snapshot = self
                .host
                .snapshot_view(&request.canonical_id)
                .map_err(|_| TypeExpansionError::SourceUnavailable)?;

            let artifact = build_minimal_artifact(
                &request.canonical_id,
                &snapshot.source.text,
                &snapshot,
                request,
            )?;

            let generated_offset = artifact
                .sfc_to_generated(request.span.start)
                .ok_or(TypeExpansionError::MappingFailed)?;

            let virtual_path = artifact.artifact_id.virtual_path();
            self.provider
                .load_file(&virtual_path, &artifact.generated_source)
                .await
                .map_err(|_| {
                    TypeExpansionError::BackendFailure(
                        crate::type_expansion::BackendFailureKind::Unavailable,
                    )
                })?;

            let hover = self
                .provider
                .get_hover(&virtual_path, generated_offset)
                .await
                .map_err(|_| {
                    TypeExpansionError::BackendFailure(
                        crate::type_expansion::BackendFailureKind::TimedOut,
                    )
                })?;

            match hover {
                Some(info) => parse_hover_to_expansion(&info),
                None => Err(TypeExpansionError::NoExpansionResult),
            }
        })
    }
}
