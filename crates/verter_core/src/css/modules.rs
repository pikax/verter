//! CSS Modules visitor using lightningcss.
//!
//! Walks the CSS AST and hashes class names for CSS module isolation.
//! Returns the original → hashed class name mapping for runtime use.

use sha2::{Digest, Sha256};
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

/// Generate a content-based hash for a CSS module class name.
///
/// Uses SHA-256 of `component_id + class_name`, truncated to 8 hex chars.
/// Deterministic across builds (unlike counter-based), matching Vue's approach.
fn content_hash(component_id: &str, class_name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(component_id.as_bytes());
    hasher.update(class_name.as_bytes());
    let result = hasher.finalize();
    // Take first 4 bytes → 8 hex chars for compact, collision-resistant hash
    format!(
        "{:02x}{:02x}{:02x}{:02x}",
        result[0], result[1], result[2], result[3]
    )
}

struct CssModulesTransformer {
    component_id: String,
    class_mapping: HashMap<String, String>,
}

impl CssModulesTransformer {
    fn new(component_id: &str) -> Self {
        Self {
            component_id: component_id.to_string(),
            class_mapping: HashMap::new(),
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
                        class_name.push(chars.next().expect("peek() succeeded"));
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
        let component_id = &self.component_id;
        self.class_mapping
            .entry(class_name.to_string())
            .or_insert_with(|| {
                let hash = content_hash(component_id, class_name);
                format!("{}_{}", class_name, hash)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: verify a class mapping exists with format `{name}_{8-hex-chars}`.
    fn assert_hashed_class(mapping: &[(String, String)], original: &str) {
        let entry = mapping.iter().find(|(k, _)| k == original);
        assert!(
            entry.is_some(),
            "Expected mapping for class '{}', got: {:?}",
            original,
            mapping
        );
        let hashed = &entry.unwrap().1;
        assert!(
            hashed.starts_with(&format!("{}_", original)),
            "Hashed class should start with '{}_', got: {}",
            original,
            hashed
        );
        let suffix = &hashed[original.len() + 1..];
        assert_eq!(
            suffix.len(),
            8,
            "Hash suffix should be 8 hex chars, got: '{}' (len={})",
            suffix,
            suffix.len()
        );
        assert!(
            suffix.chars().all(|c| c.is_ascii_hexdigit()),
            "Hash suffix should be hex, got: '{}'",
            suffix
        );
    }

    #[test]
    fn test_basic_class_hashing() {
        let (css, mapping) = apply_css_modules(".btn { color: red; }", "a4f2eed6").unwrap();
        assert_eq!(mapping.len(), 1);
        assert_hashed_class(&mapping, "btn");
        // CSS output should use the hashed class name
        assert!(css.contains(&format!(".{}", mapping[0].1)), "Got: {}", css);
        // Original class should NOT appear
        assert!(
            !css.contains(".btn{") && !css.contains(".btn "),
            "Raw class leaked: {}",
            css
        );
    }

    #[test]
    fn test_content_hash_is_deterministic() {
        // Same input always produces same hash
        let (_, m1) = apply_css_modules(".btn { }", "a4f2eed6").unwrap();
        let (_, m2) = apply_css_modules(".btn { }", "a4f2eed6").unwrap();
        assert_eq!(m1[0].1, m2[0].1, "Hash should be deterministic");
    }

    #[test]
    fn test_different_component_id_different_hash() {
        let (_, m1) = apply_css_modules(".btn { }", "aaa").unwrap();
        let (_, m2) = apply_css_modules(".btn { }", "bbb").unwrap();
        assert_ne!(
            m1[0].1, m2[0].1,
            "Different component IDs should produce different hashes"
        );
    }

    #[test]
    fn test_multiple_classes() {
        let (css, mapping) = apply_css_modules(".btn { } .card { }", "a4f2eed6").unwrap();
        assert_eq!(mapping.len(), 2);
        assert_hashed_class(&mapping, "btn");
        assert_hashed_class(&mapping, "card");
        assert!(
            css.contains(&format!(
                ".{}",
                mapping.iter().find(|(k, _)| k == "btn").unwrap().1
            )),
            "Got: {}",
            css
        );
        assert!(
            css.contains(&format!(
                ".{}",
                mapping.iter().find(|(k, _)| k == "card").unwrap().1
            )),
            "Got: {}",
            css
        );
    }

    #[test]
    fn test_same_class_reused() {
        let (css, mapping) = apply_css_modules(".btn { } .btn:hover { }", "a4f2eed6").unwrap();
        assert_eq!(mapping.len(), 1);
        // Same class should get same hash, appears twice
        let hash = &mapping[0].1;
        let count = css.matches(hash.as_str()).count();
        assert_eq!(count, 2, "Got: {}", css);
    }

    #[test]
    fn test_chained_classes() {
        let (css, mapping) = apply_css_modules(".a.b { }", "a4f2eed6").unwrap();
        assert_eq!(mapping.len(), 2);
        assert_hashed_class(&mapping, "a");
        assert_hashed_class(&mapping, "b");
        let ha = &mapping.iter().find(|(k, _)| k == "a").unwrap().1;
        let hb = &mapping.iter().find(|(k, _)| k == "b").unwrap().1;
        assert!(css.contains(&format!(".{}.{}", ha, hb)), "Got: {}", css);
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
        assert_eq!(mapping.len(), 2);
        assert_hashed_class(&mapping, "a");
        assert_hashed_class(&mapping, "b");
        let ha = &mapping.iter().find(|(k, _)| k == "a").unwrap().1;
        let hb = &mapping.iter().find(|(k, _)| k == "b").unwrap().1;
        assert!(css.contains(&format!(".{}", ha)), "Got: {}", css);
        assert!(css.contains(&format!(".{}", hb)), "Got: {}", css);
    }

    // ===================================================================
    // @ai-generated - CSS modules inside @-rule blocks
    // ===================================================================

    /// Classes inside @media must be hashed.
    #[test]
    fn test_modules_inside_media() {
        let (_, mapping) = apply_css_modules(
            "@media (max-width: 768px) { .mobile { display: block; } }",
            "a4f2eed6",
        )
        .unwrap();
        assert_eq!(mapping.len(), 1);
        assert_hashed_class(&mapping, "mobile");
    }

    /// Multiple classes inside @media.
    #[test]
    fn test_modules_multiple_inside_media() {
        let (_, mapping) = apply_css_modules(
            "@media (max-width: 768px) { .sidebar { display: none; } .content { width: 100%; } }",
            "a4f2eed6",
        )
        .unwrap();
        assert_eq!(mapping.len(), 2);
        assert_hashed_class(&mapping, "sidebar");
        assert_hashed_class(&mapping, "content");
    }

    /// Classes both inside and outside @media.
    #[test]
    fn test_modules_mixed_media() {
        let (_, mapping) = apply_css_modules(
            ".top { color: red; } @media (min-width: 1200px) { .wide { display: flex; } } .bottom { color: blue; }",
            "a4f2eed6",
        )
        .unwrap();
        assert_eq!(mapping.len(), 3);
        assert_hashed_class(&mapping, "top");
        assert_hashed_class(&mapping, "wide");
        assert_hashed_class(&mapping, "bottom");
    }

    /// Classes inside @supports must be hashed.
    #[test]
    fn test_modules_inside_supports() {
        let (_, mapping) = apply_css_modules(
            "@supports (display: grid) { .grid-item { grid-column: span 2; } }",
            "a4f2eed6",
        )
        .unwrap();
        assert_eq!(mapping.len(), 1);
        assert_hashed_class(&mapping, "grid-item");
    }

    /// Classes inside nested @media > @supports.
    #[test]
    fn test_modules_nested_at_rules() {
        let (_, mapping) = apply_css_modules(
            "@media (min-width: 768px) { @supports (display: grid) { .nested { display: grid; } } }",
            "a4f2eed6",
        )
        .unwrap();
        assert_eq!(mapping.len(), 1);
        assert_hashed_class(&mapping, "nested");
    }

    /// Same class inside and outside @media gets same hash.
    #[test]
    fn test_modules_same_class_in_media_and_top() {
        let (css, mapping) = apply_css_modules(
            ".btn { color: red; } @media (max-width: 768px) { .btn { color: blue; } }",
            "a4f2eed6",
        )
        .unwrap();
        assert_eq!(mapping.len(), 1);
        // Same class → same hash, appears twice
        let hash = &mapping[0].1;
        let count = css.matches(hash.as_str()).count();
        assert_eq!(
            count, 2,
            "Same class should appear twice with same hash. Got: {}",
            css
        );
    }

    /// @keyframes selectors should NOT have class hashing applied.
    #[test]
    fn test_modules_keyframes_not_hashed() {
        let (css, mapping) = apply_css_modules(
            ".box { animation: fade 1s; } @keyframes fade { from { opacity: 1; } to { opacity: 0; } }",
            "a4f2eed6",
        )
        .unwrap();
        assert_eq!(mapping.len(), 1);
        assert_hashed_class(&mapping, "box");
        // keyframe selectors (from, to) should not be treated as class selectors
        assert!(
            !css.contains("from_"),
            "from must not be hashed. Got: {}",
            css
        );
        assert!(!css.contains("to_"), "to must not be hashed. Got: {}", css);
    }
}
