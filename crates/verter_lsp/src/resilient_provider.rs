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
}

impl LspNotifier {
    pub fn new(client: Arc<OnceCell<Client>>) -> Self {
        Self { client }
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
}
