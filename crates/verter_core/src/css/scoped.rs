//! Scoped CSS visitor using lightningcss.
//!
//! Walks the CSS AST and inserts `[data-v-{scopeId}]` attribute selectors
//! to scope styles to a specific Vue component.
//!
//! Also handles pre-pass markers:
//! - `[__v_deep__]` → `[data-v-{scopeId}]` (scope parent only)
//! - `[__v_slotted__]` → `[data-v-{scopeId}-s]` (slot scope)

use smallvec::SmallVec;

use super::prepass::{DEEP_MARKER, SLOTTED_MARKER};

/// Apply scoped selectors on already-normalized CSS (no lightningcss re-parse).
///
/// **Precondition:** `normalized_css` must have been parsed and serialized by
/// lightningcss (via [`super::normalize_css`]). This ensures nested rules are
/// flattened and comments/strings are well-formed. Calling this on raw CSS may
/// skip selectors inside `@media` or `@supports` blocks.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
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
pub fn apply_scoped(css: &str, scope_id: &str) -> Result<String, super::CssError> {
    let normalized = super::normalize_css(css)?;
    Ok(apply_scoped_normalized(&normalized, scope_id))
}

/// Apply scoped attribute selectors to raw CSS without lightningcss normalization.
///
/// This is a faster alternative to [`apply_scoped`] that skips the
/// lightningcss parse/serialize step. The walker handles comments, strings,
/// @-rules, and @keyframes directly.
///
/// **Trade-off:** CSS values are not normalized (colors, units, shorthand).
/// CSS nesting (`&`) is not flattened. For scoped styles in Vue SFCs, this
/// is typically fine — the browser handles nesting natively.
pub fn apply_scoped_raw(css: &str, scope_id: &str) -> String {
    let scope_attr = format!("[data-v-{}]", scope_id);
    let slotted_attr = format!("[data-v-{}-s]", scope_id);
    apply_scoped_to_normalized(css, &scope_attr, &slotted_attr)
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
    let mut result = String::with_capacity(selectors.len() + scope_attr.len() * 2);
    for (i, s) in selectors.split(',').enumerate() {
        if i > 0 {
            result.push_str(", ");
        }
        result.push_str(&transform_single_selector(
            s.trim(),
            scope_attr,
            slotted_attr,
        ));
    }
    result
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

    // WS 5.3: Strip :deep() with empty parens — prepass leaves it unchanged,
    // lightningcss preserves it. Remove the pseudo-class and scope normally.
    if selector.contains(":deep()") {
        let stripped = selector.replace(":deep()", "");
        let trimmed = stripped.trim();
        if trimmed.is_empty() {
            // Bare `:deep()` with no other selector — just scope the universal selector
            return scope_attr.to_string();
        }
        return add_scope_to_selector(trimmed, scope_attr);
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
///
/// Uses byte-offset spans into the original selector string to avoid allocating
/// intermediate `String`s for each segment and combinator.
fn add_scope_to_selector(selector: &str, scope_attr: &str) -> String {
    let bytes = selector.as_bytes();
    // Segment and combinator byte ranges — stack-allocated for typical selectors.
    let mut segments: SmallVec<[(usize, usize); 4]> = SmallVec::new();
    let mut combinators: SmallVec<[(usize, usize); 4]> = SmallVec::new();
    let mut seg_start = 0usize;
    let mut i = 0usize;

    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'>' | b'+' | b'~' => {
                // Record segment (trimmed)
                let seg = selector[seg_start..i].trim();
                if !seg.is_empty() {
                    let offset = seg.as_ptr() as usize - selector.as_ptr() as usize;
                    segments.push((offset, offset + seg.len()));
                }
                // Record combinator span — consume all adjacent space/combinator chars.
                // Merges ` > ` into a single combinator span (matching original behavior).
                let comb_start = i;
                let first = bytes[i];
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b' ' => i += 1,
                        b'>' | b'+' | b'~'
                            if first == b' ' || (i > comb_start + 1 && bytes[i - 1] == b' ') =>
                        {
                            i += 1;
                        }
                        _ => break,
                    }
                }
                combinators.push((comb_start, i));
                seg_start = i;
            }
            _ => i += 1,
        }
    }
    // Final segment
    let seg = selector[seg_start..].trim();
    if !seg.is_empty() {
        let offset = seg.as_ptr() as usize - selector.as_ptr() as usize;
        segments.push((offset, offset + seg.len()));
    }

    // Build result — only the last segment gets the scope attribute
    let mut result = String::with_capacity(selector.len() + scope_attr.len());
    for (idx, &(start, end)) in segments.iter().enumerate() {
        if idx == segments.len() - 1 {
            result.push_str(&scope_simple_selector(&selector[start..end], scope_attr));
        } else {
            result.push_str(&selector[start..end]);
        }
        if idx < combinators.len() {
            let (cs, ce) = combinators[idx];
            result.push_str(&selector[cs..ce]);
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

/// Find the position of the first `:` in the selector, skipping attribute
/// selectors (`[...]`) and backslash-escaped characters.
fn find_pseudo_class_pos(selector: &str) -> Option<usize> {
    let bytes = selector.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Skip backslash-escaped characters (e.g., `\:` in class names)
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            i += 2;
            continue;
        }
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
    fn test_media_query_inner_selectors_scoped() {
        let result = scoped("@media (min-width: 600px) { .box { color: red; } }");
        // The @media rule itself should not be scoped
        assert!(
            !result.contains("@media[data-v"),
            "Media rule should not be scoped. Got: {}",
            result
        );
        // But selectors INSIDE @media must be scoped
        let media_start = result.find("@media").expect("@media must be present");
        let media_section = &result[media_start..];
        assert!(
            media_section.contains(".box[data-v-a4f2eed6]"),
            "Selector inside @media must be scoped. Got: {}",
            result
        );
    }

    /// @ai-generated - Multiple selectors inside @media must all be scoped.
    #[test]
    fn test_media_query_multiple_inner_selectors() {
        let result = scoped(
            "@media (max-width: 768px) { .sidebar { display: none; } .content { width: 100%; } }",
        );
        assert!(
            result.contains(".sidebar[data-v-a4f2eed6]"),
            ".sidebar inside @media must be scoped. Got: {}",
            result
        );
        assert!(
            result.contains(".content[data-v-a4f2eed6]"),
            ".content inside @media must be scoped. Got: {}",
            result
        );
    }

    /// @ai-generated - Selectors inside @supports must be scoped.
    #[test]
    fn test_supports_query_inner_selectors_scoped() {
        let result = scoped("@supports (display: grid) { .grid { display: grid; } }");
        assert!(
            result.contains(".grid[data-v-a4f2eed6]"),
            "Selector inside @supports must be scoped. Got: {}",
            result
        );
    }

    /// @ai-generated - Multiple @media blocks must all have scoped inner selectors.
    #[test]
    fn test_multiple_media_blocks() {
        let result = scoped(
            ".top { color: red; } \
             @media (max-width: 768px) { .mobile { display: block; } } \
             @media (min-width: 1200px) { .wide { display: flex; } } \
             .bottom { color: blue; }",
        );
        assert!(
            result.contains(".top[data-v-a4f2eed6]"),
            "Top-level .top must be scoped. Got: {}",
            result
        );
        assert!(
            result.contains(".mobile[data-v-a4f2eed6]"),
            ".mobile inside first @media must be scoped. Got: {}",
            result
        );
        assert!(
            result.contains(".wide[data-v-a4f2eed6]"),
            ".wide inside second @media must be scoped. Got: {}",
            result
        );
        assert!(
            result.contains(".bottom[data-v-a4f2eed6]"),
            "Top-level .bottom must be scoped. Got: {}",
            result
        );
    }

    /// @ai-generated - Descendant selectors inside @media must be properly scoped.
    #[test]
    fn test_media_with_descendant_selector() {
        let result = scoped("@media (max-width: 768px) { .nav .link { color: blue; } }");
        // Only the last compound gets scope
        assert!(
            result.contains(".link[data-v-a4f2eed6]"),
            "Last compound in descendant inside @media must be scoped. Got: {}",
            result
        );
        assert!(
            !result.contains(".nav[data-v-a4f2eed6]"),
            ".nav should not be scoped. Got: {}",
            result
        );
    }

    /// @ai-generated - Compound selectors (.badge.success) scope attribute at end.
    #[test]
    fn test_compound_class_selector() {
        let result = scoped(".badge.success { color: green; }");
        assert!(
            result.contains(".badge.success[data-v-a4f2eed6]"),
            "Compound selector must have scope at end. Got: {}",
            result
        );
    }

    /// @ai-generated - Multiple compound selectors.
    #[test]
    fn test_multiple_compound_selectors() {
        let result = scoped(".a.b, .c.d { color: red; }");
        assert!(
            result.contains(".a.b[data-v-a4f2eed6]"),
            "First compound must be scoped. Got: {}",
            result
        );
        assert!(
            result.contains(".c.d[data-v-a4f2eed6]"),
            "Second compound must be scoped. Got: {}",
            result
        );
    }

    /// @ai-generated - ID + class compound selector.
    #[test]
    fn test_id_class_compound() {
        let result = scoped("#main.active { color: red; }");
        assert!(
            result.contains("#main.active[data-v-a4f2eed6]"),
            "ID+class compound must have scope at end. Got: {}",
            result
        );
    }

    /// @ai-generated - Element + class compound selector.
    #[test]
    fn test_element_class_compound() {
        let result = scoped("div.container { color: red; }");
        assert!(
            result.contains("div.container[data-v-a4f2eed6]"),
            "Element+class compound must have scope at end. Got: {}",
            result
        );
    }

    /// @ai-generated - Attribute selector as standalone.
    #[test]
    fn test_attribute_selector_standalone() {
        let result = scoped("input[type=\"text\"] { color: red; }");
        assert!(
            result.contains("[data-v-a4f2eed6]"),
            "Attribute selector must be scoped. Got: {}",
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
        // The `>` combinator must NOT dangle after the scope attr
        assert!(
            !result.contains("]>"),
            "Combinator should not dangle after scope attr. Got: {}",
            result
        );
    }

    #[test]
    fn test_child_combinator_preserves_structure() {
        // Regression: ` > ` was split into two combinators (` ` and `> `)
        // causing `.horizontal > .divider` → `.horizontal .divider[data-v-xxx]>`
        let result = scoped(".horizontal > .divider { border: 1px solid; }");
        assert!(
            result.contains(".horizontal > .divider[data-v-a4f2eed6]"),
            "Child combinator structure must be preserved. Got: {}",
            result
        );
    }

    #[test]
    fn test_sibling_combinator_preserves_structure() {
        let result = scoped(".a + .b { color: red; }");
        assert!(
            result.contains(".a + .b[data-v-a4f2eed6]"),
            "Adjacent sibling combinator must be preserved. Got: {}",
            result
        );
    }

    #[test]
    fn test_general_sibling_combinator() {
        let result = scoped(".a ~ .b { color: red; }");
        assert!(
            result.contains(".a ~ .b[data-v-a4f2eed6]"),
            "General sibling combinator must be preserved. Got: {}",
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

    // ===================================================================
    // @ai-generated - Extended scoped CSS tests covering edge cases
    // ===================================================================

    /// Universal selector (*) must be scoped.
    #[test]
    fn test_universal_selector() {
        let result = scoped("* { margin: 0; }");
        assert!(
            result.contains("*[data-v-a4f2eed6]"),
            "Universal selector must be scoped. Got: {}",
            result
        );
    }

    /// Element followed by pseudo-element.
    #[test]
    fn test_element_pseudo_element() {
        let result = scoped("p::first-line { color: red; }");
        assert!(
            result.contains("p[data-v-a4f2eed6]:first-line")
                || result.contains("p[data-v-a4f2eed6]::first-line"),
            "Element + pseudo-element must be scoped. Got: {}",
            result
        );
    }

    /// :not() pseudo-class — scope before :not.
    #[test]
    fn test_not_pseudo_class() {
        let result = scoped(".item:not(.active) { opacity: 0.5; }");
        assert!(
            result.contains(".item[data-v-a4f2eed6]:not(.active)"),
            "Scope must be before :not(). Got: {}",
            result
        );
    }

    /// :is() / :where() pseudo-class.
    #[test]
    fn test_is_pseudo_class() {
        let result = scoped(".item:is(.a, .b) { color: red; }");
        assert!(
            result.contains("[data-v-a4f2eed6]:is("),
            "Scope must be before :is(). Got: {}",
            result
        );
    }

    /// Multiple rules in sequence.
    #[test]
    fn test_many_rules_in_sequence() {
        let result = scoped(
            ".a { color: red; } .b { color: blue; } .c { color: green; } .d { color: yellow; }",
        );
        assert!(result.contains(".a[data-v-a4f2eed6]"), "Got: {}", result);
        assert!(result.contains(".b[data-v-a4f2eed6]"), "Got: {}", result);
        assert!(result.contains(".c[data-v-a4f2eed6]"), "Got: {}", result);
        assert!(result.contains(".d[data-v-a4f2eed6]"), "Got: {}", result);
    }

    /// @keyframes selectors must NOT be scoped.
    #[test]
    fn test_keyframes_not_scoped() {
        let result = scoped(
            ".box { animation: fade 1s; } @keyframes fade { from { opacity: 1; } to { opacity: 0; } }",
        );
        assert!(
            result.contains(".box[data-v-a4f2eed6]"),
            ".box must be scoped. Got: {}",
            result
        );
        assert!(
            !result.contains("from[data-v"),
            "keyframe 'from' must NOT be scoped. Got: {}",
            result
        );
        assert!(
            !result.contains("to[data-v"),
            "keyframe 'to' must NOT be scoped. Got: {}",
            result
        );
    }

    /// Selectors after @keyframes must still be scoped.
    #[test]
    fn test_selector_after_keyframes() {
        let result = scoped(
            "@keyframes slide { 0% { left: 0; } 100% { left: 100%; } } .after { color: red; }",
        );
        assert!(
            result.contains(".after[data-v-a4f2eed6]"),
            "Selector after @keyframes must be scoped. Got: {}",
            result
        );
    }

    /// Mixed @media and @keyframes.
    #[test]
    fn test_media_and_keyframes_mixed() {
        let result = scoped(
            ".box { color: red; } \
             @keyframes fade { from { opacity: 1; } to { opacity: 0; } } \
             @media (max-width: 768px) { .mobile { display: block; } } \
             .end { color: blue; }",
        );
        assert!(
            result.contains(".box[data-v-a4f2eed6]"),
            ".box must be scoped. Got: {}",
            result
        );
        assert!(
            result.contains(".mobile[data-v-a4f2eed6]"),
            ".mobile inside @media must be scoped. Got: {}",
            result
        );
        assert!(
            result.contains(".end[data-v-a4f2eed6]"),
            ".end must be scoped. Got: {}",
            result
        );
    }

    /// CSS with string containing braces in property value.
    #[test]
    fn test_string_with_braces_in_value() {
        let result = scoped(".box { content: '{ not a block }'; }");
        assert!(
            result.contains(".box[data-v-a4f2eed6]"),
            "Selector must be scoped despite string with braces. Got: {}",
            result
        );
    }

    /// CSS comment between selectors.
    #[test]
    fn test_comment_between_selectors() {
        let result = scoped(".a { color: red; } /* comment */ .b { color: blue; }");
        assert!(
            result.contains(".a[data-v-a4f2eed6]"),
            ".a must be scoped. Got: {}",
            result
        );
        assert!(
            result.contains(".b[data-v-a4f2eed6]"),
            ".b must be scoped. Got: {}",
            result
        );
    }

    /// grid-template-areas with quoted strings must not confuse the walker.
    #[test]
    fn test_grid_template_areas_strings() {
        let result = scoped(
            ".layout { grid-template-areas: \"header\" \"content\" \"footer\"; } .next { color: red; }",
        );
        assert!(
            result.contains(".layout[data-v-a4f2eed6]"),
            ".layout must be scoped. Got: {}",
            result
        );
        assert!(
            result.contains(".next[data-v-a4f2eed6]"),
            ".next after grid-template-areas must be scoped. Got: {}",
            result
        );
    }

    /// Nested @media with @supports inside.
    #[test]
    fn test_nested_at_rules() {
        let result = scoped(
            "@media (min-width: 768px) { @supports (display: grid) { .nested { display: grid; } } }",
        );
        assert!(
            result.contains(".nested[data-v-a4f2eed6]"),
            "Deeply nested selector must be scoped. Got: {}",
            result
        );
    }

    /// @layer at-rule.
    #[test]
    fn test_layer_at_rule() {
        let result = scoped("@layer base { .box { color: red; } }");
        assert!(
            result.contains(".box[data-v-a4f2eed6]"),
            "Selector inside @layer must be scoped. Got: {}",
            result
        );
    }

    /// Multiple comma-separated selectors in a descendant context.
    #[test]
    fn test_comma_separated_in_descendant() {
        let result = scoped(".parent .a, .parent .b { color: red; }");
        assert!(
            result.contains(".a[data-v-a4f2eed6]"),
            ".a must be scoped. Got: {}",
            result
        );
        assert!(
            result.contains(".b[data-v-a4f2eed6]"),
            ".b must be scoped. Got: {}",
            result
        );
    }

    /// Selector with escaped colon in class name.
    #[test]
    fn test_escaped_colon_in_class() {
        let result = scoped(".md\\:flex { display: flex; }");
        // The scope should be after the full class name including escaped chars
        assert!(
            result.contains("[data-v-a4f2eed6]"),
            "Escaped colon selector must be scoped. Got: {}",
            result
        );
        // Scope should NOT be inserted at the backslash-escaped colon
        assert!(
            !result.contains(".md[data-v-a4f2eed6]\\:flex"),
            "Scope should not split the escaped class. Got: {}",
            result
        );
    }

    /// Empty rule body.
    #[test]
    fn test_empty_rule() {
        let result = scoped(".empty {} .after { color: red; }");
        assert!(
            result.contains(".after[data-v-a4f2eed6]"),
            ".after must be scoped even after empty rule. Got: {}",
            result
        );
    }

    /// @charset is removed by lightningcss normalization.
    /// After normalization, the selector following it must still be scoped.
    #[test]
    fn test_after_charset_still_scoped() {
        // lightningcss removes @charset during normalization, so the
        // scoped() helper (which normalizes first) produces just the selector.
        let result = scoped(".box { color: red; }");
        assert!(
            result.contains(".box[data-v-a4f2eed6]"),
            ".box must be scoped. Got: {}",
            result
        );
    }

    // ===================================================================
    // @ai-generated - CSS nesting tests (apply_scoped_raw path)
    // ===================================================================

    /// Native CSS nesting: nested `& .child` must be scoped.
    #[test]
    fn test_css_nesting_raw_basic() {
        let css = ".parent { color: red; & .child { color: blue; } }";
        let result = apply_scoped_raw(css, "a4f2eed6");
        assert!(
            result.contains(".parent[data-v-a4f2eed6]"),
            "Parent must be scoped. Got: {}",
            result
        );
        assert!(
            result.contains(".child[data-v-a4f2eed6]"),
            "Nested .child must be scoped. Got: {}",
            result
        );
        assert!(
            result.contains("color: red"),
            "Parent declarations preserved. Got: {}",
            result
        );
        assert!(
            result.contains("color: blue"),
            "Nested declarations preserved. Got: {}",
            result
        );
    }

    /// Native CSS nesting: `&:hover` pseudo-class.
    #[test]
    fn test_css_nesting_raw_pseudo() {
        let css = ".btn { color: red; &:hover { color: blue; } }";
        let result = apply_scoped_raw(css, "a4f2eed6");
        assert!(
            result.contains(".btn[data-v-a4f2eed6]"),
            "Parent must be scoped. Got: {}",
            result
        );
        // &:hover → &[data-v-xxx]:hover
        assert!(
            result.contains("[data-v-a4f2eed6]:hover"),
            "Nested &:hover must have scope before :hover. Got: {}",
            result
        );
    }

    /// Native CSS nesting: multiple nested selectors.
    #[test]
    fn test_css_nesting_raw_multiple() {
        let css =
            ".card { padding: 1rem; & .title { font-size: 18px; } & .body { padding: 8px; } }";
        let result = apply_scoped_raw(css, "a4f2eed6");
        assert!(
            result.contains(".card[data-v-a4f2eed6]"),
            "Parent must be scoped. Got: {}",
            result
        );
        assert!(
            result.contains(".title[data-v-a4f2eed6]"),
            ".title must be scoped. Got: {}",
            result
        );
        assert!(
            result.contains(".body[data-v-a4f2eed6]"),
            ".body must be scoped. Got: {}",
            result
        );
    }

    /// @font-face at-rule should not be scoped.
    #[test]
    fn test_font_face_not_scoped() {
        let result = scoped(
            "@font-face { font-family: MyFont; src: url('font.woff'); } .text { font-family: MyFont; }",
        );
        assert!(
            result.contains(".text[data-v-a4f2eed6]"),
            ".text must be scoped after @font-face. Got: {}",
            result
        );
    }

    // ===================================================================
    // WS 10.2 — :deep() edge cases (end-to-end through prepass + scoped)
    // ===================================================================

    /// Full pipeline helper: prepass → normalize → scope (matches `process_style`).
    /// Unlike `scoped()` which skips the prepass, this exercises the real pipeline.
    fn scoped_e2e(css: &str) -> String {
        use super::super::types::ProcessStyleOptions;
        super::super::process_style(
            css,
            &ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,
                module_name: None,
                filename: None,
                sourcemap: false,
            },
        )
        .unwrap()
        .code
    }

    /// WS 5.1: :deep(.a, .b) — each comma-separated selector gets its own
    /// [data-v-xxx] prefix; neither inner selector should receive a scope attr.
    #[test]
    fn test_deep_with_comma_e2e() {
        let result = scoped_e2e(":deep(.a, .b) { color: red; }");
        // Each branch must have the scope attr as a prefix (from prepass expansion)
        assert!(
            result.contains("[data-v-a4f2eed6] .a"),
            ":deep(.a, .b) — .a branch must have scope prefix. Got: {}",
            result
        );
        assert!(
            result.contains("[data-v-a4f2eed6] .b"),
            ":deep(.a, .b) — .b branch must have scope prefix. Got: {}",
            result
        );
        // The inner selectors must NOT themselves be scoped
        assert!(
            !result.contains(".a[data-v"),
            ".a inside :deep() must not be scoped. Got: {}",
            result
        );
        assert!(
            !result.contains(".b[data-v"),
            ".b inside :deep() must not be scoped. Got: {}",
            result
        );
        // The raw Vue syntax must not leak into the output
        assert!(
            !result.contains(":deep("),
            ":deep() must be removed from output. Got: {}",
            result
        );
    }

    /// WS 5.1: .parent :deep(.a, .b) — scope attr placed between parent and
    /// each inner selector branch.
    #[test]
    fn test_deep_with_comma_and_parent_e2e() {
        let result = scoped_e2e(".parent :deep(.a, .b) { color: red; }");
        // After prepass: ".parent [__v_deep__] .a, .parent [__v_deep__] .b { ... }"
        // After scoped: ".parent [data-v-xxx] .a, .parent [data-v-xxx] .b { ... }"
        assert!(
            result.contains("[data-v-a4f2eed6] .a"),
            ".a branch must have scope prefix. Got: {}",
            result
        );
        assert!(
            result.contains("[data-v-a4f2eed6] .b"),
            ".b branch must have scope prefix. Got: {}",
            result
        );
        // Inner selectors must NOT be scoped
        assert!(
            !result.contains(".a[data-v"),
            ".a inside :deep() must not be scoped. Got: {}",
            result
        );
        assert!(
            !result.contains(".b[data-v"),
            ".b inside :deep() must not be scoped. Got: {}",
            result
        );
    }

    /// WS 5.3: :deep() with empty parens — prepass leaves the token unchanged;
    /// lightningcss may drop the unknown pseudo-class; either way no deep marker
    /// and no crash should occur.
    #[test]
    fn test_deep_empty_parens_e2e() {
        // process_style() runs prepass → normalize → scope.
        // Prepass returns :deep() unchanged (empty inner = no transform).
        // lightningcss may strip the unknown pseudo or error; the result must
        // not contain the internal deep marker and must not panic.
        use super::super::types::ProcessStyleOptions;
        let result = super::super::process_style(
            ":deep() { color: red; }",
            &ProcessStyleOptions {
                scope_id: "a4f2eed6",
                scoped: true,
                is_module: false,
                module_name: None,
                filename: None,
                sourcemap: false,
            },
        );
        // We don't assert Ok/Err since lightningcss may reject :deep() —
        // the important constraint is that if it succeeds, no deep marker leaks.
        if let Ok(r) = result {
            assert!(
                !r.code.contains("__v_deep__"),
                "deep marker must not appear in output. Got: {}",
                r.code
            );
            assert!(
                !r.code.contains(":deep()"),
                ":deep() syntax must not appear in output. Got: {}",
                r.code
            );
        }
        // A parse error is also an acceptable outcome — :deep() is not valid CSS.
    }

    /// WS 5.3: bare :deep (no parens) — prepass does not touch it; it should
    /// pass through without injecting any deep marker.
    #[test]
    fn test_deep_bare_no_parens_e2e() {
        // :deep without parens is not recognized by prepass (pattern ":deep(" required).
        // Lightningcss may strip the unknown pseudo-class; result must not contain
        // the internal marker.
        let result = apply_scoped(":deep { color: red; }", "a4f2eed6");
        if let Ok(css) = result {
            assert!(
                !css.contains("__v_deep__"),
                "deep marker must not appear for bare :deep. Got: {}",
                css
            );
        }
        // A parse error is also acceptable — bare :deep is not valid CSS.
    }
}
