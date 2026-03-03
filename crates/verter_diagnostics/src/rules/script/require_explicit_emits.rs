//! Rule: require-explicit-emits
//!
//! Requires that all events emitted in the template are declared in `defineEmits`.

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};

pub struct RequireExplicitEmits;

impl LintRule for RequireExplicitEmits {
    fn name(&self) -> &'static str {
        "require-explicit-emits"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Severity {
        Severity::Warning
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        let template = match file.template {
            Some(t) => t,
            None => return,
        };

        // Collect declared emit names
        let declared: std::collections::HashSet<&str> = template
            .emit_definitions
            .iter()
            .filter(|e| e.is_declared)
            .map(|e| e.event_name.as_str())
            .collect();

        // If no defineEmits at all, skip (component may not need emits)
        if declared.is_empty() {
            return;
        }

        // Check for undeclared emit usages
        for emit in &template.emit_definitions {
            if !emit.is_declared && !declared.contains(emit.event_name.as_str()) {
                ctx.report_with_severity(
                    self.name(),
                    self.category().as_str(),
                    format!(
                        "Event '{}' is emitted but not declared in `defineEmits`.",
                        emit.event_name
                    ),
                    emit.span.start,
                    emit.span.end,
                    self.default_severity(),
                    DiagnosticSpanKind::ScriptCallSite,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_compiles() {
        let _rule = RequireExplicitEmits;
        assert_eq!(RequireExplicitEmits.name(), "require-explicit-emits");
    }
}
