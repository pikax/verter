//! Template code generation plugin.
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

use rustc_hash::FxHashMap;

use crate::{
    code_transform::CodeTransform,
    syntax_kai::{
        binding_types::BindingType,
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        plugins::code_gen::{
            template::interpolation::handle_interpolation, types::TemplateImportDependencies,
        },
        types::{
            Comment, CompiledRootTemplateEnd, CompiledRootTemplateStart, ElementScope, Event,
            OxcCompiledElementClosed, OxcCompiledElementStart, OxcInterpolation, Text,
        },
    },
};

pub mod comment;
pub mod directives;
pub mod element;
pub mod helper;
pub mod interpolation;
pub mod text;
pub mod types;

pub(crate) use types::{ChildInfo, ChildKind, DirectiveEntry, ScopeClose, StateStack};

pub struct TemplateGeneratorPlugin<'alloc> {
    code_transform: Rc<RefCell<CodeTransform<'alloc>>>,

    bindings: FxHashMap<&'alloc str, BindingType>,

    is_production: bool,

    is_vapor: bool,

    imports: TemplateImportDependencies,

    stack: Vec<StateStack>,

    cache_id_counter: u16,

    /// Hoisted constants emitted before the render function.
    /// Each entry is the full expression (e.g., `{ class: "app" }` or
    /// `/*#__PURE__*/_createElementVNode("span", null, "static", -1 /* HOISTED */)`).
    hoisted_constants: Vec<String>,

    /// Position of the template open tag — hoisted constants are emitted here.
    template_start_pos: u32,

    /// Component tag names encountered during traversal that need `_resolveComponent` declarations.
    /// Each entry is the original tag name (e.g., "MyComponent").
    /// Deduped — only the first occurrence per name is stored.
    resolved_components: Vec<String>,

    /// Custom directive names encountered during traversal that need `_resolveDirective` declarations.
    /// Each entry is the directive name without `v-` prefix (e.g., "focus", "my-directive").
    /// Deduped — only the first occurrence per name is stored.
    resolved_directives: Vec<String>,
}

impl<'alloc> TemplateGeneratorPlugin<'alloc> {
    pub fn new(code_transform: Rc<RefCell<CodeTransform<'alloc>>>, is_production: bool) -> Self {
        Self {
            code_transform,
            is_production,

            is_vapor: false,

            imports: TemplateImportDependencies::default(),
            bindings: FxHashMap::default(),
            stack: Vec::with_capacity(50),
            cache_id_counter: 0,
            hoisted_constants: Vec::new(),
            template_start_pos: 0,
            resolved_components: Vec::new(),
            resolved_directives: Vec::new(),
        }
    }

    /// Get the transformed code (template block only).
    pub fn get_code(&self) -> String {
        self.code_transform.borrow().to_string()
    }

    pub fn generate_source_map(&self) -> String {
        self.code_transform
            .borrow()
            .generate_map_json(Default::default())
    }

    fn handle_template_start(
        &mut self,
        ev: &CompiledRootTemplateStart,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        self.stack.push(StateStack::new());
        self.template_start_pos = ev.tag_open.start;

        let code_transform = &mut self.code_transform.borrow_mut();

        if ev.vapor.is_some() {
            self.is_vapor = true;
        }

        // clean template opening tag

        if self.is_production {
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
    }

    fn handle_template_closed(
        &mut self,
        ev: &CompiledRootTemplateEnd,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let code_transform = &mut self.code_transform.borrow_mut();

        // Emit hoisted constants before the render function.
        // Vue places these at module scope: `const _hoisted_1 = { class: "app" }`
        if !self.hoisted_constants.is_empty() {
            let mut hoist_str = String::new();
            for (i, constant) in self.hoisted_constants.iter().enumerate() {
                hoist_str.push_str(&format!(
                    "const _hoisted_{} = {};\n",
                    i + 1, // 1-indexed like Vue
                    constant
                ));
            }
            // prepend_left at template start — appears before the `function render(` replacement
            code_transform.prepend_left(self.template_start_pos, &hoist_str);
        }

        let extra_return = if let Some(state) = self.stack.pop() {
            // Emit pending v-if fallback comments for root-level children.
            // These are v-if/v-else-if chains that ended without a v-else.
            for &fallback_pos in &state.pending_vif_fallbacks {
                let comment = if self.is_production {
                    "_createCommentVNode(\"\", true)"
                } else {
                    "_createCommentVNode(\"v-if\", true)"
                };
                code_transform.append_left(fallback_pos, comment);
            }
            if !state.pending_vif_fallbacks.is_empty() {
                self.imports
                    .add(TemplateImportDependencies::CREATE_COMMENT_VNODE);
            }

            if state.children.is_empty() {
                "return null"
            } else {
                // Build _resolveComponent and _resolveDirective declarations.
                // Vue pattern: const _component_X = _resolveComponent("X")
                //              const _directive_X = _resolveDirective("X")
                let mut resolve_decls = String::new();
                for comp_name in &self.resolved_components {
                    resolve_decls.push_str(&format!(
                        "const _component_{} = _resolveComponent(\"{}\");\n",
                        comp_name, comp_name
                    ));
                }
                for dir_name in &self.resolved_directives {
                    let var_name = dir_name.replace('-', "_");
                    resolve_decls.push_str(&format!(
                        "const _directive_{} = _resolveDirective(\"{}\");\n",
                        var_name, dir_name
                    ));
                }

                let is_multi_root = state.children.len() > 1;

                if is_multi_root {
                    // Multiple roots: wrap in Fragment block
                    // return (_openBlock(), _createElementBlock(_Fragment, null, [child1, child2], 64))
                    self.imports.add(TemplateImportDependencies::OPEN_BLOCK);
                    self.imports
                        .add(TemplateImportDependencies::CREATE_ELEMENT_BLOCK);
                    self.imports.add(TemplateImportDependencies::FRAGMENT);

                    let first = &state.children[0];
                    code_transform.prepend_left(
                        first.start,
                        &format!(
                            "{}return (_openBlock(), _createElementBlock(_Fragment, null, [{}{}",
                            resolve_decls,
                            first.scope_prefix,
                            first.kind.content_prefix()
                        ),
                    );
                    for child in state.children.iter().skip(1) {
                        code_transform.prepend_left(
                            child.start,
                            &format!(", {}{}", child.scope_prefix, child.kind.content_prefix()),
                        );
                    }
                    // Close will be ], 64 /* STABLE_FRAGMENT */))
                    // stored in extra_return via the template close tag handler
                } else {
                    // Single root: direct return
                    let first = &state.children[0];
                    code_transform.prepend_left(
                        first.start,
                        &format!(
                            "{}return {}{}",
                            resolve_decls,
                            first.scope_prefix,
                            first.kind.content_prefix()
                        ),
                    );
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

        if let Some(close) = &ev.tag_close {
            // clean template closing tag
            code_transform.replace(
                close.start,
                close.end,
                format!("{}}}", extra_return).as_str(),
            );
        } else {
            // If template closing tag is missing, append a closing brace at the end of the template.
            code_transform.append_right(ev.end, format!("{}}}", extra_return).as_str());
        }
    }

    fn handle_element_start(
        &mut self,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        // Check if this element is a v-else-if/v-else continuation of a v-if chain
        let is_vif_continuation = ev
            .scopes
            .iter()
            .any(|s| matches!(s, ElementScope::ElseIf(_) | ElementScope::Else(_)));

        let parent = self
            .stack
            .last_mut()
            .expect("Element start must be inside template");

        if is_vif_continuation {
            // v-else-if/v-else: this element is part of the previous v-if chain.
            // Pop the last pending fallback (this else branch replaces the comment).
            // Don't push as a separate child of the parent.
            parent.pending_vif_fallbacks.pop();
        } else {
            // Record this element as a child of the parent (for close-phase separator decisions).
            // scope_prefix is filled below after process_scope_opens returns.
            parent.children.push(ChildInfo {
                start: ev.event.event_open_tag.start,
                kind: ChildKind::Element,
                scope_prefix: String::new(),
            });
        }

        // Extract the parent's vif_key_counter before creating child (since create_child consumes &mut parent)
        let mut parent_vif_key_counter = parent.vif_key_counter;

        let mut state = parent.create_child(ev.event.element_id);

        // Root-level elements (direct children of <template>) are block roots.
        // Stack has only the root template state → this element is root-level.
        if self.stack.len() == 1 {
            state.is_block_root = true;
        }

        let mut code_transform = self.code_transform.borrow_mut();

        // 1. Process scope directives (v-if, v-for, etc.) — mutates state.
        //    Returns scope prefix text to be stored in parent's ChildInfo.
        //    Also sets state.is_block_root for v-if/v-else-if/v-else/v-for branches.
        let scope_prefix = directives::process_scope_opens(
            &mut code_transform,
            &ev.scopes,
            ctx,
            &self.bindings,
            self.is_production,
            &mut state,
            &mut self.imports,
            &mut parent_vif_key_counter,
        );

        // Write back the updated vif_key_counter to the parent
        // (parent is at top of stack, we'll re-borrow it)
        drop(code_transform);
        if let Some(parent) = self.stack.last_mut() {
            parent.vif_key_counter = parent_vif_key_counter;
        }
        code_transform = self.code_transform.borrow_mut();

        // Store the scope prefix on the parent's last ChildInfo (if this is a child).
        if !is_vif_continuation && !scope_prefix.is_empty() {
            // parent is no longer borrowed (was dropped after create_child), re-borrow
            drop(code_transform);
            if let Some(parent) = self.stack.last_mut() {
                if let Some(last_child) = parent.children.last_mut() {
                    last_child.scope_prefix = scope_prefix;
                }
            }
            code_transform = self.code_transform.borrow_mut();
        }

        // 2. Handle v-once separately (uses cache helpers on self)
        let mut vonce_prefix = String::new();
        for scope in &ev.scopes {
            if let ElementScope::Once(prop) = scope {
                state.has_once = true;
                // v-once disables block tracking, so the element should NOT be a block root.
                // Vue outputs _createElementVNode (not _createElementBlock) for v-once.
                state.is_block_root = false;
                code_transform.remove(prop.start, prop.end);

                // Drop the RefMut so we can mutably borrow self
                drop(code_transform);

                let cache_id = self.allocate_cache_id();
                state.cache_id = Some(cache_id);

                self.imports
                    .add(TemplateImportDependencies::SET_BLOCK_TRACKING);

                // Vue pattern: _cache[N] || (_setBlockTracking(-1, true), (_cache[N] = CONTENT).cacheIndex = N, _setBlockTracking(1), _cache[N])
                // Open part stored as scope_prefix — emitted by parent's close/template phase
                vonce_prefix = format!(
                    "_cache[{}] || (_setBlockTracking(-1, true), (_cache[{}] = ",
                    cache_id, cache_id
                );

                // Re-borrow after the &mut self calls are done
                code_transform = self.code_transform.borrow_mut();
            }
        }

        // Store v-once prefix on the parent's ChildInfo (alongside any v-if/v-for prefix).
        if !vonce_prefix.is_empty() && !is_vif_continuation {
            drop(code_transform);
            if let Some(parent) = self.stack.last_mut() {
                if let Some(last_child) = parent.children.last_mut() {
                    // Prepend the v-once prefix before any existing scope prefix
                    // (v-if/v-for). v-once wraps the entire expression.
                    last_child.scope_prefix =
                        format!("{}{}", vonce_prefix, last_child.scope_prefix);
                }
            }
            code_transform = self.code_transform.borrow_mut();
        }

        // 2b. Handle v-slot (slot scopes on component elements and template children)
        //
        // SlotElement: `<Button v-slot="foo">` — default slot on the component itself
        // SlotTemplate: `<template v-slot:name="params">` — named slot on a <template> child
        //
        // For SlotElement, we store slot_params on the component state so the close phase
        // wraps children in `{ default: _withCtx((params) => [...]), _: 1 }`.
        //
        // For SlotTemplate, the <template> element itself becomes a named slot entry.
        // TODO: Named slots via SlotTemplate are tracked but not yet fully emitted.
        for scope in &ev.scopes {
            let (event, parsed, slot_name, is_dynamic) =
                match scope {
                    ElementScope::SlotElement(s) => {
                        let name =
                            s.event.arg.as_ref().map(|arg| {
                                ctx.input[arg.start as usize..arg.end as usize].to_string()
                            });
                        (&s.event, &s.parsed, name, s.event.has_dynamic_arg)
                    }
                    ElementScope::SlotTemplate(s) => {
                        let name =
                            s.event.arg.as_ref().map(|arg| {
                                ctx.input[arg.start as usize..arg.end as usize].to_string()
                            });
                        (&s.event, &s.parsed, name, s.event.has_dynamic_arg)
                    }
                    _ => continue,
                };

            // Remove the v-slot directive from source
            code_transform.remove(event.start, event.end);

            // Use raw source text for params (preserves destructuring like `{ data }`)
            let params = if let Some(val) = event.value {
                ctx.input[val.start as usize..val.end as usize].to_string()
            } else if !parsed.locals.is_empty() {
                parsed
                    .locals
                    .iter()
                    .map(|span| &ctx.input[span.start as usize..span.end as usize])
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                String::new()
            };

            state.slot_params = Some(params);
            // For dynamic slot names (`v-slot:[foo]`), apply accessor prefix
            // to the inner expression. The arg span includes brackets, e.g. `[foo]`.
            state.slot_name = if is_dynamic {
                slot_name.map(|name| {
                    // Strip brackets: `[foo]` → `foo`
                    let inner = name
                        .strip_prefix('[')
                        .and_then(|s| s.strip_suffix(']'))
                        .unwrap_or(&name);
                    // Look up binding and apply accessor prefix
                    if let Some(bt) = self.bindings.get(inner) {
                        let prefix = bt.accessor_prefix(false);
                        format!("[{}{}]", prefix, inner)
                    } else {
                        // Unresolved → _ctx. prefix
                        format!("[_ctx.{}]", inner)
                    }
                })
            } else {
                slot_name
            };
            state.slot_is_dynamic = is_dynamic;

            self.imports.add(TemplateImportDependencies::WITH_CTX);
        }

        // 3. Element VNode open — mutates state (is_component, patch_flag, etc.)
        element::handle_element_open(
            &mut code_transform,
            ev,
            ctx,
            &self.bindings,
            self.is_production,
            &mut state,
            &mut self.imports,
            &mut self.resolved_components,
            &mut self.resolved_directives,
            &mut self.hoisted_constants,
        );

        // 4. Void/self-closing elements never get an OxcCompiledElementClosed event,
        //    so close them immediately instead of pushing onto the stack.
        let open_tag_end = &ev.event.event_open_tag_end;
        if open_tag_end.is_self_closing || open_tag_end.is_void_element {
            element::handle_element_close_self_closing(
                &mut code_transform,
                &state,
                self.is_production,
            );

            // Process scope closes for self-closing elements (v-if, v-for, etc.)
            let close_pos = state.open_tag_end;
            let had_vif_close = directives::process_scope_closes(
                &mut code_transform,
                &state.pending_scope_closes,
                close_pos,
                self.is_production,
            );

            // If this element had a v-if/v-else-if close, store pending fallback on parent.
            if had_vif_close {
                drop(code_transform);
                if let Some(parent) = self.stack.last_mut() {
                    parent.pending_vif_fallbacks.push(close_pos);
                }
                code_transform = self.code_transform.borrow_mut();
            }

            // v-once close for self-closing elements
            if let Some(cache_id) = state.cache_id {
                let close_str = format!(
                    ").cacheIndex = {}, _setBlockTracking(1), _cache[{}])",
                    cache_id, cache_id
                );
                code_transform.append_left(close_pos, &close_str);
            }
            // Don't push — stack stays balanced
        } else {
            self.stack.push(state);
        }
    }

    fn handle_element_closed(
        &mut self,
        ev: &OxcCompiledElementClosed,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let state = self
            .stack
            .pop()
            .expect("Element close must have matching open");

        let mut code_transform = self.code_transform.borrow_mut();

        // 1. Emit pending v-if fallback comments for children of this element.
        //    These are v-if/v-else-if chains that ended without a v-else.
        for &fallback_pos in &state.pending_vif_fallbacks {
            let comment = if self.is_production {
                "_createCommentVNode(\"\", true)"
            } else {
                "_createCommentVNode(\"v-if\", true)"
            };
            code_transform.append_left(fallback_pos, comment);
        }
        if !state.pending_vif_fallbacks.is_empty() {
            self.imports
                .add(TemplateImportDependencies::CREATE_COMMENT_VNODE);
        }

        // 2. Close element VNode
        element::handle_element_close(
            &mut code_transform,
            ev,
            &state,
            self.is_production,
            &mut self.imports,
        );

        // 3. Close scope directives
        let close_pos = ev
            .event
            .event_close_tag
            .as_ref()
            .map(|c| c.end)
            .unwrap_or(state.open_tag_end);

        let had_vif_close = directives::process_scope_closes(
            &mut code_transform,
            &state.pending_scope_closes,
            close_pos,
            self.is_production,
        );

        // 4. If this element had a v-if/v-else-if close, store pending fallback on parent.
        //    A subsequent v-else-if/v-else element will pop it; otherwise the parent's
        //    close phase emits the comment fallback.
        if had_vif_close {
            if let Some(parent) = self.stack.last_mut() {
                parent.pending_vif_fallbacks.push(close_pos);
            }
        }

        // 5. v-once close: ).cacheIndex = N, _setBlockTracking(1), _cache[N])
        if let Some(cache_id) = state.cache_id {
            let close_str = format!(
                ").cacheIndex = {}, _setBlockTracking(1), _cache[{}])",
                cache_id, cache_id
            );
            code_transform.append_left(close_pos, &close_str);
        }
    }

    fn handle_comment(&mut self, ev: &Comment, ctx: &SyntaxPluginContext<'alloc>) {
        let state = self
            .stack
            .last_mut()
            .expect("Comment inside template must have stack");
        comment::handle_comment(
            &mut self.code_transform.borrow_mut(),
            ev,
            ctx,
            state,
            &mut self.imports,
        );
    }

    fn handle_text(&mut self, ev: &Text, ctx: &SyntaxPluginContext<'alloc>) {
        let state = self
            .stack
            .last_mut()
            .expect("Text inside template must have stack");
        text::handle_text(
            &mut self.code_transform.borrow_mut(),
            ev,
            ctx,
            state,
            &mut self.imports,
        );
    }

    fn handle_interpolation(
        &mut self,
        ev: &OxcInterpolation<'alloc>,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let state = self
            .stack
            .last_mut()
            .expect("Interpolation inside template must have stack");
        state.children.push(ChildInfo {
            start: ev.start,
            kind: ChildKind::Interpolation,
            scope_prefix: String::new(),
        });
        state.children_count += 1;

        let mut code_transform = self.code_transform.borrow_mut();

        // No separator — close phase handles it retroactively
        handle_interpolation(&mut code_transform, ev, &self.bindings, self.is_production);

        self.imports
            .add(TemplateImportDependencies::TO_DISPLAY_STRING);
    }

    /// Allocate a cache slot. Returns the cache ID for use in open/close code emission.
    /// The caller must store it on the appropriate `StateStack.cache_id`.
    fn allocate_cache_id(&mut self) -> u16 {
        let cache_id = self.cache_id_counter;
        self.cache_id_counter += 1;
        cache_id
    }
}

impl<'alloc> SyntaxPlugin<'alloc> for TemplateGeneratorPlugin<'alloc> {
    fn name(&self) -> &str {
        "TemplateGeneratorPlugin"
    }

    fn process_event(
        &mut self,
        event: Event<'alloc>,
        ctx: &mut SyntaxPluginContext<'alloc>,
    ) -> SyntaxResult<Event<'alloc>> {
        match event {
            Event::OxcScript(ev) => {
                ev.result.bindings.iter().for_each(|(name, binding)| {
                    self.bindings
                        .insert(&ctx.input[name.start as usize..name.end as usize], *binding);
                });
                // Skip processing script content in template plugin
                SyntaxResult::keep(Event::OxcScript(ev))
            }

            Event::CompiledTemplateStart(ev) => {
                self.handle_template_start(&ev, ctx);
                SyntaxResult::keep(Event::CompiledTemplateStart(ev))
            }
            Event::CompiledTemplateEnd(ev) => {
                self.handle_template_closed(&ev, ctx);
                SyntaxResult::keep(Event::CompiledTemplateEnd(ev))
            }

            Event::OxcCompiledElementStart(ev) => {
                self.handle_element_start(&ev, ctx);
                SyntaxResult::keep(Event::OxcCompiledElementStart(ev))
            }
            Event::OxcCompiledElementClosed(ev) => {
                self.handle_element_closed(&ev, ctx);
                SyntaxResult::keep(Event::OxcCompiledElementClosed(ev))
            }

            // Text, comment, interpolation events outside <template> are skipped.
            // Inside <template>, the stack must not be empty — .expect() catches bugs.
            Event::Comment(ev) => {
                // TODO remove this, if the stack is empty this event shouldn't have been send
                if !self.stack.is_empty() {
                    self.handle_comment(&ev, ctx);
                }
                SyntaxResult::keep(Event::Comment(ev))
            }
            Event::Text(ev) => {
                // TODO remove this, if the stack is empty this event shouldn't have been send
                if !self.stack.is_empty() {
                    self.handle_text(&ev, ctx);
                }
                SyntaxResult::keep(Event::Text(ev))
            }
            Event::OxcInterpolation(ev) => {
                // TODO remove this, if the stack is empty this event shouldn't have been send
                if !self.stack.is_empty() {
                    self.handle_interpolation(&ev, ctx);
                }
                SyntaxResult::keep(Event::OxcInterpolation(ev))
            }
            // Event::
            _ => SyntaxResult::keep(event),
        }
    }
}

#[cfg(test)]
mod tests;
