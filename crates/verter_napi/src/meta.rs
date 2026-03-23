//! NAPI bindings for the MetaProject/MetaSession pooled-runtime model.
//!
//! These wrap the core types from [`verter_host::meta`] and expose them
//! as JavaScript classes usable by `@verter/component-meta`.

use napi::bindgen_prelude::*;
use napi::{Error, Status};
use napi_derive::napi;
use std::sync::Arc;

use verter_host::meta::{MetaProject, MetaSession};

use crate::{buffer_to_string, catch_panic, NapiHostConfig, NapiIdeProjectConfig};

fn meta_err(e: verter_host::meta::MetaError) -> Error {
    Error::new(Status::GenericFailure, e.to_string())
}

// ---------------------------------------------------------------------------
// NapiMetaProject
// ---------------------------------------------------------------------------

/// A shared, long-lived project that wraps one native host.
///
/// Multiple sessions can be opened against the same project. The project
/// owns the host, base file caches, and session management. Create one
/// per tsconfig / project root and reuse it across checkers.
#[napi(js_name = "MetaProject")]
pub struct NapiMetaProject {
    inner: Arc<MetaProject>,
}

#[napi]
impl NapiMetaProject {
    /// Create a new MetaProject with the given host configuration.
    #[napi(constructor)]
    pub fn new(config: Option<NapiHostConfig>) -> Result<Self> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let ffi_config: verter_ffi::types::FfiHostConfig = config.unwrap_or_default().into();
            let host_config =
                verter_ffi::convert::ffi_config_to_host(ffi_config).map_err(crate::ffi_err)?;
            let host = verter_host::VerterHost::new_standalone(host_config);
            Ok(NapiMetaProject {
                inner: MetaProject::new(host),
            })
        }))?
    }

    /// Create a new MetaProject backed by an existing Workspace.
    #[napi(factory, js_name = "withWorkspace")]
    pub fn with_workspace(
        config: Option<NapiHostConfig>,
        workspace: &crate::NapiWorkspace,
    ) -> Result<Self> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let ffi_config: verter_ffi::types::FfiHostConfig = config.unwrap_or_default().into();
            let host_config =
                verter_ffi::convert::ffi_config_to_host(ffi_config).map_err(crate::ffi_err)?;
            let ws: Arc<dyn verter_vfs::WorkspaceAccess> = workspace.workspace();
            let host = verter_host::VerterHost::new(host_config, ws);
            Ok(NapiMetaProject {
                inner: MetaProject::new(host),
            })
        }))?
    }

    /// Load a file into the base project (shared across all sessions).
    #[napi(js_name = "upsertBase")]
    pub fn upsert_base(&self, canonical_id: String, source: Buffer) -> Result<()> {
        let source = buffer_to_string(source)?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner
                .upsert_base(&canonical_id, &source)
                .map_err(meta_err)
        }))?
    }

    /// Ensure a workspace-backed file is loaded into the shared base project.
    #[napi(js_name = "ensureLoaded")]
    pub fn ensure_loaded(&self, canonical_id: String) -> Result<bool> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.ensure_loaded(&canonical_id).map_err(meta_err)
        }))?
    }

    /// Refresh a workspace-backed base file from the current native workspace.
    #[napi(js_name = "refreshBase")]
    pub fn refresh_base(&self, canonical_id: String) -> Result<bool> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.refresh_base(&canonical_id).map_err(meta_err)
        }))?
    }

    /// Configure project-scoped path alias resolution.
    #[napi(js_name = "configureProjects")]
    pub fn configure_projects(&self, projects: Vec<NapiIdeProjectConfig>) -> Result<()> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let configs: Vec<verter_analysis::project_resolver::IdeProjectConfig> = projects
                .into_iter()
                .map(crate::napi_project_config_to_ide)
                .collect();
            self.inner.configure_projects(configs).map_err(meta_err)
        }))?
    }

    /// Install a project-local HTML intrinsic catalog extracted from installed
    /// TypeScript/Vue types.
    #[napi(js_name = "setHtmlIntrinsicsCatalog")]
    pub fn set_html_intrinsics_catalog(&self, catalog_json: String) -> Result<()> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner
                .set_html_intrinsics_catalog(&catalog_json)
                .map_err(meta_err)
        }))?
    }

    /// Open a new isolated session against this project.
    #[napi(js_name = "openSession")]
    pub fn open_session(&self) -> Result<NapiMetaSession> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let session = self.inner.open_session().map_err(meta_err)?;
            Ok(NapiMetaSession {
                inner: Some(session),
                project: Arc::clone(&self.inner),
            })
        }))?
    }

    /// Clear shared analysis caches without shutting down.
    #[napi(js_name = "clearCaches")]
    pub fn clear_caches(&self) -> Result<()> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.clear_caches().map_err(meta_err)
        }))?
    }

    /// Terminal shutdown. Stops the host and invalidates all sessions.
    /// Synchronous and idempotent.
    #[napi]
    pub fn shutdown(&self) -> Result<()> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner.shutdown();
        }))
    }

    /// Whether this project has been shut down.
    #[napi(js_name = "isShutdown", getter)]
    pub fn is_shutdown(&self) -> bool {
        self.inner.is_shutdown()
    }

    /// Number of active sessions.
    #[napi(js_name = "sessionCount", getter)]
    pub fn session_count(&self) -> u32 {
        self.inner.session_count() as u32
    }

    /// Returns the set of canonical IDs in the base file index.
    #[napi(js_name = "baseFileIds")]
    pub fn base_file_ids(&self) -> Vec<String> {
        self.inner.base_file_ids().into_iter().collect()
    }
}

// ---------------------------------------------------------------------------
// NapiMetaSession
// ---------------------------------------------------------------------------

/// A lightweight session handle with isolated file overlays.
///
/// Overlays are private to this session. `updateFile()` and `deleteFile()`
/// in one session never affect another session's view. Queries resolve
/// through `session overlay → shared base`.
#[napi(js_name = "MetaSession")]
pub struct NapiMetaSession {
    // Option so we can take() on close for clean release
    inner: Option<MetaSession>,
    #[allow(dead_code)]
    project: Arc<MetaProject>,
}

impl NapiMetaSession {
    fn session(&self) -> Result<&MetaSession> {
        self.inner
            .as_ref()
            .ok_or_else(|| Error::new(Status::GenericFailure, "session is closed"))
    }
}

#[napi]
impl NapiMetaSession {
    /// MetaSession instances are created via `MetaProject.openSession()`.
    /// This constructor exists only for NAPI-RS class registration.
    #[napi(constructor)]
    pub fn new() -> Result<Self> {
        Err(Error::new(
            Status::GenericFailure,
            "MetaSession cannot be constructed directly. Use MetaProject.openSession().",
        ))
    }

    /// Store a file overlay in this session.
    #[napi]
    pub fn upsert(&self, canonical_id: String, source: Buffer) -> Result<()> {
        let source = buffer_to_string(source)?;
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            session.upsert(&canonical_id, source).map_err(meta_err)
        }))?
    }

    /// Tombstone a file in this session (mark as deleted).
    #[napi]
    pub fn delete(&self, canonical_id: String) -> Result<()> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            session.delete(&canonical_id).map_err(meta_err)
        }))?
    }

    /// Get effective source for a file (overlay → base).
    /// Returns `null` for tombstoned or non-existent files.
    #[napi(js_name = "getEffectiveSource")]
    pub fn get_effective_source(&self, canonical_id: String) -> Result<Option<String>> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            session
                .get_effective_source(&canonical_id)
                .map_err(meta_err)
        }))?
    }

    /// Check if a file is visible in this session (not tombstoned).
    #[napi(js_name = "hasFile")]
    pub fn has_file(&self, canonical_id: String) -> Result<bool> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            session.has_file(&canonical_id).map_err(meta_err)
        }))?
    }

    /// Single native component-meta query.
    ///
    /// Returns a JSON string containing the full component metadata (props,
    /// events, slots, models, exposed, flags) with structured type IR.
    /// Returns `null` if the file doesn't exist.
    #[napi(js_name = "getComponentMeta")]
    pub fn get_component_meta(&self, canonical_or_alias: String) -> Result<Option<String>> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let result = session
                .get_component_meta(&canonical_or_alias)
                .map_err(meta_err)?;
            match result {
                Some(analysis) => {
                    let ffi = verter_ffi::convert::component_meta_analysis_to_ffi(analysis);
                    let json = serde_json::to_string(&ffi).map_err(|e| {
                        Error::new(
                            Status::GenericFailure,
                            format!("component-meta serialization error: {e}"),
                        )
                    })?;
                    Ok(Some(json))
                }
                None => Ok(None),
            }
        }))?
    }

    /// Return provenance counters for observability.
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

    /// Returns canonical IDs of all files visible to this session.
    #[napi(js_name = "trackedFileIds")]
    pub fn tracked_file_ids(&self) -> Result<Vec<String>> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            session.visible_file_ids().map_err(meta_err)
        }))?
    }

    /// Close the session, releasing the overlay and lease.
    /// Idempotent — safe to call multiple times.
    #[napi]
    pub fn close(&mut self) -> Result<()> {
        if let Some(session) = self.inner.take() {
            session.close();
        }
        Ok(())
    }

    /// Whether this session has been closed.
    #[napi(js_name = "isClosed", getter)]
    pub fn is_closed(&self) -> bool {
        self.inner.as_ref().is_none_or(|s| s.is_closed())
    }

    /// The overlay generation counter for this session.
    #[napi(js_name = "overlayGeneration", getter)]
    pub fn overlay_generation(&self) -> u32 {
        self.inner
            .as_ref()
            .map_or(0, |s| s.overlay_generation() as u32)
    }
}
