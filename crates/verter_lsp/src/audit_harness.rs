//! LSP-side audit harness — drives the per-handler audit and
//! cancellation contract.
//!
//! Each `handle_<method>` function has a sibling `handle_<method>_with_audit`
//! variant that:
//!
//! 1. Resolves the tagged target identity from the request URI.
//! 2. Opens an [`verter_session::host_lsp_audit::LspAuditSession`] keyed by
//!    [`verter_audit::payloads::tags::LspMethodTag`] and target identity.
//! 3. Runs the handler under the same explicitly configured request deadline
//!    whether audit is enabled or disabled. The audit SLO is observational.
//! 4. On `Ok(value)`: assembles a populated
//!    [`verter_audit::LspRequestPayload`] (response size, position,
//!    enumerable counts) and finalises the session with
//!    `finalize_ok`.
//! 5. Records requests that exceed the audit SLO without cancelling them.
//! 6. Drains the published record into `VERTER_LSP_AUDIT_TRACE_OUT`
//!    when the env var is set.
//!
//! The harness is the only call site for `lsp_audit_begin`, so the
//! audit-config consumer filter and audit-disabled fast-path stay
//! centralised.

use std::sync::Arc;

use tower_lsp_server::ls_types::{Position, Uri};
use verter_audit::payloads::tags::LspMethodTag;
use verter_audit::{LspRequestPayload, RequestAuditRecord, RequestTargetIdentity};
use verter_session::host_lsp_audit::LspAuditSession;
use verter_session::VerterHost;

/// Resolve the audit target identity for `uri`.
///
/// Registered documents carry the exact production registry identity.
/// Unregistered documents carry the raw request URI so request-before-open
/// traffic remains correlatable without predicting a later canonical form.
pub fn target_identity_for_uri(
    documents: &crate::documents::DocumentRegistry,
    uri: &Uri,
) -> RequestTargetIdentity {
    match documents.get_canonical_id(uri) {
        Some(canonical_id) => RequestTargetIdentity::RegisteredCanonical(canonical_id),
        None => RequestTargetIdentity::UnregisteredUri(uri.as_str().to_string()),
    }
}

/// Build an [`LspAuditSession`] for the request. Returns
/// [`LspAuditSession::Noop`] when audit is disabled or the consumer
/// filter rejects the kind.
pub fn begin(
    host: &Arc<VerterHost>,
    method: LspMethodTag,
    target_identity: RequestTargetIdentity,
) -> LspAuditSession {
    host.lsp_audit_begin(method, target_identity)
}

/// Build a `Position`-bound payload base for a hover / goto-def /
/// completion / inlay-hint style handler. The caller may overwrite
/// `response_size_bytes` and the optional count fields after the
/// handler body produces a result.
pub fn payload_with_position(
    method: LspMethodTag,
    target_identity: &RequestTargetIdentity,
    position: &Position,
) -> LspRequestPayload {
    LspRequestPayload {
        method,
        position: Some(verter_audit::payloads::lsp::PositionInfo {
            canonical_id: target_identity.legacy_canonical_id().to_string(),
            target_identity: Some(target_identity.clone()),
            line: position.line,
            character: position.character,
        }),
        ..LspRequestPayload::default()
    }
}

/// Drain a finalised record to `VERTER_LSP_AUDIT_TRACE_OUT` when the
/// env var is set. Mirrors the existing
/// `VERTER_COMPONENT_META_AUDIT_JSON_OUT` drainer in
/// `verter_session::component_meta_audit`. Append-only — multiple
/// records over the lifetime of one process are concatenated as
/// JSON-lines so downstream tools can stream them.
pub fn drain_to_trace_out(record: &RequestAuditRecord) {
    let path = match std::env::var("VERTER_LSP_AUDIT_TRACE_OUT") {
        Ok(p) if !p.is_empty() => p,
        _ => return,
    };
    let serialized = match serde_json::to_string(record) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("VERTER_LSP_AUDIT_TRACE_OUT serialise failed: {e}");
            return;
        }
    };
    use std::io::Write as _;
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut f) => {
            let _ = writeln!(f, "{serialized}");
        }
        Err(e) => tracing::warn!("VERTER_LSP_AUDIT_TRACE_OUT open `{path}` failed: {e}"),
    }
}

/// Run a handler body under an explicitly configured deadline, failing it closed
/// on expiry. A zero deadline is the production default and means unbounded.
///
/// [`run_with_audit`] funnels through here too, so there is one place where a
/// configured diagnostic deadline is applied and one place where it becomes an
/// ambient scope for provider hops underneath.
pub async fn run_with_deadline<T, F>(
    deadline: std::time::Duration,
    body: F,
) -> tower_lsp_server::jsonrpc::Result<T>
where
    F: std::future::Future<Output = tower_lsp_server::jsonrpc::Result<T>>,
{
    if deadline.is_zero() {
        return body.await;
    }
    match verter_type_runtime::deadline::with_deadline(
        deadline,
        tokio::time::timeout(deadline, body),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(tower_lsp_server::jsonrpc::Error::request_cancelled()),
    }
}

/// Run an async handler body under audit and any explicitly configured request
/// deadline.
///
/// `method`, `target_identity`, and `position` are used to construct the audit
/// session and the position-bound payload base. `method` also selects the
/// observational audit SLO and the explicit diagnostic/test deadline from
/// [`verter_session::types::LspMethodTimeoutsConfig`]. `body` is the handler
/// future; `populate` merges the handler's result into the payload.
///
/// On success: finalises with the merged payload.
/// On explicit configured timeout: finalises with the RPC error.
/// On audit-disabled: runs `body` directly unless a non-default explicit bound
/// was supplied by a diagnostic/test host configuration.
///
/// Audit never changes request semantics. In particular, enabling audit cannot
/// introduce a provider or feature timeout that is absent in normal operation.
pub async fn run_with_audit<T, F, P>(
    host: &Arc<VerterHost>,
    method: LspMethodTag,
    target_identity: RequestTargetIdentity,
    position: Option<Position>,
    body: F,
    populate: P,
) -> tower_lsp_server::jsonrpc::Result<T>
where
    F: std::future::Future<Output = tower_lsp_server::jsonrpc::Result<T>>,
    P: FnOnce(&mut LspRequestPayload, &T),
{
    let timeouts = &host.config().lsp_method_timeouts;
    let deadline = timeouts.request_deadlines.for_method(&method);
    let budget = timeouts.audit_supersede.for_method(&method);

    if !host.config().audit_enabled {
        // Production defaults this table to zero. Explicit diagnostic/test
        // configurations may still install a bound here.
        return run_with_deadline(deadline, body).await;
    }

    let mut base_payload = match position.as_ref() {
        Some(pos) => payload_with_position(method.clone(), &target_identity, pos),
        None => LspRequestPayload {
            method: method.clone(),
            ..LspRequestPayload::default()
        },
    };
    let session = begin(host, method.clone(), target_identity);

    let started = std::time::Instant::now();
    let outcome = run_with_deadline(deadline, body).await;
    let elapsed = started.elapsed();
    if !budget.is_zero() && elapsed > budget {
        tracing::warn!(
            ?method,
            ?elapsed,
            ?budget,
            "audited LSP request exceeded its observational SLO"
        );
    }

    match outcome {
        Ok(value) => {
            populate(&mut base_payload, &value);
            if let Some(record) = session.finalize_ok(base_payload) {
                drain_to_trace_out(&record);
            }
            Ok(value)
        }
        Err(rpc_err) => {
            // Surface RPC errors verbatim, but still finalise with
            // the populated payload (no result to populate, but the
            // method/position are already set). The leak guard
            // requires every Active session reach finalise once.
            base_payload.error = Some(format!("rpc-error: {rpc_err}"));
            if let Some(record) = session.finalize_ok(base_payload) {
                drain_to_trace_out(&record);
            }
            Err(rpc_err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::documents::DocumentRegistry;
    use tower_lsp_server::ls_types::TextDocumentItem;
    use verter_audit::RequestTargetIdentity;
    use verter_session::HostConfig;

    #[test]
    fn target_identity_for_file_uri_matches_production_document_identity() {
        let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        let documents = DocumentRegistry::new(Arc::clone(&host));
        let uri: Uri = "file:///C:/Users/dev/my%20project/App.ts"
            .parse()
            .expect("fixture URI must parse");

        let _ = documents.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "typescript".to_string(),
            version: 1,
            text: "export const value = 1;".to_string(),
        });

        let production_id = documents
            .get_canonical_id(&uri)
            .expect("production registry must record the open document");
        let audit_identity = target_identity_for_uri(&documents, &uri);

        assert_eq!(
            audit_identity,
            RequestTargetIdentity::RegisteredCanonical(production_id.clone())
        );
        assert_eq!(production_id, "c:/Users/dev/my project/App.ts");
        assert_ne!(production_id, uri.as_str());
    }

    #[test]
    fn target_identity_for_encoded_virtual_uri_matches_production_document_identity() {
        let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        let documents = DocumentRegistry::new(Arc::clone(&host));
        let uri: Uri =
            "verter-virtual:///ide.tsx?sourceUri=file%3A%2F%2F%2FC%3A%2FUsers%2Fdev%2FApp.vue"
                .parse()
                .expect("fixture URI must parse");

        let _ = documents.did_open(&TextDocumentItem {
            uri: uri.clone(),
            language_id: "typescriptreact".to_string(),
            version: 1,
            text: "export default {};".to_string(),
        });

        let production_id = documents
            .get_canonical_id(&uri)
            .expect("production registry must record the open virtual document");
        let audit_identity = target_identity_for_uri(&documents, &uri);

        assert_eq!(
            audit_identity,
            RequestTargetIdentity::RegisteredCanonical(production_id.clone())
        );
        assert_eq!(production_id, uri.as_str());
        assert_ne!(
            production_id,
            "verter-virtual:///ide.tsx?sourceUri=file:///C:/Users/dev/App.vue"
        );
    }

    #[test]
    fn unregistered_uris_produce_distinct_envelope_and_position_identities() {
        let host = Arc::new(VerterHost::new_standalone(HostConfig {
            audit_enabled: true,
            ..HostConfig::default()
        }));
        let documents = DocumentRegistry::new(Arc::clone(&host));
        let uri_a: Uri = "file:///C:/Users/dev/A.vue"
            .parse()
            .expect("fixture URI must parse");
        let uri_b: Uri = "file:///C:/Users/dev/B.vue"
            .parse()
            .expect("fixture URI must parse");

        assert_eq!(documents.get_canonical_id(&uri_a), None);
        assert_eq!(documents.get_canonical_id(&uri_b), None);

        let identity_a = target_identity_for_uri(&documents, &uri_a);
        let identity_b = target_identity_for_uri(&documents, &uri_b);
        assert_eq!(
            identity_a,
            RequestTargetIdentity::UnregisteredUri(uri_a.as_str().to_string())
        );
        assert_eq!(
            identity_b,
            RequestTargetIdentity::UnregisteredUri(uri_b.as_str().to_string())
        );
        assert_ne!(identity_a, identity_b);
        assert_ne!(identity_a, RequestTargetIdentity::NotApplicable);
        assert_ne!(identity_b, RequestTargetIdentity::NotApplicable);

        let position = Position {
            line: 4,
            character: 7,
        };
        let record_a = begin(&host, LspMethodTag::Completion, identity_a.clone())
            .finalize_ok(payload_with_position(
                LspMethodTag::Completion,
                &identity_a,
                &position,
            ))
            .expect("first audit session must publish");
        let record_b = begin(&host, LspMethodTag::Completion, identity_b.clone())
            .finalize_ok(payload_with_position(
                LspMethodTag::Completion,
                &identity_b,
                &position,
            ))
            .expect("second audit session must publish");

        assert_eq!(record_a.target_identity.as_ref(), Some(&identity_a));
        assert_eq!(record_b.target_identity.as_ref(), Some(&identity_b));
        assert_ne!(record_a.target_identity, record_b.target_identity);
        assert_eq!(record_a.canonical_id, "");
        assert_eq!(record_b.canonical_id, "");

        let position_a = record_a
            .lsp_payload()
            .and_then(|payload| payload.position.as_ref())
            .expect("first position identity must be present");
        let position_b = record_b
            .lsp_payload()
            .and_then(|payload| payload.position.as_ref())
            .expect("second position identity must be present");
        assert_eq!(position_a.target_identity, record_a.target_identity);
        assert_eq!(position_b.target_identity, record_b.target_identity);
        assert_ne!(position_a.target_identity, position_b.target_identity);
        assert_eq!(position_a.canonical_id, "");
        assert_eq!(position_b.canonical_id, "");
    }
}
