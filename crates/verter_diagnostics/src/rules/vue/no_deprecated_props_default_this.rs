//! Rule: no-deprecated-props-default-this
//!
//! In Vue 2, props factory defaults could access `this`. In Vue 3, `this` is
//! no longer available in props default functions. This rule flags the pattern
//! as a known deprecation. Full implementation requires deep AST analysis of
//! default function bodies.

// @ai-generated

use crate::context::LintContext;
use crate::diagnostic::{DiagnosticSpanKind, DiagnosticTag, Severity};
use crate::rules::{FileContext, LintRule, RuleCategory};

pub struct NoDeprecatedPropsDefaultThis;

impl LintRule for NoDeprecatedPropsDefaultThis {
    fn name(&self) -> &'static str {
        "no-deprecated-props-default-this"
    }

    fn category(&self) -> RuleCategory {
        RuleCategory::VueEssential
    }

    fn default_severity(&self) -> Option<Severity> {
        Some(Severity::Warning)
    }

    fn check_file(&self, file: &FileContext<'_>, ctx: &mut LintContext) {
        let source = match file.source {
            Some(s) => s,
            None => return,
        };

        // Simplified heuristic: look for `default()` or `default:` followed by `this.`
        // within the source. This is a best-effort check.
        let mut search_start = 0;
        while let Some(idx) = source[search_start..].find("this.") {
            let abs_idx = search_start + idx;
            // Check if we're inside a `default` context by looking backwards
            let prefix = &source[..abs_idx];
            if prefix.rfind("default").is_some_and(|di| {
                // Only trigger if "default" is reasonably close (within 200 chars)
                abs_idx - di < 200
            }) {
                ctx.report_with_tags(
                    self.name(),
                    self.category().as_str(),
                    "'this' is no longer available in props default functions in Vue 3. Use a closure parameter or a separate helper instead.".to_string(),
                    abs_idx as u32,
                    (abs_idx + 5) as u32,
                    self.default_severity(),
                    vec![DiagnosticTag::Deprecated],
                    DiagnosticSpanKind::ScriptCallSite,
                );
                // Only report the first occurrence to avoid noise
                return;
            }
            search_start = abs_idx + 5;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::rules::FileContext;
    use crate::visitor::LintVisitor;

    fn run_file(source: &str) -> Vec<crate::diagnostic::LintDiagnostic> {
        let rules: Vec<Box<dyn LintRule>> = vec![Box::new(NoDeprecatedPropsDefaultThis)];
        let visitor = LintVisitor::new(&rules);
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);
        let file = FileContext {
            template: None,
            script: None,
            styles: &[],
            source: Some(source),
        };
        visitor.visit_file(&file, &mut ctx);
        ctx.into_diagnostics()
    }

    #[test]
    fn this_in_default_reports() {
        let source = r#"<script>
export default {
  props: {
    value: {
      default() { return this.otherProp }
    }
  }
}
</script>"#;
        let diags = run_file(source);
        assert!(!diags.is_empty(), "this. in props default should trigger");
        assert!(diags
            .iter()
            .any(|d| d.rule == "no-deprecated-props-default-this"));
        assert!(
            diags[0].tags.contains(&DiagnosticTag::Deprecated),
            "should have Deprecated tag"
        );
        assert!(
            !diags.iter().any(|d| d.rule == "no-v-html"),
            "must not trigger unrelated rule"
        );
    }

    #[test]
    fn no_this_passes() {
        let source = r#"<script>
export default {
  props: {
    value: {
      default() { return 42 }
    }
  }
}
</script>"#;
        let diags = run_file(source);
        assert!(diags.is_empty(), "no this. reference should pass");
    }
}
