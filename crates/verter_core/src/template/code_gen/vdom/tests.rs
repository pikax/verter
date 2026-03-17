use super::*;
use crate::ast::types::{
    AstNode, ChildrenFlag, ChildrenMode, ElementContent, PropFlag, TagType, TemplateAst,
};
use crate::parser::types::RootNodeTemplateContent;
use crate::types::NodeTag;
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
) -> crate::ast::types::ElementNode {
    crate::ast::types::ElementNode {
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
        is_fully_static: false,
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
                kind: AstNodeKind::Element(Box::new(make_simple_element(10, 15, 14, 15, 21, 20))),
                parent: None,
                index_in_parent: 0,
            },
            AstNode {
                kind: AstNodeKind::Element(Box::new(make_simple_element(21, 27, 26, 27, 34, 33))),
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
                kind: AstNodeKind::Element(Box::new(make_simple_element(10, 15, 14, 15, 21, 20))),
                parent: None,
                index_in_parent: 0,
            },
            AstNode {
                kind: AstNodeKind::Element(Box::new(make_simple_element(21, 27, 26, 27, 34, 33))),
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

// ==================== Block-tree optimization (full pipeline) ====================

/// Helper: compile a Vue SFC source and return the template code (VDOM mode).
fn gen_vdom_template(source: &str) -> String {
    use crate::compile::{compile, CodegenOptions, VerterCompileOptions};
    let alloc = oxc_allocator::Allocator::new();
    let options = CodegenOptions {
        filename: Some("App.vue".to_string()),
        ..Default::default()
    };
    let verter_opts = VerterCompileOptions {
        force_js: true,
        ..Default::default()
    };
    let result = compile(source, &options, &verter_opts, &alloc);
    assert!(
        result.errors.is_empty(),
        "compile errors: {:?}",
        result.errors
    );
    let tpl = result
        .template
        .as_ref()
        .expect("should have template block");
    tpl.code.clone()
}

#[test]
fn block_tree_single_root_element_uses_create_element_block() {
    let code = gen_vdom_template("<template><div>hello</div></template>");
    assert!(
        code.contains("_createElementBlock(\"div\""),
        "Single root element should use _createElementBlock, got:\n{code}"
    );
    assert!(
        !code.contains("_createElementVNode(\"div\""),
        "Single root element should NOT use _createElementVNode, got:\n{code}"
    );
    assert!(
        code.contains("_openBlock()"),
        "Single root should have _openBlock(), got:\n{code}"
    );
}

#[test]
fn block_tree_single_root_component_uses_create_block() {
    let code = gen_vdom_template(
        "<template><MyComp/></template>\n<script setup>\nimport MyComp from './MyComp.vue'\n</script>",
    );
    assert!(
        code.contains("_createBlock("),
        "Single root component should use _createBlock, got:\n{code}"
    );
    assert!(
        !code.contains("_createVNode("),
        "Single root component should NOT use _createVNode, got:\n{code}"
    );
}

#[test]
fn block_tree_vif_element_uses_block() {
    let code =
        gen_vdom_template("<template><div v-if=\"show\">A</div><span v-else>B</span></template>");
    // Each v-if branch should have its own (_openBlock(), _createElementBlock(...))
    assert!(
        code.contains("(_openBlock(), _createElementBlock(\"div\""),
        "v-if element branch should use (_openBlock(), _createElementBlock(...)), got:\n{code}"
    );
    assert!(
        code.contains("(_openBlock(), _createElementBlock(\"span\""),
        "v-else element branch should use (_openBlock(), _createElementBlock(...)), got:\n{code}"
    );
    // Should NOT use regular _createElementVNode for v-if branches
    assert!(
        !code.contains("_createElementVNode(\"div\""),
        "v-if branch should NOT use _createElementVNode, got:\n{code}"
    );
}

#[test]
fn block_tree_vif_component_uses_block() {
    let code = gen_vdom_template(
        "<template><MyComp v-if=\"show\"/><OtherComp v-else/></template>\n<script setup>\nimport MyComp from './MyComp.vue'\nimport OtherComp from './Other.vue'\n</script>",
    );
    assert!(
        code.contains("(_openBlock(), _createBlock("),
        "v-if component branch should use (_openBlock(), _createBlock(...)), got:\n{code}"
    );
    assert!(
        !code.contains("_createVNode("),
        "v-if component should NOT use _createVNode, got:\n{code}"
    );
}

#[test]
fn block_tree_vfor_component_uses_block() {
    let code = gen_vdom_template(
        "<template><div><MyComp v-for=\"item in items\" :key=\"item.id\"/></div></template>\n<script setup>\nimport MyComp from './MyComp.vue'\nconst items = []\n</script>",
    );
    assert!(
        code.contains("(_openBlock(), _createBlock("),
        "v-for component should use (_openBlock(), _createBlock(...)), got:\n{code}"
    );
    assert!(
        !code.contains("_createVNode("),
        "v-for component should NOT use _createVNode, got:\n{code}"
    );
}

#[test]
fn block_tree_multi_root_children_use_regular_helpers() {
    // Use interpolations to ensure children are dynamic (not hoisted)
    let code = gen_vdom_template(
        "<template><div>{{ a }}</div><p>{{ b }}</p></template>\n<script setup>\nconst a = 1, b = 2\n</script>",
    );
    // Multi-root: individual children should use _createElementVNode, not block variant
    assert!(
        code.contains("_createElementVNode(\"div\""),
        "Multi-root children should use _createElementVNode for div, got:\n{code}"
    );
    assert!(
        code.contains("_createElementVNode(\"p\""),
        "Multi-root children should use _createElementVNode for p, got:\n{code}"
    );
    // The Fragment wrapper itself should use _createElementBlock
    assert!(
        code.contains("_createElementBlock(_Fragment"),
        "Multi-root should wrap in _createElementBlock(_Fragment, ...), got:\n{code}"
    );
    // Children should NOT use block variants
    assert!(
        !code.contains("_createElementBlock(\"div\""),
        "Multi-root children should NOT use _createElementBlock, got:\n{code}"
    );
}

#[test]
fn block_tree_inner_elements_use_regular_helpers() {
    // Use a dynamic inner element (with :class binding) to prevent static hoisting
    let code = gen_vdom_template(
        "<template><div><span :class=\"cls\">inner</span></div></template>\n<script setup>\nconst cls = 'x'\n</script>",
    );
    // Root div should use block variant
    assert!(
        code.contains("_createElementBlock(\"div\""),
        "Root element should use _createElementBlock, got:\n{code}"
    );
    // Inner span should use regular variant
    assert!(
        code.contains("_createElementVNode(\"span\""),
        "Inner element should use _createElementVNode, got:\n{code}"
    );
    // Inner span should NOT use block variant
    assert!(
        !code.contains("_createElementBlock(\"span\""),
        "Inner element should NOT use _createElementBlock, got:\n{code}"
    );
}

// ==================== normalizeProps / guardReactiveProps ====================

#[test]
fn normalize_props_vbind_spread_alone() {
    // v-bind="attrs" alone → _normalizeProps(_guardReactiveProps(attrs))
    let code = gen_vdom_template(
        "<template><div v-bind=\"attrs\">hi</div></template>\n<script setup>\nconst attrs = {}\n</script>",
    );
    assert!(
        code.contains("_normalizeProps(_guardReactiveProps("),
        "v-bind spread alone should use _normalizeProps(_guardReactiveProps(...)), got:\n{code}"
    );
    assert!(
        !code.contains("_mergeProps("),
        "v-bind spread alone should NOT use _mergeProps, got:\n{code}"
    );
}

#[test]
fn normalize_props_vbind_spread_on_component() {
    // Component with v-bind="props" alone → _normalizeProps(_guardReactiveProps(props))
    let code = gen_vdom_template(
        "<template><MyComp v-bind=\"compProps\" /></template>\n<script setup>\nimport MyComp from './MyComp.vue'\nconst compProps = {}\n</script>",
    );
    assert!(
        code.contains("_normalizeProps(_guardReactiveProps("),
        "Component v-bind spread should use _normalizeProps(_guardReactiveProps(...)), got:\n{code}"
    );
}

#[test]
fn normalize_props_vbind_spread_with_regular_props_uses_merge_only() {
    // v-bind="attrs" + class="foo" → _mergeProps({...}, attrs) — NO normalizeProps
    let code = gen_vdom_template(
        "<template><div class=\"foo\" v-bind=\"attrs\">hi</div></template>\n<script setup>\nconst attrs = {}\n</script>",
    );
    assert!(
        code.contains("_mergeProps("),
        "v-bind spread + regular props should use _mergeProps, got:\n{code}"
    );
    assert!(
        !code.contains("_normalizeProps("),
        "v-bind spread + regular props should NOT use _normalizeProps, got:\n{code}"
    );
    assert!(
        !code.contains("_guardReactiveProps("),
        "v-bind spread + regular props should NOT use _guardReactiveProps, got:\n{code}"
    );
}

#[test]
fn normalize_props_dynamic_attr_name() {
    // :[attrName]="value" → _normalizeProps({ [attrName || ""]: value })
    let code = gen_vdom_template(
        "<template><div :[attrName]=\"value\">content</div></template>\n<script setup>\nconst attrName = 'id'\nconst value = '1'\n</script>",
    );
    assert!(
        code.contains("_normalizeProps("),
        "Dynamic attr name should use _normalizeProps, got:\n{code}"
    );
    assert!(
        !code.contains("_guardReactiveProps("),
        "Dynamic attr name should NOT use _guardReactiveProps, got:\n{code}"
    );
    // The dynamic key should use computed property syntax with || ""
    assert!(
        code.contains("|| \"\""),
        "Dynamic attr key should have || \"\" fallback, got:\n{code}"
    );
}

// ==================== toHandlers (v-on spread) ====================

#[test]
fn to_handlers_von_spread_alone_on_element() {
    // v-on="handlers" → _toHandlers(handlers, true) on elements
    let code = gen_vdom_template(
        "<template><div v-on=\"handlers\">hi</div></template>\n<script setup>\nconst handlers = {}\n</script>",
    );
    assert!(
        code.contains("_toHandlers("),
        "v-on spread should use _toHandlers, got:\n{code}"
    );
    assert!(
        code.contains(", true)"),
        "v-on spread on element should have true arg, got:\n{code}"
    );
}

#[test]
fn to_handlers_von_spread_on_component() {
    // v-on="handlers" on component → _toHandlers(handlers) without true
    let code = gen_vdom_template(
        "<template><MyComp v-on=\"handlers\" /></template>\n<script setup>\nimport MyComp from './MyComp.vue'\nconst handlers = {}\n</script>",
    );
    assert!(
        code.contains("_toHandlers("),
        "Component v-on spread should use _toHandlers, got:\n{code}"
    );
    assert!(
        !code.contains("_toHandlers($setup.handlers, true)"),
        "Component v-on spread should NOT have true arg, got:\n{code}"
    );
}

#[test]
fn to_handlers_von_spread_with_regular_event() {
    // @click + v-on="handlers" → _mergeProps({onClick:...}, _toHandlers(handlers, true))
    let code = gen_vdom_template(
        "<template><div @click=\"onClick\" v-on=\"handlers\">hi</div></template>\n<script setup>\nconst onClick = () => {}\nconst handlers = {}\n</script>",
    );
    assert!(
        code.contains("_mergeProps("),
        "v-on spread + regular event should use _mergeProps, got:\n{code}"
    );
    assert!(
        code.contains("_toHandlers("),
        "v-on spread in mergeProps should use _toHandlers, got:\n{code}"
    );
}

#[test]
fn to_handlers_vbind_and_von_spreads() {
    // v-bind="attrs" v-on="handlers" → _mergeProps(attrs, _toHandlers(handlers, true))
    let code = gen_vdom_template(
        "<template><div v-bind=\"attrs\" v-on=\"handlers\">hi</div></template>\n<script setup>\nconst attrs = {}\nconst handlers = {}\n</script>",
    );
    assert!(
        code.contains("_mergeProps("),
        "v-bind + v-on spreads should use _mergeProps, got:\n{code}"
    );
    assert!(
        code.contains("_toHandlers("),
        "v-on spread should be wrapped with _toHandlers, got:\n{code}"
    );
    assert!(
        !code.contains("_toHandlers($setup.attrs"),
        "v-bind spread should NOT use _toHandlers, got:\n{code}"
    );
}

// ==================== Literal prop optimization ====================

#[test]
fn literal_bind_value_not_in_dynamic_props() {
    // :value="200" :max="99" are pure literals — should NOT generate PROPS flag
    let code = gen_vdom_template(
        "<template><MyComp :value=\"200\" :max=\"99\" class=\"item\"><template #default>content</template></MyComp></template>\n<script setup>\nimport MyComp from './MyComp.vue'\n</script>",
    );
    assert!(
        !code.contains("8 /* PROPS */"),
        "Literal bind values should NOT add PROPS flag, got:\n{code}"
    );
    assert!(
        !code.contains("[\"value\""),
        "Literal bind values should NOT appear in dynamic props, got:\n{code}"
    );
}

#[test]
fn dynamic_bind_value_in_dynamic_props() {
    // :value="count" uses a reactive variable — SHOULD generate PROPS flag
    let code = gen_vdom_template(
        "<template><MyComp :value=\"count\" class=\"item\"><template #default>content</template></MyComp></template>\n<script setup>\nimport MyComp from './MyComp.vue'\nimport { ref } from 'vue'\nconst count = ref(0)\n</script>",
    );
    assert!(
        code.contains("8 /* PROPS */") || code.contains("PROPS"),
        "Dynamic bind values should add PROPS flag, got:\n{code}"
    );
}

// ==================== Static hoisting (_hoisted_N) ====================

#[test]
fn hoisted_dynamic_props_array() {
    // :id="x" should produce _hoisted_1 = ["id"] before render function
    let code = gen_vdom_template(
        "<template><div><span :id=\"x\">hello</span></div></template>\n<script setup>\nimport { ref } from 'vue'\nconst x = ref(1)\n</script>",
    );
    assert!(
        code.contains("const _hoisted_1 = [\"id\"]"),
        "Dynamic props array should be hoisted as _hoisted_1, got:\n{code}"
    );
    assert!(
        code.contains("_hoisted_1)"),
        "Element should reference _hoisted_1 instead of inline array, got:\n{code}"
    );
    assert!(
        !code.contains(", [\"id\"])"),
        "Dynamic props array should NOT be inlined, got:\n{code}"
    );
}

#[test]
fn hoisted_multiple_dynamic_props_arrays() {
    // Multiple elements with different dynamic props get separate hoisted constants
    let code = gen_vdom_template(
        "<template><div><span :id=\"x\">a</span><span :title=\"y\">b</span></div></template>\n<script setup>\nimport { ref } from 'vue'\nconst x = ref(1)\nconst y = ref(2)\n</script>",
    );
    assert!(
        code.contains("const _hoisted_1 = [\"id\"]"),
        "First dynamic props array should be hoisted as _hoisted_1, got:\n{code}"
    );
    assert!(
        code.contains("const _hoisted_2 = [\"title\"]"),
        "Second dynamic props array should be hoisted as _hoisted_2, got:\n{code}"
    );
}

#[test]
fn hoisted_dynamic_props_array_deduplication() {
    // Two elements with the same dynamic props array should share the hoisted constant
    let code = gen_vdom_template(
        "<template><div><span :id=\"x\">a</span><span :id=\"y\">b</span></div></template>\n<script setup>\nimport { ref } from 'vue'\nconst x = ref(1)\nconst y = ref(2)\n</script>",
    );
    assert!(
        code.contains("const _hoisted_1 = [\"id\"]"),
        "Dynamic props array should be hoisted, got:\n{code}"
    );
    // Should not have _hoisted_2 since ["id"] is the same
    assert!(
        !code.contains("const _hoisted_2"),
        "Duplicate dynamic props arrays should be deduplicated, got:\n{code}"
    );
}

// ==================== Cache wrapping (_cache[N]) ====================

#[test]
fn cache_wraps_static_element() {
    // Static <p> child of a dynamic parent should use _cache[N] wrapping
    let code = gen_vdom_template(
        "<template><div><p id=\"static\">hello</p><span :class=\"cls\">world</span></div></template>\n<script setup>\nimport { ref } from 'vue'\nconst cls = ref('foo')\n</script>",
    );
    assert!(
        code.contains("_cache[0] || (_cache[0] = _createElementVNode(\"p\""),
        "Static element should be wrapped with _cache[0], got:\n{code}"
    );
    assert!(
        code.contains("-1 /* CACHED */"),
        "Cached element should have -1 CACHED patch flag, got:\n{code}"
    );
    assert!(
        !code.contains("_createStaticVNode"),
        "Should NOT use createStaticVNode, got:\n{code}"
    );
}

#[test]
fn cache_wraps_multiple_static_elements() {
    // Multiple static children each get their own _cache[N]
    let code = gen_vdom_template(
        "<template><div><p>a</p><p>b</p><span :class=\"cls\">c</span></div></template>\n<script setup>\nimport { ref } from 'vue'\nconst cls = ref('foo')\n</script>",
    );
    assert!(
        code.contains("_cache[0]"),
        "First static child should use _cache[0], got:\n{code}"
    );
    assert!(
        code.contains("_cache[1]"),
        "Second static child should use _cache[1], got:\n{code}"
    );
    assert!(
        !code.contains("_createStaticVNode"),
        "Should NOT use createStaticVNode, got:\n{code}"
    );
}

// ==================== withDirectives (v-show + custom directives) ====================

#[test]
fn vshow_with_directives() {
    let code = gen_vdom_template(
        "<template><div v-show=\"visible\">content</div></template>\n<script setup>\nimport { ref } from 'vue'\nconst visible = ref(true)\n</script>",
    );
    assert!(
        code.contains("_withDirectives("),
        "v-show should use _withDirectives, got:\n{code}"
    );
    assert!(
        code.contains("_vShow"),
        "v-show should use _vShow helper, got:\n{code}"
    );
    assert!(
        !code.contains("v-show"),
        "v-show attribute must not appear in output, got:\n{code}"
    );
}

#[test]
fn vshow_with_other_props() {
    let code = gen_vdom_template(
        "<template><div class=\"foo\" v-show=\"visible\">content</div></template>\n<script setup>\nimport { ref } from 'vue'\nconst visible = ref(true)\n</script>",
    );
    assert!(
        code.contains("_withDirectives("),
        "v-show with class should use _withDirectives, got:\n{code}"
    );
    assert!(
        code.contains("class: \"foo\""),
        "Static class should still be present, got:\n{code}"
    );
    assert!(
        code.contains("_vShow"),
        "v-show helper should be present, got:\n{code}"
    );
}

#[test]
fn custom_directive_resolve() {
    let code = gen_vdom_template("<template><div v-focus>content</div></template>");
    assert!(
        code.contains("_withDirectives("),
        "Custom directive should use _withDirectives, got:\n{code}"
    );
    assert!(
        code.contains("_resolveDirective(\"focus\")"),
        "Custom directive should use _resolveDirective, got:\n{code}"
    );
    assert!(
        !code.contains("v-focus"),
        "v-focus attribute must not appear in output, got:\n{code}"
    );
}

#[test]
fn custom_directive_with_arg_and_modifiers() {
    let code = gen_vdom_template(
        "<template><div v-my-dir:arg.mod=\"val\">content</div></template>\n<script setup>\nconst val = 123\n</script>",
    );
    assert!(
        code.contains("_withDirectives("),
        "Custom directive with arg should use _withDirectives, got:\n{code}"
    );
    assert!(
        code.contains("_resolveDirective(\"my-dir\")"),
        "Custom directive should resolve with original name, got:\n{code}"
    );
    assert!(
        code.contains("\"arg\""),
        "Directive arg should be present as string, got:\n{code}"
    );
    assert!(
        code.contains("mod: true"),
        "Directive modifier should be present, got:\n{code}"
    );
}

#[test]
fn vshow_plus_vmodel_combined() {
    // Both v-show and v-model on same native element → single _withDirectives with both entries
    let code = gen_vdom_template(
        "<template><input v-model=\"msg\" v-show=\"visible\" /></template>\n<script setup>\nimport { ref } from 'vue'\nconst msg = ref('')\nconst visible = ref(true)\n</script>",
    );
    assert!(
        code.contains("_withDirectives("),
        "Combined v-model+v-show should use _withDirectives, got:\n{code}"
    );
    assert!(
        code.contains("_vModelText"),
        "v-model should produce _vModelText, got:\n{code}"
    );
    assert!(
        code.contains("_vShow"),
        "v-show should produce _vShow, got:\n{code}"
    );
}

#[test]
fn custom_directive_with_value_no_arg() {
    let code = gen_vdom_template(
        "<template><div v-loading=\"isLoading\">content</div></template>\n<script setup>\nimport { ref } from 'vue'\nconst isLoading = ref(true)\n</script>",
    );
    assert!(
        code.contains("_withDirectives("),
        "v-loading should use _withDirectives, got:\n{code}"
    );
    assert!(
        code.contains("_resolveDirective(\"loading\")"),
        "v-loading should resolve directive, got:\n{code}"
    );
}

// ==================== Dynamic slots (createSlots, _renderList) ====================

#[test]
fn dynamic_slot_with_vfor() {
    // <template v-for="s in slots" #[s.name]> → _createSlots + _renderList
    let code = gen_vdom_template(
        "<template><MyComp><template v-for=\"s in slots\" #[s.name]><div>{{ s.content }}</div></template></MyComp></template>\n<script setup>\nimport MyComp from './MyComp.vue'\nconst slots = [{name: 'a', content: 'x'}]\n</script>",
    );
    assert!(
        code.contains("_createSlots("),
        "v-for on slot should use _createSlots, got:\n{code}"
    );
    assert!(
        code.contains("_renderList("),
        "v-for on slot should use _renderList, got:\n{code}"
    );
    assert!(
        code.contains("_: 2"),
        "Dynamic slots should have _: 2, got:\n{code}"
    );
}

#[test]
fn dynamic_slot_name() {
    // <template #[dynamicName]> → dynamic slot entry
    let code = gen_vdom_template(
        "<template><MyComp><template #[dynamicName]><div>content</div></template></MyComp></template>\n<script setup>\nimport MyComp from './MyComp.vue'\nconst dynamicName = 'header'\n</script>",
    );
    assert!(
        code.contains("_createSlots("),
        "Dynamic slot name should use _createSlots, got:\n{code}"
    );
    assert!(
        code.contains("_: 2"),
        "Dynamic slot should have _: 2, got:\n{code}"
    );
}

// ==================== DYNAMIC_SLOTS patch flag (1024) ====================

#[test]
fn dynamic_slots_flag_vif() {
    // Slot with v-if → 1024 DYNAMIC_SLOTS
    let code = gen_vdom_template(
        "<template><MyComp><template #header v-if=\"show\"><div>header</div></template><template #default><span>body</span></template></MyComp></template>\n<script setup>\nimport MyComp from './MyComp.vue'\nimport { ref } from 'vue'\nconst show = ref(true)\n</script>",
    );
    assert!(
        code.contains("1024"),
        "Slot with v-if should emit DYNAMIC_SLOTS (1024), got:\n{code}"
    );
}

#[test]
fn stable_slots_no_dynamic_flag() {
    // Static slots → no 1024 flag
    let code = gen_vdom_template(
        "<template><MyComp><template #header><div>header</div></template><template #default><span>body</span></template></MyComp></template>\n<script setup>\nimport MyComp from './MyComp.vue'\n</script>",
    );
    assert!(
        !code.contains("1024"),
        "Static slots should NOT emit DYNAMIC_SLOTS (1024), got:\n{code}"
    );
}

// ==================== resolveComponent hoisting ====================

#[test]
fn resolve_component_hoisted() {
    // Component used via _resolveComponent should be hoisted to const at top
    let code = gen_vdom_template("<template><el-button>click</el-button></template>");
    assert!(
        code.contains("const _component_el_button = _resolveComponent(\"el-button\")"),
        "resolveComponent should be hoisted as const, got:\n{code}"
    );
    // The call site should use the variable, not inline
    assert!(
        code.contains("_createBlock(_component_el_button"),
        "Component call should use hoisted variable, got:\n{code}"
    );
}

#[test]
fn resolve_component_dedup() {
    // Same component used twice → only one const
    let code = gen_vdom_template(
        "<template><div><el-button>a</el-button><el-button>b</el-button></div></template>",
    );
    let count = code.matches("const _component_el_button").count();
    assert_eq!(
        count, 1,
        "Same component should have only one hoisted const, got {count} in:\n{code}"
    );
}

#[test]
fn resolve_component_naming() {
    // el-button → _component_el_button
    let code = gen_vdom_template("<template><my-header>x</my-header></template>");
    assert!(
        code.contains("_component_my_header"),
        "Kebab-case component should use underscore-separated variable name, got:\n{code}"
    );
}

// ==================== Static attribute object hoisting ====================

#[test]
fn hoisted_static_attrs() {
    // <span class="foo"> with dynamic text → should hoist { class: "foo" } to _hoisted_N
    // The span has dynamic content so it won't be fully cached, but its props are static
    let code =
        gen_vdom_template(r#"<template><div><span class="foo">{{ msg }}</span></div></template>"#);
    assert!(
        code.contains(r#"const _hoisted_1 = { class: "foo" }"#),
        "Static class prop should be hoisted, got:\n{code}"
    );
    assert!(
        code.contains("_hoisted_1"),
        "Should reference _hoisted_1 at call site, got:\n{code}"
    );
    // Should NOT have inline { class: "foo" } inside createElementVNode
    assert!(
        !code.contains(r#"_createElementVNode("span", { class: "foo" }"#),
        "Should NOT inline static props object, got:\n{code}"
    );
}

#[test]
fn hoisted_static_attrs_dedup() {
    // Same static attrs used twice with dynamic children → single hoisted constant
    let code = gen_vdom_template(
        r#"<template><div><span class="x">{{ a }}</span><span class="x">{{ b }}</span></div></template>"#,
    );
    assert!(
        code.contains("const _hoisted_1"),
        "Should hoist static attrs, got:\n{code}"
    );
    // Both spans should reference the same hoisted constant
    let count = code.matches("_hoisted_1").count();
    // 1 for the const declaration + 2 for the two usages = 3
    assert!(
        count >= 3,
        "Same static attrs should be deduplicated (expected 3 occurrences of _hoisted_1), got {count} in:\n{code}"
    );
    // Should NOT have _hoisted_2 (since they're deduplicated)
    assert!(
        !code.contains("_hoisted_2"),
        "Identical static attrs should share hoisted constant, got:\n{code}"
    );
}

// ==================== PROPS flag on components with literal binds ====================

#[test]
fn childless_component_literal_bind_no_props_flag() {
    // A component with only literal bind (:color="'info'") should NOT get PROPS flag
    // because the binding is suppressed as non-dynamic after literal detection.
    let code = gen_vdom_template(
        r#"<template><CButton :color="'info'" /></template>
<script setup>
import CButton from './CButton.vue'
</script>"#,
    );
    assert!(
        !code.contains("PROPS"),
        "Literal bind component should NOT have PROPS flag, got:\n{code}"
    );
    assert!(
        !code.contains("8 /*"),
        "Literal bind component should NOT have patch flag 8, got:\n{code}"
    );
    assert!(
        code.contains(r#"color: "info""#) || code.contains(r#"color: 'info'"#),
        "Should have the color prop value, got:\n{code}"
    );
}

#[test]
fn childless_component_reactive_bind_has_props_flag() {
    // A component with a reactive bind should still get PROPS flag
    let code = gen_vdom_template(
        r#"<template><CButton :color="color" /></template>
<script setup>
import { ref } from 'vue'
import CButton from './CButton.vue'
const color = ref('info')
</script>"#,
    );
    assert!(
        code.contains("PROPS"),
        "Reactive bind component should have PROPS flag, got:\n{code}"
    );
}

#[test]
fn plain_element_props_unchanged_after_literal_fix() {
    // Regression guard: plain element with dynamic binding should still get PROPS
    let code = gen_vdom_template(
        r#"<template><div :id="x">text</div></template>
<script setup>
import { ref } from 'vue'
const x = ref('foo')
</script>"#,
    );
    assert!(
        code.contains("PROPS"),
        "Plain element with dynamic bind should have PROPS flag, got:\n{code}"
    );
}

// ==================== Redundant child caching ====================

#[test]
fn parent_cached_children_not_individually_cached() {
    // When a parent element is fully static and cached, children should NOT get
    // individual _cache[N] wrappers — the parent's cache encompasses them.
    // Vue only caches the outermost static ancestor.
    let code = gen_vdom_template(
        r#"<template><div><div class="clearfix"><h1>404</h1><h4>Oops!</h4></div><span :class="cls">dynamic</span></div></template>
<script setup>
import { ref } from 'vue'
const cls = ref('foo')
</script>"#,
    );
    // The outer static div.clearfix should be cached
    assert!(
        code.contains("_cache[0]"),
        "Parent static div should be cached, got:\n{code}"
    );
    // But h1 and h4 inside it should NOT be individually cached
    assert!(
        !code.contains("_cache[1]"),
        "Children of cached parent should NOT have individual cache slots, got:\n{code}"
    );
    assert!(
        !code.contains("_cache[2]"),
        "Children of cached parent should NOT have individual cache slots, got:\n{code}"
    );
}

#[test]
fn non_static_parent_children_still_cached() {
    // When parent is NOT fully static, individual static children should still be cached
    let code = gen_vdom_template(
        r#"<template><div><p>static</p><span :class="cls">dynamic</span></div></template>
<script setup>
import { ref } from 'vue'
const cls = ref('foo')
</script>"#,
    );
    // The <p> is static inside a dynamic parent → should be cached
    assert!(
        code.contains("_cache[0]"),
        "Static child in dynamic parent should be cached, got:\n{code}"
    );
}

// ==================== Slot static content caching ====================

#[test]
fn slot_single_static_text_cached() {
    // Static text inside a slot should be cached with _cache[N]
    // and wrapped in _createTextVNode()
    let code = gen_vdom_template(
        r#"<template><CList>Cras justo odio</CList></template>
<script setup>
import CList from './CList.vue'
</script>"#,
    );
    assert!(
        code.contains("_cache["),
        "Static text in slot should be cached, got:\n{code}"
    );
    assert!(
        code.contains("_createTextVNode("),
        "Static text in slot should be wrapped in _createTextVNode, got:\n{code}"
    );
}

#[test]
fn slot_multiple_static_children_spread() {
    // Multiple consecutive static children in a slot should use spread cache pattern
    let code = gen_vdom_template(
        r#"<template><CCard><strong>Title</strong><p>body</p></CCard></template>
<script setup>
import CCard from './CCard.vue'
</script>"#,
    );
    assert!(
        code.contains("...(_cache["),
        "Multiple static slot children should use spread cache, got:\n{code}"
    );
}

#[test]
fn slot_mixed_static_dynamic_split() {
    // Static runs around a dynamic child should be cached separately
    let code = gen_vdom_template(
        r#"<template><CCard><p>a</p><span>{{ msg }}</span><p>b</p></CCard></template>
<script setup>
import { ref } from 'vue'
import CCard from './CCard.vue'
const msg = ref('hi')
</script>"#,
    );
    // Dynamic interpolation should NOT be cached
    assert!(
        code.contains("_toDisplayString"),
        "Dynamic content should use _toDisplayString, got:\n{code}"
    );
    // Static children around it should be cached
    assert!(
        code.contains("_cache["),
        "Static children in mixed slot should be cached, got:\n{code}"
    );
}

#[test]
fn slot_dynamic_child_not_cached() {
    // A component slot with only dynamic content should NOT use cache
    let code = gen_vdom_template(
        r#"<template><CCard><div :class="cls">x</div></CCard></template>
<script setup>
import { ref } from 'vue'
import CCard from './CCard.vue'
const cls = ref('foo')
</script>"#,
    );
    assert!(
        !code.contains("_cache["),
        "Dynamic-only slot should NOT use cache, got:\n{code}"
    );
}

// ==================== Dot-notation component names ====================

#[test]
fn dot_notation_component_setup_binding() {
    // <Swiper.Item> where Swiper is a setup binding → $setup["Swiper"].Item
    // Vue treats dot-notation as property access on the namespace binding.
    let code = gen_vdom_template(
        r#"<template><Swiper.Item>hello</Swiper.Item></template>
<script setup>
import Swiper from './Swiper'
</script>"#,
    );
    // The prefix format depends on the resolver — may use $setup["Swiper"] or $setup.Swiper
    assert!(
        code.contains("$setup[\"Swiper\"].Item") || code.contains("$setup.Swiper.Item"),
        "Dot-notation component should resolve namespace from setup binding, got:\n{code}"
    );
    // Must NOT generate _resolveComponent or invalid variable names
    assert!(
        !code.contains("_resolveComponent"),
        "Should not use _resolveComponent for dot-notation with setup binding, got:\n{code}"
    );
    assert!(
        !code.contains("_component_Swiper.Item"),
        "Must not generate invalid variable name with dot, got:\n{code}"
    );
}

#[test]
fn dot_notation_component_fallback() {
    // <Swiper.Item> where Swiper is NOT in setup bindings → _resolveComponent fallback.
    // Vue uses toValidAssetId which replaces dots with char codes.
    // The variable name must be a valid JS identifier (no dots).
    let code = gen_vdom_template(r#"<template><Swiper.Item>hello</Swiper.Item></template>"#);
    // Must not contain dots in variable name
    assert!(
        !code.contains("_component_Swiper.Item"),
        "Fallback must not generate variable name with dot, got:\n{code}"
    );
    // Should use _resolveComponent with the full tag name
    assert!(
        code.contains(r#"_resolveComponent("Swiper.Item")"#),
        "Fallback should use _resolveComponent with full tag name, got:\n{code}"
    );
}

// ==================== Duplicate static style keys ====================

#[test]
fn static_style_duplicate_keys_last_wins() {
    // CSS cascade: last value wins. The JS object should deduplicate.
    // Matches Vue official compiler behavior (parseStringStyle uses plain object).
    let code = gen_vdom_template(
        r#"<template><div style="position: absolute; position: relative; width: 100%">x</div></template>"#,
    );
    // "position" should appear only once with last value
    let position_count = code.matches("position").count();
    assert_eq!(
        position_count, 1,
        "Duplicate style key 'position' should be deduplicated (last wins), got:\n{code}"
    );
    assert!(
        code.contains(r#"position: "relative""#),
        "Last value 'relative' should win over 'absolute', got:\n{code}"
    );
    assert!(
        !code.contains(r#"position: "absolute""#),
        "First value 'absolute' should be overwritten, got:\n{code}"
    );
}
