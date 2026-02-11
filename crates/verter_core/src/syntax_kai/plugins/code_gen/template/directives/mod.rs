use rustc_hash::FxHashMap;

use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        binding_types::BindingType,
        plugin::SyntaxPluginContext,
        plugins::code_gen::{template::helper::patch_bindings, types::TemplateImportDependencies},
        types::ElementScope,
    },
};

use super::{ScopeClose, StateStack};

/// Process structural scope directives on an element (v-if, v-else-if, v-else, v-for).
///
/// This emits "opening" code before the element VNode call and stores
/// corresponding close tokens on `state.pending_scope_closes` for the close phase.
/// All required imports are registered here — the close phase only emits stored strings.
///
/// **v-once** is handled separately in `mod.rs` (uses cache helpers on the plugin).
/// Process structural scope directives on an element.
///
/// Performs all scope-related mutations (remove directive from source, patch bindings,
/// set `is_block_root`, push scope close tokens) but does NOT emit the scope open
/// text at the element's start position. Instead, returns the scope prefix text as
/// a `String` to be stored in the parent's `ChildInfo.scope_prefix`.
///
/// The parent's close phase emits the scope prefix as part of its single
/// `prepend_left` call (separator + scope_prefix + content_prefix), ensuring
/// correct ordering despite CodeTransform's FIFO semantics.
///
/// **Exception**: v-else-if emits its condition directly via `prepend_left` because
/// v-else-if elements are NOT registered as parent children (they're continuations
/// of the v-if chain), so there's no separator FIFO conflict.
pub(crate) fn process_scope_opens<'alloc>(
    code_transform: &mut CodeTransform<'alloc>,
    scopes: &[ElementScope<'alloc>],
    ctx: &SyntaxPluginContext<'alloc>,
    bindings: &FxHashMap<&'alloc str, BindingType>,
    is_production: bool,
    state: &mut StateStack,
    imports: &mut TemplateImportDependencies,
) -> String {
    let element_start = state.id;
    let mut scope_prefix = String::new();

    for scope in scopes {
        match scope {
            ElementScope::If(cond) => {
                state.has_condition = true;
                state.is_block_root = true;

                patch_bindings(code_transform, &cond.bindings, bindings, is_production);
                code_transform.remove(cond.event.start, cond.event.end);

                let condition = if let Some(val) = cond.event.value {
                    &ctx.input[val.start as usize..val.end as usize]
                } else {
                    "true"
                };

                // Store prefix — emitted by parent's close phase
                scope_prefix.push_str(&format!("({}) ? ", condition));

                state.pending_scope_closes.push(ScopeClose::IfTernary);
            }

            ElementScope::ElseIf(cond) => {
                state.has_condition = true;
                state.is_block_root = true;

                patch_bindings(code_transform, &cond.bindings, bindings, is_production);
                code_transform.remove(cond.event.start, cond.event.end);

                let condition = if let Some(val) = cond.event.value {
                    &ctx.input[val.start as usize..val.end as usize]
                } else {
                    "true"
                };

                // v-else-if is NOT a child — emit directly (no separator conflict).
                // The ` : ` from the previous v-if/v-else-if close already transitions here.
                code_transform.prepend_left(element_start, &format!("({}) ? ", condition));

                state.pending_scope_closes.push(ScopeClose::ElseIfTernary);
            }

            ElementScope::Else(cond) => {
                state.is_block_root = true;
                code_transform.remove(cond.event.start, cond.event.end);
                // No prefix — the ` : ` from previous close transitions to this branch.
                // Block root provides grouping via `(_openBlock(), ...)`.
                state.pending_scope_closes.push(ScopeClose::Else);
            }

            ElementScope::For(vfor) => {
                state.is_block_root = true;
                code_transform.remove(vfor.event.start, vfor.event.end);

                let iterable = if let Some(val) = vfor.event.value {
                    &ctx.input[val.start as usize..val.end as usize]
                } else {
                    "[]"
                };

                let params = if vfor.parsed.locals.is_empty() {
                    "_item".to_string()
                } else {
                    vfor.parsed
                        .locals
                        .iter()
                        .map(|span| &ctx.input[span.start as usize..span.end as usize])
                        .collect::<Vec<_>>()
                        .join(", ")
                };

                // Store prefix — emitted by parent's close phase
                scope_prefix.push_str(&format!(
                    "(_openBlock(true), _createElementBlock(_Fragment, null, _renderList({}, ({}) => {{return ",
                    iterable, params
                ));

                imports.add(TemplateImportDependencies::OPEN_BLOCK);
                imports.add(TemplateImportDependencies::FRAGMENT);
                imports.add(TemplateImportDependencies::RENDER_LIST);
                imports.add(TemplateImportDependencies::CREATE_ELEMENT_BLOCK);

                state.pending_scope_closes.push(ScopeClose::For);
            }

            ElementScope::Once(_)
            | ElementScope::SlotElement(_)
            | ElementScope::SlotTemplate(_) => {}
        }
    }

    scope_prefix
}

/// Emit stored scope close strings at the given position.
///
/// For v-if/v-else-if: only appends ` : ` (the ternary separator). The comment
/// fallback (`_createCommentVNode("v-if", true)`) is NOT emitted here — it's
/// deferred to the parent's close phase via `pending_vif_fallbacks`. This allows
/// v-else-if/v-else to consume the pending fallback instead of emitting a comment.
///
/// For v-else: appends nothing (the block root's closing `)` is handled by
/// `handle_element_close`).
///
/// Returns `true` if a v-if or v-else-if ternary close was emitted (meaning
/// the parent should store a pending fallback position).
pub(crate) fn process_scope_closes(
    code_transform: &mut CodeTransform,
    pending_closes: &[ScopeClose],
    position: u32,
    is_production: bool,
) -> bool {
    let mut had_vif_close = false;

    // Emit in reverse order (innermost scope closes first)
    for close in pending_closes.iter().rev() {
        match close {
            ScopeClose::IfTernary | ScopeClose::ElseIfTernary => {
                // Just the ternary separator — comment fallback is deferred.
                // Uses append_left so it appears BEFORE the next chunk (sibling/parent close)
                // but AFTER the element's close tag overwrite.
                code_transform.append_left(position, " : ");
                had_vif_close = true;
            }
            ScopeClose::Else => {
                // Nothing — block root's `)` from handle_element_close is sufficient
            }
            ScopeClose::For => {
                if is_production {
                    code_transform.append_left(position, "}), 128))");
                } else {
                    code_transform.append_left(position, "}), 128 /* UNKEYED_FRAGMENT */))");
                }
            }
        }
    }

    had_vif_close
}
