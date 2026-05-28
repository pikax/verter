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

/// Extract the leading `{Type}` brace payload from a JSDoc tag's text, if the
/// text begins with one (`{Foo} rest` → `"Foo"`). Depth-aware so nested braces
/// (`{Record<string, {nested: true}>}`) match the right closing brace. Returns
/// the payload substring and the remainder after the closing brace.
fn split_jsdoc_brace_payload(text: &str) -> Option<(&str, &str)> {
    let trimmed = text.trim_start();
    let rest = trimmed.strip_prefix('{')?;
    let mut depth = 0u32;
    for (i, ch) in rest.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                if depth == 0 {
                    return Some((rest[..i].trim(), rest[i + 1..].trim_start()));
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    None
}

/// Lower the leading `{Type}` brace payload of a JSDoc tag's text into a
/// [`TypeExpr`] via [`parse_jsdoc_tag_type_payload`]. `None` when the tag has no
/// text or its text does not begin with a `{...}` payload.
///
/// This is the producer-side bridge that makes a JSDoc `{Type}` an ORDINARY
/// type: the returned `TypeExpr` is stored on the same shallow-analysis carrier
/// a TS annotation populates, so it resolves through the shared dispatch with no
/// JSDoc-specific resolution path.
fn lower_jsdoc_tag_type(text: Option<&str>) -> Option<TypeExpr> {
    let (payload, _rest) = split_jsdoc_brace_payload(text?)?;
    if payload.is_empty() {
        return None;
    }
    Some(parse_jsdoc_tag_type_payload(payload))
}

/// The `TypeExpr` declared by a leading JSDoc `@type {T}` annotation on the
/// declaration whose binding/name token starts at `target_start`, if present.
///
/// Used by shallow analysis to give a JSDoc-typed JS value (`/** @type {Foo} */
/// const x = ...`) the SAME `type_annotation` a TS `const x: Foo` carries — the
/// JSDoc type is a first-class regular type, not a separate path. Returns `None`
/// when there is no leading JSDoc, no `@type` tag, or the tag carries no
/// `{...}` payload.
pub fn extract_jsdoc_type_at_offset(source: &str, target_start: u32) -> Option<TypeExpr> {
    let raw = find_leading_jsdoc_near_offset(source, target_start)?;
    let (_description, tags) = parse_jsdoc(raw);
    // `@type` is the explicit value-type annotation. A `@typedef`'s OWN type
    // also lives in its leading `{...}` payload (`/** @typedef {Foo} Bar */`),
    // so accept it here too for the rare inline form.
    tags.iter()
        .find(|tag| matches!(tag.name.as_str(), "type" | "typedef"))
        .and_then(|tag| lower_jsdoc_tag_type(tag.text.as_deref()))
}

/// The return-type `TypeExpr` declared by a leading JSDoc `@returns {T}` (or
/// `@return {T}`) on the declaration whose name token starts at `target_start`.
/// `None` when absent. Used to type a JSDoc-documented function's return when no
/// TS return annotation is present.
pub fn extract_jsdoc_return_type_at_offset(source: &str, target_start: u32) -> Option<TypeExpr> {
    let raw = find_leading_jsdoc_near_offset(source, target_start)?;
    let (_description, tags) = parse_jsdoc(raw);
    tags.iter()
        .find(|tag| matches!(tag.name.as_str(), "returns" | "return"))
        .and_then(|tag| lower_jsdoc_tag_type(tag.text.as_deref()))
}

/// The `@param {T} name` parameter types declared by a leading JSDoc block on
/// the declaration whose name token starts at `target_start`, keyed by
/// parameter name. Each entry's `TypeExpr` is the lowered `{T}` payload. Empty
/// when there is no leading JSDoc or no `@param` tags carry a `{...}` payload.
/// Used to type a JSDoc-documented function's parameters that lack a TS
/// annotation.
pub fn extract_jsdoc_param_types_at_offset(
    source: &str,
    target_start: u32,
) -> Vec<(String, TypeExpr)> {
    let Some(raw) = find_leading_jsdoc_near_offset(source, target_start) else {
        return Vec::new();
    };
    let (_description, tags) = parse_jsdoc(raw);
    let mut params = Vec::new();
    for tag in &tags {
        if !matches!(tag.name.as_str(), "param" | "arg" | "argument") {
            continue;
        }
        let Some(text) = tag.text.as_deref() else {
            continue;
        };
        let Some((payload, rest)) = split_jsdoc_brace_payload(text) else {
            continue;
        };
        if payload.is_empty() {
            continue;
        }
        // The parameter name is the first whitespace-delimited token after the
        // `{T}` payload (`@param {Foo} value description`). An optional name is
        // written `[value]` in JSDoc; strip the brackets to recover the name.
        let Some(raw_name) = rest.split_whitespace().next() else {
            continue;
        };
        let name = raw_name
            .trim_start_matches('[')
            .split(['=', ']'])
            .next()
            .unwrap_or(raw_name)
            .trim();
        if name.is_empty() {
            continue;
        }
        params.push((name.to_string(), parse_jsdoc_tag_type_payload(payload)));
    }
    params
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
            // Declaration-leading keywords: a JSDoc block precedes the WHOLE
            // declaration (`/** @type {T} */ const x = ...`), but the offset a
            // value / function extractor has is the binding NAME (`x`), so the
            // walk back from the name crosses the `const` / `let` / `var` /
            // `function` keyword before reaching the comment. These are real
            // declaration leaders, so attaching the leading JSDoc through them
            // is correct (the same as crossing `export`).
            | "const"
            | "let"
            | "var"
            | "function"
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
/// Searches for `name :`, `name ?:`, or method-style `name (` patterns where
/// `name` is a complete identifier (not a substring of another). For each
/// candidate, attempts to extract the leading JSDoc using
/// `extract_jsdoc_near_offset`. Returns the first occurrence with non-empty
/// JSDoc, or `(None, Vec::new())` if none.
pub fn extract_jsdoc_for_property_name(
    source: &str,
    prop_name: &str,
) -> (Option<String>, Vec<JsdocTag>) {
    extract_jsdoc_for_property_name_in_range(source, prop_name, 0, source.len())
}

/// Span-scoped variant of [`extract_jsdoc_for_property_name`]: searches for the
/// member declaration site ONLY within the byte range `[range_start,
/// range_end)`.
///
/// This is the declaration-provenance JSDoc lookup. A file may declare the same
/// property name in two declarations (only one of which is the heritage base an
/// inherited member came from); a file-wide first match would attach the wrong
/// declaration's JSDoc. Scoping the search to the declaring declaration's full
/// span (`AnalyzedExternalTypeSource::local_symbol_span`) attaches the correct
/// leading JSDoc. The match accepts property-style (`name:` / `name?:`) AND
/// method-style (`name(` — e.g. `default(props): any`) members.
///
/// `range_start` / `range_end` are clamped to the source bounds. An empty or
/// inverted range yields `(None, Vec::new())`.
pub fn extract_jsdoc_for_property_name_in_range(
    source: &str,
    prop_name: &str,
    range_start: usize,
    range_end: usize,
) -> (Option<String>, Vec<JsdocTag>) {
    if prop_name.is_empty() {
        return (None, Vec::new());
    }
    let bytes = source.as_bytes();
    let range_end = range_end.min(bytes.len());
    if range_start >= range_end {
        return (None, Vec::new());
    }
    let pat = prop_name.as_bytes();
    let mut search_start = range_start;

    while let Some(rel) = source.get(search_start..range_end).and_then(|window| {
        window
            .find(prop_name)
            .filter(|rel| search_start + rel + pat.len() <= range_end)
    }) {
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
            // Property-style (`name:` / `name?:`) OR method-style (`name(`,
            // e.g. an interface method member `default(props): any`). A
            // method-style member declares its leading JSDoc the same way a
            // property does, so the same `extract_jsdoc_near_offset` resolves
            // it from the member-name offset.
            if cursor < bytes.len() && (bytes[cursor] == b':' || bytes[cursor] == b'(') {
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

    #[test]
    fn extract_in_range_scopes_to_declaring_declaration_span() {
        use super::extract_jsdoc_for_property_name_in_range;
        // Two declarations declare `base` with DIFFERENT JSDoc. A file-wide
        // search would return the FIRST (`Decoy.base`); scoping to the SECOND
        // declaration's byte range must return the second declaration's JSDoc.
        let source = "interface Decoy {\n  /** DECOY base doc */\n  base: string\n}\n\
                      interface BaseProps {\n  /** correct base doc */\n  base: number\n}";
        // Whole-file search returns the first textual match (the decoy).
        let (whole, _) = super::extract_jsdoc_for_property_name(source, "base");
        assert_eq!(
            whole.as_deref(),
            Some("DECOY base doc"),
            "whole-file search returns the first textual `base:` (the decoy) — \
             this is exactly the bug span-scoping fixes",
        );
        // Scope to the second declaration's span: returns the correct doc.
        let second_start = source
            .find("interface BaseProps")
            .expect("BaseProps declaration present");
        let (scoped, _) =
            extract_jsdoc_for_property_name_in_range(source, "base", second_start, source.len());
        assert_eq!(
            scoped.as_deref(),
            Some("correct base doc"),
            "scoping the search to BaseProps's span MUST return BaseProps's JSDoc, \
             NOT the file-first Decoy match",
        );
    }

    #[test]
    fn extract_in_range_matches_method_style_members() {
        use super::extract_jsdoc_for_property_name_in_range;
        // A method-style member (`default(props): any`) declares leading JSDoc.
        // The matcher must accept the `name(` form (not only `name:`).
        let source =
            "interface Slots {\n  /** the default slot */\n  default(props: { x: string }): any\n}";
        let (desc, _) =
            extract_jsdoc_for_property_name_in_range(source, "default", 0, source.len());
        assert_eq!(
            desc.as_deref(),
            Some("the default slot"),
            "method-style member `default(props): any` MUST get its leading JSDoc \
             (the matcher accepts `name(`)",
        );
    }

    #[test]
    fn extract_in_range_empty_or_inverted_range_yields_none() {
        use super::extract_jsdoc_for_property_name_in_range;
        let source = "interface X {\n  /** doc */\n  foo: number\n}";
        // Inverted range.
        let (desc, tags) = extract_jsdoc_for_property_name_in_range(source, "foo", 30, 10);
        assert!(
            desc.is_none() && tags.is_empty(),
            "inverted range yields none"
        );
        // Range that excludes the `foo:` declaration site (only the header).
        let header_end = source.find('{').expect("brace") + 1;
        let (desc2, tags2) = extract_jsdoc_for_property_name_in_range(source, "foo", 0, header_end);
        assert!(
            desc2.is_none() && tags2.is_empty(),
            "a range that excludes the member declaration site yields none",
        );
    }
}
