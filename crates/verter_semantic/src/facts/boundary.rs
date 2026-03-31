//! Boundary analysis facts — component-instance edges, provide/inject linkage.
//!
//! These facts are the substrate for boundary reports: unknown props/emits,
//! missing required props/slots, accepted-surface/fallthrough reporting,
//! and ancestry-aware provide/inject tracing.

use serde::{Deserialize, Serialize};
use verter_span::Span;

/// An edge from a parent component to a child component usage in a template.
///
/// Carries the props/events/slots/classes passed at the call site,
/// along with spread flags and attrs/fallthrough information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentInstanceEdge {
    /// Canonical file ID of the parent component.
    pub parent_file_id: String,
    /// Canonical file ID of the child component (None if unresolved).
    pub child_file_id: Option<String>,
    /// Component tag name as used in the parent template.
    pub tag_name: String,
    /// Span of the component usage in the parent template.
    pub usage_span: Span,
    /// Props passed at this call site.
    pub passed_props: Vec<String>,
    /// Events listened at this call site.
    pub passed_events: Vec<String>,
    /// Slots provided at this call site.
    pub passed_slots: Vec<String>,
    /// Whether `v-bind="..."` spread was used.
    pub has_spread: bool,
    /// Whether `v-on="..."` spread was used.
    pub has_event_spread: bool,
}

/// A provide() call site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvideSite {
    pub file_id: String,
    pub key: String,
    pub span: Span,
}

/// An inject() call site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectSite {
    pub file_id: String,
    pub key: String,
    pub has_default: bool,
    pub span: Span,
}
