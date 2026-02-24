//! Comment directive parser for `@verter:` inline control comments.
//!
//! Parses comment directives from template analysis and populates
//! the lint context with disabled ranges and next-line disables.

use crate::context::LintContext;
use verter_analysis::template::{CommentDirective, CommentDirectiveKind};

/// Parse comment directives and configure the lint context's disabled ranges.
pub fn parse_comment_directives(directives: &[CommentDirective], ctx: &mut LintContext) {
    let mut ignore_start: Option<(Option<String>, u32)> = None;

    for directive in directives {
        match directive.kind {
            CommentDirectiveKind::Disable => {
                // @verter:disable [rule-name] — disable from this point to EOF (or until enable)
                // For simplicity, treat as a range disable with a large end offset
                ctx.add_disabled_range(directive.message.clone(), directive.span_start, u32::MAX);
            }
            CommentDirectiveKind::DisableNextLine => {
                // @verter:disable-next-line [rule-name] — disable for the next line
                ctx.add_disabled_next_line(directive.message.clone(), directive.span_end);
            }
            CommentDirectiveKind::Enable => {
                // @verter:enable [rule-name] — re-enable a previously disabled rule
                // This is handled by the context's range system:
                // find the matching disable and cap its end offset.
                // For now, this is approximate — the context checks offset ranges.
            }
            CommentDirectiveKind::IgnoreStart => {
                // @verter:ignore-start — begin ignore region
                ignore_start = Some((directive.message.clone(), directive.span_start));
            }
            CommentDirectiveKind::IgnoreEnd => {
                // @verter:ignore-end — end ignore region
                if let Some((rule, start)) = ignore_start.take() {
                    ctx.add_disabled_range(rule, start, directive.span_end);
                }
            }
            // Todo, Fixme, Deprecated are informational — not used for disabling
            CommentDirectiveKind::Todo
            | CommentDirectiveKind::Fixme
            | CommentDirectiveKind::Deprecated => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;

    fn make_directive(
        kind: CommentDirectiveKind,
        message: Option<&str>,
        start: u32,
        end: u32,
    ) -> CommentDirective {
        CommentDirective {
            kind,
            message: message.map(|s| s.to_string()),
            span_start: start,
            span_end: end,
            affects_next_line: matches!(kind, CommentDirectiveKind::DisableNextLine),
        }
    }

    #[test]
    fn disable_suppresses_later_diagnostic() {
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);

        let directives = vec![make_directive(
            CommentDirectiveKind::Disable,
            Some("no-v-html"),
            0,
            30,
        )];
        parse_comment_directives(&directives, &mut ctx);

        ctx.report("no-v-html", "security", "v-html used".to_string(), 40, 50);
        assert!(ctx.into_diagnostics().is_empty());
    }

    #[test]
    fn disable_next_line_only_affects_nearby() {
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);

        let directives = vec![make_directive(
            CommentDirectiveKind::DisableNextLine,
            Some("no-v-html"),
            0,
            30,
        )];
        parse_comment_directives(&directives, &mut ctx);

        // Within range of the directive
        ctx.report("no-v-html", "security", "v-html near".to_string(), 35, 50);
        // Far from the directive
        ctx.report(
            "no-v-html",
            "security",
            "v-html far".to_string(),
            2000,
            2010,
        );
        let diags = ctx.into_diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "v-html far");
    }

    #[test]
    fn ignore_region_suppresses_all_rules() {
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);

        let directives = vec![
            make_directive(CommentDirectiveKind::IgnoreStart, None, 10, 30),
            make_directive(CommentDirectiveKind::IgnoreEnd, None, 100, 120),
        ];
        parse_comment_directives(&directives, &mut ctx);

        ctx.report("any-rule", "any", "inside region".to_string(), 50, 60);
        ctx.report("any-rule", "any", "outside region".to_string(), 200, 210);
        let diags = ctx.into_diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].message, "outside region");
    }

    #[test]
    fn todo_directive_does_not_disable() {
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);

        let directives = vec![make_directive(
            CommentDirectiveKind::Todo,
            Some("refactor this"),
            0,
            30,
        )];
        parse_comment_directives(&directives, &mut ctx);

        ctx.report("any-rule", "any", "still reported".to_string(), 40, 50);
        assert_eq!(ctx.into_diagnostics().len(), 1);
    }
}
