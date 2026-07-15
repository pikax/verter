//! Shared helpers: path resolution, auto-upsert, JSON formatting.

use std::path::Path;
use std::sync::Arc;

use rmcp::model::ErrorData as McpError;
use verter_session::{CompileProfile, UpsertRequest, VerterHost};

/// Normalize a path: resolve relative to project root, forward-slash normalize.
pub fn resolve_path(path: &str, project_root: Option<&Path>) -> String {
    let p = Path::new(path);
    if p.is_absolute() {
        path.replace('\\', "/")
    } else if let Some(root) = project_root {
        root.join(path).to_string_lossy().replace('\\', "/")
    } else {
        path.replace('\\', "/")
    }
}

/// Ensure a file is loaded in the host. Reads from the workspace if not already present.
pub fn ensure_loaded(host: &VerterHost, canonical_id: &str) -> Result<(), McpError> {
    if host.get_source(canonical_id).is_some() {
        return Ok(());
    }
    let workspace = host.workspace_read();
    let source = workspace.read_file(canonical_id).ok_or_else(|| McpError {
        code: rmcp::model::ErrorCode::INTERNAL_ERROR,
        message: format!("Cannot read file {} via workspace", canonical_id).into(),
        data: None,
    })?;
    let _update = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::from(source.as_ref()),
            file_language: host.language_classifier().classify(canonical_id),
            aliases: vec![],
        })
        .map_err(|e| McpError {
            code: rmcp::model::ErrorCode::INTERNAL_ERROR,
            message: format!("Cannot load file {canonical_id}: {e}").into(),
            data: None,
        })?;
    Ok(())
}

/// Ensure template analysis is available (requires a compilation pass).
pub fn ensure_template_analysis(host: &VerterHost, canonical_id: &str) -> Result<(), McpError> {
    ensure_loaded(host, canonical_id)?;
    if let Some(analysis) = host.get_analysis(canonical_id) {
        if analysis.template.is_some() {
            return Ok(());
        }
    }
    // Trigger compilation to populate template analysis.
    // Use ANALYSIS target (script + template data) — skips style and VDOM codegen.
    let profile = CompileProfile {
        target: verter_session::CompileTarget::ANALYSIS,
        ..CompileProfile::default()
    };
    let _ = host.ensure_compiled(canonical_id, &profile);
    Ok(())
}

/// Ensure template analysis for multiple files, then return all snapshots in batch.
/// ensure all files are loaded and have template analysis.
/// batch-fetch all analysis snapshots (single lock acquisition).
pub fn batch_analysis_with_template(
    host: &VerterHost,
    canonical_ids: &[&str],
) -> Vec<(String, verter_session::FileAnalysisSnapshot)> {
    for id in canonical_ids {
        let _ = ensure_template_analysis(host, id);
    }
    host.get_analysis_batch(canonical_ids)
}

/// Create an McpError from a string message.
pub fn mcp_err(msg: impl Into<String>) -> McpError {
    McpError {
        code: rmcp::model::ErrorCode::INTERNAL_ERROR,
        message: msg.into().into(),
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_session::HostConfig;

    fn host_with_file(canonical: &str, source: &str) -> VerterHost {
        let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
            verter_workspace::MemoryOptions::default(),
        ));
        workspace.inject_file(canonical.to_string(), Arc::from(source));
        VerterHost::new(HostConfig::default(), workspace)
    }

    /// `ensure_loaded` loads a `.svelte` carrier through the registered Svelte
    /// carrier: the path classifies to the carrier row and the load
    /// produces source state. A `.svelte` path is never carrier-less
    /// rejected now the carrier registers; the typed unsupported-language
    /// propagation stays exercised at the session layer (the Vue
    /// framework-template / same-adapter non-carrier rows).
    #[test]
    fn ensure_loaded_loads_svelte_through_the_registered_carrier() {
        let host = host_with_file(
            "/ws/src/Box.svelte",
            "<script lang=\"ts\">let { x }: { x: number } = $props();</script>",
        );
        ensure_loaded(&host, "/ws/src/Box.svelte")
            .expect("the registered Svelte carrier loads the .svelte file");
        assert!(
            host.get_source("/ws/src/Box.svelte").is_some(),
            "source state exists after the Svelte carrier load"
        );
    }

    /// Plain scripts keep loading exactly as before.
    #[test]
    fn ensure_loaded_still_loads_plain_scripts() {
        let host = host_with_file("/ws/src/util.ts", "export const x = 1;");
        ensure_loaded(&host, "/ws/src/util.ts").expect("a plain script loads");
        assert!(
            host.get_source("/ws/src/util.ts").is_some(),
            "the script's source state must exist after the load"
        );
    }
}
