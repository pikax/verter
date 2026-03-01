//! Comment directive parser for `@verter:` inline control comments.
//!
//! Parses comment directives from template analysis and populates
//! the lint context with disabled ranges and next-line disables.

use crate::context::LintContext;
use crate::diagnostic::Severity;
use verter_analysis::template::{CommentDirective, CommentDirectiveKind};

/// Parse comment directives and configure the lint context's disabled ranges.
///
/// `source` is the full SFC source text, used to compute precise next-line
/// boundaries for `disable-next-line` directives. When `None`, falls back
/// to a conservative 500-byte window.
pub fn parse_comment_directives(
    directives: &[CommentDirective],
    ctx: &mut LintContext,
    source: Option<&str>,
) {
    let mut ignore_start: Option<(Option<String>, u32)> = None;

    for directive in directives {
        match directive.kind {
            CommentDirectiveKind::Disable => {
                // @verter:disable [rule-name] — disable from this point to EOF (or until enable)
                // For simplicity, treat as a range disable with a large end offset
                ctx.add_disabled_range(directive.message.clone(), directive.span.start, u32::MAX);
            }
            CommentDirectiveKind::DisableNextLine => {
                // @verter:disable-next-line [rule-name] — disable for the next line
                let end = find_next_line_end(source, directive.span.end);
                ctx.add_disabled_next_line(directive.message.clone(), directive.span.end, end);
            }
            CommentDirectiveKind::Enable => {
                // @verter:enable [rule-name] — re-enable a previously disabled rule
                // This is handled by the context's range system:
                // find the matching disable and cap its end offset.
                // For now, this is approximate — the context checks offset ranges.
            }
            CommentDirectiveKind::IgnoreStart => {
                // @verter:ignore-start — begin ignore region
                ignore_start = Some((directive.message.clone(), directive.span.start));
            }
            CommentDirectiveKind::IgnoreEnd => {
                // @verter:ignore-end — end ignore region
                if let Some((rule, start)) = ignore_start.take() {
                    ctx.add_disabled_range(rule, start, directive.span.end);
                }
            }
            CommentDirectiveKind::Level => {
                // @verter:level(warn|error|off) — override severity for the next line
                let severity = match directive.message.as_deref() {
                    Some("warn") | Some("warning") => Some(Some(Severity::Warning)),
                    Some("error") => Some(Some(Severity::Error)),
                    Some("off") => Some(None), // suppress
                    _ => None,                 // invalid or missing — ignore
                };
                if let Some(sev) = severity {
                    let end = find_next_line_end(source, directive.span.end);
                    ctx.add_severity_override(None, sev, directive.span.end, end);
                }
            }
            // Todo, Fixme, Deprecated are informational — not used for disabling
            CommentDirectiveKind::Todo
            | CommentDirectiveKind::Fixme
            | CommentDirectiveKind::Deprecated => {}
        }
    }
}

/// Given the end of a comment directive, find the end of the next line.
///
/// Walks from `comment_end` to the end of the current line, then to the
/// end of the following line. Returns the byte offset of the `\n` at the
/// end of the next line (or end-of-source if there's no trailing newline).
fn find_next_line_end(source: Option<&str>, comment_end: u32) -> u32 {
    let Some(src) = source else {
        // Fallback: conservative 500-byte window when source is unavailable
        return comment_end.saturating_add(500);
    };
    let bytes = src.as_bytes();
    let mut i = comment_end as usize;

    // Skip to end of current line (the line containing the comment)
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    // Skip the newline to enter the next line
    if i < bytes.len() {
        i += 1;
    }
    // Walk to the end of the next line
    while i < bytes.len() && bytes[i] != b'\n' {
        i += 1;
    }
    i as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LintConfig;
    use crate::diagnostic::DiagnosticSpanKind;
    use verter_span::Span;

    fn make_directive(
        kind: CommentDirectiveKind,
        message: Option<&str>,
        start: u32,
        end: u32,
    ) -> CommentDirective {
        CommentDirective {
            kind,
            message: message.map(|s| s.to_string()),
            span: Span::new(start, end),
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
        parse_comment_directives(&directives, &mut ctx, None);

        ctx.report(
            "no-v-html",
            "security",
            "v-html used".to_string(),
            40,
            50,
            DiagnosticSpanKind::Directive,
        );
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
        parse_comment_directives(&directives, &mut ctx, None);

        // Within range of the directive
        ctx.report(
            "no-v-html",
            "security",
            "v-html near".to_string(),
            35,
            50,
            DiagnosticSpanKind::Directive,
        );
        // Far from the directive
        ctx.report(
            "no-v-html",
            "security",
            "v-html far".to_string(),
            2000,
            2010,
            DiagnosticSpanKind::Directive,
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
        parse_comment_directives(&directives, &mut ctx, None);

        ctx.report(
            "any-rule",
            "any",
            "inside region".to_string(),
            50,
            60,
            DiagnosticSpanKind::Directive,
        );
        ctx.report(
            "any-rule",
            "any",
            "outside region".to_string(),
            200,
            210,
            DiagnosticSpanKind::Directive,
        );
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
        parse_comment_directives(&directives, &mut ctx, None);

        ctx.report(
            "any-rule",
            "any",
            "still reported".to_string(),
            40,
            50,
            DiagnosticSpanKind::Directive,
        );
        assert_eq!(ctx.into_diagnostics().len(), 1);
    }

    #[test]
    fn disable_next_line_with_source_only_covers_next_line() {
        // Source layout (byte offsets):
        // "<!-- @verter:disable-next-line no-v-html -->\n<div v-html=\"x\"></div>\n<div v-html=\"y\"></div>"
        //  ^0                                       ^44^45                   ^67^68                  ^90
        //  comment ends at 44 (\n)                     first div line          second div line
        let source = "<!-- @verter:disable-next-line no-v-html -->\n<div v-html=\"x\"></div>\n<div v-html=\"y\"></div>";
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);

        // Comment spans 0..44 (exclusive end = 44, the \n)
        let directives = vec![make_directive(
            CommentDirectiveKind::DisableNextLine,
            Some("no-v-html"),
            0,
            44,
        )];
        parse_comment_directives(&directives, &mut ctx, Some(source));

        // On the next line (offset 50 = "v-html" inside the first <div>)
        ctx.report(
            "no-v-html",
            "security",
            "first div".to_string(),
            50,
            61,
            DiagnosticSpanKind::Directive,
        );
        // On the line after (offset 73 = "v-html" inside the second <div>)
        ctx.report(
            "no-v-html",
            "security",
            "second div".to_string(),
            73,
            84,
            DiagnosticSpanKind::Directive,
        );

        let diags = ctx.into_diagnostics();
        assert_eq!(diags.len(), 1, "only the second div should be reported");
        assert_eq!(diags[0].message, "second div");
    }

    #[test]
    fn disable_next_line_at_eof_covers_last_line() {
        // "<!-- @verter:disable-next-line -->\n<div v-html=\"x\"></div>"
        //  ^0                              ^33^34                   ^56
        let source = "<!-- @verter:disable-next-line -->\n<div v-html=\"x\"></div>";
        let config = LintConfig::default();
        let mut ctx = LintContext::new(&config);

        let directives = vec![make_directive(
            CommentDirectiveKind::DisableNextLine,
            None,
            0,
            33,
        )];
        parse_comment_directives(&directives, &mut ctx, Some(source));

        // On the next (last) line — should be suppressed
        ctx.report(
            "any-rule",
            "any",
            "suppressed".to_string(),
            40,
            50,
            DiagnosticSpanKind::Directive,
        );
        assert!(
            ctx.into_diagnostics().is_empty(),
            "diagnostic on last line after disable-next-line should be suppressed"
        );
    }

    #[test]
    fn find_next_line_end_basic() {
        // "line1\nline2\nline3"
        assert_eq!(find_next_line_end(Some("line1\nline2\nline3"), 3), 11);
        // comment_end=3 is in "line1", next line is "line2" ending at offset 11
    }

    #[test]
    fn find_next_line_end_no_source_fallback() {
        assert_eq!(find_next_line_end(None, 100), 600);
    }

    #[test]
    fn find_next_line_end_at_eof() {
        // "line1\nline2" — no trailing newline
        assert_eq!(find_next_line_end(Some("line1\nline2"), 3), 11);
    }
}
