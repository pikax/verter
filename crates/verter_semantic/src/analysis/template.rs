//! Template analysis types for Vue SFC templates.
//!
//! These types are populated by `verter_compiler` during compilation (as raw data),
//! then converted by `verter_session` into these analysis types. They enable:
//! - Cross-file render tree construction
//! - Prop constness optimization
//! - LSP features (references, rename, document highlights)
//! - Linter rules (unused components, accessibility, etc.)
use verter_session_query::analysis::template::{DynamicClassName, DynamicStyleVar, StaticStyleVar};

// =============================================================================
// Core Template Analysis Snapshot
// =============================================================================

// =============================================================================
// Component Usage
// =============================================================================

// =============================================================================
// Binding Occurrences
// =============================================================================

// =============================================================================
// Slots
// =============================================================================

// =============================================================================
// Template Refs
// =============================================================================

// =============================================================================
// Event Handlers
// =============================================================================

// =============================================================================
// Directives
// =============================================================================

// =============================================================================
// Elements
// =============================================================================

/// Extract class names from a `:class` binding expression (object syntax).
///
/// Handles common patterns:
/// - `{ 'my-class': condition }` → `["my-class"]`
/// - `{ active: isActive, 'text-bold': isBold }` → `["active", "text-bold"]`
/// - `[{ foo: bar }, 'static']` → `["foo"]` (extracts from objects in arrays)
///
/// Returns empty vec for unparseable expressions (ternary, function calls, variables).
pub fn extract_dynamic_class_names(expr: &str) -> Vec<String> {
    extract_dynamic_class_names_rich(expr)
        .into_iter()
        .filter(|dcn| !dcn.is_partial)
        .map(|dcn| dcn.name)
        .collect()
}

/// Extract dynamic class names with rich metadata from a `:class` expression.
///
/// Handles:
/// - Object syntax: `{ 'my-class': cond }` → class names from keys
/// - Array syntax: `['foo', { bar: cond }]` → string literals + object keys
/// - Ternary: `cond ? 'active' : 'inactive'` → both branch values
/// - Logical: `cond && 'active'` → the string literal
/// - Template literal keys: `` { `test-${foo}`: cond } `` → partial prefix
pub fn extract_dynamic_class_names_rich(expr: &str) -> Vec<DynamicClassName> {
    let trimmed = expr.trim();
    let leading_ws = expr.len() - expr.trim_start().len();
    if trimmed.starts_with('{') {
        extract_object_class_keys_rich(trimmed, leading_ws)
    } else if trimmed.starts_with('[') {
        extract_array_class_keys_rich(trimmed, leading_ws)
    } else {
        // Ternary / logical expression at top level
        extract_string_literals_from_expr(trimmed, leading_ws)
    }
}

/// Extract rich class name info from object syntax `{ 'foo': cond, bar: cond2 }`.
fn extract_object_class_keys_rich(expr: &str, base_offset: usize) -> Vec<DynamicClassName> {
    let inner = expr.trim();
    let brace_start = inner.find('{').unwrap_or(0);
    let inner_content = &inner[brace_start + 1..];
    let inner_content = inner_content.strip_suffix('}').unwrap_or(inner_content);
    let content_offset = base_offset + brace_start + 1;

    let mut results = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let bytes = inner_content.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        match bytes[i] {
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth -= 1,
            b'\'' | b'"' | b'`' if depth == 0 => {
                let quote = bytes[i];
                i += 1;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b',' if depth == 0 => {
                if let Some(dcn) =
                    extract_key_from_pair_rich(&inner_content[start..i], content_offset + start)
                {
                    results.push(dcn);
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < len {
        if let Some(dcn) =
            extract_key_from_pair_rich(&inner_content[start..], content_offset + start)
        {
            results.push(dcn);
        }
    }
    results
}

/// Extract key info from a single `key: value` pair with offset tracking.
fn extract_key_from_pair_rich(pair: &str, pair_offset: usize) -> Option<DynamicClassName> {
    let trimmed = pair.trim();
    let trim_offset = pair.len() - pair.trim_start().len();
    let abs_offset = pair_offset + trim_offset;
    let bytes = trimmed.as_bytes();
    let mut i = 0;
    let len = bytes.len();
    let mut colon_pos = None;
    while i < len {
        match bytes[i] {
            b'\'' | b'"' | b'`' => {
                let quote = bytes[i];
                i += 1;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b':' => {
                colon_pos = Some(i);
                break;
            }
            _ => {}
        }
        i += 1;
    }
    let colon_pos = colon_pos?;
    let key_part = trimmed[..colon_pos].trim();
    let key_trim_offset = trimmed[..colon_pos].len() - trimmed[..colon_pos].trim_start().len();

    if key_part.starts_with('`') && key_part.ends_with('`') {
        // Template literal key — extract static prefix
        let inner = &key_part[1..key_part.len() - 1];
        if let Some(interp_start) = inner.find("${") {
            let prefix = &inner[..interp_start];
            if !prefix.is_empty() {
                return Some(DynamicClassName {
                    name: prefix.to_string(),
                    expr_offset: (abs_offset + key_trim_offset + 1) as u32, // +1 for backtick
                    is_conditional: true,
                    is_partial: true,
                });
            }
        }
        // No interpolation — treat as regular string
        let inner = &key_part[1..key_part.len() - 1];
        return Some(DynamicClassName {
            name: inner.to_string(),
            expr_offset: (abs_offset + key_trim_offset + 1) as u32,
            is_conditional: true,
            is_partial: false,
        });
    }

    if (key_part.starts_with('\'') && key_part.ends_with('\''))
        || (key_part.starts_with('"') && key_part.ends_with('"'))
    {
        let name = key_part[1..key_part.len() - 1].to_string();
        Some(DynamicClassName {
            name,
            expr_offset: (abs_offset + key_trim_offset + 1) as u32, // +1 for opening quote
            is_conditional: true,
            is_partial: false,
        })
    } else if key_part
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'$')
        && !key_part.is_empty()
    {
        Some(DynamicClassName {
            name: key_part.to_string(),
            expr_offset: (abs_offset + key_trim_offset) as u32,
            is_conditional: true,
            is_partial: false,
        })
    } else {
        None
    }
}

/// Extract rich class names from array syntax `['foo', { bar: cond }, baz && 'qux']`.
fn extract_array_class_keys_rich(expr: &str, base_offset: usize) -> Vec<DynamicClassName> {
    let inner = expr.trim();
    let bracket_start = inner.find('[').unwrap_or(0);
    let inner_content = &inner[bracket_start + 1..];
    let inner_content = inner_content.strip_suffix(']').unwrap_or(inner_content);
    let content_offset = base_offset + bracket_start + 1;

    let mut results = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let bytes = inner_content.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        match bytes[i] {
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => {
                depth -= 1;
            }
            b'\'' | b'"' | b'`' if depth == 0 => {
                let quote = bytes[i];
                i += 1;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b',' if depth == 0 => {
                results.extend(extract_array_element_rich(
                    &inner_content[start..i],
                    content_offset + start,
                ));
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < len {
        results.extend(extract_array_element_rich(
            &inner_content[start..],
            content_offset + start,
        ));
    }
    results
}

/// Process a single array element: string literal, object, or expression with strings.
fn extract_array_element_rich(elem: &str, elem_offset: usize) -> Vec<DynamicClassName> {
    let trimmed = elem.trim();
    let trim_offset = elem.len() - elem.trim_start().len();
    let abs_offset = elem_offset + trim_offset;

    if trimmed.starts_with('{') {
        return extract_object_class_keys_rich(trimmed, abs_offset);
    }

    // Check for direct string literal: 'foo' or "foo"
    if (trimmed.starts_with('\'') && trimmed.ends_with('\''))
        || (trimmed.starts_with('"') && trimmed.ends_with('"'))
    {
        let name = trimmed[1..trimmed.len() - 1].to_string();
        if !name.is_empty() {
            return vec![DynamicClassName {
                name,
                expr_offset: (abs_offset + 1) as u32,
                is_conditional: false,
                is_partial: false,
            }];
        }
        return vec![];
    }

    // Expression containing string literals (ternary, logical, etc.)
    extract_string_literals_from_expr(trimmed, abs_offset)
}

/// Extract string literals from expressions like `cond ? 'active' : 'inactive'`
/// or `cond && 'active'`.
fn extract_string_literals_from_expr(expr: &str, base_offset: usize) -> Vec<DynamicClassName> {
    let mut results = Vec::new();
    let bytes = expr.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            let quote = bytes[i];
            let str_start = i + 1;
            i += 1;
            while i < len && bytes[i] != quote {
                if bytes[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            if i < len {
                let name = &expr[str_start..i];
                if !name.is_empty() {
                    results.push(DynamicClassName {
                        name: name.to_string(),
                        expr_offset: (base_offset + str_start) as u32,
                        is_conditional: true,
                        is_partial: false,
                    });
                }
            }
        }
        i += 1;
    }
    results
}

// Old extract_object_class_keys, extract_key_from_pair, extract_array_class_keys
// removed — all callers now use extract_dynamic_class_names_rich.

// =============================================================================
// CSS Variable Extraction from Template `:style` Bindings
// =============================================================================

/// Extract CSS variable definitions from a dynamic `:style` expression.
///
/// Handles object syntax `{ '--color': val }` and extracts only keys starting with `--`.
/// Also handles array syntax `[{ '--a': x }, { '--b': y }]`.
pub fn extract_dynamic_style_vars(expr: &str) -> Vec<DynamicStyleVar> {
    let trimmed = expr.trim();
    if trimmed.starts_with('{') {
        extract_style_vars_from_object(trimmed, 0)
    } else if trimmed.starts_with('[') {
        extract_style_vars_from_array(trimmed)
    } else {
        Vec::new()
    }
}

fn extract_style_vars_from_object(expr: &str, _base_offset: usize) -> Vec<DynamicStyleVar> {
    let inner = expr.trim();
    let brace_start = inner.find('{').unwrap_or(0);
    let inner_content = &inner[brace_start + 1..];
    let inner_content = inner_content.strip_suffix('}').unwrap_or(inner_content);
    let content_offset = brace_start + 1;

    let mut results = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let bytes = inner_content.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        match bytes[i] {
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth -= 1,
            b'\'' | b'"' | b'`' if depth == 0 => {
                let quote = bytes[i];
                i += 1;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b',' if depth == 0 => {
                extract_style_var_from_pair(
                    &inner_content[start..i],
                    content_offset + start,
                    &mut results,
                );
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < len {
        extract_style_var_from_pair(
            &inner_content[start..],
            content_offset + start,
            &mut results,
        );
    }

    results
}

fn extract_style_var_from_pair(pair: &str, offset: usize, out: &mut Vec<DynamicStyleVar>) {
    let pair = pair.trim();
    if pair.is_empty() {
        return;
    }

    // Find the colon separator (key: value), accounting for nested colons
    let bytes = pair.as_bytes();
    let mut depth = 0i32;
    let mut colon_pos = None;
    let mut i = 0;
    let len = bytes.len();
    while i < len {
        match bytes[i] {
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth -= 1,
            b'\'' | b'"' | b'`' if depth == 0 => {
                let quote = bytes[i];
                i += 1;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b':' if depth == 0 => {
                colon_pos = Some(i);
                break;
            }
            _ => {}
        }
        i += 1;
    }

    let Some(colon) = colon_pos else {
        return;
    };

    let key_text = pair[..colon].trim();
    let value_text = pair[colon + 1..].trim();

    // Extract the key — must start with --
    let (var_name, is_dynamic_key, key_offset) =
        if key_text.starts_with('\'') || key_text.starts_with('"') {
            // Quoted key
            let inner = &key_text[1..key_text.len().saturating_sub(1)];
            if inner.starts_with("--") {
                (inner.to_string(), false, offset + 1)
            } else {
                return;
            }
        } else if key_text.starts_with('`') {
            // Template literal key
            let inner = &key_text[1..key_text.len().saturating_sub(1)];
            if inner.starts_with("--") {
                // Extract the static prefix before ${
                let prefix = if let Some(dollar_pos) = inner.find("${") {
                    &inner[..dollar_pos]
                } else {
                    inner
                };
                (prefix.to_string(), inner.contains("${"), offset + 1)
            } else {
                return;
            }
        } else {
            // Unquoted identifier — not a CSS variable key (CSS vars start with --)
            return;
        };

    out.push(DynamicStyleVar {
        name: var_name,
        expr_offset: key_offset as u32,
        value_expr: value_text.to_string(),
        is_dynamic_key,
        is_conditional: false,
    });
}

fn extract_style_vars_from_array(expr: &str) -> Vec<DynamicStyleVar> {
    let inner = expr.trim();
    let bracket_start = inner.find('[').unwrap_or(0);
    let inner_content = &inner[bracket_start + 1..];
    let inner_content = inner_content.strip_suffix(']').unwrap_or(inner_content);

    let mut results = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let bytes = inner_content.as_bytes();
    let len = bytes.len();
    let mut i = 0;

    while i < len {
        match bytes[i] {
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => depth -= 1,
            b'\'' | b'"' | b'`' if depth == 0 => {
                let quote = bytes[i];
                i += 1;
                while i < len && bytes[i] != quote {
                    if bytes[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b',' if depth == 0 => {
                let element = inner_content[start..i].trim();
                if element.starts_with('{') {
                    results.extend(extract_style_vars_from_object(element, 0));
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < len {
        let element = inner_content[start..].trim();
        if element.starts_with('{') {
            results.extend(extract_style_vars_from_object(element, 0));
        }
    }

    results
}

/// Extract CSS variable definitions from a static `style` attribute value.
///
/// Parses `"--color: red; --size: 10px; color: blue"` and extracts only `--*` declarations.
///
/// Declarations are read from the shared CSS declaration-list parse entry
/// point (`verter_css_syntax::parse_inline_style_declarations` — the same
/// one VDOM/SSR static-style codegen routes through), never re-derived from
/// raw bytes here — a hand-rolled `;`/`:` scan cannot tell a
/// statement-separating `;` from one inside a quoted string value.
pub fn extract_static_style_vars(style_value: &str) -> Vec<StaticStyleVar> {
    verter_css_syntax::parse_inline_style_declarations(style_value)
        .into_iter()
        .filter_map(|decl| {
            let name_span = decl.name_span();
            let name = &style_value[name_span.start as usize..name_span.end as usize];
            if !name.starts_with("--") {
                return None;
            }
            let value_span = decl.value_span();
            let value = &style_value[value_span.start as usize..value_span.end as usize];
            Some(StaticStyleVar {
                name: name.to_string(),
                value: value.to_string(),
                name_offset: name_span.start,
            })
        })
        .collect()
}

// =============================================================================
// If Chains
// =============================================================================

// =============================================================================
// Prop & Emit Definitions
// =============================================================================

// =============================================================================
// Comment Directives
// =============================================================================

// =============================================================================
// Type Enhancements (populated by external type providers)
// =============================================================================

// =============================================================================
// Macro Usage (enriched for linter)
// =============================================================================

// =============================================================================
// Tests
// =============================================================================

// =============================================================================
// Custom Serialize/Deserialize impls (preserves spanStart/spanEnd JSON keys)
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use verter_session_query::analysis::template::*;
    use verter_span::Span;

    /// @ai-generated - TemplateAnalysisSnapshot default is empty
    #[test]
    fn template_snapshot_default_is_empty() {
        let snapshot = TemplateAnalysisSnapshot::default();
        assert!(snapshot.components.is_empty());
        assert!(snapshot.binding_occurrences.is_empty());
        assert!(snapshot.unresolved_bindings.is_empty());
        assert!(snapshot.defined_slots.is_empty());
        assert!(snapshot.template_refs.is_empty());
        assert!(snapshot.event_handlers.is_empty());
        assert!(snapshot.elements.is_empty());
        assert!(snapshot.if_chains.is_empty());
        assert_eq!(snapshot.max_nesting_depth, 0);
        assert!(snapshot.v_if_v_for_conflicts.is_empty());
        assert!(snapshot.type_enhancements.is_none());
    }

    /// @ai-generated - Serialization round-trip for TemplateAnalysisSnapshot
    #[test]
    fn template_snapshot_serde_roundtrip() {
        let snapshot = TemplateAnalysisSnapshot {
            components: vec![TemplateComponentUsage {
                name: "MyChild".to_string(),
                import_source: Some("./MyChild.vue".to_string()),
                is_dynamic: false,
                props: vec![TemplatePropUsage {
                    name: "msg".to_string(),
                    is_bound: false,
                    expression: Some("hello".to_string()),
                    expression_locator: None,
                    constness: PropValueConstness::Const,
                    referenced_bindings: vec![],
                    from_spread: false,
                    span: Span::new(0, 0),
                    name_span: Span::new(0, 0),
                    is_shorthand: false,
                }],
                has_spread: false,
                slots_used: vec!["default".to_string()],
                static_classes: vec![],
                has_dynamic_class: false,
                dynamic_classes: vec![],
                v_models: vec![],
                bindings: vec![],
                events: vec![],
                span: Span::new(10, 50),
            }],
            binding_occurrences: vec![TemplateBindingOccurrence {
                name: "count".to_string(),
                span: Span::new(20, 25),
                usage_kind: BindingUsageKind::Interpolation,
            }],
            unresolved_bindings: vec![UnresolvedBinding {
                name: "unknown".to_string(),
                span: Span::new(30, 37),
            }],
            defined_slots: vec![DefinedSlot {
                name: "header".to_string(),
                has_bindings: true,
                binding_names: vec![],
                binding_expressions: vec![],
                binding_value_spans: vec![],
                has_fallback_content: false,
                span: Span::new(0, 0),
            }],
            template_refs: vec![TemplateRef {
                name: "myEl".to_string(),
                is_dynamic: false,
                target_tag: "div".to_string(),
            }],
            event_handlers: vec![TemplateEventHandler {
                event_name: "click".to_string(),
                handler_binding: Some("handleClick".to_string()),
                is_inline: false,
                target_tag: "div".to_string(),
                span: Span::new(0, 0),
            }],
            ..Default::default()
        };

        let json = serde_json::to_string(&snapshot).expect("serialize");
        let roundtrip: TemplateAnalysisSnapshot = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(snapshot, roundtrip);
    }

    /// @ai-generated - PropValueConstness default is Unknown
    #[test]
    fn prop_constness_default_is_unknown() {
        assert_eq!(PropValueConstness::default(), PropValueConstness::Unknown);
    }

    /// @ai-generated - ElementNamespace default is Html
    #[test]
    fn element_namespace_default_is_html() {
        assert_eq!(ElementNamespace::default(), ElementNamespace::Html);
    }

    /// @ai-generated - TemplateElement with directives serializes correctly
    #[test]
    fn template_element_with_directives_serde() {
        let element = TemplateElement {
            tag: "div".to_string(),
            is_component: false,
            is_self_closing: false,
            namespace: ElementNamespace::Html,
            attributes: vec![TemplateAttribute {
                name: "class".to_string(),
                value: Some("container".to_string()),
                is_dynamic: false,
                span: Span::new(0, 20),
                name_end: 0,
                value_span: None,
            }],
            directives: vec![TemplateDirective {
                name: "if".to_string(),
                raw_name: "v-if".to_string(),
                argument: None,
                modifiers: vec![],
                expression: Some("visible".to_string()),
                span: Span::new(21, 35),
                name_end: 0,
                arg_span: None,
                expression_span: None,
                modifier_spans: Vec::new(),
            }],
            v_for: None,
            v_model: None,
            has_v_if: true,
            has_v_else: false,
            has_v_else_if: false,
            v_if_condition: Some("visible".to_string()),
            has_v_show: false,
            has_v_html: false,
            has_v_text: false,
            has_text_content: false,
            has_bare_text: false,
            has_element_children: false,
            nesting_depth: 1,
            parent_tag: None,
            parent_index: None,
            dynamic_classes: vec![],
            span: Span::new(0, 50),
            tag_span_end: 50,
            content_end: 0,
            text_children: Vec::new(),
            dynamic_style_vars: Vec::new(),
            static_style_vars: Vec::new(),
            component_usage_index: None,
        };

        let json = serde_json::to_string(&element).expect("serialize");
        let roundtrip: TemplateElement = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(element, roundtrip);
    }

    /// @ai-generated - VForDirective serialization
    #[test]
    fn v_for_directive_serde() {
        let v_for = VForDirective {
            variable: "item".to_string(),
            index: Some("i".to_string()),
            iterable: "items".to_string(),
            has_key: true,
            key_expression: Some("item.id".to_string()),
            key_uses_index: false,
            span: Span::new(0, 30),
        };

        let json = serde_json::to_string(&v_for).expect("serialize");
        let roundtrip: VForDirective = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(v_for, roundtrip);
    }

    /// @ai-generated - VModelDirective serialization
    #[test]
    fn v_model_directive_serde() {
        let v_model = VModelDirective {
            binding_name: "modelValue".to_string(),
            modifiers: vec!["lazy".to_string(), "trim".to_string()],
            target_is_component: false,
            target_tag: "input".to_string(),
            span: Span::new(0, 25),
        };

        let json = serde_json::to_string(&v_model).expect("serialize");
        let roundtrip: VModelDirective = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(v_model, roundtrip);
    }

    /// @ai-generated - CommentDirective serialization
    #[test]
    fn comment_directive_serde() {
        let directive = CommentDirective {
            kind: CommentDirectiveKind::Disable,
            message: Some("no-v-html".to_string()),
            span: Span::new(0, 40),
            affects_next_line: false,
        };

        let json = serde_json::to_string(&directive).expect("serialize");
        let roundtrip: CommentDirective = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(directive, roundtrip);
    }

    /// @ai-generated - AnalyzedMacroUsage for defineProps with extracted props
    #[test]
    fn macro_usage_define_props_serde() {
        let usage = AnalyzedMacroUsage {
            kind: MacroKind::DefineProps,
            is_type_based: true,
            type_param: Some("{ msg: string; count: number }".to_string()),
            runtime_arg: None,
            binding_name: Some("props".to_string()),
            props: Some(vec![AnalyzedPropDefinition {
                name: "msg".to_string(),
                callable_role: verter_type_expr::PropCallableRole::default(),
                type_annotation: Some("string".to_string()),
                has_default: false,
                is_required: true,
                is_boolean: false,
                used_in_template: true,
                used_in_script: false,
                span: Span::new(5, 16),
            }]),
            emits: None,
            model_name: None,
            slots: None,
            exposed: None,
            defaults: None,
            type_references: vec!["Props".to_string()],
            type_enhancement: None,
            span: Span::new(0, 50),
        };

        let json = serde_json::to_string(&usage).expect("serialize");
        let roundtrip: AnalyzedMacroUsage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(usage, roundtrip);
    }

    /// @ai-generated - All BindingUsageKind variants serialize correctly
    #[test]
    fn binding_usage_kind_all_variants_serde() {
        let variants = vec![
            BindingUsageKind::Interpolation,
            BindingUsageKind::DirectiveValue,
            BindingUsageKind::EventHandler,
            BindingUsageKind::ComponentTag,
            BindingUsageKind::TemplateRef,
            BindingUsageKind::IteratorSource,
        ];

        for kind in variants {
            let json = serde_json::to_string(&kind).expect("serialize");
            let roundtrip: BindingUsageKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(kind, roundtrip);
        }
    }

    /// @ai-generated - All CommentDirectiveKind variants serialize correctly
    #[test]
    fn comment_directive_kind_all_variants_serde() {
        let variants = vec![
            CommentDirectiveKind::Disable,
            CommentDirectiveKind::DisableNextLine,
            CommentDirectiveKind::Enable,
            CommentDirectiveKind::Todo,
            CommentDirectiveKind::Fixme,
            CommentDirectiveKind::Deprecated,
            CommentDirectiveKind::IgnoreStart,
            CommentDirectiveKind::IgnoreEnd,
        ];

        for kind in variants {
            let json = serde_json::to_string(&kind).expect("serialize");
            let roundtrip: CommentDirectiveKind = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(kind, roundtrip);
        }
    }

    /// @ai-generated - Dynamic component usage
    #[test]
    fn dynamic_component_usage() {
        let component = TemplateComponentUsage {
            name: "component".to_string(),
            import_source: None,
            is_dynamic: true,
            props: vec![],
            has_spread: false,
            slots_used: vec![],
            static_classes: vec![],
            has_dynamic_class: false,
            dynamic_classes: vec![],
            v_models: vec![],
            bindings: vec![],
            events: vec![],
            span: Span::new(0, 30),
        };

        assert!(component.is_dynamic);
        assert!(component.import_source.is_none());
    }

    /// @ai-generated - Spread prop usage
    #[test]
    fn spread_prop_usage() {
        let prop = TemplatePropUsage {
            name: "".to_string(),
            is_bound: true,
            expression: Some("obj".to_string()),
            expression_locator: None,
            constness: PropValueConstness::Unknown,
            referenced_bindings: vec!["obj".to_string()],
            from_spread: true,
            span: Span::new(0, 0),
            name_span: Span::new(0, 0),
            is_shorthand: false,
        };

        assert!(prop.from_spread);
        assert_eq!(prop.constness, PropValueConstness::Unknown);
    }

    #[test]
    fn extract_dynamic_classes_object_syntax() {
        let result = extract_dynamic_class_names("{ 'my-class': isFoo, active: isActive }");
        assert_eq!(result, vec!["my-class", "active"]);
    }

    #[test]
    fn extract_dynamic_classes_quoted_keys() {
        let result = extract_dynamic_class_names(r#"{ "text-bold": isBold, 'text-red': isRed }"#);
        assert_eq!(result, vec!["text-bold", "text-red"]);
    }

    #[test]
    fn extract_dynamic_classes_bare_identifiers() {
        let result = extract_dynamic_class_names("{ active: isActive, disabled: isDisabled }");
        assert_eq!(result, vec!["active", "disabled"]);
    }

    #[test]
    fn extract_dynamic_classes_array_with_objects() {
        let result = extract_dynamic_class_names("[{ foo: bar }, 'static']");
        assert_eq!(result, vec!["foo", "static"]);
    }

    #[test]
    fn extract_dynamic_classes_variable_returns_empty() {
        let result = extract_dynamic_class_names("myClasses");
        assert!(result.is_empty());
    }

    #[test]
    fn extract_dynamic_classes_ternary() {
        let result = extract_dynamic_class_names("isActive ? 'active' : 'inactive'");
        assert_eq!(result, vec!["active", "inactive"]);
    }

    #[test]
    fn extract_dynamic_classes_nested_value() {
        // The value expression can be complex but we only care about keys
        let result = extract_dynamic_class_names(
            "{ highlighted: items.length > 0, 'fade-in': show && ready }",
        );
        assert_eq!(result, vec!["highlighted", "fade-in"]);
    }

    #[test]
    fn extract_dynamic_classes_empty_object() {
        let result = extract_dynamic_class_names("{}");
        assert!(result.is_empty());
    }

    // =========================================================================
    // Rich dynamic class extraction tests (A0b)
    // =========================================================================

    /// @ai-generated - Array string literals extracted with offsets
    #[test]
    fn extract_rich_array_string_literals() {
        let result = extract_dynamic_class_names_rich("['foo', isLoading && 'bar']");
        let names: Vec<&str> = result.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["foo", "bar"]);
        assert!(!result[0].is_partial);
        assert!(!result[0].is_conditional); // direct string literal
        assert!(result[1].is_conditional); // conditional from &&
    }

    /// @ai-generated - Ternary expressions extract both branches
    #[test]
    fn extract_rich_ternary() {
        let result = extract_dynamic_class_names_rich("isActive ? 'active' : 'inactive'");
        let names: Vec<&str> = result.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["active", "inactive"]);
        assert!(result.iter().all(|d| d.is_conditional));
        assert!(result.iter().all(|d| !d.is_partial));
    }

    /// @ai-generated - Template literal key extracts partial prefix
    #[test]
    fn extract_rich_template_literal_prefix() {
        let result = extract_dynamic_class_names_rich("{ `test-${foo}`: cond }");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "test-");
        assert!(result[0].is_partial);
        assert!(result[0].is_conditional);
    }

    /// @ai-generated - Mixed array with objects, strings, and logical expressions
    #[test]
    fn extract_rich_mixed_array() {
        let result = extract_dynamic_class_names_rich("['foo', { bar: cond }, baz && 'qux']");
        let names: Vec<&str> = result.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["foo", "bar", "qux"]);
    }

    /// @ai-generated - Backward compat: extract_dynamic_class_names delegates to rich
    #[test]
    fn extract_dynamic_classes_backward_compat() {
        // Object syntax still works
        let result = extract_dynamic_class_names("{ active: cond }");
        assert_eq!(result, vec!["active"]);
        // Ternary now works via rich path
        let result = extract_dynamic_class_names("cond ? 'a' : 'b'");
        assert_eq!(result, vec!["a", "b"]);
        // Partial prefixes are filtered out
        let result = extract_dynamic_class_names("{ `test-${foo}`: cond }");
        assert!(result.is_empty());
    }

    /// @ai-generated - expr_offset values are correct for object keys
    #[test]
    fn extract_rich_offsets_correct() {
        // "{ 'bar': cond }" — 'bar' starts at offset 3 (after "{ '")
        let expr = "{ 'bar': cond }";
        let result = extract_dynamic_class_names_rich(expr);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].name, "bar");
        let offset = result[0].expr_offset as usize;
        assert_eq!(
            &expr[offset..offset + 3],
            "bar",
            "offset should point to 'bar' text"
        );
    }

    // ===== CSS Variable Extraction Tests =====

    /// @ai-generated - extract_dynamic_style_vars parses object with CSS variable keys
    #[test]
    fn extract_dynamic_style_vars_object() {
        let vars = extract_dynamic_style_vars("{ '--color': val, '--size': computedSize }");
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0].name, "--color");
        assert_eq!(vars[0].value_expr, "val");
        assert!(!vars[0].is_dynamic_key);
        assert_eq!(vars[1].name, "--size");
        assert_eq!(vars[1].value_expr, "computedSize");
    }

    /// @ai-generated - extract_dynamic_style_vars ignores non-CSS-variable keys
    #[test]
    fn extract_dynamic_style_vars_filters_non_css_vars() {
        let vars = extract_dynamic_style_vars("{ 'color': 'red', '--custom': val }");
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].name, "--custom");
    }

    /// @ai-generated - extract_dynamic_style_vars handles template literal keys
    #[test]
    fn extract_dynamic_style_vars_template_literal() {
        let vars = extract_dynamic_style_vars("{ `--${prefix}--color`: val }");
        assert_eq!(vars.len(), 1);
        assert_eq!(vars[0].name, "--");
        assert!(vars[0].is_dynamic_key);
    }

    /// @ai-generated - extract_dynamic_style_vars handles array syntax
    #[test]
    fn extract_dynamic_style_vars_array() {
        let vars = extract_dynamic_style_vars("[{ '--a': x }, { '--b': y }]");
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0].name, "--a");
        assert_eq!(vars[1].name, "--b");
    }

    /// @ai-generated - extract_dynamic_style_vars returns empty for non-object/array
    #[test]
    fn extract_dynamic_style_vars_non_object() {
        let vars = extract_dynamic_style_vars("someVariable");
        assert!(vars.is_empty());
    }

    /// @ai-generated - extract_static_style_vars parses CSS variable declarations
    #[test]
    fn extract_static_style_vars_basic() {
        let vars = extract_static_style_vars("--color: red; --size: 10px; color: blue");
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0].name, "--color");
        assert_eq!(vars[0].value, "red");
        assert_eq!(vars[1].name, "--size");
        assert_eq!(vars[1].value, "10px");
    }

    /// @ai-generated - extract_static_style_vars returns empty for no CSS vars
    #[test]
    fn extract_static_style_vars_no_vars() {
        let vars = extract_static_style_vars("color: red; font-size: 14px");
        assert!(vars.is_empty());
    }

    /// @ai-generated - extract_static_style_vars handles only CSS vars
    #[test]
    fn extract_static_style_vars_only_vars() {
        let vars = extract_static_style_vars("--x: 1; --y: 2");
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0].name, "--x");
        assert_eq!(vars[0].value, "1");
        assert_eq!(vars[1].name, "--y");
        assert_eq!(vars[1].value, "2");
    }

    // Discriminating positive (A20): a value containing a semicolon inside a
    // quoted string must not be treated as a statement boundary — the same
    // bug class A17 fixes for VDOM/SSR (`template.rs:1120`'s prior
    // `split(';')` loop).
    #[test]
    fn extract_static_style_vars_quoted_semicolon_in_value_parses_correctly() {
        let vars = extract_static_style_vars(r#"--label: "a;b"; --color: red;"#);
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0].name, "--label");
        assert_eq!(vars[0].value, "\"a;b\"");
        assert_eq!(vars[1].name, "--color");
        assert_eq!(vars[1].value, "red");
    }

    // Routing proof (A20): the shared declaration-list parser is invoked
    // EXACTLY once per call — not zero (a private scanner still producing
    // the output), not two-or-more (a redundant re-parse, itself a
    // parse-once violation).
    #[test]
    fn extract_static_style_vars_shared_parser_invoked_exactly_once() {
        let before = verter_css_syntax::parse_inline_style_declarations_thread_invocations();
        extract_static_style_vars("--color: red; --size: 10px; color: blue");
        let after = verter_css_syntax::parse_inline_style_declarations_thread_invocations();
        assert_eq!(after - before, 1);
    }

    // =========================================================================
    // Serialization encoding tests
    //
    // These tests verify two invariants:
    //   1. All types use serialize_struct (not serialize_map) so that
    //      serde_wasm_bindgen produces plain JS objects, not Map instances.
    //      The StructEnforcingSerializer returns an error when serialize_map
    //      is called — the tests are RED until all impls use serialize_struct.
    //   2. Span fields are always flat (spanStart / spanEnd at top level) —
    //      no nested "span" object. Verified via serde_json.
    // =========================================================================

    mod serialize_encoding {
        use super::*;
        use serde::ser::{self, Serialize};
        use std::fmt;
        use verter_span::Span;

        // -----------------------------------------------------------------
        // StructEnforcingSerializer — errors on serialize_map
        // -----------------------------------------------------------------

        #[derive(Debug)]
        struct MapUsedError;

        impl fmt::Display for MapUsedError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "serialize_map used — must use serialize_struct instead")
            }
        }

        impl ser::Error for MapUsedError {
            fn custom<T: fmt::Display>(msg: T) -> Self {
                eprintln!("serde error: {msg}");
                MapUsedError
            }
        }

        impl std::error::Error for MapUsedError {}

        /// Serializer that errors when `serialize_map` is called on a type
        /// that should be using `serialize_struct`. Call it with a value:
        /// `value.serialize(StructEnforcingSerializer).unwrap()`
        struct StructEnforcingSerializer;

        struct PassSeq;
        struct PassStruct;

        impl ser::SerializeSeq for PassSeq {
            type Ok = ();
            type Error = MapUsedError;
            fn serialize_element<T: ?Sized + Serialize>(
                &mut self,
                v: &T,
            ) -> Result<(), MapUsedError> {
                v.serialize(StructEnforcingSerializer)
            }
            fn end(self) -> Result<(), MapUsedError> {
                Ok(())
            }
        }

        impl ser::SerializeTuple for PassSeq {
            type Ok = ();
            type Error = MapUsedError;
            fn serialize_element<T: ?Sized + Serialize>(
                &mut self,
                v: &T,
            ) -> Result<(), MapUsedError> {
                v.serialize(StructEnforcingSerializer)
            }
            fn end(self) -> Result<(), MapUsedError> {
                Ok(())
            }
        }

        impl ser::SerializeTupleStruct for PassSeq {
            type Ok = ();
            type Error = MapUsedError;
            fn serialize_field<T: ?Sized + Serialize>(
                &mut self,
                v: &T,
            ) -> Result<(), MapUsedError> {
                v.serialize(StructEnforcingSerializer)
            }
            fn end(self) -> Result<(), MapUsedError> {
                Ok(())
            }
        }

        impl ser::SerializeTupleVariant for PassSeq {
            type Ok = ();
            type Error = MapUsedError;
            fn serialize_field<T: ?Sized + Serialize>(
                &mut self,
                v: &T,
            ) -> Result<(), MapUsedError> {
                v.serialize(StructEnforcingSerializer)
            }
            fn end(self) -> Result<(), MapUsedError> {
                Ok(())
            }
        }

        impl ser::SerializeStruct for PassStruct {
            type Ok = ();
            type Error = MapUsedError;
            fn serialize_field<T: ?Sized + Serialize>(
                &mut self,
                _key: &'static str,
                v: &T,
            ) -> Result<(), MapUsedError> {
                v.serialize(StructEnforcingSerializer)
            }
            fn end(self) -> Result<(), MapUsedError> {
                Ok(())
            }
        }

        impl ser::SerializeStructVariant for PassStruct {
            type Ok = ();
            type Error = MapUsedError;
            fn serialize_field<T: ?Sized + Serialize>(
                &mut self,
                _key: &'static str,
                v: &T,
            ) -> Result<(), MapUsedError> {
                v.serialize(StructEnforcingSerializer)
            }
            fn end(self) -> Result<(), MapUsedError> {
                Ok(())
            }
        }

        impl serde::Serializer for StructEnforcingSerializer {
            type Ok = ();
            type Error = MapUsedError;
            type SerializeSeq = PassSeq;
            type SerializeTuple = PassSeq;
            type SerializeTupleStruct = PassSeq;
            type SerializeTupleVariant = PassSeq;
            // SerializeMap is Impossible — calling serialize_map returns Err.
            type SerializeMap = ser::Impossible<(), MapUsedError>;
            type SerializeStruct = PassStruct;
            type SerializeStructVariant = PassStruct;

            fn serialize_bool(self, _: bool) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_i8(self, _: i8) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_i16(self, _: i16) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_i32(self, _: i32) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_i64(self, _: i64) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_u8(self, _: u8) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_u16(self, _: u16) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_u32(self, _: u32) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_u64(self, _: u64) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_f32(self, _: f32) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_f64(self, _: f64) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_char(self, _: char) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_str(self, _: &str) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_bytes(self, _: &[u8]) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_none(self) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_some<T: ?Sized + Serialize>(self, v: &T) -> Result<(), MapUsedError> {
                v.serialize(Self)
            }
            fn serialize_unit(self) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_unit_struct(self, _: &'static str) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_unit_variant(
                self,
                _: &'static str,
                _: u32,
                _: &'static str,
            ) -> Result<(), MapUsedError> {
                Ok(())
            }
            fn serialize_newtype_struct<T: ?Sized + Serialize>(
                self,
                _: &'static str,
                v: &T,
            ) -> Result<(), MapUsedError> {
                v.serialize(Self)
            }
            fn serialize_newtype_variant<T: ?Sized + Serialize>(
                self,
                _: &'static str,
                _: u32,
                _: &'static str,
                v: &T,
            ) -> Result<(), MapUsedError> {
                v.serialize(Self)
            }
            fn serialize_seq(self, _: Option<usize>) -> Result<PassSeq, MapUsedError> {
                Ok(PassSeq)
            }
            fn serialize_tuple(self, _: usize) -> Result<PassSeq, MapUsedError> {
                Ok(PassSeq)
            }
            fn serialize_tuple_struct(
                self,
                _: &'static str,
                _: usize,
            ) -> Result<PassSeq, MapUsedError> {
                Ok(PassSeq)
            }
            fn serialize_tuple_variant(
                self,
                _: &'static str,
                _: u32,
                _: &'static str,
                _: usize,
            ) -> Result<PassSeq, MapUsedError> {
                Ok(PassSeq)
            }
            /// Returns Err — any type calling serialize_map fails the test.
            fn serialize_map(
                self,
                _: Option<usize>,
            ) -> Result<ser::Impossible<(), MapUsedError>, MapUsedError> {
                Err(MapUsedError)
            }
            fn serialize_struct(
                self,
                _: &'static str,
                _: usize,
            ) -> Result<PassStruct, MapUsedError> {
                Ok(PassStruct)
            }
            fn serialize_struct_variant(
                self,
                _: &'static str,
                _: u32,
                _: &'static str,
                _: usize,
            ) -> Result<PassStruct, MapUsedError> {
                Ok(PassStruct)
            }
        }

        // Helper: assert serialize_struct is used and span fields are flat.
        fn assert_uses_struct<T: Serialize>(v: &T) {
            v.serialize(StructEnforcingSerializer)
                .expect("type must use serialize_struct, not serialize_map");
        }

        fn json<T: Serialize>(v: &T) -> serde_json::Value {
            serde_json::to_value(v).expect("serialize to json")
        }

        fn assert_flat_span(j: &serde_json::Value, start: u32, end: u32) {
            assert_eq!(j["spanStart"], start, "spanStart must be top-level");
            assert_eq!(j["spanEnd"], end, "spanEnd must be top-level");
            assert!(
                j.get("span").is_none(),
                "nested 'span' object must not appear"
            );
        }

        // -----------------------------------------------------------------
        // Tests — one per type
        // -----------------------------------------------------------------

        #[test]
        fn template_component_usage_uses_struct() {
            let v = TemplateComponentUsage {
                name: "MyComp".into(),
                import_source: None,
                is_dynamic: false,
                props: vec![],
                has_spread: false,
                slots_used: vec![],
                static_classes: vec![],
                has_dynamic_class: false,
                dynamic_classes: vec![],
                v_models: vec![],
                bindings: vec![],
                events: vec![],
                span: Span::new(10, 20),
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 10, 20);
            assert_eq!(j["name"], "MyComp");
        }

        #[test]
        fn template_component_vmodel_uses_struct() {
            let v = TemplateComponentVModel {
                binding_name: "modelValue".into(),
                span: Span::new(5, 15),
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 5, 15);
            assert_eq!(j["bindingName"], "modelValue");
        }

        #[test]
        fn template_prop_usage_uses_struct() {
            let v = TemplatePropUsage {
                name: "foo".into(),
                is_bound: true,
                expression: Some("foo".into()),
                expression_locator: None,
                constness: PropValueConstness::Const,
                referenced_bindings: vec![],
                from_spread: false,
                span: Span::new(3, 9),
                name_span: Span::new(0, 0),
                is_shorthand: false,
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 3, 9);
            assert_eq!(j["name"], "foo");
        }

        #[test]
        fn template_binding_occurrence_uses_struct() {
            let v = TemplateBindingOccurrence {
                name: "count".into(),
                span: Span::new(20, 25),
                usage_kind: BindingUsageKind::Interpolation,
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 20, 25);
            assert_eq!(j["name"], "count");
        }

        #[test]
        fn template_expression_diagnostic_uses_struct() {
            let v = TemplateExpressionDiagnostic {
                severity: TemplateDiagnosticSeverity::Error,
                code: "XInvalidExpression".into(),
                message: "invalid expression".into(),
                span: Span::new(12, 34),
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 12, 34);
            assert_eq!(j["severity"], "error");
            assert_eq!(j["code"], "XInvalidExpression");
            assert_eq!(j["message"], "invalid expression");
            // Round-trip: the flat wire shape deserializes back to the value.
            let back: TemplateExpressionDiagnostic =
                serde_json::from_value(j).expect("flat wire shape round-trips");
            assert_eq!(back, v);
        }

        #[test]
        fn unresolved_binding_uses_struct() {
            let v = UnresolvedBinding {
                name: "unknown".into(),
                span: Span::new(30, 37),
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 30, 37);
            assert_eq!(j["name"], "unknown");
        }

        #[test]
        fn defined_slot_uses_struct() {
            let v = DefinedSlot {
                name: "header".into(),
                has_bindings: false,
                binding_names: vec![],
                binding_expressions: vec![],
                binding_value_spans: vec![],
                has_fallback_content: false,
                span: Span::new(40, 60),
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 40, 60);
            assert_eq!(j["name"], "header");
            assert!(
                j.get("bindingValueSpans").is_none(),
                "bindingValueSpans must not be serialized"
            );
        }

        #[test]
        fn template_event_handler_uses_struct() {
            let v = TemplateEventHandler {
                event_name: "click".into(),
                handler_binding: Some("handleClick".into()),
                is_inline: false,
                target_tag: "button".into(),
                span: Span::new(50, 70),
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 50, 70);
            assert_eq!(j["eventName"], "click");
        }

        #[test]
        fn template_directive_uses_struct() {
            let v = TemplateDirective {
                name: "if".into(),
                raw_name: "v-if".into(),
                argument: None,
                modifiers: vec![],
                expression: Some("show".into()),
                span: Span::new(1, 10),
                name_end: 4,
                arg_span: None,
                expression_span: None,
                modifier_spans: vec![],
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 1, 10);
            assert_eq!(j["name"], "if");
        }

        #[test]
        fn v_for_directive_uses_struct() {
            let v = VForDirective {
                variable: "item".into(),
                index: None,
                iterable: "items".into(),
                has_key: false,
                key_expression: None,
                key_uses_index: false,
                span: Span::new(2, 20),
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 2, 20);
            assert_eq!(j["variable"], "item");
        }

        #[test]
        fn v_model_directive_uses_struct() {
            let v = VModelDirective {
                binding_name: "modelValue".into(),
                modifiers: vec![],
                target_is_component: true,
                target_tag: "MyInput".into(),
                span: Span::new(6, 25),
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 6, 25);
            assert_eq!(j["bindingName"], "modelValue");
        }

        #[test]
        fn template_element_uses_struct_no_text_children() {
            let v = TemplateElement {
                tag: "div".into(),
                span: Span::new(0, 50),
                ..Default::default()
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 0, 50);
            assert_eq!(j["tag"], "div");
            assert!(
                j.get("textChildren").is_none(),
                "textChildren must not be serialized"
            );
        }

        #[test]
        fn template_attribute_uses_struct() {
            let v = TemplateAttribute {
                name: "class".into(),
                value: Some("foo".into()),
                is_dynamic: false,
                span: Span::new(7, 18),
                name_end: 12,
                value_span: None,
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 7, 18);
            assert_eq!(j["name"], "class");
        }

        #[test]
        fn analyzed_prop_definition_uses_struct() {
            let v = AnalyzedPropDefinition {
                name: "msg".into(),
                callable_role: verter_type_expr::PropCallableRole::default(),
                type_annotation: Some("string".into()),
                has_default: false,
                is_required: true,
                is_boolean: false,
                used_in_template: true,
                used_in_script: false,
                span: Span::new(8, 18),
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 8, 18);
            assert_eq!(j["name"], "msg");
            assert_eq!(j["callableRole"]["kind"], "unresolved");
            assert_eq!(
                j["callableRole"]["reason"], "analysisUnavailable",
                "missing analysis must serialize fail-closed"
            );
        }

        #[test]
        fn analyzed_emit_definition_uses_struct() {
            let v = AnalyzedEmitDefinition {
                event_name: "update".into(),
                has_validator: false,
                is_declared: true,
                emit_locations: vec![],
                span: Span::new(9, 22),
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 9, 22);
            assert_eq!(j["eventName"], "update");
        }

        #[test]
        fn comment_directive_uses_struct() {
            let v = CommentDirective {
                kind: CommentDirectiveKind::Disable,
                message: None,
                span: Span::new(11, 35),
                affects_next_line: false,
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 11, 35);
        }

        #[test]
        fn type_mismatch_uses_struct() {
            let v = TypeMismatch {
                span: Span::new(12, 20),
                expected: "string".into(),
                actual: "number".into(),
                message: "Type mismatch".into(),
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 12, 20);
            assert_eq!(j["expected"], "string");
        }

        #[test]
        fn analyzed_macro_usage_uses_struct() {
            let v = AnalyzedMacroUsage {
                kind: MacroKind::DefineProps,
                is_type_based: false,
                type_param: None,
                runtime_arg: None,
                binding_name: None,
                props: None,
                emits: None,
                model_name: None,
                slots: None,
                exposed: None,
                defaults: None,
                type_references: vec![],
                type_enhancement: None,
                span: Span::new(13, 40),
            };
            assert_uses_struct(&v);
            let j = json(&v);
            assert_flat_span(&j, 13, 40);
        }
    }
}

#[cfg(test)]
mod macro_kind_conversion_tests {
    use verter_session_query::analysis::template::MacroKind as TemplateMacroKind;
    use verter_session_query::facts::registry::MacroKind;

    #[test]
    fn macro_kind_round_trips_with_template_kind() {
        let pairs = [
            (TemplateMacroKind::DefineProps, MacroKind::DefineProps),
            (TemplateMacroKind::DefineEmits, MacroKind::DefineEmits),
            (TemplateMacroKind::DefineModel, MacroKind::DefineModel),
            (TemplateMacroKind::DefineSlots, MacroKind::DefineSlots),
            (TemplateMacroKind::DefineExpose, MacroKind::DefineExpose),
            (TemplateMacroKind::DefineOptions, MacroKind::DefineOptions),
            (TemplateMacroKind::WithDefaults, MacroKind::WithDefaults),
        ];
        for (template, fact) in pairs {
            assert_eq!(MacroKind::from(template), fact);
        }
        let mut distinct: Vec<MacroKind> = pairs.iter().map(|(t, _)| MacroKind::from(*t)).collect();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            pairs.len(),
            "the conversion must be injective"
        );
    }
}
