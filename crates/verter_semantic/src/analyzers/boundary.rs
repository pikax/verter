//! Boundary analyzer — component usage validation.
//!
//! Compares props/events/slots passed at a call site against the child
//! component's declared surface to detect:
//! - Unknown props (passed but not declared)
//! - Missing required props (declared as required but not passed)
//! - Unknown events (listened but not declared)

use serde::{Deserialize, Serialize};
use verter_span::Span;

use crate::facts::boundary::ComponentInstanceEdge;
use crate::facts::component::ComponentSurface;

/// A boundary issue found at a component usage site.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoundaryIssue {
    pub kind: BoundaryIssueKind,
    /// The component tag name.
    pub component_name: String,
    /// The prop/event/slot name involved.
    pub member_name: String,
    /// Span of the usage site in the parent template.
    pub usage_span: Span,
}

/// What kind of boundary issue was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BoundaryIssueKind {
    /// A prop was passed that the child doesn't declare.
    UnknownProp,
    /// A required prop was not passed.
    MissingRequiredProp,
    /// An event was listened that the child doesn't emit.
    UnknownEvent,
}

/// Analyze a single component usage edge against the child's declared surface.
///
/// Returns all boundary issues found. An empty Vec means the usage is valid.
pub fn analyze_boundary(
    edge: &ComponentInstanceEdge,
    child_surface: &ComponentSurface,
) -> Vec<BoundaryIssue> {
    let mut issues = Vec::new();

    // Skip analysis if child uses inheritAttrs:false with spread
    // (attrs forwarding is disabled, so unknown props are intentionally rejected)
    let has_spread = edge.has_spread;

    // Check for unknown props (passed but not declared)
    if !has_spread {
        let declared_prop_names: rustc_hash::FxHashSet<&str> = child_surface
            .declared
            .props
            .iter()
            .map(|p| p.name.as_str())
            .collect();

        for passed in &edge.passed_props {
            // Skip well-known HTML attrs that Vue always forwards
            if is_always_forwarded_attr(passed) {
                continue;
            }
            if !declared_prop_names.contains(passed.as_str()) {
                issues.push(BoundaryIssue {
                    kind: BoundaryIssueKind::UnknownProp,
                    component_name: edge.tag_name.clone(),
                    member_name: passed.clone(),
                    usage_span: edge.usage_span,
                });
            }
        }
    }

    // Check for missing required props
    for prop in &child_surface.declared.props {
        if !prop.is_optional && prop.default_value.is_none() {
            let is_passed = edge.passed_props.iter().any(|p| p == &prop.name);
            if !is_passed && !has_spread {
                issues.push(BoundaryIssue {
                    kind: BoundaryIssueKind::MissingRequiredProp,
                    component_name: edge.tag_name.clone(),
                    member_name: prop.name.clone(),
                    usage_span: edge.usage_span,
                });
            }
        }
    }

    // Check for unknown events
    if !edge.has_event_spread {
        let declared_event_names: rustc_hash::FxHashSet<&str> = child_surface
            .declared
            .events
            .iter()
            .map(|e| e.name.as_str())
            .collect();

        for listened in &edge.passed_events {
            // Normalize update:xxx from v-model
            let event_name = listened
                .strip_prefix("update:")
                .map(|model_name| {
                    // v-model:foo → update:foo, which matches the model's prop event
                    format!("update:{model_name}")
                })
                .unwrap_or_else(|| listened.clone());

            // Check models for update:xxx events
            let is_model_event = child_surface
                .declared
                .models
                .iter()
                .any(|m| format!("update:{}", m.name) == event_name);

            if !declared_event_names.contains(event_name.as_str()) && !is_model_event {
                issues.push(BoundaryIssue {
                    kind: BoundaryIssueKind::UnknownEvent,
                    component_name: edge.tag_name.clone(),
                    member_name: listened.clone(),
                    usage_span: edge.usage_span,
                });
            }
        }
    }

    issues
}

/// Attrs that Vue always forwards regardless of component declaration.
fn is_always_forwarded_attr(name: &str) -> bool {
    matches!(
        name,
        "class" | "style" | "key" | "ref" | "is" | "slot" | "slot-scope"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::component::{
        ComponentSurface, DeclaredSurface, EventFact, ModelFact, PropFact,
    };

    fn make_edge(tag: &str, props: Vec<&str>, events: Vec<&str>) -> ComponentInstanceEdge {
        ComponentInstanceEdge {
            parent_file_id: "/src/App.vue".into(),
            child_file_id: Some("/src/Child.vue".into()),
            tag_name: tag.into(),
            usage_span: Span::new(100, 150),
            passed_props: props.into_iter().map(String::from).collect(),
            passed_events: events.into_iter().map(String::from).collect(),
            passed_slots: vec![],
            has_spread: false,
            has_event_spread: false,
        }
    }

    fn make_surface(props: Vec<(&str, bool)>, events: Vec<&str>) -> ComponentSurface {
        let mut surface = ComponentSurface::default();
        for (name, optional) in props {
            surface.declared.props.push(PropFact {
                name: name.into(),
                is_optional: optional,
                type_text: None,
                default_value: None,
                description: None,
                span: Span::new(0, 10),
            });
        }
        for name in events {
            surface.declared.events.push(EventFact {
                name: name.into(),
                payload_type: None,
                description: None,
                span: Span::new(0, 10),
            });
        }
        surface
    }

    #[test]
    fn no_issues_when_all_props_declared() {
        let edge = make_edge("Button", vec!["color", "size"], vec![]);
        let surface = make_surface(vec![("color", true), ("size", true)], vec![]);
        let issues = analyze_boundary(&edge, &surface);

        assert!(issues.is_empty(), "should find no issues: {issues:?}");
    }

    #[test]
    fn detects_unknown_prop() {
        let edge = make_edge("Button", vec!["color", "unknown"], vec![]);
        let surface = make_surface(vec![("color", true)], vec![]);
        let issues = analyze_boundary(&edge, &surface);

        // Positive: one unknown prop
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, BoundaryIssueKind::UnknownProp);
        assert_eq!(issues[0].member_name, "unknown");
        assert_eq!(issues[0].component_name, "Button");
    }

    #[test]
    fn detects_missing_required_prop() {
        let edge = make_edge("Button", vec!["color"], vec![]);
        let surface = make_surface(
            vec![("color", true), ("label", false)], // label is required
            vec![],
        );
        let issues = analyze_boundary(&edge, &surface);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, BoundaryIssueKind::MissingRequiredProp);
        assert_eq!(issues[0].member_name, "label");
    }

    #[test]
    fn optional_prop_not_required() {
        let edge = make_edge("Button", vec![], vec![]);
        let surface = make_surface(vec![("color", true)], vec![]); // optional
        let issues = analyze_boundary(&edge, &surface);

        // Negative: optional prop not passed → no issue
        assert!(issues.is_empty());
    }

    #[test]
    fn prop_with_default_not_required() {
        let edge = make_edge("Button", vec![], vec![]);
        let mut surface = make_surface(vec![("color", false)], vec![]); // required
        surface.declared.props[0].default_value = Some("'blue'".into()); // but has default
        let issues = analyze_boundary(&edge, &surface);

        // Negative: has default → not missing
        assert!(issues.is_empty());
    }

    #[test]
    fn spread_suppresses_unknown_and_missing() {
        let mut edge = make_edge("Button", vec!["unknown"], vec![]);
        edge.has_spread = true;
        let surface = make_surface(vec![("label", false)], vec![]); // required
        let issues = analyze_boundary(&edge, &surface);

        // Negative: spread present → skip both checks
        assert!(issues.is_empty());
    }

    #[test]
    fn class_and_style_always_forwarded() {
        let edge = make_edge("Button", vec!["class", "style", "key"], vec![]);
        let surface = make_surface(vec![], vec![]); // no declared props
        let issues = analyze_boundary(&edge, &surface);

        // Negative: class/style/key are always-forwarded, not reported
        assert!(issues.is_empty());
    }

    #[test]
    fn detects_unknown_event() {
        let edge = make_edge("Button", vec![], vec!["click", "hover"]);
        let surface = make_surface(vec![], vec!["click"]);
        let issues = analyze_boundary(&edge, &surface);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, BoundaryIssueKind::UnknownEvent);
        assert_eq!(issues[0].member_name, "hover");
    }

    #[test]
    fn v_model_event_matches_model_declaration() {
        let edge = make_edge("Input", vec![], vec!["update:modelValue"]);
        let mut surface = make_surface(vec![], vec![]);
        surface.declared.models.push(ModelFact {
            name: "modelValue".into(),
            type_text: None,
            span: Span::new(0, 10),
        });
        let issues = analyze_boundary(&edge, &surface);

        // Negative: v-model event matches declared model → not unknown
        assert!(issues.is_empty());
    }

    #[test]
    fn mixed_issues() {
        let edge = make_edge("Card", vec!["title", "unknown1", "unknown2"], vec!["bad"]);
        let surface = make_surface(
            vec![("title", true), ("required_prop", false)],
            vec!["click"],
        );
        let issues = analyze_boundary(&edge, &surface);

        // Positive: 2 unknown props + 1 missing required + 1 unknown event = 4
        assert_eq!(issues.len(), 4);
        let unknown_props: Vec<_> = issues
            .iter()
            .filter(|i| i.kind == BoundaryIssueKind::UnknownProp)
            .collect();
        let missing: Vec<_> = issues
            .iter()
            .filter(|i| i.kind == BoundaryIssueKind::MissingRequiredProp)
            .collect();
        let unknown_events: Vec<_> = issues
            .iter()
            .filter(|i| i.kind == BoundaryIssueKind::UnknownEvent)
            .collect();
        assert_eq!(unknown_props.len(), 2);
        assert_eq!(missing.len(), 1);
        assert_eq!(unknown_events.len(), 1);
    }
}
