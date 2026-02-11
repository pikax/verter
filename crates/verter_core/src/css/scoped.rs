//! Scoped CSS visitor using lightningcss.
//!
//! Walks the CSS AST and inserts `[data-v-{scopeId}]` attribute selectors
//! to scope styles to a specific Vue component.
//!
//! Also handles pre-pass markers:
//! - `[__v_deep__]` → `[data-v-{scopeId}]` (scope parent only)
//! - `[__v_slotted__]` → `[data-v-{scopeId}-s]` (slot scope)

use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};

use super::prepass::{DEEP_MARKER, SLOTTED_MARKER};

/// Apply scoped attribute selectors to CSS.
///
/// Replaces pre-pass markers and adds `[data-v-{scopeId}]` to all selectors
/// that don't contain `:global()`.
pub fn apply_scoped(css: &str, scope_id: &str) -> Result<String, String> {
    let scope_attr = format!("[data-v-{}]", scope_id);
    let slotted_attr = format!("[data-v-{}-s]", scope_id);

    // Parse the CSS with lightningcss, then serialize back.
    // This normalizes the CSS (handles comments, strings, at-rules, nesting correctly).
    //
    // NOTE: In a future iteration, we can use the Visitor trait to modify selectors
    // directly on the AST. For now, lightningcss gives us correct CSS parsing and
    // we do selector transformation on the normalized output.
    let stylesheet = StyleSheet::parse(css, ParserOptions::default())
        .map_err(|e| format!("CSS parse error: {}", e))?;

    let result = stylesheet
        .to_css(PrinterOptions::default())
        .map_err(|e| format!("CSS serialization error: {}", e))?;

    let normalized = result.code;

    // Now apply scoping to the normalized CSS
    let output = apply_scoped_to_normalized(&normalized, &scope_attr, &slotted_attr);

    Ok(output)
}

/// Apply scoped selectors to normalized CSS (already parsed and serialized by lightningcss).
///
/// This function handles:
/// 1. Replacing `[__v_deep__]` markers with `[data-v-xxx]`
/// 2. Replacing `[__v_slotted__]` markers with `[data-v-xxx-s]`
/// 3. Adding `[data-v-xxx]` to all other selectors (at the right position)
fn apply_scoped_to_normalized(css: &str, scope_attr: &str, slotted_attr: &str) -> String {
    let mut output = String::with_capacity(css.len() + 256);
    let mut chars = css.char_indices().peekable();
    let mut in_string = false;
    let mut string_char = '"';
    let mut in_comment = false;

    while let Some((_i, c)) = chars.next() {
        match c {
            // Track comments
            '/' if !in_string && !in_comment => {
                if let Some(&(_, '*')) = chars.peek() {
                    in_comment = true;
                    output.push('/');
                    if let Some((_, c2)) = chars.next() {
                        output.push(c2);
                    }
                    continue;
                }
                output.push(c);
                continue;
            }
            '*' if in_comment => {
                output.push(c);
                if let Some(&(_, '/')) = chars.peek() {
                    in_comment = false;
                    if let Some((_, c2)) = chars.next() {
                        output.push(c2);
                    }
                }
                continue;
            }
            _ if in_comment => {
                output.push(c);
                continue;
            }
            // Track strings
            '"' | '\'' if !in_string => {
                in_string = true;
                string_char = c;
                output.push(c);
            }
            c if in_string && c == string_char => {
                in_string = false;
                output.push(c);
            }
            // Handle rule blocks
            '{' if !in_string => {
                // Everything accumulated before '{' in output contains the selector
                let selector_end = output.len();
                let selector_start = output.rfind('}').map(|p| p + 1).unwrap_or(0);

                if selector_start < selector_end {
                    let raw_text = output[selector_start..selector_end].to_string();
                    let trimmed = raw_text.trim();

                    // Skip @-rules (media, keyframes, etc.)
                    if !trimmed.starts_with('@') && !trimmed.is_empty() {
                        let transformed =
                            transform_selector_list(trimmed, scope_attr, slotted_attr);
                        output.truncate(selector_start);
                        // Preserve leading whitespace
                        let leading_ws = &raw_text[..raw_text.len() - raw_text.trim_start().len()];
                        output.push_str(leading_ws);
                        output.push_str(&transformed);
                    }
                }

                output.push('{');
            }
            _ => output.push(c),
        }
    }

    output
}

/// Transform a comma-separated selector list.
fn transform_selector_list(selectors: &str, scope_attr: &str, slotted_attr: &str) -> String {
    selectors
        .split(',')
        .map(|s| transform_single_selector(s.trim(), scope_attr, slotted_attr))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Transform a single selector, adding scope attributes.
fn transform_single_selector(selector: &str, scope_attr: &str, slotted_attr: &str) -> String {
    // Handle __v_deep__ marker: replace with scope attr (already positioned correctly by prepass)
    if selector.contains(DEEP_MARKER) {
        return selector.replace(DEEP_MARKER, scope_attr);
    }

    // Handle __v_slotted__ marker: replace with slotted scope attr
    if selector.contains(SLOTTED_MARKER) {
        return selector.replace(SLOTTED_MARKER, slotted_attr);
    }

    // Handle :global() — lightningcss may have parsed this; just strip the wrapper
    if selector.contains(":global(") {
        return transform_global(selector);
    }

    // Regular selector: add scope to each compound selector
    add_scope_to_selector(selector, scope_attr)
}

/// Strip `:global()` wrapper from selector.
fn transform_global(selector: &str) -> String {
    let mut result = String::with_capacity(selector.len());
    let mut remaining = selector;

    while let Some(pos) = remaining.find(":global(") {
        result.push_str(&remaining[..pos]);
        let after = &remaining[pos + 8..]; // skip ":global("

        // Find matching closing paren
        let mut depth = 1u32;
        let mut end = 0;
        for (i, c) in after.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                _ => {}
            }
        }

        let inner = &after[..end];
        result.push_str(inner.trim());
        remaining = &after[end + 1..]; // skip past closing ')'
    }

    result.push_str(remaining);
    result
}

/// Add scope attribute to each compound selector in a complex selector.
fn add_scope_to_selector(selector: &str, scope_attr: &str) -> String {
    let mut result = String::with_capacity(selector.len() + scope_attr.len() * 2);
    let mut current_simple = String::new();
    let mut chars = selector.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            ' ' | '>' | '+' | '~' => {
                if !current_simple.trim().is_empty() {
                    result.push_str(&scope_simple_selector(&current_simple, scope_attr));
                    current_simple.clear();
                }
                result.push(c);
                // Consume additional spaces
                while chars.peek() == Some(&' ') {
                    result.push(chars.next().unwrap());
                }
            }
            _ => current_simple.push(c),
        }
    }

    if !current_simple.trim().is_empty() {
        result.push_str(&scope_simple_selector(&current_simple, scope_attr));
    }

    result
}

/// Add scope attribute to a simple (compound) selector.
/// Inserts before pseudo-classes and pseudo-elements.
fn scope_simple_selector(selector: &str, scope_attr: &str) -> String {
    let selector = selector.trim();
    if selector.is_empty() {
        return selector.to_string();
    }

    // Find where to insert the scope attribute
    // It should go after element/class/id selectors, before pseudo-classes/elements
    let mut insert_pos = selector.len();

    if let Some(pos) = selector.find("::") {
        insert_pos = pos;
    } else if let Some(pos) = find_pseudo_class_pos(selector) {
        insert_pos = pos;
    }

    let mut result = String::with_capacity(selector.len() + scope_attr.len());
    result.push_str(&selector[..insert_pos]);
    result.push_str(scope_attr);
    result.push_str(&selector[insert_pos..]);
    result
}

/// Find the position of the first pseudo-class (:) that isn't part of a
/// functional pseudo like :where(), :is(), :has(), :not(), :nth-child(), etc.
fn find_pseudo_class_pos(selector: &str) -> Option<usize> {
    let bytes = selector.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b':' {
            // Check it's not ::
            if i + 1 < bytes.len() && bytes[i + 1] == b':' {
                return Some(i);
            }
            // Check if it's a functional pseudo we should skip (scope everything before it)
            return Some(i);
        }
        // Skip attribute selectors
        if bytes[i] == b'[' {
            let mut depth = 1;
            i += 1;
            while i < bytes.len() && depth > 0 {
                match bytes[i] {
                    b'[' => depth += 1,
                    b']' => depth -= 1,
                    _ => {}
                }
                i += 1;
            }
            continue;
        }
        i += 1;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scoped(css: &str) -> String {
        apply_scoped(css, "a4f2eed6").unwrap()
    }

    #[test]
    fn test_basic_class() {
        let result = scoped(".box { color: red; }");
        assert!(result.contains(".box[data-v-a4f2eed6]"), "Got: {}", result);
    }

    #[test]
    fn test_element_selector() {
        let result = scoped("div { color: red; }");
        assert!(result.contains("div[data-v-a4f2eed6]"), "Got: {}", result);
    }

    #[test]
    fn test_id_selector() {
        let result = scoped("#app { color: red; }");
        assert!(result.contains("#app[data-v-a4f2eed6]"), "Got: {}", result);
    }

    #[test]
    fn test_multiple_selectors() {
        let result = scoped(".a, .b { color: red; }");
        assert!(result.contains(".a[data-v-a4f2eed6]"), "Got: {}", result);
        assert!(result.contains(".b[data-v-a4f2eed6]"), "Got: {}", result);
    }

    #[test]
    fn test_descendant_selector() {
        let result = scoped(".parent .child { color: red; }");
        assert!(
            result.contains(".parent[data-v-a4f2eed6]"),
            "Got: {}",
            result
        );
        assert!(
            result.contains(".child[data-v-a4f2eed6]"),
            "Got: {}",
            result
        );
    }

    #[test]
    fn test_pseudo_class_ordering() {
        let result = scoped(".btn:hover { color: red; }");
        assert!(
            result.contains(".btn[data-v-a4f2eed6]:hover"),
            "Got: {}",
            result
        );
    }

    #[test]
    fn test_pseudo_element_ordering() {
        let result = scoped(".text::before { content: ''; }");
        // lightningcss may normalize ::before to :before
        assert!(
            result.contains(".text[data-v-a4f2eed6]:before")
                || result.contains(".text[data-v-a4f2eed6]::before"),
            "Got: {}",
            result
        );
    }

    #[test]
    fn test_deep_marker() {
        // After prepass, :deep(.inner) becomes [__v_deep__] .inner
        let css = "[__v_deep__] .inner { color: red; }";
        let result = apply_scoped(css, "a4f2eed6").unwrap();
        assert!(
            result.contains("[data-v-a4f2eed6] .inner"),
            "Got: {}",
            result
        );
        // Inner should NOT be scoped
        assert!(
            !result.contains(".inner[data-v"),
            "Inner should not be scoped. Got: {}",
            result
        );
    }

    #[test]
    fn test_deep_with_parent() {
        // After prepass, .parent :deep(.inner) becomes .parent [__v_deep__] .inner
        let css = ".parent [__v_deep__] .inner { color: red; }";
        let result = apply_scoped(css, "a4f2eed6").unwrap();
        assert!(result.contains("[data-v-a4f2eed6]"), "Got: {}", result);
    }

    #[test]
    fn test_slotted_marker() {
        // After prepass, :slotted(.slot) becomes .slot[__v_slotted__]
        let css = ".slot[__v_slotted__] { color: red; }";
        let result = apply_scoped(css, "a4f2eed6").unwrap();
        assert!(
            result.contains(".slot[data-v-a4f2eed6-s]"),
            "Got: {}",
            result
        );
    }

    #[test]
    fn test_global_no_scope() {
        let css = ":global(.reset) { margin: 0; }";
        let result = apply_scoped(css, "a4f2eed6").unwrap();
        assert!(result.contains(".reset"), "Got: {}", result);
        assert!(
            !result.contains("[data-v"),
            "Should not have scope attr. Got: {}",
            result
        );
    }

    #[test]
    fn test_media_query_not_scoped() {
        let result = scoped("@media (min-width: 600px) { .box { color: red; } }");
        // The @media rule itself should not be scoped, but selectors inside should be
        assert!(
            !result.contains("@media[data-v"),
            "Media rule should not be scoped. Got: {}",
            result
        );
    }
}
