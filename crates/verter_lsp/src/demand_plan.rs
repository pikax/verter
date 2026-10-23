//! The client's declared demand, read once from `initialize`.
//!
//! A server must not spend work or wire bytes on something no client asked
//! for. Everything the server does beyond answering requests and publishing
//! diagnostics for open documents is admitted here, from three inputs only:
//! the client's LSP capabilities, the Verter `initializationOptions` features
//! it enabled, and the recurring notifications it explicitly subscribed to
//! (`initializationOptions.subscriptions`). Anything absent is off.

use std::sync::atomic::{AtomicBool, Ordering};

use tower_lsp_server::ls_types::InitializeParams;

/// Recurring server-initiated notifications a client receives only when it
/// asks for them. One-shot lifecycle notifications (`$/verter/ready`,
/// provider status, …) are not subscriptions: they are bounded by the session,
/// not by time or edit volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Subscriptions {
    /// `$/verter/heartbeat` every few seconds — the liveness signal a client
    /// watchdog restarts the server on.
    pub heartbeat: bool,
    /// `$/verter/carrierStoreChanged` after every carrier publication — for a
    /// client running its own TypeScript service over the carrier store.
    pub carrier_store_changed: bool,
    /// `carrierStoreFill` — the client runs a consumer over the durable carrier
    /// store (VS Code's own TypeScript service via the tsserver plugin) and
    /// asks the server to KEEP that store filled: the negotiated demand that
    /// starts the workspace inventory scan whose publications fill it, through
    /// the same idempotent scanner lifecycle a request's demand uses. Distinct
    /// from `carrier_store_changed` (the refresh NOTIFICATION the filled store's
    /// consumer also wants). Off, like every subscription: a session whose store
    /// would otherwise fill only when a request needs the complete inventory
    /// stays lazy, and a session with no carrier publisher treats it as inert.
    pub carrier_store_fill: bool,
}

impl Subscriptions {
    fn from_init_options(init_options: Option<&serde_json::Value>) -> Self {
        let declared = |name: &str| {
            init_options
                .and_then(|options| options.get("subscriptions"))
                .and_then(|subscriptions| subscriptions.get(name))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
        };
        Self {
            heartbeat: declared("heartbeat"),
            carrier_store_changed: declared("carrierStoreChanged"),
            carrier_store_fill: declared("carrierStoreFill"),
        }
    }
}

/// Everything the connected client declared it wants, negotiated once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LspDemandPlan {
    /// Optional native semantic enrichment (`analysis.enabled` or
    /// `lint.enabled`). Off: no semantic host and no enrichment task exist.
    pub semantic_analysis: bool,
    /// `statistics.enabled`, when the client stated it.
    pub statistics: Option<bool>,
    /// `interactionTrace.enabled`, when the client stated it. Unstated keeps
    /// the process-level trace setting.
    pub interaction_trace: Option<bool>,
    pub subscriptions: Subscriptions,
    /// `workspace.semanticTokens.refreshSupport`.
    pub refreshes_semantic_tokens: bool,
    /// `workspace.inlayHint.refreshSupport`.
    pub refreshes_inlay_hints: bool,
    /// The client applies versioned `documentChanges`.
    pub applies_versioned_edits: bool,
}

impl LspDemandPlan {
    pub fn from_initialize(params: &InitializeParams) -> Self {
        let options = params.initialization_options.as_ref();
        let feature = |name: &str| {
            options
                .and_then(|options| options.get(name))
                .and_then(|feature| feature.get("enabled"))
                .and_then(serde_json::Value::as_bool)
        };
        let workspace = params.capabilities.workspace.as_ref();
        Self {
            semantic_analysis: options.is_some_and(|options| {
                crate::config::parse_native_semantic_options(options).enabled
            }),
            statistics: feature("statistics"),
            interaction_trace: feature("interactionTrace"),
            subscriptions: Subscriptions::from_init_options(options),
            refreshes_semantic_tokens: workspace
                .and_then(|workspace| workspace.semantic_tokens.as_ref())
                .and_then(|tokens| tokens.refresh_support)
                .unwrap_or(false),
            refreshes_inlay_hints: workspace
                .and_then(|workspace| workspace.inlay_hint.as_ref())
                .and_then(|hints| hints.refresh_support)
                .unwrap_or(false),
            applies_versioned_edits: crate::features::action_utils::WorkspaceEditSupport::negotiate(
                &params.capabilities,
            )
                == crate::features::action_utils::WorkspaceEditSupport::VersionedDocumentChanges,
        }
    }
}

/// The negotiated subscriptions, readable by every producer of a recurring
/// notification. Nothing is subscribed until `initialize` installs the plan.
#[derive(Debug, Default)]
pub struct ClientSubscriptions {
    heartbeat: AtomicBool,
    carrier_store_changed: AtomicBool,
    carrier_store_fill: AtomicBool,
}

impl ClientSubscriptions {
    pub fn install(&self, subscriptions: Subscriptions) {
        self.heartbeat
            .store(subscriptions.heartbeat, Ordering::Release);
        self.carrier_store_changed
            .store(subscriptions.carrier_store_changed, Ordering::Release);
        self.carrier_store_fill
            .store(subscriptions.carrier_store_fill, Ordering::Release);
    }

    pub fn heartbeat(&self) -> bool {
        self.heartbeat.load(Ordering::Acquire)
    }

    pub fn carrier_store_changed(&self) -> bool {
        self.carrier_store_changed.load(Ordering::Acquire)
    }

    pub fn carrier_store_fill(&self) -> bool {
        self.carrier_store_fill.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan(params: serde_json::Value) -> LspDemandPlan {
        LspDemandPlan::from_initialize(&serde_json::from_value(params).unwrap())
    }

    #[test]
    fn a_client_that_declares_nothing_demands_nothing() {
        assert_eq!(
            plan(serde_json::json!({ "capabilities": {} })),
            LspDemandPlan::default()
        );
        // Unrelated or malformed options enable nothing either.
        assert_eq!(
            plan(serde_json::json!({
                "capabilities": {},
                "initializationOptions": {
                    "subscriptions": { "heartbeat": "yes", "carrierStoreChanged": 1 },
                    "hover": { "nativeSemantics": true }
                }
            })),
            LspDemandPlan::default()
        );
    }

    #[test]
    fn each_declared_demand_enables_only_itself() {
        let declared = plan(serde_json::json!({
            "capabilities": {
                "workspace": {
                    "semanticTokens": { "refreshSupport": true },
                    "workspaceEdit": { "documentChanges": true }
                }
            },
            "initializationOptions": {
                "analysis": { "enabled": true },
                "interactionTrace": { "enabled": false },
                "subscriptions": { "carrierStoreChanged": true }
            }
        }));
        assert_eq!(
            declared,
            LspDemandPlan {
                semantic_analysis: true,
                statistics: None,
                interaction_trace: Some(false),
                subscriptions: Subscriptions {
                    heartbeat: false,
                    carrier_store_changed: true,
                    carrier_store_fill: false,
                },
                refreshes_semantic_tokens: true,
                refreshes_inlay_hints: false,
                applies_versioned_edits: true,
            }
        );
    }

    #[test]
    fn installed_subscriptions_replace_the_previous_negotiation() {
        let gate = ClientSubscriptions::default();
        assert!(!gate.heartbeat() && !gate.carrier_store_changed());
        gate.install(Subscriptions {
            heartbeat: true,
            carrier_store_changed: true,
            carrier_store_fill: true,
        });
        assert!(gate.heartbeat() && gate.carrier_store_changed());
        gate.install(Subscriptions::default());
        assert!(!gate.heartbeat() && !gate.carrier_store_changed());
        assert!(!gate.carrier_store_fill());
    }
}
