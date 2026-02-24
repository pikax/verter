//! CSS Modules visitor using lightningcss.
//!
//! Walks the CSS AST and hashes class names for CSS module isolation.
//! Returns the original → hashed class name mapping for runtime use.

use std::collections::HashMap;

/// Apply CSS modules on already-normalized CSS (no lightningcss re-parse).
///
/// **Precondition:** `normalized_css` must have been parsed and serialized by
/// lightningcss (via [`super::normalize_css`]). This ensures nested rules are
/// flattened and comments/strings are well-formed. Calling this on raw CSS may
/// skip class selectors inside `@media` or `@supports` blocks.
pub fn apply_css_modules_normalized(
    normalized_css: &str,
    component_id: &str,
) -> (String, Vec<(String, String)>) {
    let mut transformer = CssModulesTransformer::new(component_id);
    let output = transformer.transform(normalized_css);

    let mut mapping: Vec<(String, String)> = transformer.class_mapping.into_iter().collect();
    mapping.sort_by(|a, b| a.0.cmp(&b.0));

    (output, mapping)
}

/// Apply CSS modules transformation: hash class names and return mappings.
///
/// Standalone entry point that normalizes CSS internally.
pub fn apply_css_modules(
    css: &str,
    component_id: &str,
) -> Result<(String, Vec<(String, String)>), super::CssError> {
    let normalized = super::normalize_css(css)?;
    Ok(apply_css_modules_normalized(&normalized, component_id))
}

struct CssModulesTransformer {
    component_id: String,
    class_mapping: HashMap<String, String>,
    class_counter: usize,
}

impl CssModulesTransformer {
    fn new(component_id: &str) -> Self {
        Self {
            component_id: component_id.to_string(),
            class_mapping: HashMap::new(),
            class_counter: 0,
        }
    }

    fn transform(&mut self, css: &str) -> String {
        super::walk::walk_and_transform_selectors(css, |selectors| {
            self.transform_selector_list(selectors)
        })
    }

    fn transform_selector_list(&mut self, selectors: &str) -> String {
        selectors
            .split(',')
            .map(|s| self.transform_selector(s.trim()))
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn transform_selector(&mut self, selector: &str) -> String {
        let mut result = String::with_capacity(selector.len() + 32);
        let mut chars = selector.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '.' {
                // Extract class name
                let mut class_name = String::new();
                while let Some(&next_c) = chars.peek() {
                    if next_c.is_alphanumeric() || next_c == '-' || next_c == '_' {
                        // SAFETY: .next() is guaranteed Some after successful .peek()
                        class_name.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }

                if !class_name.is_empty() {
                    result.push('.');
                    result.push_str(self.get_or_create_hash(&class_name));
                } else {
                    result.push('.');
                }
            } else {
                result.push(c);
            }
        }

        result
    }

    fn get_or_create_hash(&mut self, class_name: &str) -> &str {
        let counter = &mut self.class_counter;
        let component_id = &self.component_id;
        self.class_mapping
            .entry(class_name.to_string())
            .or_insert_with(|| {
                let hashed = format!("{}_{}_{}", class_name, component_id, counter);
                *counter += 1;
                hashed
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_class_hashing() {
        let (css, mapping) = apply_css_modules(".btn { color: red; }", "a4f2eed6").unwrap();
        assert!(css.contains(".btn_a4f2eed6_0"), "Got: {}", css);
        assert_eq!(mapping.len(), 1);
        assert_eq!(mapping[0].0, "btn");
        assert_eq!(mapping[0].1, "btn_a4f2eed6_0");
    }

    #[test]
    fn test_multiple_classes() {
        let (css, mapping) = apply_css_modules(".btn { } .card { }", "a4f2eed6").unwrap();
        assert!(css.contains(".btn_a4f2eed6_"), "Got: {}", css);
        assert!(css.contains(".card_a4f2eed6_"), "Got: {}", css);
        assert_eq!(mapping.len(), 2);
    }

    #[test]
    fn test_same_class_reused() {
        let (css, mapping) = apply_css_modules(".btn { } .btn:hover { }", "a4f2eed6").unwrap();
        // Same class should get same hash
        let btn_count = css.matches(".btn_a4f2eed6_0").count();
        assert_eq!(btn_count, 2, "Got: {}", css);
        assert_eq!(mapping.len(), 1);
    }

    #[test]
    fn test_chained_classes() {
        let (css, mapping) = apply_css_modules(".a.b { }", "a4f2eed6").unwrap();
        assert!(css.contains(".a_a4f2eed6_"), "Got: {}", css);
        assert!(css.contains(".b_a4f2eed6_"), "Got: {}", css);
        assert_eq!(mapping.len(), 2);
    }

    #[test]
    fn test_element_not_hashed() {
        let (css, _) = apply_css_modules("div { }", "a4f2eed6").unwrap();
        assert!(
            css.contains("div"),
            "Element should not be hashed. Got: {}",
            css
        );
        assert!(!css.contains("._div"), "Got: {}", css);
    }

    #[test]
    fn test_id_not_hashed() {
        let (css, _) = apply_css_modules("#app { }", "a4f2eed6").unwrap();
        assert!(
            css.contains("#app"),
            "ID should not be hashed. Got: {}",
            css
        );
    }

    #[test]
    fn test_selector_list() {
        let (css, mapping) = apply_css_modules(".a, .b { }", "a4f2eed6").unwrap();
        assert!(css.contains(".a_a4f2eed6_"), "Got: {}", css);
        assert!(css.contains(".b_a4f2eed6_"), "Got: {}", css);
        assert_eq!(mapping.len(), 2);
    }

    // ===================================================================
    // @ai-generated - CSS modules inside @-rule blocks
    // ===================================================================

    /// Classes inside @media must be hashed.
    #[test]
    fn test_modules_inside_media() {
        let (css, mapping) = apply_css_modules(
            "@media (max-width: 768px) { .mobile { display: block; } }",
            "a4f2eed6",
        )
        .unwrap();
        assert!(
            css.contains(".mobile_a4f2eed6_"),
            "Class inside @media must be hashed. Got: {}",
            css
        );
        assert_eq!(mapping.len(), 1);
        assert_eq!(mapping[0].0, "mobile");
    }

    /// Multiple classes inside @media.
    #[test]
    fn test_modules_multiple_inside_media() {
        let (css, mapping) = apply_css_modules(
            "@media (max-width: 768px) { .sidebar { display: none; } .content { width: 100%; } }",
            "a4f2eed6",
        )
        .unwrap();
        assert!(
            css.contains(".sidebar_a4f2eed6_"),
            "sidebar must be hashed. Got: {}",
            css
        );
        assert!(
            css.contains(".content_a4f2eed6_"),
            "content must be hashed. Got: {}",
            css
        );
        assert_eq!(mapping.len(), 2);
    }

    /// Classes both inside and outside @media.
    #[test]
    fn test_modules_mixed_media() {
        let (css, mapping) = apply_css_modules(
            ".top { color: red; } @media (min-width: 1200px) { .wide { display: flex; } } .bottom { color: blue; }",
            "a4f2eed6",
        )
        .unwrap();
        assert!(css.contains(".top_a4f2eed6_"), "Got: {}", css);
        assert!(css.contains(".wide_a4f2eed6_"), "Got: {}", css);
        assert!(css.contains(".bottom_a4f2eed6_"), "Got: {}", css);
        assert_eq!(mapping.len(), 3);
    }

    /// Classes inside @supports must be hashed.
    #[test]
    fn test_modules_inside_supports() {
        let (css, mapping) = apply_css_modules(
            "@supports (display: grid) { .grid-item { grid-column: span 2; } }",
            "a4f2eed6",
        )
        .unwrap();
        assert!(
            css.contains(".grid-item_a4f2eed6_"),
            "Class inside @supports must be hashed. Got: {}",
            css
        );
        assert_eq!(mapping.len(), 1);
    }

    /// Classes inside nested @media > @supports.
    #[test]
    fn test_modules_nested_at_rules() {
        let (css, mapping) = apply_css_modules(
            "@media (min-width: 768px) { @supports (display: grid) { .nested { display: grid; } } }",
            "a4f2eed6",
        )
        .unwrap();
        assert!(
            css.contains(".nested_a4f2eed6_"),
            "Deeply nested class must be hashed. Got: {}",
            css
        );
        assert_eq!(mapping.len(), 1);
    }

    /// Same class inside and outside @media gets same hash.
    #[test]
    fn test_modules_same_class_in_media_and_top() {
        let (css, mapping) = apply_css_modules(
            ".btn { color: red; } @media (max-width: 768px) { .btn { color: blue; } }",
            "a4f2eed6",
        )
        .unwrap();
        // Same class → same hash, appears twice
        let hash = &mapping[0].1;
        let count = css.matches(hash.as_str()).count();
        assert_eq!(
            count, 2,
            "Same class should appear twice with same hash. Got: {}",
            css
        );
        assert_eq!(mapping.len(), 1, "Only one unique class mapping");
    }

    /// @keyframes selectors should NOT have class hashing applied.
    #[test]
    fn test_modules_keyframes_not_hashed() {
        let (css, mapping) = apply_css_modules(
            ".box { animation: fade 1s; } @keyframes fade { from { opacity: 1; } to { opacity: 0; } }",
            "a4f2eed6",
        )
        .unwrap();
        assert!(
            css.contains(".box_a4f2eed6_"),
            ".box must be hashed. Got: {}",
            css
        );
        // keyframe selectors (from, to) should not be treated as class selectors
        assert!(
            !css.contains("from_a4f2eed6"),
            "from must not be hashed. Got: {}",
            css
        );
        assert!(
            !css.contains("to_a4f2eed6"),
            "to must not be hashed. Got: {}",
            css
        );
        assert_eq!(mapping.len(), 1);
    }
}
