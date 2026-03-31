//! Reactive flow analyzer — reactivity loss and dependency tracking.
//!
//! Detects reactivity-loss sites (destructuring without toRefs, plain
//! assignment from reactive source) and computed/watch dependency issues.

use serde::{Deserialize, Serialize};
use verter_span::Span;

use crate::facts::binding::BindingDeclaration;
use crate::facts::reactivity::{ProvenanceStepKind, ReactivityFact, ReactivityStatus};

/// A reactive flow issue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactiveFlowIssue {
    pub kind: ReactiveFlowIssueKind,
    pub binding_name: String,
    pub span: Span,
    pub explanation: String,
}

/// What kind of reactive flow issue was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReactiveFlowIssueKind {
    /// Reactivity was lost (destructuring, plain assignment from reactive source).
    ReactivityLoss,
    /// A binding that looks reactive is never used in a reactive context.
    UnusedReactive,
    /// A binding used in template/computed/watch may not be reactive.
    PossiblyNonReactive,
}

/// Analyze bindings for reactive flow issues.
///
/// Examines each binding's reactivity fact and usage pattern to detect:
/// - Reactivity loss (provenance trace contains a Loss step)
/// - Unused reactive bindings (reactive but not used in template/style)
/// - Possibly non-reactive bindings used in reactive contexts
pub fn analyze_reactive_flow(
    bindings: &[(BindingDeclaration, ReactivityFact)],
) -> Vec<ReactiveFlowIssue> {
    let mut issues = Vec::new();

    for (decl, fact) in bindings {
        // Check for reactivity loss in provenance trace
        if fact
            .trace
            .iter()
            .any(|s| s.kind == ProvenanceStepKind::Loss)
        {
            if let Some(loss_step) = fact
                .trace
                .iter()
                .find(|s| s.kind == ProvenanceStepKind::Loss)
            {
                issues.push(ReactiveFlowIssue {
                    kind: ReactiveFlowIssueKind::ReactivityLoss,
                    binding_name: decl.name.clone(),
                    span: loss_step.span,
                    explanation: loss_step.description.clone(),
                });
            }
        }

        // Check for unused reactive bindings
        if fact.status == ReactivityStatus::Reactive {
            let has_template_usage = decl
                .usages
                .iter()
                .any(|u| u.block == crate::facts::binding::UsageBlock::Template);
            let has_style_usage = decl
                .usages
                .iter()
                .any(|u| u.block == crate::facts::binding::UsageBlock::Style);
            let has_script_usage = decl
                .usages
                .iter()
                .any(|u| u.block == crate::facts::binding::UsageBlock::Script);

            if !has_template_usage && !has_style_usage && !has_script_usage {
                issues.push(ReactiveFlowIssue {
                    kind: ReactiveFlowIssueKind::UnusedReactive,
                    binding_name: decl.name.clone(),
                    span: decl.span,
                    explanation: format!(
                        "`{}` is reactive but never used in template, style, or script",
                        decl.name
                    ),
                });
            }
        }

        // Check for possibly non-reactive bindings in template
        if fact.status == ReactivityStatus::MaybeReactive {
            let used_in_template = decl
                .usages
                .iter()
                .any(|u| u.block == crate::facts::binding::UsageBlock::Template);

            if used_in_template {
                issues.push(ReactiveFlowIssue {
                    kind: ReactiveFlowIssueKind::PossiblyNonReactive,
                    binding_name: decl.name.clone(),
                    span: decl.span,
                    explanation: format!(
                        "`{}` may not be reactive — template changes might not trigger re-render",
                        decl.name
                    ),
                });
            }
        }
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::binding::{BindingKind, BindingUsage, UsageBlock, UsageKind};
    use crate::facts::reactivity::{ProvenanceStep, ReactivitySource};

    fn make_decl(name: &str, usages: Vec<BindingUsage>) -> BindingDeclaration {
        BindingDeclaration {
            name: name.into(),
            kind: BindingKind::Const,
            span: Span::new(0, 10),
            usages,
        }
    }

    fn template_usage() -> BindingUsage {
        BindingUsage {
            kind: UsageKind::TemplateInterpolation,
            span: Span::new(100, 110),
            block: UsageBlock::Template,
        }
    }

    fn script_usage() -> BindingUsage {
        BindingUsage {
            kind: UsageKind::Read,
            span: Span::new(50, 55),
            block: UsageBlock::Script,
        }
    }

    #[test]
    fn no_issues_for_clean_reactive_binding() {
        let decl = make_decl("count", vec![template_usage()]);
        let fact = ReactivityFact {
            status: ReactivityStatus::Reactive,
            source: Some(ReactivitySource::Ref),
            trace: vec![],
        };
        let issues = analyze_reactive_flow(&[(decl, fact)]);
        assert!(issues.is_empty());
    }

    #[test]
    fn detects_reactivity_loss() {
        let decl = make_decl("msg", vec![template_usage()]);
        let fact = ReactivityFact {
            status: ReactivityStatus::NonReactive,
            source: Some(ReactivitySource::Props),
            trace: vec![
                ProvenanceStep {
                    kind: ProvenanceStepKind::Source,
                    span: Span::new(5, 10),
                    description: "defineProps".into(),
                },
                ProvenanceStep {
                    kind: ProvenanceStepKind::Loss,
                    span: Span::new(15, 30),
                    description: "destructuring loses reactivity".into(),
                },
            ],
        };
        let issues = analyze_reactive_flow(&[(decl, fact)]);

        // Positive: detects the loss
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, ReactiveFlowIssueKind::ReactivityLoss);
        assert_eq!(issues[0].binding_name, "msg");
        assert!(issues[0].explanation.contains("destructuring"));
    }

    #[test]
    fn detects_unused_reactive() {
        let decl = make_decl("unused", vec![]); // no usages
        let fact = ReactivityFact {
            status: ReactivityStatus::Reactive,
            source: Some(ReactivitySource::Ref),
            trace: vec![],
        };
        let issues = analyze_reactive_flow(&[(decl, fact)]);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, ReactiveFlowIssueKind::UnusedReactive);
    }

    #[test]
    fn reactive_used_in_script_only_not_unused() {
        let decl = make_decl("internal", vec![script_usage()]);
        let fact = ReactivityFact {
            status: ReactivityStatus::Reactive,
            source: Some(ReactivitySource::Ref),
            trace: vec![],
        };
        let issues = analyze_reactive_flow(&[(decl, fact)]);

        // Negative: used in script → not flagged as unused
        assert!(issues.is_empty());
    }

    #[test]
    fn detects_maybe_reactive_in_template() {
        let decl = make_decl("result", vec![template_usage()]);
        let fact = ReactivityFact {
            status: ReactivityStatus::MaybeReactive,
            source: None,
            trace: vec![],
        };
        let issues = analyze_reactive_flow(&[(decl, fact)]);

        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].kind, ReactiveFlowIssueKind::PossiblyNonReactive);
    }

    #[test]
    fn maybe_reactive_not_in_template_ok() {
        let decl = make_decl("result", vec![script_usage()]);
        let fact = ReactivityFact {
            status: ReactivityStatus::MaybeReactive,
            source: None,
            trace: vec![],
        };
        let issues = analyze_reactive_flow(&[(decl, fact)]);

        // Negative: not used in template → no issue
        assert!(issues.is_empty());
    }

    #[test]
    fn non_reactive_without_loss_no_issue() {
        let decl = make_decl("label", vec![template_usage()]);
        let fact = ReactivityFact::non_reactive();
        let issues = analyze_reactive_flow(&[(decl, fact)]);

        // Negative: non-reactive with no loss trace → intentional, no issue
        assert!(issues.is_empty());
    }
}
