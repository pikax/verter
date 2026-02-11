//! Scoped CSS visitor using lightningcss.
//!
//! Walks the CSS AST and inserts `[data-v-{scopeId}]` attribute selectors
//! to scope styles to a specific Vue component.
//!
//! Also handles pre-pass markers:
//! - `[__v_deep__]` → `[data-v-{scopeId}]` (scope parent only)
//! - `[__v_slotted__]` → `[data-v-{scopeId}-s]` (slot scope)

use super::prepass::{DEEP_MARKER, SLOTTED_MARKER};

/// Apply scoped selectors on already-normalized CSS (no lightningcss re-parse).
///
/// **Precondition:** `normalized_css` must have been parsed and serialized by
/// lightningcss (via [`super::normalize_css`]). This ensures nested rules are
/// flattened and comments/strings are well-formed. Calling this on raw CSS may
/// skip selectors inside `@media` or `@supports` blocks.
pub fn apply_scoped_normalized(normalized_css: &str, scope_id: &str) -> String {
    let scope_attr = format!("[data-v-{}]", scope_id);
    let slotted_attr = format!("[data-v-{}-s]", scope_id);
    apply_scoped_to_normalized(normalized_css, &scope_attr, &slotted_attr)
}

/// Apply scoped attribute selectors to CSS.
///
/// Standalone entry point that normalizes CSS internally.
/// Replaces pre-pass markers and adds `[data-v-{scopeId}]` to all selectors
/// that don't contain `:global()`.
pub fn apply_scoped(css: &str, scope_id: &str) -> Result<String, String> {
    let normalized = super::normalize_css(css)?;
    Ok(apply_scoped_normalized(&normalized, scope_id))
}

/// Apply scoped selectors to normalized CSS (already parsed and serialized by lightningcss).
///
/// This function handles:
/// 1. Replacing `[__v_deep__]` markers with `[data-v-xxx]`
/// 2. Replacing `[__v_slotted__]` markers with `[data-v-xxx-s]`
/// 3. Adding `[data-v-xxx]` to all other selectors (at the right position)
fn apply_scoped_to_normalized(css: &str, scope_attr: &str, slotted_attr: &str) -> String {
    super::walk::walk_and_transform_selectors(css, |selectors| {
        transform_selector_list(selectors, scope_attr, slotted_attr)
    })
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

/// Add scope attribute to the last compound selector in a complex selector.
///
/// Matches Vue's official compiler behavior: only the rightmost compound selector
/// receives the scope attribute (e.g., `.parent .child` → `.parent .child[data-v-xxx]`).
fn add_scope_to_selector(selector: &str, scope_attr: &str) -> String {
    let mut segments: Vec<String> = Vec::new();
    let mut combinators: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut chars = selector.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            ' ' | '>' | '+' | '~' => {
                if !current.trim().is_empty() {
                    segments.push(current.clone());
                    current.clear();
                }
                let mut comb = String::new();
                comb.push(c);
                // Consume additional spaces
                while chars.peek() == Some(&' ') {
                    comb.push(chars.next().unwrap());
                }
                combinators.push(comb);
            }
            _ => current.push(c),
        }
    }
    if !current.trim().is_empty() {
        segments.push(current);
    }

    // Build result: only the last segment gets the scope attribute
    let mut result = String::with_capacity(selector.len() + scope_attr.len());
    for (i, seg) in segments.iter().enumerate() {
        if i == segments.len() - 1 {
            result.push_str(&scope_simple_selector(seg, scope_attr));
        } else {
            result.push_str(seg);
        }
        if i < combinators.len() {
            result.push_str(&combinators[i]);
        }
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

    if let Some(pos) = find_pseudo_class_pos(selector) {
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
        // Vue behavior: only the last compound selector gets the scope attribute
        let result = scoped(".parent .child { color: red; }");
        assert!(
            !result.contains(".parent[data-v-a4f2eed6]"),
            "Parent should not be scoped. Got: {}",
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

    #[test]
    fn test_attribute_selector_with_pseudo() {
        // [attr]:hover — scope should be inserted before :hover, after [attr]
        let result = scoped("a[href]:hover { color: red; }");
        assert!(
            result.contains("[data-v-a4f2eed6]:hover"),
            "Scope attr should be before :hover. Got: {}",
            result
        );
    }

    #[test]
    fn test_child_combinator() {
        // Vue behavior: only the last compound selector gets the scope attribute
        let result = scoped(".parent > .child { color: red; }");
        assert!(
            !result.contains(".parent[data-v-a4f2eed6]"),
            "Parent should not be scoped. Got: {}",
            result
        );
        assert!(
            result.contains(".child[data-v-a4f2eed6]"),
            "Got: {}",
            result
        );
    }

    #[test]
    fn test_pseudo_class_and_pseudo_element() {
        let result = scoped(".btn:hover::before { content: ''; }");
        assert!(
            result.contains(".btn[data-v-a4f2eed6]:hover:before")
                || result.contains(".btn[data-v-a4f2eed6]:hover::before"),
            "Scope should be before :hover. Got: {}",
            result
        );
    }

    #[test]
    fn test_nth_child_and_pseudo_element() {
        let result = scoped(".item:nth-child(2)::after { content: ''; }");
        assert!(
            result.contains(".item[data-v-a4f2eed6]:nth-child(2):after")
                || result.contains(".item[data-v-a4f2eed6]:nth-child(2)::after"),
            "Scope should be before :nth-child. Got: {}",
            result
        );
    }

    #[test]
    fn test_three_level_descendant() {
        // Only the last segment should be scoped
        let result = scoped(".a .b .c { color: red; }");
        assert!(
            !result.contains(".a[data-v-a4f2eed6]"),
            "First should not be scoped. Got: {}",
            result
        );
        assert!(
            !result.contains(".b[data-v-a4f2eed6]"),
            "Middle should not be scoped. Got: {}",
            result
        );
        assert!(
            result.contains(".c[data-v-a4f2eed6]"),
            "Last should be scoped. Got: {}",
            result
        );
    }
}
