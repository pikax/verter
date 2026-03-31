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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_instance_edge_round_trips() {
        let edge = ComponentInstanceEdge {
            parent_file_id: "/src/App.vue".into(),
            child_file_id: Some("/src/Button.vue".into()),
            tag_name: "Button".into(),
            usage_span: Span::new(100, 150),
            passed_props: vec!["color".into(), "size".into()],
            passed_events: vec!["click".into()],
            passed_slots: vec!["default".into()],
            has_spread: false,
            has_event_spread: false,
        };
        let json = serde_json::to_string(&edge).unwrap();
        let back: ComponentInstanceEdge = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tag_name, "Button");
        assert_eq!(back.passed_props.len(), 2);
        assert!(!back.has_spread);
    }

    #[test]
    fn provide_inject_linkage() {
        let provide = ProvideSite {
            file_id: "/src/App.vue".into(),
            key: "theme".into(),
            span: Span::new(50, 70),
        };
        let inject = InjectSite {
            file_id: "/src/Child.vue".into(),
            key: "theme".into(),
            has_default: false,
            span: Span::new(30, 50),
        };
        // Positive: keys match
        assert_eq!(provide.key, inject.key);
        // Negative: no default on inject
        assert!(!inject.has_default);
    }

    #[test]
    fn edge_with_spread_flags() {
        let edge = ComponentInstanceEdge {
            parent_file_id: "p.vue".into(),
            child_file_id: None,
            tag_name: "Comp".into(),
            usage_span: Span::new(0, 10),
            passed_props: vec![],
            passed_events: vec![],
            passed_slots: vec![],
            has_spread: true,
            has_event_spread: true,
        };
        assert!(edge.has_spread);
        assert!(edge.has_event_spread);
        assert!(edge.child_file_id.is_none());
    }

    #[test]
    fn inject_with_default() {
        let inject = InjectSite {
            file_id: "c.vue".into(),
            key: "config".into(),
            has_default: true,
            span: Span::new(10, 20),
        };
        assert!(inject.has_default);
    }

    #[test]
    fn edge_serializes() {
        let edge = ComponentInstanceEdge {
            parent_file_id: "p.vue".into(),
            child_file_id: Some("c.vue".into()),
            tag_name: "Button".into(),
            usage_span: Span::new(50, 100),
            passed_props: vec!["color".into()],
            passed_events: vec!["click".into()],
            passed_slots: vec!["default".into()],
            has_spread: false,
            has_event_spread: false,
        };
        let json = serde_json::to_string(&edge).unwrap();
        let back: ComponentInstanceEdge = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tag_name, "Button");
        assert_eq!(back.passed_props, vec!["color"]);
        assert_eq!(back.passed_events, vec!["click"]);
    }
}
