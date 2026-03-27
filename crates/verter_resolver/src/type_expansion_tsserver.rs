//! tsserver-backed `TypeExpander` implementation.
//!
//! Consumes a `TsserverTypeProvider` session from `verter_type_runtime`
//! and uses hover queries to resolve types at macro positions.
//!
//! Flow:
//! 1. Accept `TypeExpansionRequest` with SFC span
//! 2. Get coherent snapshot from `TypeExpansionHost`
//! 3. Build `GeneratedQueryArtifact` under requested profile
//! 4. Map SFC span → generated offset via artifact
//! 5. Sync generated file to backend via `TypeProvider::load_file()`
//! 6. Query hover at generated offset
//! 7. Parse hover text → `TypeExpansionResult` via `type_text_parser`
//! 8. Normalize `ExpansionCompleteness`

#[cfg(feature = "type-runtime")]
use std::sync::Arc;

#[cfg(feature = "type-runtime")]
use verter_type_runtime::{HoverInfo, TypeProvider};

#[cfg(feature = "type-runtime")]
use crate::query_artifact::GeneratedQueryArtifact;
#[cfg(feature = "type-runtime")]
use crate::type_expansion::{
    ExpandedMember, ExpanderFuture, ExpansionCompleteness, TypeExpander, TypeExpansionError,
    TypeExpansionRequest, TypeExpansionResult,
};
#[cfg(feature = "type-runtime")]
use crate::type_expansion_host::TypeExpansionHost;
#[cfg(feature = "type-runtime")]
use crate::type_text_parser;

/// tsserver-backed `TypeExpander`.
///
/// Uses quickinfo (hover) at generated offsets to resolve types.
/// The generated artifact is built from the SFC snapshot under the
/// requested profile and synced to the tsserver process.
#[cfg(feature = "type-runtime")]
pub struct TsserverTypeExpander<H: TypeExpansionHost> {
    host: Arc<H>,
    provider: Arc<dyn TypeProvider>,
}

#[cfg(feature = "type-runtime")]
impl<H: TypeExpansionHost> TsserverTypeExpander<H> {
    pub fn new(host: Arc<H>, provider: Arc<dyn TypeProvider>) -> Self {
        Self { host, provider }
    }
}

#[cfg(feature = "type-runtime")]
impl<H: TypeExpansionHost + Send + Sync + 'static> TypeExpander for TsserverTypeExpander<H> {
    fn expand_type<'a>(
        &'a self,
        request: &'a TypeExpansionRequest,
    ) -> ExpanderFuture<'a, TypeExpansionResult> {
        Box::pin(async move {
            // 1. Get snapshot from host
            let snapshot = self
                .host
                .snapshot_view(&request.canonical_id)
                .map_err(|_| TypeExpansionError::SourceUnavailable)?;

            // 2. Build generated artifact
            // TODO: Artifact construction from snapshot + profile
            // For now, use the raw source as the generated content
            let artifact = build_minimal_artifact(
                &request.canonical_id,
                &snapshot.source.text,
                &snapshot,
                request,
            )?;

            // 3. Map SFC span → generated offset
            let generated_offset = artifact
                .sfc_to_generated(request.span.start)
                .ok_or(TypeExpansionError::MappingFailed)?;

            // 4. Sync file to provider
            let virtual_path = artifact.artifact_id.virtual_path();
            self.provider
                .load_file(&virtual_path, &artifact.generated_source)
                .await
                .map_err(|_| {
                    TypeExpansionError::BackendFailure(
                        crate::type_expansion::BackendFailureKind::Unavailable,
                    )
                })?;

            // 5. Query hover at generated offset
            let hover = self
                .provider
                .get_hover(&virtual_path, generated_offset)
                .await
                .map_err(|_| {
                    TypeExpansionError::BackendFailure(
                        crate::type_expansion::BackendFailureKind::TimedOut,
                    )
                })?;

            // 6. Parse hover response
            match hover {
                Some(info) => parse_hover_to_expansion(&info),
                None => Err(TypeExpansionError::NoExpansionResult),
            }
        })
    }
}

/// Parse a hover response into a `TypeExpansionResult`.
#[cfg(feature = "type-runtime")]
pub(crate) fn parse_hover_to_expansion(
    info: &HoverInfo,
) -> Result<TypeExpansionResult, TypeExpansionError> {
    let contents = info.contents.trim();
    if contents.is_empty() {
        return Err(TypeExpansionError::NoExpansionResult);
    }

    // Extract the type text from the hover response.
    // tsserver quickinfo format: "type TypeName = { ... }" or just "{ ... }"
    let type_text = extract_type_from_hover(contents);
    let type_expr = type_text_parser::parse_type_text(type_text);

    // Determine completeness based on whether we got Unknown
    let completeness = match &type_expr {
        verter_analysis::type_expr::TypeExpr::Unknown { .. } => {
            ExpansionCompleteness::OpaqueFallback
        }
        _ => ExpansionCompleteness::Exact,
    };

    // Extract members if the type is an object
    let members = extract_members_from_type(&type_expr);

    Ok(TypeExpansionResult {
        type_expr,
        members,
        completeness,
    })
}

/// Extract the type text from a hover/quickinfo response.
///
/// Handles formats like:
/// - `"type ButtonProps = { msg: string; count?: number }"` → `"{ msg: string; count?: number }"`
/// - `"{ msg: string; count?: number }"` → `"{ msg: string; count?: number }"`
/// - `"(property) msg: string"` → `"string"`
#[cfg(feature = "type-runtime")]
pub(crate) fn extract_type_from_hover(contents: &str) -> &str {
    // Skip "type Name = " prefix (match " = " to avoid false positives with comparison operators)
    if let Some(eq_pos) = contents.find(" = ") {
        let after = contents[eq_pos + 3..].trim();
        if !after.is_empty() {
            return after;
        }
    }
    // Skip "(kind) name: " prefix
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

/// Extract members from an object type expression.
#[cfg(feature = "type-runtime")]
pub(crate) fn extract_members_from_type(
    type_expr: &verter_analysis::type_expr::TypeExpr,
) -> Vec<ExpandedMember> {
    use verter_analysis::type_expr::{ObjectMember, TypeExpr};

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
        _ => vec![],
    }
}

/// Build a generated artifact from the SFC snapshot for backend queries.
///
/// For `ComponentMeta` profile: merges script blocks with cleanup:
/// - Strips `export default { ... }` from companion `<script>` (runtime config)
/// - Preserves imports, type declarations, and setup body
/// - Tracks SFC span → generated offset mappings
///
/// For `Lsp` profile: same merge (full IDE codegen requires verter_core pipeline).
#[cfg(feature = "type-runtime")]
pub(crate) fn build_minimal_artifact(
    canonical_id: &str,
    source: &str,
    snapshot: &crate::type_expansion_host::TypeExpansionSnapshot,
    request: &TypeExpansionRequest,
) -> Result<GeneratedQueryArtifact, TypeExpansionError> {
    use crate::query_artifact::{ArtifactId, ArtifactProfile, QuerySpanMapping};

    let profile = ArtifactProfile::from(request.profile);
    let mut generated = String::new();
    let mut mappings = Vec::new();

    // Companion <script> block: extract imports and type declarations,
    // strip `export default { ... }` (runtime-only Options API config).
    if let Some(script) = &snapshot.sfc_structure.script {
        let start = script.content.start;
        let end = script.content.end;
        let block_text = &source[start as usize..end as usize];
        let gen_offset = generated.len() as u32;

        // Strip `export default` blocks (simple heuristic: find and remove)
        let cleaned = strip_export_default(block_text);
        generated.push_str(&cleaned);
        generated.push('\n');

        mappings.push(QuerySpanMapping {
            sfc_span: script.content,
            generated_offset: gen_offset,
            generated_len: cleaned.len() as u32,
        });
    }

    // <script setup> block: include in full (macros, imports, bindings)
    if let Some(setup) = &snapshot.sfc_structure.script_setup {
        let start = setup.content.start;
        let end = setup.content.end;
        let block_text = &source[start as usize..end as usize];
        let gen_offset = generated.len() as u32;
        generated.push_str(block_text);
        generated.push('\n');
        mappings.push(QuerySpanMapping {
            sfc_span: setup.content,
            generated_offset: gen_offset,
            generated_len: (end - start),
        });
    }

    if generated.is_empty() {
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

/// Strip `export default { ... }` or `export default defineComponent({ ... })`
/// from companion script content. Preserves everything else (imports, types, etc.).
#[cfg(feature = "type-runtime")]
fn strip_export_default(content: &str) -> String {
    // Find "export default" at the start of a line (possibly with leading whitespace)
    let mut result = String::with_capacity(content.len());
    let mut skip_until_close = false;
    let mut brace_depth = 0i32;

    for line in content.lines() {
        let trimmed = line.trim();
        if skip_until_close {
            // Count braces to find the end of the export default block
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
            // Check if it contains an opening brace on this line
            if let Some(brace_pos) = trimmed.find('{') {
                brace_depth = 1;
                // Count remaining braces on this line
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
                // Either way, skip this line
                continue;
            }
            // Single-line export default without braces — skip the line
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "type-runtime"))]
mod tests {
    use super::*;

    #[test]
    fn extract_type_from_hover_strips_type_prefix() {
        assert_eq!(
            extract_type_from_hover("type ButtonProps = { msg: string }"),
            "{ msg: string }"
        );
    }

    #[test]
    fn extract_type_from_hover_strips_property_prefix() {
        assert_eq!(extract_type_from_hover("(property) msg: string"), "string");
    }

    #[test]
    fn extract_type_from_hover_returns_raw_for_plain_type() {
        assert_eq!(
            extract_type_from_hover("{ msg: string }"),
            "{ msg: string }"
        );
    }

    #[test]
    fn parse_hover_to_expansion_object() {
        let info = HoverInfo {
            contents: "type Props = { msg: string; count?: number }".to_string(),
            range_start: None,
            range_end: None,
        };
        let result = parse_hover_to_expansion(&info).unwrap();
        assert_eq!(result.completeness, ExpansionCompleteness::Exact);
        assert_eq!(result.members.len(), 2);
        assert_eq!(result.members[0].name, "msg");
        assert!(!result.members[0].optional);
        assert_eq!(result.members[1].name, "count");
        assert!(result.members[1].optional);
    }

    #[test]
    fn parse_hover_to_expansion_empty_returns_error() {
        let info = HoverInfo {
            contents: "".to_string(),
            range_start: None,
            range_end: None,
        };
        assert!(parse_hover_to_expansion(&info).is_err());
    }

    #[test]
    fn parse_hover_no_implicit_fallback_to_verter() {
        // Backend failure must return TypeExpansionError, never silently use Verter
        let info = HoverInfo {
            contents: "".to_string(),
            range_start: None,
            range_end: None,
        };
        match parse_hover_to_expansion(&info) {
            Err(TypeExpansionError::NoExpansionResult) => {} // correct
            other => panic!("expected NoExpansionResult, got: {other:?}"),
        }
    }
}
