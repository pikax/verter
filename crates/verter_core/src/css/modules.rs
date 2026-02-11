//! CSS Modules visitor using lightningcss.
//!
//! Walks the CSS AST and hashes class names for CSS module isolation.
//! Returns the original → hashed class name mapping for runtime use.

use lightningcss::stylesheet::{ParserOptions, PrinterOptions, StyleSheet};
use std::collections::HashMap;

/// Apply CSS modules transformation: hash class names and return mappings.
pub fn apply_css_modules(
    css: &str,
    component_id: &str,
) -> Result<(String, Vec<(String, String)>), String> {
    // Parse with lightningcss, then serialize back to normalize the CSS
    let stylesheet = StyleSheet::parse(css, ParserOptions::default())
        .map_err(|e| format!("CSS parse error: {}", e))?;

    let result = stylesheet
        .to_css(PrinterOptions::default())
        .map_err(|e| format!("CSS serialization error: {}", e))?;

    let normalized = result.code;

    // Apply class hashing on normalized CSS
    let mut transformer = CssModulesTransformer::new(component_id);
    let output = transformer.transform(&normalized);

    let mut mapping: Vec<(String, String)> = transformer.class_mapping.into_iter().collect();
    mapping.sort_by(|a, b| a.0.cmp(&b.0));

    Ok((output, mapping))
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
                // Handle selector areas before {
                '{' if !in_string => {
                    let selector_end = output.len();
                    let selector_start = output.rfind('}').map(|p| p + 1).unwrap_or(0);

                    if selector_start < selector_end {
                        let raw_text = output[selector_start..selector_end].to_string();
                        let trimmed = raw_text.trim();

                        if !trimmed.starts_with('@') && !trimmed.is_empty() {
                            let transformed = self.transform_selector_list(trimmed);
                            output.truncate(selector_start);
                            let leading_ws =
                                &raw_text[..raw_text.len() - raw_text.trim_start().len()];
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
                    let hashed = self.get_or_create_hash(&class_name);
                    result.push('.');
                    result.push_str(&hashed);
                } else {
                    result.push('.');
                }
            } else {
                result.push(c);
            }
        }

        result
    }

    fn get_or_create_hash(&mut self, class_name: &str) -> String {
        if let Some(hashed) = self.class_mapping.get(class_name) {
            return hashed.clone();
        }

        let hashed = format!(
            "_{}_{}{}",
            class_name, self.component_id, self.class_counter
        );
        self.class_counter += 1;

        self.class_mapping
            .insert(class_name.to_string(), hashed.clone());
        hashed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_class_hashing() {
        let (css, mapping) = apply_css_modules(".btn { color: red; }", "a4f2eed6").unwrap();
        assert!(css.contains("._btn_a4f2eed60"), "Got: {}", css);
        assert_eq!(mapping.len(), 1);
        assert_eq!(mapping[0].0, "btn");
        assert_eq!(mapping[0].1, "_btn_a4f2eed60");
    }

    #[test]
    fn test_multiple_classes() {
        let (css, mapping) = apply_css_modules(".btn { } .card { }", "a4f2eed6").unwrap();
        assert!(css.contains("._btn_a4f2eed6"), "Got: {}", css);
        assert!(css.contains("._card_a4f2eed6"), "Got: {}", css);
        assert_eq!(mapping.len(), 2);
    }

    #[test]
    fn test_same_class_reused() {
        let (css, mapping) = apply_css_modules(".btn { } .btn:hover { }", "a4f2eed6").unwrap();
        // Same class should get same hash
        let btn_count = css.matches("._btn_a4f2eed60").count();
        assert_eq!(btn_count, 2, "Got: {}", css);
        assert_eq!(mapping.len(), 1);
    }

    #[test]
    fn test_chained_classes() {
        let (css, mapping) = apply_css_modules(".a.b { }", "a4f2eed6").unwrap();
        assert!(css.contains("._a_a4f2eed6"), "Got: {}", css);
        assert!(css.contains("._b_a4f2eed6"), "Got: {}", css);
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
        assert!(css.contains("._a_a4f2eed6"), "Got: {}", css);
        assert!(css.contains("._b_a4f2eed6"), "Got: {}", css);
        assert_eq!(mapping.len(), 2);
    }
}
