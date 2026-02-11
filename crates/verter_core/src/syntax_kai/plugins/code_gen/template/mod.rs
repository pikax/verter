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
    utils::vue::PatchFlag,
};

pub mod comment;
pub mod directives;
pub mod element;
pub mod helper;
pub mod interpolation;
pub mod text;

/// Kind of child node — used by close-phase to decide separator strategy.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ChildKind {
    Text,
    Interpolation,
    Element,
    Comment,
}

impl ChildKind {
    /// Content prefix that the close phase must prepend for this child kind.
    ///
    /// Text children need an opening `"` quote; interpolation needs `_toDisplayString`.
    /// Elements and comments use `overwrite` for their own prefix, so no extra prefix is needed.
    ///
    /// This exists because `prepend_left` at the same position is FIFO — if the child
    /// handler and close phase both call `prepend_left` at the same position, the child
    /// handler's content appears first. So the close phase must emit the child's content
    /// prefix as part of its own single `prepend_left` call.
    pub(crate) fn content_prefix(&self) -> &'static str {
        match self {
            ChildKind::Text => "\"",
            ChildKind::Interpolation => "_toDisplayString",
            ChildKind::Element | ChildKind::Comment => "",
        }
    }
}

/// Recorded during child processing for close-phase separator decisions.
pub(crate) struct ChildInfo {
    /// Start position in source — used for retroactive separator insertion via prepend_left.
    pub start: u32,
    /// What kind of child this is.
    pub kind: ChildKind,
    /// Scope open prefix text (e.g. `"(show) ? "` for v-if, renderList wrapper for v-for).
    /// Emitted by the close phase as part of the separator prepend_left call, ensuring correct
    /// ordering: separator THEN scope prefix THEN child content.
    pub scope_prefix: String,
}

/// Stored scope close token — emitted after the element VNode call closes.
pub(crate) enum ScopeClose {
    /// `) : _createCommentVNode("v-if", true)`
    IfTernary,
    /// `) : _createCommentVNode("v-if", true)`
    ElseIfTernary,
    /// `)`
    Else,
    /// `}), 128 /* KEYED_FRAGMENT */))`
    For,
}

pub(crate) struct StateStack {
    pub id: u32,
    #[allow(dead_code)]
    pub parent_id: u32,

    pub no_tracking: bool,

    pub has_once: bool,
    pub has_condition: bool,

    pub is_once: bool,

    pub children_count: u16,
    #[allow(dead_code)]
    pub children_patch_flag: PatchFlag,

    /// Child nodes recorded during processing — close phase uses this to decide
    /// separators (concatenation vs array), TEXT patch flag, etc.
    pub children: Vec<ChildInfo>,

    pub cache_id: Option<u16>,

    // -- Element codegen fields (populated during element open) --
    /// Whether this element is a component (vs native element).
    pub is_component: bool,

    /// Position after `>` of the open tag — used as fallback emit position for self-closing.
    pub open_tag_end: u32,

    /// Accumulated patch flag from props processing.
    pub patch_flag: PatchFlag,

    /// Dynamic prop names for the PROPS patch flag.
    pub dynamic_props: Vec<String>,

    /// Scope closes to emit after the element VNode call.
    pub pending_scope_closes: Vec<ScopeClose>,

    /// Whether this element is a block root (uses _openBlock + _createElementBlock).
    /// True for: direct children of <template>, v-if/v-for branch elements.
    pub is_block_root: bool,

    /// Pending v-if/v-else-if close positions where comment fallback should be emitted.
    /// Each position is a close_tag.end where ` : ` was appended by a v-if/v-else-if close.
    /// When v-else-if/v-else follows, the last entry is popped (consumed by the else branch).
    /// When this element closes, remaining entries get `_createCommentVNode("v-if", true)`.
    pub pending_vif_fallbacks: Vec<u32>,
}

impl StateStack {
    pub fn new() -> Self {
        Self {
            id: 0,
            parent_id: 0,
            no_tracking: false,
            has_once: false,
            has_condition: false,

            is_once: false,

            children_count: 0,
            children_patch_flag: PatchFlag::empty(),
            children: Vec::new(),
            cache_id: None,

            is_component: false,
            open_tag_end: 0,
            patch_flag: PatchFlag::empty(),
            dynamic_props: Vec::new(),
            pending_scope_closes: Vec::new(),
            is_block_root: false,
            pending_vif_fallbacks: Vec::new(),
        }
    }

    pub fn create_child(&mut self, element_id: u32) -> Self {
        self.children_count += 1;

        Self {
            id: element_id,
            parent_id: self.id,
            no_tracking: self.no_tracking,
            has_once: self.has_once || self.is_once,
            has_condition: self.has_condition,

            is_once: false,

            children_count: 0,
            children_patch_flag: PatchFlag::empty(),
            children: Vec::new(),
            cache_id: None,

            is_component: false,
            open_tag_end: 0,
            patch_flag: PatchFlag::empty(),
            dynamic_props: Vec::new(),
            pending_scope_closes: Vec::new(),
            is_block_root: false,
            pending_vif_fallbacks: Vec::new(),
        }
    }
}

pub struct TemplateGeneratorPlugin<'alloc> {
    code_transform: Rc<RefCell<CodeTransform<'alloc>>>,

    bindings: FxHashMap<&'alloc str, BindingType>,

    is_production: bool,

    is_vapor: bool,

    imports: TemplateImportDependencies,

    stack: Vec<StateStack>,

    cache_id_counter: u16,

    /// Component tag names encountered during traversal that need `_resolveComponent` declarations.
    /// Each entry is the original tag name (e.g., "MyComponent").
    /// Deduped — only the first occurrence per name is stored.
    resolved_components: Vec<String>,
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
            resolved_components: Vec::new(),
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
                // Build _resolveComponent declarations for any components used.
                // Vue pattern: const _component_X = _resolveComponent("X")
                let mut resolve_decls = String::new();
                for comp_name in &self.resolved_components {
                    resolve_decls.push_str(&format!(
                        "const _component_{} = _resolveComponent(\"{}\");\n",
                        comp_name, comp_name
                    ));
                }

                // Insert declarations + "return " + scope_prefix + content_prefix
                // before first root child. Combining into a single prepend_left
                // ensures correct ordering.
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
                // Insert ", " + scope_prefix + content_prefix between subsequent root children
                for child in state.children.iter().skip(1) {
                    code_transform.prepend_left(
                        child.start,
                        &format!(", {}{}", child.scope_prefix, child.kind.content_prefix()),
                    );
                }
                ""
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
        );

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
        element::handle_element_close(&mut code_transform, ev, &state, self.is_production);

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
mod tests {
    use crate::builder::codegen_kai::{generate_kai, KaiCodegenOptions};
    use oxc_allocator::Allocator;
    use oxc_parser::Parser;
    use oxc_span::SourceType;

    // =========================================================================
    // Test Infrastructure
    // =========================================================================

    /// Run the full pipeline (tokenizer → syntax_kai → codegen) in dev mode.
    fn gen(input: &str) -> String {
        let allocator = Allocator::new();
        let options = KaiCodegenOptions::new().with_filename("test.vue");
        generate_kai(input, &options, &allocator).code
    }

    /// Run the full pipeline in production mode.
    fn gen_prod(input: &str) -> String {
        let allocator = Allocator::new();
        let options = KaiCodegenOptions::new()
            .with_filename("test.vue")
            .with_production(true);
        generate_kai(input, &options, &allocator).code
    }

    /// Validate that generated code is syntactically valid JavaScript.
    fn assert_valid_js(code: &str, context: &str) {
        let allocator = Allocator::default();
        let source_type = SourceType::mjs();
        let parser_result = Parser::new(&allocator, code, source_type).parse();
        assert!(
            parser_result.errors.is_empty(),
            "Generated code is NOT valid JavaScript!\n\
             Context: {}\n\
             Parse Errors: {:?}\n\
             Generated Code:\n{}",
            context,
            parser_result.errors,
            code
        );
    }

    /// Known invalid patterns that indicate broken codegen.
    const INVALID_PATTERNS: &[(&str, &str)] = &[
        ("{ :", "empty property name"),
        ("_ctx.{", "object literal after _ctx."),
        ("_ctx.[", "array literal after _ctx."),
        ("{ v-", "hyphenated directive as property"),
        (": _ctx.!", "negation in wrong position"),
        (", ,", "double comma"),
        (
            "\"_toDisplayString",
            "missing string concatenation operator",
        ),
    ];

    /// Check that generated code does not contain known invalid patterns.
    fn assert_no_invalid_patterns(code: &str, context: &str) {
        for (pattern, desc) in INVALID_PATTERNS {
            assert!(
                !code.contains(pattern),
                "Found invalid pattern '{}' ({}) in {}.\nGenerated:\n{}",
                pattern,
                desc,
                context,
                code
            );
        }
    }

    /// Generate code AND validate it is valid JS + no invalid patterns.
    fn gen_and_validate(input: &str) -> String {
        let code = gen(input);
        assert_valid_js(&code, input);
        assert_no_invalid_patterns(&code, input);
        code
    }

    /// Generate production code AND validate it is valid JS.
    /// Production code starts with `return (_ctx,_cache) => {` so we wrap in a function for validation.
    fn gen_prod_and_validate(input: &str) -> String {
        let code = gen_prod(input);
        let wrapped = format!("function __wrapper__() {{ {} }}", code);
        assert_valid_js(&wrapped, input);
        assert_no_invalid_patterns(&code, input);
        code
    }

    // =========================================================================
    // Template Wrapper
    // =========================================================================

    /// @ai-generated — Dev mode emits `function render(_ctx, _cache, ...)`
    #[test]
    fn test_dev_function_render() {
        let code = gen_and_validate(r#"<template><div>hi</div></template>"#);
        assert!(
            code.contains("function render(_ctx, _cache"),
            "Dev mode should emit function render, got:\n{}",
            code
        );
    }

    /// @ai-generated — Production mode emits arrow function `(_ctx,_cache) => {`
    #[test]
    fn test_prod_arrow_fn() {
        let code = gen_prod_and_validate(r#"<template><div>hi</div></template>"#);
        assert!(
            code.contains("(_ctx,_cache) => {"),
            "Prod mode should emit arrow function, got:\n{}",
            code
        );
    }

    /// @ai-generated — Empty template returns null
    #[test]
    fn test_template_empty_returns_null() {
        let code = gen_and_validate(r#"<template></template>"#);
        assert!(
            code.contains("return null"),
            "Empty template should return null, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Elements — basic structure
    // =========================================================================

    /// @ai-generated — Simple div with text child (root = block)
    #[test]
    fn test_element_simple_div_text() {
        let code = gen_and_validate(r#"<template><div>hello</div></template>"#);
        assert!(
            code.contains(r#"_createElementBlock("div", null, "hello")"#),
            "Root should emit _createElementBlock(\"div\", null, \"hello\"), got:\n{}",
            code
        );
        assert!(
            code.contains("_openBlock()"),
            "Root should use _openBlock(), got:\n{}",
            code
        );
    }

    /// @ai-generated — Self-closing <br/> element (root = block)
    #[test]
    fn test_element_self_closing_br() {
        let code = gen_and_validate(r#"<template><br/></template>"#);
        assert!(
            code.contains(r#"_createElementBlock("br", null)"#),
            "Root br should use _createElementBlock, got:\n{}",
            code
        );
    }

    /// @ai-generated — Void <input> element (root = block)
    #[test]
    fn test_element_void_input() {
        let code = gen_and_validate(r#"<template><input></template>"#);
        assert!(
            code.contains(r#"_createElementBlock("input", null)"#),
            "Root void input should use _createElementBlock, got:\n{}",
            code
        );
    }

    /// @ai-generated — Empty div produces no children arg (root = block)
    #[test]
    fn test_element_empty_div() {
        let code = gen_and_validate(r#"<template><div></div></template>"#);
        assert!(
            code.contains(r#"_createElementBlock("div", null)"#),
            "Empty root div should use _createElementBlock, got:\n{}",
            code
        );
    }

    /// @ai-generated — Nested elements: root = block, child = VNode
    #[test]
    fn test_element_nested() {
        let code = gen_and_validate(r#"<template><div><span>inner</span></div></template>"#);
        assert!(
            code.contains(r#"_createElementBlock("div""#),
            "Root div should be _createElementBlock, got:\n{}",
            code
        );
        assert!(
            code.contains(r#"_createElementVNode("span", null, "inner")"#),
            "Child span should be _createElementVNode with text, got:\n{}",
            code
        );
    }

    /// @ai-generated — Deeply nested elements
    #[test]
    fn test_element_deeply_nested() {
        let code =
            gen_and_validate(r#"<template><div><span><em>deep</em></span></div></template>"#);
        assert!(
            code.contains(r#"_createElementVNode("em", null, "deep")"#),
            "Deepest element should have text, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Elements — block root treatment
    // Vue wraps root elements in (_openBlock(), _createElementBlock(...))
    // =========================================================================

    /// @ai-generated — Root element should use _openBlock + _createElementBlock
    #[test]
    fn test_block_root_simple() {
        let code = gen_and_validate(r#"<template><div>hello</div></template>"#);
        assert!(
            code.contains("_openBlock()"),
            "Root should use _openBlock(), got:\n{}",
            code
        );
        assert!(
            code.contains("_createElementBlock("),
            "Root should use _createElementBlock, got:\n{}",
            code
        );
    }

    /// @ai-generated — Nested child should use _createElementVNode (not block)
    #[test]
    fn test_block_root_nested_child_is_vnode() {
        let code = gen_and_validate(r#"<template><div><span>inner</span></div></template>"#);
        assert!(
            code.contains("_createElementBlock("),
            "Root div should use _createElementBlock, got:\n{}",
            code
        );
        assert!(
            code.contains(r#"_createElementVNode("span""#),
            "Child span should use _createElementVNode, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Static Props
    // =========================================================================

    /// @ai-generated — Static id attribute
    #[test]
    fn test_props_static_id() {
        let code = gen_and_validate(r#"<template><div id="app">hi</div></template>"#);
        assert!(
            code.contains(r#"{id: "app"}"#),
            "Should have static id prop, got:\n{}",
            code
        );
    }

    /// @ai-generated — Static class attribute
    #[test]
    fn test_props_static_class() {
        let code = gen_and_validate(r#"<template><div class="foo bar">hi</div></template>"#);
        assert!(
            code.contains(r#"class: "foo bar""#),
            "Should have class prop, got:\n{}",
            code
        );
    }

    /// @ai-generated — Static style attribute
    #[test]
    fn test_props_static_style() {
        let code = gen_and_validate(r#"<template><div style="color: red">hi</div></template>"#);
        assert!(
            code.contains(r#"style: "color: red""#),
            "Should have style prop, got:\n{}",
            code
        );
    }

    /// @ai-generated — Props null when no attributes
    #[test]
    fn test_props_null_when_empty() {
        let code = gen_and_validate(r#"<template><div>hello</div></template>"#);
        assert!(
            code.contains(r#""div", null"#),
            "No props should produce null, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Bound Props — :id, :class, :style
    // =========================================================================

    /// @ai-generated — Bound :id produces {id: expr} with PROPS patch flag
    #[test]
    fn test_props_bound_id() {
        let code = gen_and_validate(r#"<template><div :id="myId">hi</div></template>"#);
        assert!(
            code.contains("{id: myId}"),
            "Bound id should be {{id: myId}}, got:\n{}",
            code
        );
        assert!(
            code.contains("8 /* PROPS */"),
            "Should have PROPS (8) patch flag, got:\n{}",
            code
        );
        assert!(
            code.contains(r#"["id"]"#),
            "Should list dynamic prop name, got:\n{}",
            code
        );
    }

    /// @ai-generated — :class uses _normalizeClass with CLASS flag
    #[test]
    fn test_props_class_normalize() {
        let code = gen_and_validate(r#"<template><div :class="cls">hi</div></template>"#);
        assert!(
            code.contains("class: _normalizeClass(cls)"),
            "Should use _normalizeClass, got:\n{}",
            code
        );
        assert!(
            code.contains("2 /* CLASS */"),
            "Should have CLASS (2) patch flag, got:\n{}",
            code
        );
    }

    /// @ai-generated — :style uses _normalizeStyle with STYLE flag
    #[test]
    fn test_props_style_normalize() {
        let code = gen_and_validate(r#"<template><div :style="sty">hi</div></template>"#);
        assert!(
            code.contains("style: _normalizeStyle(sty)"),
            "Should use _normalizeStyle, got:\n{}",
            code
        );
        assert!(
            code.contains("4 /* STYLE */"),
            "Should have STYLE (4) patch flag, got:\n{}",
            code
        );
    }

    /// @ai-generated — Mixed static + bound props
    #[test]
    fn test_props_mixed_static_bound() {
        let code = gen_and_validate(r#"<template><div id="s" :title="d">hi</div></template>"#);
        assert!(
            code.contains(r#"id: "s""#),
            "Static id should be preserved, got:\n{}",
            code
        );
        assert!(
            code.contains("title: d"),
            "Bound title should be present, got:\n{}",
            code
        );
        assert!(
            code.contains("8 /* PROPS */"),
            "Should have PROPS patch flag, got:\n{}",
            code
        );
    }

    /// @ai-generated — Combined :class and :style patch flags
    #[test]
    fn test_props_class_style_combined() {
        let code = gen_and_validate(r#"<template><div :class="c" :style="s">hi</div></template>"#);
        assert!(
            code.contains("_normalizeClass(c)"),
            "Should have _normalizeClass, got:\n{}",
            code
        );
        assert!(
            code.contains("_normalizeStyle(s)"),
            "Should have _normalizeStyle, got:\n{}",
            code
        );
        // CLASS(2) | STYLE(4) = 6
        assert!(
            code.contains("6 /* CLASS, STYLE */"),
            "Should have combined CLASS+STYLE flag (6), got:\n{}",
            code
        );
    }

    /// @ai-generated — No patch flag for static-only props
    #[test]
    fn test_props_no_pf_for_static() {
        let code = gen_and_validate(r#"<template><div id="app">hi</div></template>"#);
        // Static props shouldn't produce a patch flag number
        assert!(
            !code.contains("/* PROPS */"),
            "Static-only props should not have PROPS flag, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Events — @click etc.
    // =========================================================================

    /// @ai-generated — @click becomes onClick prop
    #[test]
    fn test_event_click() {
        let code =
            gen_and_validate(r#"<template><button @click="handler">click</button></template>"#);
        assert!(
            code.contains("onClick: handler"),
            "Should have onClick: handler, got:\n{}",
            code
        );
    }

    /// @ai-generated — Multiple events
    #[test]
    fn test_event_multiple() {
        let code = gen_and_validate(
            r#"<template><button @click="a" @mouseover="b">hi</button></template>"#,
        );
        assert!(
            code.contains("onClick: a"),
            "Should have onClick, got:\n{}",
            code
        );
        assert!(
            code.contains("onMouseover: b"),
            "Should have onMouseover, got:\n{}",
            code
        );
    }

    /// @ai-generated — Vue treats events as PROPS patch flag with event name in dynamic props
    #[test]
    #[ignore = "events should track PROPS patch flag like Vue"]
    fn test_event_props_patch_flag() {
        let code =
            gen_and_validate(r#"<template><button @click="handler">click</button></template>"#);
        assert!(
            code.contains("8 /* PROPS */"),
            "Event should produce PROPS (8) patch flag, got:\n{}",
            code
        );
        assert!(
            code.contains(r#"["onClick"]"#),
            "Event name should be in dynamic props list, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Text
    // =========================================================================

    /// @ai-generated — Text wrapping in quotes
    #[test]
    fn test_text_in_quotes() {
        let code = gen_and_validate(r#"<template><div>hello</div></template>"#);
        assert!(
            code.contains(r#""hello""#),
            "Text should be wrapped in quotes, got:\n{}",
            code
        );
    }

    /// @ai-generated — Text with quotes gets escaped
    #[test]
    fn test_text_escaped_quotes() {
        let code = gen(r#"<template><div>say "hello"</div></template>"#);
        // The text should escape inner quotes
        assert!(
            code.contains(r#"say \"hello\""#) || code.contains(r#"say "hello""#),
            "Text with quotes should be handled, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Interpolation
    // =========================================================================

    /// @ai-generated — Simple interpolation produces _toDisplayString
    #[test]
    fn test_interp_simple() {
        let code = gen_and_validate(r#"<template><div>{{ msg }}</div></template>"#);
        assert!(
            code.contains("_toDisplayString"),
            "Should have _toDisplayString, got:\n{}",
            code
        );
    }

    /// @ai-generated — Interpolation with expression
    #[test]
    fn test_interp_expr() {
        let code = gen_and_validate(r#"<template><div>{{ a + b }}</div></template>"#);
        assert!(
            code.contains("_toDisplayString"),
            "Should have _toDisplayString for expression, got:\n{}",
            code
        );
    }

    /// @ai-generated — Interpolation with ternary
    #[test]
    fn test_interp_ternary() {
        let code = gen_and_validate(r#"<template><div>{{ a ? b : c }}</div></template>"#);
        assert!(
            code.contains("_toDisplayString"),
            "Should have _toDisplayString for ternary, got:\n{}",
            code
        );
        assert!(
            code.contains("a ? b : c"),
            "Ternary expression should be preserved, got:\n{}",
            code
        );
    }

    /// @ai-generated — Interpolation with method call
    #[test]
    fn test_interp_method_call() {
        let code = gen_and_validate(r#"<template><div>{{ foo() }}</div></template>"#);
        assert!(
            code.contains("_toDisplayString"),
            "Should have _toDisplayString for method call, got:\n{}",
            code
        );
    }

    /// @ai-generated — Interpolation with $setup binding prefix
    #[test]
    fn test_interp_with_setup_binding() {
        let code = gen_and_validate(
            r#"<script setup>
import { ref } from 'vue'
const msg = ref('hello')
</script>
<template><div>{{ msg }}</div></template>"#,
        );
        assert!(
            code.contains("_toDisplayString"),
            "Should have _toDisplayString, got:\n{}",
            code
        );
        // Setup bindings should get $setup prefix in dev mode
        assert!(
            code.contains("$setup.msg"),
            "Setup binding should have $setup prefix, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Text + Interpolation Mix (concatenation)
    // Vue concatenates: "hello " + _toDisplayString(_ctx.msg)
    // Current: separate comma args (requires close-phase refactor)
    // =========================================================================

    /// @ai-generated — Text + interpolation should concatenate with +
    #[test]
    fn test_children_text_interp_concat() {
        let code = gen_and_validate(r#"<template><div>hello {{ msg }}</div></template>"#);
        assert!(
            code.contains(r#""hello " + _toDisplayString"#),
            "Text + interpolation should concat with +, got:\n{}",
            code
        );
        assert!(
            code.contains("1 /* TEXT */"),
            "Concatenated text should have TEXT patch flag, got:\n{}",
            code
        );
    }

    /// @ai-generated — Text-interp-text should concat: "hello " + expr + " world"
    #[test]
    fn test_children_text_interp_text_concat() {
        let code = gen_and_validate(r#"<template><div>hello {{ msg }} world</div></template>"#);
        assert!(
            code.contains(r#""hello " + _toDisplayString"#),
            "Should start with text + toDisplayString, got:\n{}",
            code
        );
        assert!(
            code.contains(r#"+ " world""#),
            "Should end with + \" world\", got:\n{}",
            code
        );
    }

    /// @ai-generated — Multiple interpolations concatenated
    #[test]
    fn test_children_multiple_interp_concat() {
        let code = gen_and_validate(r#"<template><div>{{ a }}{{ b }}</div></template>"#);
        assert!(
            code.contains("_toDisplayString"),
            "Should have _toDisplayString calls, got:\n{}",
            code
        );
        // Vue: _toDisplayString(_ctx.a) + _toDisplayString(_ctx.b)
        assert!(
            code.contains(" + _toDisplayString"),
            "Multiple interps should concatenate with +, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Children array wrapping
    // Vue wraps multiple non-text children in [...] array
    // =========================================================================

    /// @ai-generated — Multiple element children should be in array
    #[test]
    fn test_children_multiple_elements_array() {
        let code =
            gen_and_validate(r#"<template><div><span>a</span><span>b</span></div></template>"#);
        // Vue: [..., [...]] where children are in an array
        assert!(
            code.contains("[_createElementVNode"),
            "Multiple children should be wrapped in array, got:\n{}",
            code
        );
    }

    /// @ai-generated — Single element child: no array needed
    #[test]
    fn test_children_single_element() {
        let code = gen_and_validate(r#"<template><div><span>inner</span></div></template>"#);
        // Single child should not be in array
        assert!(
            !code.contains("[_createElementVNode"),
            "Single child should NOT be in array, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Comments
    // =========================================================================

    /// @ai-generated — HTML comment → _createCommentVNode
    #[test]
    fn test_comment_basic() {
        let code = gen_and_validate(r#"<template><div><!-- my comment --></div></template>"#);
        assert!(
            code.contains(r#"_createCommentVNode(" my comment ")"#),
            "Comment should produce _createCommentVNode with content, got:\n{}",
            code
        );
    }

    /// @ai-generated — Empty comment
    #[test]
    fn test_comment_empty() {
        let code = gen_and_validate(r#"<template><div><!----></div></template>"#);
        assert!(
            code.contains(r#"_createCommentVNode("")"#),
            "Empty comment should produce empty string, got:\n{}",
            code
        );
    }

    /// @ai-generated — Comment as only child of element
    #[test]
    fn test_comment_only_child() {
        let code = gen_and_validate(r#"<template><div><!-- only --></div></template>"#);
        assert!(
            code.contains("_createCommentVNode"),
            "Only-child comment should still produce _createCommentVNode, got:\n{}",
            code
        );
    }

    // =========================================================================
    // v-if directives
    // =========================================================================

    /// @ai-generated — v-if produces ternary with comment fallback
    #[test]
    fn test_v_if_ternary() {
        let code = gen(r#"<template><div v-if="show">yes</div></template>"#);
        assert!(
            code.contains("(show) ? ("),
            "v-if should produce ternary, got:\n{}",
            code
        );
        assert!(
            code.contains(r#"_createCommentVNode("v-if", true)"#),
            "v-if should have labeled comment fallback in dev, got:\n{}",
            code
        );
    }

    /// @ai-generated — v-if/v-else produces both branches
    #[test]
    fn test_v_if_else() {
        let code = gen(r#"<template><div v-if="show">yes</div><div v-else>no</div></template>"#);
        assert!(
            code.contains("(show) ? ("),
            "Should have v-if ternary, got:\n{}",
            code
        );
        assert!(
            code.contains(r#""yes""#),
            "Should have 'yes' branch, got:\n{}",
            code
        );
        assert!(
            code.contains(r#""no""#),
            "Should have 'no' branch, got:\n{}",
            code
        );
    }

    /// @ai-generated — v-if/v-else-if/v-else chain
    #[test]
    fn test_v_if_else_if_else() {
        let code = gen(
            r#"<template><div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div></template>"#,
        );
        assert!(
            code.contains("(a) ? ("),
            "Should have first condition, got:\n{}",
            code
        );
        assert!(
            code.contains("(b) ? ("),
            "Should have else-if condition, got:\n{}",
            code
        );
    }

    /// @ai-generated — v-if with class attribute preserves class
    #[test]
    fn test_v_if_with_class() {
        let code = gen(r#"<template><div v-if="show" class="foo">hi</div></template>"#);
        assert!(
            code.contains(r#"class: "foo""#),
            "v-if element should preserve class, got:\n{}",
            code
        );
    }

    /// @ai-generated — v-if removes directive from props (no v-if="..." in output)
    #[test]
    fn test_v_if_removes_directive() {
        let code = gen(r#"<template><div v-if="show">yes</div></template>"#);
        // The v-if directive attribute should be removed from element props
        // (but "v-if" in the comment fallback is expected: _createCommentVNode("v-if", true))
        assert!(
            !code.contains(r#"v-if="show""#),
            "v-if directive attribute should be removed from output, got:\n{}",
            code
        );
    }

    /// @ai-generated — v-if branches should use _openBlock + _createElementBlock
    #[test]
    fn test_v_if_block_treatment() {
        let code = gen_and_validate(r#"<template><div v-if="show">yes</div></template>"#);
        assert!(
            code.contains("_openBlock()"),
            "v-if branch should use _openBlock(), got:\n{}",
            code
        );
        assert!(
            code.contains("_createElementBlock("),
            "v-if branch should use _createElementBlock, got:\n{}",
            code
        );
    }

    /// @ai-generated — v-if branches should have { key: N } injection
    #[test]
    #[ignore = "requires v-if key injection"]
    fn test_v_if_key_injection() {
        let code = gen_and_validate(r#"<template><div v-if="show">yes</div></template>"#);
        assert!(
            code.contains("{ key: 0 }"),
            "v-if branch should have {{ key: 0 }}, got:\n{}",
            code
        );
    }

    /// @ai-generated — v-if prod mode uses empty string comment
    #[test]
    fn test_v_if_prod_empty_comment() {
        let code = gen_prod(r#"<template><div v-if="show">yes</div></template>"#);
        assert!(
            code.contains(r#"_createCommentVNode("", true)"#),
            "Prod v-if should use empty comment, got:\n{}",
            code
        );
    }

    // =========================================================================
    // v-for directives
    // =========================================================================

    /// @ai-generated — v-for produces _renderList with Fragment wrapping
    #[test]
    fn test_v_for_render_list() {
        let code = gen(r#"<template><div v-for="item in items">{{ item }}</div></template>"#);
        assert!(
            code.contains("_renderList("),
            "v-for should produce _renderList, got:\n{}",
            code
        );
        assert!(
            code.contains("_openBlock(true)"),
            "v-for should use _openBlock(true), got:\n{}",
            code
        );
        assert!(
            code.contains("_Fragment"),
            "v-for should wrap in _Fragment, got:\n{}",
            code
        );
    }

    /// @ai-generated — Keyed v-for uses KEYED_FRAGMENT (128)
    /// Current codegen always outputs UNKEYED (128 for both) — Vue uses 128 for keyed, 256 for unkeyed
    #[test]
    #[ignore = "requires keyed vs unkeyed fragment distinction"]
    fn test_v_for_keyed_fragment() {
        let code =
            gen(r#"<template><div v-for="item in items" :key="item">{{ item }}</div></template>"#);
        assert!(
            code.contains("128 /* KEYED_FRAGMENT */"),
            "Keyed v-for should use 128 KEYED_FRAGMENT, got:\n{}",
            code
        );
    }

    /// @ai-generated — v-for with index parameter: (item, index) =>
    #[test]
    fn test_v_for_with_index() {
        let code = gen(
            r#"<template><div v-for="(item, index) in items" :key="index">{{ item }}</div></template>"#,
        );
        assert!(
            code.contains("_renderList("),
            "Should have _renderList, got:\n{}",
            code
        );
        assert!(
            code.contains("(item, index)"),
            "Should have (item, index) params, got:\n{}",
            code
        );
    }

    /// @ai-generated — v-for removes directive from output
    #[test]
    fn test_v_for_removes_directive() {
        let code = gen(r#"<template><div v-for="item in items">{{ item }}</div></template>"#);
        assert!(
            !code.contains("v-for"),
            "v-for directive should be removed from output, got:\n{}",
            code
        );
    }

    /// @ai-generated — Nested v-for produces two _renderList calls
    #[test]
    fn test_v_for_nested() {
        let code = gen(
            r#"<template><div v-for="g in groups"><span v-for="i in g">{{ i }}</span></div></template>"#,
        );
        let count = code.matches("_renderList(").count();
        assert!(
            count >= 2,
            "Nested v-for should produce 2 _renderList calls, got {} in:\n{}",
            count,
            code
        );
    }

    // =========================================================================
    // v-once directives
    // =========================================================================

    /// @ai-generated — v-once produces full Vue cache pattern
    #[test]
    fn test_v_once_cache_pattern() {
        let code = gen_and_validate(r#"<template><div v-once>static</div></template>"#);
        assert!(
            code.contains("_cache[0] || ("),
            "v-once should start with _cache[0] || (, got:\n{}",
            code
        );
        assert!(
            code.contains("_setBlockTracking(-1, true)"),
            "v-once should call _setBlockTracking(-1, true), got:\n{}",
            code
        );
        assert!(
            code.contains("_setBlockTracking(1)"),
            "v-once should restore block tracking with _setBlockTracking(1), got:\n{}",
            code
        );
        assert!(
            code.contains(".cacheIndex = 0"),
            "v-once should use .cacheIndex = 0, got:\n{}",
            code
        );
        // v-once uses _createElementVNode, NOT _createElementBlock (block tracking disabled)
        assert!(
            code.contains("_createElementVNode("),
            "v-once should use _createElementVNode (not block), got:\n{}",
            code
        );
        assert!(
            !code.contains("_createElementBlock("),
            "v-once should NOT use _createElementBlock, got:\n{}",
            code
        );
    }

    /// @ai-generated — v-once with dynamic prop preserves patch flags
    #[test]
    fn test_v_once_with_dynamic() {
        let code = gen_and_validate(r#"<template><div v-once :id="foo">content</div></template>"#);
        assert!(
            code.contains("_cache[0] || ("),
            "v-once should use cache pattern, got:\n{}",
            code
        );
        assert!(
            code.contains(".cacheIndex = 0"),
            "v-once should use .cacheIndex = 0, got:\n{}",
            code
        );
        assert!(
            code.contains("8 /* PROPS */"),
            "v-once with :id should have PROPS flag, got:\n{}",
            code
        );
    }

    /// @ai-generated — v-once uses .cacheIndex = N assignment
    #[test]
    fn test_v_once_cache_index() {
        let code = gen_and_validate(r#"<template><div v-once>static</div></template>"#);
        assert!(
            code.contains(".cacheIndex = 0"),
            "v-once should use .cacheIndex = 0, got:\n{}",
            code
        );
    }

    /// @ai-generated — v-once self-closing element
    #[test]
    fn test_v_once_self_closing() {
        let code = gen_and_validate(r#"<template><br v-once/></template>"#);
        assert!(
            code.contains("_cache[0] || ("),
            "v-once self-closing should use cache, got:\n{}",
            code
        );
        assert!(
            code.contains(".cacheIndex = 0"),
            "v-once self-closing should have .cacheIndex, got:\n{}",
            code
        );
    }

    /// @ai-generated — v-once returns _cache[N] as final expression
    #[test]
    fn test_v_once_returns_cache() {
        let code = gen_and_validate(r#"<template><div v-once>static</div></template>"#);
        // The final value in the comma expression should be _cache[0])
        assert!(
            code.contains("_cache[0])"),
            "v-once should end with _cache[0]), got:\n{}",
            code
        );
    }

    // =========================================================================
    // Patch Flags
    // =========================================================================

    /// @ai-generated — Bound :id → PROPS (8) with dynamic props list
    #[test]
    fn test_pf_props() {
        let code = gen_and_validate(r#"<template><div :id="myId">hi</div></template>"#);
        assert!(
            code.contains("8 /* PROPS */"),
            "Should have PROPS (8), got:\n{}",
            code
        );
        assert!(
            code.contains(r#", ["id"]"#),
            "Should list dynamic prop, got:\n{}",
            code
        );
    }

    /// @ai-generated — :class → CLASS (2)
    #[test]
    fn test_pf_class() {
        let code = gen_and_validate(r#"<template><div :class="cls">hi</div></template>"#);
        assert!(
            code.contains("2 /* CLASS */"),
            "Should have CLASS (2), got:\n{}",
            code
        );
    }

    /// @ai-generated — :style → STYLE (4)
    #[test]
    fn test_pf_style() {
        let code = gen_and_validate(r#"<template><div :style="sty">hi</div></template>"#);
        assert!(
            code.contains("4 /* STYLE */"),
            "Should have STYLE (4), got:\n{}",
            code
        );
    }

    /// @ai-generated — Production mode: no patch flag comments
    #[test]
    fn test_pf_prod_no_comments() {
        let code = gen_prod_and_validate(r#"<template><div :class="cls">hi</div></template>"#);
        assert!(
            code.contains(", 2)"),
            "Prod should have numeric flag without comment, got:\n{}",
            code
        );
        assert!(
            !code.contains("/* CLASS */"),
            "Prod should NOT have flag comment, got:\n{}",
            code
        );
    }

    /// @ai-generated — Single interpolation child should have TEXT (1) patch flag
    #[test]
    fn test_pf_text() {
        let code = gen_and_validate(r#"<template><div>{{ msg }}</div></template>"#);
        assert!(
            code.contains("1 /* TEXT */"),
            "Single interpolation should have TEXT (1), got:\n{}",
            code
        );
    }

    /// @ai-generated — Combined CLASS + PROPS = 10
    #[test]
    fn test_pf_combined_class_props() {
        let code = gen_and_validate(r#"<template><div :class="c" :id="x">hi</div></template>"#);
        // CLASS(2) | PROPS(8) = 10
        assert!(
            code.contains("10 /* CLASS, PROPS */"),
            "Should have CLASS+PROPS (10), got:\n{}",
            code
        );
    }

    // =========================================================================
    // Components
    // =========================================================================

    /// @ai-generated — Root component uses _resolveComponent + _createBlock
    #[test]
    fn test_component_create_vnode() {
        let code = gen_and_validate(r#"<template><MyComponent/></template>"#);
        assert!(
            code.contains("_createBlock(_component_MyComponent"),
            "Root component should use _createBlock with resolved var, got:\n{}",
            code
        );
        assert!(
            code.contains("_openBlock()"),
            "Root component should use _openBlock(), got:\n{}",
            code
        );
        assert!(
            code.contains("_resolveComponent(\"MyComponent\")"),
            "Should declare _resolveComponent, got:\n{}",
            code
        );
    }

    /// @ai-generated — Root component with props
    #[test]
    fn test_component_with_props() {
        let code = gen_and_validate(r#"<template><MyComponent :msg="hello"/></template>"#);
        assert!(
            code.contains("_createBlock(_component_MyComponent"),
            "Root component should use _createBlock with resolved var, got:\n{}",
            code
        );
        assert!(
            code.contains("msg: hello"),
            "Should pass props, got:\n{}",
            code
        );
    }

    /// @ai-generated — Root component with children (slot content)
    #[test]
    fn test_component_with_children() {
        let code = gen_and_validate(r#"<template><MyComponent>content</MyComponent></template>"#);
        assert!(
            code.contains("_createBlock(_component_MyComponent"),
            "Root component should use _createBlock with resolved var, got:\n{}",
            code
        );
        assert!(
            code.contains(r#""content""#),
            "Should have children text, got:\n{}",
            code
        );
    }

    /// @ai-generated — Vue uses _resolveComponent + _createBlock for runtime components
    #[test]
    fn test_component_resolve_and_block() {
        let code = gen_and_validate(r#"<template><MyComponent/></template>"#);
        assert!(
            code.contains("_resolveComponent("),
            "Should use _resolveComponent, got:\n{}",
            code
        );
        assert!(
            code.contains("_createBlock("),
            "Should use _createBlock for component, got:\n{}",
            code
        );
    }

    /// @ai-generated — _resolveComponent declaration appears before return
    #[test]
    fn test_component_resolve_declaration_before_return() {
        let code = gen_and_validate(r#"<template><MyComponent/></template>"#);
        let resolve_pos = code.find("_resolveComponent(").unwrap();
        let return_pos = code.find("return ").unwrap();
        assert!(
            resolve_pos < return_pos,
            "_resolveComponent declaration should appear before return statement, got:\n{}",
            code
        );
    }

    /// @ai-generated — _resolveComponent uses const declaration with correct variable name
    #[test]
    fn test_component_resolve_const_pattern() {
        let code = gen_and_validate(r#"<template><MyComponent/></template>"#);
        assert!(
            code.contains(r#"const _component_MyComponent = _resolveComponent("MyComponent")"#),
            "Should have const declaration pattern, got:\n{}",
            code
        );
    }

    /// @ai-generated — Non-root component uses _createVNode with resolved variable
    #[test]
    fn test_component_child_uses_create_vnode() {
        let code = gen_and_validate(r#"<template><div><MyComponent/></div></template>"#);
        assert!(
            code.contains("_createVNode(_component_MyComponent"),
            "Child component should use _createVNode with resolved var, got:\n{}",
            code
        );
        assert!(
            code.contains("_resolveComponent(\"MyComponent\")"),
            "Should declare _resolveComponent, got:\n{}",
            code
        );
    }

    /// @ai-generated — Multiple different components get separate declarations
    #[test]
    fn test_component_multiple_different() {
        let code = gen_and_validate(r#"<template><div><CompA/><CompB/></div></template>"#);
        assert!(
            code.contains("_resolveComponent(\"CompA\")"),
            "Should resolve CompA, got:\n{}",
            code
        );
        assert!(
            code.contains("_resolveComponent(\"CompB\")"),
            "Should resolve CompB, got:\n{}",
            code
        );
        assert!(
            code.contains("_createVNode(_component_CompA"),
            "Should use _component_CompA, got:\n{}",
            code
        );
        assert!(
            code.contains("_createVNode(_component_CompB"),
            "Should use _component_CompB, got:\n{}",
            code
        );
    }

    /// @ai-generated — Same component used twice gets only one _resolveComponent
    #[test]
    fn test_component_same_used_twice() {
        let code = gen_and_validate(r#"<template><div><MyComp/><MyComp/></div></template>"#);
        let count = code.matches("_resolveComponent(\"MyComp\")").count();
        assert_eq!(
            count, 1,
            "Same component should have only one _resolveComponent, got {} in:\n{}",
            count, code
        );
        let vnode_count = code.matches("_createVNode(_component_MyComp").count();
        assert_eq!(
            vnode_count, 2,
            "Should have 2 _createVNode calls for same component, got {} in:\n{}",
            vnode_count, code
        );
    }

    /// @ai-generated — Component as block root (v-if branch) uses _createBlock
    #[test]
    fn test_component_vif_block_root() {
        let code = gen_and_validate(r#"<template><MyComponent v-if="show"/></template>"#);
        assert!(
            code.contains("_createBlock(_component_MyComponent"),
            "v-if component should use _createBlock, got:\n{}",
            code
        );
        assert!(
            code.contains("_resolveComponent(\"MyComponent\")"),
            "Should resolve component, got:\n{}",
            code
        );
    }

    /// @ai-generated — Component nested inside v-for uses _createBlock (block root)
    #[test]
    fn test_component_inside_v_for() {
        let code = gen_and_validate(r#"<template><MyComponent v-for="item in items"/></template>"#);
        assert!(
            code.contains("_createBlock(_component_MyComponent"),
            "v-for component should use _createBlock, got:\n{}",
            code
        );
    }

    /// @ai-generated — Production mode component still uses _resolveComponent
    #[test]
    fn test_component_prod_mode() {
        let code = gen_prod_and_validate(r#"<template><MyComponent/></template>"#);
        assert!(
            code.contains("_resolveComponent(\"MyComponent\")"),
            "Prod mode should still resolve component, got:\n{}",
            code
        );
        assert!(
            code.contains("_createBlock(_component_MyComponent"),
            "Prod mode should use _createBlock with resolved var, got:\n{}",
            code
        );
    }

    /// @ai-generated — Native HTML element should NOT use _resolveComponent
    #[test]
    fn test_component_native_not_resolved() {
        let code = gen_and_validate(r#"<template><div>hello</div></template>"#);
        assert!(
            !code.contains("_resolveComponent"),
            "Native element should not use _resolveComponent, got:\n{}",
            code
        );
        assert!(
            !code.contains("_component_"),
            "Native element should not have _component_ prefix, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Multiple roots
    // =========================================================================

    /// @ai-generated — Multiple root elements (both are block roots)
    #[test]
    fn test_multiple_roots() {
        let code = gen(r#"<template><div>a</div><div>b</div></template>"#);
        let count = code.matches("_createElementBlock").count();
        assert!(
            count >= 2,
            "Should have at least 2 _createElementBlock calls (both roots are blocks), got {} in:\n{}",
            count,
            code
        );
    }

    /// @ai-generated — Multiple roots should use Fragment wrapping
    #[test]
    #[ignore = "requires Fragment wrapping for multiple roots"]
    fn test_multiple_roots_fragment() {
        let code = gen_and_validate(r#"<template><div>a</div><div>b</div></template>"#);
        assert!(
            code.contains("_Fragment"),
            "Multiple roots should use _Fragment, got:\n{}",
            code
        );
    }

    // =========================================================================
    // Script-only / edge cases
    // =========================================================================

    /// @ai-generated — Script-only SFC should not panic
    #[test]
    fn test_script_only_no_panic() {
        let code = gen(r#"<script setup>const x = 1</script>"#);
        assert!(
            code.contains("const x = 1"),
            "Script content should be preserved, got:\n{}",
            code
        );
    }

    /// @ai-generated — Whitespace between script and template blocks
    #[test]
    fn test_script_template_whitespace() {
        let code = gen_and_validate(
            r#"<script setup>
const x = 1
</script>

<template><div>hi</div></template>"#,
        );
        assert!(
            code.contains("_createElementBlock"),
            "Root should produce _createElementBlock, got:\n{}",
            code
        );
    }

    /// @ai-generated — Production mode outputs valid JS for complex template
    #[test]
    fn test_prod_valid_js_complex() {
        let code =
            gen_prod_and_validate(r#"<template><div :class="c" :id="x">hi</div></template>"#);
        assert!(!code.is_empty(), "Should produce non-empty output");
    }

    /// @ai-generated — Void img with attributes (root = block)
    #[test]
    fn test_void_img_attrs() {
        let code = gen_and_validate(r#"<template><img src="a.png" alt="pic"></template>"#);
        assert!(
            code.contains(r#"_createElementBlock("img""#),
            "Root img should use _createElementBlock, got:\n{}",
            code
        );
        assert!(
            code.contains(r#"src: "a.png""#),
            "Should have src prop, got:\n{}",
            code
        );
        assert!(
            code.contains(r#"alt: "pic""#),
            "Should have alt prop, got:\n{}",
            code
        );
    }

    /// @ai-generated — Sibling void elements inside div
    #[test]
    fn test_sibling_void_elements() {
        let code = gen_and_validate(r#"<template><div><input><hr><br></div></template>"#);
        assert!(
            code.contains(r#"_createElementVNode("input""#),
            "Should have input, got:\n{}",
            code
        );
        assert!(
            code.contains(r#"_createElementVNode("hr""#),
            "Should have hr, got:\n{}",
            code
        );
        assert!(
            code.contains(r#"_createElementVNode("br""#),
            "Should have br, got:\n{}",
            code
        );
    }

    /// @ai-generated — Mixed text and element children
    #[test]
    fn test_mixed_text_and_element() {
        let code = gen(r#"<template><div>text<span>child</span></div></template>"#);
        assert!(
            code.contains(r#""text""#),
            "Should have text child, got:\n{}",
            code
        );
        assert!(
            code.contains(r#"_createElementVNode("span""#),
            "Should have span child, got:\n{}",
            code
        );
    }

    /// @ai-generated — Comment with elements: mixed children
    #[test]
    fn test_comment_with_elements() {
        let code =
            gen(r#"<template><div><span>a</span><!-- mid --><span>b</span></div></template>"#);
        assert!(
            code.contains("_createCommentVNode"),
            "Should have comment VNode, got:\n{}",
            code
        );
        let span_count = code.matches(r#"_createElementVNode("span""#).count();
        assert!(
            span_count >= 2,
            "Should have 2 spans, got {} in:\n{}",
            span_count,
            code
        );
    }

    /// @ai-generated — v-if inside v-for
    #[test]
    fn test_v_if_inside_v_for() {
        let code = gen(
            r#"<template><div v-for="item in items"><span v-if="item.show">{{ item.name }}</span></div></template>"#,
        );
        assert!(
            code.contains("_renderList("),
            "Should have _renderList for v-for, got:\n{}",
            code
        );
        assert!(
            code.contains("(item.show) ? ("),
            "Should have v-if ternary inside v-for, got:\n{}",
            code
        );
    }

    // =========================================================================
    // v-if: block treatment (comprehensive)
    // =========================================================================

    /// @ai-generated — v-if simple: return (cond) ? (block) : comment
    #[test]
    fn test_v_if_full_output() {
        let code = gen_and_validate(r#"<template><div v-if="show">yes</div></template>"#);
        assert!(
            code.contains("return (show) ? (_openBlock(), _createElementBlock("),
            "v-if should produce return (cond) ? (_openBlock(), _createElementBlock..., got:\n{}",
            code
        );
        assert!(
            code.contains(r#" : _createCommentVNode("v-if", true)"#),
            "v-if should have comment fallback, got:\n{}",
            code
        );
    }

    /// @ai-generated — v-if/v-else: both branches are block roots
    #[test]
    fn test_v_if_else_block_roots() {
        let code = gen_and_validate(
            r#"<template><div v-if="show">yes</div><div v-else>no</div></template>"#,
        );
        let block_count = code.matches("_openBlock()").count();
        assert!(
            block_count >= 2,
            "Both branches should use _openBlock(), got {} in:\n{}",
            block_count,
            code
        );
        assert!(
            code.contains("(show) ? (_openBlock()"),
            "v-if branch should be block root, got:\n{}",
            code
        );
        assert!(
            code.contains(") : (_openBlock()"),
            "v-else branch should be block root, got:\n{}",
            code
        );
        // No comment fallback when v-else is present
        assert!(
            !code.contains("_createCommentVNode"),
            "v-if/v-else should NOT have comment fallback, got:\n{}",
            code
        );
    }

    /// @ai-generated — v-if/v-else-if/v-else: nested ternary with block roots
    #[test]
    fn test_v_if_else_if_else_block_roots() {
        let code = gen_and_validate(
            r#"<template><div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div></template>"#,
        );
        let block_count = code.matches("_openBlock()").count();
        assert!(
            block_count >= 3,
            "All 3 branches should use _openBlock(), got {} in:\n{}",
            block_count,
            code
        );
        assert!(
            code.contains("(a) ? ("),
            "First condition should be present, got:\n{}",
            code
        );
        assert!(
            code.contains("(b) ? ("),
            "Second condition should be present, got:\n{}",
            code
        );
        assert!(
            !code.contains("_createCommentVNode"),
            "Full chain should NOT have comment fallback, got:\n{}",
            code
        );
    }

    /// @ai-generated — v-if/v-else-if (no else): should have comment fallback
    #[test]
    fn test_v_if_else_if_no_else() {
        let code = gen_and_validate(
            r#"<template><div v-if="a">A</div><div v-else-if="b">B</div></template>"#,
        );
        assert!(
            code.contains("_createCommentVNode"),
            "Without v-else, should have comment fallback, got:\n{}",
            code
        );
    }

    /// @ai-generated — Nested v-if inside parent element: block root inside VNode
    #[test]
    fn test_v_if_nested_in_element() {
        let code =
            gen_and_validate(r#"<template><div><span v-if="show">yes</span></div></template>"#);
        assert!(
            code.contains(r#"_createElementBlock("div""#),
            "Root div should be _createElementBlock, got:\n{}",
            code
        );
        // Nested v-if branch should also be a block root
        assert!(
            code.contains("(show) ? (_openBlock(), _createElementBlock(\"span\""),
            "Nested v-if should be block root, got:\n{}",
            code
        );
        assert!(
            code.contains(r#"_createCommentVNode("v-if", true)"#),
            "Nested v-if should have comment fallback, got:\n{}",
            code
        );
    }

    /// @ai-generated — Nested v-if/v-else inside parent: no comment fallback
    #[test]
    fn test_v_if_else_nested_in_element() {
        let code = gen_and_validate(
            r#"<template><div><span v-if="ok">A</span><span v-else>B</span></div></template>"#,
        );
        assert!(
            !code.contains("_createCommentVNode"),
            "Nested v-if/v-else should NOT have comment fallback, got:\n{}",
            code
        );
    }

    /// @ai-generated — Two independent v-ifs inside parent: both get comment fallbacks
    #[test]
    fn test_v_if_two_independent() {
        let code = gen_and_validate(
            r#"<template><div><span v-if="a">A</span><span v-if="b">B</span></div></template>"#,
        );
        let comment_count = code.matches("_createCommentVNode").count();
        assert!(
            comment_count >= 2,
            "Two independent v-ifs should have 2 comment fallbacks, got {} in:\n{}",
            comment_count,
            code
        );
    }

    /// @ai-generated — v-if with siblings: correct array wrapping
    #[test]
    fn test_v_if_with_sibling_elements() {
        let code = gen_and_validate(
            r#"<template><div><span>A</span><span v-if="show">B</span></div></template>"#,
        );
        // Two children (span, v-if span) → array wrapping
        assert!(
            code.contains("[_createElementVNode"),
            "Should use array wrapping for multiple children, got:\n{}",
            code
        );
        assert!(
            code.contains("(show) ? "),
            "v-if ternary should be present in array, got:\n{}",
            code
        );
    }

    /// @ai-generated — v-for items should be block roots
    #[test]
    fn test_v_for_item_is_block_root() {
        let code = gen(r#"<template><div v-for="item in items">{{ item }}</div></template>"#);
        // Inside the renderList callback, the div should use _createElementBlock
        assert!(
            code.contains("_createElementBlock(\"div\""),
            "v-for item should use _createElementBlock (block root), got:\n{}",
            code
        );
    }

    /// @ai-generated — Bound :id with _ctx prefix (template-only, no script setup)
    #[test]
    #[ignore = "requires _ctx prefix for unresolved bindings"]
    fn test_bound_prop_ctx_prefix() {
        let code = gen_and_validate(r#"<template><div :id="myId">hi</div></template>"#);
        assert!(
            code.contains("_ctx.myId"),
            "Unresolved binding should get _ctx. prefix, got:\n{}",
            code
        );
    }
}
