//! MCP server configuration and CLI argument parsing.

use clap::Parser;
use std::path::PathBuf;
use verter_session::workspace_index::QueryBudgets;

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

    /// Host process id that owns this server's lifetime; the server exits
    /// when that process dies (the same containment contract as
    /// `verter-lsp --client-pid`). Omit for standalone agent launches,
    /// whose lifetime is owned by their transport.
    #[arg(long)]
    pub client_pid: Option<u32>,

    /// Enable strict slot type checking in IDE/TSX output
    #[arg(long)]
    pub strict_slots: bool,

    /// Workspace-index postings one project-wide tool query may visit before
    /// its answer reports a partial population
    #[arg(
        long,
        default_value_t = DEFAULT_INDEX_POSTING_BUDGET,
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub index_posting_budget: u32,

    /// Workspace-index records one project-wide tool query may return before
    /// its answer reports a partial population
    #[arg(
        long,
        default_value_t = DEFAULT_INDEX_RECORD_BUDGET,
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub index_record_budget: u32,
}

/// Default posting visits of one index query behind a project-wide tool.
pub const DEFAULT_INDEX_POSTING_BUDGET: u32 = 100_000;

/// Default records returned by one index query behind a project-wide tool.
pub const DEFAULT_INDEX_RECORD_BUDGET: u32 = 100_000;

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
    pub strict_slots: bool,
    /// Budgets of each workspace-index query a project-wide tool runs.
    pub index_budgets: QueryBudgets,
}

impl Default for McpServerConfig {
    fn default() -> Self {
        Self {
            project_root: None,
            lint_preset: "recommended".to_string(),
            strict_slots: false,
            index_budgets: QueryBudgets {
                postings: DEFAULT_INDEX_POSTING_BUDGET,
                records: DEFAULT_INDEX_RECORD_BUDGET,
            },
        }
    }
}

impl From<&Cli> for McpServerConfig {
    fn from(cli: &Cli) -> Self {
        Self {
            project_root: cli.project_root.clone(),
            lint_preset: cli.lint_preset.clone(),
            strict_slots: cli.strict_slots,
            index_budgets: QueryBudgets {
                postings: cli.index_posting_budget,
                records: cli.index_record_budget,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn default_config_has_strict_slots_false() {
        let config = McpServerConfig::default();
        assert!(!config.strict_slots);
    }

    #[test]
    fn cli_strict_slots_flag_propagates_to_config() {
        let cli = Cli::parse_from(["verter-mcp", "--strict-slots"]);
        assert!(cli.strict_slots);
        let config = McpServerConfig::from(&cli);
        assert!(config.strict_slots);
    }

    #[test]
    fn a_zero_index_budget_is_refused_at_parse_time() {
        for flag in ["--index-posting-budget", "--index-record-budget"] {
            assert!(
                Cli::try_parse_from(["verter-mcp", flag, "0"]).is_err(),
                "{flag}"
            );
            let cli = Cli::try_parse_from(["verter-mcp", flag, "1"]).expect(flag);
            let config = McpServerConfig::from(&cli);
            assert!(config.index_budgets.postings >= 1 && config.index_budgets.records >= 1);
        }
    }

    #[test]
    fn cli_without_strict_slots_defaults_to_false() {
        let cli = Cli::parse_from(["verter-mcp"]);
        assert!(!cli.strict_slots);
        let config = McpServerConfig::from(&cli);
        assert!(!config.strict_slots);
    }
}
