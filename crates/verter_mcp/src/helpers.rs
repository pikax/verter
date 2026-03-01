//! Shared helpers: path resolution, auto-upsert, JSON formatting.

use std::path::Path;
use std::sync::Arc;

use rmcp::model::ErrorData as McpError;
use verter_host::{CompileProfile, FileKind, UpsertRequest, VerterHost};

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

/// Ensure a file is loaded in the host. Reads from disk if not already present.
pub fn ensure_loaded(host: &VerterHost, canonical_id: &str) -> Result<(), McpError> {
    if host.get_source(canonical_id).is_some() {
        return Ok(());
    }
    let source = std::fs::read_to_string(canonical_id).map_err(|e| McpError {
        code: rmcp::model::ErrorCode::INTERNAL_ERROR,
        message: format!("Cannot read file {}: {}", canonical_id, e).into(),
        data: None,
    })?;
    let file_kind = if canonical_id.ends_with(".vue") {
        FileKind::VueSfc
    } else {
        FileKind::NonSfc
    };
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical_id.to_string()),
        input_id: canonical_id.to_string(),
        source: Arc::from(source.as_str()),
        file_kind,
        aliases: vec![],
    });
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
        target: verter_host::CompileTarget::ANALYSIS,
        ..CompileProfile::default()
    };
    let _ = host.ensure_compiled(canonical_id, &profile);
    Ok(())
}

/// Create an McpError from a string message.
pub fn mcp_err(msg: impl Into<String>) -> McpError {
    McpError {
        code: rmcp::model::ErrorCode::INTERNAL_ERROR,
        message: msg.into().into(),
        data: None,
    }
}
