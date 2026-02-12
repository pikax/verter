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
    syntax_kai::{
        binding_types::BindingType,
        plugin::SyntaxPluginContext,
        plugins::code_gen::types::TemplateImportDependencies,
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

    imports: TemplateImportDependencies,

    stack: Vec<StateStack>,

    cache_id_counter: u16,

    /// Hoisted constants emitted before the render function.
    hoisted_constants: Vec<String>,

    /// Position of the template open tag — hoisted constants are emitted here.
    template_start_pos: u32,

    /// Component tag names that need `_resolveComponent` declarations (ordered for deterministic output).
    resolved_components: Vec<String>,
    /// Fast lookup set for `resolved_components` deduplication.
    resolved_components_set: FxHashSet<String>,

    /// Custom directive names that need `_resolveDirective` declarations (ordered for deterministic output).
    resolved_directives: Vec<String>,
    /// Fast lookup set for `resolved_directives` deduplication.
    resolved_directives_set: FxHashSet<String>,
}

impl<'alloc> VdomTemplateGenerator<'alloc> {
    pub(crate) fn new(
        code_transform: Rc<RefCell<CodeTransform<'alloc>>>,
        is_production: bool,
    ) -> Self {
        Self {
            code_transform,
            is_production,

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
        }
    }

    /// Receive the bindings collected by the orchestrator from OxcScript events.
    pub(crate) fn set_bindings(&mut self, bindings: FxHashMap<&'alloc str, BindingType>) {
        self.bindings = bindings;
    }

    /// Get the transformed code.
    pub(crate) fn get_code(&self) -> String {
        self.code_transform.borrow().to_string()
    }

    pub(crate) fn generate_source_map(&self) -> String {
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
    ) {
        self.stack.push(StateStack::new());
        self.template_start_pos = ev.tag_open.start;

        let code_transform = &mut self.code_transform.borrow_mut();

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

    pub(crate) fn handle_template_closed(
        &mut self,
        ev: &CompiledRootTemplateEnd,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let code_transform = &mut self.code_transform.borrow_mut();

        // Emit hoisted constants before the render function.
        if !self.hoisted_constants.is_empty() {
            let mut hoist_str = String::new();
            for (i, constant) in self.hoisted_constants.iter().enumerate() {
                hoist_str.push_str(&format!("const _hoisted_{} = {};\n", i + 1, constant));
            }
            code_transform.prepend_left(self.template_start_pos, &hoist_str);
        }

        let extra_return = if let Some(state) = self.stack.pop() {
            // Emit pending v-if fallback comments for root-level children.
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
                } else {
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
            code_transform.replace(
                close.start,
                close.end,
                format!("{}}}", extra_return).as_str(),
            );
        } else {
            code_transform.append_right(ev.end, format!("{}}}", extra_return).as_str());
        }
    }

    pub(crate) fn handle_element_start(
        &mut self,
        ev: &OxcCompiledElementStart<'alloc>,
        ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let is_vif_continuation = ev
            .scopes
            .iter()
            .any(|s| matches!(s, ElementScope::ElseIf(_) | ElementScope::Else(_)));

        let parent = self
            .stack
            .last_mut()
            .expect("Element start must be inside template");

        if is_vif_continuation {
            parent.pending_vif_fallbacks.pop();
        } else {
            parent.children.push(ChildInfo {
                start: ev.event.event_open_tag.start,
                kind: ChildKind::Element,
                scope_prefix: String::new(),
            });
        }

        let mut parent_vif_key_counter = parent.vif_key_counter;

        let mut state = parent.create_child(ev.event.element_id);

        if self.stack.len() == 1 {
            state.is_block_root = true;
        }

        // --- Phase 1: CodeTransform operations (scope directives + v-once remove) ---
        let scope_prefix;
        let mut vonce_remove_span: Option<(u32, u32)> = None;

        {
            let mut code_transform = self.code_transform.borrow_mut();

            scope_prefix = directives::process_scope_opens(
                &mut code_transform,
                &ev.scopes,
                ctx,
                &self.bindings,
                self.is_production,
                &mut state,
                &mut self.imports,
                &mut parent_vif_key_counter,
            );

            for scope in &ev.scopes {
                if let ElementScope::Once(prop) = scope {
                    state.is_block_root = false;
                    code_transform.remove(prop.start, prop.end);
                    vonce_remove_span = Some((prop.start, prop.end));
                }
            }
        }

        // --- Phase 2: Stack + self mutations (no code_transform borrow needed) ---

        if let Some(parent) = self.stack.last_mut() {
            parent.vif_key_counter = parent_vif_key_counter;
        }

        if !is_vif_continuation && !scope_prefix.is_empty() {
            if let Some(parent) = self.stack.last_mut() {
                if let Some(last_child) = parent.children.last_mut() {
                    last_child.scope_prefix = scope_prefix;
                }
            }
        }

        if vonce_remove_span.is_some() {
            let cache_id = self.allocate_cache_id();
            state.cache_id = Some(cache_id);

            self.imports
                .add(TemplateImportDependencies::SET_BLOCK_TRACKING);

            let vonce_prefix = format!(
                "_cache[{}] || (_setBlockTracking(-1, true), (_cache[{}] = ",
                cache_id, cache_id
            );

            if !is_vif_continuation {
                if let Some(parent) = self.stack.last_mut() {
                    if let Some(last_child) = parent.children.last_mut() {
                        last_child.scope_prefix =
                            format!("{}{}", vonce_prefix, last_child.scope_prefix);
                    }
                }
            }
        }

        // --- Phase 3: Remaining CodeTransform operations ---

        let mut code_transform = self.code_transform.borrow_mut();

        // Handle v-slot
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

            code_transform.remove(event.start, event.end);

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
            state.slot_name = if is_dynamic {
                slot_name.map(|name| {
                    let inner = name
                        .strip_prefix('[')
                        .and_then(|s| s.strip_suffix(']'))
                        .unwrap_or(&name);
                    if let Some(bt) = self.bindings.get(inner) {
                        let prefix = bt.accessor_prefix(false);
                        format!("[{}{}]", prefix, inner)
                    } else {
                        format!("[_ctx.{}]", inner)
                    }
                })
            } else {
                slot_name
            };
            state.slot_is_dynamic = is_dynamic;

            self.imports.add(TemplateImportDependencies::WITH_CTX);
        }

        // Element VNode open
        let mut ectx = element::ElementOpenContext {
            bindings: &self.bindings,
            is_production: self.is_production,
            imports: &mut self.imports,
            resolved_components: &mut self.resolved_components,
            resolved_components_set: &mut self.resolved_components_set,
            resolved_directives: &mut self.resolved_directives,
            resolved_directives_set: &mut self.resolved_directives_set,
            hoisted_constants: &mut self.hoisted_constants,
        };
        element::handle_element_open(&mut code_transform, ev, ctx, &mut state, &mut ectx);

        // Void/self-closing elements
        let open_tag_end = &ev.event.event_open_tag_end;
        if open_tag_end.is_self_closing || open_tag_end.is_void_element {
            element::handle_element_close_self_closing(
                &mut code_transform,
                &state,
                self.is_production,
            );

            let close_pos = state.open_tag_end;
            let had_vif_close = directives::process_scope_closes(
                &mut code_transform,
                &state.pending_scope_closes,
                close_pos,
                self.is_production,
            );

            if let Some(cache_id) = state.cache_id {
                let close_str = format!(
                    ").cacheIndex = {}, _setBlockTracking(1), _cache[{}])",
                    cache_id, cache_id
                );
                code_transform.append_left(close_pos, &close_str);
            }

            drop(code_transform);

            if had_vif_close {
                if let Some(parent) = self.stack.last_mut() {
                    parent.pending_vif_fallbacks.push(close_pos);
                }
            }
        } else {
            self.stack.push(state);
        }
    }

    pub(crate) fn handle_element_closed(
        &mut self,
        ev: &OxcCompiledElementClosed,
        _ctx: &SyntaxPluginContext<'alloc>,
    ) {
        let state = self
            .stack
            .pop()
            .expect("Element close must have matching open");

        let mut code_transform = self.code_transform.borrow_mut();

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

        element::handle_element_close(
            &mut code_transform,
            ev,
            &state,
            self.is_production,
            &mut self.imports,
        );

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

        if had_vif_close {
            if let Some(parent) = self.stack.last_mut() {
                parent.pending_vif_fallbacks.push(close_pos);
            }
        }

        if let Some(cache_id) = state.cache_id {
            let close_str = format!(
                ").cacheIndex = {}, _setBlockTracking(1), _cache[{}])",
                cache_id, cache_id
            );
            code_transform.append_left(close_pos, &close_str);
        }
    }

    pub(crate) fn handle_comment(&mut self, ev: &Comment, ctx: &SyntaxPluginContext<'alloc>) {
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

    pub(crate) fn handle_text(&mut self, ev: &Text, ctx: &SyntaxPluginContext<'alloc>) {
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

    pub(crate) fn handle_interpolation(
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

        let mut code_transform = self.code_transform.borrow_mut();

        interpolation::handle_interpolation(
            &mut code_transform,
            ev,
            &self.bindings,
            self.is_production,
        );

        self.imports
            .add(TemplateImportDependencies::TO_DISPLAY_STRING);
    }

    fn allocate_cache_id(&mut self) -> u16 {
        let cache_id = self.cache_id_counter;
        self.cache_id_counter += 1;
        cache_id
    }
}
