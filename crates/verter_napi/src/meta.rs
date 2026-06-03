//! NAPI bindings for component-meta.
//!
//! The public JS surface keeps the existing `MetaProject` / `MetaSession`
//! class names for now, but those names now wrap the native
//! `ComponentMetaHost` / isolated session layer.

use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi::{Error, Status};
use napi_derive::napi;
use serde::{Deserialize, Serialize};
use verter_protocol::types::{FfiComponentMeta, FfiComponentMetaResolution};
use verter_session::component_meta_audit::{
    assertions::RequestAuditRecordAssertions, RequestAuditRecord,
};
use verter_session::component_meta_host::{
    ComponentMetaHost, ComponentMetaHostError, ComponentMetaSession as HostComponentMetaSession,
};

use crate::{buffer_to_string, catch_panic, NapiHostConfig, NapiIdeProjectConfig};

fn meta_err(e: ComponentMetaHostError) -> Error {
    Error::new(Status::GenericFailure, e.to_string())
}

/// Shared encode function passed to session payload methods.
fn encode_meta_payload(
    analysis: verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    resolved: &verter_session::meta_resolve::ResolvedComponentMetaState,
) -> Vec<u8> {
    let ffi = verter_ffi::convert::component_meta_analysis_to_ffi_with_resolution(
        analysis,
        Some(resolved),
    );
    verter_protocol::component_meta::encode_component_meta_payload(&ffi)
}

/// JSON bundle emitted by `getComponentMetaWithAudit`. Three lanes:
/// - `analysis`: FFI projection of `ComponentMetaAnalysis` — `FfiComponentMeta`
///   already derives `Serialize` (camelCase).
/// - `resolution`: FFI projection of `ResolvedComponentMetaState` —
///   `FfiComponentMetaResolution` (camelCase).
/// - `record`: `RequestAuditRecord` (ts-rs–generated type surface; u64/i64
///   fields transport as decimal strings).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuditBundle {
    analysis: FfiComponentMeta,
    resolution: FfiComponentMetaResolution,
    record: RequestAuditRecord,
}

/// Minimal decoder for `whyLoadedFromAuditJson` / `whyInstantiatedFromAuditJson`.
/// Only the `record` field is consumed; the `analysis` and
/// `resolution` fields present in the JSON bundle are silently
/// ignored by serde's default "unknown fields" handling, so the
/// struct omits them entirely (no dead-code fields to carry).
#[derive(Deserialize)]
struct AuditBundleForWalker {
    record: RequestAuditRecord,
}

/// Parse a 32-character lowercase hex string into a
/// `verter_session::types::Hash16`. Returns a NAPI error on malformed
/// input.
fn parse_hash16_hex(hex: &str) -> Result<verter_session::Hash16> {
    if hex.len() != 32 {
        return Err(Error::new(
            Status::InvalidArg,
            format!(
                "args_fingerprint_hex must be 32 hex chars (16 bytes), got {} chars",
                hex.len()
            ),
        ));
    }
    let mut out = [0u8; 16];
    for (i, byte_out) in out.iter_mut().enumerate() {
        let hi = hex
            .as_bytes()
            .get(i * 2)
            .and_then(|c| (*c as char).to_digit(16))
            .ok_or_else(|| {
                Error::new(
                    Status::InvalidArg,
                    format!("args_fingerprint_hex[{idx}] not a hex digit", idx = i * 2),
                )
            })?;
        let lo = hex
            .as_bytes()
            .get(i * 2 + 1)
            .and_then(|c| (*c as char).to_digit(16))
            .ok_or_else(|| {
                Error::new(
                    Status::InvalidArg,
                    format!(
                        "args_fingerprint_hex[{idx}] not a hex digit",
                        idx = i * 2 + 1
                    ),
                )
            })?;
        *byte_out = ((hi << 4) | lo) as u8;
    }
    Ok(out)
}

#[napi(js_name = "MetaProject")]
pub struct NapiMetaProject {
    inner: Arc<ComponentMetaHost>,
}

#[napi]
impl NapiMetaProject {
    #[napi(constructor)]
    pub fn new(config: Option<NapiHostConfig>) -> Result<Self> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let ffi_config: verter_ffi::types::FfiHostConfig = config.unwrap_or_default().into();
            let host_config =
                verter_ffi::convert::ffi_config_to_host(ffi_config).map_err(crate::ffi_err)?;
            Ok(NapiMetaProject {
                inner: Arc::new(ComponentMetaHost::new_standalone(host_config)),
            })
        }))?
    }

    #[napi(factory, js_name = "withWorkspace")]
    pub fn with_workspace(
        config: Option<NapiHostConfig>,
        workspace: &crate::NapiWorkspace,
    ) -> Result<Self> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let ffi_config: verter_ffi::types::FfiHostConfig = config.unwrap_or_default().into();
            let host_config =
                verter_ffi::convert::ffi_config_to_host(ffi_config).map_err(crate::ffi_err)?;
            let ws: Arc<dyn verter_workspace::WorkspaceAccess> = workspace.workspace();
            Ok(NapiMetaProject {
                inner: Arc::new(ComponentMetaHost::new(host_config, ws)),
            })
        }))?
    }

    #[napi(js_name = "upsertBase")]
    pub fn upsert_base(&self, canonical_id: String, source: Buffer) -> Result<()> {
        let source = buffer_to_string(source)?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner
                .upsert_base(&canonical_id, &source)
                .map_err(meta_err)
        }))?
    }

    #[napi(js_name = "ensureLoaded")]
    pub fn ensure_loaded(&self, canonical_id: String) -> Result<bool> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.ensure_loaded(&canonical_id).map_err(meta_err)
        }))?
    }

    #[napi(js_name = "refreshBase")]
    pub fn refresh_base(&self, canonical_id: String) -> Result<bool> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.refresh_base(&canonical_id).map_err(meta_err)
        }))?
    }

    #[napi(js_name = "configureProjects")]
    pub fn configure_projects(&self, projects: Vec<NapiIdeProjectConfig>) -> Result<()> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let configs: Vec<verter_semantic::analysis::project_resolver::IdeProjectConfig> =
                projects
                    .into_iter()
                    .map(crate::napi_project_config_to_ide)
                    .collect();
            self.inner.configure_projects(configs).map_err(meta_err)
        }))?
    }

    #[napi(js_name = "openSession")]
    pub fn open_session(&self) -> Result<NapiMetaSession> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let session = self.inner.open_session().map_err(meta_err)?;
            Ok(NapiMetaSession {
                inner: Some(session),
            })
        }))?
    }

    #[napi(js_name = "clearCaches")]
    pub fn clear_caches(&self) -> Result<()> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.clear_caches().map_err(meta_err)
        }))?
    }

    #[napi]
    pub fn shutdown(&self) -> Result<()> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.shutdown();
        }))
    }

    #[napi(js_name = "isShutdown", getter)]
    pub fn is_shutdown(&self) -> bool {
        self.inner.is_shutdown()
    }

    #[napi(js_name = "sessionCount", getter)]
    pub fn session_count(&self) -> u32 {
        self.inner.session_count() as u32
    }

    #[napi(js_name = "baseFileIds")]
    pub fn base_file_ids(&self) -> Vec<String> {
        self.inner.base_file_ids()
    }
}

#[napi(js_name = "MetaSession")]
pub struct NapiMetaSession {
    inner: Option<HostComponentMetaSession>,
}

impl NapiMetaSession {
    fn session(&self) -> Result<&HostComponentMetaSession> {
        self.inner
            .as_ref()
            .ok_or_else(|| Error::new(Status::GenericFailure, "session is closed"))
    }
}

#[napi]
impl NapiMetaSession {
    #[napi(constructor)]
    pub fn new() -> Result<Self> {
        Err(Error::new(
            Status::GenericFailure,
            "MetaSession cannot be constructed directly. Use MetaProject.openSession().",
        ))
    }

    #[napi]
    pub fn upsert(&self, canonical_id: String, source: Buffer) -> Result<()> {
        let session = self.session()?;
        let source = buffer_to_string(source)?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            session.upsert(&canonical_id, source).map_err(meta_err)
        }))?
    }

    #[napi]
    pub fn delete(&self, canonical_id: String) -> Result<()> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            session.delete(&canonical_id).map_err(meta_err)
        }))?
    }

    #[napi(js_name = "reset")]
    pub fn reset(&self, canonical_id: String) -> Result<()> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            session.reset(&canonical_id).map_err(meta_err)
        }))?
    }

    #[napi(js_name = "getEffectiveSource")]
    pub fn get_effective_source(&self, canonical_id: String) -> Result<Option<String>> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            session
                .get_effective_source(&canonical_id)
                .map_err(meta_err)
        }))?
    }

    #[napi(js_name = "hasFile")]
    pub fn has_file(&self, canonical_id: String) -> Result<bool> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            session.has_file(&canonical_id).map_err(meta_err)
        }))?
    }

    #[napi(js_name = "getComponentMeta")]
    pub fn get_component_meta(&self, canonical_or_alias: String) -> Result<Option<Buffer>> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let payload = session
                .get_component_meta_payload(&canonical_or_alias, encode_meta_payload)
                .map_err(meta_err)?;
            Ok(payload.map(Buffer::from))
        }))?
    }

    /// Batch surface for `getComponentMeta`: compute metadata for
    /// `canonicalsOrAliases` under one shared overlay view. The host batch
    /// coordinator fans the N queries out on the host-owned `HostCpuPool`
    /// and accounts the submission once per non-empty batch (skipped for
    /// an empty batch). Returns one
    /// slot per input in input order; a missing slot is encoded as a
    /// sentinel zero-length `Buffer`
    /// (a real payload always contains the FFI envelope, so JS readers
    /// can use `buf.length === 0` as the canonical "no result"
    /// sentinel). Per-id failures (budget overruns, alias errors)
    /// surface as the sentinel as well.
    ///
    /// Throws only on project-level shutdown.
    #[napi(js_name = "getComponentMetaBatch")]
    pub fn get_component_meta_batch(
        &self,
        canonicals_or_aliases: Vec<String>,
    ) -> Result<Vec<Buffer>> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let payloads = session
                .get_component_meta_batch_payloads(&canonicals_or_aliases, encode_meta_payload)
                .map_err(meta_err)?;
            Ok(payloads
                .into_iter()
                .map(|slot| match slot {
                    Some(bytes) => Buffer::from(bytes),
                    None => Buffer::from(Vec::<u8>::new()),
                })
                .collect())
        }))?
    }

    /// Synchronous audit bundle — returns JSON bytes of
    /// `{ analysis: FfiComponentMeta, resolution: FfiComponentMetaResolution,
    ///   record: RequestAuditRecord }`. Requires the host to have
    /// `audit_enabled` + `footprint_capture` set on construction.
    ///
    /// NOT async. Audit capture completes on the same call that produces
    /// the analysis; there is no background work to await. Consumer-side
    /// Promise wrapping (if desired) lives in
    /// `packages/native/audit.ts`.
    #[napi(js_name = "getComponentMetaWithAudit")]
    pub fn get_component_meta_with_audit(
        &self,
        canonical_or_alias: String,
    ) -> Result<Option<Buffer>> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let (outcome, record) = session
                .get_component_meta_with_audit(&canonical_or_alias)
                .into_parts();
            let Some((analysis, resolution)) = outcome.map_err(meta_err)? else {
                return Ok(None);
            };
            let ffi = verter_ffi::convert::component_meta_analysis_to_ffi_with_resolution(
                analysis,
                Some(&resolution),
            );
            let ffi_resolution = verter_ffi::convert::component_meta_resolution_to_ffi(&resolution);
            let bundle = AuditBundle {
                analysis: ffi,
                resolution: ffi_resolution,
                record,
            };
            let json = serde_json::to_vec(&bundle).map_err(|e| {
                Error::new(
                    Status::GenericFailure,
                    format!("audit bundle serialization error: {e}"),
                )
            })?;
            Ok(Some(Buffer::from(json)))
        }))?
    }

    /// Run the Rust walker against a committed audit record (supplied
    /// as JSON from a prior `getComponentMetaWithAudit`) rooted at the
    /// given `canonical_id`. Returns the `ProvenanceChain` encoded as
    /// JSON. Single-walker rule — TS helpers format the JSON via pure
    /// rendering; they do not re-walk the graph.
    #[napi(js_name = "whyLoadedFromAuditJson")]
    pub fn why_loaded_from_audit_json(
        &self,
        audit_json: String,
        canonical_id: String,
    ) -> Result<String> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            // The input JSON is an AuditBundle. Decode just the record.
            let bundle: AuditBundleForWalker = serde_json::from_str(&audit_json).map_err(|e| {
                Error::new(
                    Status::GenericFailure,
                    format!("audit_json is not a valid AuditBundle: {e}"),
                )
            })?;
            let chain = bundle.record.why_loaded(&canonical_id);
            serde_json::to_string(&chain).map_err(|e| {
                Error::new(
                    Status::GenericFailure,
                    format!("chain serialization error: {e}"),
                )
            })
        }))?
    }

    /// Run the Rust walker against a committed audit record rooted at
    /// the instantiation keyed by `(decl_canonical_id, decl_symbol_name,
    /// args_fingerprint_hex)`. Returns the `ProvenanceChain` encoded as
    /// JSON. `args_fingerprint_hex` is the 32-character lowercase hex
    /// rendering of the 16-byte `Hash16`.
    #[napi(js_name = "whyInstantiatedFromAuditJson")]
    pub fn why_instantiated_from_audit_json(
        &self,
        audit_json: String,
        decl_canonical_id: String,
        decl_symbol_name: String,
        args_fingerprint_hex: String,
    ) -> Result<String> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let bundle: AuditBundleForWalker = serde_json::from_str(&audit_json).map_err(|e| {
                Error::new(
                    Status::GenericFailure,
                    format!("audit_json is not a valid AuditBundle: {e}"),
                )
            })?;
            let fingerprint = parse_hash16_hex(&args_fingerprint_hex)?;
            let chain =
                bundle
                    .record
                    .why_instantiated(&decl_canonical_id, &decl_symbol_name, fingerprint);
            serde_json::to_string(&chain).map_err(|e| {
                Error::new(
                    Status::GenericFailure,
                    format!("chain serialization error: {e}"),
                )
            })
        }))?
    }

    #[napi(js_name = "getResolvedComponentMeta")]
    pub fn get_resolved_component_meta(
        &self,
        canonical_or_alias: String,
    ) -> Result<Option<Buffer>> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let payload = session
                .get_component_meta_payload(&canonical_or_alias, encode_meta_payload)
                .map_err(meta_err)?;
            Ok(payload.map(Buffer::from))
        }))?
    }

    #[napi(js_name = "getProvenance")]
    pub fn get_provenance(&self) -> Result<String> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let snapshot = session.get_provenance().map_err(meta_err)?;
            serde_json::to_string(&snapshot).map_err(|e| {
                Error::new(
                    Status::GenericFailure,
                    format!("provenance serialization error: {e}"),
                )
            })
        }))?
    }

    #[napi(js_name = "trackedFileIds")]
    pub fn tracked_file_ids(&self) -> Result<Vec<String>> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            session.tracked_file_ids().map_err(meta_err)
        }))?
    }

    #[napi]
    pub fn close(&mut self) -> Result<()> {
        if let Some(session) = self.inner.take() {
            session.close();
        }
        Ok(())
    }

    #[napi(js_name = "isClosed", getter)]
    pub fn is_closed(&self) -> bool {
        self.inner
            .as_ref()
            .is_none_or(|session| session.is_closed())
    }

    #[napi(js_name = "overlayGeneration", getter)]
    pub fn overlay_generation(&self) -> u32 {
        self.inner
            .as_ref()
            .map_or(0, |session| session.overlay_generation() as u32)
    }

    /// Tier 1B selective surface API (D32 + D63). Returns the
    /// `verter.v1.ComponentMetaSurface` proto bytes, or `None` when the
    /// canonical does not resolve. Errors are surfaced via NAPI's
    /// existing exception path; D114 magic-byte error envelopes are
    /// not produced from this entry-point because the surface envelope
    /// itself is a top-level shape, not a per-handle BFS frontier.
    #[napi]
    pub fn get_component_meta_surface(&self, canonical_or_alias: String) -> Result<Option<Buffer>> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let surface = session
                .get_component_meta_surface(&canonical_or_alias)
                .map_err(meta_err)?;
            Ok(surface.map(|s| Buffer::from(s.to_proto_bytes())))
        }))?
    }

    /// Tier 1B selective surface API (D32 + D63). Resolves a
    /// `verter.v1.TypeHandle` (decoded from `handle_buf`) to a one-layer
    /// `verter.v1.TypeExpansion`. Returns the encoded
    /// `verter.v1.TypeExpansion` proto bytes on success, or a magic-byte
    /// (`0xFF`) prefixed `verter.v1.TypeHandleError` envelope on error
    /// (D114).
    #[napi]
    pub fn get_component_meta_type_expansion(
        &self,
        handle_buf: Buffer,
        depth: Option<u32>,
    ) -> Result<Buffer> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            use prost::Message;
            let proto_handle = verter_protocol::verter::v1::TypeHandle::decode(handle_buf.as_ref())
                .map_err(|e| Error::from_reason(format!("invalid TypeHandle: {e}")))?;
            let handle =
                verter_session::component_meta_payload::TypeHandle::from_proto(proto_handle)
                    .map_err(|e| Error::from_reason(format!("invalid TypeHandle: {e}")))?;
            match session.get_component_meta_type_expansion(handle, depth.map(|d| d as usize)) {
                Ok(expansion) => Ok(Buffer::from(expansion.to_proto().encode_to_vec())),
                Err(typed) => Ok(Buffer::from(typed.to_error_envelope())),
            }
        }))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meta_session_cannot_be_constructed_directly() {
        let err = match NapiMetaSession::new() {
            Ok(_) => panic!("constructor should stay disabled"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("MetaProject.openSession"));
    }

    #[test]
    fn audit_bundle_for_walker_tolerates_full_bundle_with_analysis_and_resolution() {
        // Review F14: pin the serde "tolerate unknown fields" posture
        // for `AuditBundleForWalker`. The walker bindings
        // (`whyLoadedFromAuditJson`, `whyInstantiatedFromAuditJson`)
        // receive a full `AuditBundle` JSON — `{ analysis, resolution,
        // record }` — but only need `record`. If a future refactor
        // accidentally adds `#[serde(deny_unknown_fields)]` to
        // `AuditBundleForWalker` (or to `RequestAuditRecord`), the walker
        // bindings silently regress: consumers who stringify their
        // full bundle would start hitting "unknown field `analysis`"
        // deserialization errors that only surface at runtime.
        //
        // Discriminating: flip to `deny_unknown_fields` on
        // `AuditBundleForWalker` and this test's `.expect("deserialize")`
        // fails with a clear serde message naming the extra field.
        let full_bundle = serde_json::json!({
            "analysis": { "component": "Widget", "props": [] },
            "resolution": { "canonicalId": "/Widget.vue", "kind": "some-kind" },
            "record": {
                "request_id": "42",
                "canonical_id": "/Widget.vue",
                "kind": "ComponentMeta",
                "parent_request_id": null,
                "timings": {
                    "total_ms": 0.0,
                    "capture_inputs_ms": 0.0,
                    "store_read_ms": 0.0,
                    "store_merge_ms": 0.0,
                    "direct_import_proof_ms": 0.0,
                    "imported_root_proof_ms": 0.0,
                    "solver_ms": 0.0,
                    "materialize_ms": 0.0,
                    "serialize_ms": 0.0
                },
                "store": {
                    "store_view_hits": 0,
                    "store_view_misses": 0,
                    "structural_merges": 0,
                    "imported_dependency_entries": 0,
                    "imported_dependency_bytes": "0",
                    "prepared_type_decls": 0,
                    "prepared_value_decls": 0
                },
                "memory": {
                    "process_rss_before_bytes": "0",
                    "process_rss_after_bytes": "0",
                    "process_rss_delta_bytes": "0",
                    "host_cache_before_bytes": "0",
                    "host_cache_after_bytes": "0",
                    "workspace_before_bytes": "0",
                    "workspace_after_bytes": "0"
                },
                "footprint": null,
                "kind_payload": {
                    "kind": "ComponentMeta",
                    "total_resolve_steps": "0",
                    "solve_count": 0,
                    "materialize_structure_calls": "0",
                    "materialize_structure_cache_hits": "0",
                    "node_arena_lock_acquisitions": "0",
                    "family_map_lock_acquisitions": "0",
                    "dep_signature_merges": "0",
                    "dep_signature_intern_hits": "0"
                }
            }
        });
        let bundle_json = serde_json::to_string(&full_bundle).expect("encode");
        let walker_bundle: AuditBundleForWalker =
            serde_json::from_str(&bundle_json).expect("deserialize full bundle as walker-only");
        assert_eq!(walker_bundle.record.request_id, 42);
        assert_eq!(walker_bundle.record.canonical_id, "/Widget.vue");
        assert!(walker_bundle.record.footprint.is_none());
    }
}
