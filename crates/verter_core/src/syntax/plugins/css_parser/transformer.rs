//! CSS transformation for Vue scoped styles
//!
//! Transforms CSS selectors to add scoping attributes:
//! - `.class` → `.class[data-v-{id}]`
//! - `:deep(.inner)` → `[data-v-{id}] .inner`
//! - `:slotted(.slot)` → `.slot[data-v-{id}-s]`
//! - `:global(.global)` → `.global` (no transformation)
//!
//! Also extracts v-bind() expressions:
//! - `color: v-bind(color)` → `color: var(--{id}-color)`

use crate::common::Span;
use crate::syntax::types::CssVBindExpression;

/// Result of CSS transformation
#[derive(Debug)]
pub struct TransformResult {
    /// Transformed CSS bytes
    pub css: Vec<u8>,
    /// Extracted v-bind() expressions
    pub v_bind_expressions: Vec<CssVBindExpression>,
}

/// Transform CSS for scoped styles
///
/// # Arguments
/// * `css` - Raw CSS bytes
/// * `scope_id` - 8-character scope ID (e.g., b"a4f2eed6")
/// * `content_offset` - Offset of CSS content in the original source
///
/// # Returns
/// TransformResult with transformed CSS and extracted v-bind expressions
pub fn transform_scoped_css(
    css: &[u8],
    scope_id: &[u8; 8],
    content_offset: u32,
) -> Result<TransformResult, String> {
    let css_str: &str =
        std::str::from_utf8(css).map_err(|e| format!("Invalid UTF-8 in CSS: {}", e))?;

    let scope_id_str = std::str::from_utf8(scope_id).unwrap_or("00000000");
    let scope_attr = format!("[data-v-{}]", scope_id_str);

    let mut transformer = CssTransformer::new(css_str, &scope_attr, content_offset);
    let (transformed_css, v_bind_expressions) = transformer.transform()?;

    Ok(TransformResult {
        css: transformed_css.into_bytes(),
        v_bind_expressions,
    })
}

/// CSS transformer that processes CSS rules and selectors
struct CssTransformer<'a> {
    input: &'a str,
    scope_attr: &'a str,
    content_offset: u32,
    v_bind_expressions: Vec<CssVBindExpression>,
}

impl<'a> CssTransformer<'a> {
    fn new(input: &'a str, scope_attr: &'a str, content_offset: u32) -> Self {
        Self {
            input,
            scope_attr,
            content_offset,
            v_bind_expressions: Vec::new(),
        }
    }

    /// Transform the CSS, returning transformed CSS and v-bind expressions
    fn transform(&mut self) -> Result<(String, Vec<CssVBindExpression>), String> {
        let mut output = String::with_capacity(self.input.len() + 256);
        let mut chars = self.input.char_indices().peekable();
        let mut in_string = false;
        let mut string_char = '"';
        let mut in_comment = false;

        while let Some((i, c)) = chars.next() {
            match c {
                // Track comment boundaries
                '/' if !in_string && !in_comment => {
                    if let Some(&(_, '*')) = chars.peek() {
                        // Start of /* comment
                        in_comment = true;
                        output.push('/');
                        if let Some((_, next_c)) = chars.next() {
                            output.push(next_c);
                        }
                        continue;
                    }
                    output.push(c);
                    continue;
                }
                '*' if in_comment => {
                    output.push(c);
                    if let Some(&(_, '/')) = chars.peek() {
                        // End of comment
                        in_comment = false;
                        if let Some((_, next_c)) = chars.next() {
                            output.push(next_c);
                        }
                    }
                    continue;
                }
                _ if in_comment => {
                    // Inside comment, just copy
                    output.push(c);
                    continue;
                }
                // Track string boundaries
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
                    // Everything before '{' is a selector list
                    // Find the start of this selector (after previous } or start)
                    let selector_end = output.len();
                    let selector_start = output.rfind('}').map(|p| p + 1).unwrap_or(0);

                    // Also skip past any @ rules
                    let at_rule_start = output[selector_start..].find('@');
                    let actual_start = if let Some(at_pos) = at_rule_start {
                        // This is an at-rule, don't transform
                        selector_start + at_pos
                    } else {
                        selector_start
                    };

                    if actual_start < selector_end {
                        let raw_text = &output[actual_start..selector_end];

                        // Strip comments from selector text before transformation
                        let selector_no_comments = strip_css_comments(raw_text);
                        let selector_text = selector_no_comments.trim();

                        // Check if this is an @-rule (media, keyframes, etc.)
                        if !selector_text.starts_with('@') && !selector_text.is_empty() {
                            let transformed = self.transform_selector_list(selector_text);
                            // Reconstruct: preserve comments + transformed selector
                            let comments = extract_css_comments(raw_text);
                            output.truncate(actual_start);
                            output.push_str(&comments);
                            output.push_str(&transformed);
                        }
                    }

                    output.push('{');
                }
                // Handle v-bind() in declaration values
                'v' if !in_string && self.peek_str(&self.input[i..], "v-bind(") => {
                    // Extract and transform v-bind expression
                    let (transformed, new_pos) = self.transform_v_bind(i)?;
                    output.push_str(&transformed);

                    // Skip the consumed characters
                    while chars.peek().map(|(idx, _)| *idx < new_pos).unwrap_or(false) {
                        chars.next();
                    }
                }
                _ => output.push(c),
            }
        }

        Ok((output, std::mem::take(&mut self.v_bind_expressions)))
    }

    /// Check if the input starts with the given string
    fn peek_str(&self, input: &str, pattern: &str) -> bool {
        input.starts_with(pattern)
    }

    /// Transform a v-bind() expression
    fn transform_v_bind(&mut self, start_pos: usize) -> Result<(String, usize), String> {
        // Find the matching closing parenthesis
        let input = &self.input[start_pos..];
        let paren_start = input.find('(').ok_or("Invalid v-bind: missing (")?;
        let mut depth = 1;
        let mut end_pos = paren_start + 1;

        for c in input[paren_start + 1..].chars() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
            end_pos += c.len_utf8();
        }

        let expr = input[paren_start + 1..end_pos].trim();

        // Remove quotes if present
        let expr_clean = if (expr.starts_with('\'') && expr.ends_with('\''))
            || (expr.starts_with('"') && expr.ends_with('"'))
        {
            &expr[1..expr.len() - 1]
        } else {
            expr
        };

        // Generate CSS variable name from expression
        let var_name = self.generate_var_name(expr_clean);

        // Store the v-bind expression
        self.v_bind_expressions.push(CssVBindExpression {
            var_name_start: 0, // Will be set properly when we know output position
            var_name_end: 0,
            expression: Span::new(
                self.content_offset + start_pos as u32 + paren_start as u32 + 1,
                self.content_offset + start_pos as u32 + end_pos as u32,
            ),
            css_start: self.content_offset + start_pos as u32,
            css_end: self.content_offset + start_pos as u32 + end_pos as u32 + 1,
        });

        Ok((format!("var({})", var_name), start_pos + end_pos + 1))
    }

    /// Generate CSS variable name from v-bind expression
    fn generate_var_name(&self, expr: &str) -> String {
        // Extract the scope ID from scope_attr (e.g., "[data-v-abc123]" -> "abc123")
        let scope_id = self
            .scope_attr
            .strip_prefix("[data-v-")
            .and_then(|s| s.strip_suffix(']'))
            .unwrap_or("0");

        // Sanitize expression for CSS variable name
        // Replace . with - for object access, remove spaces and quotes
        // Note: Order matters - we replace . first before other transformations
        let sanitized = expr.replace([' ', '\'', '"'], "").replace('.', "-");

        format!("--{}-{}", scope_id, sanitized)
    }

    /// Transform a comma-separated selector list
    fn transform_selector_list(&self, selectors: &str) -> String {
        selectors
            .split(',')
            .map(|s| self.transform_single_selector(s.trim()))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Transform a single selector
    fn transform_single_selector(&self, selector: &str) -> String {
        // Handle special pseudo-classes
        if selector.contains(":deep(") || selector.contains("::v-deep(") {
            return self.transform_deep_selector(selector);
        }
        if selector.contains(":slotted(") || selector.contains("::v-slotted(") {
            return self.transform_slotted_selector(selector);
        }
        if selector.contains(":global(") || selector.contains("::v-global(") {
            return self.transform_global_selector(selector);
        }

        // Regular selector - add scope to each simple selector
        self.add_scope_to_selector(selector)
    }

    /// Add scope attribute to a regular selector
    fn add_scope_to_selector(&self, selector: &str) -> String {
        // Split by combinators, keeping them in the result
        // Combinators: space, >, +, ~
        let mut result = String::with_capacity(selector.len() + self.scope_attr.len() * 2);
        let mut current_simple = String::new();
        let mut chars = selector.chars().peekable();

        while let Some(c) = chars.next() {
            match c {
                ' ' | '>' | '+' | '~' => {
                    if !current_simple.trim().is_empty() {
                        result.push_str(&self.scope_simple_selector(&current_simple));
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

        // Handle the last simple selector
        if !current_simple.trim().is_empty() {
            result.push_str(&self.scope_simple_selector(&current_simple));
        }

        result
    }

    /// Add scope attribute to a simple selector
    fn scope_simple_selector(&self, selector: &str) -> String {
        let selector = selector.trim();
        if selector.is_empty() {
            return selector.to_string();
        }

        // Find where to insert the scope attribute
        // It should go after the element name and any class/id selectors,
        // but before pseudo-classes and pseudo-elements
        let mut insert_pos = selector.len();

        // Look for pseudo-elements (::) or pseudo-classes (:)
        if let Some(pos) = selector.find("::") {
            insert_pos = pos;
        } else if let Some(pos) = selector.find(':') {
            // Make sure it's not part of :where(), :is(), :has(), etc.
            let before = &selector[..pos];
            if !before.ends_with('\\') {
                insert_pos = pos;
            }
        }

        // Insert the scope attribute
        let mut result = String::with_capacity(selector.len() + self.scope_attr.len());
        result.push_str(&selector[..insert_pos]);
        result.push_str(self.scope_attr);
        result.push_str(&selector[insert_pos..]);
        result
    }

    /// Transform :deep() selector
    /// :deep(.inner) → [data-v-{id}] .inner
    fn transform_deep_selector(&self, selector: &str) -> String {
        // Find :deep( or ::v-deep(
        let (prefix, deep_start) = if let Some(pos) = selector.find(":deep(") {
            (":deep(", pos)
        } else if let Some(pos) = selector.find("::v-deep(") {
            ("::v-deep(", pos)
        } else {
            return selector.to_string();
        };

        let before = &selector[..deep_start];
        let after_deep_start = deep_start + prefix.len();

        // Find matching closing paren
        let rest = &selector[after_deep_start..];
        let mut depth = 1;
        let mut end_pos = 0;
        for c in rest.chars() {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            end_pos += c.len_utf8();
        }

        let inner = &rest[..end_pos];
        let after = &rest[end_pos + 1..]; // skip the closing )

        // Transform: before + [data-v-{id}] + " " + inner + after
        let mut result = String::new();

        // Add scoped version of "before" part (if any)
        if !before.trim().is_empty() {
            result.push_str(&self.add_scope_to_selector(before.trim()));
            result.push(' ');
        } else {
            // If there's nothing before :deep(), add scope attr
            result.push_str(self.scope_attr);
            result.push(' ');
        }

        result.push_str(inner.trim());
        result.push_str(after);
        result
    }

    /// Transform :slotted() selector
    /// :slotted(.slot) → .slot[data-v-{id}-s]
    fn transform_slotted_selector(&self, selector: &str) -> String {
        // Find :slotted( or ::v-slotted(
        let (prefix, slotted_start) = if let Some(pos) = selector.find(":slotted(") {
            (":slotted(", pos)
        } else if let Some(pos) = selector.find("::v-slotted(") {
            ("::v-slotted(", pos)
        } else {
            return selector.to_string();
        };

        let before = &selector[..slotted_start];
        let after_slotted_start = slotted_start + prefix.len();

        // Find matching closing paren
        let rest = &selector[after_slotted_start..];
        let mut depth = 1;
        let mut end_pos = 0;
        for c in rest.chars() {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            end_pos += c.len_utf8();
        }

        let inner = &rest[..end_pos];
        let after = &rest[end_pos + 1..];

        // Create slotted scope attr: [data-v-{id}-s]
        let slotted_scope = self.scope_attr.replace(']', "-s]");

        let mut result = String::new();
        result.push_str(before);
        result.push_str(inner.trim());
        result.push_str(&slotted_scope);
        result.push_str(after);
        result
    }

    /// Transform :global() selector
    /// :global(.global) → .global
    fn transform_global_selector(&self, selector: &str) -> String {
        // Find :global( or ::v-global(
        let (prefix, global_start) = if let Some(pos) = selector.find(":global(") {
            (":global(", pos)
        } else if let Some(pos) = selector.find("::v-global(") {
            ("::v-global(", pos)
        } else {
            return selector.to_string();
        };

        let before = &selector[..global_start];
        let after_global_start = global_start + prefix.len();

        // Find matching closing paren
        let rest = &selector[after_global_start..];
        let mut depth = 1;
        let mut end_pos = 0;
        for c in rest.chars() {
            if c == '(' {
                depth += 1;
            } else if c == ')' {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            end_pos += c.len_utf8();
        }

        let inner = &rest[..end_pos];
        let after = &rest[end_pos + 1..];

        // Just remove :global() wrapper
        let mut result = String::new();
        result.push_str(before);
        result.push_str(inner.trim());
        result.push_str(after);
        result
    }
}

// TODO this should use the CSS AST and remove through there....
/// Strip CSS comments from a string, returning just the non-comment content
fn strip_css_comments(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut in_comment = false;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if !in_comment {
            if c == '/' && chars.peek() == Some(&'*') {
                in_comment = true;
                chars.next(); // consume '*'
            } else {
                result.push(c);
            }
        } else if c == '*' && chars.peek() == Some(&'/') {
            in_comment = false;
            chars.next(); // consume '/'
        }
    }

    result
}

/// Extract only the CSS comments from a string
fn extract_css_comments(input: &str) -> String {
    let mut result = String::new();
    let mut in_comment = false;
    let mut comment_start = 0;
    let mut chars = input.char_indices().peekable();

    while let Some((i, c)) = chars.next() {
        if !in_comment {
            if c == '/' && chars.peek().map(|(_, c)| *c) == Some('*') {
                in_comment = true;
                comment_start = i;
                chars.next(); // consume '*'
            }
        } else if c == '*' && chars.peek().map(|(_, c)| *c) == Some('/') {
            chars.next(); // consume '/'
                          // Include the comment end + 1 for the '/' we just consumed
            let end_pos = chars.peek().map(|(i, _)| *i).unwrap_or(input.len());
            result.push_str(&input[comment_start..end_pos]);
            in_comment = false;
        }
    }

    result
}

// ============================================================================
// CSS Modules Transformation
// ============================================================================

/// Result of CSS modules transformation
#[derive(Debug)]
pub struct ModulesTransformResult {
    /// Transformed CSS bytes with hashed class names
    pub css: Vec<u8>,
    /// Mapping of original class name → hashed class name
    pub class_mapping: Vec<(String, String)>,
}

/// Transform CSS for CSS modules
/// Hashes class names and returns a mapping for runtime use
///
/// # Arguments
/// * `css` - Raw CSS bytes
/// * `component_id` - 8-character component ID for hashing
///
/// # Returns
/// ModulesTransformResult with transformed CSS and class mappings
pub fn transform_css_modules(
    css: &[u8],
    component_id: &[u8; 8],
) -> Result<ModulesTransformResult, String> {
    let css_str = std::str::from_utf8(css).map_err(|e| format!("Invalid UTF-8 in CSS: {}", e))?;
    let component_id_str = std::str::from_utf8(component_id).unwrap_or("00000000");

    let mut transformer = CssModulesTransformer::new(css_str, component_id_str);
    let (transformed_css, class_mapping) = transformer.transform()?;

    Ok(ModulesTransformResult {
        css: transformed_css.into_bytes(),
        class_mapping,
    })
}

/// CSS modules transformer that hashes class names
struct CssModulesTransformer<'a> {
    input: &'a str,
    component_id: &'a str,
    /// Map of original class name → hashed class name
    class_mapping: std::collections::HashMap<String, String>,
    /// Counter for generating unique hashes
    class_counter: usize,
}

impl<'a> CssModulesTransformer<'a> {
    fn new(input: &'a str, component_id: &'a str) -> Self {
        Self {
            input,
            component_id,
            class_mapping: std::collections::HashMap::new(),
            class_counter: 0,
        }
    }

    /// Transform the CSS, returning transformed CSS and class mappings
    fn transform(&mut self) -> Result<(String, Vec<(String, String)>), String> {
        let mut output = String::with_capacity(self.input.len() + 256);
        let mut chars = self.input.char_indices().peekable();
        let mut in_string = false;
        let mut string_char = '"';
        let mut in_comment = false;

        while let Some((_i, c)) = chars.next() {
            match c {
                // Track comment boundaries
                '/' if !in_string && !in_comment => {
                    if let Some(&(_, '*')) = chars.peek() {
                        in_comment = true;
                        output.push('/');
                        if let Some((_, next_c)) = chars.next() {
                            output.push(next_c);
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
                        if let Some((_, next_c)) = chars.next() {
                            output.push(next_c);
                        }
                    }
                    continue;
                }
                _ if in_comment => {
                    output.push(c);
                    continue;
                }
                // Track string boundaries
                '"' | '\'' if !in_string => {
                    in_string = true;
                    string_char = c;
                    output.push(c);
                }
                c if in_string && c == string_char => {
                    in_string = false;
                    output.push(c);
                }
                // Handle rule blocks - transform class selectors
                '{' if !in_string => {
                    let selector_end = output.len();
                    let selector_start = output.rfind('}').map(|p| p + 1).unwrap_or(0);

                    // Skip @ rules
                    let at_rule_start = output[selector_start..].find('@');
                    let actual_start = if let Some(at_pos) = at_rule_start {
                        selector_start + at_pos
                    } else {
                        selector_start
                    };

                    if actual_start < selector_end {
                        let raw_text = &output[actual_start..selector_end];
                        let selector_no_comments = strip_css_comments(raw_text);
                        let selector_text = selector_no_comments.trim();

                        if !selector_text.starts_with('@') && !selector_text.is_empty() {
                            let transformed = self.transform_modules_selector_list(selector_text);
                            let comments = extract_css_comments(raw_text);
                            output.truncate(actual_start);
                            output.push_str(&comments);
                            output.push_str(&transformed);
                        }
                    }

                    output.push('{');
                }
                _ => output.push(c),
            }
        }

        // Convert HashMap to Vec for stable ordering
        let mut mapping: Vec<(String, String)> = self.class_mapping.drain().collect();
        mapping.sort_by(|a, b| a.0.cmp(&b.0));

        Ok((output, mapping))
    }

    /// Transform a comma-separated selector list for modules
    fn transform_modules_selector_list(&mut self, selectors: &str) -> String {
        selectors
            .split(',')
            .map(|s| self.transform_modules_selector(s.trim()))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// Transform a single selector - hash class names
    fn transform_modules_selector(&mut self, selector: &str) -> String {
        let mut result = String::with_capacity(selector.len() + 32);
        let mut chars = selector.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '.' {
                // Found a class selector - extract and hash the class name
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

    /// Get existing hash or create new one for a class name
    fn get_or_create_hash(&mut self, class_name: &str) -> String {
        if let Some(hashed) = self.class_mapping.get(class_name) {
            return hashed.clone();
        }

        // Generate hash: _{class_name}_{component_id}_{counter}
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

    fn transform(css: &str, scope_id: &str) -> String {
        let mut id = [0u8; 8];
        id.copy_from_slice(scope_id.as_bytes());
        let result = transform_scoped_css(css.as_bytes(), &id, 0).unwrap();
        String::from_utf8(result.css).unwrap()
    }

    #[test]
    fn test_basic_class_selector() {
        let result = transform(".box { color: red; }", "a4f2eed6");
        assert!(
            result.contains(".box[data-v-a4f2eed6]"),
            "Should scope class selector. Got: {}",
            result
        );
    }

    #[test]
    fn test_element_selector() {
        let result = transform("div { color: red; }", "a4f2eed6");
        assert!(
            result.contains("div[data-v-a4f2eed6]"),
            "Should scope element selector. Got: {}",
            result
        );
    }

    #[test]
    fn test_id_selector() {
        let result = transform("#app { color: red; }", "a4f2eed6");
        assert!(
            result.contains("#app[data-v-a4f2eed6]"),
            "Should scope ID selector. Got: {}",
            result
        );
    }

    #[test]
    fn test_multiple_selectors() {
        let result = transform(".a, .b { color: red; }", "a4f2eed6");
        assert!(
            result.contains(".a[data-v-a4f2eed6]"),
            "Should scope first selector. Got: {}",
            result
        );
        assert!(
            result.contains(".b[data-v-a4f2eed6]"),
            "Should scope second selector. Got: {}",
            result
        );
    }

    #[test]
    fn test_descendant_selector() {
        let result = transform(".parent .child { color: red; }", "a4f2eed6");
        assert!(
            result.contains(".parent[data-v-a4f2eed6]"),
            "Should scope parent. Got: {}",
            result
        );
        assert!(
            result.contains(".child[data-v-a4f2eed6]"),
            "Should scope child. Got: {}",
            result
        );
    }

    #[test]
    fn test_deep_selector() {
        let result = transform(":deep(.inner) { color: red; }", "a4f2eed6");
        assert!(
            result.contains("[data-v-a4f2eed6]"),
            "Should have scope attr. Got: {}",
            result
        );
        assert!(
            result.contains(".inner"),
            "Should have inner selector. Got: {}",
            result
        );
        // The inner selector should NOT have the scope attr
        assert!(
            !result.contains(".inner[data-v"),
            "Inner should not be scoped. Got: {}",
            result
        );
    }

    #[test]
    fn test_slotted_selector() {
        let result = transform(":slotted(.slot-content) { color: red; }", "a4f2eed6");
        assert!(
            result.contains(".slot-content[data-v-a4f2eed6-s]"),
            "Should have slotted scope. Got: {}",
            result
        );
    }

    #[test]
    fn test_global_selector() {
        let result = transform(":global(.global-class) { color: red; }", "a4f2eed6");
        assert!(
            result.contains(".global-class"),
            "Should have global class. Got: {}",
            result
        );
        assert!(
            !result.contains("[data-v"),
            "Should NOT have scope attr. Got: {}",
            result
        );
    }

    #[test]
    fn test_selector_with_pseudo_class() {
        let result = transform(".btn:hover { color: red; }", "a4f2eed6");
        assert!(
            result.contains(".btn[data-v-a4f2eed6]:hover"),
            "Scope should be before pseudo-class. Got: {}",
            result
        );
    }

    #[test]
    fn test_selector_with_pseudo_element() {
        let result = transform(".text::before { content: ''; }", "a4f2eed6");
        assert!(
            result.contains(".text[data-v-a4f2eed6]::before"),
            "Scope should be before pseudo-element. Got: {}",
            result
        );
    }

    #[test]
    fn test_v_bind_simple() {
        let css = ".box { color: v-bind(color); }";
        let mut id = [0u8; 8];
        id.copy_from_slice(b"a4f2eed6");
        let result = transform_scoped_css(css.as_bytes(), &id, 0).unwrap();
        let css_out = String::from_utf8(result.css).unwrap();

        assert!(
            css_out.contains("var(--a4f2eed6-color)"),
            "Should transform v-bind. Got: {}",
            css_out
        );
        assert_eq!(result.v_bind_expressions.len(), 1);
    }

    #[test]
    fn test_v_bind_with_quotes() {
        let css = ".box { color: v-bind('theme.color'); }";
        let mut id = [0u8; 8];
        id.copy_from_slice(b"a4f2eed6");
        let result = transform_scoped_css(css.as_bytes(), &id, 0).unwrap();
        let css_out = String::from_utf8(result.css).unwrap();

        assert!(
            css_out.contains("var(--a4f2eed6-theme-color)"),
            "Should transform v-bind with quotes. Got: {}",
            css_out
        );
    }
}
