/// Lightweight SFC block scanner for LSP structural features.
///
/// Finds `<script>`, `<template>`, `<style>`, and custom block boundaries
/// by scanning the raw Vue source. Returns byte offsets for each block's
/// opening tag, content, and closing tag.
///
/// This is intentionally simple — it doesn't parse attributes or validate
/// nesting. It's used for document symbols, folding ranges, and determining
/// which block the cursor is in.
///
/// A detected SFC block with its byte positions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SfcBlock {
    /// Block tag name (e.g., "script", "template", "style", "i18n").
    pub tag_name: String,
    /// Byte offset of the '<' in the opening tag.
    pub open_tag_start: u32,
    /// Byte offset just past the '>' of the opening tag.
    pub open_tag_end: u32,
    /// Byte offset of the '<' in the closing tag.
    pub close_tag_start: u32,
    /// Byte offset just past the '>' of the closing tag.
    pub close_tag_end: u32,
    /// Raw attributes string from the opening tag (e.g., ` setup lang="ts"`).
    pub attrs_raw: String,
}

impl SfcBlock {
    /// Byte range of the block's inner content (between open and close tags).
    pub fn content_range(&self) -> (u32, u32) {
        (self.open_tag_end, self.close_tag_start)
    }

    /// Whether this is a `<script setup>` block.
    pub fn is_setup(&self) -> bool {
        self.tag_name == "script" && self.attrs_raw.contains("setup")
    }

    /// Extract the `lang` attribute value, if present.
    pub fn lang(&self) -> Option<&str> {
        extract_attr_value(&self.attrs_raw, "lang")
    }

    /// Whether this block has the `scoped` attribute.
    pub fn is_scoped(&self) -> bool {
        self.attrs_raw.contains("scoped")
    }

    /// Whether this block has the `module` attribute.
    pub fn is_module(&self) -> bool {
        self.attrs_raw.contains("module")
    }

    /// Extract the `attrs` or `attributes` attribute value, if present.
    pub fn attrs(&self) -> Option<&str> {
        // Try `attrs` first (short form), then `attributes` (long form).
        // Use `attributes` first to avoid matching the `attrs` prefix of `attributes`.
        extract_attr_value(&self.attrs_raw, "attributes")
            .or_else(|| extract_attr_value(&self.attrs_raw, "attrs"))
    }
}

// ── Cursor Context ──────────────────────────────────────────────────────────

/// Where the cursor is in the SFC structure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SfcCursorContext {
    /// Inside a block's content (between open and close tags).
    BlockContent { block_index: usize },
    /// On the opening tag of a block (from `<` to `>`).
    OpeningTag { block_index: usize },
    /// On the closing tag of a block (from `</` to `>`).
    ClosingTag { block_index: usize },
    /// Outside all blocks (root level of the SFC).
    RootLevel,
}

/// Classify the cursor position within the SFC structure.
pub fn classify_cursor(offset: u32, blocks: &[SfcBlock]) -> SfcCursorContext {
    for (i, block) in blocks.iter().enumerate() {
        if offset >= block.open_tag_start && offset < block.open_tag_end {
            return SfcCursorContext::OpeningTag { block_index: i };
        }
        if offset >= block.close_tag_start && offset < block.close_tag_end {
            return SfcCursorContext::ClosingTag { block_index: i };
        }
        let (content_start, content_end) = block.content_range();
        if offset >= content_start && offset < content_end {
            return SfcCursorContext::BlockContent { block_index: i };
        }
    }
    SfcCursorContext::RootLevel
}

/// A parsed attribute from an SFC opening tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAttr {
    /// Attribute name (e.g., "lang", "setup", "scoped").
    pub name: String,
    /// Attribute value, if present (e.g., "ts", "scss"). `None` for boolean attrs.
    pub value: Option<String>,
    /// Absolute byte offset of the attribute name start.
    pub name_start: u32,
    /// Absolute byte offset past the attribute name end.
    pub name_end: u32,
    /// Absolute byte offset of the value start (inside quotes), if present.
    pub value_start: Option<u32>,
    /// Absolute byte offset past the value end (inside quotes), if present.
    pub value_end: Option<u32>,
}

/// Context for the opening tag region of an SFC block.
#[derive(Debug, Clone)]
pub struct OpeningTagContext {
    /// Tag name (e.g., "script", "template", "style").
    pub tag_name: String,
    /// Absolute byte offset of the tag name start (after `<`).
    pub tag_name_start: u32,
    /// Absolute byte offset past the tag name end.
    pub tag_name_end: u32,
    /// Parsed attributes on the opening tag.
    pub attrs: Vec<ParsedAttr>,
}

/// Parse the opening tag region of an SFC block to extract tag name and attributes
/// with absolute byte offsets.
pub fn parse_opening_tag(source: &str, block: &SfcBlock) -> OpeningTagContext {
    let start = block.open_tag_start as usize;
    let end = block.open_tag_end as usize;
    let region = &source[start..end];
    let bytes = region.as_bytes();

    // Skip '<'
    let mut i = 1;

    // Read tag name
    let tag_name_start = i;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
        i += 1;
    }
    let tag_name_end = i;
    let tag_name = region[tag_name_start..tag_name_end].to_string();

    // Parse attributes
    let mut attrs = Vec::new();
    while i < bytes.len() && bytes[i] != b'>' {
        // Skip whitespace
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] == b'>' || bytes[i] == b'/' {
            break;
        }

        // Read attribute name
        let attr_name_start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'='
            && bytes[i] != b'>'
            && bytes[i] != b'/'
        {
            i += 1;
        }
        let attr_name_end = i;
        if attr_name_start == attr_name_end {
            i += 1;
            continue;
        }
        let attr_name = region[attr_name_start..attr_name_end].to_string();

        // Skip whitespace
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }

        // Check for '='
        if i < bytes.len() && bytes[i] == b'=' {
            i += 1;
            // Skip whitespace after '='
            while i < bytes.len() && bytes[i].is_ascii_whitespace() {
                i += 1;
            }

            // Read value
            if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
                let quote = bytes[i];
                i += 1;
                let val_start = i;
                while i < bytes.len() && bytes[i] != quote {
                    i += 1;
                }
                let val_end = i;
                if i < bytes.len() {
                    i += 1; // skip closing quote
                }
                attrs.push(ParsedAttr {
                    name: attr_name,
                    value: Some(region[val_start..val_end].to_string()),
                    name_start: (start + attr_name_start) as u32,
                    name_end: (start + attr_name_end) as u32,
                    value_start: Some((start + val_start) as u32),
                    value_end: Some((start + val_end) as u32),
                });
            } else {
                // Unquoted value
                let val_start = i;
                while i < bytes.len()
                    && !bytes[i].is_ascii_whitespace()
                    && bytes[i] != b'>'
                    && bytes[i] != b'/'
                {
                    i += 1;
                }
                let val_end = i;
                attrs.push(ParsedAttr {
                    name: attr_name,
                    value: Some(region[val_start..val_end].to_string()),
                    name_start: (start + attr_name_start) as u32,
                    name_end: (start + attr_name_end) as u32,
                    value_start: Some((start + val_start) as u32),
                    value_end: Some((start + val_end) as u32),
                });
            }
        } else {
            // Boolean attribute (no value)
            attrs.push(ParsedAttr {
                name: attr_name,
                value: None,
                name_start: (start + attr_name_start) as u32,
                name_end: (start + attr_name_end) as u32,
                value_start: None,
                value_end: None,
            });
        }
    }

    OpeningTagContext {
        tag_name,
        tag_name_start: (start + tag_name_start) as u32,
        tag_name_end: (start + tag_name_end) as u32,
        attrs,
    }
}

/// Scan Vue SFC source text and return all top-level blocks found.
///
/// Blocks are returned in source order. Self-closing tags (e.g., `<template />`)
/// are not treated as blocks since they have no content.
pub fn scan_sfc_blocks(source: &str) -> Vec<SfcBlock> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut blocks = Vec::new();
    let mut i = 0;

    while i < len {
        // Look for '<' that starts a tag (skip comments, text, etc.)
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }

        // Skip comments <!-- ... -->
        if i + 3 < len && &bytes[i..i + 4] == b"<!--" {
            if let Some(end) = find_bytes(bytes, i + 4, b"-->") {
                i = end + 3;
            } else {
                i += 4;
            }
            continue;
        }

        // Skip closing tags (we handle them when matching open tags)
        if i + 1 < len && bytes[i + 1] == b'/' {
            i += 1;
            continue;
        }

        // Try to parse an opening tag
        let tag_start = i;
        i += 1; // skip '<'

        // Read tag name (letters, digits, hyphens)
        let name_start = i;
        while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
            i += 1;
        }
        if i == name_start {
            continue; // empty tag name
        }
        let tag_name = &source[name_start..i];

        // Only match known top-level SFC tags and custom blocks
        // Skip DOCTYPE, html, head, body, etc.
        if !is_sfc_tag(tag_name) {
            continue;
        }

        // Read attributes until '>' or '/>'
        let attrs_start = i;
        let mut self_closing = false;
        while i < len {
            if bytes[i] == b'>' {
                if i > 0 && bytes[i - 1] == b'/' {
                    self_closing = true;
                }
                break;
            }
            // Skip quoted attribute values
            if bytes[i] == b'"' || bytes[i] == b'\'' {
                let quote = bytes[i];
                i += 1;
                while i < len && bytes[i] != quote {
                    i += 1;
                }
            }
            i += 1;
        }
        if i >= len {
            break; // unclosed tag
        }

        let attrs_end = if self_closing { i - 1 } else { i };
        let attrs_raw = source[attrs_start..attrs_end].to_string();
        let open_tag_end = (i + 1) as u32; // past the '>'
        i += 1;

        if self_closing {
            continue; // self-closing blocks have no content
        }

        // Find the matching closing tag
        let close_pattern = format!("</{tag_name}");
        if let Some(close_start) = find_close_tag(source, i, &close_pattern) {
            let close_end = match source[close_start..].find('>') {
                Some(offset) => close_start + offset + 1,
                None => continue,
            };

            blocks.push(SfcBlock {
                tag_name: tag_name.to_string(),
                open_tag_start: tag_start as u32,
                open_tag_end,
                close_tag_start: close_start as u32,
                close_tag_end: close_end as u32,
                attrs_raw,
            });

            i = close_end;
        }
    }

    blocks
}

/// Check if a tag name is an SFC top-level block.
fn is_sfc_tag(name: &str) -> bool {
    matches!(name, "script" | "template" | "style") || is_custom_block_tag(name)
}

/// Check if a tag name could be a custom block.
/// Custom blocks are any tag name that isn't a standard HTML element
/// and isn't script/template/style.
fn is_custom_block_tag(name: &str) -> bool {
    // Custom blocks typically have short, known names like "i18n", "docs", etc.
    // We accept any tag name that:
    // 1. Isn't a standard HTML tag
    // 2. Contains only lowercase letters, digits, hyphens
    // This is a heuristic — the real SFC parser is the source of truth.
    !is_standard_html_tag(name)
}

fn is_standard_html_tag(name: &str) -> bool {
    matches!(
        name,
        "html"
            | "head"
            | "body"
            | "div"
            | "span"
            | "p"
            | "a"
            | "img"
            | "input"
            | "button"
            | "form"
            | "table"
            | "tr"
            | "td"
            | "th"
            | "ul"
            | "ol"
            | "li"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "header"
            | "footer"
            | "nav"
            | "main"
            | "section"
            | "article"
            | "aside"
            | "details"
            | "summary"
            | "dialog"
            | "data"
            | "time"
            | "code"
            | "pre"
            | "blockquote"
            | "hr"
            | "br"
            | "em"
            | "strong"
            | "small"
            | "sub"
            | "sup"
            | "mark"
            | "del"
            | "ins"
            | "abbr"
            | "cite"
            | "dfn"
            | "kbd"
            | "samp"
            | "var"
            | "meta"
            | "link"
            | "base"
            | "title"
            | "noscript"
            | "canvas"
            | "svg"
            | "video"
            | "audio"
            | "source"
            | "track"
            | "embed"
            | "object"
            | "param"
            | "iframe"
            | "map"
            | "area"
            | "select"
            | "textarea"
            | "label"
            | "fieldset"
            | "legend"
            | "output"
            | "progress"
            | "meter"
            | "datalist"
            | "option"
            | "optgroup"
            | "picture"
            | "figure"
            | "figcaption"
            | "colgroup"
            | "col"
            | "thead"
            | "tbody"
            | "tfoot"
            | "caption"
            | "slot"
    )
}

/// Find the position of a closing tag pattern (case-insensitive for the tag name).
fn find_close_tag(source: &str, start: usize, pattern: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    let pat_bytes = pattern.as_bytes();
    let pat_len = pat_bytes.len();
    let mut i = start;

    while i + pat_len <= bytes.len() {
        if bytes[i] == b'<'
            && bytes.get(i + 1) == Some(&b'/')
            && source[i..i + pat_len].eq_ignore_ascii_case(pattern)
        {
            // Verify the next char after tag name is '>' or whitespace
            let after = i + pat_len;
            if after < bytes.len() && (bytes[after] == b'>' || bytes[after].is_ascii_whitespace()) {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Find a byte pattern in `bytes` starting at `start`.
fn find_bytes(bytes: &[u8], start: usize, pattern: &[u8]) -> Option<usize> {
    let pat_len = pattern.len();
    if pat_len == 0 || start + pat_len > bytes.len() {
        return None;
    }
    (start..=bytes.len() - pat_len).find(|&i| &bytes[i..i + pat_len] == pattern)
}

/// Extract a quoted attribute value from a raw attributes string.
fn extract_attr_value<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let pattern = format!("{name}=");
    let idx = attrs.find(&pattern)?;
    let rest = &attrs[idx + pattern.len()..];
    let rest = rest.trim_start();

    if let Some(stripped) = rest.strip_prefix('"') {
        let end = stripped.find('"')?;
        Some(&stripped[..end])
    } else if let Some(stripped) = rest.strip_prefix('\'') {
        let end = stripped.find('\'')?;
        Some(&stripped[..end])
    } else {
        // Unquoted: take until whitespace or end
        let end = rest.find(|c: char| c.is_ascii_whitespace() || c == '>');
        Some(&rest[..end.unwrap_or(rest.len())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Basic scanning
    // ========================================================================

    #[test]
    fn test_basic_sfc() {
        let source = "<template>\n  <div>hello</div>\n</template>\n\n<script setup lang=\"ts\">\nconst x = 1;\n</script>\n\n<style scoped>\n.foo { color: red }\n</style>\n";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].tag_name, "template");
        assert_eq!(blocks[1].tag_name, "script");
        assert_eq!(blocks[2].tag_name, "style");
    }

    #[test]
    fn test_script_setup_detection() {
        let source = "<script setup lang=\"ts\">\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].is_setup());
        assert_eq!(blocks[0].lang(), Some("ts"));
    }

    #[test]
    fn test_style_attributes() {
        let source = "<style scoped lang=\"scss\" module>\n.foo {}\n</style>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].is_scoped());
        assert!(blocks[0].is_module());
        assert_eq!(blocks[0].lang(), Some("scss"));
    }

    #[test]
    fn test_custom_block() {
        let source = "<template>\n  <div/>\n</template>\n\n<i18n lang=\"json\">\n{\"en\": {\"hello\": \"Hello\"}}\n</i18n>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].tag_name, "template");
        assert_eq!(blocks[1].tag_name, "i18n");
        assert_eq!(blocks[1].lang(), Some("json"));
    }

    #[test]
    fn test_multiple_style_blocks() {
        let source = "<style>\n.a {}\n</style>\n<style scoped>\n.b {}\n</style>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].tag_name, "style");
        assert!(!blocks[0].is_scoped());
        assert_eq!(blocks[1].tag_name, "style");
        assert!(blocks[1].is_scoped());
    }

    // ========================================================================
    // Content ranges
    // ========================================================================

    #[test]
    fn test_content_range() {
        let source = "<script setup>\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 1);
        let (start, end) = blocks[0].content_range();
        assert_eq!(&source[start as usize..end as usize], "\nconst x = 1;\n");
    }

    #[test]
    fn test_open_close_tag_positions() {
        let source = "<template>\n  <div/>\n</template>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 1);
        let b = &blocks[0];
        assert_eq!(
            &source[b.open_tag_start as usize..b.open_tag_end as usize],
            "<template>"
        );
        assert_eq!(
            &source[b.close_tag_start as usize..b.close_tag_end as usize],
            "</template>"
        );
    }

    // ========================================================================
    // Edge cases
    // ========================================================================

    #[test]
    fn test_empty_source() {
        let blocks = scan_sfc_blocks("");
        assert!(blocks.is_empty());
    }

    #[test]
    fn test_no_blocks() {
        let blocks = scan_sfc_blocks("just plain text\nno blocks here");
        assert!(blocks.is_empty());
    }

    #[test]
    fn test_html_comment_ignored() {
        let source = "<!-- this is a comment -->\n<template>\n  <div/>\n</template>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].tag_name, "template");
    }

    #[test]
    fn test_self_closing_ignored() {
        let source = "<template />\n<script setup>\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);

        // Self-closing <template /> is skipped
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].tag_name, "script");
    }

    #[test]
    fn test_script_with_companion() {
        // Two script blocks: one <script> and one <script setup>
        let source =
            "<script>\nexport default { name: 'Foo' }\n</script>\n\n<script setup>\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].tag_name, "script");
        assert!(!blocks[0].is_setup());
        assert_eq!(blocks[1].tag_name, "script");
        assert!(blocks[1].is_setup());
    }

    // ========================================================================
    // Attribute extraction
    // ========================================================================

    #[test]
    fn test_extract_attr_value_double_quotes() {
        assert_eq!(extract_attr_value(" lang=\"ts\" setup", "lang"), Some("ts"));
    }

    #[test]
    fn test_extract_attr_value_single_quotes() {
        assert_eq!(extract_attr_value(" lang='scss'", "lang"), Some("scss"));
    }

    #[test]
    fn test_extract_attr_value_missing() {
        assert_eq!(extract_attr_value(" setup", "lang"), None);
    }

    // ========================================================================
    // Real-world SFC
    // ========================================================================

    #[test]
    fn test_realistic_sfc() {
        let source = r#"<template>
  <div class="container">
    <h1>{{ title }}</h1>
    <p v-if="show">Content</p>
  </div>
</template>

<script setup lang="ts">
import { ref } from 'vue'

const title = ref('Hello')
const show = ref(true)
</script>

<style scoped>
.container {
  padding: 16px;
}
</style>
"#;
        let blocks = scan_sfc_blocks(source);

        assert_eq!(blocks.len(), 3);

        // Template
        assert_eq!(blocks[0].tag_name, "template");
        let (cs, ce) = blocks[0].content_range();
        let content = &source[cs as usize..ce as usize];
        assert!(content.contains("<div class=\"container\">"));
        assert!(content.contains("{{ title }}"));

        // Script
        assert_eq!(blocks[1].tag_name, "script");
        assert!(blocks[1].is_setup());
        assert_eq!(blocks[1].lang(), Some("ts"));
        let (cs, ce) = blocks[1].content_range();
        let content = &source[cs as usize..ce as usize];
        assert!(content.contains("import { ref } from 'vue'"));

        // Style
        assert_eq!(blocks[2].tag_name, "style");
        assert!(blocks[2].is_scoped());
        let (cs, ce) = blocks[2].content_range();
        let content = &source[cs as usize..ce as usize];
        assert!(content.contains(".container"));
    }

    // ========================================================================
    // attrs() helper (B8)
    // ========================================================================

    #[test]
    fn test_attrs_short_form() {
        let source = r#"<script setup attrs="{ class?: string }">
const x = 1;
</script>"#;
        let blocks = scan_sfc_blocks(source);
        assert_eq!(blocks[0].attrs(), Some("{ class?: string }"));
    }

    #[test]
    fn test_attrs_long_form() {
        let source = r#"<script setup attributes="{ class?: string }">
const x = 1;
</script>"#;
        let blocks = scan_sfc_blocks(source);
        assert_eq!(blocks[0].attrs(), Some("{ class?: string }"));
    }

    #[test]
    fn test_attrs_not_present() {
        let source = "<script setup lang=\"ts\">\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);
        assert_eq!(blocks[0].attrs(), None);
    }

    #[test]
    fn test_attrs_long_form_preferred_over_short() {
        // When `attributes` is present, it should win even if there's also `attrs`
        // In practice only one would be used, but test the precedence.
        let source = r#"<script setup attributes="{ a: number }" attrs="{ b: string }">
const x = 1;
</script>"#;
        let blocks = scan_sfc_blocks(source);
        assert_eq!(blocks[0].attrs(), Some("{ a: number }"));
    }

    // ========================================================================
    // Cursor context classification (A1)
    // ========================================================================

    #[test]
    fn test_classify_cursor_in_opening_tag() {
        let source = "<script setup lang=\"ts\">\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);

        // Cursor on '<' of opening tag
        assert_eq!(
            classify_cursor(0, &blocks),
            SfcCursorContext::OpeningTag { block_index: 0 }
        );
        // Cursor on 's' of 'setup'
        assert_eq!(
            classify_cursor(8, &blocks),
            SfcCursorContext::OpeningTag { block_index: 0 }
        );
    }

    #[test]
    fn test_classify_cursor_in_content() {
        let source = "<script setup>\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);

        // Cursor in content area
        let (cs, _) = blocks[0].content_range();
        assert_eq!(
            classify_cursor(cs + 1, &blocks),
            SfcCursorContext::BlockContent { block_index: 0 }
        );
    }

    #[test]
    fn test_classify_cursor_in_closing_tag() {
        let source = "<script setup>\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);

        assert_eq!(
            classify_cursor(blocks[0].close_tag_start, &blocks),
            SfcCursorContext::ClosingTag { block_index: 0 }
        );
    }

    #[test]
    fn test_classify_cursor_root_level() {
        let source = "<template>\n  <div/>\n</template>\n\n<script setup>\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);

        // Between blocks (the newlines between </template> and <script>)
        let between_offset = blocks[0].close_tag_end;
        assert_eq!(
            classify_cursor(between_offset, &blocks),
            SfcCursorContext::RootLevel
        );
    }

    #[test]
    fn test_classify_cursor_empty_file() {
        assert_eq!(classify_cursor(0, &[]), SfcCursorContext::RootLevel);
    }

    #[test]
    fn test_classify_cursor_multi_block() {
        let source = "<template>\n  <div/>\n</template>\n\n<script setup lang=\"ts\">\nconst x = 1;\n</script>\n\n<style scoped>\n.foo {}\n</style>";
        let blocks = scan_sfc_blocks(source);
        assert_eq!(blocks.len(), 3);

        // Template opening tag
        assert_eq!(
            classify_cursor(0, &blocks),
            SfcCursorContext::OpeningTag { block_index: 0 }
        );
        // Script opening tag
        assert_eq!(
            classify_cursor(blocks[1].open_tag_start, &blocks),
            SfcCursorContext::OpeningTag { block_index: 1 }
        );
        // Style content
        let (style_cs, _) = blocks[2].content_range();
        assert_eq!(
            classify_cursor(style_cs + 1, &blocks),
            SfcCursorContext::BlockContent { block_index: 2 }
        );
    }

    // ========================================================================
    // Opening tag parser (A1)
    // ========================================================================

    #[test]
    fn test_parse_opening_tag_basic() {
        let source = "<script setup lang=\"ts\">\nconst x = 1;\n</script>";
        let blocks = scan_sfc_blocks(source);
        let ctx = parse_opening_tag(source, &blocks[0]);

        assert_eq!(ctx.tag_name, "script");
        assert_eq!(ctx.attrs.len(), 2);

        // setup is boolean
        assert_eq!(ctx.attrs[0].name, "setup");
        assert_eq!(ctx.attrs[0].value, None);

        // lang has value
        assert_eq!(ctx.attrs[1].name, "lang");
        assert_eq!(ctx.attrs[1].value, Some("ts".to_string()));
    }

    #[test]
    fn test_parse_opening_tag_no_attrs() {
        let source = "<template>\n  <div/>\n</template>";
        let blocks = scan_sfc_blocks(source);
        let ctx = parse_opening_tag(source, &blocks[0]);

        assert_eq!(ctx.tag_name, "template");
        assert!(ctx.attrs.is_empty());
    }

    #[test]
    fn test_parse_opening_tag_attrs_value() {
        let source = r#"<script setup attrs="{ class?: string }" lang="ts">
const x = 1;
</script>"#;
        let blocks = scan_sfc_blocks(source);
        let ctx = parse_opening_tag(source, &blocks[0]);

        assert_eq!(ctx.attrs.len(), 3);
        assert_eq!(ctx.attrs[0].name, "setup");
        assert_eq!(ctx.attrs[1].name, "attrs");
        assert_eq!(ctx.attrs[1].value, Some("{ class?: string }".to_string()));
        assert_eq!(ctx.attrs[2].name, "lang");
        assert_eq!(ctx.attrs[2].value, Some("ts".to_string()));

        // Verify value offsets point to correct source positions
        let vs = ctx.attrs[1].value_start.unwrap() as usize;
        let ve = ctx.attrs[1].value_end.unwrap() as usize;
        assert_eq!(&source[vs..ve], "{ class?: string }");
    }

    #[test]
    fn test_parse_opening_tag_single_quotes() {
        let source = "<style lang='scss' scoped>\n.foo {}\n</style>";
        let blocks = scan_sfc_blocks(source);
        let ctx = parse_opening_tag(source, &blocks[0]);

        assert_eq!(ctx.attrs.len(), 2);
        assert_eq!(ctx.attrs[0].name, "lang");
        assert_eq!(ctx.attrs[0].value, Some("scss".to_string()));
        assert_eq!(ctx.attrs[1].name, "scoped");
        assert_eq!(ctx.attrs[1].value, None);
    }
}
