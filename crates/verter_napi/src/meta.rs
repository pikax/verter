//! NAPI bindings for component-meta.
//!
//! The public JS surface keeps the existing `MetaProject` / `MetaSession`
//! class names for now, but those names now wrap the new
//! `ComponentMetaHost` / isolated session layer.

use std::result::Result as StdResult;
use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi::{Error, Status};
use napi_derive::napi;
use verter_analysis::type_expr::{ObjectMember, TypeExpr};
use verter_host::component_meta_host::{
    ComponentMetaHost, ComponentMetaHostError, ComponentMetaSession as HostComponentMetaSession,
    ComponentMetaTypeExpander,
};
use verter_resolver::query_artifact::{
    ArtifactId, ArtifactProfile as ResolverArtifactProfile, GeneratedQueryArtifact,
    QuerySpanMapping,
};
use verter_resolver::type_expansion::{
    BackendFailureKind, ExpandedMember, ExpansionCompleteness, ExpansionProfile,
    TypeExpansionBackend, TypeExpansionError, TypeExpansionRequest, TypeExpansionResult,
};
use verter_resolver::type_expansion_host::TypeExpansionSnapshot;
use verter_resolver::type_text_parser;
use verter_type_runtime::tsgo::{find_tsgo_binary, TsgoTypeProvider};
use verter_type_runtime::tsserver::TsserverTypeProvider;
use verter_type_runtime::{
    find_node, find_tsserver, path_to_file_uri_string, BackendError, BackendTypeCompleteness,
    BackendTypeData, BackendTypeQuery, GeneratedFileId, GeneratedQueryBackend, TypeProvider,
    TypeProviderAdapter,
};

use crate::{buffer_to_string, catch_panic, NapiHostConfig, NapiIdeProjectConfig};

fn meta_err(e: ComponentMetaHostError) -> Error {
    Error::new(Status::GenericFailure, e.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeComponentMetaBackend {
    Tsserver,
    Tsgo,
}

impl RuntimeComponentMetaBackend {
    fn label(self) -> &'static str {
        match self {
            Self::Tsserver => "tsserver",
            Self::Tsgo => "tsgo",
        }
    }

    fn runtime_key(self) -> &'static str {
        match self {
            Self::Tsserver => "component-meta-tsserver",
            Self::Tsgo => "component-meta-tsgo",
        }
    }
}

struct RuntimeBackedComponentMetaExpander {
    backend: Arc<dyn GeneratedQueryBackend>,
    runtime: tokio::runtime::Runtime,
    runtime_key: String,
}

impl RuntimeBackedComponentMetaExpander {
    fn new(
        runtime: tokio::runtime::Runtime,
        backend: Arc<dyn GeneratedQueryBackend>,
        runtime_key: impl Into<String>,
    ) -> Self {
        Self {
            backend,
            runtime,
            runtime_key: runtime_key.into(),
        }
    }
}

impl ComponentMetaTypeExpander for RuntimeBackedComponentMetaExpander {
    fn expand_type(
        &self,
        request: &TypeExpansionRequest,
        snapshot: TypeExpansionSnapshot,
    ) -> StdResult<TypeExpansionResult, TypeExpansionError> {
        let artifact = build_component_meta_artifact(&request.canonical_id, &snapshot, request)?;
        let generated_offset = artifact
            .sfc_to_generated(request.span.start)
            .ok_or(TypeExpansionError::MappingFailed)?;
        let file_id = GeneratedFileId {
            canonical_id: artifact.artifact_id.canonical_id.clone(),
            profile: runtime_artifact_profile(artifact.profile),
            runtime_key: self.runtime_key.clone(),
        };
        let backend = Arc::clone(&self.backend);
        let data = self.runtime.block_on(async move {
            backend
                .sync_file(
                    &file_id,
                    artifact.source_revision,
                    &artifact.generated_source,
                )
                .await
                .map_err(map_backend_error)?;
            backend
                .query_type_data(
                    &file_id,
                    artifact.source_revision,
                    generated_offset,
                    BackendTypeQuery::TypeAtOffset,
                )
                .await
                .map_err(map_backend_error)
        })?;

        type_expansion_from_backend_data(data)
    }

    fn shutdown(&self) {
        let _ = self.runtime.block_on(self.backend.shutdown());
    }
}

fn create_component_meta_host(
    host_config: verter_host::HostConfig,
    workspace: Option<Arc<dyn verter_vfs::WorkspaceAccess>>,
    workspace_roots: &[String],
) -> Result<Arc<ComponentMetaHost>> {
    create_component_meta_host_with_factory(
        host_config,
        workspace,
        workspace_roots,
        build_component_meta_type_expander,
    )
}

fn create_component_meta_host_with_factory<F>(
    host_config: verter_host::HostConfig,
    workspace: Option<Arc<dyn verter_vfs::WorkspaceAccess>>,
    workspace_roots: &[String],
    build_expander: F,
) -> Result<Arc<ComponentMetaHost>>
where
    F: FnOnce(
        TypeExpansionBackend,
        &[String],
    ) -> Result<Option<Arc<dyn ComponentMetaTypeExpander>>>,
{
    let backend = host_config.type_expansion_backend;
    let host = Arc::new(match workspace {
        Some(workspace) => ComponentMetaHost::new(host_config, workspace),
        None => ComponentMetaHost::new_standalone(host_config),
    });

    if backend != TypeExpansionBackend::Verter {
        if let Some(expander) = build_expander(backend, workspace_roots)? {
            host.set_type_expander(expander);
        }
    }

    Ok(host)
}

fn build_component_meta_type_expander(
    backend: TypeExpansionBackend,
    workspace_roots: &[String],
) -> Result<Option<Arc<dyn ComponentMetaTypeExpander>>> {
    match backend {
        TypeExpansionBackend::Verter => Ok(None),
        TypeExpansionBackend::Tsserver => build_runtime_component_meta_expander(
            RuntimeComponentMetaBackend::Tsserver,
            workspace_roots,
        )
        .map(Some),
        TypeExpansionBackend::Tsgo => build_runtime_component_meta_expander(
            RuntimeComponentMetaBackend::Tsgo,
            workspace_roots,
        )
        .map(Some),
        TypeExpansionBackend::Auto => {
            let workspace_root = runtime_workspace_root(workspace_roots)?;
            match select_auto_runtime_backend(&workspace_root) {
                Some(runtime_backend) => {
                    build_runtime_component_meta_expander(runtime_backend, workspace_roots)
                        .map(Some)
                }
                None => Ok(None),
            }
        }
    }
}

fn build_runtime_component_meta_expander(
    runtime_backend: RuntimeComponentMetaBackend,
    workspace_roots: &[String],
) -> Result<Arc<dyn ComponentMetaTypeExpander>> {
    let workspace_root = runtime_workspace_root(workspace_roots)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            Error::new(
                Status::GenericFailure,
                format!(
                    "failed to initialize {} component-meta runtime: {error}",
                    runtime_backend.label()
                ),
            )
        })?;
    let backend = spawn_runtime_backend(&runtime, runtime_backend, &workspace_root)?;

    Ok(Arc::new(RuntimeBackedComponentMetaExpander::new(
        runtime,
        backend,
        runtime_backend.runtime_key(),
    )) as Arc<dyn ComponentMetaTypeExpander>)
}

fn select_auto_runtime_backend(workspace_root: &str) -> Option<RuntimeComponentMetaBackend> {
    if find_tsgo_binary().is_ok() {
        Some(RuntimeComponentMetaBackend::Tsgo)
    } else if find_node().is_some() && find_tsserver(None, Some(workspace_root)).is_some() {
        Some(RuntimeComponentMetaBackend::Tsserver)
    } else {
        None
    }
}

fn runtime_workspace_root(workspace_roots: &[String]) -> Result<String> {
    if let Some(root) = workspace_roots.iter().find(|root| !root.trim().is_empty()) {
        return Ok(root.clone());
    }

    std::env::current_dir()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| {
            Error::new(
                Status::GenericFailure,
                format!("could not determine component-meta workspace root: {error}"),
            )
        })
}

fn spawn_runtime_backend(
    runtime: &tokio::runtime::Runtime,
    runtime_backend: RuntimeComponentMetaBackend,
    workspace_root: &str,
) -> Result<Arc<dyn GeneratedQueryBackend>> {
    match runtime_backend {
        RuntimeComponentMetaBackend::Tsserver => {
            let node_path = find_node().ok_or_else(|| {
                Error::new(
                    Status::GenericFailure,
                    "could not find `node` for tsserver component-meta backend",
                )
            })?;
            let tsserver_path = find_tsserver(None, Some(workspace_root)).ok_or_else(|| {
                Error::new(
                    Status::GenericFailure,
                    format!(
                        "could not find `tsserver.js` for component-meta backend under `{workspace_root}`"
                    ),
                )
            })?;
            let tsserver_path = tsserver_path.to_string_lossy().into_owned();
            let provider: Arc<dyn TypeProvider> = Arc::new(
                runtime
                    .block_on(TsserverTypeProvider::spawn(
                        &node_path,
                        &tsserver_path,
                        workspace_root,
                        None,
                        None,
                    ))
                    .map_err(|error| runtime_backend_start_error(runtime_backend, error))?,
            );
            Ok(Arc::new(TypeProviderAdapter::new(provider)) as Arc<dyn GeneratedQueryBackend>)
        }
        RuntimeComponentMetaBackend::Tsgo => {
            let tsgo_binary = find_tsgo_binary().map_err(|error| {
                Error::new(
                    Status::GenericFailure,
                    format!("could not find `tsgo` for component-meta backend: {error}"),
                )
            })?;
            let root_uri = path_to_file_uri_string(workspace_root);
            let provider: Arc<dyn TypeProvider> = Arc::new(
                runtime
                    .block_on(TsgoTypeProvider::spawn(&tsgo_binary, &root_uri))
                    .map_err(|error| runtime_backend_start_error(runtime_backend, error))?,
            );
            Ok(Arc::new(TypeProviderAdapter::new(provider)) as Arc<dyn GeneratedQueryBackend>)
        }
    }
}

fn runtime_backend_start_error(
    runtime_backend: RuntimeComponentMetaBackend,
    error: verter_type_runtime::TypeProviderError,
) -> Error {
    Error::new(
        Status::GenericFailure,
        format!(
            "failed to start {} component-meta backend: {error}",
            runtime_backend.label()
        ),
    )
}

fn map_backend_error(error: BackendError) -> TypeExpansionError {
    match error {
        BackendError::Unavailable | BackendError::StartupFailed(_) => {
            TypeExpansionError::BackendFailure(BackendFailureKind::Unavailable)
        }
        BackendError::TransportClosed | BackendError::BackendReported(_) => {
            TypeExpansionError::BackendFailure(BackendFailureKind::Died)
        }
        BackendError::TimedOut => TypeExpansionError::BackendFailure(BackendFailureKind::TimedOut),
        BackendError::ProtocolViolation(_) => {
            TypeExpansionError::BackendFailure(BackendFailureKind::ProtocolViolation)
        }
        BackendError::UnsupportedQuery => TypeExpansionError::UnsupportedByBackend,
    }
}

fn build_component_meta_artifact(
    canonical_id: &str,
    snapshot: &TypeExpansionSnapshot,
    request: &TypeExpansionRequest,
) -> StdResult<GeneratedQueryArtifact, TypeExpansionError> {
    let profile = match request.profile {
        ExpansionProfile::ComponentMeta => ResolverArtifactProfile::ComponentMeta,
        ExpansionProfile::Lsp => ResolverArtifactProfile::Lsp,
    };
    let source = &snapshot.source.text;
    let mut generated = String::new();
    let mut mappings = Vec::new();

    if let Some(script) = &snapshot.sfc_structure.script {
        let start = script.content.start as usize;
        let end = script.content.end as usize;
        let block_text = &source[start..end];
        let generated_offset = generated.len() as u32;
        let cleaned = strip_export_default(block_text);
        generated.push_str(&cleaned);
        generated.push('\n');
        mappings.push(QuerySpanMapping {
            sfc_span: script.content,
            generated_offset,
            generated_len: cleaned.len() as u32,
        });
    }

    if let Some(script_setup) = &snapshot.sfc_structure.script_setup {
        let start = script_setup.content.start as usize;
        let end = script_setup.content.end as usize;
        let block_text = &source[start..end];
        let generated_offset = generated.len() as u32;
        generated.push_str(block_text);
        generated.push('\n');
        mappings.push(QuerySpanMapping {
            sfc_span: script_setup.content,
            generated_offset,
            generated_len: (end - start) as u32,
        });
    }

    if mappings.is_empty() {
        return Err(TypeExpansionError::SourceUnavailable);
    }

    Ok(GeneratedQueryArtifact {
        generated_source: generated,
        profile,
        mappings,
        source_revision: snapshot.revision,
        artifact_id: ArtifactId::new(canonical_id, profile),
    })
}

fn strip_export_default(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut skip_until_close = false;
    let mut brace_depth = 0i32;

    for line in content.lines() {
        let trimmed = line.trim();
        if skip_until_close {
            for ch in trimmed.chars() {
                match ch {
                    '{' => brace_depth += 1,
                    '}' => {
                        brace_depth -= 1;
                        if brace_depth <= 0 {
                            skip_until_close = false;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }

        if trimmed.starts_with("export default") {
            if let Some(brace_pos) = trimmed.find('{') {
                brace_depth = 1;
                for ch in trimmed[brace_pos + 1..].chars() {
                    match ch {
                        '{' => brace_depth += 1,
                        '}' => brace_depth -= 1,
                        _ => {}
                    }
                }
                if brace_depth > 0 {
                    skip_until_close = true;
                }
            }
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    result
}

fn runtime_artifact_profile(
    profile: ResolverArtifactProfile,
) -> verter_type_runtime::ArtifactProfile {
    match profile {
        ResolverArtifactProfile::ComponentMeta => {
            verter_type_runtime::ArtifactProfile::ComponentMeta
        }
        ResolverArtifactProfile::Lsp => verter_type_runtime::ArtifactProfile::Lsp,
    }
}

fn type_expansion_from_backend_data(
    data: BackendTypeData,
) -> StdResult<TypeExpansionResult, TypeExpansionError> {
    let synthesized_type_text = if data.type_text.is_none() {
        synthesize_type_text_from_members(&data)
    } else {
        None
    };
    let type_text = data
        .type_text
        .as_deref()
        .or(synthesized_type_text.as_deref())
        .ok_or(TypeExpansionError::NoExpansionResult)?;
    let type_expr = type_text_parser::parse_type_text(strip_type_display_prefix(type_text));
    let members = if data.members.is_empty() {
        extract_members_from_type_expr(&type_expr)
    } else {
        data.members
            .into_iter()
            .map(|member| ExpandedMember {
                name: member.name,
                type_expr: member
                    .type_text
                    .as_deref()
                    .map(strip_type_display_prefix)
                    .map(type_text_parser::parse_type_text)
                    .unwrap_or_else(|| type_text_parser::parse_type_text("unknown")),
                optional: member.optional,
                description: member.documentation,
            })
            .collect()
    };

    let completeness = if matches!(&type_expr, TypeExpr::Unknown { .. }) {
        ExpansionCompleteness::OpaqueFallback
    } else {
        match data.completeness {
            BackendTypeCompleteness::Exact => ExpansionCompleteness::Exact,
            BackendTypeCompleteness::Partial => ExpansionCompleteness::LowerBound,
            BackendTypeCompleteness::Failed => ExpansionCompleteness::OpaqueFallback,
        }
    };

    Ok(TypeExpansionResult {
        type_expr,
        members,
        completeness,
    })
}

fn synthesize_type_text_from_members(data: &BackendTypeData) -> Option<String> {
    if data.members.is_empty() {
        return None;
    }

    let mut text = String::from("{ ");
    for (index, member) in data.members.iter().enumerate() {
        if index > 0 {
            text.push_str("; ");
        }
        text.push_str(&member.name);
        if member.optional {
            text.push('?');
        }
        text.push_str(": ");
        text.push_str(member.type_text.as_deref().unwrap_or("unknown"));
    }
    text.push_str(" }");
    Some(text)
}

fn strip_type_display_prefix(contents: &str) -> &str {
    if let Some(eq_pos) = contents.find(" = ") {
        let after = contents[eq_pos + 3..].trim();
        if !after.is_empty() {
            return after;
        }
    }

    if contents.starts_with('(') {
        if let Some(colon) = contents.find(':') {
            let after = contents[colon + 1..].trim();
            if !after.is_empty() {
                return after;
            }
        }
    }

    contents
}

fn extract_members_from_type_expr(type_expr: &TypeExpr) -> Vec<ExpandedMember> {
    match type_expr {
        TypeExpr::Object(obj) => obj
            .properties
            .iter()
            .filter_map(|member| match member {
                ObjectMember::Property(prop) => Some(ExpandedMember {
                    name: prop.name.clone(),
                    type_expr: prop.ty.clone(),
                    optional: prop.optional,
                    description: None,
                }),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    }
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
                inner: create_component_meta_host(host_config, None, &[])?,
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
            let roots = workspace.roots();
            let ws: Arc<dyn verter_vfs::WorkspaceAccess> = workspace.workspace();
            Ok(NapiMetaProject {
                inner: create_component_meta_host(host_config, Some(ws), &roots)?,
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
            let configs: Vec<verter_analysis::project_resolver::IdeProjectConfig> = projects
                .into_iter()
                .map(crate::napi_project_config_to_ide)
                .collect();
            self.inner.configure_projects(configs).map_err(meta_err)
        }))?
    }

    #[napi(js_name = "setHtmlIntrinsicsCatalog")]
    pub fn set_html_intrinsics_catalog(&self, catalog_json: String) -> Result<()> {
        catch_panic(std::panic::AssertUnwindSafe(|| {
            self.inner
                .set_html_intrinsics_catalog(&catalog_json)
                .map_err(meta_err)
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

    #[napi(js_name = "getDeclaredComponentMeta")]
    pub fn get_declared_component_meta(
        &self,
        canonical_or_alias: String,
    ) -> Result<Option<String>> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let result = session
                .get_declared_component_meta(&canonical_or_alias)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct FakeComponentMetaTypeExpander;

    impl ComponentMetaTypeExpander for FakeComponentMetaTypeExpander {
        fn expand_type(
            &self,
            _request: &TypeExpansionRequest,
            _snapshot: TypeExpansionSnapshot,
        ) -> StdResult<TypeExpansionResult, TypeExpansionError> {
            Err(TypeExpansionError::NoExpansionResult)
        }
    }

    #[test]
    fn create_component_meta_host_installs_expander_for_non_verter_backend() {
        let mut host_config = verter_host::HostConfig::default();
        host_config.type_expansion_backend = TypeExpansionBackend::Tsgo;
        let roots = vec!["/workspace".to_string()];
        let invoked = AtomicBool::new(false);

        let host = create_component_meta_host_with_factory(
            host_config,
            None,
            &roots,
            |backend, observed_roots| {
                invoked.store(true, Ordering::Release);
                assert_eq!(backend, TypeExpansionBackend::Tsgo);
                assert_eq!(observed_roots, roots.as_slice());
                Ok(Some(
                    Arc::new(FakeComponentMetaTypeExpander) as Arc<dyn ComponentMetaTypeExpander>
                ))
            },
        )
        .unwrap();

        assert!(invoked.load(Ordering::Acquire));
        assert!(host.has_external_expander());
    }

    #[test]
    fn create_component_meta_host_skips_expander_factory_for_verter_backend() {
        let host_config = verter_host::HostConfig::default();
        let invoked = AtomicBool::new(false);

        let host = create_component_meta_host_with_factory(host_config, None, &[], |_, _| {
            invoked.store(true, Ordering::Release);
            Ok(Some(
                Arc::new(FakeComponentMetaTypeExpander) as Arc<dyn ComponentMetaTypeExpander>
            ))
        })
        .unwrap();

        assert!(!invoked.load(Ordering::Acquire));
        assert!(!host.has_external_expander());
    }

    #[test]
    fn tsgo_backend_smoke_test_when_runtime_is_available() {
        if find_tsgo_binary().is_err() {
            return;
        }

        let repo_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(|path| path.parent())
            .expect("crate should live under workspace/crates")
            .to_path_buf();
        let test_root = repo_root
            .join("target")
            .join(format!("component-meta-tsgo-smoke-{}", std::process::id()));
        let src_dir = test_root.join("src");
        let _ = std::fs::remove_dir_all(&test_root);
        std::fs::create_dir_all(&src_dir).unwrap();

        let types_path = src_dir.join("types.ts");
        let app_path = src_dir.join("App.vue");
        let types_source = "export interface Props { label: string }\n";
        let app_source = r#"<script setup lang="ts">
import type { Props } from "./types"
defineProps<Props>()
</script>
<template><div>{{ label }}</div></template>"#;
        std::fs::write(&types_path, types_source).unwrap();
        std::fs::write(&app_path, app_source).unwrap();

        let app_id = app_path.to_string_lossy().into_owned();
        let types_id = types_path.to_string_lossy().into_owned();

        let mut host_config = verter_host::HostConfig::default();
        host_config.type_expansion_backend = TypeExpansionBackend::Tsgo;
        let workspace_roots = vec![test_root.to_string_lossy().into_owned()];
        let host = create_component_meta_host(host_config, None, &workspace_roots).unwrap();

        host.upsert_base(&types_id, types_source).unwrap();
        host.upsert_base(&app_id, app_source).unwrap();
        host.host().set_import_dependencies(
            &app_id,
            vec![verter_host::DependencyResolution {
                specifier: "./types".to_string(),
                resolved_canonical_id: Some(types_id.clone()),
                possible_canonical_ids: Vec::new(),
            }],
        );

        let session = host.open_session().unwrap();
        let result = session
            .get_component_meta(&app_id)
            .unwrap()
            .expect("tsgo backend should return component meta");

        let props: Vec<_> = result.props.iter().map(|prop| prop.name.as_str()).collect();
        assert_eq!(props, vec!["label"]);

        session.close();
        host.shutdown();
        let _ = std::fs::remove_dir_all(&test_root);
    }
}
