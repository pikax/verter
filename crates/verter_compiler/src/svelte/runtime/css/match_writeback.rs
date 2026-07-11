//! Deferred metadata write-back: apply the collected `used` / `scoped`
//! sink writes onto the CSS AST, span-keyed so official spread-copy
//! aliasing lands on the original node (see the `matcher` module docs).

use super::MatchSink;
use crate::svelte::runtime::css::types::{
    Atrule, Block, BlockChild, Rule, SelectorList, SimpleSelector, StyleChild,
};

/// Apply the collected `used` / `scoped` writes onto the AST metadata —
/// span-keyed, so official spread-copy aliasing lands on the original node.
pub(super) fn apply_sink_to_children(children: &mut [StyleChild], sink: &MatchSink) {
    for child in children {
        match child {
            StyleChild::Rule(rule) => apply_sink_to_rule(rule, sink),
            StyleChild::Atrule(atrule) => apply_sink_to_atrule(atrule, sink),
        }
    }
}

pub(super) fn apply_sink_to_atrule(atrule: &mut Atrule, sink: &MatchSink) {
    if let Some(block) = &mut atrule.block {
        apply_sink_to_block(block, sink);
    }
}

pub(super) fn apply_sink_to_block(block: &mut Block, sink: &MatchSink) {
    for child in &mut block.children {
        match child {
            BlockChild::Declaration(_) => {}
            BlockChild::Rule(rule) => apply_sink_to_rule(rule, sink),
            BlockChild::Atrule(atrule) => apply_sink_to_atrule(atrule, sink),
        }
    }
}

pub(super) fn apply_sink_to_rule(rule: &mut Rule, sink: &MatchSink) {
    apply_sink_to_selector_list(&mut rule.prelude, sink);
    apply_sink_to_block(&mut rule.block, sink);
}

pub(super) fn apply_sink_to_selector_list(list: &mut SelectorList, sink: &MatchSink) {
    for complex in &mut list.children {
        if sink.used_selectors.contains(&complex.span) {
            complex.metadata.used = true;
        }
        for relative in &mut complex.children {
            if sink.scoped_selectors.contains(&relative.span) {
                relative.metadata.scoped = true;
            }
            for simple in &mut relative.selectors {
                if let SimpleSelector::PseudoClass {
                    args: Some(args), ..
                } = simple
                {
                    apply_sink_to_selector_list(args, sink);
                }
            }
        }
    }
}
