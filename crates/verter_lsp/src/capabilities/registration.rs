//! Mask-derived dynamic registration of withdrawable capability cells.
//!
//! A method the client can register dynamically is not advertised
//! statically. The server registers it after `initialized` while some served
//! profile admits its interactive demand, and unregisters it when none does,
//! so a fully withdrawn cell leaves the client. A partially withdrawn cell
//! (one profile, document or region) stays registered and is refused by the
//! server with its typed reason. A method the client can only receive
//! statically keeps its static advertisement and the server-side refusal:
//! that is the transport's truthful limit.

use std::collections::BTreeSet;

use serde_json::Value;
use tower_lsp_server::ls_types::{
    ClientCapabilities, Registration, ServerCapabilities, Unregistration,
};
use verter_language::participation::CapabilityOperation as Op;

use crate::documents::DocumentRegistry;

/// One dynamically registrable method and the cells it serves.
struct DynamicMethod {
    method: &'static str,
    /// The `ServerCapabilities` member its static advertisement lives in.
    capability: &'static str,
    /// The client capability path whose `dynamicRegistration` it needs.
    client: [&'static str; 2],
    /// Whether its registration options carry a document selector.
    document_scoped: bool,
    /// Registered while any of these cells admits an interactive demand.
    operations: &'static [Op],
}

const fn document(
    method: &'static str,
    capability: &'static str,
    client: &'static str,
    operations: &'static [Op],
) -> DynamicMethod {
    DynamicMethod {
        method,
        capability,
        client: ["textDocument", client],
        document_scoped: true,
        operations,
    }
}

const METHODS: &[DynamicMethod] = &[
    document("textDocument/hover", "hoverProvider", "hover", &[Op::Hover]),
    document(
        "textDocument/completion",
        "completionProvider",
        "completion",
        &[Op::Completion],
    ),
    document(
        "textDocument/definition",
        "definitionProvider",
        "definition",
        &[Op::Definition],
    ),
    document(
        "textDocument/typeDefinition",
        "typeDefinitionProvider",
        "typeDefinition",
        &[Op::TypeDefinition],
    ),
    document(
        "textDocument/references",
        "referencesProvider",
        "references",
        &[Op::References],
    ),
    document(
        "textDocument/rename",
        "renameProvider",
        "rename",
        &[Op::Rename, Op::PrepareRename],
    ),
    document(
        "textDocument/documentSymbol",
        "documentSymbolProvider",
        "documentSymbol",
        &[Op::DocumentSymbol],
    ),
    document(
        "textDocument/foldingRange",
        "foldingRangeProvider",
        "foldingRange",
        &[Op::FoldingRange],
    ),
    document(
        "textDocument/selectionRange",
        "selectionRangeProvider",
        "selectionRange",
        &[Op::SelectionRange],
    ),
    document(
        "textDocument/documentHighlight",
        "documentHighlightProvider",
        "documentHighlight",
        &[Op::DocumentHighlight],
    ),
    document(
        "textDocument/signatureHelp",
        "signatureHelpProvider",
        "signatureHelp",
        &[Op::SignatureHelp],
    ),
    document(
        "textDocument/codeAction",
        "codeActionProvider",
        "codeAction",
        &[Op::QuickFix, Op::OrganizeImports, Op::Refactor],
    ),
    document(
        "textDocument/semanticTokens",
        "semanticTokensProvider",
        "semanticTokens",
        &[Op::SemanticTokens],
    ),
    document(
        "textDocument/codeLens",
        "codeLensProvider",
        "codeLens",
        &[Op::CodeLens],
    ),
    document(
        "textDocument/inlayHint",
        "inlayHintProvider",
        "inlayHint",
        &[Op::InlayHint],
    ),
    document(
        "textDocument/linkedEditingRange",
        "linkedEditingRangeProvider",
        "linkedEditingRange",
        &[Op::LinkedEditing],
    ),
    document(
        "textDocument/documentLink",
        "documentLinkProvider",
        "documentLink",
        &[Op::DocumentLink],
    ),
    document(
        "textDocument/documentColor",
        "colorProvider",
        "colorProvider",
        &[Op::DocumentColor, Op::ColorPresentation],
    ),
    document(
        "textDocument/formatting",
        "documentFormattingProvider",
        "formatting",
        &[Op::Formatting],
    ),
    document(
        "textDocument/onTypeFormatting",
        "documentOnTypeFormattingProvider",
        "onTypeFormatting",
        &[Op::OnTypeTagClosing],
    ),
    document(
        "textDocument/prepareCallHierarchy",
        "callHierarchyProvider",
        "callHierarchy",
        &[
            Op::CallHierarchyPrepare,
            Op::CallHierarchyIncoming,
            Op::CallHierarchyOutgoing,
        ],
    ),
    DynamicMethod {
        method: "workspace/symbol",
        capability: "workspaceSymbolProvider",
        client: ["workspace", "symbol"],
        document_scoped: false,
        operations: &[Op::WorkspaceSymbol],
    },
];

/// The registration id of one method: stable, so a later unregistration
/// names exactly the registration it withdraws.
fn registration_id(method: &str) -> String {
    format!("verter-participation:{method}")
}

/// The dynamically registered methods of one session and the subset the
/// client currently holds.
#[derive(Default)]
pub(crate) struct DynamicRegistrations {
    /// Each method the client registers dynamically, with the options its
    /// static advertisement would have carried.
    methods: Vec<(&'static DynamicMethod, Value)>,
    registered: tokio::sync::Mutex<BTreeSet<&'static str>>,
}

impl DynamicRegistrations {
    /// Move every method the client can register dynamically out of the
    /// static advertisement `capabilities`, keeping its options for the
    /// registration.
    pub(crate) fn negotiate(
        client: &ClientCapabilities,
        capabilities: &mut ServerCapabilities,
    ) -> Self {
        let client = serde_json::to_value(client).unwrap_or_default();
        let Ok(Value::Object(mut advertised)) = serde_json::to_value(&*capabilities) else {
            return Self::default();
        };
        let mut methods = Vec::new();
        for method in METHODS {
            let [section, feature] = method.client;
            if client[section][feature]["dynamicRegistration"] != Value::Bool(true) {
                continue;
            }
            let Some(advertisement) = advertised.remove(method.capability) else {
                continue;
            };
            let mut options = match advertisement {
                Value::Object(options) => options,
                Value::Bool(true) => serde_json::Map::new(),
                // Not advertised: nothing to register.
                _ => continue,
            };
            if method.document_scoped {
                // `null`: the client's own document selector applies.
                options.insert("documentSelector".to_owned(), Value::Null);
            }
            methods.push((method, Value::Object(options)));
        }
        if let Ok(stripped) = serde_json::from_value(Value::Object(advertised)) {
            *capabilities = stripped;
        }
        Self {
            methods,
            registered: Default::default(),
        }
    }

    /// The methods the current mask keeps registered: a document method
    /// with some cell some served profile admits for an interactive demand,
    /// and a workspace-wide method every served profile admits.
    pub(crate) fn desired(&self, documents: &DocumentRegistry) -> BTreeSet<&'static str> {
        self.methods
            .iter()
            .filter(|(method, _)| {
                method.operations.iter().any(|&operation| {
                    if method.document_scoped {
                        crate::participation::admitted_anywhere(documents, operation)
                    } else {
                        crate::participation::admitted_workspace_wide(documents, operation)
                    }
                })
            })
            .map(|(method, _)| method.method)
            .collect()
    }

    /// Register what the current mask admits and unregister what it no
    /// longer does. Serialized, so concurrent transitions apply in order; a
    /// request the client refused leaves the recorded state unchanged.
    pub(crate) async fn reconcile(
        &self,
        client: &crate::outbound::Outbound,
        documents: &DocumentRegistry,
    ) {
        if self.methods.is_empty() {
            return;
        }
        let mut registered = self.registered.lock().await;
        let desired = self.desired(documents);
        let added: Vec<&(&'static DynamicMethod, Value)> = self
            .methods
            .iter()
            .filter(|(method, _)| {
                desired.contains(method.method) && !registered.contains(method.method)
            })
            .collect();
        if !added.is_empty() {
            let registrations = added
                .iter()
                .map(|(method, options)| Registration {
                    id: registration_id(method.method),
                    method: method.method.to_owned(),
                    register_options: Some(options.clone()),
                })
                .collect();
            match client.register_capability(registrations).await {
                Ok(()) => registered.extend(added.iter().map(|(method, _)| method.method)),
                Err(error) => {
                    let methods: Vec<&str> =
                        added.iter().map(|(method, _)| method.method).collect();
                    degraded(client, "register", &methods, &error).await;
                }
            }
        }
        let removed: Vec<&'static str> = registered
            .iter()
            .copied()
            .filter(|method| !desired.contains(method))
            .collect();
        if !removed.is_empty() {
            let unregistrations = removed
                .iter()
                .map(|method| Unregistration {
                    id: registration_id(method),
                    method: (*method).to_owned(),
                })
                .collect();
            match client.unregister_capability(unregistrations).await {
                Ok(()) => {
                    for method in &removed {
                        registered.remove(method);
                    }
                }
                Err(error) => degraded(client, "unregister", &removed, &error).await,
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn negotiated(&self) -> Vec<(&'static str, &Value)> {
        self.methods
            .iter()
            .map(|(method, options)| (method.method, options))
            .collect()
    }
}

/// A refused (un)registration is never silent: the session is degraded for
/// those methods until a later reconciliation succeeds, and the user is
/// told. Server-side admission keeps refusing every withdrawn cell either
/// way, so a method left registered still answers its typed refusal.
async fn degraded(
    client: &crate::outbound::Outbound,
    action: &str,
    methods: &[&str],
    error: &tower_lsp_server::jsonrpc::Error,
) {
    let message = format!(
        "verter could not {action} {} with the editor ({}); participation \
         changes for them apply on the next settings change",
        methods.join(", "),
        error.message
    );
    tracing::warn!("{message}");
    client
        .show_message(tower_lsp_server::ls_types::MessageType::WARNING, message)
        .await;
}
