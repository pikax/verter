//! File-level analysis aggregation for cross-file static analysis.
//!
//! This module provides types for aggregating script and template usage
//! information into a single file-level summary, suitable for cross-file
//! analysis, MCP integration, and caching.
//!
//! # Module Structure
//!
//! - [`file_usage`] - Single-file usage aggregation (FileUsageInfo)
//! - [`project_index`] - Cross-file project index (ProjectIndex)
//! - [`typescript`] - TypeScript/JavaScript file parsing for composables
//!
//! # Usage Example
//!
//! ```ignore
//! use verter_core::analysis::{FileUsageInfo, ProjectIndex, parse_typescript_file};
//!
//! // Parse Vue SFC and create usage info
//! let usage = FileUsageInfo::from_script_result(&script_result);
//!
//! // Parse TypeScript composable
//! let ts_info = parse_typescript_file(source, true);
//!
//! // Convert to owned for cross-file indexing
//! let owned = FileUsageInfoOwned::from_span_based(&usage, source);
//! let ts_owned = ts_info.to_file_usage_owned(source.as_bytes());
//!
//! // Add to project index
//! let mut index = ProjectIndex::new();
//! index.add_file(vue_path, owned);
//! index.add_file(ts_path, ts_owned);
//!
//! // Query cross-file relationships
//! let providers = index.files_providing("theme");
//! let validation = index.validate_inject(&file, "theme");
//! ```

pub mod file_usage;
pub mod project_index;
pub mod typescript;

pub use file_usage::{
    FileUsageFlags, FileUsageInfo, FileUsageInfoOwned, ImportInfoOwned, MacroInfoOwned,
};

pub use project_index::{
    ComponentEdge, ComponentUsageSummary, DynamicInjectEntry, FileInjectValidation,
    InjectValidation, InjectValidationEntry, ProjectIndex, ProjectStats, ProvideInjectSummary,
};

pub use typescript::{
    parse_file_by_extension, parse_typescript_file, ExportKind, ExportedName, ImportBinding,
    ParseError, TypeScriptExport, TypeScriptFileInfo, TypeScriptImport,
};
