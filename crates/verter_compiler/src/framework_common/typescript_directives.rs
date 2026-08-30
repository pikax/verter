//! Shared extraction of authored JavaScript file-check pragmas for framework
//! IDE companions.

/// Return genuine TypeScript file-check pragmas from the leading trivia of the
/// authored script bodies, in carrier source order.
///
/// `bodies` contains byte ranges for framework script contents. Only line-form
/// `@ts-check` and `@ts-nocheck` directives in leading trivia are file pragmas;
/// block comments and token lookalikes remain ordinary comments.
pub(crate) fn authored_check_directives(
    source: &str,
    bodies: impl IntoIterator<Item = (u32, u32)>,
) -> Vec<&str> {
    let mut bodies = bodies.into_iter().collect::<Vec<_>>();
    bodies.sort_unstable_by_key(|(start, _)| *start);

    let mut directives = Vec::new();
    for (start, end) in bodies {
        let mut leading = &source[start as usize..end as usize];
        loop {
            leading = leading.trim_start_matches(char::is_whitespace);
            if let Some(line) = leading.strip_prefix("//") {
                let (comment, rest) = line
                    .split_once('\n')
                    .map_or((line, ""), |(comment, rest)| (comment, rest));
                let comment = comment.trim_start();
                for directive in ["@ts-check", "@ts-nocheck"] {
                    if let Some(suffix) = comment.strip_prefix(directive) {
                        if suffix.chars().next().is_none_or(|character| {
                            character.is_ascii_whitespace() || character == ':'
                        }) {
                            directives.push(directive);
                        }
                    }
                }
                leading = rest;
                continue;
            }
            if let Some(block) = leading.strip_prefix("/*") {
                let Some(end) = block.find("*/") else {
                    break;
                };
                leading = &block[end + 2..];
                continue;
            }
            break;
        }
    }
    directives
}

#[cfg(test)]
mod tests {
    use super::authored_check_directives;

    #[test]
    fn accepts_only_leading_line_form_file_check_pragmas() {
        for source in [
            "// @ts-check\nlet value = 1",
            "  // @ts-nocheck: reason\nvalue",
        ] {
            let expected = if source.contains("nocheck") {
                "@ts-nocheck"
            } else {
                "@ts-check"
            };
            assert_eq!(
                authored_check_directives(source, [(0, source.len() as u32)]),
                vec![expected]
            );
        }
        for source in [
            "/* @ts-check */\nlet value = 1",
            "// @ts-check/foo\nlet value = 1",
            "let value = 1;\n// @ts-check",
        ] {
            assert!(authored_check_directives(source, [(0, source.len() as u32)]).is_empty());
        }
    }
}
