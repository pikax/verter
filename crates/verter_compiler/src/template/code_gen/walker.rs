//! DFS tree walker for template code generation.
//!
//! Traverses the [`TemplateAst`] arena in DFS order using an explicit stack,
//! calling [`TemplateCodeGen`] trait methods at appropriate points.

use crate::ast::types::{AstNodeKind, TemplateAst};
use crate::template::oxc::types::{OxcNodeData, OxcParsedAst};
use crate::types::NodeId;

use super::types::CodeGenOutput;
use super::{TemplateCodeGen, WalkAction};

/// DFS phase for element nodes.
enum Phase {
    Enter,
    Leave,
}

/// Walk the template AST in DFS order, calling codegen trait methods.
///
/// Uses an explicit stack (not recursion) for bounded stack depth.
/// Elements get `enter_element` before children and `leave_element` after.
/// Leaf nodes (text, interpolation, comment) only get a single visit call.
///
/// The walker also calls `enter_template` / `leave_template` for the root.
#[cfg_attr(feature = "hotpath", hotpath::measure)]
pub fn walk_template<'alloc>(
    ast: &TemplateAst,
    oxc_ast: &OxcParsedAst<'alloc>,
    source: &'alloc str,
    gen: &mut dyn TemplateCodeGen<'alloc>,
    out: &mut CodeGenOutput<'alloc>,
) {
    // Enter template root
    gen.enter_template(&ast.root, source, out);

    // Get root children
    let root_children = ast
        .root
        .content
        .as_ref()
        .map(|c| c.children.as_slice())
        .unwrap_or(&[]);

    // Explicit DFS stack: (NodeId, Phase)
    let mut stack: Vec<(NodeId, Phase)> = Vec::with_capacity(ast.nodes.len());

    // Push root children in reverse order (so first child is processed first)
    for &child_id in root_children.iter().rev() {
        stack.push((child_id, Phase::Enter));
    }

    while let Some((id, phase)) = stack.pop() {
        let node = &ast.nodes[id.0];
        let oxc_data = &oxc_ast.data[id.0];

        match (&node.kind, &phase) {
            (AstNodeKind::Element(el), Phase::Enter) => {
                let oxc_el = match oxc_data {
                    OxcNodeData::Element(e) => Some(e.as_ref()),
                    _ => None,
                };
                let action = gen.enter_element(id, el, oxc_el, source, out);

                // Always push Leave phase (stack balance for scope_closes etc.)
                stack.push((id, Phase::Leave));

                // Only push children if the backend wants them visited
                if action == WalkAction::Continue {
                    if let Some(content) = &el.content {
                        for &child_id in content.children.iter().rev() {
                            stack.push((child_id, Phase::Enter));
                        }
                    }
                }
            }
            (AstNodeKind::Element(el), Phase::Leave) => {
                let oxc_el = match oxc_data {
                    OxcNodeData::Element(e) => Some(e.as_ref()),
                    _ => None,
                };
                gen.leave_element(id, el, oxc_el, source, out);
            }
            (AstNodeKind::Text(text), Phase::Enter) => {
                gen.visit_text(id, text, source, out);
            }
            (AstNodeKind::Interpolation(interp), Phase::Enter) => {
                let oxc_expr = match oxc_data {
                    OxcNodeData::Interpolation(e) => e,
                    _ => {
                        // Interpolation node should always have OxcNodeData::Interpolation.
                        // If it doesn't (shouldn't happen), skip silently.
                        continue;
                    }
                };
                gen.visit_interpolation(id, interp, oxc_expr, source, out);
            }
            (AstNodeKind::Comment(comment), Phase::Enter) => {
                gen.visit_comment(id, comment, source, out);
            }
            // Leaf nodes don't have a Leave phase
            (_, Phase::Leave) => {}
        }
    }

    // Leave template root
    gen.leave_template(&ast.root, source, out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::builder::TemplateAstBuilder;
    use crate::ast::types::*;
    use crate::parser::types::RootNodeTemplate;
    use crate::template::oxc::types::{OxcNodeData, OxcParsedAst, OxcParsedExpression};
    use crate::test_helpers::{make_root, make_tag};
    use crate::types::NodeId;
    use crate::utils::oxc::Dynamism;

    use super::super::types::CodeGenOutput;
    use super::super::{TemplateCodeGen, WalkAction};

    /// Test codegen that records visit order as strings.
    struct RecordingCodeGen {
        visits: Vec<String>,
    }

    impl RecordingCodeGen {
        fn new() -> Self {
            Self { visits: Vec::new() }
        }
    }

    impl<'alloc> TemplateCodeGen<'alloc> for RecordingCodeGen {
        fn enter_template(
            &mut self,
            _root: &RootNodeTemplate,
            _source: &'alloc str,
            _out: &mut CodeGenOutput<'alloc>,
        ) {
            self.visits.push("enter_template".to_string());
        }

        fn leave_template(
            &mut self,
            _root: &RootNodeTemplate,
            _source: &'alloc str,
            _out: &mut CodeGenOutput<'alloc>,
        ) {
            self.visits.push("leave_template".to_string());
        }

        fn enter_element(
            &mut self,
            id: NodeId,
            _el: &ElementNode,
            _oxc: Option<&crate::template::oxc::types::OxcParsedElement<'alloc>>,
            _source: &'alloc str,
            _out: &mut CodeGenOutput<'alloc>,
        ) -> WalkAction {
            self.visits.push(format!("enter_element({})", id.0));
            WalkAction::Continue
        }

        fn leave_element(
            &mut self,
            id: NodeId,
            _el: &ElementNode,
            _oxc: Option<&crate::template::oxc::types::OxcParsedElement<'alloc>>,
            _source: &'alloc str,
            _out: &mut CodeGenOutput<'alloc>,
        ) {
            self.visits.push(format!("leave_element({})", id.0));
        }

        fn visit_text(
            &mut self,
            id: NodeId,
            _text: &TextNode,
            _source: &'alloc str,
            _out: &mut CodeGenOutput<'alloc>,
        ) {
            self.visits.push(format!("visit_text({})", id.0));
        }

        fn visit_interpolation(
            &mut self,
            id: NodeId,
            _interp: &InterpolationNode,
            _oxc: &OxcParsedExpression<'alloc>,
            _source: &'alloc str,
            _out: &mut CodeGenOutput<'alloc>,
        ) {
            self.visits.push(format!("visit_interpolation({})", id.0));
        }

        fn visit_comment(
            &mut self,
            id: NodeId,
            _comment: &CommentNode,
            _source: &'alloc str,
            _out: &mut CodeGenOutput<'alloc>,
        ) {
            self.visits.push(format!("visit_comment({})", id.0));
        }
    }

    /// Build OxcParsedAst with None entries (no expression parsing needed for walker tests).
    fn make_oxc_none(len: usize) -> OxcParsedAst<'static> {
        OxcParsedAst::new((0..len).map(|_| OxcNodeData::None).collect())
    }

    /// Build OxcParsedAst where one entry is an Interpolation.
    fn make_oxc_with_interpolation_at<'alloc>(len: usize, idx: usize) -> OxcParsedAst<'alloc> {
        let mut data: Vec<OxcNodeData<'alloc>> = (0..len).map(|_| OxcNodeData::None).collect();
        data[idx] = OxcNodeData::Interpolation(OxcParsedExpression {
            offset: 0,
            expression: None,
            errors: None,
            bindings: None,
            dynamism: Dynamism::Static,
        });
        OxcParsedAst::new(data)
    }

    // ── Test 1: Empty template ──────────────────────────────────

    #[test]
    fn empty_template_only_enter_leave() {
        let b = TemplateAstBuilder::new(make_root());
        let ast = b.finish();
        let oxc_ast = make_oxc_none(ast.nodes.len());
        let alloc = oxc_allocator::Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut gen = RecordingCodeGen::new();

        walk_template(&ast, &oxc_ast, "", &mut gen, &mut out);

        assert_eq!(gen.visits, vec!["enter_template", "leave_template"]);
    }

    // ── Test 2: Single text node ────────────────────────────────

    #[test]
    fn single_text_node() {
        let mut b = TemplateAstBuilder::new(make_root());
        b.add_text(0, 5, false, false);
        let ast = b.finish();
        let oxc_ast = make_oxc_none(ast.nodes.len());
        let alloc = oxc_allocator::Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut gen = RecordingCodeGen::new();

        walk_template(&ast, &oxc_ast, "hello", &mut gen, &mut out);

        assert_eq!(
            gen.visits,
            vec!["enter_template", "visit_text(0)", "leave_template"]
        );
    }

    // ── Test 3: Single element with text child ──────────────────

    #[test]
    fn element_with_text_child() {
        // <div>hello</div>
        let input = "<div>hello</div>";
        let mut b = TemplateAstBuilder::new(make_root());
        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        b.add_text(5, 10, false, false);
        b.close_element(Some(make_tag(10, 16, 15)), 10);
        let ast = b.finish();
        let oxc_ast = make_oxc_none(ast.nodes.len());
        let alloc = oxc_allocator::Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut gen = RecordingCodeGen::new();

        walk_template(&ast, &oxc_ast, input, &mut gen, &mut out);

        assert_eq!(
            gen.visits,
            vec![
                "enter_template",
                "enter_element(0)",
                "visit_text(1)",
                "leave_element(0)",
                "leave_template"
            ]
        );
    }

    // ── Test 4: Nested elements ─────────────────────────────────

    #[test]
    fn nested_elements_dfs_order() {
        // <div><span>text</span></div>
        let input = "<div><span>text</span></div>";
        let mut b = TemplateAstBuilder::new(make_root());
        b.open_element(make_tag(0, 5, 4)); // div
        b.mark_element_content_start(5);
        b.open_element(make_tag(5, 11, 10)); // span
        b.mark_element_content_start(11);
        b.add_text(11, 15, false, false); // text
        b.close_element(Some(make_tag(15, 22, 21)), 15); // </span>
        b.close_element(Some(make_tag(22, 28, 27)), 22); // </div>
        let ast = b.finish();
        let oxc_ast = make_oxc_none(ast.nodes.len());
        let alloc = oxc_allocator::Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut gen = RecordingCodeGen::new();

        walk_template(&ast, &oxc_ast, input, &mut gen, &mut out);

        assert_eq!(
            gen.visits,
            vec![
                "enter_template",
                "enter_element(0)", // div
                "enter_element(1)", // span
                "visit_text(2)",    // text
                "leave_element(1)", // span
                "leave_element(0)", // div
                "leave_template"
            ]
        );
    }

    // ── Test 5: Sibling elements ────────────────────────────────

    #[test]
    fn sibling_elements_correct_order() {
        // <a></a><b></b>
        let input = "<a></a><b></b>";
        let mut b = TemplateAstBuilder::new(make_root());
        b.open_element(make_tag(0, 3, 2)); // a
        b.mark_element_content_start(3);
        b.close_element(Some(make_tag(3, 7, 6)), 3); // </a>
        b.open_element(make_tag(7, 10, 9)); // b
        b.mark_element_content_start(10);
        b.close_element(Some(make_tag(10, 14, 13)), 10); // </b>
        let ast = b.finish();
        let oxc_ast = make_oxc_none(ast.nodes.len());
        let alloc = oxc_allocator::Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut gen = RecordingCodeGen::new();

        walk_template(&ast, &oxc_ast, input, &mut gen, &mut out);

        assert_eq!(
            gen.visits,
            vec![
                "enter_template",
                "enter_element(0)", // a
                "leave_element(0)", // a
                "enter_element(1)", // b
                "leave_element(1)", // b
                "leave_template"
            ]
        );
    }

    // ── Test 6: Interpolation node ──────────────────────────────

    #[test]
    fn interpolation_receives_oxc_expression() {
        // <div>{{ foo }}</div>
        let input = "<div>{{ foo }}</div>";
        let mut b = TemplateAstBuilder::new(make_root());
        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        b.add_interpolation(5, 14, 8, 11); // {{ foo }}
        b.close_element(Some(make_tag(14, 20, 19)), 14);
        let ast = b.finish();
        // Node 0 = div (None), Node 1 = interpolation (needs OxcParsedExpression)
        let oxc_ast = make_oxc_with_interpolation_at(ast.nodes.len(), 1);
        let alloc = oxc_allocator::Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut gen = RecordingCodeGen::new();

        walk_template(&ast, &oxc_ast, input, &mut gen, &mut out);

        assert_eq!(
            gen.visits,
            vec![
                "enter_template",
                "enter_element(0)",
                "visit_interpolation(1)",
                "leave_element(0)",
                "leave_template"
            ]
        );
    }

    // ── Test 7: Comment node ────────────────────────────────────

    #[test]
    fn comment_node_visited() {
        // <!-- hello -->
        let input = "<!-- hello -->";
        let mut b = TemplateAstBuilder::new(make_root());
        b.add_comment(0, 14, 5, 10);
        let ast = b.finish();
        let oxc_ast = make_oxc_none(ast.nodes.len());
        let alloc = oxc_allocator::Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut gen = RecordingCodeGen::new();

        walk_template(&ast, &oxc_ast, input, &mut gen, &mut out);

        assert_eq!(
            gen.visits,
            vec!["enter_template", "visit_comment(0)", "leave_template"]
        );
    }

    // ── Test 8: Mixed children ──────────────────────────────────

    #[test]
    fn mixed_children_all_types() {
        // <div>text{{ x }}<!-- c --></div>
        let input = "<div>text{{ x }}<!-- c --></div>";
        let mut b = TemplateAstBuilder::new(make_root());
        b.open_element(make_tag(0, 5, 4));
        b.mark_element_content_start(5);
        b.add_text(5, 9, false, false); // text
        b.add_interpolation(9, 16, 12, 13); // {{ x }}
        b.add_comment(16, 26, 21, 22); // <!-- c -->
        b.close_element(Some(make_tag(26, 32, 31)), 26);
        let ast = b.finish();
        // Node 0=div, 1=text, 2=interpolation, 3=comment
        let oxc_ast = make_oxc_with_interpolation_at(ast.nodes.len(), 2);
        let alloc = oxc_allocator::Allocator::default();
        let mut out = CodeGenOutput::new(&alloc);
        let mut gen = RecordingCodeGen::new();

        walk_template(&ast, &oxc_ast, input, &mut gen, &mut out);

        assert_eq!(
            gen.visits,
            vec![
                "enter_template",
                "enter_element(0)",
                "visit_text(1)",
                "visit_interpolation(2)",
                "visit_comment(3)",
                "leave_element(0)",
                "leave_template"
            ]
        );
    }
}
