//! VDOM template code generation.
//!
//! # Deferred-Emission Architecture
//!
//! This module uses a "store on open, emit on close" pattern throughout.
//! The fundamental reason is `CodeTransform`'s `prepend_left` FIFO semantics:
//! multiple `prepend_left` calls at the same position produce output in
//! call order (first call appears first). This makes it impossible to emit
//! separators and content prefixes in separate calls at the same position.
//!
//! ## Core Invariant
//!
//! At any child's start position, there is exactly ONE `prepend_left` call,
//! made by the **parent's close phase**, combining three parts:
//!   1. **Separator**: `, ` or ` + ` or `, [` (depends on children mode)
//!   2. **Scope prefix**: `(condition) ? ` for v-if, `renderList(...)` for v-for
//!   3. **Content prefix**: `"` for text, `_toDisplayString` for interpolation
//!
//! Child handlers (text, interpolation, element open) must NEVER call
//! `prepend_left` at their own start position. They record metadata in
//! [`ChildInfo`] and [`StateStack`] for the close phase to use.
//!
//! ## Deferred Patterns
//!
//! | Pattern | Stored where | Emitted when |
//! |---------|-------------|--------------|
//! | Text opening quote | `ChildKind::Text.content_prefix()` | Parent close |
//! | `_toDisplayString` | `ChildKind::Interpolation.content_prefix()` | Parent close |
//! | v-if/v-for prefix | `ChildInfo.scope_prefix` | Parent close |
//! | v-if fallback comment | `StateStack.pending_vif_fallbacks` | Grandparent close |
//! | `_withDirectives(` open | Embedded in open tag `overwrite` | Immediate (no conflict) |
//! | Directive array suffix | N/A | Element close via `emit_with_directives()` |
//! | Hoisted constants | `self.hoisted_constants` | `handle_template_closed()` |
//! | Resolve declarations | `self.resolved_components/directives` | `handle_template_closed()` |
//!
//! ## Exception: v-else-if
//!
//! v-else-if elements emit their condition directly via `prepend_left`
//! (in `directives::process_scope_opens`). This is safe because v-else-if
//! elements are NOT registered as parent children — they continue the
//! previous v-if chain. No separator is emitted for them, so there is
//! no FIFO conflict.

use std::{cell::RefCell, rc::Rc};

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    code_transform::CodeTransform,
    syntax::{
        binding_types::BindingType,
        plugin::SyntaxPluginContext,
        plugins::code_gen::types::{
            TemplateCodeGenError, TemplateCodeGenResult, TemplateImportDependencies,
        },
        types::{
            Comment, CompiledRootTemplateEnd, CompiledRootTemplateStart, ElementScope,
            OxcCompiledElementClosed, OxcCompiledElementStart, OxcInterpolation, Text,
        },
    },
};

pub(crate) mod comment;
pub(crate) mod directives;
pub(crate) mod element;
pub(crate) mod helper;
pub(crate) mod interpolation;
pub(crate) mod text;
pub(crate) mod types;

pub(crate) use types::{ChildInfo, ChildKind, DirectiveEntry, ScopeClose, StateStack};

pub(crate) struct VdomTemplateGenerator<'alloc> {
    code_transform: Rc<RefCell<CodeTransform<'alloc>>>,

    bindings: FxHashMap<&'alloc str, BindingType>,

    is_production: bool,
    inline: bool,
    comments: bool,
    hoist_static: bool,
    #[allow(dead_code)] // wired in API, behavioral implementation deferred
    cache_handlers: bool,
    runtime_module_name: String,
    #[allow(dead_code)] // wired in API, behavioral implementation deferred
    prefix_identifiers: bool,

    imports: TemplateImportDependencies,

    stack: Vec<StateStack<'alloc>>,

    cache_id_counter: u16,

    /// Hoisted constants emitted before the render function.
    /// Bump-allocated strings to avoid per-element heap allocation.
    hoisted_constants: Vec<&'alloc str>,

    /// Position of the template open tag — hoisted constants are emitted here.
    template_start_pos: u32,

    /// Component tag names that need `_resolveComponent` declarations (ordered for deterministic output).
    resolved_components: Vec<&'alloc str>,
    /// Fast lookup set for `resolved_components` deduplication.
    resolved_components_set: FxHashSet<&'alloc str>,

    /// Custom directive names that need `_resolveDirective` declarations (ordered for deterministic output).
    resolved_directives: Vec<&'alloc str>,
    /// Fast lookup set for `resolved_directives` deduplication.
    resolved_directives_set: FxHashSet<&'alloc str>,

    /// Deferred prepend_left operations collected during template codegen.
    /// Includes binding patches (_ctx., $setup., etc.) and close-phase child
    /// separators. Applied in a single O(n+m) pass via `batch_prepend_left_static`.
    pending_prepend_lefts: Vec<(u32, &'alloc str)>,

    /// Deferred overwrite operations. Applied in a single O(n+m) pass via `batch_overwrite`.
    pending_overwrites: Vec<(u32, u32, &'alloc str)>,

    /// Deferred append_left operations. Applied in a single O(n+m) pass via `batch_prepend_left_static`.
    pending_append_lefts: Vec<(u32, &'alloc str)>,

    /// Reusable String buffer — avoids per-element heap allocations.
    /// Taken via `std::mem::take()` before element processing, put back after.
    buf: String,

    /// Reusable buffer for merged append+prepend operations in `finalize()`.
    combined_buffer: Vec<(u32, &'alloc str)>,

    /// Pool of recycled StateStack objects — avoids re-allocating inner Vecs
    /// on every element open/close cycle.
    state_pool: Vec<StateStack<'alloc>>,
}

impl<'alloc> VdomTemplateGenerator<'alloc> {
    /// Create with explicit template options.
    pub(crate) fn with_options(
        code_transform: Rc<RefCell<CodeTransform<'alloc>>>,
        options: &super::TemplateOptions,
    ) -> Self {
        Self {
            code_transform,
            is_production: options.is_production,
            inline: options.inline,
            comments: options.comments,
            hoist_static: options.hoist_static,
            cache_handlers: options.cache_handlers,
            runtime_module_name: options.runtime_module_name.clone(),
            prefix_identifiers: options.prefix_identifiers,

            imports: TemplateImportDependencies::default(),
            bindings: FxHashMap::default(),
            stack: Vec::with_capacity(50),
            cache_id_counter: 0,
            hoisted_constants: Vec::new(),
            template_start_pos: 0,
            resolved_components: Vec::new(),
            resolved_components_set: FxHashSet::default(),
            resolved_directives: Vec::new(),
            resolved_directives_set: FxHashSet::default(),
            pending_prepend_lefts: Vec::with_capacity(256),
            pending_overwrites: Vec::with_capacity(512),
            pending_append_lefts: Vec::with_capacity(64),
            buf: String::with_capacity(128),
            combined_buffer: Vec::with_capacity(320),
            state_pool: Vec::with_capacity(16),
        }
    }

    /// Receive the bindings collected by the orchestrator from OxcScript events.
    pub(crate) fn set_bindings(&mut self, bindings: FxHashMap<&'alloc str, BindingType>) {
        self.bindings = bindings;
    }

    /// Take a StateStack from the pool (or create a new one), reset for the given element ID.
    #[inline]
    fn take_state(&mut self, element_id: u32) -> StateStack<'alloc> {
        if let Some(mut state) = self.state_pool.pop() {
            state.reset(element_id);
            state
        } else {
            StateStack {
                id: element_id,
                ..StateStack::default()
            }
        }
    }

    /// Return a used StateStack to the pool for later reuse.
    #[inline]
    fn return_state(&mut self, state: StateStack<'alloc>) {
        self.state_pool.push(state);
    }

    /// Flush all deferred operations into the shared CodeTransform.
    ///
    /// Applies three O(n+m) batch passes in order:
    /// 1. **Overwrites** — split Original chunks and insert Edited replacements
    /// 2. **Append-lefts** — insert content at boundary positions (after overwrites)
    /// 3. **Prepend-lefts** — insert content at positions (binding patches, separators)
    ///
    /// Must be called before reading the CodeTransform directly (e.g. in `compile()`).
    pub(crate) fn finalize(&mut self) {
        let mut ct = self.code_transform.borrow_mut();

        // Phase 1: apply all overwrites (already in document order — no sort needed)
        if !self.pending_overwrites.is_empty() {
            debug_assert!(
                self.pending_overwrites.windows(2).all(|w| w[0].0 <= w[1].0),
                "INVARIANT VIOLATED: pending_overwrites not in document order"
            );
            ct.batch_overwrite(&self.pending_overwrites);
            self.pending_overwrites.clear();
        }

        // Phase 2+3 merged: append_lefts + prepend_lefts in a single batch pass.
        // Append_lefts are placed first so that stable sort keeps them before
        // prepend_lefts at the same position (correct: suffixes before prefixes).
        let append_count = self.pending_append_lefts.len();
        let prepend_count = self.pending_prepend_lefts.len();
        if append_count > 0 || prepend_count > 0 {
            self.combined_buffer.clear();
            self.combined_buffer.append(&mut self.pending_append_lefts);
            self.combined_buffer.append(&mut self.pending_prepend_lefts);
            self.combined_buffer.sort_by_key(|(pos, _)| *pos);
            ct.batch_prepend_left_static(&self.combined_buffer);
        }
    }

    /// Prepend the `import { ... } from '<runtime>'` statement for template helpers.
    pub(crate) fn emit_imports(&self) {
        if !self.imports.is_empty() {
            self.code_transform.borrow_mut().prepend(&format!(
                "import {{{}}} from '{}';\n",
                self.imports.to_import_string(),
                self.runtime_module_name,
            ));
        }
    }

    /// Get the transformed code.
    pub(crate) fn get_code(&mut self) -> String {
        self.finalize();
        self.code_transform.borrow().build_string()
    }

    pub(crate) fn generate_source_map(&mut self) -> String {
        self.finalize();
        self.code_transform
            .borrow()
            .generate_map_json(Default::default())
    }

    pub(crate) fn is_inside_template(&self) -> bool {
        !self.stack.is_empty()
    }

    pub(crate) fn handle_template_start(
        &mut self,
        ev: &CompiledRootTemplateStart,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) -> TemplateCodeGenResult {
        let root_state = self.take_state(0);
        self.stack.push(root_state);
        self.template_start_pos = ev.tag_open.start;

        let code_transform = &mut self.code_transform.borrow_mut();

        if self.inline {
            code_transform.replace(
                ev.tag_open.start,
                ev.tag_open.end,
                "return (_ctx,_cache) => {",
            );
        } else {
            code_transform.replace(
                ev.tag_open.start,
                ev.tag_open.end,
                "function render(_ctx, _cache, $props, $setup, $data, $options) {",
            );
        }

        Ok(())
    }

    pub(crate) fn handle_template_closed(
        &mut self,
        ev: &CompiledRootTemplateEnd,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) -> TemplateCodeGenResult {
        let code_transform = &mut self.code_transform.borrow_mut();

        // Emit hoisted constants before the render function.
        if !self.hoisted_constants.is_empty() {
            // Pre-calculate total size: "const _hoisted_N = ...;\n" per entry
            let total_size: usize = self
                .hoisted_constants
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    // "const _hoisted_" + digits + " = " + constant + ";\n"
                    16 + num_digits(i + 1) + 3 + c.len() + 2
                })
                .sum();
            let mut hoist_str = String::with_capacity(total_size);
            for (i, constant) in self.hoisted_constants.iter().enumerate() {
                hoist_str.push_str("const _hoisted_");
                helper::push_u32(&mut hoist_str, (i + 1) as u32);
                hoist_str.push_str(" = ");
                hoist_str.push_str(constant);
                hoist_str.push_str(";\n");
            }
            code_transform.prepend_left(self.template_start_pos, &hoist_str);
        }

        let extra_return = if let Some(state) = self.stack.pop() {
            // Defer pending v-if fallback comments for root-level children.
            for &fallback_pos in &state.pending_vif_fallbacks {
                let comment = if self.is_production {
                    "_createCommentVNode(\"\", true)"
                } else {
                    "_createCommentVNode(\"v-if\", true)"
                };
                self.pending_append_lefts.push((fallback_pos, comment));
            }
            if !state.pending_vif_fallbacks.is_empty() {
                self.imports
                    .add(TemplateImportDependencies::CREATE_COMMENT_VNODE);
            }

            if state.children.is_empty() {
                "return null"
            } else {
                // Build _resolveComponent and _resolveDirective declarations.
                // Pre-calculate size: "const _component_X = _resolveComponent("X");\n"
                let resolve_size: usize = self
                    .resolved_components
                    .iter()
                    .map(|n| 17 + n.len() + 22 + n.len() + 3) // const _component_ + name + = _resolveComponent(" + name + ");\n
                    .chain(self.resolved_directives.iter().map(|n| {
                        17 + n.len() + 22 + n.len() + 3 // const _directive_ + name + = _resolveDirective(" + name + ");\n
                    }))
                    .sum();
                let mut resolve_decls = String::with_capacity(resolve_size);
                for comp_name in &self.resolved_components {
                    resolve_decls.push_str("const _component_");
                    for ch in comp_name.chars() {
                        if ch == '-' {
                            resolve_decls.push('_');
                        } else {
                            resolve_decls.push(ch);
                        }
                    }
                    resolve_decls.push_str(" = _resolveComponent(\"");
                    resolve_decls.push_str(comp_name);
                    resolve_decls.push_str("\");\n");
                }
                for dir_name in &self.resolved_directives {
                    resolve_decls.push_str("const _directive_");
                    for ch in dir_name.chars() {
                        if ch == '-' {
                            resolve_decls.push('_');
                        } else {
                            resolve_decls.push(ch);
                        }
                    }
                    resolve_decls.push_str(" = _resolveDirective(\"");
                    resolve_decls.push_str(dir_name);
                    resolve_decls.push_str("\");\n");
                }

                let is_multi_root = state.children.len() > 1;

                let mut buf = String::with_capacity(128);

                if is_multi_root {
                    self.imports.add(TemplateImportDependencies::OPEN_BLOCK);
                    self.imports
                        .add(TemplateImportDependencies::CREATE_ELEMENT_BLOCK);
                    self.imports.add(TemplateImportDependencies::FRAGMENT);

                    let first = &state.children[0];
                    buf.push_str(&resolve_decls);
                    buf.push_str("return (_openBlock(), _createElementBlock(_Fragment, null, [");
                    buf.push_str(first.scope_prefix);
                    buf.push_str(first.kind.content_prefix());
                    code_transform.prepend_left(first.start, &buf);

                    for child in state.children.iter().skip(1) {
                        buf.clear();
                        buf.push_str(", ");
                        buf.push_str(child.scope_prefix);
                        buf.push_str(child.kind.content_prefix());
                        code_transform.prepend_left(child.start, &buf);
                    }
                } else {
                    let first = &state.children[0];
                    buf.push_str(&resolve_decls);
                    buf.push_str("return ");
                    buf.push_str(first.scope_prefix);
                    buf.push_str(first.kind.content_prefix());
                    code_transform.prepend_left(first.start, &buf);
                }

                if is_multi_root {
                    if self.is_production {
                        "], 64))"
                    } else {
                        "], 64 /* STABLE_FRAGMENT */))"
                    }
                } else {
                    ""
                }
            }
        } else {
            "return null"
        };

        let mut close_buf = String::with_capacity(extra_return.len() + 1);
        close_buf.push_str(extra_return);
        close_buf.push('}');

        if let Some(close) = &ev.tag_close {
            code_transform.replace(close.start, close.end, &close_buf);
        } else {
            code_transform.append_right(ev.end, &close_buf);
        }

        Ok(())
    }

    pub(crate) fn handle_element_start(
        &mut self,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> TemplateCodeGenResult {
        let is_vif_continuation = ev
            .scopes
            .iter()
            .any(|s| matches!(s, ElementScope::ElseIf(_) | ElementScope::Else(_)));

        let parent = self
            .stack
            .last_mut()
            .ok_or(TemplateCodeGenError::StackUnderflow(
                "element start must be inside template",
            ))?;

        if is_vif_continuation {
            parent.pending_vif_fallbacks.pop();
        } else {
            parent.children.push(ChildInfo {
                start: ev.event.event_open_tag.start,
                end: 0, // unused for elements
                kind: ChildKind::Element,
                scope_prefix: "",
                is_named_slot: false,
            });
        }

        let mut parent_vif_key_counter = parent.vif_key_counter;

        let mut state = self.take_state(ev.event.element_id);

        if self.stack.len() == 1 {
            state.is_block_root = true;
        }

        // Pre-allocate cache_id for v-once before borrowing code_transform
        let vonce_cache_id = ev.scopes.iter().find_map(|s| {
            if matches!(s, ElementScope::Once(_)) {
                Some(self.allocate_cache_id())
            } else {
                None
            }
        });

        // Take the reusable buffer — avoids per-element heap allocation.
        let mut buf = std::mem::take(&mut self.buf);

        // All CodeTransform operations are deferred to pending vecs.
        // Only a single immutable borrow is needed for alloc_str().
        let code_transform = self.code_transform.borrow();

        // Scope directives (v-if, v-else-if, v-else, v-for)
        let scope_prefix = directives::process_scope_opens(
            &code_transform,
            &ev.scopes,
            ctx,
            &self.bindings,
            self.is_production,
            &mut state,
            &mut self.imports,
            &mut parent_vif_key_counter,
            &mut self.pending_prepend_lefts,
            &mut buf,
        );

        // v-once: set up cache (no remove needed — handle_element_open covers the region)
        for scope in &ev.scopes {
            if let ElementScope::Once(_) = scope {
                state.is_block_root = false;
            }
        }

        // Handle v-slot
        for scope in &ev.scopes {
            let (event, parsed, slot_name, is_dynamic) = match scope {
                ElementScope::SlotElement(s) => {
                    let name: Option<&'alloc str> = s
                        .event
                        .arg
                        .as_ref()
                        .map(|arg| &ctx.input[arg.start as usize..arg.end as usize]);
                    (&s.event, &s.parsed, name, s.event.has_dynamic_arg)
                }
                ElementScope::SlotTemplate(s) => {
                    let name: Option<&'alloc str> = s
                        .event
                        .arg
                        .as_ref()
                        .map(|arg| &ctx.input[arg.start as usize..arg.end as usize]);
                    (&s.event, &s.parsed, name, s.event.has_dynamic_arg)
                }
                _ => continue,
            };

            // No remove needed — handle_element_open covers name_end..open_tag_end.

            let params: &'alloc str = if let Some(val) = event.value {
                &ctx.input[val.start as usize..val.end as usize]
            } else if !parsed.locals.is_empty() {
                let joined = parsed
                    .locals
                    .iter()
                    .map(|span| &ctx.input[span.start as usize..span.end as usize])
                    .collect::<Vec<_>>()
                    .join(", ");
                code_transform.alloc_str(&joined)
            } else {
                ""
            };

            state.slot_params = Some(params);
            state.slot_name = if is_dynamic {
                slot_name.map(|name| {
                    let inner = name
                        .strip_prefix('[')
                        .and_then(|s| s.strip_suffix(']'))
                        .unwrap_or(name);
                    buf.clear();
                    buf.push('[');
                    if let Some(bt) = self.bindings.get(inner) {
                        buf.push_str(bt.accessor_prefix(false));
                    } else {
                        buf.push_str("_ctx.");
                    }
                    buf.push_str(inner);
                    buf.push(']');
                    code_transform.alloc_str(&buf)
                })
            } else {
                slot_name
            };
            state.slot_is_dynamic = is_dynamic;

            self.imports.add(TemplateImportDependencies::WITH_CTX);
        }

        // Named slot templates: <template #name> inside a component parent.
        // Mark both this state and parent for named-slot object children mode.
        if state.slot_params.is_some() {
            let is_slot_template = ev
                .scopes
                .iter()
                .any(|s| matches!(s, ElementScope::SlotTemplate(_)));
            if is_slot_template {
                if let Some(parent) = self.stack.last() {
                    if parent.is_component {
                        state.is_named_slot_template = true;
                    }
                }
            }
        }

        // Stack + self mutations
        if let Some(parent) = self.stack.last_mut() {
            parent.vif_key_counter = parent_vif_key_counter;
            // Propagate named slot flags to parent component
            if state.is_named_slot_template {
                parent.has_named_slot_children = true;
                if state.slot_is_dynamic {
                    parent.any_dynamic_slots = true;
                }
                // Mark the child info so the close phase can distinguish
                // named slot entries from implicit default slot content.
                if let Some(last_child) = parent.children.last_mut() {
                    last_child.is_named_slot = true;
                }
            }
        }

        if !is_vif_continuation && !scope_prefix.is_empty() {
            if let Some(parent) = self.stack.last_mut() {
                if let Some(last_child) = parent.children.last_mut() {
                    last_child.scope_prefix = scope_prefix;
                }
            }
        }

        if let Some(cache_id) = vonce_cache_id {
            state.cache_id = Some(cache_id);

            self.imports
                .add(TemplateImportDependencies::SET_BLOCK_TRACKING);

            // Build vonce prefix using shared buf (save/truncate pattern)
            let saved = buf.len();
            buf.push_str("_cache[");
            helper::push_u32(&mut buf, cache_id as u32);
            buf.push_str("] || (_setBlockTracking(-1, true), (_cache[");
            helper::push_u32(&mut buf, cache_id as u32);
            buf.push_str("] = ");

            if !is_vif_continuation {
                if let Some(parent) = self.stack.last_mut() {
                    if let Some(last_child) = parent.children.last_mut() {
                        buf.push_str(last_child.scope_prefix);
                        last_child.scope_prefix = code_transform.alloc_str(&buf[saved..]);
                    }
                }
            }
            buf.truncate(saved);
        }

        // Element VNode open
        let mut ectx = element::ElementOpenContext {
            bindings: &self.bindings,
            is_production: self.is_production,
            inline: self.inline,
            hoist_static: self.hoist_static,
            imports: &mut self.imports,
            resolved_components: &mut self.resolved_components,
            resolved_components_set: &mut self.resolved_components_set,
            resolved_directives: &mut self.resolved_directives,
            resolved_directives_set: &mut self.resolved_directives_set,
            hoisted_constants: &mut self.hoisted_constants,
        };
        element::handle_element_open(
            &code_transform,
            ev,
            ctx,
            &mut state,
            &mut ectx,
            &mut self.pending_prepend_lefts,
            &mut self.pending_overwrites,
            &mut buf,
        );

        // Void/self-closing elements
        let open_tag_end = &ev.event.event_open_tag_end;
        if open_tag_end.is_self_closing || open_tag_end.is_void_element {
            element::handle_element_close_self_closing(
                &code_transform,
                &state,
                self.is_production,
                &mut self.pending_append_lefts,
                &mut buf,
            );

            let close_pos = state.open_tag_end;
            let had_vif_close = directives::process_scope_closes(
                &code_transform,
                &state.pending_scope_closes,
                close_pos,
                self.is_production,
                &mut self.pending_append_lefts,
                &mut buf,
            );

            if let Some(cache_id) = state.cache_id {
                buf.clear();
                buf.push_str(").cacheIndex = ");
                helper::push_u32(&mut buf, cache_id as u32);
                buf.push_str(", _setBlockTracking(1), _cache[");
                helper::push_u32(&mut buf, cache_id as u32);
                buf.push_str("])");
                let s = code_transform.alloc_str(&buf);
                self.pending_append_lefts.push((close_pos, s));
            }

            drop(code_transform);

            if had_vif_close {
                if let Some(parent) = self.stack.last_mut() {
                    parent.pending_vif_fallbacks.push(close_pos);
                }
            }

            // Return state to pool — inner Vecs retain capacity for reuse.
            self.return_state(state);
        } else {
            self.stack.push(state);
        }

        // Return the reusable buffer (retains capacity for next element).
        self.buf = buf;

        Ok(())
    }

    pub(crate) fn handle_element_closed(
        &mut self,
        ev: &OxcCompiledElementClosed,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) -> TemplateCodeGenResult {
        let state = self
            .stack
            .pop()
            .ok_or(TemplateCodeGenError::StackUnderflow(
                "element close must have matching open",
            ))?;

        let code_transform = self.code_transform.borrow();

        for &fallback_pos in &state.pending_vif_fallbacks {
            let comment = if self.is_production {
                "_createCommentVNode(\"\", true)"
            } else {
                "_createCommentVNode(\"v-if\", true)"
            };
            self.pending_append_lefts.push((fallback_pos, comment));
        }
        if !state.pending_vif_fallbacks.is_empty() {
            self.imports
                .add(TemplateImportDependencies::CREATE_COMMENT_VNODE);
        }

        element::handle_element_close(
            &code_transform,
            ev,
            &state,
            self.is_production,
            &mut self.imports,
            &mut self.pending_prepend_lefts,
            &mut self.pending_overwrites,
            &mut self.pending_append_lefts,
            &mut self.buf,
        );

        let close_pos = ev
            .event
            .event_close_tag
            .as_ref()
            .map(|c| c.end)
            .unwrap_or(state.open_tag_end);

        let had_vif_close = directives::process_scope_closes(
            &code_transform,
            &state.pending_scope_closes,
            close_pos,
            self.is_production,
            &mut self.pending_append_lefts,
            &mut self.buf,
        );

        if had_vif_close {
            if let Some(parent) = self.stack.last_mut() {
                parent.pending_vif_fallbacks.push(close_pos);
            }
        }

        if let Some(cache_id) = state.cache_id {
            self.buf.clear();
            self.buf.push_str(").cacheIndex = ");
            helper::push_u32(&mut self.buf, cache_id as u32);
            self.buf.push_str(", _setBlockTracking(1), _cache[");
            helper::push_u32(&mut self.buf, cache_id as u32);
            self.buf.push_str("])");
            let s = code_transform.alloc_str(&self.buf);
            self.pending_append_lefts.push((close_pos, s));
        }

        // Drop code_transform borrow before mutating self.
        drop(code_transform);

        // Return state to pool — inner Vecs retain capacity for reuse.
        self.return_state(state);

        Ok(())
    }

    pub(crate) fn handle_comment(
        &mut self,
        ev: &Comment,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> TemplateCodeGenResult {
        let state = self
            .stack
            .last_mut()
            .ok_or(TemplateCodeGenError::StackUnderflow(
                "comment inside template must have stack",
            ))?;
        comment::handle_comment(
            ev,
            ctx,
            state,
            &mut self.imports,
            &mut self.pending_overwrites,
            self.comments,
        );

        Ok(())
    }

    pub(crate) fn handle_text(
        &mut self,
        ev: &Text,
        ctx: &SyntaxPluginContext<'alloc>,
    ) -> TemplateCodeGenResult {
        let state = self
            .stack
            .last_mut()
            .ok_or(TemplateCodeGenError::StackUnderflow(
                "text inside template must have stack",
            ))?;
        text::handle_text(
            &self.code_transform.borrow(),
            ev,
            ctx,
            state,
            &mut self.imports,
            &mut self.pending_overwrites,
            &mut self.pending_append_lefts,
        );

        Ok(())
    }

    pub(crate) fn handle_interpolation(
        &mut self,
        ev: &OxcInterpolation<'alloc>,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) -> TemplateCodeGenResult {
        let state = self
            .stack
            .last_mut()
            .ok_or(TemplateCodeGenError::StackUnderflow(
                "interpolation inside template must have stack",
            ))?;
        state.children.push(ChildInfo {
            start: ev.start,
            end: ev.end,
            kind: ChildKind::Interpolation,
            scope_prefix: "",
            is_named_slot: false,
        });

        interpolation::handle_interpolation(
            ev,
            &self.bindings,
            self.is_production,
            &mut self.pending_prepend_lefts,
            &mut self.pending_overwrites,
        );

        self.imports
            .add(TemplateImportDependencies::TO_DISPLAY_STRING);

        Ok(())
    }

    fn allocate_cache_id(&mut self) -> u16 {
        let cache_id = self.cache_id_counter;
        self.cache_id_counter = self.cache_id_counter.wrapping_add(1);
        cache_id
    }
}

/// Count decimal digits in a positive number (for pre-allocation).
#[inline]
fn num_digits(n: usize) -> usize {
    if n < 10 {
        1
    } else if n < 100 {
        2
    } else if n < 1000 {
        3
    } else {
        // Fallback for very large numbers (unlikely in practice)
        ((n as f64).log10().floor() as usize) + 1
    }
}
