//! MCP server configuration and CLI argument parsing.

use clap::Parser;
use std::path::PathBuf;

/// Verter MCP server — exposes Vue analysis, diagnostics, and compilation to AI agents.
#[derive(Parser, Debug)]
#[command(name = "verter-mcp", about = "Verter Vue compiler MCP server")]
pub struct Cli {
    /// Transport mode: "stdio" or "http"
    #[arg(long, default_value = "stdio")]
    pub transport: Transport,

    /// HTTP port (only for http transport)
    #[arg(long, default_value_t = 6772)]
    pub port: u16,

    /// Project root for initial file scan
    #[arg(long)]
    pub project_root: Option<PathBuf>,

    /// Lint preset: essential, recommended, all, performance, a11y, strict
    #[arg(long, default_value = "recommended")]
    pub lint_preset: String,

    /// Skip initial project scan even if project_root is set
    #[arg(long)]
    pub no_scan: bool,
}

#[derive(Debug, Clone, clap::ValueEnum)]
pub enum Transport {
    Stdio,
    Http,
}

/// Runtime configuration derived from CLI args.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct McpServerConfig {
    pub project_root: Option<PathBuf>,
    pub lint_preset: String,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            project_root: None,
            lint_preset: "recommended".to_string(),
        }
    }
}

impl From<&Cli> for McpServerConfig {
    fn from(cli: &Cli) -> Self {
        Self {
            project_root: cli.project_root.clone(),
            lint_preset: cli.lint_preset.clone(),
        }
    }
}
