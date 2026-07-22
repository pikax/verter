//! Resilient TypeProvider wrapper — delegates to `verter_type_runtime`.
//!
//! This module re-exports the `ResilientProvider` and `ResilientBackend` from
//! `verter_type_runtime::resilient`, and provides the LSP-specific `LspNotifier`
//! that bridges `tower_lsp_server::Client` → `ProviderNotifier`.

use std::sync::Arc;
use tokio::sync::OnceCell;
use tower_lsp_server::Client;

// Re-export the shared resilient provider
pub(crate) use verter_type_runtime::resilient::{ResilientBackend, ResilientProvider};

/// LSP-specific notifier that uses `client.show_message()` / `client.log_message()`.
pub(crate) struct LspNotifier {
    client: Arc<OnceCell<Client>>,
    /// The provider kind (`tsserver` / `tsgo`) carried on the structural
    /// respawn notification, matching the backend's own user label.
    kind: &'static str,
}

impl LspNotifier {
    pub fn new(client: Arc<OnceCell<Client>>, kind: &'static str) -> Self {
        Self { client, kind }
    }
}

impl verter_type_runtime::resilient::ProviderNotifier for LspNotifier {
    fn notify(&self, severity: verter_type_runtime::resilient::NotifySeverity, message: String) {
        use verter_type_runtime::resilient::NotifySeverity;

        let client = self.client.clone();
        // Spawn a task to send the notification (ProviderNotifier::notify is sync)
        tokio::spawn(async move {
            if let Some(client) = client.get() {
                let msg_type = match severity {
                    NotifySeverity::Info => tower_lsp_server::ls_types::MessageType::INFO,
                    NotifySeverity::Warning => tower_lsp_server::ls_types::MessageType::WARNING,
                    NotifySeverity::Error => tower_lsp_server::ls_types::MessageType::ERROR,
                };
                client.show_message(msg_type, message).await;
            }
        });
    }

    fn provider_started(&self, pid: Option<u32>) {
        // No pid, no notification: the contract carries a real child process
        // id, and fabricating one would make a restart look like a fresh start
        // against a process that does not exist.
        let Some(pid) = pid else {
            tracing::warn!("{} respawned without a reportable child pid", self.kind);
            return;
        };
        let client = self.client.clone();
        let kind = self.kind.to_string();
        tokio::spawn(async move {
            if let Some(client) = client.get() {
                client
                    .send_notification::<crate::server::protocol_types::TypeProviderStarted>(
                        crate::server::protocol_types::TypeProviderStartedParams { pid, kind },
                    )
                    .await;
            }
        });
    }
}
