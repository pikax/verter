use oxc_ast::{Comment, CommentContent};

use verter_type_expr::TypeExpr;

use crate::analysis::types::JsdocTag;

/// Parse a JSDoc `{Type}` tag-type payload string into a [`TypeExpr`].
///
/// This is the **single permitted text-input boundary** for the typed-IR
/// resolver: JSDoc tag payloads are inherently text (a `@param {Foo}` tag
/// carries `Foo` as a string from the parser), so they must be lowered
/// through a wrap-and-lower OXC parse here. Every other producer-side
/// caller in the resolver / projector / registry / policy / materialiser
/// pipeline operates on a `TSType<'_>` AST node and goes through
/// [`verter_type_expr_oxc::lower_ts_type`] directly — see the
/// "Typed-IR-Only Resolver Rule" in CLAUDE.md.
///
/// Wraps `input` in `type __T = <input>`, parses via OXC, and lowers the
/// resulting `TSType` node via `lower_ts_type`. Returns
/// [`TypeExpr::Unknown`] if the input is empty or the wrapper parse does
/// not produce a `TSTypeAliasDeclaration`.
pub fn parse_jsdoc_tag_type_payload(input: &str) -> TypeExpr {
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    if input.trim().is_empty() {
        return TypeExpr::Unknown {
            raw: input.to_string(),
        };
    }

    let wrapper = format!("type __T = {input}");
    let allocator = Allocator::default();
    let source_type = SourceType::ts();
    let ret = Parser::new(&allocator, &wrapper, source_type).parse();

    for stmt in &ret.program.body {
        if let oxc_ast::ast::Statement::TSTypeAliasDeclaration(alias) = stmt {
            return verter_type_expr_oxc::lower_ts_type(&alias.type_annotation, &wrapper);
        }
    }

    TypeExpr::Unknown {
        raw: input.to_string(),
    }
}

fn find_leading_jsdoc_from_comments<'a>(
    comments: &[Comment],
    target_start: u32,
    source: &'a str,
) -> Option<&'a str> {
    for comment in comments {
        if comment.attached_to == target_start
            && comment.is_block()
            && matches!(
                comment.content,
                CommentContent::Jsdoc | CommentContent::JsdocLegal
            )
        {
            let start = comment.span.start as usize;
            let end = comment.span.end as usize;
            if end <= source.len() {
                return Some(&source[start..end]);
            }
        }
    }

    None
}

fn find_leading_jsdoc_immediately_before(source: &str, start: usize) -> Option<&str> {
    if start == 0 || start > source.len() {
        return None;
    }

    let prefix = source.get(..start)?;
    let trimmed = prefix.trim_end();
    if !trimmed.ends_with("*/") {
        return None;
    }

    let comment_start = trimmed.rfind("/**")?;
    let raw = trimmed.get(comment_start..)?;
    if raw.ends_with("*/") {
        Some(raw)
    } else {
        None
    }
}

fn previous_identifier_token(source: &str, end: usize) -> Option<(usize, &str)> {
    if end == 0 || end > source.len() {
        return None;
    }

    let bytes = source.as_bytes();
    let mut token_end = end;
    while token_end > 0 && bytes[token_end - 1].is_ascii_whitespace() {
        token_end -= 1;
    }
    if token_end == 0 {
        return None;
    }

    let mut token_start = token_end;
    while token_start > 0 {
        let byte = bytes[token_start - 1];
        if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' {
            token_start -= 1;
            continue;
        }
        break;
    }

    (token_start != token_end).then_some((token_start, &source[token_start..token_end]))
}

fn is_jsdoc_prefix_token(token: &str) -> bool {
    matches!(
        token,
        "export"
            | "default"
            | "declare"
            | "abstract"
            | "async"
            | "public"
            | "private"
            | "protected"
            | "readonly"
            | "static"
            | "override"
            | "accessor"
    )
}

fn find_leading_jsdoc_near_offset(source: &str, target_start: u32) -> Option<&str> {
    let start = target_start as usize;
    if let Some(raw) = find_leading_jsdoc_immediately_before(source, start) {
        return Some(raw);
    }

    let mut cursor = start;
    for _ in 0..8 {
        let (token_start, token) = previous_identifier_token(source, cursor)?;
        if !is_jsdoc_prefix_token(token) {
            return None;
        }
        if let Some(raw) = find_leading_jsdoc_immediately_before(source, token_start) {
            return Some(raw);
        }
        cursor = token_start;
    }

    None
}

pub fn parse_jsdoc(raw: &str) -> (Option<String>, Vec<JsdocTag>) {
    let inner = raw.trim_start_matches("/**").trim_end_matches("*/").trim();

    let lines: Vec<&str> = inner
        .lines()
        .map(|line| line.trim_start())
        .map(|line| line.strip_prefix('*').unwrap_or(line))
        .map(|line| line.trim_start())
        .collect();

    let mut description_parts = Vec::new();
    let mut tags = Vec::new();
    let mut current_tag: Option<(String, Vec<String>)> = None;

    for line in &lines {
        if let Some(stripped) = line.strip_prefix('@') {
            if let Some((name, text_parts)) = current_tag.take() {
                let text = text_parts.join(" ");
                tags.push(JsdocTag {
                    name,
                    text: if text.is_empty() { None } else { Some(text) },
                });
            }

            let mut parts = stripped.splitn(2, char::is_whitespace);
            let name = parts.next().unwrap_or("").trim().to_string();
            let rest = parts.next().unwrap_or("").trim();
            let text_parts = if rest.is_empty() {
                Vec::new()
            } else {
                vec![rest.to_string()]
            };
            current_tag = Some((name, text_parts));
        } else if let Some((_, text_parts)) = current_tag.as_mut() {
            if !line.is_empty() {
                text_parts.push((*line).to_string());
            }
        } else if description_parts.is_empty() && line.is_empty() {
            // Skip leading blank lines before any description text.
        } else {
            // Preserve blank lines as empty strings for paragraph breaks.
            description_parts.push(*line);
        }
    }

    if let Some((name, text_parts)) = current_tag {
        let text = text_parts.join(" ");
        tags.push(JsdocTag {
            name,
            text: if text.is_empty() { None } else { Some(text) },
        });
    }

    // Join description lines with newlines to preserve multi-line formatting.
    // Blank lines between paragraphs become "\n\n".
    let description = if description_parts.is_empty() {
        None
    } else {
        // Trim trailing blank lines.
        while description_parts.last() == Some(&"") {
            description_parts.pop();
        }
        let joined = description_parts.join("\n");
        if joined.is_empty() {
            None
        } else {
            Some(joined)
        }
    };

    (description, tags)
}

pub fn extract_jsdoc_for_comments(
    comments: &[Comment],
    target_start: u32,
    source: &str,
) -> (Option<String>, Vec<JsdocTag>) {
    match find_leading_jsdoc_from_comments(comments, target_start, source) {
        Some(raw) => parse_jsdoc(raw),
        None => (None, Vec::new()),
    }
}

pub fn extract_jsdoc_near_offset(
    source: &str,
    target_start: u32,
) -> (Option<String>, Vec<JsdocTag>) {
    match find_leading_jsdoc_near_offset(source, target_start) {
        Some(raw) => parse_jsdoc(raw),
        None => (None, Vec::new()),
    }
}

/// Find JSDoc preceding a property declaration with the given name in the source
/// text. Used as a name-based fallback for expanded-only props that have no
/// span on the AST (`ExpandedProperty` carries no span).
///
/// Searches for `name :` or `name ?:` patterns where `name` is a complete
/// identifier (not a substring of another). For each candidate, attempts to
/// extract the leading JSDoc using `extract_jsdoc_near_offset`. Returns the
/// first occurrence with non-empty JSDoc, or `(None, Vec::new())` if none.
pub fn extract_jsdoc_for_property_name(
    source: &str,
    prop_name: &str,
) -> (Option<String>, Vec<JsdocTag>) {
    if prop_name.is_empty() {
        return (None, Vec::new());
    }
    let bytes = source.as_bytes();
    let pat = prop_name.as_bytes();
    let mut search_start = 0usize;

    while let Some(rel) = source[search_start..].find(prop_name) {
        let abs = search_start + rel;
        let after = abs + pat.len();

        let word_boundary_before = abs == 0 || !is_identifier_continue(bytes[abs - 1]);
        let word_boundary_after = after >= bytes.len() || !is_identifier_continue(bytes[after]);

        if word_boundary_before && word_boundary_after {
            let mut cursor = after;
            while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b'?' {
                cursor += 1;
            }
            if cursor < bytes.len() && bytes[cursor] == b':' {
                let (description, tags) = extract_jsdoc_near_offset(source, abs as u32);
                if description.is_some() || !tags.is_empty() {
                    return (description, tags);
                }
            }
        }

        search_start = abs + 1;
    }

    (None, Vec::new())
}

fn is_identifier_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

#[cfg(test)]
mod tests {
    use super::{extract_jsdoc_near_offset, parse_jsdoc_tag_type_payload};
    use verter_type_expr::{PrimitiveName, TypeExpr};

    #[test]
    fn parse_jsdoc_tag_type_payload_lowers_primitive_keyword() {
        let expr = parse_jsdoc_tag_type_payload("string");
        assert_eq!(
            expr,
            TypeExpr::Primitive(PrimitiveName::String),
            "primitive JSDoc payload should lower to the matching TypeExpr primitive"
        );
    }

    #[test]
    fn parse_jsdoc_tag_type_payload_lowers_array_with_element_type() {
        // OXC lowers `Array<number>` to `TypeExpr::Array { element }`,
        // not a `Ref<Array, [number]>`. The lowering is canonical: any
        // `Array<T>` / `T[]` / `ReadonlyArray<T>` collapses into `Array`.
        let expr = parse_jsdoc_tag_type_payload("Array<number>");
        match expr {
            TypeExpr::Array { element, .. } => {
                assert_eq!(&*element, &TypeExpr::Primitive(PrimitiveName::Number));
            }
            other => panic!("expected Array<number>, got {other:?}"),
        }
    }

    #[test]
    fn parse_jsdoc_tag_type_payload_lowers_union() {
        let expr = parse_jsdoc_tag_type_payload("string | number");
        match expr {
            TypeExpr::Union(members) => {
                assert_eq!(members.len(), 2, "union must lower with two members");
                assert!(members
                    .iter()
                    .any(|m| matches!(m, TypeExpr::Primitive(PrimitiveName::String))));
                assert!(members
                    .iter()
                    .any(|m| matches!(m, TypeExpr::Primitive(PrimitiveName::Number))));
            }
            other => panic!("expected Union, got {other:?}"),
        }
    }

    #[test]
    fn parse_jsdoc_tag_type_payload_unknown_for_empty_input() {
        let expr = parse_jsdoc_tag_type_payload("");
        match expr {
            TypeExpr::Unknown { raw } => assert_eq!(raw, "", "empty input keeps empty raw"),
            other => panic!("expected Unknown for empty payload, got {other:?}"),
        }
    }

    #[test]
    fn extract_jsdoc_near_offset_skips_export_modifier_tokens() {
        let source = r#"
/** Description of the Props interface.
 * @deprecated Use NewProps instead.
 */
export interface Props { a: string }
"#;
        let target_start = source
            .find("interface Props")
            .expect("interface keyword should exist") as u32;

        let (description, tags) = extract_jsdoc_near_offset(source, target_start);

        assert_eq!(
            description.as_deref(),
            Some("Description of the Props interface.")
        );
        assert!(tags.iter().any(|tag| tag.name == "deprecated"));
    }

    #[test]
    fn extract_jsdoc_near_offset_skips_multiple_declaration_modifiers() {
        let source = r#"
/** Description of the Value class. */
export declare abstract class Value {}
"#;
        let target_start = source
            .find("class Value")
            .expect("class keyword should exist") as u32;

        let (description, tags) = extract_jsdoc_near_offset(source, target_start);

        assert_eq!(
            description.as_deref(),
            Some("Description of the Value class.")
        );
        assert!(tags.is_empty());
    }

    #[test]
    fn parse_jsdoc_preserves_newlines_between_description_lines() {
        let raw = r#"/**
         * When type is "single", allows closing content when clicking trigger for an open item.
         * When type is "multiple", this prop has no effect.
         */"#;
        let (description, _) = super::parse_jsdoc(raw);
        assert_eq!(
            description.as_deref(),
            Some("When type is \"single\", allows closing content when clicking trigger for an open item.\nWhen type is \"multiple\", this prop has no effect.")
        );
    }

    #[test]
    fn parse_jsdoc_preserves_paragraph_breaks() {
        let raw = r#"/**
         * The default active value of the item(s).
         *
         * Use when you do not need to control the state of the item(s).
         */"#;
        let (description, _) = super::parse_jsdoc(raw);
        assert_eq!(
            description.as_deref(),
            Some("The default active value of the item(s).\n\nUse when you do not need to control the state of the item(s).")
        );
    }

    #[test]
    fn parse_jsdoc_single_line_unchanged() {
        let raw = "/** Simple description. */";
        let (description, _) = super::parse_jsdoc(raw);
        assert_eq!(description.as_deref(), Some("Simple description."));
    }
}
