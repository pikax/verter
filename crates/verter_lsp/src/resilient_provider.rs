//! The provider lifecycle owner — delegates to `verter_type_runtime`.
//!
//! This module re-exports the shared [`ProviderHub`] surface from
//! `verter_type_runtime::provider_hub`, and provides the LSP-specific
//! `LspNotifier` that bridges the outbound transport → `ProviderNotifier`.

use std::sync::Arc;

use crate::outbound::Outbound;
use verter_type_runtime::provider_hub::{DroppedAdmittedState, EngineStart};

// Re-export the shared lifecycle owner.
pub(crate) use verter_type_runtime::provider_hub::{
    EstablishFuture, HubPolicy, ProviderEstablisher, ProviderHub,
};

/// The recovery re-arm for admitted generated state: invoked with everything a
/// replacement install dropped, AFTER the replacement serves, so the tier that
/// minted the admissions can re-publish them through a fresh admission bound
/// to the new serving epoch (never an unproven replay).
pub(crate) type AdmittedStateRearm = Arc<dyn Fn(&DroppedAdmittedState) + Send + Sync>;

/// LSP-specific notifier. Its callbacks are synchronous, so every message joins
/// the transport's ordered, accounted control line
/// ([`Outbound::notify_detached`]) instead of a detached send.
pub(crate) struct LspNotifier {
    control: Outbound,
    /// The provider kind (`tsserver` / `tsgo`) carried on the structural
    /// respawn notification, matching the backend's own user label.
    kind: &'static str,
    /// Whether an [`EngineStart::Initial`] establishment is announced on the
    /// wire. A route that attests its managed engine stays cold (shared-tsgo:
    /// "managed TSGO remains cold until an observed attach failure") keeps the
    /// fallback's first serve off the `$/verter/typeProviderStarted` channel
    /// the attestation is asserted over; a crash REPLACEMENT is still
    /// announced so the editor's pid tracking follows the fresh child.
    announce_initial_starts: bool,
}

impl LspNotifier {
    /// A notifier that announces every engine start on the wire — the policy
    /// of every route whose engine is editor-visible from its first serve.
    pub fn new(client: Outbound, kind: &'static str) -> Self {
        Self {
            control: client,
            kind,
            announce_initial_starts: true,
        }
    }

    /// A notifier that announces only crash replacements — the policy of the
    /// shared route's managed fallback, whose attested promise is that the
    /// managed engine stays cold.
    pub fn recovery_only(client: Outbound, kind: &'static str) -> Self {
        Self {
            control: client,
            kind,
            announce_initial_starts: false,
        }
    }
}

impl verter_type_runtime::provider_hub::ProviderNotifier for LspNotifier {
    fn notify(&self, severity: verter_type_runtime::provider_hub::NotifySeverity, message: String) {
        use verter_type_runtime::provider_hub::NotifySeverity;

        let typ = match severity {
            NotifySeverity::Info => tower_lsp_server::ls_types::MessageType::INFO,
            NotifySeverity::Warning => tower_lsp_server::ls_types::MessageType::WARNING,
            NotifySeverity::Error => tower_lsp_server::ls_types::MessageType::ERROR,
        };
        self.control
            .notify_detached::<tower_lsp_server::ls_types::notification::ShowMessage>(
                tower_lsp_server::ls_types::ShowMessageParams { typ, message },
            );
    }

    fn provider_started(&self, pid: Option<u32>, start: EngineStart) {
        if start == EngineStart::Initial && !self.announce_initial_starts {
            tracing::info!(
                kind = self.kind,
                ?pid,
                "managed engine began serving; its initial start stays off the wire \
                 (the route attests the managed engine stays cold)"
            );
            return;
        }
        // No pid, no notification: the contract carries a real child process
        // id, and fabricating one would make a restart look like a fresh start
        // against a process that does not exist.
        let Some(pid) = pid else {
            tracing::warn!("{} respawned without a reportable child pid", self.kind);
            return;
        };
        self.control
            .notify_detached::<crate::server::protocol_types::TypeProviderStarted>(
                crate::server::protocol_types::TypeProviderStartedParams {
                    pid,
                    kind: self.kind.to_string(),
                },
            );
    }
}
