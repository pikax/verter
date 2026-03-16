/// HTML void elements that must not be auto-closed.
const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

/// Compute the auto-closing tag edit when `>` is typed.
///
/// Returns a `TextEdit` that inserts `$0</tagname>` right after the `>`
/// at `position`, or `None` if no closing tag should be inserted.
pub fn auto_close_tag(source: &str, offset: usize) -> Option<String> {
    // `offset` points right after the typed `>`.
    // Walk backward to find the opening tag.
    if offset == 0 || offset > source.len() {
        return None;
    }

    let bytes = source.as_bytes();

    // The `>` itself is at offset - 1.
    let gt_pos = offset - 1;
    if bytes[gt_pos] != b'>' {
        return None;
    }

    // Skip if self-closing: `/>` or `?>`
    if gt_pos > 0 && (bytes[gt_pos - 1] == b'/' || bytes[gt_pos - 1] == b'?') {
        return None;
    }

    // Walk backward from gt_pos to find the matching `<`
    let mut pos = gt_pos;
    let mut found_lt = false;
    while pos > 0 {
        pos -= 1;
        if bytes[pos] == b'<' {
            found_lt = true;
            break;
        }
        // If we hit another `>`, we've gone past a different tag
        if bytes[pos] == b'>' {
            return None;
        }
    }

    if !found_lt {
        return None;
    }

    // pos points to `<`. Check it's not a closing tag, comment, or special tag.
    let after_lt = pos + 1;
    if after_lt >= gt_pos {
        return None;
    }
    let first_char = bytes[after_lt];

    // Skip closing tags `</`, comments `<!`, processing `<?`
    if first_char == b'/' || first_char == b'!' || first_char == b'?' {
        return None;
    }

    // Extract tag name: from after `<` up to first whitespace, `/`, or `>`
    let tag_content = &source[after_lt..gt_pos];
    let tag_name = tag_content
        .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .next()
        .unwrap_or("")
        .trim();

    if tag_name.is_empty() {
        return None;
    }

    // Skip void elements
    let tag_lower = tag_name.to_ascii_lowercase();
    if VOID_TAGS.contains(&tag_lower.as_str()) {
        return None;
    }

    // Check if there's already a closing tag immediately after
    let remaining = &source[offset..];
    let expected_close = format!("</{}", tag_name);
    if remaining.trim_start().starts_with(&expected_close) {
        return None;
    }

    Some(format!("$0</{}>", tag_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_close_div() {
        let source = "<template><div></div></template>";
        // Cursor right after `<div>` at offset 15
        let result = auto_close_tag(source, 15);
        // Already has </div> immediately after
        assert!(
            result.is_none(),
            "should not close when </div> already exists"
        );
    }

    #[test]
    fn auto_close_div_no_existing() {
        let source = "<template><div>\n</template>";
        let result = auto_close_tag(source, 15);
        assert_eq!(result, Some("$0</div>".to_string()));
    }

    #[test]
    fn no_close_for_void_element() {
        let source = "<template><br></template>";
        let result = auto_close_tag(source, 14);
        assert!(result.is_none(), "void elements should not be closed");
    }

    #[test]
    fn no_close_for_self_closing() {
        let source = "<template><MyComp /></template>";
        // Offset after `/>` — but `>` is at pos 19, so offset is 20
        let result = auto_close_tag(source, 20);
        assert!(result.is_none(), "self-closing tags should not be closed");
    }

    #[test]
    fn auto_close_component() {
        let source = "<template><MyComponent>\n</template>";
        let result = auto_close_tag(source, 23);
        assert_eq!(result, Some("$0</MyComponent>".to_string()));
    }

    #[test]
    fn auto_close_with_attributes() {
        let source = r#"<template><div class="foo" id="bar">"#;
        let result = auto_close_tag(source, 36);
        assert_eq!(result, Some("$0</div>".to_string()));
    }

    #[test]
    fn no_close_for_closing_tag() {
        let source = "<template></div></template>";
        // Cursor after `</div>` at offset 16
        let result = auto_close_tag(source, 16);
        assert!(
            result.is_none(),
            "closing tags should not trigger auto-close"
        );
    }

    #[test]
    fn no_close_for_comment() {
        let source = "<template><!-- comment --></template>";
        // This is `-->` so `>` at offset 25
        let result = auto_close_tag(source, 26);
        assert!(result.is_none(), "comments should not trigger auto-close");
    }

    #[test]
    fn auto_close_template_tag() {
        let source = "<template>\n</template>";
        // Offset after first `<template>`
        let result = auto_close_tag(source, 10);
        // Already has </template> right after (with newline)
        assert!(
            result.is_none(),
            "should not close when </template> already exists after whitespace"
        );
    }

    #[test]
    fn auto_close_preserves_case() {
        let source = "<template><MyButton>\n</template>";
        let result = auto_close_tag(source, 20);
        assert_eq!(
            result,
            Some("$0</MyButton>".to_string()),
            "should preserve original tag case"
        );
    }

    #[test]
    fn no_close_for_void_input() {
        let source = r#"<template><input type="text"></template>"#;
        let result = auto_close_tag(source, 29);
        assert!(result.is_none(), "input is a void element");
    }
}
