//! The F11 store auto-subscription rewrite — a continuation of the Svelte IDE
//! projector, extracted for file size.
//!
//! A classified [`StoreSub`] (from `store_scan`) is rewritten through the F11
//! prelude helpers (`__verter_store_get` / `__verter_store_set`), mutating ONLY
//! the `$`-byte / operator spans so the original `store` identifier / RHS bytes
//! keep their source span (hover on a rewritten `$store` lands on the original
//! identifier). The whole rewrite goes through `CodeTransform` ops — never a
//! post-hoc string edit.

use verter_span::Span;

use crate::code_transform::CodeTransform;

use super::super::store_scan::{
    scan_pattern_default_store_subs, scan_store_subscriptions_with, StoreSub, StoreSubKind,
};
use super::TemplateProjector;

impl TemplateProjector<'_, '_> {
    /// Rewrite every F11 store auto-subscription within an expression `span` (a
    /// markup expression that stays an Original chunk — a plain attribute value, a
    /// block-head condition / iterable / key expression, a tag inner, a directive
    /// value). The store rewrite touches ONLY the interior `$` / operator bytes,
    /// composing with any boundary overwrite the caller applies to the same span.
    /// The script-declared `$`-names are consulted so a markup `$x` that refers to
    /// a script-declared local stays an ordinary reference.
    pub(super) fn rewrite_store_subs_in(&mut self, span: Span) {
        let source = self.source;
        let body = &source[span.start as usize..span.end as usize];
        let declared = self.declared_dollar_names();
        for sub in scan_store_subscriptions_with(body, &declared) {
            rewrite_store_sub(self.ct, span.start, &sub);
        }
    }

    /// Rewrite every F11 store auto-subscription READ inside the DEFAULT-VALUE
    /// expressions of a block-binding PATTERN TEXT (`{ x = $store }` / `$item =
    /// $store`), returning the rewritten text. Used for the SLICED-into-synthesised
    /// patterns (`{#each}` item / snippet params / `{:then}`/`{:catch}` binding)
    /// whose bytes are copied into a projection string, not kept as a mapped
    /// chunk. The bound names are binding identifiers (never references) and stay
    /// untouched; only the default initializers (read contexts) become
    /// `__verter_store_get(store)`. Runs the same `rewrite_store_sub` ops over a
    /// throwaway transform on the text (byte-identical to the mapped mode), so a
    /// nested store read inside a default has no offset / overlap hazard.
    pub(super) fn rewrite_pattern_default_store_subs_text(
        &self,
        pattern_text: &str,
        declared: &[String],
    ) -> String {
        let subs = scan_pattern_default_store_subs(pattern_text, declared);
        if subs.is_empty() {
            return pattern_text.to_string();
        }
        let allocator = oxc_allocator::Allocator::default();
        let mut ct = CodeTransform::new(pattern_text, &allocator);
        for sub in &subs {
            rewrite_store_sub(&mut ct, 0, sub);
        }
        ct.build_string()
    }

    /// The full set of `$`-names the current scan must treat as ORDINARY locals:
    /// the component SCRIPT's top-level bindings UNIONED with every enclosing
    /// markup-block binding (`{#each … as $item}` / `{:then $v}` / `{:catch $e}` /
    /// `{#snippet n($p)}` / `let:$prop`) currently in scope (the `block_declared`
    /// stack). A `$`-named block binding lexically scopes to its block subtree
    /// only, so the projector pushes its names while projecting that subtree and
    /// pops them after — a `$`-binding does not leak to a sibling block.
    pub(super) fn declared_dollar_names(&self) -> Vec<String> {
        if self.block_declared.is_empty() {
            return self.script_declared.clone();
        }
        let mut declared = self.script_declared.clone();
        for frame in &self.block_declared {
            declared.extend(frame.iter().cloned());
        }
        declared
    }

    /// Rewrite every F11 store auto-subscription in `text` (a slice that the
    /// caller re-emits as TEXT, NOT as a mapped chunk — e.g. the
    /// `<svelte:component this={$store}>` value, which the F8 dynamic-component
    /// IIFE interpolates into `__verter_dynamic_component((…))`). Because the
    /// caller already re-slices the bytes into a synthesised overwrite (the
    /// original `this` position carries no independent mapped chunk), the store
    /// rewrite is applied to the text and returned for the caller's single
    /// CodeTransform overwrite — no second transform op on the original bytes. The
    /// script-declared `$`-names are consulted so a script-declared local stays an
    /// ordinary reference.
    pub(super) fn rewrite_store_subs_in_text(&self, text: &str) -> String {
        rewrite_store_subs_text(text, &self.declared_dollar_names())
    }
}

/// Apply every classified store-sub rewrite to `text`, returning the rewritten
/// fragment. Used ONLY for re-emitted-as-text values (the F8 dynamic-component
/// `this` expression, the hoisted store-bearing `{@const}`/`{@let}` inner) where
/// the bytes are NOT kept as an independently-mapped Original chunk — the caller
/// re-slices them into a synthesised overwrite / a text emission at a hoist
/// anchor. The rewrite runs the SAME `rewrite_store_sub` CodeTransform ops over a
/// throwaway transform on `text`, then `build_string()` — so the text mode is
/// byte-identical to the mapped mode (no hand-rolled offset arithmetic, no
/// overlap hazard for a nested store read inside a store write).
fn rewrite_store_subs_text(text: &str, declared: &[String]) -> String {
    let subs = scan_store_subscriptions_with(text, declared);
    if subs.is_empty() {
        return text.to_string();
    }
    let allocator = oxc_allocator::Allocator::default();
    let mut ct = CodeTransform::new(text, &allocator);
    for sub in &subs {
        rewrite_store_sub(&mut ct, 0, sub);
    }
    ct.build_string()
}

/// Rewrite ONE classified store auto-subscription (F11) through the F11 prelude
/// helpers. The PRIMARY identifier occurrence keeps its source span (only the
/// `$`-byte / operator spans are overwritten), so hover on the rewritten
/// `$store` lands on the original `store` identifier bytes. `base` is the
/// absolute source offset of the scanned fragment (the script content start, or
/// the markup expression start) — `sub`'s offsets are relative to it.
///
///   READ      `$store`         → `__verter_store_get(store)`.
///   WRITE     `$store = rhs`    → `__verter_store_set(store, rhs)`.
///   COMPOUND  `$store += rhs`   → `__verter_store_set(store, __verter_store_get(store) + (rhs))`.
///   UPDATE    `$store++`        → `__verter_store_set(store, __verter_store_update(__verter_store_get(store)))`.
///
/// For the compound/update forms the store NAME appears twice; the original
/// `store` identifier occurrence keeps its source span, and the second (injected)
/// occurrence is unmapped text (it is generated read machinery, not a user
/// token, so it carries no source position — the primary occurrence is the one
/// hover/go-to resolves).
pub(super) fn rewrite_store_sub(ct: &mut CodeTransform, base: u32, sub: &StoreSub) {
    let dollar = base + sub.dollar;
    let ident_end = base + sub.ident_end;
    let name = sub.name.as_str();
    match &sub.kind {
        StoreSubKind::Read => {
            // `$store` → `__verter_store_get(store)`.
            ct.overwrite(dollar, dollar + 1, "__verter_store_get(");
            ct.append_left(ident_end, ")");
        }
        StoreSubKind::ShorthandRead => {
            // `{ $store }` → `{ $store: __verter_store_get(store) }`. The `$store`
            // key bytes stay (a valid identifier key), and the value side becomes
            // the store-get — a bare `__verter_store_get(store)` would be invalid in
            // a shorthand slot. The injected `store` (in `get(store)`) is unmapped
            // read machinery; the key `$store` keeps its source span.
            ct.append_left(ident_end, &format!(": __verter_store_get({name})"));
        }
        StoreSubKind::LvalueWrite => {
            // `$store` (destructuring / `for`-of WRITE LEAF) →
            // `__verter_store_lvalue(store).value`. The `$` byte opens the helper
            // call; `.value` is appended past the original `store` identifier so
            // the leaf becomes a valid assignment-target member access referencing
            // only `store`. The original `store` identifier keeps its source span.
            ct.overwrite(dollar, dollar + 1, "__verter_store_lvalue(");
            ct.append_left(ident_end, ").value");
        }
        StoreSubKind::ShorthandLvalueWrite => {
            // `({ $store } = obj)` → `({ $store: __verter_store_lvalue(store).value
            // } = obj)`. The `$store` key bytes stay (a valid identifier key), the
            // value side becomes the writable lvalue — a bare
            // `__verter_store_lvalue(store).value` would be invalid in a shorthand
            // slot. The injected `store` is unmapped read machinery; the key
            // `$store` keeps its source span.
            ct.append_left(ident_end, &format!(": __verter_store_lvalue({name}).value"));
        }
        StoreSubKind::SimpleWrite {
            eq,
            eq_end,
            rhs_end,
        } => {
            // `$store = rhs` → `__verter_store_set(store, rhs)`.
            ct.overwrite(dollar, dollar + 1, "__verter_store_set(");
            ct.overwrite(base + eq, base + eq_end, ",");
            ct.append_left(base + rhs_end, ")");
        }
        StoreSubKind::CompoundWrite {
            op_base,
            op,
            op_end,
            rhs_end,
        } => {
            // `$store OP= rhs` →
            // `__verter_store_set(store, __verter_store_get(store) OP_BASE (rhs))`.
            // The leading `$` opens the set call; the original `store` is the
            // set's first arg; the compound operator becomes `, __verter_store_get(
            // store) OP_BASE (` (re-reading the store via the injected duplicate);
            // the RHS closes with `))`.
            ct.overwrite(dollar, dollar + 1, "__verter_store_set(");
            ct.overwrite(
                base + op,
                base + op_end,
                &format!(", __verter_store_get({name}) {op_base} ("),
            );
            ct.append_left(base + rhs_end, "))");
        }
        StoreSubKind::Update {
            op, op_end, prefix, ..
        } => {
            // `$store++` / `--$store` →
            // `__verter_store_set(store, __verter_store_update(__verter_store_get(
            // store)))`. The `__verter_store_update<T extends number | bigint>`
            // helper enforces the exact `++`/`--` operand constraint while
            // preserving the value type (so a `bigint` store passes and a
            // `string`/`boolean` store FAILS — a plain `get(store) + 1` would
            // mis-judge both). The `$` opens the set call; `store` is the set's
            // first arg; the `++`/`--` span becomes the update-wrap + close.
            let update_tail = format!(", __verter_store_update(__verter_store_get({name})))");
            if *prefix {
                // `++$store` — the `op` span [op,op_end) precedes `$store`.
                ct.overwrite(base + op, base + op_end, "");
                ct.overwrite(dollar, dollar + 1, "__verter_store_set(");
                ct.append_left(ident_end, &update_tail);
            } else {
                // `$store++` — the `op` span [op,op_end) follows the identifier.
                ct.overwrite(dollar, dollar + 1, "__verter_store_set(");
                ct.overwrite(base + op, base + op_end, &update_tail);
            }
        }
    }
}
