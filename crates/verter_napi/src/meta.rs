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
use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};
use verter_session::component_meta_host::{
    ComponentMetaHost, ComponentMetaHostError, ComponentMetaSession as HostComponentMetaSession,
    ComponentMetaTraceCursor, ComponentMetaTypeExpander,
};
use verter_type_runtime::tsgo::{find_tsgo_binary, TsgoTypeProvider};
use verter_type_runtime::tsserver::TsserverTypeProvider;
use verter_type_runtime::{
    find_node, find_tsserver, path_to_file_uri_string, with_type_runtime_trace_context,
    BackendError, BackendTypeCompleteness, BackendTypeData, BackendTypeQuery, GeneratedFileId,
    GeneratedQueryBackend, TypeProvider, TypeProviderAdapter, TypeRuntimeTraceContext,
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
    backend_label: &'static str,
    backend: Arc<dyn GeneratedQueryBackend>,
    runtime: tokio::runtime::Runtime,
    runtime_key: String,
}

struct ComponentMetaQueryArtifact {
    artifact: GeneratedQueryArtifact,
    hover_offset: u32,
    members_offset: Option<u32>,
    generic_clause: Option<String>,
}

struct ComponentMetaMembersQueryArtifact {
    artifact: GeneratedQueryArtifact,
    members_offset: u32,
}

struct ComponentMetaProbeOffsets {
    hover_offset: u32,
    members_offset: u32,
}

impl RuntimeBackedComponentMetaExpander {
    fn new(
        backend_label: &'static str,
        runtime: tokio::runtime::Runtime,
        backend: Arc<dyn GeneratedQueryBackend>,
        runtime_key: impl Into<String>,
    ) -> Self {
        Self {
            backend_label,
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
        trace_cursor: Option<ComponentMetaTraceCursor>,
    ) -> StdResult<TypeExpansionResult, TypeExpansionError> {
        let context = trace_cursor.map(|cursor| TypeRuntimeTraceContext {
            request_id: cursor.request_id,
            parent_span_id: cursor.span_id,
            base_depth: cursor.depth + 1,
        });
        with_type_runtime_trace_context(context, || {
            let _trace = verter_type_runtime::type_runtime_trace_scope!(
                "runtime_component_meta_expand_type",
                format!(
                    "backend={} owner={} span={}..{} revision={}",
                    self.backend_label,
                    request.canonical_id,
                    request.span.start,
                    request.span.end,
                    snapshot.revision,
                ),
            );
            let query_artifact =
                build_component_meta_query_artifact(&request.canonical_id, &snapshot, request)?;
            verter_type_runtime::type_runtime_trace_event!(
                "runtime_component_meta_artifact",
                format!(
                    "backend={} owner={} generated_len={} mappings={} revision={} members_offset={:?}",
                    self.backend_label,
                    request.canonical_id,
                    query_artifact.artifact.generated_source.len(),
                    query_artifact.artifact.mappings.len(),
                    query_artifact.artifact.source_revision,
                    query_artifact.members_offset,
                ),
            );
            verter_type_runtime::type_runtime_trace_event!(
                "runtime_component_meta_generated_offset",
                format!(
                    "backend={} owner={} generated_offset={} members_offset={:?}",
                    self.backend_label,
                    request.canonical_id,
                    query_artifact.hover_offset,
                    query_artifact.members_offset,
                ),
            );
            let file_id = GeneratedFileId {
                canonical_id: query_artifact.artifact.artifact_id.canonical_id.clone(),
                profile: runtime_artifact_profile(query_artifact.artifact.profile),
                runtime_key: self.runtime_key.clone(),
            };
            let backend = Arc::clone(&self.backend);
            let data = self.runtime.block_on(async move {
                backend
                    .sync_file(
                        &file_id,
                        query_artifact.artifact.source_revision,
                        &query_artifact.artifact.generated_source,
                    )
                    .await
                    .map_err(map_backend_error)?;
                let member_data = if let Some(members_offset) = query_artifact.members_offset {
                    match backend
                        .query_type_data(
                            &file_id,
                            query_artifact.artifact.source_revision,
                            members_offset,
                            BackendTypeQuery::MembersAtOffset,
                        )
                        .await
                    {
                        Ok(data) if backend_members_are_useful(&data) => {
                            let data = if query_artifact.generic_clause.is_none() {
                                fill_missing_backend_member_types(
                                    backend.as_ref(),
                                    &file_id,
                                    query_artifact.artifact.source_revision,
                                    &query_artifact.artifact.generated_source,
                                    COMPONENT_META_QUERY_TYPE_ALIAS,
                                    COMPONENT_META_QUERY_VALUE,
                                    "__VERTER_COMPONENT_META_MEMBER",
                                    data,
                                )
                                .await?
                            } else {
                                data
                            };
                            verter_type_runtime::type_runtime_trace_event!(
                                "runtime_component_meta_member_probe",
                                format!(
                                    "backend={} owner={} member_count={} used=true",
                                    self.backend_label,
                                    request.canonical_id,
                                    data.members.len(),
                                ),
                            );
                            return Ok(data);
                        }
                        Ok(data) => {
                            verter_type_runtime::type_runtime_trace_event!(
                                "runtime_component_meta_member_probe",
                                format!(
                                    "backend={} owner={} member_count={} used=false",
                                    self.backend_label,
                                    request.canonical_id,
                                    data.members.len(),
                                ),
                            );
                            Some(data)
                        }
                        Err(error) => {
                            verter_type_runtime::type_runtime_trace_event!(
                                "runtime_component_meta_member_probe",
                                format!(
                                    "backend={} owner={} error={error}",
                                    self.backend_label, request.canonical_id,
                                ),
                            );
                            None
                        }
                    }
                } else {
                    None
                };

                let hover_data = backend
                    .query_type_data(
                        &file_id,
                        query_artifact.artifact.source_revision,
                        query_artifact.hover_offset,
                        BackendTypeQuery::TypeAtOffset,
                    )
                    .await
                    .map_err(map_backend_error)?;
                Ok(merge_component_meta_backend_data(hover_data, member_data))
            })?;

            verter_type_runtime::type_runtime_trace_event!(
                "runtime_component_meta_backend_result",
                format!(
                    "backend={} owner={} type_text_len={} members={} completeness={:?}",
                    self.backend_label,
                    request.canonical_id,
                    data.type_text.as_ref().map(|text| text.len()).unwrap_or(0),
                    data.members.len(),
                    data.completeness,
                ),
            );

            type_expansion_from_backend_data(data)
        })
    }

    fn expand_slot_bindings(
        &self,
        request: &TypeExpansionRequest,
        snapshot: TypeExpansionSnapshot,
        slot_name: &str,
        trace_cursor: Option<ComponentMetaTraceCursor>,
    ) -> StdResult<Option<TypeExpansionResult>, TypeExpansionError> {
        let context = trace_cursor.map(|cursor| TypeRuntimeTraceContext {
            request_id: cursor.request_id,
            parent_span_id: cursor.span_id,
            base_depth: cursor.depth + 1,
        });
        with_type_runtime_trace_context(context, || {
            let _trace = verter_type_runtime::type_runtime_trace_scope!(
                "runtime_component_meta_expand_slot_bindings",
                format!(
                    "backend={} owner={} slot={} span={}..{} revision={}",
                    self.backend_label,
                    request.canonical_id,
                    slot_name,
                    request.span.start,
                    request.span.end,
                    snapshot.revision,
                ),
            );
            let query_artifact = build_component_meta_slot_bindings_query_artifact(
                &request.canonical_id,
                &snapshot,
                request,
                slot_name,
            )?;
            let file_id = GeneratedFileId {
                canonical_id: query_artifact.artifact.artifact_id.canonical_id.clone(),
                profile: runtime_artifact_profile(query_artifact.artifact.profile),
                runtime_key: self.runtime_key.clone(),
            };
            let backend = Arc::clone(&self.backend);
            let owner = request.canonical_id.clone();
            let slot = slot_name.to_string();
            let backend_label = self.backend_label;
            let data = self.runtime.block_on(async move {
                backend
                    .sync_file(
                        &file_id,
                        query_artifact.artifact.source_revision,
                        &query_artifact.artifact.generated_source,
                    )
                    .await
                    .map_err(map_backend_error)?;
                let data = match backend
                    .query_type_data(
                        &file_id,
                        query_artifact.artifact.source_revision,
                        query_artifact.members_offset,
                        BackendTypeQuery::MembersAtOffset,
                    )
                    .await
                {
                    Ok(data) => data,
                    Err(error) => {
                        verter_type_runtime::type_runtime_trace_event!(
                            "runtime_component_meta_slot_binding_probe",
                            format!(
                                "backend={} owner={} slot={} error={error}",
                                backend_label, owner, slot,
                            ),
                        );
                        return Ok(None);
                    }
                };
                if !backend_members_are_useful(&data) {
                    verter_type_runtime::type_runtime_trace_event!(
                        "runtime_component_meta_slot_binding_probe",
                        format!(
                            "backend={} owner={} slot={} member_count={} used=false",
                            backend_label,
                            owner,
                            slot,
                            data.members.len(),
                        ),
                    );
                    return Ok(None);
                }
                let data = match fill_missing_backend_member_types(
                    backend.as_ref(),
                    &file_id,
                    query_artifact.artifact.source_revision,
                    &query_artifact.artifact.generated_source,
                    COMPONENT_META_SLOT_BINDINGS_TYPE_ALIAS,
                    COMPONENT_META_SLOT_BINDINGS_VALUE,
                    "__VERTER_COMPONENT_META_SLOT_BINDING",
                    data,
                )
                .await
                {
                    Ok(data) => data,
                    Err(error) => {
                        verter_type_runtime::type_runtime_trace_event!(
                            "runtime_component_meta_slot_binding_probe",
                            format!(
                                "backend={} owner={} slot={} fill_error={error}",
                                backend_label, owner, slot,
                            ),
                        );
                        return Ok(None);
                    }
                };
                verter_type_runtime::type_runtime_trace_event!(
                    "runtime_component_meta_slot_binding_probe",
                    format!(
                        "backend={} owner={} slot={} member_count={} used=true",
                        backend_label,
                        owner,
                        slot,
                        data.members.len(),
                    ),
                );
                Ok(Some(data))
            })?;

            let Some(data) = data else {
                return Ok(None);
            };

            type_expansion_from_backend_data(data).map(Some)
        })
    }

    fn shutdown(&self) {
        let _ = self.runtime.block_on(self.backend.shutdown());
    }
}

fn backend_members_are_useful(data: &BackendTypeData) -> bool {
    !data.members.is_empty()
        && data
            .members
            .iter()
            .any(|member| !is_builtin_callable_member_name(&member.name))
}

fn is_builtin_callable_member_name(name: &str) -> bool {
    matches!(
        name,
        "apply"
            | "arguments"
            | "bind"
            | "call"
            | "caller"
            | "length"
            | "name"
            | "prototype"
            | "toString"
    )
}

async fn fill_missing_backend_member_types(
    backend: &dyn GeneratedQueryBackend,
    file_id: &GeneratedFileId,
    revision: u64,
    base_generated_source: &str,
    type_probe_owner: &str,
    value_probe_owner: &str,
    probe_prefix: &str,
    mut data: BackendTypeData,
) -> StdResult<BackendTypeData, TypeExpansionError> {
    if data.members.is_empty() {
        return Ok(data);
    }

    let missing_names: Vec<String> = data
        .members
        .iter()
        .filter(|member| member_type_text_needs_hover(member))
        .map(|member| member.name.clone())
        .collect();

    let mut generated_source = base_generated_source.to_string();
    let definition_offsets = append_member_definition_probes(
        &mut generated_source,
        &all_backend_member_names(&data),
        type_probe_owner,
        probe_prefix,
    );
    let hover_offsets = append_member_hover_probes(
        &mut generated_source,
        &missing_names,
        value_probe_owner,
        probe_prefix,
    );
    if definition_offsets.is_empty() && hover_offsets.is_empty() {
        return Ok(data);
    }

    backend
        .sync_file(file_id, revision, &generated_source)
        .await
        .map_err(map_backend_error)?;

    let mut definition_filled = std::collections::HashMap::new();
    for (name, offset) in definition_offsets {
        match backend
            .query_type_data(
                file_id,
                revision,
                offset,
                BackendTypeQuery::DefinitionTypeAtOffset,
            )
            .await
        {
            Ok(definition) => {
                if let Some(type_text) = definition
                    .type_text
                    .filter(|text| definition_hover_should_replace_member(&name, &data, text))
                {
                    definition_filled
                        .insert(name, strip_type_display_prefix(&type_text).to_string());
                }
            }
            Err(error) => {
                verter_type_runtime::type_runtime_trace_event!(
                    "runtime_component_meta_member_definition_fill",
                    format!("name={} error={error}", name),
                );
            }
        }
    }

    for member in &mut data.members {
        if let Some(type_text) = definition_filled.get(&member.name) {
            member.type_text = Some(type_text.clone());
        }
    }

    let mut filled = std::collections::HashMap::new();
    for (name, offset) in hover_offsets {
        if !data
            .members
            .iter()
            .find(|member| member.name == name)
            .is_some_and(member_type_text_needs_hover)
        {
            continue;
        }
        match backend
            .query_type_data(file_id, revision, offset, BackendTypeQuery::TypeAtOffset)
            .await
        {
            Ok(hover) => {
                if let Some(type_text) = hover.type_text.filter(|text| !text.trim().is_empty()) {
                    filled.insert(name, strip_type_display_prefix(&type_text).to_string());
                }
            }
            Err(error) => {
                verter_type_runtime::type_runtime_trace_event!(
                    "runtime_component_meta_member_hover_fill",
                    format!("name={} error={error}", name),
                );
            }
        }
    }

    for member in &mut data.members {
        if member_type_text_needs_hover(member) {
            if let Some(type_text) = filled.get(&member.name) {
                member.type_text = Some(type_text.clone());
            }
        }
    }

    Ok(data)
}

fn all_backend_member_names(data: &BackendTypeData) -> Vec<String> {
    data.members
        .iter()
        .map(|member| member.name.clone())
        .collect()
}

fn definition_hover_should_replace_member(
    name: &str,
    data: &BackendTypeData,
    raw_definition: &str,
) -> bool {
    let trimmed = unwrap_markdown_code_fence(raw_definition.trim()).trim();
    if trimmed.is_empty() {
        return false;
    }

    if trimmed.starts_with("(property)") {
        return true;
    }

    if trimmed.starts_with("(type)") {
        return data
            .members
            .iter()
            .find(|member| member.name == name)
            .and_then(|member| member.type_text.as_deref())
            .is_some_and(|current| current.contains(name) || current.contains(" | undefined"));
    }

    false
}

fn member_type_text_needs_hover(member: &verter_type_runtime::BackendTypeMember) -> bool {
    member
        .type_text
        .as_deref()
        .map(|text| {
            let normalized = strip_type_display_prefix(text).trim();
            normalized.is_empty()
                || normalized == "unknown"
                || normalized == "any"
                || normalized == "any | undefined"
                || normalized == "undefined | any"
        })
        .unwrap_or(true)
}

fn append_member_hover_probes(
    generated: &mut String,
    member_names: &[String],
    probe_owner: &str,
    probe_prefix: &str,
) -> Vec<(String, u32)> {
    let mut offsets = Vec::with_capacity(member_names.len());

    for (index, name) in member_names.iter().enumerate() {
        let probe_name = format!("{probe_prefix}_{index}");
        let access = format!("{probe_owner}[{}]", ts_string_literal(name));
        let probe = format!("\nconst {probe_name} = {access};\n{probe_name}\n");
        let marker = format!("\n{probe_name}\n");
        let base = generated.len();
        let relative = probe
            .rfind(&marker)
            .map(|offset| offset + 1)
            .unwrap_or_else(|| probe.rfind(&probe_name).unwrap_or(0));
        generated.push_str(&probe);
        offsets.push((name.clone(), (base + relative) as u32));
    }

    offsets
}

fn append_member_definition_probes(
    generated: &mut String,
    member_names: &[String],
    type_probe_owner: &str,
    probe_prefix: &str,
) -> Vec<(String, u32)> {
    let mut offsets = Vec::with_capacity(member_names.len());

    for (index, name) in member_names.iter().enumerate() {
        let probe_name = format!("{probe_prefix}_DEF_{index}");
        let member_literal = ts_string_literal(name);
        let probe = format!("\ntype {probe_name} = {type_probe_owner}[{member_literal}];\n");
        let base = generated.len();
        let relative = probe.find(&member_literal).map(|offset| offset + 1);
        generated.push_str(&probe);
        if let Some(relative) = relative {
            offsets.push((name.clone(), (base + relative) as u32));
        }
    }

    offsets
}

fn ts_string_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn merge_component_meta_backend_data(
    hover_data: BackendTypeData,
    member_data: Option<BackendTypeData>,
) -> BackendTypeData {
    let Some(member_data) = member_data else {
        return hover_data;
    };
    if backend_members_are_useful(&member_data) {
        return member_data;
    }
    hover_data
}

fn create_component_meta_host(
    host_config: verter_session::HostConfig,
    workspace: Option<Arc<dyn verter_workspace::WorkspaceAccess>>,
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
    host_config: verter_session::HostConfig,
    workspace: Option<Arc<dyn verter_workspace::WorkspaceAccess>>,
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
        runtime_backend.label(),
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
            Ok(
                Arc::new(TypeProviderAdapter::new(provider, runtime_backend.label()))
                    as Arc<dyn GeneratedQueryBackend>,
            )
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
            Ok(
                Arc::new(TypeProviderAdapter::new(provider, runtime_backend.label()))
                    as Arc<dyn GeneratedQueryBackend>,
            )
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

fn build_component_meta_query_artifact(
    canonical_id: &str,
    snapshot: &TypeExpansionSnapshot,
    request: &TypeExpansionRequest,
) -> StdResult<ComponentMetaQueryArtifact, TypeExpansionError> {
    let mut artifact = build_component_meta_artifact(canonical_id, snapshot, request)?;
    let generic_clause = extract_script_setup_generic_clause(snapshot);
    let probe_offsets = append_component_meta_member_probe(
        &mut artifact.generated_source,
        &snapshot.source.text,
        request.span,
        generic_clause.as_deref(),
    );
    let hover_offset = probe_offsets
        .as_ref()
        .map(|offsets| offsets.hover_offset)
        .or_else(|| artifact.sfc_to_generated(request.span.start))
        .ok_or(TypeExpansionError::MappingFailed)?;
    Ok(ComponentMetaQueryArtifact {
        artifact,
        hover_offset,
        members_offset: probe_offsets.map(|offsets| offsets.members_offset),
        generic_clause,
    })
}

fn build_component_meta_slot_bindings_query_artifact(
    canonical_id: &str,
    snapshot: &TypeExpansionSnapshot,
    request: &TypeExpansionRequest,
    slot_name: &str,
) -> StdResult<ComponentMetaMembersQueryArtifact, TypeExpansionError> {
    let mut artifact = build_component_meta_artifact(canonical_id, snapshot, request)?;
    let generic_clause = extract_script_setup_generic_clause(snapshot);
    let members_offset = append_component_meta_slot_binding_probe(
        &mut artifact.generated_source,
        &snapshot.source.text,
        request.span,
        slot_name,
        generic_clause.as_deref(),
    )
    .ok_or(TypeExpansionError::NoExpansionResult)?;
    Ok(ComponentMetaMembersQueryArtifact {
        artifact,
        members_offset,
    })
}

fn append_component_meta_member_probe(
    generated: &mut String,
    source: &str,
    type_span: verter_span::Span,
    generic_clause: Option<&str>,
) -> Option<ComponentMetaProbeOffsets> {
    let type_text = source
        .get(type_span.start as usize..type_span.end as usize)?
        .trim();
    if type_text.is_empty() {
        return None;
    }

    let wrapper_open =
        component_meta_probe_wrapper_open(COMPONENT_META_QUERY_WRAPPER, generic_clause);
    let probe = format!(
        "\n{wrapper_open}  type {COMPONENT_META_QUERY_TYPE_ALIAS} = {type_text};\n  const {COMPONENT_META_QUERY_VALUE} = null as unknown as {COMPONENT_META_QUERY_TYPE_ALIAS};\n  {COMPONENT_META_QUERY_VALUE};\n  {COMPONENT_META_QUERY_VALUE}.\n}}\n"
    );
    let hover_marker = format!("\n  {COMPONENT_META_QUERY_VALUE};\n");
    let marker = format!("{COMPONENT_META_QUERY_VALUE}.");
    let base = generated.len();
    let hover_relative = probe
        .find(&hover_marker)
        .map(|offset| offset + 3)
        .unwrap_or_else(|| probe.find(COMPONENT_META_QUERY_VALUE).unwrap_or(0));
    let relative = probe.find(&marker)? + marker.len();
    generated.push_str(&probe);
    Some(ComponentMetaProbeOffsets {
        hover_offset: (base + hover_relative) as u32,
        members_offset: (base + relative) as u32,
    })
}

const COMPONENT_META_QUERY_TYPE_ALIAS: &str = "__VERTER_COMPONENT_META_QUERY";
const COMPONENT_META_QUERY_VALUE: &str = "__verter_component_meta_query";
const COMPONENT_META_QUERY_WRAPPER: &str = "__verter_component_meta_query_wrapper";
const COMPONENT_META_SLOT_BINDINGS_TYPE_ALIAS: &str = "__VERTER_COMPONENT_META_SLOT_BINDINGS";
const COMPONENT_META_SLOT_BINDINGS_VALUE: &str = "__verter_component_meta_slot_bindings";
const COMPONENT_META_SLOT_BINDINGS_WRAPPER: &str = "__verter_component_meta_slot_bindings_wrapper";

fn append_component_meta_slot_binding_probe(
    generated: &mut String,
    source: &str,
    type_span: verter_span::Span,
    slot_name: &str,
    generic_clause: Option<&str>,
) -> Option<u32> {
    let type_text = source
        .get(type_span.start as usize..type_span.end as usize)?
        .trim();
    if type_text.is_empty() {
        return None;
    }

    let wrapper_open =
        component_meta_probe_wrapper_open(COMPONENT_META_SLOT_BINDINGS_WRAPPER, generic_clause);
    let probe = format!(
        "\n{wrapper_open}  type {COMPONENT_META_QUERY_TYPE_ALIAS} = {type_text};\n  type {COMPONENT_META_SLOT_BINDINGS_TYPE_ALIAS} = Parameters<NonNullable<{COMPONENT_META_QUERY_TYPE_ALIAS}[{}]>>[0];\n  const {COMPONENT_META_SLOT_BINDINGS_VALUE} = null as unknown as {COMPONENT_META_SLOT_BINDINGS_TYPE_ALIAS};\n  {COMPONENT_META_SLOT_BINDINGS_VALUE}.\n}}\n",
        ts_string_literal(slot_name)
    );
    let marker = format!("{COMPONENT_META_SLOT_BINDINGS_VALUE}.");
    let base = generated.len();
    let relative = probe.find(&marker)? + marker.len();
    generated.push_str(&probe);
    Some((base + relative) as u32)
}

fn component_meta_probe_wrapper_open(wrapper_name: &str, generic_clause: Option<&str>) -> String {
    match generic_clause
        .map(str::trim)
        .filter(|clause| !clause.is_empty())
    {
        Some(clause) => format!("function {wrapper_name}<{clause}>() {{\n"),
        None => format!("function {wrapper_name}() {{\n"),
    }
}

fn extract_script_setup_generic_clause(snapshot: &TypeExpansionSnapshot) -> Option<String> {
    let script_setup = snapshot.sfc_structure.script_setup?;
    let content_start = script_setup.content.start as usize;
    let prefix = snapshot.source.text.get(..content_start)?;
    let open_start = prefix.rfind("<script")?;
    let open_tag = snapshot.source.text.get(open_start..content_start)?;
    extract_tag_attribute_value(open_tag, "generic")
}

fn extract_tag_attribute_value(tag_source: &str, attribute: &str) -> Option<String> {
    let mut search_from = 0usize;
    while let Some(relative) = tag_source[search_from..].find(attribute) {
        let start = search_from + relative;
        if start > 0 {
            let prev = tag_source[..start].chars().next_back()?;
            if prev.is_ascii_alphanumeric() || prev == '_' || prev == '-' {
                search_from = start + attribute.len();
                continue;
            }
        }
        let mut rest = &tag_source[start + attribute.len()..];
        rest = rest.trim_start();
        let Some(rest_after_eq) = rest.strip_prefix('=') else {
            search_from = start + attribute.len();
            continue;
        };
        let rest_after_eq = rest_after_eq.trim_start();
        let quote = rest_after_eq.chars().next()?;
        if quote != '"' && quote != '\'' {
            search_from = start + attribute.len();
            continue;
        }
        let value_rest = &rest_after_eq[quote.len_utf8()..];
        let end = value_rest.find(quote)?;
        return Some(value_rest[..end].to_string());
    }
    None
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
            .map(|member| {
                let normalized_raw_type = member
                    .type_text
                    .as_deref()
                    .map(strip_type_display_prefix)
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                    .map(str::to_string);
                ExpandedMember {
                    name: member.name,
                    raw_type: normalized_raw_type.clone(),
                    type_expr: normalized_raw_type
                        .as_deref()
                        .map(type_text_parser::parse_type_text)
                        .unwrap_or_else(|| type_text_parser::parse_type_text("unknown")),
                    optional: member.optional,
                    description: member.documentation,
                }
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
        text.push_str(
            member
                .type_text
                .as_deref()
                .map(strip_type_display_prefix)
                .unwrap_or("unknown"),
        );
    }
    text.push_str(" }");
    Some(text)
}

fn strip_type_display_prefix(contents: &str) -> &str {
    let trimmed = unwrap_markdown_code_fence(contents.trim()).trim();

    if let Some(eq_pos) = trimmed.find(" = ") {
        let after = trimmed[eq_pos + 3..].trim();
        if !after.is_empty() {
            return after;
        }
    }

    for keyword in ["const ", "let ", "var "] {
        if let Some(rest) = trimmed.strip_prefix(keyword) {
            if let Some(colon) = rest.find(':') {
                let after = rest[colon + 1..].trim();
                if !after.is_empty() {
                    return after;
                }
            }
        }
    }

    if let Some(after) = strip_quoted_member_display_prefix(trimmed) {
        return after;
    }

    if trimmed.starts_with('(') {
        if let Some(colon) = trimmed.find(':') {
            let after = trimmed[colon + 1..].trim();
            if !after.is_empty() {
                return after;
            }
        }
    }

    if let Some(optional_idx) = trimmed.find("?:") {
        let prefix = trimmed[..optional_idx].trim();
        if looks_like_member_display_prefix(prefix) {
            let after = trimmed[optional_idx + 2..].trim();
            if !after.is_empty() {
                return after;
            }
        }
    }

    if let Some(colon_idx) = trimmed.find(':') {
        let prefix = trimmed[..colon_idx].trim();
        if looks_like_member_display_prefix(prefix) {
            let after = trimmed[colon_idx + 1..].trim();
            if !after.is_empty() {
                return after;
            }
        }
    }

    trimmed
}

fn strip_quoted_member_display_prefix(trimmed: &str) -> Option<&str> {
    let candidate = trimmed
        .strip_prefix("(property) ")
        .unwrap_or(trimmed)
        .trim_start();
    let mut chars = candidate.char_indices();
    let (_, quote) = chars.next()?;
    if !matches!(quote, '\'' | '"') {
        return None;
    }
    let end = chars.find_map(|(index, ch)| (ch == quote).then_some(index))?;
    let rest = candidate[end + quote.len_utf8()..].trim_start();
    let after = rest
        .strip_prefix("?:")
        .or_else(|| rest.strip_prefix(':'))?
        .trim();
    (!after.is_empty()).then_some(after)
}

fn unwrap_markdown_code_fence(contents: &str) -> &str {
    let Some(rest) = contents.strip_prefix("```") else {
        return contents;
    };
    let Some(first_newline) = rest.find('\n') else {
        return contents;
    };
    let body = &rest[first_newline + 1..];
    let Some(closing) = body.rfind("\n```") else {
        return contents;
    };
    &body[..closing]
}

fn looks_like_member_display_prefix(prefix: &str) -> bool {
    if prefix.is_empty()
        || prefix.starts_with('{')
        || prefix.starts_with('[')
        || prefix.contains("=>")
        || prefix.contains(';')
    {
        return false;
    }

    prefix.chars().all(|ch| {
        ch.is_alphanumeric()
            || matches!(
                ch,
                '_' | '$' | '.' | '<' | '>' | ',' | ' ' | '?' | '[' | ']' | '\'' | '"'
            )
    })
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
                    raw_type: None,
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
            let ws: Arc<dyn verter_workspace::WorkspaceAccess> = workspace.workspace();
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
            let result = session
                .get_component_meta(&canonical_or_alias)
                .map_err(meta_err)?;
            match result {
                Some(analysis) => {
                    let ffi = verter_ffi::convert::component_meta_analysis_to_ffi(analysis);
                    let payload =
                        verter_protocol::component_meta::encode_component_meta_payload(&ffi);
                    Ok(Some(Buffer::from(payload)))
                }
                None => Ok(None),
            }
        }))?
    }

    #[napi(js_name = "getResolvedComponentMeta")]
    pub fn get_resolved_component_meta(
        &self,
        canonical_or_alias: String,
    ) -> Result<Option<Buffer>> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let result = session
                .get_component_meta_with_resolution(&canonical_or_alias)
                .map_err(meta_err)?;
            match result {
                Some((analysis, resolved)) => {
                    let ffi = verter_ffi::convert::component_meta_analysis_to_ffi_with_resolution(
                        analysis,
                        Some(&resolved),
                    );
                    let payload =
                        verter_protocol::component_meta::encode_component_meta_payload(&ffi);
                    Ok(Some(Buffer::from(payload)))
                }
                None => Ok(None),
            }
        }))?
    }

    #[napi(js_name = "getDeclaredComponentMeta")]
    pub fn get_declared_component_meta(
        &self,
        canonical_or_alias: String,
    ) -> Result<Option<Buffer>> {
        let session = self.session()?;
        catch_panic(std::panic::AssertUnwindSafe(|| {
            let result = session
                .get_declared_component_meta(&canonical_or_alias)
                .map_err(meta_err)?;
            match result {
                Some(analysis) => {
                    let ffi = verter_ffi::convert::component_meta_analysis_to_ffi(analysis);
                    let payload =
                        verter_protocol::component_meta::encode_component_meta_payload(&ffi);
                    Ok(Some(Buffer::from(payload)))
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
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex as StdMutex;

    struct FakeComponentMetaTypeExpander;

    impl ComponentMetaTypeExpander for FakeComponentMetaTypeExpander {
        fn expand_type(
            &self,
            _request: &TypeExpansionRequest,
            _snapshot: TypeExpansionSnapshot,
            _trace_cursor: Option<ComponentMetaTraceCursor>,
        ) -> StdResult<TypeExpansionResult, TypeExpansionError> {
            Err(TypeExpansionError::NoExpansionResult)
        }
    }

    #[derive(Default)]
    struct FakeGeneratedQueryBackend {
        synced_sources: StdMutex<Vec<String>>,
        member_results: StdMutex<VecDeque<StdResult<BackendTypeData, BackendError>>>,
        definition_results: StdMutex<VecDeque<StdResult<BackendTypeData, BackendError>>>,
        hover_results: StdMutex<VecDeque<StdResult<BackendTypeData, BackendError>>>,
    }

    impl GeneratedQueryBackend for FakeGeneratedQueryBackend {
        fn sync_file<'a>(
            &'a self,
            _file_id: &'a GeneratedFileId,
            _revision: u64,
            content: &'a str,
        ) -> verter_type_runtime::BackendFuture<'a, ()> {
            self.synced_sources
                .lock()
                .unwrap()
                .push(content.to_string());
            Box::pin(async { Ok(()) })
        }

        fn close_file<'a>(
            &'a self,
            _file_id: &'a GeneratedFileId,
        ) -> verter_type_runtime::BackendFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn evict_file<'a>(
            &'a self,
            _file_id: &'a GeneratedFileId,
        ) -> verter_type_runtime::BackendFuture<'a, ()> {
            Box::pin(async { Ok(()) })
        }

        fn query_type_data<'a>(
            &'a self,
            _file_id: &'a GeneratedFileId,
            _expected_revision: u64,
            _generated_offset: u32,
            query: BackendTypeQuery,
        ) -> verter_type_runtime::BackendFuture<'a, BackendTypeData> {
            Box::pin(async move {
                match query {
                    BackendTypeQuery::MembersAtOffset => self
                        .member_results
                        .lock()
                        .unwrap()
                        .pop_front()
                        .unwrap_or_else(|| Ok(BackendTypeData::default())),
                    BackendTypeQuery::DefinitionTypeAtOffset => self
                        .definition_results
                        .lock()
                        .unwrap()
                        .pop_front()
                        .unwrap_or_else(|| Ok(BackendTypeData::default())),
                    BackendTypeQuery::TypeAtOffset => self
                        .hover_results
                        .lock()
                        .unwrap()
                        .pop_front()
                        .unwrap_or_else(|| Ok(BackendTypeData::default())),
                    _ => Ok(BackendTypeData::default()),
                }
            })
        }

        fn shutdown(&self) -> verter_type_runtime::BackendFuture<'_, ()> {
            Box::pin(async { Ok(()) })
        }
    }

    #[test]
    fn create_component_meta_host_installs_expander_for_non_verter_backend() {
        let mut host_config = verter_session::HostConfig::default();
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
        let host_config = verter_session::HostConfig::default();
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

    fn setup_only_snapshot(source: &str) -> TypeExpansionSnapshot {
        let script_open_end = source.find('>').expect("script setup open tag") + 1;
        let script_close = source.find("</script>").expect("script setup close tag");
        TypeExpansionSnapshot {
            source: verter_resolver::type_expansion_host::SourceSnapshot {
                text: source.to_string(),
                lang: verter_resolver::type_expansion_host::ScriptLang::Ts,
            },
            sfc_structure: verter_resolver::type_expansion_host::SfcStructure {
                script: None,
                script_setup: Some(verter_resolver::type_expansion_host::SfcBlockSpan {
                    content: verter_span::Span::new(script_open_end as u32, script_close as u32),
                }),
                template: None,
            },
            revision: 1,
        }
    }

    #[test]
    fn extract_script_setup_generic_clause_reads_generic_attribute() {
        let snapshot = setup_only_snapshot(
            r#"<script setup lang="ts" generic="T extends Item = Item">
defineProps<Props<T>>()
</script>"#,
        );

        assert_eq!(
            extract_script_setup_generic_clause(&snapshot).as_deref(),
            Some("T extends Item = Item")
        );
    }

    #[test]
    fn component_meta_query_artifact_wraps_script_setup_generics() {
        let source = r#"<script setup lang="ts" generic="T extends Item = Item">
defineProps<Props<T>>()
</script>"#;
        let snapshot = setup_only_snapshot(source);
        let type_start = source.find("Props<T>").expect("type reference");
        let script_open_end = source.find('>').expect("script setup open tag") + 1;
        let script_close = source.find("</script>").expect("script setup close tag");
        let base_generated_len = (script_close - script_open_end) + 1;
        let request = TypeExpansionRequest {
            canonical_id: "/src/Generic.vue".to_string(),
            span: verter_span::Span::new(type_start as u32, (type_start + "Props<T>".len()) as u32),
            profile: ExpansionProfile::ComponentMeta,
        };

        let artifact =
            build_component_meta_query_artifact("/src/Generic.vue", &snapshot, &request).unwrap();

        assert_eq!(
            artifact.generic_clause.as_deref(),
            Some("T extends Item = Item")
        );
        assert!(
            artifact.artifact.generated_source.contains(
                "function __verter_component_meta_query_wrapper<T extends Item = Item>()"
            ),
            "query artifact should wrap the probe in a generic function: {}",
            artifact.artifact.generated_source
        );
        assert!(
            artifact.hover_offset as usize >= base_generated_len,
            "hover query should point at the appended probe so generic params are in scope"
        );
        assert!(
            artifact
                .members_offset
                .is_some_and(|offset| offset as usize >= base_generated_len),
            "member query should point at the appended probe so generic params are in scope"
        );
    }

    #[test]
    fn runtime_component_meta_slot_bindings_degrade_backend_member_errors() {
        let backend = Arc::new(FakeGeneratedQueryBackend {
            member_results: StdMutex::new(VecDeque::from([Err(BackendError::BackendReported(
                "No content available.".to_string(),
            ))])),
            ..Default::default()
        });
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let expander = RuntimeBackedComponentMetaExpander::new(
            "tsserver",
            runtime,
            backend,
            "component-meta-tsserver",
        );
        let source = r#"<script setup lang="ts">
type Slots = {
  description(props: { label: string }): any
}
defineSlots<Slots>()
</script>"#;
        let snapshot = setup_only_snapshot(source);
        let type_start = source.rfind("Slots").expect("type reference");
        let request = TypeExpansionRequest {
            canonical_id: "/src/AuthForm.vue".to_string(),
            span: verter_span::Span::new(type_start as u32, (type_start + "Slots".len()) as u32),
            profile: ExpansionProfile::ComponentMeta,
        };

        let result = expander
            .expand_slot_bindings(&request, snapshot, "description", None)
            .expect("slot binding failures should degrade");

        assert!(result.is_none());
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

        let mut host_config = verter_session::HostConfig::default();
        host_config.type_expansion_backend = TypeExpansionBackend::Tsgo;
        let workspace_roots = vec![test_root.to_string_lossy().into_owned()];
        let host = create_component_meta_host(host_config, None, &workspace_roots).unwrap();

        host.upsert_base(&types_id, types_source).unwrap();
        host.upsert_base(&app_id, app_source).unwrap();
        host.host().set_import_dependencies(
            &app_id,
            vec![verter_session::DependencyResolution {
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

    #[test]
    fn strip_type_display_prefix_handles_member_display_forms() {
        assert_eq!(
            strip_type_display_prefix("(property) collapsible?: boolean | undefined"),
            "boolean | undefined"
        );
        assert_eq!(
            strip_type_display_prefix("AccordionProps<T>.items?: T[] | undefined"),
            "T[] | undefined"
        );
        assert_eq!(
            strip_type_display_prefix(
                "const __verter_component_meta_member_0: SingleOrMultipleType | undefined"
            ),
            "SingleOrMultipleType | undefined"
        );
        assert_eq!(
            strip_type_display_prefix(
                "```typescript\n(const) const __VERTER_COMPONENT_META_MEMBER_2: any\n```"
            ),
            "any"
        );
        assert_eq!(
            strip_type_display_prefix(
                "(property) 'update:modelValue': [value: (T extends 'single' ? string : string[]) | undefined]"
            ),
            "[value: (T extends 'single' ? string : string[]) | undefined]"
        );
    }

    #[test]
    fn member_type_text_needs_hover_for_any_members() {
        assert!(member_type_text_needs_hover(
            &verter_type_runtime::BackendTypeMember {
                name: "labelKey".to_string(),
                type_text: Some("labelKey?: any".to_string()),
                optional: true,
                documentation: None,
            }
        ));
        assert!(!member_type_text_needs_hover(
            &verter_type_runtime::BackendTypeMember {
                name: "items".to_string(),
                type_text: Some("items?: T[] | undefined".to_string()),
                optional: true,
                documentation: None,
            }
        ));
    }

    #[test]
    fn definition_hover_should_replace_member_rejects_non_member_displays() {
        let data = BackendTypeData {
            type_text: None,
            members: vec![verter_type_runtime::BackendTypeMember {
                name: "collapsible".to_string(),
                type_text: Some("collapsible?: boolean | undefined".to_string()),
                optional: true,
                documentation: None,
            }],
            documentation: None,
            completeness: BackendTypeCompleteness::Exact,
        };

        assert!(definition_hover_should_replace_member(
            "collapsible",
            &data,
            "(property) collapsible?: boolean | undefined"
        ));
        assert!(!definition_hover_should_replace_member(
            "collapsible",
            &data,
            "(function) function createContext<ContextValue>(providerComponentName: string | string[])"
        ));
        assert!(!definition_hover_should_replace_member(
            "collapsible",
            &data,
            "(type parameter) T in <T extends ContextValue | null | undefined = ContextValue>(fallback?: T)"
        ));
    }

    #[test]
    fn type_expansion_from_backend_data_normalizes_member_raw_types() {
        let result = type_expansion_from_backend_data(BackendTypeData {
            type_text: None,
            members: vec![
                verter_type_runtime::BackendTypeMember {
                    name: "items".to_string(),
                    type_text: Some("AccordionProps<T>.items?: T[] | undefined".to_string()),
                    optional: true,
                    documentation: None,
                },
                verter_type_runtime::BackendTypeMember {
                    name: "type".to_string(),
                    type_text: Some(
                        "const __verter_component_meta_member_0: SingleOrMultipleType | undefined"
                            .to_string(),
                    ),
                    optional: true,
                    documentation: None,
                },
            ],
            documentation: None,
            completeness: BackendTypeCompleteness::Exact,
        })
        .expect("member-only backend expansion should succeed");

        let raw_types: Vec<_> = result
            .members
            .iter()
            .map(|member| (member.name.as_str(), member.raw_type.as_deref()))
            .collect();

        assert_eq!(
            raw_types,
            vec![
                ("items", Some("T[] | undefined")),
                ("type", Some("SingleOrMultipleType | undefined")),
            ]
        );
    }

    #[tokio::test]
    async fn fill_missing_backend_member_types_prefers_definition_site_text() {
        let backend = FakeGeneratedQueryBackend {
            synced_sources: StdMutex::new(Vec::new()),
            member_results: StdMutex::new(VecDeque::new()),
            definition_results: StdMutex::new(VecDeque::from([
                Ok(BackendTypeData {
                    type_text: Some("(property) type?: SingleOrMultipleType | undefined".to_string()),
                    members: vec![],
                    documentation: None,
                    completeness: BackendTypeCompleteness::Exact,
                }),
                Ok(BackendTypeData {
                    type_text: Some(
                        "(property) 'update:modelValue': [value: (T extends 'single' ? string : string[]) | undefined]"
                            .to_string(),
                    ),
                    members: vec![],
                    documentation: None,
                    completeness: BackendTypeCompleteness::Exact,
                }),
            ])),
            hover_results: StdMutex::new(VecDeque::from([Ok(BackendTypeData {
                type_text: Some("const __VERTER_COMPONENT_META_MEMBER_0: any".to_string()),
                members: vec![],
                documentation: None,
                completeness: BackendTypeCompleteness::Exact,
            })])),
        };
        let file_id = GeneratedFileId {
            canonical_id: "/src/Foo.vue".into(),
            profile: verter_type_runtime::ArtifactProfile::ComponentMeta,
            runtime_key: "test".into(),
        };
        let data = BackendTypeData {
            type_text: None,
            members: vec![
                verter_type_runtime::BackendTypeMember {
                    name: "type".to_string(),
                    type_text: Some(
                        "const __verter_component_meta_member_0: SingleOrMultipleType | undefined"
                            .to_string(),
                    ),
                    optional: true,
                    documentation: None,
                },
                verter_type_runtime::BackendTypeMember {
                    name: "update:modelValue".to_string(),
                    type_text: Some("unknown".to_string()),
                    optional: false,
                    documentation: None,
                },
            ],
            documentation: None,
            completeness: BackendTypeCompleteness::Exact,
        };

        let filled = fill_missing_backend_member_types(
            &backend,
            &file_id,
            1,
            "type Query = {}",
            COMPONENT_META_QUERY_TYPE_ALIAS,
            COMPONENT_META_QUERY_VALUE,
            "__VERTER_COMPONENT_META_MEMBER",
            data,
        )
        .await
        .expect("member fill should succeed");

        let filled_types: Vec<_> = filled
            .members
            .iter()
            .map(|member| (member.name.as_str(), member.type_text.as_deref()))
            .collect();
        assert_eq!(
            filled_types,
            vec![
                ("type", Some("SingleOrMultipleType | undefined")),
                (
                    "update:modelValue",
                    Some("[value: (T extends 'single' ? string : string[]) | undefined]")
                ),
            ]
        );
        let synced_sources = backend.synced_sources.lock().unwrap();
        assert_eq!(synced_sources.len(), 1);
        assert!(
            synced_sources[0].contains("__VERTER_COMPONENT_META_QUERY[\"update:modelValue\"]"),
            "definition probe should query the raw indexed-access member"
        );
    }
}
