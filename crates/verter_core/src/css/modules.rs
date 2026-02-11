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
) -> Result<(String, Vec<(String, String)>), String> {
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
}
