use rustc_hash::FxHashMap;

use crate::{
    code_transform::CodeTransform,
    syntax::{
        binding_types::BindingType,
        plugin::SyntaxPluginContext,
        plugins::code_gen::{
            template::shared::helper::{build_prefixed_value_into, prefix_vfor_references_into},
            types::TemplateImportDependencies,
        },
        types::ElementScope,
    },
};

use super::{ScopeClose, StateStack};

/// Process structural scope directives on an element (v-if, v-else-if, v-else, v-for).
///
/// Performs all scope-related mutations (set `is_block_root`, push scope close tokens)
/// and returns the scope prefix text as `&'alloc str` for the parent's `ChildInfo.scope_prefix`.
///
/// Uses the shared `buf` to build the scope prefix. The caller's buf content is preserved
/// via save/truncate — the function only appends temporarily and alloc_strs the result.
///
/// **Exception**: v-else-if emits its condition directly via `prepend_left` because
/// v-else-if elements are NOT registered as parent children (they're continuations
/// of the v-if chain), so there's no separator FIFO conflict.
#[allow(clippy::too_many_arguments)]
pub(crate) fn process_scope_opens<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    scopes: &[ElementScope<'alloc>],
    ctx: &SyntaxPluginContext<'alloc>,
    bindings: &FxHashMap<&'alloc str, BindingType>,
    is_production: bool,
    state: &mut StateStack<'alloc>,
    imports: &mut TemplateImportDependencies,
    parent_vif_key_counter: &mut u32,
    pending_prepend_lefts: &mut Vec<(u32, &'alloc str)>,
    buf: &mut String,
) -> &'alloc str {
    let element_start = state.id;
    let prefix_start = buf.len();

    for scope in scopes {
        match scope {
            ElementScope::If(cond) => {
                state.is_block_root = true;

                *parent_vif_key_counter = 0;
                state.vif_branch_key = Some(*parent_vif_key_counter);
                *parent_vif_key_counter += 1;

                buf.push('(');
                if let Some(val) = cond.event.value {
                    let val_text = &ctx.input[val.start as usize..val.end as usize];
                    build_prefixed_value_into(
                        buf,
                        val_text,
                        val.start,
                        cond.bindings.as_ref(),
                        bindings,
                        is_production,
                        &[],
                    );
                } else {
                    buf.push_str("true");
                }
                buf.push_str(") ? ");

                state.pending_scope_closes.push(ScopeClose::IfTernary);
            }

            ElementScope::ElseIf(cond) => {
                state.is_block_root = true;

                state.vif_branch_key = Some(*parent_vif_key_counter);
                *parent_vif_key_counter += 1;

                // v-else-if emits directly — use save/truncate to preserve scope_prefix content.
                let saved = buf.len();
                buf.push('(');
                if let Some(val) = cond.event.value {
                    let val_text = &ctx.input[val.start as usize..val.end as usize];
                    build_prefixed_value_into(
                        buf,
                        val_text,
                        val.start,
                        cond.bindings.as_ref(),
                        bindings,
                        is_production,
                        &[],
                    );
                } else {
                    buf.push_str("true");
                }
                buf.push_str(") ? ");
                let s = code_transform.alloc_str(&buf[saved..]);
                buf.truncate(saved);
                pending_prepend_lefts.push((element_start, s));

                state.pending_scope_closes.push(ScopeClose::ElseIfTernary);
            }

            ElementScope::Else(_cond) => {
                state.is_block_root = true;

                state.vif_branch_key = Some(*parent_vif_key_counter);
                *parent_vif_key_counter += 1;

                state.pending_scope_closes.push(ScopeClose::Else);
            }

            ElementScope::For(vfor) => {
                state.is_block_root = true;

                buf.push_str(
                    "(_openBlock(true), _createElementBlock(_Fragment, null, _renderList(",
                );
                if let Some(val) = vfor.event.value {
                    // Use only the right side (the iterable) of the v-for expression.
                    // e.g., for "(item, index) in items", emit only "items", not the full expression.
                    let right_offset = vfor.parsed.result.right_offset;
                    let right_text = &ctx.input[right_offset as usize..val.end as usize];
                    prefix_vfor_references_into(
                        buf,
                        right_text,
                        right_offset,
                        &vfor.parsed.references,
                        None,
                        ctx.input,
                        bindings,
                        is_production,
                    );
                } else {
                    buf.push_str("[]");
                }
                buf.push_str(", (");
                if vfor.parsed.locals.is_empty() {
                    buf.push_str("_item");
                } else {
                    for (i, span) in vfor.parsed.locals.iter().enumerate() {
                        if i > 0 {
                            buf.push_str(", ");
                        }
                        buf.push_str(&ctx.input[span.start as usize..span.end as usize]);
                    }
                }
                buf.push_str(") => {return ");

                imports.add(TemplateImportDependencies::OPEN_BLOCK);
                imports.add(TemplateImportDependencies::FRAGMENT);
                imports.add(TemplateImportDependencies::RENDER_LIST);
                imports.add(TemplateImportDependencies::CREATE_ELEMENT_BLOCK);

                state
                    .pending_scope_closes
                    .push(ScopeClose::For { is_keyed: false });
            }

            ElementScope::Once(_)
            | ElementScope::SlotElement(_)
            | ElementScope::SlotTemplate(_) => {}
        }
    }

    if buf.len() > prefix_start {
        let s = code_transform.alloc_str(&buf[prefix_start..]);
        buf.truncate(prefix_start);
        s
    } else {
        ""
    }
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
pub(crate) fn process_scope_closes<'alloc>(
    code_transform: &CodeTransform<'alloc>,
    pending_closes: &[ScopeClose],
    position: u32,
    is_production: bool,
    pending_append_lefts: &mut Vec<(u32, &'alloc str)>,
    buf: &mut String,
) -> bool {
    if pending_closes.is_empty() {
        return false;
    }

    let mut had_vif_close = false;

    // Fast path: single scope close (most common case — avoids buffer allocation)
    if pending_closes.len() == 1 {
        match &pending_closes[0] {
            ScopeClose::IfTernary | ScopeClose::ElseIfTernary => {
                pending_append_lefts.push((position, " : "));
                return true;
            }
            ScopeClose::Else => return false,
            ScopeClose::For { is_keyed } => {
                let s = match (is_keyed, is_production) {
                    (true, true) => "}), 128))",
                    (true, false) => "}), 128 /* KEYED_FRAGMENT */))",
                    (false, true) => "}), 256))",
                    (false, false) => "}), 256 /* UNKEYED_FRAGMENT */))",
                };
                pending_append_lefts.push((position, s));
                return false;
            }
        }
    }

    // Multi-scope: batch all closes into a single string using shared buf.
    let saved = buf.len();

    // Emit in reverse order (innermost scope closes first)
    for close in pending_closes.iter().rev() {
        match close {
            ScopeClose::IfTernary | ScopeClose::ElseIfTernary => {
                buf.push_str(" : ");
                had_vif_close = true;
            }
            ScopeClose::Else => {}
            ScopeClose::For { is_keyed } => {
                let s = match (is_keyed, is_production) {
                    (true, true) => "}), 128))",
                    (true, false) => "}), 128 /* KEYED_FRAGMENT */))",
                    (false, true) => "}), 256))",
                    (false, false) => "}), 256 /* UNKEYED_FRAGMENT */))",
                };
                buf.push_str(s);
            }
        }
    }

    if buf.len() > saved {
        let s = code_transform.alloc_str(&buf[saved..]);
        buf.truncate(saved);
        pending_append_lefts.push((position, s));
    }

    had_vif_close
}
