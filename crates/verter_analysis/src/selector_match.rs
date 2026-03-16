//! CSS selector matching against template elements.
//!
//! Provides three-valued matching: `Matches`, `MaybeMatches`, `NoMatch`.
//! `MaybeMatches` is returned when dynamic attributes (`:class`) prevent
//! definite determination.
//!
//! # Algorithm
//!
//! 1. Start from the rightmost compound selector, match against the target element.
//! 2. Walk left through combinators:
//!    - `Child` → check `parent_index`, match parent
//!    - `Descendant` → walk ancestors via `parent_index` chain
//!    - `NextSibling`/`LaterSibling` → scan siblings (same `parent_index`)
//! 3. Runtime pseudo-classes (`:hover`, `:focus`) don't prevent matching.
//! 4. `:not()` inverts, `:is()`/`:where()` takes best match across alternatives.

use crate::style::{
    AttributeOperator, CompoundSelector, SelectorCombinator, SelectorPseudoClass,
    StructuredSelector,
};
use crate::template::TemplateElement;

/// Result of matching a CSS selector against a template element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MatchResult {
    /// The selector definitely does not match this element.
    NoMatch,
    /// The selector might match (dynamic class/attribute prevents certainty).
    MaybeMatches,
    /// The selector definitely matches this element.
    Matches,
}

impl MatchResult {
    /// Combine two match results: both must match for the compound to match.
    fn and(self, other: MatchResult) -> MatchResult {
        match (self, other) {
            (MatchResult::NoMatch, _) | (_, MatchResult::NoMatch) => MatchResult::NoMatch,
            (MatchResult::MaybeMatches, _) | (_, MatchResult::MaybeMatches) => {
                MatchResult::MaybeMatches
            }
            (MatchResult::Matches, MatchResult::Matches) => MatchResult::Matches,
        }
    }

    /// Combine results: at least one must match (for :is()/:where() alternatives).
    fn or(self, other: MatchResult) -> MatchResult {
        match (self, other) {
            (MatchResult::Matches, _) | (_, MatchResult::Matches) => MatchResult::Matches,
            (MatchResult::MaybeMatches, _) | (_, MatchResult::MaybeMatches) => {
                MatchResult::MaybeMatches
            }
            (MatchResult::NoMatch, MatchResult::NoMatch) => MatchResult::NoMatch,
        }
    }

    /// Invert for :not().
    fn invert(self) -> MatchResult {
        match self {
            MatchResult::Matches => MatchResult::NoMatch,
            MatchResult::NoMatch => MatchResult::Matches,
            MatchResult::MaybeMatches => MatchResult::MaybeMatches,
        }
    }
}

/// Match a structured CSS selector against a template element.
///
/// `element_index` is the index into `elements` for the target element.
/// Returns `NoMatch` if the selector cannot match, `MaybeMatches` if dynamic
/// attributes prevent certainty, or `Matches` for a definite match.
pub fn match_selector(
    selector: &StructuredSelector,
    element_index: usize,
    elements: &[TemplateElement],
) -> MatchResult {
    if selector.compounds.is_empty() {
        return MatchResult::NoMatch;
    }

    // Start from the rightmost compound
    let rightmost_idx = selector.compounds.len() - 1;
    let rightmost_match =
        match_compound(&selector.compounds[rightmost_idx], element_index, elements);

    if rightmost_match == MatchResult::NoMatch {
        return MatchResult::NoMatch;
    }

    // Walk left through combinators
    let mut current_element_idx = element_index;
    let mut result = rightmost_match;

    for compound_idx in (0..rightmost_idx).rev() {
        let combinator = selector.combinators[compound_idx];
        let compound = &selector.compounds[compound_idx];

        match combinator {
            SelectorCombinator::Child => {
                // Parent must match
                let parent_idx = match elements[current_element_idx].parent_index {
                    Some(idx) => idx as usize,
                    None => return MatchResult::NoMatch,
                };
                let parent_match = match_compound(compound, parent_idx, elements);
                result = result.and(parent_match);
                if result == MatchResult::NoMatch {
                    return MatchResult::NoMatch;
                }
                current_element_idx = parent_idx;
            }
            SelectorCombinator::Descendant => {
                // Walk ancestors until one matches
                let mut found = MatchResult::NoMatch;
                let mut ancestor_idx = elements[current_element_idx].parent_index;
                while let Some(idx) = ancestor_idx {
                    let idx = idx as usize;
                    let ancestor_match = match_compound(compound, idx, elements);
                    if ancestor_match == MatchResult::Matches {
                        found = MatchResult::Matches;
                        current_element_idx = idx;
                        break;
                    }
                    if ancestor_match == MatchResult::MaybeMatches {
                        found = MatchResult::MaybeMatches;
                        // Continue looking for a definite match, but remember this maybe
                    }
                    ancestor_idx = elements[idx].parent_index;
                }
                if found == MatchResult::MaybeMatches {
                    // Found a maybe match but no definite — use the last maybe ancestor
                    result = result.and(MatchResult::MaybeMatches);
                    // We can't reliably track which ancestor to continue from
                    // For simplicity, stop here — this is conservative
                    break;
                }
                result = result.and(found);
                if result == MatchResult::NoMatch {
                    return MatchResult::NoMatch;
                }
            }
            SelectorCombinator::NextSibling => {
                // Previous sibling (same parent_index, immediately before) must match
                let match_result = match_sibling(
                    compound,
                    current_element_idx,
                    elements,
                    SiblingMode::Adjacent,
                );
                result = result.and(match_result);
                if result == MatchResult::NoMatch {
                    return MatchResult::NoMatch;
                }
                // Update current_element_idx to the matched sibling
                // For simplicity, we don't track which sibling matched
                break;
            }
            SelectorCombinator::LaterSibling => {
                // Any preceding sibling (same parent_index) must match
                let match_result = match_sibling(
                    compound,
                    current_element_idx,
                    elements,
                    SiblingMode::General,
                );
                result = result.and(match_result);
                if result == MatchResult::NoMatch {
                    return MatchResult::NoMatch;
                }
                break;
            }
        }
    }

    result
}

#[derive(Debug, Clone, Copy)]
enum SiblingMode {
    /// `+` — only the immediately preceding sibling
    Adjacent,
    /// `~` — any preceding sibling
    General,
}

/// Match a sibling combinator by scanning elements with the same parent_index.
fn match_sibling(
    compound: &CompoundSelector,
    element_index: usize,
    elements: &[TemplateElement],
    mode: SiblingMode,
) -> MatchResult {
    let parent_index = elements[element_index].parent_index;

    // Find siblings: elements with the same parent_index that appear before this one
    let mut best = MatchResult::NoMatch;
    let mut last_sibling_before: Option<usize> = None;

    for (i, el) in elements.iter().enumerate() {
        if i >= element_index {
            break;
        }
        if el.parent_index == parent_index {
            last_sibling_before = Some(i);

            if matches!(mode, SiblingMode::General) {
                let m = match_compound(compound, i, elements);
                best = best.or(m);
                if best == MatchResult::Matches {
                    return MatchResult::Matches;
                }
            }
        }
    }

    match mode {
        SiblingMode::Adjacent => {
            if let Some(idx) = last_sibling_before {
                match_compound(compound, idx, elements)
            } else {
                MatchResult::NoMatch
            }
        }
        SiblingMode::General => best,
    }
}

/// Match a single compound selector against a specific element.
fn match_compound(
    compound: &CompoundSelector,
    element_index: usize,
    elements: &[TemplateElement],
) -> MatchResult {
    let element = &elements[element_index];
    let mut result = MatchResult::Matches;

    // Element type selector
    if let Some(ref el_type) = compound.element {
        if element.is_component {
            // Component elements might match any type — we can't know
            result = result.and(MatchResult::MaybeMatches);
        } else if !element.tag.eq_ignore_ascii_case(el_type) {
            return MatchResult::NoMatch;
        }
    }

    // ID selector
    if let Some(ref id) = compound.id {
        match element.static_id() {
            Some(static_id) => {
                if static_id != id {
                    // Check for dynamic id
                    if has_dynamic_attr(element, "id") {
                        result = result.and(MatchResult::MaybeMatches);
                    } else {
                        return MatchResult::NoMatch;
                    }
                }
            }
            None => {
                if has_dynamic_attr(element, "id") {
                    result = result.and(MatchResult::MaybeMatches);
                } else {
                    return MatchResult::NoMatch;
                }
            }
        }
    }

    // Class selectors
    let static_classes: Vec<&str> = element.static_classes().collect();
    for class in &compound.classes {
        if static_classes.contains(&class.as_str()) {
            // Matched statically — this class is always present
            continue;
        }
        // Check extracted dynamic class names (conditional match)
        if element.dynamic_classes.iter().any(|dc| dc == class) {
            // Class found in :class object syntax — it's conditional
            result = result.and(MatchResult::MaybeMatches);
        } else if has_dynamic_class(element) {
            // Dynamic :class present but this class isn't in extracted names.
            // Conservative: still MaybeMatches (extraction may be incomplete).
            result = result.and(MatchResult::MaybeMatches);
        } else {
            return MatchResult::NoMatch;
        }
    }

    // Attribute selectors
    for attr_sel in &compound.attributes {
        let attr_match = match_attribute_selector(attr_sel, element);
        result = result.and(attr_match);
        if result == MatchResult::NoMatch {
            return MatchResult::NoMatch;
        }
    }

    // Pseudo-classes
    for pseudo in &compound.pseudo_classes {
        match pseudo {
            SelectorPseudoClass::Not(inner) => {
                // :not() — none of the inner selectors should match
                let mut inner_result = MatchResult::NoMatch;
                for inner_sel in inner {
                    let m = match_selector(inner_sel, element_index, elements);
                    inner_result = inner_result.or(m);
                }
                result = result.and(inner_result.invert());
            }
            SelectorPseudoClass::Is(inner) | SelectorPseudoClass::Where(inner) => {
                // :is()/:where() — at least one inner selector should match
                let mut inner_result = MatchResult::NoMatch;
                for inner_sel in inner {
                    let m = match_selector(inner_sel, element_index, elements);
                    inner_result = inner_result.or(m);
                    if inner_result == MatchResult::Matches {
                        break;
                    }
                }
                result = result.and(inner_result);
            }
            SelectorPseudoClass::Runtime(_) => {
                // Runtime pseudo-classes (:hover, :focus, etc.) don't prevent matching
                // They're state-based, so the element CAN match when in that state
            }
        }
        if result == MatchResult::NoMatch {
            return MatchResult::NoMatch;
        }
    }

    result
}

/// Match an attribute selector against an element's attributes.
fn match_attribute_selector(
    attr_sel: &crate::style::AttributeSelector,
    element: &TemplateElement,
) -> MatchResult {
    // Find matching attribute
    let static_attr = element
        .attributes
        .iter()
        .find(|a| !a.is_dynamic && a.name == attr_sel.name);

    let dynamic_attr = element
        .attributes
        .iter()
        .find(|a| a.is_dynamic && a.name == attr_sel.name);

    if let Some(attr) = static_attr {
        // Presence-only selector
        if attr_sel.operator.is_none() {
            return MatchResult::Matches;
        }

        // Value comparison
        if let (Some(op), Some(expected), Some(actual)) =
            (&attr_sel.operator, &attr_sel.value, &attr.value)
        {
            let matches = match op {
                AttributeOperator::Equal => actual == expected,
                AttributeOperator::Includes => actual.split_whitespace().any(|w| w == expected),
                AttributeOperator::DashMatch => {
                    actual == expected || actual.starts_with(&format!("{expected}-"))
                }
                AttributeOperator::Prefix => actual.starts_with(expected.as_str()),
                AttributeOperator::Suffix => actual.ends_with(expected.as_str()),
                AttributeOperator::Substring => actual.contains(expected.as_str()),
            };
            return if matches {
                MatchResult::Matches
            } else {
                MatchResult::NoMatch
            };
        }

        // Attribute exists but no value to compare
        return MatchResult::Matches;
    }

    if dynamic_attr.is_some() {
        // Dynamic attribute — can't evaluate the expression
        return MatchResult::MaybeMatches;
    }

    MatchResult::NoMatch
}

/// Check if an element has a dynamic `:class` binding.
fn has_dynamic_class(element: &TemplateElement) -> bool {
    element
        .attributes
        .iter()
        .any(|a| a.is_dynamic && a.name == "class")
        || element
            .directives
            .iter()
            .any(|d| d.name == "bind" && d.argument.as_deref() == Some("class"))
}

/// Check if an element has a dynamic binding for a specific attribute.
fn has_dynamic_attr(element: &TemplateElement, name: &str) -> bool {
    element
        .attributes
        .iter()
        .any(|a| a.is_dynamic && a.name == name)
        || element
            .directives
            .iter()
            .any(|d| d.name == "bind" && d.argument.as_deref() == Some(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::{TemplateAttribute, TemplateElement};
    use verter_span::Span;

    fn make_element(tag: &str, classes: &str, id: Option<&str>) -> TemplateElement {
        let mut attributes = Vec::new();
        if !classes.is_empty() {
            attributes.push(TemplateAttribute {
                name: "class".to_string(),
                value: Some(classes.to_string()),
                is_dynamic: false,
                span: Span::new(0, 0),
                name_end: 0,
                value_span: None,
            });
        }
        if let Some(id_val) = id {
            attributes.push(TemplateAttribute {
                name: "id".to_string(),
                value: Some(id_val.to_string()),
                is_dynamic: false,
                span: Span::new(0, 0),
                name_end: 0,
                value_span: None,
            });
        }
        TemplateElement {
            tag: tag.to_string(),
            attributes,
            parent_index: None,
            content_end: 0,
            ..Default::default()
        }
    }

    fn make_element_with_parent(
        tag: &str,
        classes: &str,
        parent_index: Option<u32>,
    ) -> TemplateElement {
        let mut el = make_element(tag, classes, None);
        el.parent_index = parent_index;
        el
    }

    fn parse(s: &str) -> StructuredSelector {
        crate::style::parse_selector(s).unwrap()
    }

    /// @ai-generated - Simple class selector matches element with that class
    #[test]
    fn test_match_simple_class() {
        let elements = vec![make_element("div", "btn active", None)];
        let sel = parse(".btn");
        assert_eq!(match_selector(&sel, 0, &elements), MatchResult::Matches);
    }

    /// @ai-generated - Class selector doesn't match element without that class
    #[test]
    fn test_match_class_no_match() {
        let elements = vec![make_element("div", "btn", None)];
        let sel = parse(".active");
        assert_eq!(match_selector(&sel, 0, &elements), MatchResult::NoMatch);
    }

    /// @ai-generated - Compound class selector (.foo.bar) requires both classes
    #[test]
    fn test_match_compound_class() {
        let elements = vec![make_element("div", "foo bar", None)];
        let sel = parse(".foo.bar");
        assert_eq!(match_selector(&sel, 0, &elements), MatchResult::Matches);
    }

    /// @ai-generated - Compound class fails when one class is missing
    #[test]
    fn test_match_compound_class_partial() {
        let elements = vec![make_element("div", "foo", None)];
        let sel = parse(".foo.bar");
        assert_eq!(match_selector(&sel, 0, &elements), MatchResult::NoMatch);
    }

    /// @ai-generated - Type + class selector (div.active)
    #[test]
    fn test_match_type_and_class() {
        let elements = vec![make_element("div", "active", None)];
        let sel = parse("div.active");
        assert_eq!(match_selector(&sel, 0, &elements), MatchResult::Matches);

        // Wrong type
        let elements = vec![make_element("span", "active", None)];
        assert_eq!(match_selector(&sel, 0, &elements), MatchResult::NoMatch);
    }

    /// @ai-generated - ID selector matches
    #[test]
    fn test_match_id() {
        let elements = vec![make_element("div", "", Some("app"))];
        let sel = parse("#app");
        assert_eq!(match_selector(&sel, 0, &elements), MatchResult::Matches);
    }

    /// @ai-generated - Child combinator (parent > child)
    #[test]
    fn test_match_child_combinator() {
        let elements = vec![
            make_element_with_parent("div", "parent", None),
            make_element_with_parent("span", "child", Some(0)),
        ];
        let sel = parse(".parent > .child");
        assert_eq!(match_selector(&sel, 1, &elements), MatchResult::Matches);
    }

    /// @ai-generated - Child combinator fails when parent doesn't match
    #[test]
    fn test_match_child_combinator_no_match() {
        let elements = vec![
            make_element_with_parent("div", "other", None),
            make_element_with_parent("span", "child", Some(0)),
        ];
        let sel = parse(".parent > .child");
        assert_eq!(match_selector(&sel, 1, &elements), MatchResult::NoMatch);
    }

    /// @ai-generated - Descendant combinator (ancestor descendant)
    #[test]
    fn test_match_descendant_combinator() {
        let elements = vec![
            make_element_with_parent("div", "ancestor", None),
            make_element_with_parent("section", "", Some(0)),
            make_element_with_parent("p", "target", Some(1)),
        ];
        let sel = parse(".ancestor .target");
        assert_eq!(match_selector(&sel, 2, &elements), MatchResult::Matches);
    }

    /// @ai-generated - Dynamic class produces MaybeMatches
    #[test]
    fn test_match_dynamic_class() {
        let elements = vec![{
            let mut el = make_element("div", "", None);
            el.attributes.push(TemplateAttribute {
                name: "class".to_string(),
                value: Some("expr".to_string()),
                is_dynamic: true,
                span: Span::new(0, 0),
                name_end: 0,
                value_span: None,
            });
            el
        }];
        let sel = parse(".active");
        assert_eq!(
            match_selector(&sel, 0, &elements),
            MatchResult::MaybeMatches
        );
    }

    /// @ai-generated - :not() pseudo-class
    #[test]
    fn test_match_not() {
        let elements = vec![make_element("div", "visible", None)];

        // :not(.hidden) should match element that doesn't have .hidden
        let sel = parse(":not(.hidden)");
        assert_eq!(match_selector(&sel, 0, &elements), MatchResult::Matches);

        // :not(.visible) should NOT match element that has .visible
        let sel = parse(":not(.visible)");
        assert_eq!(match_selector(&sel, 0, &elements), MatchResult::NoMatch);
    }

    /// @ai-generated - :is() pseudo-class
    #[test]
    fn test_match_is() {
        let elements = vec![make_element("div", "foo", None)];
        let sel = parse(":is(.foo, .bar)");
        assert_eq!(match_selector(&sel, 0, &elements), MatchResult::Matches);
    }

    /// @ai-generated - Attribute selector matches
    #[test]
    fn test_match_attribute() {
        let elements = vec![{
            let mut el = make_element("input", "", None);
            el.attributes.push(TemplateAttribute {
                name: "type".to_string(),
                value: Some("text".to_string()),
                is_dynamic: false,
                span: Span::new(0, 0),
                name_end: 0,
                value_span: None,
            });
            el
        }];
        let sel = parse("[type=\"text\"]");
        assert_eq!(match_selector(&sel, 0, &elements), MatchResult::Matches);
    }

    /// @ai-generated - Component elements produce MaybeMatches for type selectors
    #[test]
    fn test_match_component_type() {
        let elements = vec![{
            let mut el = make_element("MyComponent", "", None);
            el.is_component = true;
            el
        }];
        let sel = parse("div.foo");
        // Component doesn't have the class, so NoMatch wins
        assert_eq!(match_selector(&sel, 0, &elements), MatchResult::NoMatch);

        // With the right class
        let elements = vec![{
            let mut el = make_element("MyComponent", "foo", None);
            el.is_component = true;
            el
        }];
        assert_eq!(
            match_selector(&sel, 0, &elements),
            MatchResult::MaybeMatches
        );
    }

    /// @ai-generated - Universal selector (*) matches anything
    #[test]
    fn test_match_universal() {
        let elements = vec![make_element("div", "", None)];
        let sel = parse("*");
        assert_eq!(match_selector(&sel, 0, &elements), MatchResult::Matches);
    }

    /// @ai-generated - Sibling combinator (+)
    #[test]
    fn test_match_adjacent_sibling() {
        let elements = vec![
            make_element_with_parent("div", "first", None),
            make_element_with_parent("div", "second", None),
        ];
        let sel = parse(".first + .second");
        assert_eq!(match_selector(&sel, 1, &elements), MatchResult::Matches);
    }

    /// @ai-generated - General sibling combinator (~)
    #[test]
    fn test_match_general_sibling() {
        let elements = vec![
            make_element_with_parent("div", "first", None),
            make_element_with_parent("div", "middle", None),
            make_element_with_parent("div", "target", None),
        ];
        let sel = parse(".first ~ .target");
        assert_eq!(match_selector(&sel, 2, &elements), MatchResult::Matches);
    }

    /// @ai-generated - Attribute presence selector [disabled]
    #[test]
    fn test_match_attribute_presence() {
        let elements = vec![{
            let mut el = make_element("input", "", None);
            el.attributes.push(TemplateAttribute {
                name: "disabled".to_string(),
                value: None,
                is_dynamic: false,
                span: Span::new(0, 0),
                name_end: 0,
                value_span: None,
            });
            el
        }];
        let sel = parse("[disabled]");
        assert_eq!(match_selector(&sel, 0, &elements), MatchResult::Matches);
    }

    /// @ai-generated - Runtime pseudo-classes don't prevent matching
    #[test]
    fn test_match_runtime_pseudo() {
        let elements = vec![make_element("div", "btn", None)];
        let sel = parse(".btn:hover");
        assert_eq!(match_selector(&sel, 0, &elements), MatchResult::Matches);
    }
}
