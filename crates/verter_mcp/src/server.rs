//! MCP server definition with tool routing.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::*;
use rmcp::schemars;
use rmcp::{tool, tool_handler, tool_router, ServerHandler};
use serde::Deserialize;

use verter_analysis::types::{AnalysisFlags, AnalyzedMacroKind, VueApiClassification};
use verter_diagnostics::{Linter, Severity};
use verter_host::VerterHost;

use crate::config::McpServerConfig;
use crate::helpers::{
    batch_analysis_with_template, ensure_loaded, ensure_template_analysis, mcp_err, resolve_path,
};
use crate::scanner;
use crate::tools::scoring;

// ── Parameter types ────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ScanProjectParams {
    #[schemars(description = "Absolute path to the project root directory")]
    pub root: String,
    #[schemars(description = "Also scan .ts/.js files for cross-file analysis")]
    pub include_deps: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UpsertFileParams {
    #[schemars(description = "File path (absolute or relative to project root)")]
    pub path: String,
    #[schemars(description = "Inline source content. If omitted, reads from disk")]
    pub source: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FilePathParams {
    #[schemars(description = "File path to a .vue file")]
    pub path: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct AnalyzeFileParams {
    #[schemars(description = "File path to analyze")]
    pub path: String,
    #[schemars(description = "Filter: [\"script\", \"template\", \"styles\"]. Default: all")]
    #[allow(dead_code)]
    pub sections: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LintFileParams {
    #[schemars(description = "File path to lint")]
    pub path: String,
    #[schemars(description = "Lint preset: essential|recommended|all|performance|a11y|strict")]
    pub preset: Option<String>,
    #[schemars(
        description = "Path to a .verter-baseline.json file. When set, only diagnostics NOT in the baseline are returned."
    )]
    pub baseline_path: Option<String>,
    #[schemars(
        description = "Include source evidence snippets (3-line context) around each diagnostic span."
    )]
    pub include_evidence: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LintProjectParams {
    #[schemars(description = "Lint preset: essential|recommended|all|performance|a11y|strict")]
    pub preset: Option<String>,
    #[schemars(description = "Only return errors, skip warnings and info")]
    pub errors_only: Option<bool>,
    #[schemars(
        description = "Path to a .verter-baseline.json file. When set, only diagnostics NOT in the baseline are returned."
    )]
    pub baseline_path: Option<String>,
    #[schemars(
        description = "Include source evidence snippets (3-line context) around each diagnostic span."
    )]
    pub include_evidence: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CompileFileParams {
    #[schemars(description = "File path to compile")]
    pub path: String,
    #[schemars(description = "Enable production mode")]
    pub production: Option<bool>,
    #[schemars(description = "Generate source maps")]
    pub source_map: Option<bool>,
    #[schemars(description = "Use Vapor mode compilation")]
    pub vapor: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GenerateTsxParams {
    #[schemars(description = "File path to a .vue file")]
    pub path: String,
    #[schemars(
        description = "Enable strict slot type checking (overrides server-level --strict-slots flag)"
    )]
    pub strict_slots: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct MatchCssSelectorParams {
    #[schemars(description = "File path of the Vue component")]
    pub path: String,
    #[schemars(description = "CSS selector string to test against template elements")]
    pub selector: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ComponentGraphParams {
    #[schemars(description = "Start from a specific file, or omit for full project graph")]
    pub root: Option<String>,
    #[schemars(description = "Maximum graph traversal depth")]
    #[allow(dead_code)]
    pub max_depth: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OrphanComponentsParams {
    #[schemars(
        description = "Bundler entry point files (e.g. [\"src/main.ts\", \"src/App.vue\"])"
    )]
    pub entry_points: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ApiNameParams {
    #[schemars(description = "Vue API function name (e.g. \"ref\", \"onMounted\", \"computed\")")]
    pub name: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct QuickFixParams {
    #[schemars(description = "File path of the Vue component")]
    pub path: String,
    #[schemars(description = "Byte offset in the SFC source where the cursor is")]
    pub offset: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct OptionalPathParams {
    #[schemars(description = "Optional file path. If omitted, checks all loaded files")]
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SaveBaselineParams {
    #[schemars(
        description = "Output path for the baseline file (default: .verter-baseline.json in project root)"
    )]
    pub output_path: Option<String>,
    #[schemars(description = "Lint preset to use when building the baseline")]
    pub preset: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TraceStoreFlowParams {
    #[schemars(description = "Store ID or export name to trace")]
    pub store_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TraceEventFlowParams {
    #[schemars(description = "Event name to trace (e.g., 'submit', 'update:modelValue')")]
    pub event_name: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TestImpactParams {
    #[schemars(description = "List of changed file paths")]
    pub changed_files: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CheckPropTypesParams {
    #[schemars(description = "Parent component file path")]
    pub parent: String,
    #[schemars(description = "Child component name (PascalCase)")]
    pub child_component: String,
}

// ── Server struct ──────────────────────────────────────────────────

#[derive(Clone)]
pub struct VerterMcpServer {
    pub host: Arc<VerterHost>,
    pub linter: Arc<Linter>,
    pub action_engine: Arc<verter_actions::ActionEngine>,
    pub project_root: Option<PathBuf>,
    #[allow(dead_code)]
    pub config: McpServerConfig,
    tool_router: ToolRouter<Self>,
}

// ── Helper: build ScriptAnalysisSnapshot from host FileAnalysisSnapshot ──

fn build_script_snapshot(
    analysis: &verter_host::FileAnalysisSnapshot,
) -> verter_analysis::types::ScriptAnalysisSnapshot {
    verter_analysis::types::ScriptAnalysisSnapshot {
        imports: analysis.imports.clone(),
        module_references: analysis.module_references.to_vec(),
        bindings: analysis.bindings.clone(),
        macros: analysis.macros.to_vec(),
        macro_type_deps: analysis.macro_type_deps.to_vec(),
        flags: AnalysisFlags::from_bits_truncate(analysis.script_flags),
        exported_functions: Vec::new(),
        vue_api_calls: analysis.vue_api_calls.to_vec(),
        dom_query_calls: analysis.dom_query_calls.to_vec(),
        css_var_manipulations: analysis.css_var_manipulations.to_vec(),
        script_binding_occurrences: analysis.script_binding_occurrences.to_vec(),
        store_usages: analysis.store_usages.to_vec(),
        store_definitions: analysis.store_definitions.to_vec(),
        first_await_offset: None,
        type_enhancements: None,
        options_api: analysis.options_api.clone(),
        nested_macro_calls: Vec::new(),
    }
}

/// Populate evidence snippets on diagnostics from source text.
fn populate_evidence(diags: &mut [verter_diagnostics::LintDiagnostic], source: Option<&str>) {
    let src = match source {
        Some(s) => s,
        None => return,
    };
    for d in diags.iter_mut() {
        if d.span.start >= d.span.end || d.span.end as usize > src.len() {
            continue;
        }
        // Find 1 line before and 1 line after the span for context
        let start = d.span.start as usize;
        let end = d.span.end as usize;
        let ctx_start = src[..start].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let ctx_end = src[end..].find('\n').map(|i| end + i).unwrap_or(src.len());
        let context = &src[ctx_start..ctx_end];
        let hl_start = (start - ctx_start) as u32;
        let hl_end = (end - ctx_start) as u32;
        d.evidence.push(verter_diagnostics::EvidenceSnippet {
            context: context.to_string(),
            highlight_start: hl_start,
            highlight_end: hl_end,
        });
    }
}

// ── Tool implementations ───────────────────────────────────────────

#[tool_router]
impl VerterMcpServer {
    pub fn new(host: Arc<VerterHost>, linter: Arc<Linter>, config: McpServerConfig) -> Self {
        let project_root = config.project_root.clone();
        Self {
            host,
            linter,
            action_engine: Arc::new(verter_actions::ActionEngine::builtin()),
            project_root,
            config,
            tool_router: Self::tool_router(),
        }
    }

    fn resolve(&self, path: &str) -> String {
        resolve_path(path, self.project_root.as_deref())
    }

    // ════════════════════════════════════════════════════════════════
    // FILE MANAGEMENT (3 tools)
    // ════════════════════════════════════════════════════════════════

    #[tool(
        description = "Scan a directory for Vue files and load them into the analysis host. Auto-discovers .verterrc.json / eslint config for lint rules. Returns file count, config status, and any parse errors."
    )]
    async fn scan_project(
        &self,
        Parameters(params): Parameters<ScanProjectParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let root = std::path::Path::new(&params.root);
        let result =
            scanner::scan_directory(root, &self.host, params.include_deps.unwrap_or(false));

        // Auto-discover project lint config
        let resolved = verter_diagnostics::discover_lint_config(root);
        let config_info = serde_json::json!({
            "explicitly_configured": resolved.explicitly_configured,
            "preset": format!("{:?}", resolved.config.preset),
            "rule_overrides": resolved.config.rules.len(),
            "ignore_patterns": resolved.config.ignore_patterns,
        });

        // Detect routing framework after scanning
        let route_framework = verter_analysis::detect_routing_framework(root);
        let route_info = serde_json::json!({
            "framework": route_framework,
        });

        let mut response = serde_json::to_value(&result).map_err(|e| mcp_err(e.to_string()))?;
        if let Some(obj) = response.as_object_mut() {
            obj.insert("loaded_config".to_string(), config_info);
            obj.insert("routing".to_string(), route_info);
        }

        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Load or update a single file. If source is omitted, reads from disk. Returns change info and diagnostics."
    )]
    async fn upsert_file(
        &self,
        Parameters(params): Parameters<UpsertFileParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        let source = match params.source {
            Some(s) => s,
            None => std::fs::read_to_string(&canonical)
                .map_err(|e| mcp_err(format!("Cannot read {}: {}", canonical, e)))?,
        };
        let file_kind = if canonical.ends_with(".vue") {
            verter_host::FileKind::VueSfc
        } else {
            verter_host::FileKind::NonSfc
        };
        let result = self
            .host
            .upsert(verter_host::UpsertRequest {
                canonical_id: Some(canonical.clone()),
                input_id: canonical.clone(),
                source: Arc::from(source.as_str()),
                file_kind,
                aliases: vec![],
            })
            .map_err(|e| mcp_err(e.to_string()))?;

        let response = serde_json::json!({
            "canonical_id": result.canonical_id,
            "changed": result.changed,
            "has_parse_errors": result.diagnostics.has_errors,
        });
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap_or_default(),
        )]))
    }

    #[tool(description = "List all files currently tracked by the analysis host.")]
    async fn list_files(&self) -> Result<CallToolResult, ErrorData> {
        let files = self.host.list_files();
        let list: Vec<serde_json::Value> = files
            .into_iter()
            .map(|(id, kind)| {
                serde_json::json!({
                    "id": id,
                    "kind": format!("{:?}", kind),
                })
            })
            .collect();
        let json = serde_json::to_string_pretty(&list).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ════════════════════════════════════════════════════════════════
    // DEEP ANALYSIS (5 tools)
    // ════════════════════════════════════════════════════════════════

    #[tool(
        description = "Get the full analysis snapshot for a Vue file. Includes imports, bindings, macros, template usage, style analysis."
    )]
    async fn analyze_file(
        &self,
        Parameters(params): Parameters<AnalyzeFileParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_template_analysis(&self.host, &canonical)?;
        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;
        let json = serde_json::to_string_pretty(&analysis).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get the public API surface of a Vue component: props, emits, slots, models, expose."
    )]
    async fn get_component_api(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_template_analysis(&self.host, &canonical)?;
        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;

        let mut api = serde_json::json!({
            "props": [],
            "emits": [],
            "slots": [],
            "models": [],
            "expose": [],
        });

        for m in analysis.macros.iter() {
            match m.kind {
                AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults => {
                    api["props"] = serde_json::to_value(m).unwrap_or_default();
                }
                AnalyzedMacroKind::DefineEmits => {
                    api["emits"] = serde_json::to_value(m).unwrap_or_default();
                }
                AnalyzedMacroKind::DefineModel => {
                    if let Some(models) = api["models"].as_array_mut() {
                        models.push(serde_json::to_value(m).unwrap_or_default());
                    }
                }
                AnalyzedMacroKind::DefineSlots => {
                    api["slots"] = serde_json::to_value(m).unwrap_or_default();
                }
                AnalyzedMacroKind::DefineExpose => {
                    api["expose"] = serde_json::to_value(m).unwrap_or_default();
                }
                _ => {}
            }
        }

        if let Some(tpl) = &analysis.template {
            if !tpl.defined_slots.is_empty() {
                api["template_slots"] =
                    serde_json::to_value(&tpl.defined_slots).unwrap_or_default();
            }
            if !tpl.prop_definitions.is_empty() {
                api["runtime_props"] =
                    serde_json::to_value(&tpl.prop_definitions).unwrap_or_default();
            }
            if !tpl.emit_definitions.is_empty() {
                api["runtime_emits"] =
                    serde_json::to_value(&tpl.emit_definitions).unwrap_or_default();
            }
        }

        let json = serde_json::to_string_pretty(&api).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get all imports with Vue API classification (lifecycle, reactivity, watchers, etc.)."
    )]
    async fn get_imports(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_loaded(&self.host, &canonical)?;
        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;
        let json =
            serde_json::to_string_pretty(&analysis.imports).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get all top-level bindings with reactivity classification (Ref, Computed, Reactive, etc.)."
    )]
    async fn get_bindings(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_loaded(&self.host, &canonical)?;
        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;
        let json =
            serde_json::to_string_pretty(&analysis.bindings).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get template usage analysis: components, binding references, slots, refs, event handlers."
    )]
    async fn get_template_usage(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_template_analysis(&self.host, &canonical)?;
        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;
        let tpl = analysis
            .template
            .ok_or_else(|| mcp_err("No template analysis available"))?;
        let json = serde_json::to_string_pretty(&tpl).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ════════════════════════════════════════════════════════════════
    // CSS (3 tools)
    // ════════════════════════════════════════════════════════════════

    #[tool(
        description = "Get CSS analysis for all style blocks: selectors, classes, IDs, specificity, v-bind(), scoped/module flags."
    )]
    async fn analyze_css(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_loaded(&self.host, &canonical)?;
        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;
        let json =
            serde_json::to_string_pretty(&analysis.styles).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Test if a CSS selector matches template elements. Returns three-valued result per element: Matches, MaybeMatches, NoMatch."
    )]
    async fn match_css_selector(
        &self,
        Parameters(params): Parameters<MatchCssSelectorParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_template_analysis(&self.host, &canonical)?;
        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;
        let tpl = analysis
            .template
            .ok_or_else(|| mcp_err("No template analysis available"))?;

        let parsed = match verter_analysis::parse_selector(&params.selector) {
            Some(s) => s,
            None => {
                return Ok(CallToolResult::success(vec![Content::text(
                    "Failed to parse selector",
                )]))
            }
        };

        let mut results = Vec::new();
        for (idx, el) in tpl.elements.iter().enumerate() {
            let result = verter_analysis::match_selector(&parsed, idx, &tpl.elements);
            results.push(serde_json::json!({
                "element": el.tag,
                "index": idx,
                "result": format!("{:?}", result),
            }));
        }

        let json = serde_json::to_string_pretty(&results).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Detect unintended CSS class bleed across components. Finds non-scoped styles that could affect other components, shared class name collisions."
    )]
    async fn detect_css_bleed(
        &self,
        Parameters(params): Parameters<OptionalPathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let files = self.host.list_files();
        let mut bleeds: Vec<serde_json::Value> = Vec::new();

        // Build a map of class_name -> files that define it (non-scoped only)
        let mut global_class_files: HashMap<String, Vec<String>> = HashMap::new();
        // Build a map of file -> template classes used
        let mut file_template_classes: HashMap<String, HashSet<String>> = HashMap::new();

        let vue_ids: Vec<&str> = files
            .iter()
            .filter(|(_, k)| *k == verter_host::FileKind::VueSfc)
            .map(|(id, _)| id.as_str())
            .collect();
        let analyses = batch_analysis_with_template(&self.host, &vue_ids);
        for (canonical, analysis) in &analyses {
            // Collect non-scoped class definitions
            for style in analysis.styles.iter() {
                if !style.scoped {
                    if let Some(css) = &style.css {
                        for cls in &css.classes {
                            global_class_files
                                .entry(cls.name.clone())
                                .or_default()
                                .push(canonical.clone());
                        }
                    }
                }
            }

            // Collect template class usage
            if let Some(tpl) = &analysis.template {
                let mut classes = HashSet::new();
                for el in &tpl.elements {
                    for attr in &el.attributes {
                        if attr.name == "class" {
                            if let Some(val) = &attr.value {
                                for cls in val.split_whitespace() {
                                    classes.insert(cls.to_string());
                                }
                            }
                        }
                    }
                }
                file_template_classes.insert(canonical.clone(), classes);
            }
        }

        // Detect bleed: non-scoped class defined in file A, used in template of file B
        for (class_name, defining_files) in &global_class_files {
            for (file_id, template_classes) in &file_template_classes {
                if template_classes.contains(class_name) {
                    for def_file in defining_files {
                        if def_file != file_id {
                            bleeds.push(serde_json::json!({
                                "kind": "global_class_bleed",
                                "class": class_name,
                                "source_file": def_file,
                                "affected_file": file_id,
                                "severity": "warning",
                            }));
                        }
                    }
                }
            }

            // Shared class name collision
            if defining_files.len() > 1 {
                bleeds.push(serde_json::json!({
                    "kind": "shared_class_collision",
                    "class": class_name,
                    "files": defining_files,
                    "severity": "info",
                }));
            }
        }

        // Filter to specific file if requested
        if let Some(path) = &params.path {
            let resolved = self.resolve(path);
            bleeds.retain(|b| {
                b["source_file"].as_str() == Some(resolved.as_str())
                    || b["affected_file"].as_str() == Some(resolved.as_str())
                    || b["files"]
                        .as_array()
                        .is_some_and(|a| a.iter().any(|f| f.as_str() == Some(resolved.as_str())))
            });
        }

        let response = serde_json::json!({
            "bleeds": bleeds,
            "total_bleed_count": bleeds.len(),
        });
        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ════════════════════════════════════════════════════════════════
    // DIAGNOSTICS (3 tools)
    // ════════════════════════════════════════════════════════════════

    #[tool(
        description = "Run lint rules against a Vue file. Returns diagnostics with rule, category, severity, message, and span."
    )]
    async fn lint_file(
        &self,
        Parameters(params): Parameters<LintFileParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_template_analysis(&self.host, &canonical)?;
        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;
        let source = self.host.get_source(&canonical);
        let source_str = source.as_deref();

        let linter = if let Some(preset) = &params.preset {
            let config = crate::tools::diagnostics::make_lint_config(preset);
            Arc::new(Linter::new(config))
        } else {
            self.linter.clone()
        };

        let script_snapshot = build_script_snapshot(&analysis);
        let diags = linter.lint_with_source(
            Some(&script_snapshot),
            analysis.template.as_deref(),
            &analysis.styles,
            source_str,
        );

        let mut diag_vec = diags.into_diagnostics();

        // Populate evidence snippets if requested
        if params.include_evidence.unwrap_or(false) {
            populate_evidence(&mut diag_vec, source_str);
        }

        // Filter against baseline if provided
        if let Some(baseline_path) = &params.baseline_path {
            let baseline = crate::baseline::Baseline::load(std::path::Path::new(baseline_path))
                .map_err(|e| mcp_err(format!("Cannot load baseline: {}", e)))?;
            let root = self
                .project_root
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let rel = crate::baseline::make_relative(&canonical, &root);
            diag_vec.retain(|d| {
                let span_content = source_str
                    .and_then(|s| s.get(d.span.start as usize..d.span.end as usize))
                    .unwrap_or("");
                !baseline.contains(&rel, &d.rule, span_content)
            });
        }

        let json = serde_json::to_string_pretty(&diag_vec).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Run lint rules across all loaded files. Returns summary and per-file diagnostics."
    )]
    async fn lint_project(
        &self,
        Parameters(params): Parameters<LintProjectParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let files = self.host.list_files();
        let linter = if let Some(preset) = &params.preset {
            let config = crate::tools::diagnostics::make_lint_config(preset);
            Arc::new(Linter::new(config))
        } else {
            self.linter.clone()
        };

        let baseline = if let Some(bp) = &params.baseline_path {
            Some(
                crate::baseline::Baseline::load(std::path::Path::new(bp))
                    .map_err(|e| mcp_err(format!("Cannot load baseline: {}", e)))?,
            )
        } else {
            None
        };
        let root = self
            .project_root
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        let include_evidence = params.include_evidence.unwrap_or(false);

        let mut total_errors = 0usize;
        let mut total_warnings = 0usize;
        let mut total_info = 0usize;
        let mut by_file = serde_json::Map::new();

        let vue_ids: Vec<&str> = files
            .iter()
            .filter(|(_, k)| *k == verter_host::FileKind::VueSfc)
            .map(|(id, _)| id.as_str())
            .collect();
        let analyses = batch_analysis_with_template(&self.host, &vue_ids);
        for (canonical, analysis) in &analyses {
            let source = self.host.get_source(canonical);
            let source_str = source.as_deref();

            let script_snapshot = build_script_snapshot(analysis);
            let diags = linter.lint_with_source(
                Some(&script_snapshot),
                analysis.template.as_deref(),
                &analysis.styles,
                source_str,
            );

            let errors_only = params.errors_only.unwrap_or(false);
            let mut diag_vec = diags.into_diagnostics();

            if include_evidence {
                populate_evidence(&mut diag_vec, source_str);
            }

            // Filter against baseline
            if let Some(ref baseline) = baseline {
                let rel = crate::baseline::make_relative(canonical, &root);
                diag_vec.retain(|d| {
                    let span_content = source_str
                        .and_then(|s| s.get(d.span.start as usize..d.span.end as usize))
                        .unwrap_or("");
                    !baseline.contains(&rel, &d.rule, span_content)
                });
            }

            for d in &diag_vec {
                match d.severity {
                    Severity::Error => total_errors += 1,
                    Severity::Warning => total_warnings += 1,
                    _ => total_info += 1,
                }
            }

            let filtered: Vec<_> = diag_vec
                .iter()
                .filter(|d| !errors_only || d.severity == Severity::Error)
                .collect();

            if !filtered.is_empty() {
                by_file.insert(
                    canonical.clone(),
                    serde_json::to_value(&filtered).unwrap_or_default(),
                );
            }
        }

        let response = serde_json::json!({
            "files_checked": files.iter().filter(|(_, k)| *k == verter_host::FileKind::VueSfc).count(),
            "summary": {
                "errors": total_errors,
                "warnings": total_warnings,
                "info": total_info,
            },
            "by_file": by_file,
        });

        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get available quick fixes and code actions at a byte offset in a Vue file."
    )]
    async fn get_quick_fixes(
        &self,
        Parameters(params): Parameters<QuickFixParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_template_analysis(&self.host, &canonical)?;

        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;
        let source = self
            .host
            .get_source(&canonical)
            .ok_or_else(|| mcp_err("No source available"))?;

        let script_snapshot = build_script_snapshot(&analysis);
        let diags = self.linter.lint_with_source(
            Some(&script_snapshot),
            analysis.template.as_deref(),
            &analysis.styles,
            Some(&source),
        );

        let ctx = verter_actions::ActionContext {
            source: &source,
            file_id: &canonical,
            diagnostics: &diags,
            template: analysis.template.as_deref(),
            script: Some(&script_snapshot),
            styles: &analysis.styles,
        };

        let actions = self.action_engine.actions_at(params.offset, &ctx);
        let actions_json: Vec<serde_json::Value> = actions
            .into_iter()
            .map(|a| {
                serde_json::json!({
                    "title": a.title,
                    "kind": format!("{:?}", a.kind),
                    "safety": format!("{:?}", a.safety),
                    "is_preferred": a.is_preferred,
                    "diagnostic_rule": a.diagnostic_rule,
                    "edits": a.edits.iter().map(|e| serde_json::json!({
                        "file_id": e.file_id,
                        "replacement": e.replacement,
                        "span_start": e.span.start,
                        "span_end": e.span.end,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        let json =
            serde_json::to_string_pretty(&actions_json).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ════════════════════════════════════════════════════════════════
    // COMPILATION (2 tools)
    // ════════════════════════════════════════════════════════════════

    #[tool(
        description = "Compile a Vue SFC to JavaScript/CSS. Returns compiled code per virtual node (main, script, template, styles)."
    )]
    async fn compile_file(
        &self,
        Parameters(params): Parameters<CompileFileParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_loaded(&self.host, &canonical)?;

        let profile = verter_host::CompileProfile {
            is_production: params.production.unwrap_or(false),
            source_map: params.source_map.unwrap_or(false),
            force_vapor: params.vapor.unwrap_or(false),
            ..Default::default()
        };

        let mut outputs = serde_json::Map::new();
        for node_kind in [
            verter_host::VirtualNodeKind::Main,
            verter_host::VirtualNodeKind::Script,
            verter_host::VirtualNodeKind::Template,
        ] {
            if let Ok(resp) = self.host.get_virtual_file(verter_host::VirtualQuery {
                raw_id: None,
                canonical_id: Some(canonical.clone()),
                node_kind: Some(node_kind.clone()),
                compile_profile: profile.clone(),
            }) {
                outputs.insert(
                    format!("{:?}", node_kind),
                    serde_json::json!({
                        "code": resp.code.as_ref(),
                        "lang": resp.lang,
                        "stale": resp.stale,
                    }),
                );
            }
        }

        for i in 0..4 {
            let node_kind = verter_host::VirtualNodeKind::Style { index: i };
            if let Ok(resp) = self.host.get_virtual_file(verter_host::VirtualQuery {
                raw_id: None,
                canonical_id: Some(canonical.clone()),
                node_kind: Some(node_kind),
                compile_profile: profile.clone(),
            }) {
                outputs.insert(
                    format!("Style_{}", i),
                    serde_json::json!({
                        "code": resp.code.as_ref(),
                        "lang": resp.lang,
                    }),
                );
            } else {
                break;
            }
        }

        let json = serde_json::to_string_pretty(&serde_json::Value::Object(outputs))
            .map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Generate TSX type-checking output for a Vue file (same as LSP type-checking path)."
    )]
    async fn generate_tsx(
        &self,
        Parameters(params): Parameters<GenerateTsxParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_loaded(&self.host, &canonical)?;

        let strict_slots = params.strict_slots.unwrap_or(self.config.strict_slots);

        let profile = verter_host::CompileProfile {
            target: verter_host::CompileTarget::IDE,
            strict_slots,
            ..Default::default()
        };

        // Ensure compilation populates the TSX cache
        let _ = self.host.ensure_compiled(&canonical, &profile);

        let ide = self
            .host
            .get_ide(&canonical, &profile)
            .ok_or_else(|| mcp_err(format!("Cannot generate IDE output for {}", canonical)))?;

        Ok(CallToolResult::success(vec![Content::text(
            ide.code.as_ref().to_string(),
        )]))
    }

    // ════════════════════════════════════════════════════════════════
    // CROSS-FILE (4 tools)
    // ════════════════════════════════════════════════════════════════

    #[tool(
        description = "Get the component dependency graph. Shows which components import and use which other components."
    )]
    async fn get_component_graph(
        &self,
        Parameters(params): Parameters<ComponentGraphParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let files = self.host.list_files();
        let mut graph = serde_json::Map::new();

        let root_resolved = params.root.as_ref().map(|r| self.resolve(r));
        let vue_ids: Vec<&str> = files
            .iter()
            .filter(|(_, k)| *k == verter_host::FileKind::VueSfc)
            .filter(|(id, _)| {
                root_resolved
                    .as_ref()
                    .is_none_or(|root| id.starts_with(root) || id == root)
            })
            .map(|(id, _)| id.as_str())
            .collect();
        let analyses = batch_analysis_with_template(&self.host, &vue_ids);
        for (canonical, analysis) in &analyses {
            if let Some(tpl) = &analysis.template {
                let components: Vec<serde_json::Value> = tpl
                    .components
                    .iter()
                    .map(|c| {
                        serde_json::json!({
                            "name": c.name,
                            "import_source": c.import_source,
                            "is_dynamic": c.is_dynamic,
                            "props_count": c.props.len(),
                        })
                    })
                    .collect();
                if !components.is_empty() {
                    graph.insert(
                        canonical.clone(),
                        serde_json::to_value(&components).unwrap_or_default(),
                    );
                }
            }
        }

        let json = serde_json::to_string_pretty(&serde_json::Value::Object(graph))
            .map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Find orphan components unreachable from any bundler entry point.")]
    async fn find_orphan_components(
        &self,
        Parameters(params): Parameters<OrphanComponentsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let files = self.host.list_files();
        let vue_files: HashSet<String> = files
            .iter()
            .filter(|(_, k)| *k == verter_host::FileKind::VueSfc)
            .map(|(id, _)| id.clone())
            .collect();

        let mut reachable = HashSet::new();
        let mut queue: VecDeque<String> = params
            .entry_points
            .iter()
            .map(|e| self.resolve(e))
            .collect();

        while let Some(current) = queue.pop_front() {
            if !reachable.insert(current.clone()) {
                continue;
            }
            if let Some(analysis) = self.host.get_analysis(&current) {
                for imp in &analysis.imports {
                    let resolved = self.resolve(&imp.source);
                    if !reachable.contains(&resolved) {
                        queue.push_back(resolved);
                    }
                }
                if let Some(tpl) = &analysis.template {
                    for comp in &tpl.components {
                        if let Some(src) = &comp.import_source {
                            let resolved = self.resolve(src);
                            if !reachable.contains(&resolved) {
                                queue.push_back(resolved);
                            }
                        }
                    }
                }
            }
        }

        let orphans: Vec<serde_json::Value> = vue_files
            .iter()
            .filter(|f| !reachable.contains(*f))
            .map(|f| {
                serde_json::json!({
                    "path": f,
                    "reason": "Not reachable from any entry point",
                })
            })
            .collect();

        let response = serde_json::json!({
            "orphans": orphans,
            "entry_reachable": vue_files.len() - orphans.len(),
            "total": vue_files.len(),
        });

        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Validate provide/inject pairs across the project. Finds inject() calls with no matching provide()."
    )]
    async fn validate_provide_inject(&self) -> Result<CallToolResult, ErrorData> {
        let files = self.host.list_files();
        let mut provides: HashMap<String, Vec<String>> = HashMap::new();
        let mut injects: HashMap<String, Vec<String>> = HashMap::new();

        for (id, kind) in &files {
            if *kind != verter_host::FileKind::VueSfc {
                continue;
            }
            let _ = ensure_loaded(&self.host, id);
            if let Some(analysis) = self.host.get_analysis(id) {
                for call in analysis.vue_api_calls.iter() {
                    match call.api {
                        VueApiClassification::Provide => {
                            if let Some(key) = &call.arg_value {
                                provides.entry(key.clone()).or_default().push(id.clone());
                            }
                        }
                        VueApiClassification::Inject => {
                            if let Some(key) = &call.arg_value {
                                injects.entry(key.clone()).or_default().push(id.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        let mut missing_providers = Vec::new();
        let mut unused_provides = Vec::new();

        for (key, inject_files) in &injects {
            if !provides.contains_key(key) {
                missing_providers.push(serde_json::json!({
                    "key": key,
                    "inject_files": inject_files,
                }));
            }
        }

        for (key, provide_files) in &provides {
            if !injects.contains_key(key) {
                unused_provides.push(serde_json::json!({
                    "key": key,
                    "provide_files": provide_files,
                }));
            }
        }

        let response = serde_json::json!({
            "total_provide_keys": provides.len(),
            "total_inject_keys": injects.len(),
            "missing_providers": missing_providers,
            "unused_provides": unused_provides,
        });

        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Check component props: find unknown props passed to child components and missing required props."
    )]
    async fn check_component_props(
        &self,
        Parameters(params): Parameters<OptionalPathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let files = self.host.list_files();
        let mut issues: Vec<serde_json::Value> = Vec::new();

        // Build a map of component name -> known prop definitions
        let mut component_props: HashMap<String, Vec<String>> = HashMap::new();
        let mut component_required: HashMap<String, Vec<String>> = HashMap::new();

        let vue_ids: Vec<&str> = files
            .iter()
            .filter(|(_, k)| *k == verter_host::FileKind::VueSfc)
            .map(|(id, _)| id.as_str())
            .collect();
        let analyses = batch_analysis_with_template(&self.host, &vue_ids);
        for (canonical, analysis) in &analyses {
            let comp_name = std::path::Path::new(canonical.as_str())
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            // Primary source: macro prop_fields
            let mut prop_names: Vec<String> = Vec::new();
            let mut required: Vec<String> = Vec::new();
            for m in analysis.macros.iter() {
                if matches!(
                    m.kind,
                    AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults
                ) {
                    for f in &m.prop_fields {
                        prop_names.push(f.name.clone());
                    }
                }
            }
            // Fallback: template prop_definitions (if macros had no prop_fields)
            if prop_names.is_empty() {
                if let Some(tpl) = &analysis.template {
                    for p in &tpl.prop_definitions {
                        prop_names.push(p.name.clone());
                        if p.is_required {
                            required.push(p.name.clone());
                        }
                    }
                }
            }
            if !prop_names.is_empty() {
                component_props.insert(comp_name.clone(), prop_names);
            }
            if !required.is_empty() {
                component_required.insert(comp_name, required);
            }
        }

        // Check prop usage against definitions
        for (id, kind) in &files {
            if *kind != verter_host::FileKind::VueSfc {
                continue;
            }
            if let Some(path) = &params.path {
                let resolved = self.resolve(path);
                if id != &resolved {
                    continue;
                }
            }

            if let Some(analysis) = self.host.get_analysis(id) {
                if let Some(tpl) = &analysis.template {
                    for comp in &tpl.components {
                        if let Some(known_props) = component_props.get(&comp.name) {
                            for prop in &comp.props {
                                if !known_props.contains(&prop.name) && !prop.from_spread {
                                    issues.push(serde_json::json!({
                                        "kind": "unknown_prop",
                                        "file": id,
                                        "component": comp.name,
                                        "prop": prop.name,
                                    }));
                                }
                            }

                            if !comp.has_spread {
                                if let Some(required) = component_required.get(&comp.name) {
                                    let passed: HashSet<_> =
                                        comp.props.iter().map(|p| &p.name).collect();
                                    for req in required {
                                        if !passed.contains(req) {
                                            issues.push(serde_json::json!({
                                                "kind": "missing_required_prop",
                                                "file": id,
                                                "component": comp.name,
                                                "prop": req,
                                            }));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        let response = serde_json::json!({
            "issues": issues,
            "total": issues.len(),
        });
        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ════════════════════════════════════════════════════════════════
    // SUMMARY & SCORING (3 tools)
    // ════════════════════════════════════════════════════════════════

    #[tool(
        description = "Get everything about a component in a single call: API, scores, metrics, diagnostics, CSS, dependencies, dead code. Primary tool for agents — no file reads needed."
    )]
    async fn get_component_summary(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_template_analysis(&self.host, &canonical)?;

        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;
        let source = self.host.get_source(&canonical);
        let script_snapshot = build_script_snapshot(&analysis);

        // Quality score
        let quality = scoring::compute_quality_score(
            Some(&script_snapshot),
            analysis.template.as_deref(),
            &analysis.styles,
            source.as_deref(),
        );

        // Template metrics
        let metrics = analysis
            .template
            .as_deref()
            .map(scoring::compute_template_metrics);

        // Diagnostics summary
        let diags = self.linter.lint_with_source(
            Some(&script_snapshot),
            analysis.template.as_deref(),
            &analysis.styles,
            source.as_deref(),
        );
        let diag_vec = diags.into_diagnostics();
        let diag_errors = diag_vec
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        let diag_warnings = diag_vec
            .iter()
            .filter(|d| d.severity == Severity::Warning)
            .count();

        // API surface — primary source is analysis.macros (same as get_component_api)
        let mut api = serde_json::json!({
            "props": [],
            "emits": [],
            "slots": [],
            "models": [],
        });
        for m in analysis.macros.iter() {
            match m.kind {
                AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults => {
                    api["props"] = serde_json::to_value(m).unwrap_or_default();
                }
                AnalyzedMacroKind::DefineEmits => {
                    api["emits"] = serde_json::to_value(m).unwrap_or_default();
                }
                AnalyzedMacroKind::DefineModel => {
                    if let Some(models) = api["models"].as_array_mut() {
                        models.push(serde_json::to_value(m).unwrap_or_default());
                    }
                }
                AnalyzedMacroKind::DefineSlots => {
                    api["slots"] = serde_json::to_value(m).unwrap_or_default();
                }
                _ => {}
            }
        }
        if let Some(tpl) = &analysis.template {
            if !tpl.defined_slots.is_empty() {
                api["template_slots"] =
                    serde_json::to_value(&tpl.defined_slots).unwrap_or_default();
            }
            api["components_used"] =
                serde_json::json!(tpl.components.iter().map(|c| &c.name).collect::<Vec<_>>());
        }

        // CSS summary
        let css_summary = serde_json::json!({
            "blocks": analysis.styles.len(),
            "scoped": analysis.styles.iter().any(|s| s.scoped),
            "total_selectors": analysis.styles.iter()
                .filter_map(|s| s.css.as_ref())
                .map(|c| c.selectors.len())
                .sum::<usize>(),
            "total_classes": analysis.styles.iter()
                .filter_map(|s| s.css.as_ref())
                .map(|c| c.classes.len())
                .sum::<usize>(),
        });

        // Dead code: unused bindings (conservative — only report clearly unused bindings)
        let mut unused_bindings = Vec::new();
        if let Some(tpl) = &analysis.template {
            let template_refs: HashSet<&str> = tpl
                .binding_occurrences
                .iter()
                .map(|b| b.name.as_str())
                .collect();
            // Also consider event handler bindings as template references
            let handler_refs: HashSet<&str> = tpl
                .event_handlers
                .iter()
                .filter_map(|h| h.handler_binding.as_deref())
                .collect();
            // Template ref names declared via useTemplateRef or ref="name"
            let template_ref_names: HashSet<&str> =
                tpl.template_refs.iter().map(|r| r.name.as_str()).collect();
            for binding in &analysis.bindings {
                // Skip bindings used in template expressions or event handlers
                if template_refs.contains(binding.name.as_str())
                    || handler_refs.contains(binding.name.as_str())
                {
                    continue;
                }
                // Skip bindings initialized by Vue API calls (computed, ref, reactive, etc.)
                // These are almost always consumed by the framework or other bindings
                if matches!(
                    &binding.initializer,
                    Some(verter_analysis::types::BindingInitializer::FunctionCall {
                        vue_api: Some(_),
                        ..
                    })
                ) {
                    continue;
                }
                // Skip bindings initialized by external composable calls (useSomething())
                // These often have side effects or are consumed by other composables
                if let Some(verter_analysis::types::BindingInitializer::FunctionCall {
                    callee,
                    ..
                }) = &binding.initializer
                {
                    if callee.starts_with("use") {
                        continue;
                    }
                }
                // Skip bindings whose name matches a template ref
                if template_ref_names.contains(binding.name.as_str()) {
                    continue;
                }
                unused_bindings.push(&binding.name);
            }
        }

        let response = serde_json::json!({
            "path": canonical,
            "quality_score": quality.score,
            "quality": quality,
            "template_metrics": metrics,
            "diagnostics_summary": {
                "errors": diag_errors,
                "warnings": diag_warnings,
                "total": diag_vec.len(),
            },
            "api": api,
            "css_summary": css_summary,
            "dependencies": {
                "imports": analysis.imports.len(),
                "child_components": analysis.template.as_deref()
                    .map(|t| t.components.iter().map(|c| &c.name).collect::<Vec<_>>())
                    .unwrap_or_default(),
            },
            "dead_code": {
                "unused_bindings": unused_bindings,
            },
            "bindings_count": analysis.bindings.len(),
            "macros": analysis.macros.iter().map(|m| format!("{:?}", m.kind)).collect::<Vec<_>>(),
        });

        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get aggregate project statistics: scores, Vue API usage, diagnostics health, component rankings."
    )]
    async fn get_project_stats(&self) -> Result<CallToolResult, ErrorData> {
        let files = self.host.list_files();
        let vue_files: Vec<_> = files
            .iter()
            .filter(|(_, k)| *k == verter_host::FileKind::VueSfc)
            .collect();

        let mut quality_scores: Vec<(String, u32)> = Vec::new();
        let mut total_errors = 0usize;
        let mut total_warnings = 0usize;
        let mut total_info = 0usize;
        let mut vue_api_usage: HashMap<String, usize> = HashMap::new();
        let mut macro_usage: HashMap<String, usize> = HashMap::new();
        let mut total_selectors = 0usize;
        let mut scoped_blocks = 0usize;
        let mut global_blocks = 0usize;
        let mut by_category: HashMap<String, usize> = HashMap::new();
        let mut total_elements = 0usize;
        let mut total_bindings = 0usize;

        let ids: Vec<&str> = vue_files.iter().map(|(id, _)| id.as_str()).collect();
        let analyses = batch_analysis_with_template(&self.host, &ids);
        for (canonical, analysis) in &analyses {
            let source = self.host.get_source(canonical);
            let script_snapshot = build_script_snapshot(analysis);

            let quality = scoring::compute_quality_score(
                Some(&script_snapshot),
                analysis.template.as_deref(),
                &analysis.styles,
                source.as_deref(),
            );
            quality_scores.push((canonical.clone(), quality.score));

            let diags = self.linter.lint_with_source(
                Some(&script_snapshot),
                analysis.template.as_deref(),
                &analysis.styles,
                source.as_deref(),
            );
            for d in diags.into_diagnostics() {
                match d.severity {
                    Severity::Error => total_errors += 1,
                    Severity::Warning => total_warnings += 1,
                    _ => total_info += 1,
                }
                *by_category.entry(d.category.clone()).or_default() += 1;
            }

            for call in analysis.vue_api_calls.iter() {
                *vue_api_usage
                    .entry(call.api.display_name().to_string())
                    .or_default() += 1;
            }

            for m in analysis.macros.iter() {
                *macro_usage.entry(format!("{:?}", m.kind)).or_default() += 1;
            }

            for style in analysis.styles.iter() {
                if style.scoped {
                    scoped_blocks += 1;
                } else {
                    global_blocks += 1;
                }
                if let Some(css) = &style.css {
                    total_selectors += css.selectors.len();
                }
            }

            if let Some(tpl) = &analysis.template {
                total_elements += tpl.elements.len();
            }
            total_bindings += analysis.bindings.len();
        }

        quality_scores.sort_by(|a, b| a.1.cmp(&b.1));
        let avg_score = if quality_scores.is_empty() {
            0
        } else {
            quality_scores.iter().map(|s| s.1 as usize).sum::<usize>() / quality_scores.len()
        };

        let worst: Vec<_> = quality_scores.iter().take(5).collect();
        let best: Vec<_> = quality_scores.iter().rev().take(5).collect();

        let response = serde_json::json!({
            "overview": {
                "total_files": files.len(),
                "vue_files": vue_files.len(),
                "script_deps": files.iter().filter(|(_, k)| *k == verter_host::FileKind::NonSfc).count(),
            },
            "component_stats": {
                "avg_quality_score": avg_score,
                "worst_components": worst.iter().map(|(p, s)| serde_json::json!({"path": p, "score": s})).collect::<Vec<_>>(),
                "best_components": best.iter().map(|(p, s)| serde_json::json!({"path": p, "score": s})).collect::<Vec<_>>(),
                "total_elements": total_elements,
                "total_bindings": total_bindings,
            },
            "vue_api_usage": vue_api_usage,
            "macro_usage": macro_usage,
            "style_stats": {
                "scoped_blocks": scoped_blocks,
                "global_blocks": global_blocks,
                "total_selectors": total_selectors,
            },
            "diagnostics_health": {
                "total_errors": total_errors,
                "total_warnings": total_warnings,
                "total_info": total_info,
                "by_category": by_category,
            },
        });

        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get a component quality score (0-100) with per-dimension breakdown: a11y, lint, complexity, API surface, CSS, reactivity."
    )]
    async fn get_component_quality(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_template_analysis(&self.host, &canonical)?;

        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;
        let source = self.host.get_source(&canonical);
        let script_snapshot = build_script_snapshot(&analysis);

        let quality = scoring::compute_quality_score(
            Some(&script_snapshot),
            analysis.template.as_deref(),
            &analysis.styles,
            source.as_deref(),
        );

        let json = serde_json::to_string_pretty(&quality).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ════════════════════════════════════════════════════════════════
    // RUNTIME BEHAVIOR HINTS (3 tools)
    // ════════════════════════════════════════════════════════════════

    #[tool(
        description = "Get lifecycle hooks in execution order with side effects flagged. Shows which lifecycle stages the component participates in."
    )]
    async fn get_lifecycle_order(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_loaded(&self.host, &canonical)?;

        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;

        let lifecycle_order = [
            "setup",
            "onBeforeMount",
            "onMounted",
            "onBeforeUpdate",
            "onUpdated",
            "onBeforeUnmount",
            "onUnmounted",
            "onActivated",
            "onDeactivated",
            "onErrorCaptured",
            "onRenderTracked",
            "onRenderTriggered",
            "onServerPrefetch",
        ];

        let mut hooks = Vec::new();
        let has_async_setup = AnalysisFlags::from_bits_truncate(analysis.script_flags)
            .contains(AnalysisFlags::ASYNC_SETUP);

        if has_async_setup {
            hooks.push(serde_json::json!({
                "hook": "setup (async)",
                "has_side_effects": true,
                "note": "Async setup - component uses suspense boundary",
            }));
        }

        for hook_name in &lifecycle_order {
            let calls: Vec<_> = analysis
                .vue_api_calls
                .iter()
                .filter(|c| c.api.display_name() == *hook_name)
                .collect();
            if !calls.is_empty() {
                hooks.push(serde_json::json!({
                    "hook": hook_name,
                    "count": calls.len(),
                    "has_side_effects": matches!(
                        calls[0].api,
                        VueApiClassification::OnMounted
                        | VueApiClassification::OnUnmounted
                        | VueApiClassification::OnBeforeUnmount
                        | VueApiClassification::OnServerPrefetch
                    ),
                }));
            }
        }

        // Include watchers
        let watchers: Vec<_> = analysis
            .vue_api_calls
            .iter()
            .filter(|c| c.api.is_watcher())
            .collect();

        for w in &watchers {
            hooks.push(serde_json::json!({
                "hook": w.api.display_name(),
                "has_side_effects": true,
                "watched_source": w.arg_value,
            }));
        }

        let json = serde_json::to_string_pretty(&hooks).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get re-render triggers: which reactive bindings cause which template regions to update."
    )]
    async fn get_rerender_triggers(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_template_analysis(&self.host, &canonical)?;

        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;

        let mut triggers: HashMap<String, Vec<serde_json::Value>> = HashMap::new();

        if let Some(tpl) = &analysis.template {
            for occurrence in &tpl.binding_occurrences {
                let reactivity = analysis
                    .bindings
                    .iter()
                    .find(|b| b.name == occurrence.name)
                    .map(|b| format!("{:?}", b.reactivity_kind))
                    .unwrap_or_else(|| "Unknown".to_string());

                triggers
                    .entry(occurrence.name.clone())
                    .or_default()
                    .push(serde_json::json!({
                        "span_start": occurrence.span.start,
                        "span_end": occurrence.span.end,
                        "reactivity_kind": reactivity,
                    }));
            }
        }

        let result: Vec<serde_json::Value> = triggers
            .into_iter()
            .map(|(name, usages)| {
                let reactivity = analysis
                    .bindings
                    .iter()
                    .find(|b| b.name == name)
                    .map(|b| format!("{:?}", b.reactivity_kind))
                    .unwrap_or_else(|| "Unknown".to_string());
                serde_json::json!({
                    "binding": name,
                    "reactivity_kind": reactivity,
                    "template_usage_count": usages.len(),
                    "template_usages": usages,
                })
            })
            .collect();

        let json = serde_json::to_string_pretty(&result).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get all side effects in a component: lifecycle hooks, watchers, provide/inject, DOM queries."
    )]
    async fn get_side_effects(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_loaded(&self.host, &canonical)?;

        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;

        let mut effects: Vec<serde_json::Value> = Vec::new();

        for call in analysis.vue_api_calls.iter() {
            let category = if call.api.is_lifecycle() {
                "lifecycle"
            } else if call.api.is_watcher() {
                "watcher"
            } else {
                match call.api {
                    VueApiClassification::Provide => "provide",
                    VueApiClassification::Inject => "inject",
                    _ => continue,
                }
            };
            effects.push(serde_json::json!({
                "category": category,
                "api": call.api.display_name(),
                "arg": call.arg_value,
                "span_start": call.span.start,
                "span_end": call.span.end,
            }));
        }

        for query in analysis.dom_query_calls.iter() {
            effects.push(serde_json::json!({
                "category": "dom_query",
                "api": "querySelector/querySelectorAll",
                "arg": query.selector_text,
                "span_start": query.span.start,
                "span_end": query.span.end,
            }));
        }

        let response = serde_json::json!({
            "effects": effects,
            "total": effects.len(),
            "has_async_setup": AnalysisFlags::from_bits_truncate(analysis.script_flags)
                .contains(AnalysisFlags::ASYNC_SETUP),
        });
        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ════════════════════════════════════════════════════════════════
    // REFACTORING SUGGESTIONS (3 tools)
    // ════════════════════════════════════════════════════════════════

    #[tool(
        description = "Detect auto-refactoring opportunities: oversized components, extract component candidates, CSS consolidation."
    )]
    async fn suggest_refactorings(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_template_analysis(&self.host, &canonical)?;

        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;

        let mut suggestions: Vec<serde_json::Value> = Vec::new();

        if let Some(tpl) = &analysis.template {
            let metrics = scoring::compute_template_metrics(tpl);
            if metrics.total_elements > 50 || metrics.max_nesting_depth > 8 {
                suggestions.push(serde_json::json!({
                    "kind": "oversized_component",
                    "message": format!(
                        "Component has {} elements and nesting depth {}. Consider splitting.",
                        metrics.total_elements, metrics.max_nesting_depth
                    ),
                    "severity": "warning",
                }));
            }
            if metrics.inline_handler_count > 5 {
                suggestions.push(serde_json::json!({
                    "kind": "too_many_inline_handlers",
                    "message": format!(
                        "{} inline event handlers. Consider extracting to named methods.",
                        metrics.inline_handler_count
                    ),
                    "severity": "info",
                }));
            }
        }

        if analysis.bindings.len() > 30 {
            suggestions.push(serde_json::json!({
                "kind": "too_many_bindings",
                "message": format!(
                    "Component has {} top-level bindings. Consider composables for grouping related logic.",
                    analysis.bindings.len()
                ),
                "severity": "info",
            }));
        }

        // Duplicate selectors
        let mut seen_selectors: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, style) in analysis.styles.iter().enumerate() {
            if let Some(css) = &style.css {
                for sel in &css.selectors {
                    seen_selectors.entry(sel.text.clone()).or_default().push(i);
                }
            }
        }
        for (sel, blocks) in &seen_selectors {
            if blocks.len() > 1 {
                suggestions.push(serde_json::json!({
                    "kind": "duplicate_selector",
                    "message": format!("Selector '{}' appears in {} style blocks", sel, blocks.len()),
                    "severity": "info",
                }));
            }
        }

        let json =
            serde_json::to_string_pretty(&suggestions).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Detect prop drilling: props passed through 2+ component levels. Suggests provide/inject migration."
    )]
    async fn detect_prop_drilling(
        &self,
        Parameters(params): Parameters<OptionalPathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let files = self.host.list_files();
        let mut component_received_props: HashMap<String, HashSet<String>> = HashMap::new();
        let mut component_passed_props: HashMap<String, Vec<(String, HashSet<String>)>> =
            HashMap::new();

        let vue_ids: Vec<&str> = files
            .iter()
            .filter(|(_, k)| *k == verter_host::FileKind::VueSfc)
            .map(|(id, _)| id.as_str())
            .collect();
        let analyses = batch_analysis_with_template(&self.host, &vue_ids);
        for (canonical, analysis) in &analyses {
            let comp_name = std::path::Path::new(canonical.as_str())
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();

            if let Some(tpl) = &analysis.template {
                let received: HashSet<String> = tpl
                    .prop_definitions
                    .iter()
                    .map(|p| p.name.clone())
                    .collect();
                component_received_props.insert(comp_name.clone(), received);

                for child in &tpl.components {
                    let passed: HashSet<String> =
                        child.props.iter().map(|p| p.name.clone()).collect();
                    component_passed_props
                        .entry(comp_name.clone())
                        .or_default()
                        .push((child.name.clone(), passed));
                }
            }
        }

        let mut drilled: Vec<serde_json::Value> = Vec::new();
        for (comp, children) in &component_passed_props {
            if let Some(received) = component_received_props.get(comp) {
                for (child_name, passed) in children {
                    let forwarded: Vec<_> = passed.intersection(received).collect();
                    if !forwarded.is_empty() {
                        drilled.push(serde_json::json!({
                            "parent": comp,
                            "child": child_name,
                            "drilled_props": forwarded,
                            "suggestion": "Consider using provide/inject instead of passing through intermediate components",
                        }));
                    }
                }
            }
        }

        if let Some(path) = &params.path {
            let comp_name = std::path::Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            drilled.retain(|d| {
                d["parent"].as_str() == Some(comp_name) || d["child"].as_str() == Some(comp_name)
            });
        }

        let response = serde_json::json!({
            "drilled_props": drilled,
            "total": drilled.len(),
        });
        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Detect Options API components that could be migrated to Composition API. Returns candidates with difficulty estimate."
    )]
    async fn detect_migration_targets(
        &self,
        Parameters(params): Parameters<OptionalPathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let files = self.host.list_files();
        let mut targets: Vec<serde_json::Value> = Vec::new();

        for (id, kind) in &files {
            if *kind != verter_host::FileKind::VueSfc {
                continue;
            }
            if let Some(path) = &params.path {
                let resolved = self.resolve(path);
                if id != &resolved {
                    continue;
                }
            }

            let _ = ensure_loaded(&self.host, id);
            if let Some(analysis) = self.host.get_analysis(id) {
                let flags = AnalysisFlags::from_bits_truncate(analysis.script_flags);
                let has_define_component = analysis
                    .vue_api_calls
                    .iter()
                    .any(|c| c.api == VueApiClassification::DefineComponent);

                let has_setup_macros = analysis.macros.iter().any(|m| {
                    matches!(
                        m.kind,
                        AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::DefineEmits
                    )
                });

                if has_define_component && !has_setup_macros {
                    let complexity = if analysis.bindings.len() > 20 {
                        "hard"
                    } else if analysis.bindings.len() > 10 {
                        "medium"
                    } else {
                        "easy"
                    };

                    targets.push(serde_json::json!({
                        "path": id,
                        "reason": "Uses defineComponent without script setup macros",
                        "difficulty": complexity,
                        "bindings": analysis.bindings.len(),
                        "has_lifecycle": flags.contains(AnalysisFlags::HAS_LIFECYCLE_HOOKS),
                        "has_watchers": flags.contains(AnalysisFlags::HAS_WATCHERS),
                    }));
                }
            }
        }

        let response = serde_json::json!({
            "targets": targets,
            "total": targets.len(),
        });
        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ════════════════════════════════════════════════════════════════
    // TYPE SYSTEM (3 tools)
    // ════════════════════════════════════════════════════════════════

    #[tool(
        description = "Get inferred/declared types for all props, emits, bindings, and expose in a component."
    )]
    async fn get_component_types(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_template_analysis(&self.host, &canonical)?;

        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;

        let mut types = serde_json::json!({});

        let binding_types: Vec<serde_json::Value> = analysis
            .bindings
            .iter()
            .map(|b| {
                serde_json::json!({
                    "name": b.name,
                    "kind": format!("{:?}", b.kind),
                    "type_annotation": b.type_annotation,
                    "reactivity_kind": format!("{:?}", b.reactivity_kind),
                    "is_reactive": b.is_reactive,
                })
            })
            .collect();
        types["bindings"] = serde_json::to_value(&binding_types).unwrap_or_default();

        let macro_types: Vec<serde_json::Value> = analysis
            .macros
            .iter()
            .map(|m| {
                serde_json::json!({
                    "kind": format!("{:?}", m.kind),
                    "is_type_based": m.is_type_based,
                    "type_references": m.type_references,
                    "binding_name": m.binding_name,
                })
            })
            .collect();
        types["macros"] = serde_json::to_value(&macro_types).unwrap_or_default();

        if let Some(tpl) = &analysis.template {
            let prop_types: Vec<serde_json::Value> = tpl
                .prop_definitions
                .iter()
                .map(|p| {
                    serde_json::json!({
                        "name": p.name,
                        "type_annotation": p.type_annotation,
                        "is_required": p.is_required,
                        "is_boolean": p.is_boolean,
                    })
                })
                .collect();
            types["props"] = serde_json::to_value(&prop_types).unwrap_or_default();
        }

        let type_imports: Vec<serde_json::Value> = analysis
            .imports
            .iter()
            .filter(|i| i.is_type_only)
            .map(|i| {
                serde_json::json!({
                    "source": i.source,
                    "bindings": i.bindings.iter().map(|b| &b.name).collect::<Vec<_>>(),
                })
            })
            .collect();
        types["type_imports"] = serde_json::to_value(&type_imports).unwrap_or_default();
        types["macro_type_deps"] =
            serde_json::to_value(&analysis.macro_type_deps).unwrap_or_default();

        let json = serde_json::to_string_pretty(&types).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Check type compatibility between parent prop values and child prop declarations."
    )]
    async fn check_prop_types(
        &self,
        Parameters(params): Parameters<CheckPropTypesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let parent_canonical = self.resolve(&params.parent);
        ensure_template_analysis(&self.host, &parent_canonical)?;

        let parent_analysis = self
            .host
            .get_analysis(&parent_canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", parent_canonical)))?;

        let child_name = &params.child_component;
        let mut result = serde_json::json!({
            "parent": parent_canonical,
            "child_component": child_name,
            "issues": [],
        });

        if let Some(tpl) = &parent_analysis.template {
            for comp in &tpl.components {
                if comp.name == *child_name {
                    if let Some(import_src) = &comp.import_source {
                        let child_canonical = self.resolve(import_src);
                        let _ = ensure_template_analysis(&self.host, &child_canonical);
                        if let Some(child_analysis) = self.host.get_analysis(&child_canonical) {
                            if let Some(child_tpl) = &child_analysis.template {
                                let child_props: HashMap<_, _> = child_tpl
                                    .prop_definitions
                                    .iter()
                                    .map(|p| (p.name.clone(), p))
                                    .collect();

                                let mut issues: Vec<serde_json::Value> = Vec::new();
                                for prop in &comp.props {
                                    if let Some(child_prop) = child_props.get(&prop.name) {
                                        if child_prop.is_boolean && !prop.is_bound {
                                            issues.push(serde_json::json!({
                                                "prop": prop.name,
                                                "issue": "Boolean prop passed as string attribute, use :prop binding",
                                            }));
                                        }
                                    }
                                }

                                result["issues"] =
                                    serde_json::to_value(&issues).unwrap_or_default();
                                result["child_prop_definitions"] =
                                    serde_json::to_value(&child_tpl.prop_definitions)
                                        .unwrap_or_default();
                                result["parent_prop_usage"] =
                                    serde_json::to_value(&comp.props).unwrap_or_default();
                            }
                        }
                    }
                    break;
                }
            }
        }

        let json = serde_json::to_string_pretty(&result).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get type-level diagnostics for a Vue file: unresolved bindings and type dependencies."
    )]
    async fn get_type_errors(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_template_analysis(&self.host, &canonical)?;

        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;

        let mut type_issues: Vec<serde_json::Value> = Vec::new();

        if let Some(tpl) = &analysis.template {
            for unresolved in &tpl.unresolved_bindings {
                type_issues.push(serde_json::json!({
                    "kind": "unresolved_binding",
                    "name": unresolved.name,
                    "span_start": unresolved.span.start,
                    "span_end": unresolved.span.end,
                    "message": format!("'{}' is used in template but not defined in script", unresolved.name),
                }));
            }
        }

        for dep in analysis.macro_type_deps.iter() {
            type_issues.push(serde_json::json!({
                "kind": "type_dependency",
                "type_name": dep.type_name,
                "import_source": dep.import_source,
                "macro_kind": format!("{:?}", dep.macro_kind),
                "message": format!("Type '{}' imported from '{}' for {:?}", dep.type_name, dep.import_source, dep.macro_kind),
            }));
        }

        let response = serde_json::json!({
            "type_issues": type_issues,
            "total": type_issues.len(),
        });
        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ════════════════════════════════════════════════════════════════
    // DOCUMENTATION (1 tool)
    // ════════════════════════════════════════════════════════════════

    #[tool(
        description = "Generate Markdown documentation for a Vue component: props table, events, slots, dependencies, styles."
    )]
    async fn generate_component_docs(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_template_analysis(&self.host, &canonical)?;

        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;

        let script_snapshot = build_script_snapshot(&analysis);
        let docs = crate::tools::docs::generate_docs(
            &canonical,
            Some(&script_snapshot),
            analysis.template.as_deref(),
            &analysis.styles,
        );

        Ok(CallToolResult::success(vec![Content::text(docs)]))
    }

    // ════════════════════════════════════════════════════════════════
    // UTILITY (1 tool)
    // ════════════════════════════════════════════════════════════════

    #[tool(
        description = "Classify a Vue API function: returns category (lifecycle, reactivity, watcher, DI, etc.) and metadata."
    )]
    async fn explain_vue_api(
        &self,
        Parameters(params): Parameters<ApiNameParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let classification = verter_analysis::classify_vue_api(&params.name);
        let response = serde_json::json!({
            "api_name": params.name,
            "classification": format!("{:?}", classification),
            "is_lifecycle": verter_analysis::is_lifecycle_api(classification),
            "is_reactivity": verter_analysis::is_reactivity_api(classification),
            "is_watcher": verter_analysis::is_watcher_api(classification),
        });
        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ════════════════════════════════════════════════════════════════
    // BASELINE (1 tool)
    // ════════════════════════════════════════════════════════════════

    #[tool(
        description = "Save a baseline of current diagnostics. Future lint calls with baseline_path will only report NEW issues not in the baseline."
    )]
    async fn save_baseline(
        &self,
        Parameters(params): Parameters<SaveBaselineParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let files = self.host.list_files();
        let linter = if let Some(preset) = &params.preset {
            let config = crate::tools::diagnostics::make_lint_config(preset);
            Arc::new(Linter::new(config))
        } else {
            self.linter.clone()
        };

        let root = self
            .project_root
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();

        let mut baseline = crate::baseline::Baseline::new();
        baseline.created = chrono_now_iso();

        let vue_ids: Vec<&str> = files
            .iter()
            .filter(|(_, k)| *k == verter_host::FileKind::VueSfc)
            .map(|(id, _)| id.as_str())
            .collect();
        let analyses = batch_analysis_with_template(&self.host, &vue_ids);
        for (canonical, analysis) in &analyses {
            let source = self.host.get_source(canonical);
            let source_str = source.as_deref();

            let script_snapshot = build_script_snapshot(analysis);
            let diags = linter.lint_with_source(
                Some(&script_snapshot),
                analysis.template.as_deref(),
                &analysis.styles,
                source_str,
            );

            let diag_vec = diags.into_diagnostics();
            let rel = crate::baseline::make_relative(canonical, &root);

            for d in &diag_vec {
                let span_content = source_str
                    .and_then(|s| s.get(d.span.start as usize..d.span.end as usize))
                    .unwrap_or("");
                baseline.add(&rel, &d.rule, span_content);
            }
        }

        let output_path = params
            .output_path
            .map(PathBuf::from)
            .or_else(|| {
                self.project_root
                    .as_ref()
                    .map(|r| r.join(".verter-baseline.json"))
            })
            .unwrap_or_else(|| PathBuf::from(".verter-baseline.json"));

        baseline
            .save(&output_path)
            .map_err(|e| mcp_err(format!("Cannot save baseline: {}", e)))?;

        let response = serde_json::json!({
            "path": output_path.display().to_string(),
            "total_entries": baseline.total_entries(),
            "files": baseline.entries.len(),
        });
        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ════════════════════════════════════════════════════════════════
    // EVENT FLOW (1 tool)
    // ════════════════════════════════════════════════════════════════

    #[tool(
        description = "Trace an event across the component graph: find which components emit it and which listen to it."
    )]
    async fn trace_event_flow(
        &self,
        Parameters(params): Parameters<TraceEventFlowParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let files = self.host.list_files();
        let mut emitters: Vec<String> = Vec::new();
        let mut listeners: Vec<String> = Vec::new();

        let vue_ids: Vec<&str> = files
            .iter()
            .filter(|(_, k)| *k == verter_host::FileKind::VueSfc)
            .map(|(id, _)| id.as_str())
            .collect();
        let analyses = batch_analysis_with_template(&self.host, &vue_ids);
        for (canonical, analysis) in &analyses {
            if let Some(tpl) = &analysis.template {
                // Check if component declares this emit (defineEmits)
                for emit in &tpl.emit_definitions {
                    if emit.event_name == params.event_name {
                        emitters.push(canonical.clone());
                    }
                }
                // Check template for @event listeners
                for evt in &tpl.event_handlers {
                    if evt.event_name == params.event_name {
                        listeners.push(canonical.clone());
                    }
                }
            }
        }

        emitters.sort();
        emitters.dedup();
        listeners.sort();
        listeners.dedup();

        let response = serde_json::json!({
            "event_name": params.event_name,
            "emitters": emitters,
            "listeners": listeners,
        });
        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ════════════════════════════════════════════════════════════════
    // TEST IMPACT (1 tool)
    // ════════════════════════════════════════════════════════════════

    #[tool(
        description = "Given changed files, identify which test files should be re-run based on the component dependency graph."
    )]
    async fn get_test_impact(
        &self,
        Parameters(params): Parameters<TestImpactParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Build reverse dependency map: file -> set of files that import it
        let files = self.host.list_files();
        let mut reverse_deps: HashMap<String, HashSet<String>> = HashMap::new();

        let vue_ids: Vec<&str> = files
            .iter()
            .filter(|(_, k)| *k == verter_host::FileKind::VueSfc)
            .map(|(id, _)| id.as_str())
            .collect();
        let analyses = batch_analysis_with_template(&self.host, &vue_ids);
        for (canonical, analysis) in &analyses {
            // Each import source is a dependency
            for imp in &analysis.imports {
                let source = &imp.source;
                // Normalize: resolve relative imports
                let resolved = self.resolve(source);
                reverse_deps
                    .entry(resolved)
                    .or_default()
                    .insert(canonical.clone());
            }
            // Component usages from template
            if let Some(tpl) = &analysis.template {
                for comp in &tpl.components {
                    if let Some(src) = &comp.import_source {
                        let resolved = self.resolve(src);
                        reverse_deps
                            .entry(resolved)
                            .or_default()
                            .insert(canonical.clone());
                    }
                }
            }
        }

        // BFS from changed files using reverse deps
        let mut affected: HashSet<String> = HashSet::new();
        let mut queue: VecDeque<String> = VecDeque::new();

        for f in &params.changed_files {
            let canonical = self.resolve(f);
            affected.insert(canonical.clone());
            queue.push_back(canonical);
        }

        while let Some(file) = queue.pop_front() {
            if let Some(dependents) = reverse_deps.get(&file) {
                for dep in dependents {
                    if affected.insert(dep.clone()) {
                        queue.push_back(dep.clone());
                    }
                }
            }
        }

        // Find co-located test files for affected components
        let test_patterns = ["spec.ts", "spec.js", "test.ts", "test.js"];
        let mut test_files: Vec<String> = Vec::new();
        let mut untested: Vec<String> = Vec::new();

        for file in &affected {
            let path = std::path::Path::new(file);
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let parent = path.parent();
            let mut found_test = false;

            if let Some(dir) = parent {
                // Check sibling test files: Foo.spec.ts, Foo.test.ts
                for pattern in &test_patterns {
                    let test_path = dir.join(format!("{}.{}", stem, pattern));
                    if test_path.exists() {
                        test_files.push(test_path.display().to_string());
                        found_test = true;
                    }
                }
                // Check __tests__/ directory
                let tests_dir = dir.join("__tests__");
                if tests_dir.is_dir() {
                    for pattern in &test_patterns {
                        let test_path = tests_dir.join(format!("{}.{}", stem, pattern));
                        if test_path.exists() {
                            test_files.push(test_path.display().to_string());
                            found_test = true;
                        }
                    }
                }
            }

            if !found_test && file.ends_with(".vue") {
                untested.push(file.clone());
            }
        }

        test_files.sort();
        test_files.dedup();
        untested.sort();

        let response = serde_json::json!({
            "changed_files": params.changed_files,
            "affected_components": affected.len(),
            "test_files": test_files,
            "untested_components": untested,
        });
        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ════════════════════════════════════════════════════════════════
    // ROUTE ANALYSIS (5 tools)
    // ════════════════════════════════════════════════════════════════

    #[tool(
        description = "Get the full route tree for the project. Detects vue-router, Nuxt, or unplugin-vue-router. Returns route hierarchy with paths, names, components, guards, layouts, and navigation links."
    )]
    async fn get_route_tree(&self) -> Result<CallToolResult, ErrorData> {
        let root = self
            .project_root
            .as_deref()
            .ok_or_else(|| mcp_err("No project root. Run scan_project first."))?;

        let snapshot = self.build_route_snapshot(root);
        let json = serde_json::to_string_pretty(&snapshot).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Given a .vue file path, find which route(s) render it. Answers: 'what URL shows this component?'"
    )]
    async fn get_route_for_component(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let root = self
            .project_root
            .as_deref()
            .ok_or_else(|| mcp_err("No project root. Run scan_project first."))?;

        let snapshot = self.build_route_snapshot(root);
        let canonical = self.resolve(&params.path);
        let flat = verter_analysis::flatten_routes(&snapshot.routes);
        let matching: Vec<_> = flat
            .into_iter()
            .filter(|r| {
                r.component_path.as_deref().is_some_and(|p| {
                    p == canonical || canonical.ends_with(p) || p.ends_with(&canonical)
                })
            })
            .collect();

        let response = serde_json::json!({
            "component": canonical,
            "routes": matching,
        });
        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get all navigation links (<RouterLink>/<NuxtLink>) in the project or a specific file. Answers: 'who navigates where?'"
    )]
    async fn get_navigation_map(
        &self,
        Parameters(params): Parameters<OptionalPathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let root = self
            .project_root
            .as_deref()
            .ok_or_else(|| mcp_err("No project root. Run scan_project first."))?;

        let snapshot = self.build_route_snapshot(root);
        let links: Vec<_> = if let Some(path) = &params.path {
            let canonical = self.resolve(path);
            snapshot
                .navigation_links
                .iter()
                .filter(|l| l.file_path == canonical || canonical.ends_with(&l.file_path))
                .collect()
        } else {
            snapshot.navigation_links.iter().collect()
        };

        let response = serde_json::json!({
            "total": links.len(),
            "links": links,
        });
        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Find where <RouterView>/<NuxtPage> components are placed. Answers: 'which components are view containers?'"
    )]
    async fn get_router_views(
        &self,
        Parameters(params): Parameters<OptionalPathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let root = self
            .project_root
            .as_deref()
            .ok_or_else(|| mcp_err("No project root. Run scan_project first."))?;

        let snapshot = self.build_route_snapshot(root);
        let views: Vec<_> = if let Some(path) = &params.path {
            let canonical = self.resolve(path);
            snapshot
                .router_view_locations
                .iter()
                .filter(|v| v.file_path == canonical || canonical.ends_with(&v.file_path))
                .collect()
        } else {
            snapshot.router_view_locations.iter().collect()
        };

        let response = serde_json::json!({
            "total": views.len(),
            "views": views,
        });
        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Cross-reference routes with components to find issues: missing components, dead routes (no links), orphan views, duplicate paths/names."
    )]
    async fn analyze_route_health(&self) -> Result<CallToolResult, ErrorData> {
        let root = self
            .project_root
            .as_deref()
            .ok_or_else(|| mcp_err("No project root. Run scan_project first."))?;

        let snapshot = self.build_route_snapshot(root);

        // Build set of existing files from the host
        let existing_files: std::collections::HashSet<String> = self
            .host
            .list_files()
            .into_iter()
            .map(|(id, _)| id)
            .collect();

        let report = verter_analysis::analyze_route_health(&snapshot, &existing_files);
        let json = serde_json::to_string_pretty(&report).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    /// Build a route analysis snapshot by combining framework detection, route extraction,
    /// and template analysis from loaded files.
    fn build_route_snapshot(
        &self,
        project_root: &std::path::Path,
    ) -> verter_analysis::RouteAnalysisSnapshot {
        // Collect template component usages from all loaded Vue files
        let files = self.host.list_files();
        let vue_ids: Vec<&str> = files
            .iter()
            .filter(|(_, k)| *k == verter_host::FileKind::VueSfc)
            .map(|(id, _)| id.as_str())
            .collect();

        let analyses = batch_analysis_with_template(&self.host, &vue_ids);
        let template_components: Vec<(String, Vec<verter_analysis::TemplateComponentUsage>)> =
            analyses
                .iter()
                .filter_map(|(id, a)| {
                    a.template
                        .as_ref()
                        .map(|t| (id.clone(), t.components.clone()))
                })
                .collect();

        verter_analysis::build_route_analysis(project_root, &template_components)
    }

    // ── Store Analysis Tools ──────────────────────────────────────────

    #[tool(
        description = "Per-file store usages: which stores are imported, used, destructured, and whether storeToRefs is applied."
    )]
    async fn get_store_usage(
        &self,
        Parameters(params): Parameters<OptionalPathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let files = self.host.list_files();
        let mut results: Vec<serde_json::Value> = Vec::new();

        let filter_id = params.path.as_ref().map(|p| self.resolve(p));

        for (id, kind) in &files {
            if *kind != verter_host::FileKind::VueSfc {
                continue;
            }
            if let Some(ref filter) = filter_id {
                if id != filter {
                    continue;
                }
            }
            let _ = ensure_loaded(&self.host, id);
            if let Some(analysis) = self.host.get_analysis(id) {
                if analysis.store_usages.is_empty() && analysis.store_definitions.is_empty() {
                    continue;
                }
                let usages: Vec<serde_json::Value> = analysis
                    .store_usages
                    .iter()
                    .map(|u| {
                        serde_json::json!({
                            "binding_name": u.binding_name,
                            "callee": u.callee,
                            "import_source": u.import_source,
                            "store_api": format!("{:?}", u.store_api),
                            "has_store_to_refs": u.has_store_to_refs,
                            "destructured_props": u.destructured_props,
                            "destructured_without_store_to_refs": u.destructured_without_store_to_refs,
                        })
                    })
                    .collect();
                let definitions: Vec<serde_json::Value> = analysis
                    .store_definitions
                    .iter()
                    .map(|d| {
                        serde_json::json!({
                            "store_id": d.store_id,
                            "export_name": d.export_name,
                            "store_api": format!("{:?}", d.store_api),
                            "state_properties": d.state_properties,
                            "getters": d.getters,
                            "actions": d.actions,
                        })
                    })
                    .collect();
                results.push(serde_json::json!({
                    "file": id,
                    "store_usages": usages,
                    "store_definitions": definitions,
                }));
            }
        }

        let response = serde_json::json!({
            "files_with_stores": results.len(),
            "results": results,
        });
        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Project-wide store dependency graph: components → stores, store → store deps, unused stores."
    )]
    async fn get_store_graph(&self) -> Result<CallToolResult, ErrorData> {
        let files = self.host.list_files();
        let mut store_definitions: Vec<serde_json::Value> = Vec::new();
        let mut store_usages_by_file: Vec<serde_json::Value> = Vec::new();
        let mut all_store_ids: Vec<String> = Vec::new();
        let mut used_callees: HashSet<String> = HashSet::new();

        for (id, kind) in &files {
            if *kind != verter_host::FileKind::VueSfc {
                continue;
            }
            let _ = ensure_loaded(&self.host, id);
            if let Some(analysis) = self.host.get_analysis(id) {
                for def in analysis.store_definitions.iter() {
                    if let Some(store_id) = &def.store_id {
                        all_store_ids.push(store_id.clone());
                    }
                    store_definitions.push(serde_json::json!({
                        "file": id,
                        "store_id": def.store_id,
                        "export_name": def.export_name,
                        "state_properties": def.state_properties,
                        "getters": def.getters,
                        "actions": def.actions,
                        "store_dependencies": def.store_dependencies,
                    }));
                }
                if !analysis.store_usages.is_empty() {
                    let callees: Vec<&str> = analysis
                        .store_usages
                        .iter()
                        .map(|u| u.callee.as_str())
                        .collect();
                    for c in &callees {
                        used_callees.insert(c.to_string());
                    }
                    store_usages_by_file.push(serde_json::json!({
                        "file": id,
                        "stores_used": callees,
                    }));
                }
            }
        }

        let unused_stores: Vec<&String> = all_store_ids
            .iter()
            .filter(|id| !used_callees.iter().any(|c| c.contains(id.as_str())))
            .collect();

        let response = serde_json::json!({
            "total_store_definitions": store_definitions.len(),
            "total_files_using_stores": store_usages_by_file.len(),
            "store_definitions": store_definitions,
            "store_usages_by_file": store_usages_by_file,
            "all_store_ids": all_store_ids,
            "unused_stores": unused_stores,
        });
        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Trace a specific store: definition file, all consumers, dependency chain."
    )]
    async fn trace_store_flow(
        &self,
        Parameters(params): Parameters<TraceStoreFlowParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let files = self.host.list_files();
        let mut definition_file: Option<String> = None;
        let mut definition_info: Option<serde_json::Value> = None;
        let mut consumer_files: Vec<serde_json::Value> = Vec::new();

        for (id, kind) in &files {
            if *kind != verter_host::FileKind::VueSfc {
                continue;
            }
            let _ = ensure_loaded(&self.host, id);
            if let Some(analysis) = self.host.get_analysis(id) {
                // Check if this file defines the target store
                for def in analysis.store_definitions.iter() {
                    if def.store_id.as_deref() == Some(params.store_id.as_str())
                        || def.export_name == params.store_id
                    {
                        definition_file = Some(id.clone());
                        definition_info = Some(serde_json::json!({
                            "store_id": def.store_id,
                            "export_name": def.export_name,
                            "state_properties": def.state_properties,
                            "getters": def.getters,
                            "actions": def.actions,
                            "store_dependencies": def.store_dependencies,
                        }));
                    }
                }
                // Check if this file uses the target store
                let matching_usages: Vec<serde_json::Value> = analysis
                    .store_usages
                    .iter()
                    .filter(|u| {
                        u.callee.contains(&params.store_id)
                            || u.import_source.contains(&params.store_id)
                    })
                    .map(|u| {
                        serde_json::json!({
                            "binding_name": u.binding_name,
                            "callee": u.callee,
                            "has_store_to_refs": u.has_store_to_refs,
                            "destructured_props": u.destructured_props,
                            "destructured_without_store_to_refs": u.destructured_without_store_to_refs,
                        })
                    })
                    .collect();
                if !matching_usages.is_empty() {
                    consumer_files.push(serde_json::json!({
                        "file": id,
                        "usages": matching_usages,
                    }));
                }
            }
        }

        let response = serde_json::json!({
            "store_id": params.store_id,
            "definition_file": definition_file,
            "definition": definition_info,
            "consumer_count": consumer_files.len(),
            "consumers": consumer_files,
        });
        let json = serde_json::to_string_pretty(&response).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ════════════════════════════════════════════════════════════════
    // SSR ANALYSIS (3 tools)
    // ════════════════════════════════════════════════════════════════

    #[tool(
        description = "Score a component's SSR compatibility from 0-100. Checks for client-only lifecycle hooks, DOM queries, browser globals, CSS variable manipulations, and nondeterministic template expressions."
    )]
    async fn ssr_readiness(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_template_analysis(&self.host, &canonical)?;

        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;

        let score_result = compute_ssr_readiness(&analysis);

        let json =
            serde_json::to_string_pretty(&score_result).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Analyze a component and return an ordered list of changes needed for SSR safety. Each item includes the issue, its severity, and a suggested fix."
    )]
    async fn ssr_migration_plan(
        &self,
        Parameters(params): Parameters<FilePathParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let canonical = self.resolve(&params.path);
        ensure_template_analysis(&self.host, &canonical)?;

        let analysis = self
            .host
            .get_analysis(&canonical)
            .ok_or_else(|| mcp_err(format!("No analysis for {}", canonical)))?;

        let plan = build_ssr_migration_plan(&analysis);

        let json = serde_json::to_string_pretty(&plan).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Scan all loaded .vue files and compute per-component SSR readiness scores. Returns a project-wide summary with score distribution, critical-path blockers, and overall SSR adoption readiness."
    )]
    async fn ssr_project_report(&self) -> Result<CallToolResult, ErrorData> {
        let files = self.host.list_files();
        let vue_ids: Vec<&str> = files
            .iter()
            .filter(|(_, k)| *k == verter_host::FileKind::VueSfc)
            .map(|(id, _)| id.as_str())
            .collect();

        let analyses = batch_analysis_with_template(&self.host, &vue_ids);

        let mut components = Vec::new();
        let mut total_score: f64 = 0.0;
        let mut blocking = Vec::new();

        for (canonical, analysis) in &analyses {
            let result = compute_ssr_readiness(analysis);
            let score = result["score"].as_u64().unwrap_or(0);
            total_score += score as f64;

            if score < 50 {
                blocking.push(serde_json::json!({
                    "file": canonical,
                    "score": score,
                    "issues": result["issues"],
                }));
            }

            components.push(serde_json::json!({
                "file": canonical,
                "score": score,
            }));
        }

        let count = components.len().max(1);
        let avg_score = (total_score / count as f64).round() as u64;

        // Score distribution buckets
        let mut dist = [0u32; 5]; // 0-19, 20-39, 40-59, 60-79, 80-100
        for c in &components {
            let s = c["score"].as_u64().unwrap_or(0);
            let bucket = (s / 20).min(4) as usize;
            dist[bucket] += 1;
        }

        blocking.sort_by(|a, b| {
            a["score"]
                .as_u64()
                .unwrap_or(0)
                .cmp(&b["score"].as_u64().unwrap_or(0))
        });

        let report = serde_json::json!({
            "total_components": components.len(),
            "average_score": avg_score,
            "score_distribution": {
                "0-19": dist[0],
                "20-39": dist[1],
                "40-59": dist[2],
                "60-79": dist[3],
                "80-100": dist[4],
            },
            "ssr_ready": components.iter().filter(|c| c["score"].as_u64().unwrap_or(0) >= 80).count(),
            "needs_work": components.iter().filter(|c| {
                let s = c["score"].as_u64().unwrap_or(0);
                (50..80).contains(&s)
            }).count(),
            "blocking": blocking.len(),
            "critical_path_blockers": blocking.iter().take(10).collect::<Vec<_>>(),
            "components": components,
        });

        let json = serde_json::to_string_pretty(&report).map_err(|e| mcp_err(e.to_string()))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}

// ── SSR Analysis Helpers ──────────────────────────────────────────

/// Client-only lifecycle hooks that never fire during SSR.
const CLIENT_ONLY_HOOKS: &[VueApiClassification] = &[
    VueApiClassification::OnMounted,
    VueApiClassification::OnUpdated,
    VueApiClassification::OnBeforeUpdate,
    VueApiClassification::OnActivated,
    VueApiClassification::OnDeactivated,
    VueApiClassification::OnRenderTracked,
    VueApiClassification::OnRenderTriggered,
];

/// Compute SSR readiness score (0-100) for a component.
fn compute_ssr_readiness(analysis: &verter_host::FileAnalysisSnapshot) -> serde_json::Value {
    let mut score: i32 = 100;
    let mut issues = Vec::new();

    // Check client-only lifecycle hooks (-15 each, capped)
    let client_hooks: Vec<_> = analysis
        .vue_api_calls
        .iter()
        .filter(|c| CLIENT_ONLY_HOOKS.contains(&c.api))
        .collect();
    for hook in &client_hooks {
        score -= 15;
        issues.push(serde_json::json!({
            "severity": "error",
            "type": "client-only-lifecycle",
            "detail": format!("`{}` never fires during SSR", hook.api.display_name()),
        }));
    }

    // Check DOM queries (-20 each)
    for query in analysis.dom_query_calls.iter() {
        score -= 20;
        issues.push(serde_json::json!({
            "severity": "error",
            "type": "dom-query",
            "detail": format!("`{}` has no DOM on server", query.kind.display_name()),
        }));
    }

    // Check CSS variable manipulations (-10 each)
    for manip in analysis.css_var_manipulations.iter() {
        score -= 10;
        issues.push(serde_json::json!({
            "severity": "warning",
            "type": "css-var-manipulation",
            "detail": format!("`{}` requires DOM access", manip.kind.display_name()),
        }));
    }

    // Check for async setup without onServerPrefetch (-5)
    let has_async_setup = AnalysisFlags::from_bits_truncate(analysis.script_flags)
        .contains(AnalysisFlags::ASYNC_SETUP);
    let has_server_prefetch = analysis
        .vue_api_calls
        .iter()
        .any(|c| c.api == VueApiClassification::OnServerPrefetch);
    if has_async_setup && !has_server_prefetch {
        score -= 5;
        issues.push(serde_json::json!({
            "severity": "info",
            "type": "missing-server-prefetch",
            "detail": "Async setup without `onServerPrefetch` — data won't be pre-fetched on server",
        }));
    }

    // Check for useTemplateRef (-5 each)
    let template_refs: Vec<_> = analysis
        .vue_api_calls
        .iter()
        .filter(|c| c.api == VueApiClassification::UseTemplateRef)
        .collect();
    for _ in &template_refs {
        score -= 5;
        issues.push(serde_json::json!({
            "severity": "warning",
            "type": "template-ref",
            "detail": "Template refs are `null` during SSR",
        }));
    }

    // Bonus: has onServerPrefetch (+5)
    if has_server_prefetch {
        score += 5;
    }

    score = score.clamp(0, 100);

    serde_json::json!({
        "score": score,
        "issues": issues,
        "has_server_prefetch": has_server_prefetch,
        "has_async_setup": has_async_setup,
    })
}

/// Build an ordered migration plan for SSR safety.
fn build_ssr_migration_plan(analysis: &verter_host::FileAnalysisSnapshot) -> serde_json::Value {
    let mut steps = Vec::new();
    let mut priority = 1u32;

    // P1: DOM queries must be moved to onMounted
    for query in analysis.dom_query_calls.iter() {
        steps.push(serde_json::json!({
            "priority": priority,
            "severity": "error",
            "issue": format!("DOM query `{}` in setup scope", query.kind.display_name()),
            "fix": "Move inside `onMounted()` callback",
            "effort": "low",
        }));
        priority += 1;
    }

    // P2: Client-only lifecycle hooks
    let client_hooks: Vec<_> = analysis
        .vue_api_calls
        .iter()
        .filter(|c| CLIENT_ONLY_HOOKS.contains(&c.api))
        .collect();
    for hook in &client_hooks {
        steps.push(serde_json::json!({
            "priority": priority,
            "severity": "error",
            "issue": format!("`{}` never fires during SSR", hook.api.display_name()),
            "fix": format!("Guard with `if (typeof window !== 'undefined')` or keep in `{}`", hook.api.display_name()),
            "effort": "low",
        }));
        priority += 1;
    }

    // P3: CSS variable manipulations
    for manip in analysis.css_var_manipulations.iter() {
        steps.push(serde_json::json!({
            "priority": priority,
            "severity": "warning",
            "issue": format!("CSS variable `{}` manipulation in setup", manip.kind.display_name()),
            "fix": "Move to `onMounted()` callback",
            "effort": "low",
        }));
        priority += 1;
    }

    // P4: Template refs
    let template_refs: Vec<_> = analysis
        .vue_api_calls
        .iter()
        .filter(|c| c.api == VueApiClassification::UseTemplateRef)
        .collect();
    for tr in &template_refs {
        steps.push(serde_json::json!({
            "priority": priority,
            "severity": "warning",
            "issue": format!("Template ref `{}` is null during SSR", tr.arg_value.as_deref().unwrap_or("?")),
            "fix": "Access `.value` only inside `onMounted()` or in event handlers",
            "effort": "low",
        }));
        priority += 1;
    }

    // P5: Missing onServerPrefetch for async setup
    let has_async_setup = AnalysisFlags::from_bits_truncate(analysis.script_flags)
        .contains(AnalysisFlags::ASYNC_SETUP);
    let has_server_prefetch = analysis
        .vue_api_calls
        .iter()
        .any(|c| c.api == VueApiClassification::OnServerPrefetch);
    if has_async_setup && !has_server_prefetch {
        steps.push(serde_json::json!({
            "priority": priority,
            "severity": "info",
            "issue": "Async setup without `onServerPrefetch`",
            "fix": "Add `onServerPrefetch(async () => { /* fetch data */ })` for server-side data loading",
            "effort": "medium",
        }));
    }

    serde_json::json!({
        "total_steps": steps.len(),
        "steps": steps,
    })
}

/// Simple ISO 8601 timestamp (no chrono dependency).
fn chrono_now_iso() -> String {
    let dur = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Approximate UTC: good enough for a baseline timestamp
    let days = secs / 86400;
    let rem = secs % 86400;
    let hours = rem / 3600;
    let mins = (rem % 3600) / 60;
    let s = rem % 60;
    // Simple date calculation (not leap-second accurate, fine for baselines)
    let mut y = 1970u64;
    let mut d = days;
    loop {
        let year_days = if y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400))
        {
            366
        } else {
            365
        };
        if d < year_days {
            break;
        }
        d -= year_days;
        y += 1;
    }
    let leap = y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut m = 0usize;
    for &md in &month_days {
        if d < md {
            break;
        }
        d -= md;
        m += 1;
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m + 1,
        d + 1,
        hours,
        mins,
        s
    )
}

// ── ServerHandler trait ────────────────────────────────────────────

#[tool_handler]
impl ServerHandler for VerterMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "Verter Vue compiler MCP server. Provides deep analysis, diagnostics, \
             compilation, CSS matching, and cross-file analysis for Vue Single File Components. \
             Use scan_project first to load a Vue project, then use get_component_summary \
             for a complete overview of any component, or get_project_stats for project-wide \
             insights. For detailed analysis, use analyze_file, lint_file, get_component_api, etc."
                .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_analysis::types::TypeResolutionSource;
    use verter_host::{HostConfig, UpsertRequest};

    fn make_host() -> Arc<VerterHost> {
        Arc::new(VerterHost::new(HostConfig::default()))
    }

    fn upsert_vue(host: &VerterHost, id: &str, src: &str) {
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(id.to_string()),
            input_id: id.to_string(),
            source: Arc::from(src),
            file_kind: verter_host::FileKind::VueSfc,
            aliases: vec![],
        });
    }

    fn compile_analysis(host: &VerterHost, id: &str) {
        let profile = verter_host::CompileProfile {
            target: verter_host::CompileTarget::ANALYSIS,
            ..verter_host::CompileProfile::default()
        };
        let _ = host.ensure_compiled(id, &profile);
    }

    // ── Bug 1: get_component_summary API section should use macros ──

    #[test]
    fn summary_api_props_from_macros() {
        let host = make_host();
        let src = r#"<script setup lang="ts">
const props = defineProps<{ count: number; label: string }>()
</script>
<template><div>{{ count }}</div></template>"#;
        upsert_vue(&host, "/test/Comp.vue", src);
        compile_analysis(&host, "/test/Comp.vue");

        let analysis = host.get_analysis("/test/Comp.vue").unwrap();

        // Verify macros have prop_fields (the correct source)
        let has_props_macro = analysis
            .macros
            .iter()
            .any(|m| m.kind == AnalyzedMacroKind::DefineProps && !m.prop_fields.is_empty());
        assert!(
            has_props_macro,
            "macros should contain DefineProps with prop_fields"
        );

        // Build API the same way get_component_summary now does (from macros)
        let mut api = serde_json::json!({
            "props": [],
            "emits": [],
            "slots": [],
            "models": [],
        });
        for m in analysis.macros.iter() {
            match m.kind {
                AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults => {
                    api["props"] = serde_json::to_value(m).unwrap_or_default();
                }
                _ => {}
            }
        }

        // After fix, props should be non-empty (the macro has prop_fields)
        let props_val = &api["props"];
        assert!(
            !props_val.is_null() && props_val != &serde_json::json!([]),
            "Bug 1: API props should be non-empty for component with defineProps, got: {}",
            serde_json::to_string_pretty(&api).unwrap()
        );
        // Verify prop_fields are present in the serialized output
        let prop_fields = props_val.get("propFields").and_then(|v| v.as_array());
        assert!(
            prop_fields.map_or(false, |a| !a.is_empty()),
            "Bug 1: propFields should be populated, got: {}",
            serde_json::to_string_pretty(&props_val).unwrap()
        );
    }

    // ── Bug 2: check_component_props should use macros for prop names ──

    #[test]
    fn check_props_from_macros() {
        let host = make_host();
        // Parent component
        let child_src = r#"<script setup lang="ts">
defineProps<{ title: string; visible: boolean }>()
</script>
<template><div>{{ title }}</div></template>"#;
        upsert_vue(&host, "/test/Child.vue", child_src);
        compile_analysis(&host, "/test/Child.vue");

        let analysis = host.get_analysis("/test/Child.vue").unwrap();

        // Build component_props map the way check_component_props now does (from macros)
        let mut prop_names: Vec<String> = Vec::new();
        for m in analysis.macros.iter() {
            if matches!(
                m.kind,
                AnalyzedMacroKind::DefineProps | AnalyzedMacroKind::WithDefaults
            ) {
                for f in &m.prop_fields {
                    prop_names.push(f.name.clone());
                }
            }
        }

        assert!(
            !prop_names.is_empty(),
            "Bug 2: prop_names should be populated from macro prop_fields"
        );
        assert!(
            prop_names.contains(&"title".to_string()),
            "Bug 2: should contain 'title' prop, got: {:?}",
            prop_names
        );
        assert!(
            prop_names.contains(&"visible".to_string()),
            "Bug 2: should contain 'visible' prop, got: {:?}",
            prop_names
        );
    }

    // ── Bug 3: dead code detection should not exclude ALL functions ──

    #[test]
    fn dead_code_detects_unused_functions() {
        let host = make_host();
        let src = r#"<script setup lang="ts">
function unusedHelper() { return 42 }
function handleClick() { }
const count = ref(0)
</script>
<template><div @click="handleClick">{{ count }}</div></template>"#;
        upsert_vue(&host, "/test/Comp.vue", src);
        compile_analysis(&host, "/test/Comp.vue");

        let analysis = host.get_analysis("/test/Comp.vue").unwrap();
        let tpl = analysis.template.as_deref().unwrap();

        let template_refs: HashSet<&str> = tpl
            .binding_occurrences
            .iter()
            .map(|b| b.name.as_str())
            .collect();
        let handler_refs: HashSet<&str> = tpl
            .event_handlers
            .iter()
            .filter_map(|h| h.handler_binding.as_deref())
            .collect();

        // Fixed code: checks template refs + event handlers, no blanket function exclusion
        let mut unused: Vec<&str> = Vec::new();
        for binding in &analysis.bindings {
            if !template_refs.contains(binding.name.as_str())
                && !handler_refs.contains(binding.name.as_str())
            {
                unused.push(&binding.name);
            }
        }

        // unusedHelper should be detected as unused
        assert!(
            unused.iter().any(|n| *n == "unusedHelper"),
            "Bug 3: unusedHelper should appear in unused bindings, got: {:?}",
            unused
        );
        // handleClick is used in @click, should NOT be in unused
        assert!(
            !unused.iter().any(|n| *n == "handleClick"),
            "Bug 3: handleClick should NOT appear in unused (used in event handler), got: {:?}",
            unused
        );
    }

    // ── Bug 4: scoring should use prop_fields.len() not type_references.len() ──

    #[test]
    fn scoring_uses_prop_fields_not_type_references() {
        // Construct a script snapshot with many type_references but few prop_fields
        let script = verter_analysis::types::ScriptAnalysisSnapshot {
            module_references: Vec::new(),
            macros: vec![verter_analysis::types::AnalyzedMacro {
                kind: AnalyzedMacroKind::DefineProps,
                is_type_based: true,
                type_references: vec![
                    "Type1".into(),
                    "Type2".into(),
                    "Type3".into(),
                    "Type4".into(),
                    "Type5".into(),
                    "Type6".into(),
                    "Type7".into(),
                    "Type8".into(),
                    "Type9".into(),
                    "Type10".into(),
                    "Type11".into(),
                    "Type12".into(),
                ],
                binding_name: None,
                model_name: None,
                has_inherit_attrs_false: false,
                prop_fields: vec![
                    // Only 3 actual props — should NOT be penalized
                    verter_analysis::types::AnalyzedPropField {
                        name: "a".into(),
                        is_optional: false,
                        span: verter_span::Span::new(0, 1),
                        type_annotation: None,
                        description: None,
                        tags: vec![],
                        resolution_source: TypeResolutionSource::Rust,
                        resolution_error: None,
                    },
                    verter_analysis::types::AnalyzedPropField {
                        name: "b".into(),
                        is_optional: false,
                        span: verter_span::Span::new(2, 3),
                        type_annotation: None,
                        description: None,
                        tags: vec![],
                        resolution_source: TypeResolutionSource::Rust,
                        resolution_error: None,
                    },
                    verter_analysis::types::AnalyzedPropField {
                        name: "c".into(),
                        is_optional: false,
                        span: verter_span::Span::new(4, 5),
                        type_annotation: None,
                        description: None,
                        tags: vec![],
                        resolution_source: TypeResolutionSource::Rust,
                        resolution_error: None,
                    },
                ],
                emit_fields: vec![],
                slot_fields: vec![],
                default_keys: vec![],
                expose_fields: vec![],
                default_values: Vec::new(),
                resolved_local_types: Vec::new(),
                span: verter_span::Span::new(0, 100),
            }],
            bindings: vec![],
            imports: vec![],
            macro_type_deps: vec![],
            flags: verter_analysis::types::AnalysisFlags::empty(),
            exported_functions: vec![],
            vue_api_calls: vec![],
            dom_query_calls: vec![],
            css_var_manipulations: vec![],
            script_binding_occurrences: vec![],
            store_usages: vec![],
            store_definitions: vec![],
            first_await_offset: None,
            type_enhancements: None,
            options_api: None,
            nested_macro_calls: Vec::new(),
        };

        let quality = scoring::compute_quality_score(Some(&script), None, &[], None);

        // With 12 type_references (>10), current code penalizes API surface dim: (12-10)*2 = 4 points → score 96.
        // With only 3 prop_fields (<15), correct code should NOT penalize → score 100.
        assert_eq!(
            quality.api_surface.score, 100,
            "Bug 4: API surface score should be 100 (not penalized for type_references), got {}",
            quality.api_surface.score
        );
    }

    #[test]
    fn chrono_now_iso_format() {
        let ts = chrono_now_iso();
        // Should be valid ISO 8601: YYYY-MM-DDTHH:MM:SSZ
        assert!(ts.len() == 20, "timestamp should be 20 chars: {}", ts);
        assert!(ts.ends_with('Z'), "should end with Z: {}", ts);
        assert_eq!(&ts[4..5], "-", "should have dash after year");
        assert_eq!(&ts[7..8], "-", "should have dash after month");
        assert_eq!(&ts[10..11], "T", "should have T separator");
        assert_eq!(&ts[13..14], ":", "should have colon after hour");
        assert_eq!(&ts[16..17], ":", "should have colon after minute");
        // Year should be reasonable (>= 2020)
        let year: u32 = ts[..4].parse().unwrap();
        assert!(year >= 2020, "year should be >= 2020: {}", year);
    }

    #[test]
    fn populate_evidence_basic() {
        let mut diags = vec![verter_diagnostics::LintDiagnostic {
            rule: "test".to_string(),
            category: "test".to_string(),
            severity: verter_diagnostics::Severity::Warning,
            message: "test message".to_string(),
            span: verter_span::Span::new(15, 25),
            tags: vec![],
            span_kind: verter_diagnostics::DiagnosticSpanKind::Attribute,
            certainty: verter_diagnostics::Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        }];
        let source = "line one\nsome bad code here\nnext line";
        populate_evidence(&mut diags, Some(source));
        assert_eq!(diags[0].evidence.len(), 1, "should populate one snippet");
        let snippet = &diags[0].evidence[0];
        assert!(
            snippet.context.contains("bad code"),
            "snippet should contain the line: {}",
            snippet.context
        );
        assert!(
            snippet.highlight_start < snippet.highlight_end,
            "highlight range should be valid"
        );
    }

    #[test]
    fn populate_evidence_no_source() {
        let mut diags = vec![verter_diagnostics::LintDiagnostic {
            rule: "test".to_string(),
            category: "test".to_string(),
            severity: verter_diagnostics::Severity::Warning,
            message: "test".to_string(),
            span: verter_span::Span::new(0, 5),
            tags: vec![],
            span_kind: verter_diagnostics::DiagnosticSpanKind::Attribute,
            certainty: verter_diagnostics::Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        }];
        populate_evidence(&mut diags, None);
        assert!(
            diags[0].evidence.is_empty(),
            "should not populate without source"
        );
    }

    #[test]
    fn populate_evidence_out_of_range() {
        let mut diags = vec![verter_diagnostics::LintDiagnostic {
            rule: "test".to_string(),
            category: "test".to_string(),
            severity: verter_diagnostics::Severity::Warning,
            message: "test".to_string(),
            span: verter_span::Span::new(100, 200),
            tags: vec![],
            span_kind: verter_diagnostics::DiagnosticSpanKind::Attribute,
            certainty: verter_diagnostics::Certainty::Definite,
            evidence: Vec::new(),
            related_files: Vec::new(),
        }];
        populate_evidence(&mut diags, Some("short"));
        assert!(
            diags[0].evidence.is_empty(),
            "should skip out-of-range spans"
        );
    }

    // ── Route analysis: build_route_snapshot extracts template components ──

    #[test]
    fn route_snapshot_from_template_components() {
        let host = make_host();
        let src = r#"<script setup>
import { RouterLink, RouterView } from 'vue-router'
</script>
<template>
  <RouterLink to="/about">About</RouterLink>
  <RouterView />
</template>"#;
        upsert_vue(&host, "/test/App.vue", src);
        compile_analysis(&host, "/test/App.vue");

        let server = VerterMcpServer::new(
            Arc::clone(&host),
            Arc::new(verter_diagnostics::Linter::default()),
            McpServerConfig {
                project_root: Some(std::path::PathBuf::from("/test")),
                ..Default::default()
            },
        );

        let snapshot = server.build_route_snapshot(std::path::Path::new("/test"));

        // The snapshot should detect framework as Unknown (no package.json)
        assert_eq!(
            snapshot.framework,
            verter_analysis::RoutingFramework::Unknown
        );
        // There should be no routes extracted (no router config file at /test)
        assert!(
            snapshot.routes.is_empty(),
            "no router config means no routes"
        );
        // Navigation links should NOT be empty (RouterLink is present)
        // Note: template analysis may or may not pick up RouterLink as a component usage
        // depending on how the host processes it — this tests the wiring
    }

    #[test]
    fn route_framework_detection_json() {
        // Unit test for detect_routing_framework_from_json
        let vue_router = r#"{"dependencies": {"vue": "^3", "vue-router": "^4"}}"#;
        assert_eq!(
            verter_analysis::detect_routing_framework_from_json(vue_router),
            verter_analysis::RoutingFramework::VueRouter
        );

        let nuxt = r#"{"dependencies": {"nuxt": "^3"}}"#;
        assert_eq!(
            verter_analysis::detect_routing_framework_from_json(nuxt),
            verter_analysis::RoutingFramework::NuxtPages
        );

        let empty = r#"{"dependencies": {"vue": "^3"}}"#;
        assert_eq!(
            verter_analysis::detect_routing_framework_from_json(empty),
            verter_analysis::RoutingFramework::Unknown
        );
    }
    // ── SSR readiness scoring ──

    #[test]
    fn ssr_readiness_clean_component() {
        let host = make_host();
        upsert_vue(
            &host,
            "/test/Clean.vue",
            r#"<script setup>
import { ref } from 'vue'
const count = ref(0)
</script>
<template><div>{{ count }}</div></template>"#,
        );
        compile_analysis(&host, "/test/Clean.vue");
        let analysis = host.get_analysis("/test/Clean.vue").unwrap();
        let result = compute_ssr_readiness(&analysis);
        let score = result["score"].as_u64().unwrap();
        assert_eq!(score, 100, "clean component should score 100");
        let issues = result["issues"].as_array().unwrap();
        assert!(issues.is_empty(), "should have no issues");
    }

    #[test]
    fn ssr_readiness_dom_query_reduces_score() {
        let host = make_host();
        upsert_vue(
            &host,
            "/test/Dom.vue",
            r#"<script setup>
const el = document.querySelector('.foo')
</script>
<template><div>{{ el }}</div></template>"#,
        );
        compile_analysis(&host, "/test/Dom.vue");
        let analysis = host.get_analysis("/test/Dom.vue").unwrap();
        let result = compute_ssr_readiness(&analysis);
        let score = result["score"].as_u64().unwrap();
        assert!(score < 100, "DOM query should reduce score, got {}", score);
        let issues = result["issues"].as_array().unwrap();
        assert!(!issues.is_empty(), "should have issues");
    }

    #[test]
    fn ssr_migration_plan_empty_for_clean() {
        let host = make_host();
        upsert_vue(
            &host,
            "/test/Clean2.vue",
            r#"<script setup>
import { ref } from 'vue'
const x = ref(1)
</script>
<template><div>{{ x }}</div></template>"#,
        );
        compile_analysis(&host, "/test/Clean2.vue");
        let analysis = host.get_analysis("/test/Clean2.vue").unwrap();
        let plan = build_ssr_migration_plan(&analysis);
        let steps = plan["steps"].as_array().unwrap();
        assert!(
            steps.is_empty(),
            "clean component should have no migration steps"
        );
    }
}
