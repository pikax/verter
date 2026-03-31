//! Class flow analyzer — tracks CSS class forwarding through component boundaries.
//!
//! Determines whether a class applied to a parent component gets forwarded
//! to the child through attrs/fallthrough, scoped styles, or explicit binding.

use serde::{Deserialize, Serialize};

use crate::facts::boundary::ComponentInstanceEdge;
use crate::facts::component::ComponentSurface;
use crate::facts::css::{ClassFlowCertainty, ClassFlowFact};

/// Class flow report for a component-instance edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassFlowReport {
    pub facts: Vec<ClassFlowFact>,
}

/// Analyze class forwarding through a component-instance edge.
///
/// Checks whether classes applied on the parent's usage site
/// will reach the child component's root element(s).
pub fn analyze_class_flow(
    edge: &ComponentInstanceEdge,
    child_surface: &ComponentSurface,
) -> ClassFlowReport {
    let mut facts = Vec::new();

    // Static classes on the component tag are forwarded if inheritAttrs is not disabled
    if !child_surface.inherit_attrs_disabled {
        // Classes are always forwarded (Vue never consumes them)
        for class_name in &edge.passed_props {
            if class_name == "class" {
                // The class attr itself is forwarded
                let certainty = if edge.has_spread {
                    ClassFlowCertainty::Possible
                } else {
                    ClassFlowCertainty::Definite
                };

                if let (Some(parent), Some(child)) =
                    (&Some(&edge.parent_file_id), &edge.child_file_id)
                {
                    facts.push(ClassFlowFact {
                        class_name: "(class attr)".to_string(),
                        source_file_id: parent.to_string(),
                        target_file_id: child.clone(),
                        certainty,
                        span: edge.usage_span,
                    });
                }
            }
        }
    } else {
        // inheritAttrs: false — no class forwarding through attrs
        // Classes only flow if explicitly bound via $attrs.class
    }

    ClassFlowReport { facts }
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_span::Span;

    fn make_edge_with_class(child_id: Option<&str>) -> ComponentInstanceEdge {
        ComponentInstanceEdge {
            parent_file_id: "/src/App.vue".into(),
            child_file_id: child_id.map(String::from),
            tag_name: "Button".into(),
            usage_span: Span::new(100, 150),
            passed_props: vec!["class".into(), "color".into()],
            passed_events: vec![],
            passed_slots: vec![],
            has_spread: false,
            has_event_spread: false,
        }
    }

    #[test]
    fn class_forwarded_when_inherit_attrs_enabled() {
        let edge = make_edge_with_class(Some("/src/Button.vue"));
        let surface = ComponentSurface::default(); // inheritAttrs not disabled
        let report = analyze_class_flow(&edge, &surface);

        assert_eq!(report.facts.len(), 1);
        assert_eq!(report.facts[0].certainty, ClassFlowCertainty::Definite);
        assert_eq!(report.facts[0].target_file_id, "/src/Button.vue");
    }

    #[test]
    fn class_not_forwarded_when_inherit_attrs_false() {
        let edge = make_edge_with_class(Some("/src/Button.vue"));
        let mut surface = ComponentSurface::default();
        surface.inherit_attrs_disabled = true;
        let report = analyze_class_flow(&edge, &surface);

        // Negative: no forwarding
        assert!(report.facts.is_empty());
    }

    #[test]
    fn no_class_in_props_means_no_flow() {
        let mut edge = make_edge_with_class(Some("/src/Button.vue"));
        edge.passed_props = vec!["color".into()]; // no "class"
        let surface = ComponentSurface::default();
        let report = analyze_class_flow(&edge, &surface);

        assert!(report.facts.is_empty());
    }

    #[test]
    fn spread_makes_class_flow_possible() {
        let mut edge = make_edge_with_class(Some("/src/Button.vue"));
        edge.has_spread = true;
        let surface = ComponentSurface::default();
        let report = analyze_class_flow(&edge, &surface);

        assert_eq!(report.facts.len(), 1);
        assert_eq!(report.facts[0].certainty, ClassFlowCertainty::Possible);
    }

    #[test]
    fn unresolved_child_no_flow() {
        let edge = make_edge_with_class(None);
        let surface = ComponentSurface::default();
        let report = analyze_class_flow(&edge, &surface);

        // Negative: can't determine target
        assert!(report.facts.is_empty());
    }

    #[test]
    fn empty_edge_no_flow() {
        let edge = ComponentInstanceEdge {
            parent_file_id: "/src/App.vue".into(),
            child_file_id: Some("/src/Child.vue".into()),
            tag_name: "Child".into(),
            usage_span: Span::new(0, 50),
            passed_props: vec![],
            passed_events: vec![],
            passed_slots: vec![],
            has_spread: false,
            has_event_spread: false,
        };
        let surface = ComponentSurface::default();
        let report = analyze_class_flow(&edge, &surface);
        assert!(report.facts.is_empty());
    }

    #[test]
    fn class_flow_fact_carries_source_and_target() {
        let edge = make_edge_with_class(Some("/src/Button.vue"));
        let surface = ComponentSurface::default();
        let report = analyze_class_flow(&edge, &surface);

        assert_eq!(report.facts.len(), 1);
        assert_eq!(report.facts[0].source_file_id, "/src/App.vue");
        assert_eq!(report.facts[0].target_file_id, "/src/Button.vue");
    }

    // ── Plan-required class flow coverage ──────────────────────────────────

    #[test]
    fn inherit_attrs_false_blocks_all_class_flow() {
        // Plan: "inheritAttrs: false" → no class forwarding
        let edge = make_edge_with_class(Some("/src/Button.vue"));
        let mut surface = ComponentSurface::default();
        surface.inherit_attrs_disabled = true;
        let report = analyze_class_flow(&edge, &surface);

        // Negative: no flow when inheritAttrs disabled
        assert!(report.facts.is_empty());
        // Confirm the edge DID pass class
        assert!(edge.passed_props.contains(&"class".to_string()));
    }

    #[test]
    fn non_class_props_dont_create_flow() {
        // Only "class" prop creates flow, not other props
        let mut edge = make_edge_with_class(Some("/src/Button.vue"));
        edge.passed_props = vec!["color".into(), "size".into(), "disabled".into()];
        let surface = ComponentSurface::default();
        let report = analyze_class_flow(&edge, &surface);

        assert!(report.facts.is_empty());
    }

    #[test]
    fn spread_with_inherit_attrs_false_no_flow() {
        let mut edge = make_edge_with_class(Some("/src/Button.vue"));
        edge.has_spread = true;
        let mut surface = ComponentSurface::default();
        surface.inherit_attrs_disabled = true;
        let report = analyze_class_flow(&edge, &surface);

        // Negative: even with spread, inheritAttrs:false blocks flow
        assert!(report.facts.is_empty());
    }

    #[test]
    fn multiple_class_usages_each_produce_flow() {
        // Two edges with class should produce two flow facts
        let edge1 = make_edge_with_class(Some("/src/A.vue"));
        let edge2 = make_edge_with_class(Some("/src/B.vue"));
        let surface = ComponentSurface::default();

        let r1 = analyze_class_flow(&edge1, &surface);
        let r2 = analyze_class_flow(&edge2, &surface);

        assert_eq!(r1.facts.len(), 1);
        assert_eq!(r2.facts.len(), 1);
        assert_eq!(r1.facts[0].target_file_id, "/src/A.vue");
        assert_eq!(r2.facts[0].target_file_id, "/src/B.vue");
    }
}
