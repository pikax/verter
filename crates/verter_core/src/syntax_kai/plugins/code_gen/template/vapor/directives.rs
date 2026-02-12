//! Vapor structural directive processing: v-if, v-else-if, v-else, v-for, v-slot.

use crate::syntax_kai::{
    plugin::SyntaxPluginContext,
    plugins::code_gen::{
        template::shared::helper::{build_prefixed_value, prefix_vfor_references},
        types::VaporImportDependencies,
    },
    types::{ElementScope, OxcCompiledElementStart},
};

use super::types::{VaporElementKind, VaporElementState, VaporScopeKind, VaporVIfChainState};
use super::VaporTemplateGenerator;

impl<'alloc> VaporTemplateGenerator<'alloc> {
    /// Process structural directive scopes (v-if, v-else-if, v-else, v-for, v-slot).
    pub(super) fn process_scopes(
        &mut self,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
        state: &mut VaporElementState,
    ) {
        for scope in &ev.scopes {
            match scope {
                ElementScope::If(cond) => {
                    let condition = if let Some(ref val_span) = cond.event.value {
                        let raw = &ctx.input[val_span.start as usize..val_span.end as usize];
                        build_prefixed_value(
                            raw,
                            val_span.start,
                            &cond.bindings,
                            &self.bindings,
                            self.is_production,
                        )
                    } else {
                        "true".to_string()
                    };
                    state.scope = Some(VaporScopeKind::If { condition });
                }
                ElementScope::ElseIf(cond) => {
                    let condition = if let Some(ref val_span) = cond.event.value {
                        let raw = &ctx.input[val_span.start as usize..val_span.end as usize];
                        build_prefixed_value(
                            raw,
                            val_span.start,
                            &cond.bindings,
                            &self.bindings,
                            self.is_production,
                        )
                    } else {
                        "true".to_string()
                    };
                    state.scope = Some(VaporScopeKind::ElseIf { condition });
                }
                ElementScope::Else(_) => {
                    state.scope = Some(VaporScopeKind::Else);
                }
                ElementScope::For(vfor) => {
                    // The v-for value span contains the full expression: "item in items"
                    // We need to extract just the iterable (right side).
                    // `right_offset()` is already absolute (file-relative).
                    let val_span = vfor.event.value.as_ref();
                    let iterable = if let Some(val) = val_span {
                        let right_offset = vfor.parsed.right_offset();
                        let iterable_raw = &ctx.input[right_offset as usize..val.end as usize];
                        prefix_vfor_references(
                            iterable_raw,
                            right_offset,
                            &vfor.parsed.references,
                            Some((right_offset, val.end)),
                            ctx.input,
                            &self.bindings,
                            self.is_production,
                        )
                    } else {
                        "[]".to_string()
                    };

                    // Build callback parameter names.
                    let depth = self.for_depth;
                    let original_params: Vec<String> = vfor
                        .parsed
                        .locals
                        .iter()
                        .map(|span| ctx.input[span.start as usize..span.end as usize].to_string())
                        .collect();

                    let callback_params: Vec<String> = (0..original_params.len().max(1))
                        .map(|i| match i {
                            0 => format!("_for_item{}", depth),
                            1 => format!("_for_key{}", depth),
                            _ => format!("_for_index{}", depth),
                        })
                        .collect();

                    // Extract :key expression if present.
                    let key_fn = self.extract_key_fn(ev, ctx, &original_params);

                    state.scope = Some(VaporScopeKind::For {
                        iterable,
                        callback_params,
                        original_params,
                        key_fn,
                        depth,
                    });
                    self.for_depth += 1;
                }
                ElementScope::SlotElement(slot) => {
                    self.process_slot_scope(
                        slot.event.arg.as_ref(),
                        slot.event.has_dynamic_arg,
                        &slot.parsed.locals,
                        ctx,
                        state,
                    );
                }
                ElementScope::SlotTemplate(slot) => {
                    self.process_slot_scope(
                        slot.event.arg.as_ref(),
                        slot.event.has_dynamic_arg,
                        &slot.parsed.locals,
                        ctx,
                        state,
                    );
                }
                ElementScope::Once(_) => {
                    state.is_once = true;
                }
            }
        }
    }

    /// Shared logic for processing SlotElement and SlotTemplate scopes.
    fn process_slot_scope(
        &mut self,
        arg: Option<&crate::common::Span>,
        has_dynamic_arg: bool,
        locals: &[crate::common::Span],
        ctx: &SyntaxPluginContext<'alloc>,
        state: &mut VaporElementState,
    ) {
        let slot_name = if let Some(arg) = arg {
            let name = &ctx.input[arg.start as usize..arg.end as usize];
            name.to_string()
        } else {
            "default".to_string()
        };
        // Set slot name and params based on element kind.
        // v-slot can appear on both <template #name> (TemplateWrapper) and
        // directly on components like <MyComp v-slot="{ item }"> (Component).
        match &mut state.kind {
            VaporElementKind::TemplateWrapper {
                slot_name: ref mut sn,
                slot_name_is_dynamic: ref mut sn_dyn,
                slot_dynamic_name_expr: ref mut sn_dyn_expr,
                slot_params: ref mut sp,
            } => {
                *sn = Some(slot_name);
                *sn_dyn = has_dynamic_arg;
                if has_dynamic_arg {
                    if let Some(arg) = arg {
                        let name_expr = &ctx.input[arg.start as usize..arg.end as usize];
                        let prefixed = build_prefixed_value(
                            name_expr,
                            arg.start,
                            &None,
                            &self.bindings,
                            self.is_production,
                        );
                        *sn_dyn_expr = Some(prefixed);
                    }
                }
                if !locals.is_empty() {
                    let slot_props_var = format!("_slotProps{}", self.slot_props_counter);
                    self.slot_props_counter += 1;
                    *sp = Some(slot_props_var.clone());
                    for local_span in locals {
                        let local_name = ctx.input
                            [local_span.start as usize..local_span.end as usize]
                            .to_string();
                        let mapped = format!("{}.{}", slot_props_var, local_name);
                        state.for_var_mappings.push((local_name, mapped));
                    }
                }
            }
            VaporElementKind::Component {
                slot_name: ref mut sn,
                slot_params: ref mut sp,
                ..
            } => {
                *sn = Some(slot_name);
                if !locals.is_empty() {
                    let slot_props_var = format!("_slotProps{}", self.slot_props_counter);
                    self.slot_props_counter += 1;
                    *sp = Some(slot_props_var.clone());
                    for local_span in locals {
                        let local_name = ctx.input
                            [local_span.start as usize..local_span.end as usize]
                            .to_string();
                        let mapped = format!("{}.{}", slot_props_var, local_name);
                        state.for_var_mappings.push((local_name, mapped));
                    }
                }
            }
            _ => {}
        }
    }

    /// Extract `:key` expression from element props for v-for key function.
    pub(super) fn extract_key_fn(
        &self,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
        original_params: &[String],
    ) -> Option<String> {
        for oxc_prop in &ev.props {
            let prop = &oxc_prop.event;
            if prop.kind == crate::syntax_kai::types::PropKind::Bind {
                if let Some(ref arg) = prop.arg {
                    let attr_name = &ctx.input[arg.start as usize..arg.end as usize];
                    if attr_name == "key" {
                        if let Some(ref exp) = oxc_prop.exp {
                            let expr_text = &ctx.input[exp.start as usize..exp.end as usize];
                            // The key function uses original param names, not _for_item{N}.
                            let params_str = original_params.join(", ");
                            return Some(format!("({}) => ({})", params_str, expr_text));
                        }
                    }
                }
            }
        }
        None
    }

    /// Complete a structural element close (v-if, v-else-if, v-else, v-for).
    pub(super) fn complete_structural_element_close(
        &mut self,
        state: &mut VaporElementState,
        close_tag: Option<&crate::syntax_kai::types::ElementCloseTag>,
    ) {
        let scope = state
            .scope
            .take()
            .expect("complete_structural_element_close: scope must be set");
        let close_end = close_tag.map(|ct| ct.end).unwrap_or(state.open_tag_end);

        match scope {
            VaporScopeKind::If { condition } => {
                self.imports.add(VaporImportDependencies::CREATE_IF);

                // Emit _setInsertionState for nested v-if inside native elements.
                let insertion_state = self.build_insertion_state(state);

                // Build the block body for this branch.
                let body = self.build_block_body(state, close_tag, "    ");

                let node_ref = state.node_ref;
                let code = format!(
                    "{}  const n{} = _createIf(() => ({}), () => {{\n{}  }}",
                    insertion_state, node_ref, condition, body
                );

                // Start a new v-if chain.
                self.pending_vif_chains.push(VaporVIfChainState {
                    node_ref,
                    branch_index: 0,
                    code,
                    open_parens: 1,
                    chain_start: state.open_tag_start,
                    chain_end: close_end,
                    child_index: state.child_index,
                });

                // Remove source from code_transform.
                let code_transform = &mut self.code_transform.borrow_mut();
                code_transform.overwrite(state.open_tag_start, close_end, "");
            }

            VaporScopeKind::ElseIf { condition } => {
                self.imports.add(VaporImportDependencies::CREATE_IF);

                // Build the block body for this branch.
                let body = self.build_block_body(state, close_tag, "    ");

                // Extend the pending v-if chain.
                if let Some(chain) = self.pending_vif_chains.last_mut() {
                    chain.branch_index += 1;
                    // Close the previous _createIf and start a nested one.
                    chain.code.push_str(&format!(
                        ", () => _createIf(() => ({}), () => {{\n{}  }}",
                        condition, body
                    ));
                    chain.open_parens += 1;
                    chain.chain_end = close_end;
                }

                // Remove source from code_transform.
                let code_transform = &mut self.code_transform.borrow_mut();
                code_transform.overwrite(state.open_tag_start, close_end, "");
            }

            VaporScopeKind::Else => {
                // Build the block body for this branch.
                let body = self.build_block_body(state, close_tag, "    ");

                // Extend the pending v-if chain with the else branch.
                if let Some(chain) = self.pending_vif_chains.last_mut() {
                    chain.code.push_str(&format!(", () => {{\n{}  }}", body));
                    chain.chain_end = close_end;

                    // Close all open parens now (chain is complete).
                    for i in (0..chain.open_parens).rev() {
                        chain.code.push_str(&format!(", null, {})", i));
                    }
                    chain.open_parens = 0;

                    // Flush the chain immediately since it's complete.
                    let chain = self
                        .pending_vif_chains
                        .pop()
                        .expect("complete_structural_element_close: v-else chain must exist");
                    let code = chain.code;
                    let code_with_newline = format!("{}\n", code);

                    let is_root = self.stack.is_empty();
                    if is_root {
                        self.root_nodes.push(chain.node_ref);
                        let code_transform = &mut self.code_transform.borrow_mut();
                        code_transform.overwrite(
                            chain.chain_start,
                            chain.chain_end,
                            &code_with_newline,
                        );
                    } else {
                        if let Some(parent) = self.stack.last_mut() {
                            parent.structural_children.push(code_with_newline.clone());
                        }
                        let code_transform = &mut self.code_transform.borrow_mut();
                        code_transform.overwrite(chain.chain_start, chain.chain_end, "");
                    }
                }

                // Remove source from code_transform (already handled above for chain).
                // The chain_start..chain_end overwrite covers this element too.
            }

            VaporScopeKind::For {
                iterable,
                callback_params,
                original_params: _,
                key_fn,
                depth: _,
            } => {
                self.imports.add(VaporImportDependencies::CREATE_FOR);

                // Emit _setInsertionState for nested v-for inside native elements.
                let insertion_state = self.build_insertion_state(state);

                // Build the block body.
                let body = self.build_block_body(state, close_tag, "    ");

                let params_str = callback_params.join(", ");
                let node_ref = state.node_ref;

                let mut code = format!(
                    "{}  const n{} = _createFor(() => ({}), ({}) => {{\n{}  }}",
                    insertion_state, node_ref, iterable, params_str, body
                );

                // Add key function if present.
                if let Some(ref kf) = key_fn {
                    code.push_str(&format!(", {}", kf));
                }

                code.push_str(")\n");

                // Decrement for_depth.
                self.for_depth = self.for_depth.saturating_sub(1);

                let is_root = self.stack.is_empty();
                if is_root {
                    self.root_nodes.push(node_ref);
                    let code_transform = &mut self.code_transform.borrow_mut();
                    code_transform.overwrite(state.open_tag_start, close_end, &code);
                } else {
                    if let Some(parent) = self.stack.last_mut() {
                        parent.structural_children.push(code);
                    }
                    let code_transform = &mut self.code_transform.borrow_mut();
                    code_transform.overwrite(state.open_tag_start, close_end, "");
                }
            }
        }
    }

    /// Flush any pending v-if chain (emit the accumulated code).
    /// Called when a non-continuation sibling appears or when the parent closes.
    pub(super) fn flush_pending_vif_chain(&mut self) {
        if let Some(chain) = self.pending_vif_chains.pop() {
            let mut code = chain.code;

            // Close all open _createIf parens that haven't been closed by v-else.
            // Each open paren corresponds to a _createIf( call.
            // For a simple v-if (no else), open_parens=1, branch_index=0 → just close with `)`
            // For v-if/v-else-if (no else), open_parens=2, branch_index=1 → close inner then outer
            if chain.open_parens > 0 {
                // Close from innermost to outermost.
                // The innermost _createIf has the highest branch index.
                for _ in 0..chain.open_parens {
                    code.push(')');
                }
            }

            code.push('\n');

            let is_root = self.stack.is_empty();
            if is_root {
                // Root-level v-if: emit as a root node.
                self.root_nodes.push(chain.node_ref);
                let code_transform = &mut self.code_transform.borrow_mut();
                code_transform.overwrite(chain.chain_start, chain.chain_end, &code);
            } else {
                // Nested v-if: emit as a structural child of the parent.
                if let Some(parent) = self.stack.last_mut() {
                    parent.structural_children.push(code);
                }
                let code_transform = &mut self.code_transform.borrow_mut();
                code_transform.overwrite(chain.chain_start, chain.chain_end, "");
            }
        }
    }
}
