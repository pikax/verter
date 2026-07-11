//! The official `get_relative_selectors` / `truncate` relative-selector
//! list construction: discard trailing `:global(...)`, reduce a
//! `:root...:has(...)` compound to its `:has` selectors, and prepend the
//! implicit `& ` (nesting + descendant) for a nested rule without an
//! explicit `&`. Pure AST transforms feeding the selector walk.

use std::borrow::Cow;

use super::{descendant_combinator, nesting_selector, RelView};
use crate::svelte::runtime::css::types::{ComplexSelector, SimpleSelector};

/// The official `get_relative_selectors(node)` — the truncated relative
/// selectors, with an implicit `& ` (nesting + descendant) prepended for a
/// nested rule without an explicit `&`.
pub(super) fn get_relative_selectors(
    complex: &ComplexSelector,
    rule_idx: usize,
) -> Vec<RelView<'_>> {
    let mut selectors = truncate(complex);

    // `node.metadata.rule?.metadata.parent_rule && selectors.length > 0`.
    if rule_idx >= 1 && !selectors.is_empty() {
        let mut has_explicit_nesting_selector = false;
        for selector in &selectors {
            if selectors_contain_nesting(&selector.as_ref().selectors) {
                has_explicit_nesting_selector = true;
                break;
            }
        }

        if !has_explicit_nesting_selector {
            if selectors[0].as_ref().combinator.is_none() {
                let mut owned = selectors[0].as_ref().clone();
                owned.combinator = Some(descendant_combinator());
                selectors[0] = Cow::Owned(owned);
            }
            selectors.insert(0, Cow::Owned(nesting_selector()));
        }
    }

    selectors
}

/// The official nesting-selector search (the zimmerframe `NestingSelector`
/// walk — recursive through pseudo-class argument lists).
pub(super) fn selectors_contain_nesting(selectors: &[SimpleSelector]) -> bool {
    for simple in selectors {
        match simple {
            SimpleSelector::Nesting { .. } => return true,
            SimpleSelector::PseudoClass {
                args: Some(args), ..
            } => {
                for complex in &args.children {
                    for relative in &complex.children {
                        if selectors_contain_nesting(&relative.selectors) {
                            return true;
                        }
                    }
                }
            }
            _ => {}
        }
    }
    false
}

/// The official `truncate(node)` — discard trailing `:global(...)` selectors,
/// and reduce a `:root...:has(...)` compound to its `:has` selectors.
pub(super) fn truncate(complex: &ComplexSelector) -> Vec<RelView<'_>> {
    let last_scoped = complex.children.iter().rposition(|child| {
        let first = child.selectors.first();
        let first_is_bare_global = matches!(
            first,
            Some(SimpleSelector::PseudoClass { name, args: None, .. }) if name == "global"
        );
        // Not after a `:global` selector, not a bare `:global`, not a
        // `:global(...)` without a scoped modifier.
        !child.metadata.is_global_like && !first_is_bare_global && !child.metadata.is_global
    });

    let upto = last_scoped.map_or(0, |i| i + 1);
    complex.children[..upto]
        .iter()
        .map(|child| {
            // In `:root.y:has(...)`, `y` is unscoped but the `:has(...)`
            // contents stay scoped — keep only the `:has` selectors.
            let has_root = child
                .selectors
                .iter()
                .any(|s| matches!(s, SimpleSelector::PseudoClass { name, .. } if name == "root"));
            if !has_root || child.metadata.is_global_like {
                return Cow::Borrowed(child);
            }
            let mut owned = child.clone();
            owned
                .selectors
                .retain(|s| matches!(s, SimpleSelector::PseudoClass { name, .. } if name == "has"));
            Cow::Owned(owned)
        })
        .collect()
}
