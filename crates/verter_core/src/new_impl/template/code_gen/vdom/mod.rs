//! VDOM (Virtual DOM) template code generation backend.

pub mod comment;
pub mod directives;
pub mod element;
pub mod interpolation;
pub mod props;
pub mod text;

use crate::new_impl::ast::types::{
    AstNodeKind, CommentNode, ElementNode, ElementNodeConditionKind, InterpolationNode,
    TemplateAst, TextNode,
};
use crate::new_impl::syntax::types::RootNodeTemplate;
use crate::new_impl::template::oxc::types::{OxcParsedElement, OxcParsedExpression};
use crate::new_impl::types::NodeId;

use super::binding::BindingResolver;
use super::shared::helpers::{self, VdomHelper};
use super::types::{ChildKind, ChildRecord, CodeGenOutput, ConditionChainRole, ScopeClose};
use super::{TemplateCodeGen, TemplateCodeGenOptions};

/// VDOM code generation backend.
///
/// Produces `_createElementVNode()` / `_createElementBlock()` calls with
/// patch flags, dynamic props arrays, and proper children wrapping.
///
/// Child records are built on-demand from the AST in `leave_element` /
/// `leave_template`, eliminating the need for a per-element state stack.
pub struct VdomCodeGen<'ast, 'alloc> {
    /// Reference to the template AST arena for O(1) node lookups.
    ast: &'ast TemplateAst,
    resolver: BindingResolver<'alloc>,
    options: TemplateCodeGenOptions,
    /// Reusable buffer for building open/close tag strings.
    /// Uses `std::mem::take` pattern to avoid per-element allocation.
    buf: String,
    /// Scope close stack for structural directives (v-if, v-for).
    /// Pushed in `enter_element`, popped in `leave_element`.
    scope_closes: Vec<Option<ScopeClose>>,
}

impl<'ast, 'alloc> VdomCodeGen<'ast, 'alloc> {
    pub fn new(
        ast: &'ast TemplateAst,
        resolver: BindingResolver<'alloc>,
        options: &TemplateCodeGenOptions,
    ) -> Self {
        Self {
            ast,
            resolver,
            options: options.clone(),
            buf: String::with_capacity(128),
            scope_closes: Vec::new(),
        }
    }

    /// Check whether the next *element* sibling of `id` is a v-else-if or
    /// v-else continuation. Scans forward from `index_in_parent + 1`,
    /// skipping non-element nodes (text, comments, interpolations) that
    /// commonly appear as whitespace between conditional elements.
    ///
    /// Used to downgrade `IfTernary` to `ElseIfTernary` (or upgrade
    /// `ElseIfTernary` to `IfTernary`) so the scope close emits the
    /// correct suffix.
    fn has_next_condition_sibling(&self, id: NodeId) -> bool {
        let node = &self.ast.nodes[id.0];

        let children = match node.parent {
            None => self
                .ast
                .root
                .content
                .as_ref()
                .map(|c| c.children.as_slice())
                .unwrap_or(&[]),
            Some(parent_id) => {
                if let AstNodeKind::Element(ref parent_el) = self.ast.nodes[parent_id.0].kind {
                    parent_el
                        .content
                        .as_ref()
                        .map(|c| c.children.as_slice())
                        .unwrap_or(&[])
                } else {
                    return false;
                }
            }
        };

        // Scan forward, skipping non-element siblings (text, comments, etc.)
        for idx in (node.index_in_parent + 1)..children.len() {
            let next_node = &self.ast.nodes[children[idx].0];
            if let AstNodeKind::Element(ref next_el) = next_node.kind {
                if let Some(cond) = &next_el.v_condition {
                    return matches!(
                        cond.kind,
                        ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else
                    );
                }
                return false; // Next element has no condition — stop scanning
            }
            // Text/Comment/Interpolation: skip and continue scanning
        }
        false
    }

    /// Build child records from AST children (O(n) scan).
    ///
    /// Replaces the old per-element `ElementState.children` accumulator.
    /// Children are classified on-demand from the AST when the parent's
    /// leave phase needs them.
    fn build_child_records(&self, children: &[NodeId], source: &str) -> Vec<ChildRecord> {
        let mut records = Vec::with_capacity(children.len());
        for &child_id in children {
            let node = &self.ast.nodes[child_id.0];
            match &node.kind {
                AstNodeKind::Text(text_node) => {
                    let content = &source[text_node.start as usize..text_node.end as usize];
                    if let Some(kind) = text::classify_text_kind(content) {
                        records.push(ChildRecord {
                            start: text_node.start,
                            end: text_node.end,
                            kind,
                            condition: None,
                            condition_prefix: None,
                        });
                    }
                }
                AstNodeKind::Interpolation(interp) => {
                    records.push(ChildRecord {
                        start: interp.start,
                        end: interp.end,
                        kind: ChildKind::Interpolation,
                        condition: None,
                        condition_prefix: None,
                    });
                }
                AstNodeKind::Element(el) => {
                    let end = el
                        .tag_close
                        .as_ref()
                        .map(|tc| tc.end)
                        .unwrap_or(el.tag_open.end);
                    let condition = el.v_condition.as_ref().map(|c| match c.kind {
                        ElementNodeConditionKind::If => ConditionChainRole::Start,
                        ElementNodeConditionKind::ElseIf | ElementNodeConditionKind::Else => {
                            ConditionChainRole::Continuation
                        }
                    });
                    records.push(ChildRecord {
                        start: el.tag_open.start,
                        end,
                        kind: ChildKind::Element,
                        condition,
                        condition_prefix: None,
                    });
                }
                AstNodeKind::Comment(comment) => {
                    if self.options.comments {
                        records.push(ChildRecord {
                            start: comment.start,
                            end: comment.end,
                            kind: ChildKind::Comment,
                            condition: None,
                            condition_prefix: None,
                        });
                    }
                }
            }
        }
        records
    }
}

impl<'ast, 'alloc> TemplateCodeGen<'alloc> for VdomCodeGen<'ast, 'alloc> {
    fn enter_template(
        &mut self,
        _root: &RootNodeTemplate,
        _source: &'alloc str,
        _out: &mut CodeGenOutput<'alloc>,
    ) {
        // Open tag overwrite is deferred to leave_template where we have
        // full context (children count, v-if status) to emit the correct
        // combined prefix (function signature + return + openBlock).
    }

    fn leave_template(
        &mut self,
        root: &RootNodeTemplate,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        let root_children = root
            .content
            .as_ref()
            .map(|c| c.children.as_slice())
            .unwrap_or(&[]);
        let mut children = self.build_child_records(root_children, source);

        // Resolve whitespace at root level. Leading and trailing whitespace
        // are dropped from the children vec WITHOUT overwrites — the combined
        // open/close tag overwrites below cover those source regions.
        // Interior whitespace is resolved with overwrites as usual.
        {
            // Drop leading whitespace (no overwrite)
            let leading = children
                .iter()
                .take_while(|c| element::is_whitespace_kind_pub(c.kind))
                .count();
            children.drain(..leading);

            // Drop trailing whitespace (no overwrite)
            while children
                .last()
                .is_some_and(|c| element::is_whitespace_kind_pub(c.kind))
            {
                children.pop();
            }

            // Resolve interior whitespace (with overwrites)
            let mut i = 0;
            while i < children.len() {
                match children[i].kind {
                    ChildKind::WhitespaceNewline => {
                        let removed = children.remove(i);
                        out.overwrite(removed.start, removed.end, "");
                    }
                    ChildKind::WhitespaceSpace => {
                        out.overwrite(children[i].start, children[i].end, " ");
                        children[i].kind = ChildKind::Text;
                        i += 1;
                    }
                    _ => {
                        i += 1;
                    }
                }
            }
        }

        // Function signature prefix
        let fn_sig = if self.options.is_inline {
            "return (_ctx,_cache) => {\n"
        } else {
            "function render(_ctx, _cache, $props, $setup, $data, $options) {\n"
        };

        // Determine close tag region
        let (close_start, close_end) = match root.tag_close.as_ref() {
            Some(tc) => (tc.start, tc.end),
            None => {
                let pos = root
                    .content
                    .as_ref()
                    .map(|c| c.end)
                    .unwrap_or(root.tag_open.end);
                (pos, pos)
            }
        };

        // Count effective roots: v-if chains collapse into a single root.
        let effective_count = children
            .iter()
            .filter(|c| c.condition != Some(ConditionChainRole::Continuation))
            .count();

        let tag_open = &root.tag_open;

        match effective_count {
            0 => {
                // Empty template — overwrite everything
                let mut buf = String::with_capacity(fn_sig.len() + 16);
                buf.push_str(fn_sig);
                buf.push_str("return null\n}");
                out.overwrite(tag_open.start, close_end, &buf);
            }
            1 => {
                let child = &children[0];
                let is_v_if = child.condition == Some(ConditionChainRole::Start);

                if is_v_if {
                    // Root-level v-if chain — the ternary from enter_element
                    // is the return value. Overwrite open tag + leading ws with
                    // the function signature + return, so `return` comes BEFORE
                    // the `(expr) ? ` prefix from enter_element.
                    let mut prefix = String::with_capacity(fn_sig.len() + 8);
                    prefix.push_str(fn_sig);
                    prefix.push_str("return ");
                    out.overwrite(tag_open.start, child.start, &prefix);
                    out.overwrite(close_start, close_end, "\n}");
                } else {
                    // Single root — block root with _openBlock + _createElementBlock
                    out.add_vdom_import(VdomHelper::OpenBlock);
                    let mut prefix = String::with_capacity(fn_sig.len() + 24);
                    prefix.push_str(fn_sig);
                    prefix.push_str("return (_openBlock(), ");
                    out.overwrite(tag_open.start, child.start, &prefix);
                    out.overwrite(close_start, close_end, ")\n}");
                }
            }
            _ => {
                // Multi-root — wrap in Fragment
                out.add_vdom_import(VdomHelper::OpenBlock);
                out.add_vdom_import(VdomHelper::CreateElementBlock);
                out.add_vdom_import(VdomHelper::Fragment);

                // Prefix: function sig + return + openBlock + Fragment + array open
                let mut prefix = String::with_capacity(fn_sig.len() + 80);
                prefix.push_str(fn_sig);
                prefix.push_str("return (_openBlock(), _createElementBlock(_Fragment, null, [\n  ");
                out.overwrite(tag_open.start, children[0].start, &prefix);

                // Separators between children.
                // Commas are prepended at the PREVIOUS child's end position
                // (not the current child's start) to avoid ordering conflicts
                // with v-if condition prefixes at the same position.
                // v-else-if / v-else children are connected to their v-if
                // via the ternary `: ` — they do NOT get comma separators.
                let mut prev_end = children[0].end;
                for child in children.iter().skip(1) {
                    let is_continuation = child.condition == Some(ConditionChainRole::Continuation);
                    if !is_continuation {
                        out.prepend_static(prev_end, ",\n  ");
                    }
                    prev_end = child.end;
                }

                // Close fragment + render function
                let flag_str = helpers::format_patch_flag(
                    helpers::PATCH_STABLE_FRAGMENT,
                    self.options.is_production,
                    |s| out.alloc_str(s),
                );
                let mut close_buf = String::with_capacity(32);
                close_buf.push_str("\n], ");
                close_buf.push_str(flag_str);
                close_buf.push_str("))\n}");
                out.overwrite(close_start, close_end, &close_buf);
            }
        }
    }

    fn enter_element(
        &mut self,
        id: NodeId,
        element: &ElementNode,
        _oxc: Option<&OxcParsedElement<'alloc>>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        helpers::debug_assert_element_bounds(
            source,
            element.tag_open.start,
            element.tag_open.end,
            element.tag_open.name_end,
        );
        // Process structural directives: v-if/v-else-if/v-else, v-for
        if let Some(condition) = &element.v_condition {
            let (prefix, mut close) = directives::build_condition_prefix(condition, source);
            // Adjust scope close based on whether there's a continuation sibling.
            //
            // If this v-if has a v-else-if/v-else continuation after it,
            // downgrade IfTernary → ElseIfTernary so the scope close emits
            // ` : ` instead of the comment fallback.
            //
            // Conversely, if a v-else-if has NO continuation after it (end of
            // chain without v-else), upgrade ElseIfTernary → IfTernary so the
            // scope close emits `_createCommentVNode("v-if", true)` as the
            // false branch of the ternary.
            let has_next = self.has_next_condition_sibling(id);
            if close == ScopeClose::IfTernary && has_next {
                close = ScopeClose::ElseIfTernary;
            } else if close == ScopeClose::ElseIfTernary && !has_next {
                close = ScopeClose::IfTernary;
            }
            directives::collect_scope_imports(&close, out);
            if !prefix.is_empty() {
                out.prepend_alloc(element.tag_open.start, &prefix);
            }
            self.scope_closes.push(Some(close));
        } else if let Some(v_for) = &element.v_for {
            // Check if element has a :key prop
            let is_keyed = element.props.iter().any(|p| {
                if !p.is_directive {
                    return false;
                }
                if let (Some(as_), Some(ae)) = (p.arg_start, p.arg_end) {
                    &source[as_ as usize..ae as usize] == "key"
                } else {
                    false
                }
            });
            let (prefix, close) = directives::build_for_prefix(v_for, source, is_keyed);
            directives::collect_scope_imports(&close, out);
            out.prepend_alloc(element.tag_open.start, &prefix);
            self.scope_closes.push(Some(close));
        } else {
            self.scope_closes.push(None);
        }
    }

    fn leave_element(
        &mut self,
        _id: NodeId,
        el: &ElementNode,
        oxc: Option<&OxcParsedElement<'alloc>>,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        helpers::debug_assert_element_bounds(
            source,
            el.tag_open.start,
            el.tag_open.end,
            el.tag_open.name_end,
        );
        let el_children = el
            .content
            .as_ref()
            .map(|c| c.children.as_slice())
            .unwrap_or(&[]);
        let mut children = self.build_child_records(el_children, source);
        // Take the reusable buffer, use it, then put it back (std::mem::take pattern)
        let mut buf = std::mem::take(&mut self.buf);
        let record = element::process_element_leave(
            el,
            oxc,
            &mut children,
            source,
            out,
            &self.options,
            &mut buf,
        );
        buf.clear();
        self.buf = buf;

        // Emit scope close suffix for structural directives
        if let Some(scope_close) = self.scope_closes.pop().flatten() {
            let suffix =
                directives::format_scope_close(&scope_close, self.options.is_production, out);
            if !suffix.is_empty() {
                out.prepend_alloc(record.end, &suffix);
            }
        }
    }

    fn visit_text(
        &mut self,
        _id: NodeId,
        text_node: &TextNode,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        helpers::debug_assert_slice_bounds(source, text_node.start, text_node.end, "visit_text");
        // Apply text overwrites (condensation, escaping).
        // Child classification is handled by build_child_records from the AST.
        let _ = text::process_text(text_node, source, out);
    }

    fn visit_interpolation(
        &mut self,
        _id: NodeId,
        interp: &InterpolationNode,
        oxc: &OxcParsedExpression<'alloc>,
        _source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        // Apply delimiter overwrites and binding patches.
        // Child classification is handled by build_child_records from the AST.
        let _ = interpolation::process_interpolation(interp, oxc, &self.resolver, out);
    }

    fn visit_comment(
        &mut self,
        _id: NodeId,
        comment_node: &CommentNode,
        source: &'alloc str,
        out: &mut CodeGenOutput<'alloc>,
    ) {
        helpers::debug_assert_slice_bounds(
            source,
            comment_node.start,
            comment_node.end,
            "visit_comment",
        );
        // Apply comment overwrites (or removal if disabled).
        // Child classification is handled by build_child_records from the AST.
        let _ = comment::process_comment(comment_node, source, self.options.comments, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::new_impl::ast::types::{
        AstNode, ChildrenFlag, ChildrenMode, ElementContent, PropFlag, TagType, TemplateAst,
    };
    use crate::new_impl::syntax::types::RootNodeTemplateContent;
    use crate::new_impl::types::NodeTag;
    use oxc_allocator::Allocator;
    use rustc_hash::FxHashMap;
    use smallvec::SmallVec;

    /// Create a minimal empty TemplateAst for tests that don't need AST lookups.
    fn make_empty_ast(root: &RootNodeTemplate) -> TemplateAst {
        TemplateAst {
            nodes: Vec::new(),
            root: root.clone(),
        }
    }

    /// Create a minimal ElementNode for test ASTs.
    fn make_simple_element(
        open_start: u32,
        open_end: u32,
        open_name_end: u32,
        close_start: u32,
        close_end: u32,
        close_name_end: u32,
    ) -> crate::new_impl::ast::types::ElementNode {
        crate::new_impl::ast::types::ElementNode {
            tag_open: NodeTag {
                start: open_start,
                end: open_end,
                name_end: open_name_end,
            },
            tag_close: Some(NodeTag {
                start: close_start,
                end: close_end,
                name_end: close_name_end,
            }),
            tag_type: TagType::Element,
            is_self_closing: false,
            props: Vec::new(),
            content: Some(ElementContent {
                start: open_end,
                end: close_start,
                children: SmallVec::new(),
            }),
            v_condition: None,
            v_for: None,
            v_slot: None,
            v_once: None,
            v_ref: None,
            prop_flag: PropFlag::empty(),
            children_flag: ChildrenFlag::empty(),
            children_mode: ChildrenMode::Empty,
        }
    }

    fn make_options_standalone() -> TemplateCodeGenOptions {
        TemplateCodeGenOptions {
            is_inline: false,
            is_production: false,
            ..Default::default()
        }
    }

    fn make_options_inline() -> TemplateCodeGenOptions {
        TemplateCodeGenOptions {
            is_inline: true,
            is_production: false,
            ..Default::default()
        }
    }

    fn make_resolver(_alloc: &Allocator) -> BindingResolver<'_> {
        BindingResolver::new(FxHashMap::default(), false)
    }

    fn make_root(
        tag_open: NodeTag,
        tag_close: Option<NodeTag>,
        content: Option<RootNodeTemplateContent>,
    ) -> RootNodeTemplate {
        RootNodeTemplate {
            tag_open,
            tag_close,
            lang: None,
            attributes: Vec::new(),
            content,
        }
    }

    fn apply_output<'a>(source: &str, out: CodeGenOutput<'a>, alloc: &'a Allocator) -> String {
        let mut ct = crate::code_transform::CodeTransform::new(source, alloc);
        out.apply_to(&mut ct);
        ct.build_string()
    }

    // ==================== enter_template ====================

    #[test]
    fn enter_template_standalone_defers_to_leave() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let options = make_options_standalone();
        let resolver = make_resolver(&alloc);

        let root = make_root(
            NodeTag {
                start: 0,
                end: 10,
                name_end: 9,
            },
            None,
            None,
        );
        let ast = make_empty_ast(&root);
        let mut gen = VdomCodeGen::new(&ast, resolver, &options);
        gen.enter_template(&root, "", &mut out);

        // Open tag overwrite is deferred to leave_template
        assert_eq!(out.overwrites.len(), 0);
    }

    #[test]
    fn enter_template_inline_defers_to_leave() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let options = make_options_inline();
        let resolver = make_resolver(&alloc);

        let root = make_root(
            NodeTag {
                start: 0,
                end: 10,
                name_end: 9,
            },
            None,
            None,
        );
        let ast = make_empty_ast(&root);
        let mut gen = VdomCodeGen::new(&ast, resolver, &options);
        gen.enter_template(&root, "", &mut out);

        // Open tag overwrite is deferred to leave_template
        assert_eq!(out.overwrites.len(), 0);
    }

    // ==================== leave_template: empty ====================

    #[test]
    fn leave_template_empty_returns_null() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let options = make_options_standalone();
        let resolver = make_resolver(&alloc);

        // <template></template>  (0-10 open, 10-21 close)
        let source = "<template></template>";
        let root = make_root(
            NodeTag {
                start: 0,
                end: 10,
                name_end: 9,
            },
            Some(NodeTag {
                start: 10,
                end: 21,
                name_end: 20,
            }),
            Some(RootNodeTemplateContent {
                start: 10,
                end: 10,
                children: SmallVec::new(),
            }),
        );
        let ast = make_empty_ast(&root);
        let mut gen = VdomCodeGen::new(&ast, resolver, &options);

        gen.enter_template(&root, source, &mut out);
        gen.leave_template(&root, source, &mut out);

        let result = apply_output(source, out, &alloc);
        assert!(result.contains("return null"));
        assert!(result.ends_with('}'));
    }

    // ==================== leave_template: single root ====================

    #[test]
    fn leave_template_single_root_prepends_return() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let options = make_options_standalone();
        let resolver = make_resolver(&alloc);

        // Simulate: <template><div></div></template>
        // positions: 0-10 open, 10-15 <div>, 15-21 </div>, 21-32 close
        let source = "<template><div></div></template>";
        let root = make_root(
            NodeTag {
                start: 0,
                end: 10,
                name_end: 9,
            },
            Some(NodeTag {
                start: 21,
                end: 32,
                name_end: 31,
            }),
            Some(RootNodeTemplateContent {
                start: 10,
                end: 21,
                children: SmallVec::from_elem(NodeId(0), 1),
            }),
        );
        let ast = TemplateAst {
            nodes: vec![AstNode {
                kind: AstNodeKind::Element(Box::new(make_simple_element(10, 15, 14, 15, 21, 20))),
                parent: None,
                index_in_parent: 0,
            }],
            root,
        };
        let mut gen = VdomCodeGen::new(&ast, resolver, &options);

        gen.enter_template(&ast.root, source, &mut out);
        gen.leave_template(&ast.root, source, &mut out);

        let result = apply_output(source, out, &alloc);
        // Open tag replaced with function signature
        assert!(result.starts_with("function render("));
        // Single root uses block root: _openBlock() wrapper
        assert!(
            result.contains("return (_openBlock(), "),
            "Expected _openBlock() for single root, got: {result}"
        );
        // Close tag replaced with closing paren + newline + "}"
        assert!(result.ends_with(")\n}"));
    }

    // ==================== leave_template: multi root ====================

    #[test]
    fn leave_template_multi_root_wraps_in_fragment() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let options = make_options_standalone();
        let resolver = make_resolver(&alloc);

        // <template><div></div><span></span></template>
        // 0-10 open, 10-15 <div>, 15-21 </div>, 21-27 <span>, 27-34 </span>, 34-45 close
        let source = "<template><div></div><span></span></template>";
        let root = make_root(
            NodeTag {
                start: 0,
                end: 10,
                name_end: 9,
            },
            Some(NodeTag {
                start: 34,
                end: 45,
                name_end: 44,
            }),
            Some(RootNodeTemplateContent {
                start: 10,
                end: 34,
                children: SmallVec::from_slice(&[NodeId(0), NodeId(1)]),
            }),
        );
        let ast = TemplateAst {
            nodes: vec![
                AstNode {
                    kind: AstNodeKind::Element(Box::new(make_simple_element(
                        10, 15, 14, 15, 21, 20,
                    ))),
                    parent: None,
                    index_in_parent: 0,
                },
                AstNode {
                    kind: AstNodeKind::Element(Box::new(make_simple_element(
                        21, 27, 26, 27, 34, 33,
                    ))),
                    parent: None,
                    index_in_parent: 1,
                },
            ],
            root,
        };
        let mut gen = VdomCodeGen::new(&ast, resolver, &options);

        gen.enter_template(&ast.root, source, &mut out);
        gen.leave_template(&ast.root, source, &mut out);

        let result = apply_output(source, out, &alloc);
        assert!(result.contains("_openBlock()"));
        assert!(result.contains("_createElementBlock(_Fragment, null, ["));
        assert!(result.contains("64 /* STABLE_FRAGMENT */"));
        assert!(result.ends_with("))\n}"));
    }

    #[test]
    fn leave_template_multi_root_production_no_comment() {
        let alloc = Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let options = TemplateCodeGenOptions {
            is_inline: false,
            is_production: true,
            ..Default::default()
        };
        let resolver = make_resolver(&alloc);

        // <template><div></div><span></span></template>
        let source = "<template><div></div><span></span></template>";
        let root = make_root(
            NodeTag {
                start: 0,
                end: 10,
                name_end: 9,
            },
            Some(NodeTag {
                start: 34,
                end: 45,
                name_end: 44,
            }),
            Some(RootNodeTemplateContent {
                start: 10,
                end: 34,
                children: SmallVec::from_slice(&[NodeId(0), NodeId(1)]),
            }),
        );
        let ast = TemplateAst {
            nodes: vec![
                AstNode {
                    kind: AstNodeKind::Element(Box::new(make_simple_element(
                        10, 15, 14, 15, 21, 20,
                    ))),
                    parent: None,
                    index_in_parent: 0,
                },
                AstNode {
                    kind: AstNodeKind::Element(Box::new(make_simple_element(
                        21, 27, 26, 27, 34, 33,
                    ))),
                    parent: None,
                    index_in_parent: 1,
                },
            ],
            root,
        };
        let mut gen = VdomCodeGen::new(&ast, resolver, &options);

        gen.enter_template(&ast.root, source, &mut out);
        gen.leave_template(&ast.root, source, &mut out);

        let result = apply_output(source, out, &alloc);
        // Production: no comment after 64
        assert!(result.contains("\n], 64)"));
        assert!(!result.contains("/*"));
    }
}
