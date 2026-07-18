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
            v_if_chains: SmallVec::new(),
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
    let oxc_ast = crate::template::oxc::types::OxcParsedAst::new(Vec::new());
    let mut gen = VdomCodeGen::new(&ast, &oxc_ast, resolver, &options);
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
    let oxc_ast = crate::template::oxc::types::OxcParsedAst::new(Vec::new());
    let mut gen = VdomCodeGen::new(&ast, &oxc_ast, resolver, &options);
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
            v_if_chains: SmallVec::new(),
        }),
    );
    let ast = make_empty_ast(&root);
    let oxc_ast = crate::template::oxc::types::OxcParsedAst::new(Vec::new());
    let mut gen = VdomCodeGen::new(&ast, &oxc_ast, resolver, &options);

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
            v_if_chains: SmallVec::new(),
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
    let oxc_ast = crate::template::oxc::types::OxcParsedAst::new(Vec::new());
    let mut gen = VdomCodeGen::new(&ast, &oxc_ast, resolver, &options);

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
            v_if_chains: SmallVec::new(),
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
    let oxc_ast = crate::template::oxc::types::OxcParsedAst::new(Vec::new());
    let mut gen = VdomCodeGen::new(&ast, &oxc_ast, resolver, &options);

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
            v_if_chains: SmallVec::new(),
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
    let oxc_ast = crate::template::oxc::types::OxcParsedAst::new(Vec::new());
    let mut gen = VdomCodeGen::new(&ast, &oxc_ast, resolver, &options);

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

/// F5 regression guard: a lone `<li v-if v-for>` (no v-else) must still emit
/// the ternary FALSE edge. Dropping it (grok's `let _ = close`) produces an
/// unterminated `cond ? (...)` — a syntax error at runtime.
#[test]
fn v_if_v_for_lone_element_emits_balanced_ternary_false_edge() {
    let code = gen_vdom_template(
        r#"<template><ul><li v-if="ok" v-for="x in xs">{{x}}</li></ul></template>
<script setup>const ok = 1; const xs = [];</script>"#,
    );
    // TRUE branch: the v-if-over-v-for fragment.
    assert!(
        code.contains("? (_openBlock(true), _createElementBlock(_Fragment"),
        "v-if+v-for must open its true branch as a fragment block.\n{code}"
    );
    // FALSE branch: the ternary must be terminated with a comment vnode.
    assert!(
        code.contains(": _createCommentVNode(\"v-if\", true)"),
        "lone v-if+v-for must terminate the ternary with a comment false-edge.\n{code}"
    );
    // NEGATIVE: the fragment close must be followed by ` : ` (the else edge),
    // never immediately by the enclosing array/paren close (unterminated ternary).
    assert!(
        !code.contains("UNKEYED_FRAGMENT */))]") && !code.contains("UNKEYED_FRAGMENT */)))\n}"),
        "v-if+v-for ternary must not be unterminated.\n{code}"
    );
}

/// F19 regression guard: two genuine element roots with a leading comment are a
/// plain STABLE_FRAGMENT (64). DEV_ROOT_FRAGMENT (2112 = 2048|64) must NOT be
/// over-triggered — it applies only when comments surround a SINGLE logical root.
#[test]
fn multi_root_fragment_with_comment_stays_stable_not_dev_root() {
    let code = gen_vdom_template(
        r#"<template><!--c--><div>a</div><span>b</span></template><script setup></script>"#,
    );
    assert!(
        code.contains("64 /* STABLE_FRAGMENT */"),
        "multi-root fragment with a comment must be plain STABLE_FRAGMENT (64).\n{code}"
    );
    // NEGATIVE: must not over-trigger DEV_ROOT_FRAGMENT.
    assert!(
        !code.contains("2112") && !code.contains("2048"),
        "two element roots must NOT flag DEV_ROOT_FRAGMENT.\n{code}"
    );
}

/// F19 (ported from grok spec, verified against official @vue/compiler-dom):
/// a comment beside a SINGLE logical root (here a v-if/v-else chain, which is
/// ONE logical root) flags the Fragment `STABLE_FRAGMENT | DEV_ROOT_FRAGMENT`
/// (2112) so fallthrough / single-root filtering ignore the comment vnode.
#[test]
fn dev_root_fragment_comment_plus_single_conditional_root() {
    let code = gen_vdom_template(
        r#"<template>
  <!-- note -->
  <div v-if="a" />
  <div v-else />
</template>
<script setup>const a = true;</script>"#,
    );
    assert!(
        code.contains("2112 /* STABLE_FRAGMENT, DEV_ROOT_FRAGMENT */"),
        "comment + single conditional root must be DEV_ROOT_FRAGMENT (2112).\n{code}"
    );
}

/// F19 (ported from grok spec): a comment beside a SINGLE component root flags
/// DEV_ROOT_FRAGMENT (2112), and the component is a plain `_createVNode`
/// (fragment child), never a bare `_createBlock` without `_openBlock`.
#[test]
fn dev_root_fragment_comment_plus_single_component_root() {
    let code = gen_vdom_template(
        r#"<template>
  <!-- no scoped styles -->
  <CheckboxRoot name="test" />
</template>
<script setup>import CheckboxRoot from './CheckboxRoot.vue'</script>"#,
    );
    assert!(
        code.contains("2112 /* STABLE_FRAGMENT, DEV_ROOT_FRAGMENT */"),
        "comment + single component root must be DEV_ROOT_FRAGMENT (2112).\n{code}"
    );
    assert!(
        code.contains("_createVNode("),
        "the component beside a root comment must be a _createVNode fragment child.\n{code}"
    );
    // NEGATIVE: must not be a bare block without openBlock.
    assert!(
        !code.contains("_createBlock($setup.CheckboxRoot") || code.contains("_createVNode("),
        "component next to a root comment must not be a bare _createBlock.\n{code}"
    );
}

/// F13: sibling v-if/v-else chains under one parent get a GLOBAL running branch
/// key (0,1 then 2,3), never a per-chain reset (0,1,0,1). Duplicate keys break
/// Vue's keyed patching and log "Duplicate keys".
#[test]
fn sibling_v_if_chains_get_global_running_branch_keys() {
    let code = gen_vdom_template(
        r#"<template><div><p v-if="a">A</p><p v-else>B</p><span v-if="c">C</span><span v-else>D</span></div></template>
<script setup>const a = 1; const c = 2;</script>"#,
    );
    assert!(
        code.contains("{ key: 0 }"),
        "first branch must be key 0.\n{code}"
    );
    assert!(
        code.contains("{ key: 1 }"),
        "second branch must be key 1.\n{code}"
    );
    // The counter must CONTINUE across the sibling chain, not reset.
    assert!(
        code.contains("{ key: 2 }"),
        "third branch must be key 2 (counter must not reset to 0).\n{code}"
    );
    assert!(
        code.contains("{ key: 3 }"),
        "fourth branch must be key 3.\n{code}"
    );
}

/// F13: a lone v-if branch (native element) is keyed `{ key: 0 }` and keeps its
/// comment false-edge.
#[test]
fn single_v_if_branch_element_gets_key_zero() {
    let code = gen_vdom_template(
        r#"<template><div><p v-if="a">A</p></div></template><script setup>const a = 1;</script>"#,
    );
    assert!(
        code.contains("_createElementBlock(\"p\", { key: 0 }"),
        "lone v-if <p> branch must carry {{ key: 0 }}.\n{code}"
    );
    assert!(
        code.contains(": _createCommentVNode(\"v-if\", true)"),
        "false edge must remain.\n{code}"
    );
}

/// F8: `<template v-if>` routes through key injection — its Fragment carries
/// `{ key: 0 }`, and the following `<p v-else>` continues the counter to key 1.
#[test]
fn template_v_if_injects_fragment_branch_key() {
    let code = gen_vdom_template(
        r#"<template><div><template v-if="a"><b>x</b><i>y</i></template><p v-else>z</p></div></template><script setup>const a = 1;</script>"#,
    );
    assert!(
        code.contains("_createElementBlock(_Fragment, { key: 0 }"),
        "<template v-if> Fragment must carry {{ key: 0 }} (not null).\n{code}"
    );
    assert!(
        code.contains("{ key: 1 }"),
        "the <p v-else> must continue the branch counter to key 1.\n{code}"
    );
}

/// F5/F13: a lone `<li v-if v-for>` injects the branch key on the OUTER
/// `_renderList` Fragment (`{ key: 0 }`), matching official Vue.
#[test]
fn v_if_v_for_outer_fragment_gets_branch_key() {
    let code = gen_vdom_template(
        r#"<template><ul><li v-if="ok" v-for="x in xs">{{x}}</li></ul></template><script setup>const ok = 1; const xs = [];</script>"#,
    );
    assert!(
        code.contains("_createElementBlock(_Fragment, { key: 0 }, _renderList"),
        "v-if+v-for outer Fragment must carry branch key 0.\n{code}"
    );
}

/// F13: a v-if branch with an explicit `:key` uses the user key — no synthetic
/// branch key is injected.
#[test]
fn v_if_branch_with_user_key_is_not_double_keyed() {
    let code = gen_vdom_template(
        r#"<template><div><p v-if="a" :key="myKey">A</p></div></template><script setup>const a = 1; const myKey = 'k';</script>"#,
    );
    assert!(
        !code.contains("{ key: 0 }"),
        "explicit :key must suppress the synthetic branch key.\n{code}"
    );
    assert!(
        code.contains("key: $setup.myKey") || code.contains("key: myKey"),
        "the user-authored key must be emitted.\n{code}"
    );
}

/// F13: a v-if branch that is a COMPONENT gets the injected key inside the
/// component props object (official Vue `_createBlock(_Hidden, { key: 0 })`).
#[test]
fn v_if_component_branch_gets_injected_key() {
    let code = gen_vdom_template(
        r#"<template><div>a</div><Hidden v-if="show" /></template>
<script setup>import Hidden from './Hidden.vue'; const show = true;</script>"#,
    );
    assert!(
        code.contains("{ key: 0 }"),
        "v-if component branch must carry {{ key: 0 }}.\n{code}"
    );
    assert!(
        !code.contains("_createBlock($setup.Hidden, null")
            && !code.contains("_createBlock($setup.Hidden)"),
        "component branch props must not be null/absent when a key is injected.\n{code}"
    );
}

/// F13 (ported from grok spec, verified against official @vue/compiler-dom):
/// when v-for coexists with v-else, the OUTER `_renderList` Fragment carries the
/// branch key `{ key: 1 }` while loop items keep their own `:key`. Official Vue
/// output: `_createElementBlock(_Fragment, { key: 1 }, _renderList(...))` with
/// item `key: parsed.name`.
#[test]
fn v_for_on_v_else_fragment_gets_branch_key() {
    let code = gen_vdom_template(
        r#"<template>
  <!-- comment forces multi-root fragment -->
  <Comp v-if="empty" :key="name" />
  <Comp v-for="parsed in items" v-else :key="parsed.name" />
</template>
<script setup>
import Comp from './Comp.vue'
const empty = false
const name = 'n'
const items = [{ name: 'a' }]
</script>"#,
    );
    assert!(
        code.contains("_createElementBlock(_Fragment, { key: 1 }, _renderList"),
        "v-else v-for Fragment must carry branch key 1 (Vue parity).\n{code}"
    );
    // The v-if arm keeps its user-authored :key (no synthetic branch key).
    assert!(
        code.contains("key: $setup.name") || code.contains("key: name"),
        "the v-if arm must keep its explicit :key.\n{code}"
    );
}

/// F6: v-memo on a NESTED native element wraps the block vnode factory in
/// `_withMemo([deps], () => (_openBlock(), _createElementBlock(...)), _cache, N)`.
#[test]
fn v_memo_native_element_emits_with_memo_block() {
    let code = gen_vdom_template(
        r#"<template><section><div v-memo="[x]">{{ x }}</div></section></template>
<script setup>const x = 1;</script>"#,
    );
    assert!(
        code.contains("_withMemo([$setup.x], () => "),
        "native v-memo must wrap in _withMemo with resolved deps.\n{code}"
    );
    // Native element memo factory returns a BLOCK.
    assert!(
        code.contains("_withMemo([$setup.x], () => (_openBlock(), _createElementBlock(\"div\""),
        "native v-memo factory must return a block.\n{code}"
    );
    assert!(
        code.contains(", _cache, "),
        "v-memo must pass the _cache slot.\n{code}"
    );
}

/// F6: v-memo on a CHILDLESS component wraps `_createVNode` (no block) —
/// `_withMemo([deps], () => _createVNode(Comp), _cache, N)`.
#[test]
fn v_memo_childless_component_emits_with_memo() {
    let code = gen_vdom_template(
        r#"<template><section><Comp v-memo="[x]"/></section></template>
<script setup>import Comp from './Comp.vue'; const x = 1;</script>"#,
    );
    assert!(
        code.contains("_withMemo([$setup.x], () => _createVNode($setup.Comp"),
        "childless component v-memo must wrap a plain _createVNode.\n{code}"
    );
    assert!(
        code.contains(", _cache, "),
        "must pass _cache slot.\n{code}"
    );
    // NEGATIVE: a childless component memo must NOT force a block.
    assert!(
        !code.contains("() => (_openBlock(), _createBlock($setup.Comp"),
        "nested childless component v-memo must not be block-forced.\n{code}"
    );
}

/// F6: v-memo on a NAMED-slot component wraps the component vnode (with its slot
/// object) in _withMemo.
#[test]
fn v_memo_named_slot_component_emits_with_memo() {
    let code = gen_vdom_template(
        r#"<template><section><Comp v-memo="[x]"><template #foo>hi</template></Comp></section></template>
<script setup>import Comp from './Comp.vue'; const x = 1;</script>"#,
    );
    assert!(
        code.contains("_withMemo([$setup.x], () => "),
        "named-slot component v-memo must wrap in _withMemo.\n{code}"
    );
    assert!(
        code.contains("foo: _withCtx("),
        "the named slot must still be emitted inside the memo factory.\n{code}"
    );
    assert!(
        code.contains(", _cache, "),
        "must pass _cache slot.\n{code}"
    );
}

/// F6 (ported from grok spec): v-memo on a ROOT component (with default slot)
/// emits `return _withMemo([deps], () => (_openBlock(), _createBlock(...)), _cache, N)`
/// — a single openBlock INSIDE the memo factory, never a double openBlock.
#[test]
fn v_memo_on_root_component_emits_with_memo() {
    let code = gen_vdom_template(
        r#"<template>
  <MyComp v-memo="[a, b]" :class="cls">
    <slot />
  </MyComp>
</template>
<script setup>
import MyComp from './MyComp.vue'
const a = 1
const b = 2
const cls = 'x'
</script>"#,
    );
    assert!(
        code.contains("_withMemo([$setup.a, $setup.b], () => "),
        "root component v-memo must wrap with resolved deps.\n{code}"
    );
    assert!(
        code.contains(", _cache, "),
        "must pass _cache slot.\n{code}"
    );
    // Exactly ONE openBlock inside the memo factory — never a double openBlock.
    assert!(
        !code.contains("(_openBlock(), _withMemo"),
        "root v-memo must not double-wrap openBlock outside the memo factory.\n{code}"
    );
}

/// F7: v-memo INSIDE v-for uses per-item cache topology — the `_renderList`
/// callback receives a 4th `_cached` param, compares the item key, uses
/// `_isMemoSame`, stores `_item.memo`, and passes `_cache, N` to `_renderList`.
/// It must NOT collapse to a single global `_withMemo([deps], ...)` wrap.
#[test]
fn v_memo_in_v_for_emits_per_item_cache() {
    let code = gen_vdom_template(
        r#"<template><div v-for="i in list" :key="i" v-memo="[i]">{{ i }}</div></template>
<script setup>const list = [];</script>"#,
    );
    assert!(
        code.contains("_cached) => {"),
        "renderList callback must receive the _cached param.\n{code}"
    );
    assert!(
        code.contains("const _memo = ([i])"),
        "must compute _memo from the (loop-local) deps.\n{code}"
    );
    assert!(
        code.contains("_isMemoSame(_cached, _memo)"),
        "must short-circuit via _isMemoSame.\n{code}"
    );
    assert!(
        code.contains("_cached.key === i"),
        "must compare the item key.\n{code}"
    );
    assert!(
        code.contains("_item.memo = _memo"),
        "must stamp _item.memo.\n{code}"
    );
    assert!(
        code.contains(", _cache, "),
        "renderList must receive the _cache slot.\n{code}"
    );
    // NEGATIVE: per-item cache, NOT a single global _withMemo wrap.
    assert!(
        !code.contains("_withMemo([i], () =>"),
        "v-for + v-memo must use per-item cache, not a _withMemo wrap.\n{code}"
    );
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

/// Inline multi-statement arrow handlers must not be double-wrapped.
/// `@keydown="(event) => { if (x); }"` → `onKeydown: (event) => {…}`
/// NOT `onKeydown: $event => {(event) => {…}}` (no-op at runtime).
#[test]
fn inline_arrow_event_handler_not_double_wrapped() {
    let code = gen_vdom_template(
        r#"<template>
  <div @keydown="(event) => {
    if (event.key === 'Home') {
      goHome(event);
    }
  }"></div>
</template>
<script setup>
function goHome(e) {}
</script>"#,
    );
    assert!(
        !code.contains("$event => {(event)") && !code.contains("$event => { (event)"),
        "must not double-wrap arrow handler, got:\n{code}"
    );
    assert!(
        code.contains("onKeydown:") && (code.contains("(event)") || code.contains("event =>")),
        "handler must remain an arrow taking event, got:\n{code}"
    );
    assert!(
        code.contains("goHome") || code.contains("$setup.goHome"),
        "handler body must call goHome, got:\n{code}"
    );
}

/// reka-ui VisuallyHiddenInput: `v-for="parsed in …" v-else` on one element.
/// Loop alias must be introduced via `_renderList` callback — without that,
/// props like `parsed.name` are free identifiers → ReferenceError.
#[test]
fn v_for_on_v_else_emits_render_list_with_alias_in_scope() {
    let code = gen_vdom_template(
        r#"<template>
  <div v-if="cond" key="x">one</div>
  <div v-for="parsed in items" v-else :key="parsed.name">{{ parsed.value }}</div>
</template>
<script setup>
const cond = false
const items = [{ name: 'a', value: 1 }]
</script>"#,
    );
    assert!(
        code.contains("_renderList"),
        "v-for on v-else must emit _renderList, got:\n{code}"
    );
    assert!(
        code.contains("(parsed") || code.contains("parsed)"),
        "loop alias `parsed` must appear as _renderList callback param, got:\n{code}"
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
    // class first, then v-bind → _mergeProps({class}, attrs) so attrs can override
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
    // class before attrs in source → regular props object first, then spread
    let merge = code.find("_mergeProps(").expect("mergeProps");
    let class_pos = code[merge..]
        .find("class:")
        .or_else(|| code[merge..].find("\"class\""));
    let attrs_pos = code[merge..].find("attrs");
    if let (Some(c), Some(a)) = (class_pos, attrs_pos) {
        assert!(
            c < a,
            "class then v-bind should emit props object before spread, got:\n{code}"
        );
    }
}

/// v-bind first then explicit :name must let :name win (later mergeProps arg).
/// reka-ui VisuallyHiddenInput: `v-bind="{...props}" :name="parsed.name"`.
#[test]
fn merge_props_vbind_then_explicit_keeps_explicit_last() {
    let code = gen_vdom_template(
        r#"<template>
  <input v-bind="props" :name="computedName" :value="computedValue" />
</template>
<script setup>
const props = { name: 'base' }
const computedName = 'override'
const computedValue = 1
</script>"#,
    );
    assert!(
        code.contains("_mergeProps("),
        "expected _mergeProps, got:\n{code}"
    );
    let merge = code.find("_mergeProps(").expect("mergeProps");
    let rest = &code[merge..];
    // Spread `props` (or $setup.props) must appear before the explicit name/value object
    // so later keys win in Vue's mergeProps.
    let spread_pos = rest.find("props").expect("props spread");
    let name_pos = rest
        .find("name:")
        .or_else(|| rest.find("\"name\""))
        .expect("explicit name");
    assert!(
        spread_pos < name_pos,
        "v-bind then :name must emit spread before explicit object, got:\n{code}"
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

/// `<slot v-bind="slotProps" />` must pass the object as the third arg of
/// `_renderSlot` (AlertDialogRoot / DialogRoot reka-ui pattern).
#[test]
fn slot_outlet_v_bind_spread_passes_props() {
    let code = gen_vdom_template(
        r#"<template>
  <Comp v-slot="slotProps">
    <slot v-bind="slotProps" />
  </Comp>
</template>
<script setup>
const x = 1
</script>"#,
    );
    assert!(
        code.contains("_renderSlot(_ctx.$slots, \"default\",")
            || code.contains("_renderSlot(_ctx.$slots, \"default\", $setup.slotProps")
            || code.contains("slotProps"),
        "slot v-bind spread must appear in renderSlot args, got:\n{code}"
    );
    // Must not be bare `_renderSlot(_ctx.$slots, "default")` with no third arg
    // inside the withCtx that receives slotProps.
    assert!(
        code.contains("_withCtx((slotProps)") && code.contains("slotProps"),
        "scoped slot param must flow into renderSlot, got:\n{code}"
    );
    // The renderSlot call inside withCtx must receive the spread.
    let with_ctx = code
        .find("_withCtx((slotProps)")
        .expect("withCtx with slotProps");
    let tail = &code[with_ctx..];
    assert!(
        tail.contains("_renderSlot(_ctx.$slots, \"default\", slotProps)")
            || tail.contains("_renderSlot(_ctx.$slots, \"default\", $setup.slotProps)"),
        "renderSlot must take slotProps as 3rd arg, got tail:\n{tail}"
    );
}

/// Component with children + v-show must wrap createBlock in _withDirectives.
/// Regression: AvatarImage uses `<Primitive v-show="..."><slot/></Primitive>` —
/// the slots path used to drop v-show entirely (display:none never applied).
#[test]
fn component_with_children_v_show_uses_with_directives() {
    let code = gen_vdom_template(
        "<template><Comp v-show=\"visible\"><span>x</span></Comp></template>\n<script setup>\nimport Comp from './Comp.vue'\nconst visible = true\n</script>",
    );
    assert!(
        code.contains("_withDirectives"),
        "component+children+v-show should use _withDirectives, got:\n{code}"
    );
    assert!(
        code.contains("_vShow"),
        "component+children+v-show should use _vShow helper, got:\n{code}"
    );
    assert!(
        code.contains("createBlock") || code.contains("_createBlock"),
        "should still emit createBlock, got:\n{code}"
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

/// Components inside `v-for` must mark default slots as DYNAMIC (`_: 2`) and
/// set the DYNAMIC_SLOTS patch flag. STABLE (`_: 1`) freezes slot content so
/// `{{ item.value }}` never updates when the iterated item changes (reka-ui
/// TimeField / DateField segment text regression).
#[test]
fn component_inside_vfor_default_slot_is_dynamic() {
    let code = gen_vdom_template(
        r#"<template>
  <MyComp
    v-for="item in items"
    :key="item.part"
    :part="item.part"
  >{{ item.value }}</MyComp>
</template>
<script setup>
import MyComp from './MyComp.vue'
const items = [{ part: 'minute', value: '30' }]
</script>"#,
    );
    assert!(
        code.contains("_: 2"),
        "v-for component default slot must be DYNAMIC (_: 2), got:\n{code}"
    );
    assert!(
        !code.contains("_: 1 /* STABLE */") && !code.contains(", _: 1}"),
        "v-for component default slot must NOT be STABLE, got:\n{code}"
    );
    assert!(
        code.contains("DYNAMIC_SLOTS") || code.contains("1024"),
        "v-for component must emit DYNAMIC_SLOTS patch flag, got:\n{code}"
    );
    // Component patch flag is PROPS|DYNAMIC_SLOTS (1032), never TEXT|PROPS (9).
    // TEXT (1) on `_createTextVNode(..., 1 /* TEXT */)` is correct and unrelated.
    assert!(
        code.contains("1032") || code.contains("PROPS, DYNAMIC_SLOTS"),
        "v-for component patch flag should be PROPS|DYNAMIC_SLOTS (1032), got:\n{code}"
    );
    assert!(
        !code.contains("9 /* TEXT, PROPS */") && !code.contains("TEXT, PROPS, DYNAMIC"),
        "component patch flag must not include TEXT, got:\n{code}"
    );
}

#[test]
fn component_nested_in_vfor_parent_default_slot_is_dynamic() {
    let code = gen_vdom_template(
        r#"<template>
  <div v-for="item in items" :key="item.id">
    <MyComp>{{ item.value }}</MyComp>
  </div>
</template>
<script setup>
import MyComp from './MyComp.vue'
const items = [{ id: 1, value: 'x' }]
</script>"#,
    );
    assert!(
        code.contains("_: 2"),
        "component nested in v-for must use DYNAMIC slots, got:\n{code}"
    );
    assert!(
        code.contains("1024") || code.contains("DYNAMIC_SLOTS"),
        "component nested in v-for must emit DYNAMIC_SLOTS, got:\n{code}"
    );
}

#[test]
fn component_outside_vfor_default_slot_stays_stable() {
    let code = gen_vdom_template(
        r#"<template>
  <MyComp>{{ msg }}</MyComp>
</template>
<script setup>
import MyComp from './MyComp.vue'
const msg = 'hi'
</script>"#,
    );
    assert!(
        code.contains("_: 1"),
        "component outside v-for keeps STABLE slots, got:\n{code}"
    );
    assert!(
        !code.contains("1024") && !code.contains("DYNAMIC_SLOTS"),
        "component outside v-for must not emit DYNAMIC_SLOTS, got:\n{code}"
    );
}

// ==================== Keyboard event key filters (_withKeys) ====================

/// `@keydown.backspace` must use `_withKeys(..., ["backspace"])`, never
/// `_withModifiers`. Unknown runtime modifiers are no-ops so the handler would
/// fire for every key and `preventDefault` typing (reka-ui PinInput).
#[test]
fn keydown_backspace_uses_with_keys_not_with_modifiers() {
    let code = gen_vdom_template(
        r#"<template>
  <input @keydown.backspace="onBack" @keydown.delete="onDel" />
</template>
<script setup>
function onBack() {}
function onDel() {}
</script>"#,
    );
    assert!(
        code.contains(r#"_withKeys("#) && code.contains(r#""backspace""#),
        "keydown.backspace must use _withKeys, got:\n{code}"
    );
    assert!(
        code.contains(r#""delete""#),
        "keydown.delete must use _withKeys, got:\n{code}"
    );
    // The backspace/delete filters must not be runtime modifiers.
    assert!(
        !code.contains(r#"_withModifiers($setup.onBack"#)
            && !code.contains(r#"_withModifiers(onBack"#)
            && !code.contains(r#"_withModifiers($setup.onDel"#),
        "keydown.backspace/delete must NOT use _withModifiers, got:\n{code}"
    );
}

#[test]
fn keydown_chained_arrow_home_end_all_with_keys() {
    // Official Vue: _withKeys(handler, ["left","right","up","down","home","end"])
    let code = gen_vdom_template(
        r#"<template>
  <input @keydown.left.right.up.down.home.end="onNav" />
</template>
<script setup>
function onNav() {}
</script>"#,
    );
    assert!(
        code.contains("_withKeys("),
        "keyboard key filters must use _withKeys, got:\n{code}"
    );
    for key in ["left", "right", "up", "down", "home", "end"] {
        assert!(
            code.contains(&format!("\"{key}\"")),
            "expected key filter {key:?} in _withKeys, got:\n{code}"
        );
    }
    // home/end must not be routed to withModifiers (Vue treats them as keys).
    assert!(
        !code.contains("_withModifiers("),
        "keydown.left.right.up.down.home.end must not use _withModifiers, got:\n{code}"
    );
}

#[test]
fn click_stop_prevent_still_uses_with_modifiers() {
    let code = gen_vdom_template(
        r#"<template>
  <button @click.stop.prevent="onClick">x</button>
</template>
<script setup>
function onClick() {}
</script>"#,
    );
    assert!(
        code.contains("_withModifiers("),
        "click.stop.prevent must use _withModifiers, got:\n{code}"
    );
    assert!(
        code.contains("\"stop\"") && code.contains("\"prevent\""),
        "expected stop/prevent modifiers, got:\n{code}"
    );
}

/// Props objects with dynamic `:key` must not be hoisted to module scope —
/// `const _hoisted = { key: item.name }` evaluates `item` outside v-for
/// (reka-ui CheckboxGroup `item is not defined`).
#[test]
fn dynamic_key_props_object_not_hoisted() {
    let code = gen_vdom_template(
        r#"<template>
  <div v-for="item in items" :key="item.name" class="static-cls">
    {{ item.name }}
  </div>
</template>
<script setup>
const items = [{ name: 'a' }]
</script>"#,
    );
    assert!(
        !code.contains("key: item.name")
            || !code
                .lines()
                .any(|l| l.contains("const _hoisted") && l.contains("item.name")),
        "must not hoist props object containing v-for key expression, got:\n{code}"
    );
    // key expression must appear inside the render function, not only in hoists
    assert!(
        code.contains("key:") || code.contains("key "),
        "key must still be emitted on the element, got:\n{code}"
    );
}

/// Vnode `key` must never appear in the dynamicProps array. Official Vue only
/// puts it on the VNode; listing `"key"` in dynamicProps breaks keyed fragment
/// reuse (reka-ui Calendar cells remount → keyboard focus lost).
#[test]
fn vnode_key_not_in_dynamic_props_array() {
    let code = gen_vdom_template(
        r#"<template>
  <MyComp
    v-for="item in items"
    :key="item.id"
    :date="item.date"
    :data-testid="item.id"
  />
</template>
<script setup>
import MyComp from './MyComp.vue'
const items = [{ id: 'a', date: 1 }]
</script>"#,
    );
    // key must still be on the createBlock props object
    assert!(
        code.contains("key:") || code.contains("key "),
        "vnode key property must still be emitted, got:\n{code}"
    );
    // dynamicProps hoisted array must not include "key"
    assert!(
        !code.contains(r#"["key""#)
            && !code.contains(r#"["key","#)
            && !code.contains(r#", "key""#)
            && !code.contains(r#","key""#),
        "dynamicProps must not include \"key\", got:\n{code}"
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

// ── Merge-prescan output invariants + gate ────────────────────────────────
//
// The merge prescans are gated/allocation-optimized for the no-merge case. The
// emitted render function must be byte-identical regardless: duplicate event
// handlers, v-model + explicit `@update:*`, and class/style static+dynamic must
// still array-merge exactly as before, and the single-handler fast path must
// emit the same code while skipping the grouping maps.

#[test]
fn duplicate_event_handlers_array_merge_byte_identical() {
    let code =
        gen_vdom_template("<template><button @click=\"a\" @click=\"b\">x</button></template>");
    let expected = [
        r#"const _hoisted_1 = ["onClick"]"#,
        r#""#,
        r#"function render(_ctx, _cache, $props, $setup, $data, $options) {"#,
        r#"return (_openBlock(), _createElementBlock("button", { onClick: [_ctx.a, _ctx.b] }, "x", 8 /* PROPS */, _hoisted_1))"#,
        r#"}"#,
    ]
    .join("\n");
    assert_eq!(code, expected);
    // Positive: the two handlers collapse into one array-valued key.
    assert!(
        code.contains("onClick: [_ctx.a, _ctx.b]"),
        "duplicate @click must array-merge, got:\n{code}"
    );
    // Negative: no un-merged first handler, no duplicate `onClick` key.
    assert!(
        !code.contains("onClick: _ctx.a,"),
        "first handler must not be emitted un-merged, got:\n{code}"
    );
    assert_eq!(
        code.matches("onClick:").count(),
        1,
        "merged handlers must produce exactly one onClick key, got:\n{code}"
    );
}

#[test]
fn vmodel_with_explicit_update_handler_byte_identical() {
    let code = gen_vdom_template(
        "<template><MyComp v-model=\"val\" @update:modelValue=\"onUp\"/></template>\n<script setup>\nimport MyComp from './MyComp.vue'\nconst val = 1\nconst onUp = () => {}\n</script>",
    );
    let expected = [
        r#"const _hoisted_1 = ["modelValue", "onUpdate:modelValue"]"#,
        r#""#,
        r#"function render(_ctx, _cache, $props, $setup, $data, $options) {"#,
        r#"return (_openBlock(), _createBlock($setup.MyComp, { modelValue: $setup.val, "onUpdate:modelValue": [$event => (($setup.val) = $event), $setup.onUp] }, null, 8 /* PROPS */, _hoisted_1))"#,
        r#"}"#,
    ]
    .join("\n");
    assert_eq!(code, expected);
    // Positive: the v-model writer and the explicit handler merge into one array.
    assert!(
        code.contains(r#""onUpdate:modelValue": [$event => (($setup.val) = $event), $setup.onUp]"#),
        "v-model + explicit @update:* must array-merge, got:\n{code}"
    );
    // Negative: the explicit handler must not also appear as its own key.
    assert_eq!(
        code.matches("onUpdate:modelValue").count(),
        2,
        "onUpdate:modelValue should appear once as a key and once in the hoisted array, got:\n{code}"
    );
}

#[test]
fn class_style_static_dynamic_merge_byte_identical() {
    let code = gen_vdom_template(
        "<template><div class=\"a\" :class=\"b\" style=\"color: red\" :style=\"s\">x</div></template>",
    );
    let expected = [
        r#"function render(_ctx, _cache, $props, $setup, $data, $options) {"#,
        r#"return (_openBlock(), _createElementBlock("div", { class: _normalizeClass(["a", _ctx.b]), style: _normalizeStyle([{ color: "red" }, _ctx.s]) }, "x", 6 /* CLASS, STYLE */))"#,
        r#"}"#,
    ]
    .join("\n");
    assert_eq!(code, expected);
    // Positive: static + dynamic are folded into the normalize helpers.
    assert!(
        code.contains(r#"class: _normalizeClass(["a", _ctx.b])"#),
        "static + dynamic class must merge via _normalizeClass, got:\n{code}"
    );
    assert!(
        code.contains(r#"style: _normalizeStyle([{ color: "red" }, _ctx.s])"#),
        "static + dynamic style must merge via _normalizeStyle, got:\n{code}"
    );
    // Negative: the static class/style must not also be emitted as bare props.
    assert!(
        !code.contains(r#"class: "a""#),
        "static class must be merged, not emitted bare, got:\n{code}"
    );
}

#[test]
fn single_event_handler_no_merge_byte_identical() {
    let code = gen_vdom_template("<template><button @click=\"a\">x</button></template>");
    let expected = [
        r#"const _hoisted_1 = ["onClick"]"#,
        r#""#,
        r#"function render(_ctx, _cache, $props, $setup, $data, $options) {"#,
        r#"return (_openBlock(), _createElementBlock("button", { onClick: _ctx.a }, "x", 8 /* PROPS */, _hoisted_1))"#,
        r#"}"#,
    ]
    .join("\n");
    assert_eq!(code, expected);
    // Positive: a single handler stays a scalar value.
    assert!(
        code.contains("onClick: _ctx.a "),
        "single @click must stay a scalar handler, got:\n{code}"
    );
    // Negative: no array-merge wrapping for a lone handler.
    assert!(
        !code.contains("onClick: ["),
        "single handler must not be array-wrapped, got:\n{code}"
    );
}

#[test]
fn event_merge_prescan_gated_when_no_duplicate_possible() {
    // A single event handler cannot collide, so the grouping maps must never be
    // built. (Discriminating: against the always-build prescan this count is 1.)
    element::reset_event_merge_full_scan_count();
    let single = gen_vdom_template("<template><button @click=\"a\">x</button></template>");
    assert_eq!(
        element::event_merge_full_scan_count(),
        0,
        "single handler must skip the grouping maps, got code:\n{single}"
    );

    // No event handlers at all: still skipped.
    element::reset_event_merge_full_scan_count();
    let none = gen_vdom_template("<template><div id=\"x\">y</div></template>");
    assert_eq!(
        element::event_merge_full_scan_count(),
        0,
        "element without handlers must skip the grouping maps, got code:\n{none}"
    );

    // Two handlers for the same event CAN collide, so the grouping path must run
    // and still produce the merged array.
    element::reset_event_merge_full_scan_count();
    let dup =
        gen_vdom_template("<template><button @click=\"a\" @click=\"b\">x</button></template>");
    assert!(
        element::event_merge_full_scan_count() > 0,
        "duplicate handlers must build the grouping maps, got code:\n{dup}"
    );
    assert!(
        dup.contains("onClick: [_ctx.a, _ctx.b]"),
        "duplicate @click must still array-merge after gating, got:\n{dup}"
    );
}

// ── condition prefix segmented mapping ──────────────────────────────

use crate::template::code_gen::binding::BindingType;

/// Build a binding resolver for the condition tests.
fn cond_resolver(
    entries: &[(&'static str, BindingType)],
    inline: bool,
) -> BindingResolver<'static> {
    let mut map = FxHashMap::default();
    for &(n, bt) in entries {
        map.insert(n as &str, bt);
    }
    BindingResolver::new(map, inline)
}

/// `(dst_col, src_col, has_source)` for every source-map token.
fn token_dump(ct: &crate::code_transform::CodeTransform<'_>) -> Vec<(u32, u32, bool)> {
    let map = ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("t.vue"));
    map.get_tokens()
        .map(|t| {
            (
                t.get_dst_col(),
                t.get_src_col(),
                t.get_source_id().is_some(),
            )
        })
        .collect()
}

/// Assert no token in the generated column range `[lo, hi)` carries a source id.
fn assert_unmapped_region(tokens: &[(u32, u32, bool)], lo: u32, hi: u32) {
    assert!(
        !tokens
            .iter()
            .any(|&(dst, _, has_src)| dst >= lo && dst < hi && has_src),
        "no source token may map the synthetic region [{lo}, {hi}); tokens: {tokens:?}"
    );
}

/// Discriminating — an inline `SetupRef` v-if (`count`) emits `(count.value) ? `;
/// `count` maps to source while the synthetic `.value` suffix AND the `) ? `
/// wrapper stay unmapped. A flat single-token map would cover `.value` with the
/// `count` token (mapping bleed); the `.value`-region negative assertion fails on that.
#[test]
fn condition_prefix_inline_setup_ref_keeps_value_unmapped() {
    let alloc = Allocator::default();
    // `count` lives at byte 4 in the source.
    let source = "abc count";
    let resolver = cond_resolver(&[("count", BindingType::SetupRef)], true);
    let cond = resolve_simple_expr_segments(&resolver, "count", 4).wrapped("(", ") ? ");
    assert_eq!(cond.text, "(count.value) ? ");

    let mut out = CodeGenOutput::new(&alloc);
    children::emit_condition_prefix_mapped(&mut out, 0, &cond);
    let mut ct = crate::code_transform::CodeTransform::new(source, &alloc);
    out.apply_to(&mut ct);

    // Bytes unchanged — segmentation only refines the source map.
    assert_eq!(ct.build_string(), "(count.value) ? abc count");

    let tokens = token_dump(&ct);
    // `count` body: token at gen col 1 (after `(`) → src col 4.
    let body = tokens.iter().find(|&&(dst, _, has)| dst == 1 && has);
    assert!(body.is_some(), "`count` must map; tokens: {tokens:?}");
    assert_eq!(body.unwrap().1, 4, "`count` must map to src col 4");

    // `.value` occupies gen cols [6, 12) — entirely unmapped, with its own
    // unmapped token starting at col 6.
    assert!(
        tokens.iter().any(|&(dst, _, has)| dst == 6 && !has),
        "synthetic `.value` must start an unmapped segment at col 6; tokens: {tokens:?}"
    );
    assert_unmapped_region(&tokens, 6, 12);
    // `) ? ` wrapper [12, 16) stays unmapped.
    assert_unmapped_region(&tokens, 12, 16);
}

/// Build an OXC condition expression with two prop bindings.
fn two_prop_cond_oxc(
    inner_start: u32,
    a: (&'static str, u32),
    b: (&'static str, u32),
) -> crate::template::oxc::types::OxcParsedExpression<'static> {
    use crate::utils::oxc::{Binding, BindingExtractionResult, Dynamism};
    let mk = |name: &'static str, pos: u32| Binding {
        name,
        span: crate::common::RelativeSpan::new(
            pos - inner_start,
            pos - inner_start + name.len() as u32,
        ),
        pos,
        ignore: false,
        is_shorthand: false,
    };
    crate::template::oxc::types::OxcParsedExpression {
        offset: inner_start,
        expression: None,
        errors: None,
        bindings: Some(BindingExtractionResult {
            bindings: vec![mk(a.0, a.1), mk(b.0, b.1)],
            functions: vec![],
            literals: vec![],
            has_errors: false,
            dynamism: Dynamism::Dynamic,
        }),
        dynamism: Dynamism::Dynamic,
    }
}

/// Discriminating — a compound v-if `foo && bar` (both props) emits
/// `(__props.foo && __props.bar) ? `; `foo` AND `bar` each get their own source
/// token while every synthetic `__props.` run stays unmapped. Mapping the whole
/// body to one token at the leading `__props.` would leave `bar` with no token.
#[test]
fn condition_prefix_compound_props_map_each_identifier() {
    let alloc = Allocator::default();
    // Source `x foo && bar`: foo at byte 2, bar at byte 9.
    let source = "x foo && bar";
    let resolver = cond_resolver(
        &[("foo", BindingType::Props), ("bar", BindingType::Props)],
        true,
    );
    let oxc = two_prop_cond_oxc(2, ("foo", 2), ("bar", 9));
    let cond =
        build_prefixed_expr_segments("foo && bar", 2, &oxc, &resolver, &[]).wrapped("(", ") ? ");
    assert_eq!(cond.text, "(__props.foo && __props.bar) ? ");

    let mut out = CodeGenOutput::new(&alloc);
    children::emit_condition_prefix_mapped(&mut out, 0, &cond);
    let mut ct = crate::code_transform::CodeTransform::new(source, &alloc);
    out.apply_to(&mut ct);
    assert_eq!(
        ct.build_string(),
        "(__props.foo && __props.bar) ? x foo && bar"
    );

    let tokens = token_dump(&ct);
    // `foo` token at gen col 9 (`(` + `__props.`) → src col 2.
    let foo = tokens.iter().find(|&&(dst, _, has)| dst == 9 && has);
    assert!(foo.is_some(), "`foo` must map; tokens: {tokens:?}");
    assert_eq!(foo.unwrap().1, 2, "`foo` must map to src col 2");
    // `bar` token at gen col 24 (after the second `__props.`) → src col 9.
    let bar = tokens.iter().find(|&&(dst, _, has)| dst == 24 && has);
    assert!(
        bar.is_some(),
        "`bar` must have its OWN source token; tokens: {tokens:?}"
    );
    assert_eq!(bar.unwrap().1, 9, "`bar` must map to src col 9");

    // Both `__props.` runs are unmapped: `(__props.` body region [1, 9) and the
    // second `__props.` region [16, 24).
    assert_unmapped_region(&tokens, 1, 9);
    assert_unmapped_region(&tokens, 16, 24);
    // `) ? ` wrapper stays unmapped.
    assert_unmapped_region(&tokens, 27, 31);
}

#[test]
fn vbind_spread_alone_emits_full_props_patch_flag() {
    // Official Vue emits `16 /* FULL_PROPS */` for bare v-bind spreads so attrs
    // (class/style) re-diff every update. Missing the flag freezes initial class.
    let code = gen_vdom_template(
        "<template><div v-bind=\"attrs\">hi</div></template>\n<script setup>\nconst attrs = {}\n</script>",
    );
    assert!(
        code.contains("16") && (code.contains("FULL_PROPS") || code.contains(", 16")),
        "v-bind spread alone must emit FULL_PROPS patch flag, got:\n{code}"
    );
}

#[test]
fn vbind_spread_on_component_with_slot_emits_full_props_patch_flag() {
    // Label.vue pattern: component + v-bind expr + default slot
    let code = gen_vdom_template(
        r#"<template>
  <MyComp v-bind="normalizeAttrs(label.attrs([$attrs, { as }]))">
    <slot />
  </MyComp>
</template>
<script setup>
import MyComp from './MyComp.vue'
const as = 'label'
const label = { attrs: (x) => x }
const normalizeAttrs = (x) => x
</script>"#,
    );
    assert!(
        code.contains("_normalizeProps(_guardReactiveProps(") || code.contains("normalizeProps"),
        "should use normalizeProps for single spread, got:\n{code}"
    );
    assert!(
        code.contains("FULL_PROPS") || code.contains(", 16"),
        "component v-bind + slot must emit FULL_PROPS so $attrs.class updates, got:\n{code}"
    );
}

#[test]
fn vbind_spread_on_component_self_closing_emits_full_props() {
    let code = gen_vdom_template(
        r#"<template><MyComp v-bind="attrs" /></template>
<script setup>
import MyComp from './MyComp.vue'
const attrs = {}
</script>"#,
    );
    assert!(
        code.contains("FULL_PROPS") || code.contains(", 16"),
        "self-closing component v-bind must emit FULL_PROPS, got:\n{code}"
    );
}

#[test]
fn vbind_spread_on_component_with_static_child_emits_full_props() {
    let code = gen_vdom_template(
        r#"<template><MyComp v-bind="attrs">hi</MyComp></template>
<script setup>
import MyComp from './MyComp.vue'
const attrs = {}
</script>"#,
    );
    assert!(
        code.contains("FULL_PROPS") || code.contains(", 16"),
        "component v-bind + static child must emit FULL_PROPS, got:\n{code}"
    );
}

/// Both structural directives on ONE element with `v-if` (no chain): the
/// condition stays OUTER, the true branch is the `_renderList` fragment,
/// and the false branch is the v-if comment.
#[test]
fn v_for_with_v_if_on_same_element_keeps_condition_outer() {
    let code = gen_vdom_template(
        r#"<template>
  <div v-if="cond" v-for="p in items" :key="p.id">{{ p.x }}</div>
</template>
<script setup>
const cond = true
const items = [{ id: 1, x: 'a' }]
</script>"#,
    );
    assert!(
        code.contains("_renderList"),
        "v-for beside v-if must emit _renderList, got:\n{code}"
    );
    let cond_pos = code.find("$setup.cond").expect("condition present");
    let list_pos = code.find("_renderList").expect("renderList present");
    assert!(
        cond_pos < list_pos,
        "condition must be OUTER (official v-if priority), got:\n{code}"
    );
    assert!(
        code.contains("_createCommentVNode(\"v-if\", true)"),
        "chain-terminal v-if needs the comment false branch, got:\n{code}"
    );
    assert!(
        code.contains("KEYED_FRAGMENT"),
        ":key on the loop element makes the fragment keyed, got:\n{code}"
    );
}

// ==================== hasScopeRef slot flags (official parity) ====================

/// Official oracle: a component whose OWN `v-slot="{ x }"` params are the
/// only scope variables in its slot content compiles to `_: 1 /* STABLE */`
/// with NO DYNAMIC_SLOTS — the child's own effect re-invokes the slot
/// function with fresh args. (`@vue/compiler-core` build-mode
/// `hasScopeRef`: own slot params are out of scope at buildSlots.)
#[test]
fn own_scoped_slot_params_alone_stay_stable() {
    let code = gen_vdom_template(
        r#"<template>
  <Picker v-slot="{ grid }">
    <span>{{ grid.rows }}</span>
  </Picker>
</template>
<script setup>
import Picker from './Picker.vue'
</script>"#,
    );
    assert!(
        code.contains("_: 1 /* STABLE */"),
        "own scoped slot params alone must stay STABLE like official, got:\n{code}"
    );
    assert!(
        !code.contains("_: 2"),
        "own scoped slot params must NOT force DYNAMIC, got:\n{code}"
    );
    assert!(
        !code.contains("DYNAMIC_SLOTS") && !code.contains("1024"),
        "no DYNAMIC_SLOTS patch flag for a stable scoped slot, got:\n{code}"
    );
    // The wiring itself: params destructure through _withCtx.
    assert!(
        code.contains("_withCtx(({ grid })"),
        "slot params must thread through _withCtx, got:\n{code}"
    );
}

/// Official oracle: a named `<template #body=\"{ row }\">` scoped slot with
/// no outer-scope references is STABLE.
#[test]
fn named_template_scoped_slot_params_alone_stay_stable() {
    let code = gen_vdom_template(
        r#"<template>
  <Table>
    <template #body="{ row }"><b>{{ row.id }}</b></template>
  </Table>
</template>
<script setup>
import Table from './Table.vue'
</script>"#,
    );
    assert!(
        code.contains("_: 1 /* STABLE */"),
        "named scoped slot with own params only must stay STABLE, got:\n{code}"
    );
    assert!(
        !code.contains("DYNAMIC_SLOTS") && !code.contains("1024"),
        "no DYNAMIC_SLOTS for own-params-only named slot, got:\n{code}"
    );
}

/// Official oracle (the under-mark direction): a component whose slot
/// content references an OUTER component's slot parameter must be DYNAMIC —
/// official forces this via `hasScopeRef`; STABLE would let
/// `shouldUpdateComponent` skip and serve stale content.
#[test]
fn inner_component_referencing_outer_slot_param_is_dynamic() {
    let code = gen_vdom_template(
        r#"<template>
  <Outer v-slot="{ a }">
    <Inner><em>{{ a }}</em></Inner>
  </Outer>
</template>
<script setup>
import Outer from './Outer.vue'
import Inner from './Inner.vue'
</script>"#,
    );
    // Inner (referencing `a` from Outer's scope) must be DYNAMIC + 1024.
    assert!(
        code.contains("_: 2 /* DYNAMIC */"),
        "inner component referencing outer slot param must be DYNAMIC, got:\n{code}"
    );
    assert!(
        code.contains("1024") || code.contains("DYNAMIC_SLOTS"),
        "inner component must set DYNAMIC_SLOTS, got:\n{code}"
    );
    // Outer itself only uses its OWN params → STABLE.
    assert!(
        code.contains("_: 1 /* STABLE */"),
        "outer component with own params only stays STABLE, got:\n{code}"
    );
}

/// Official oracle (build-mode refinement): a component inside `v-for`
/// whose slot content is SCOPE-INDEPENDENT stays STABLE — official's
/// `hasScopeRef` replaces the coarse in-v-for check.
#[test]
fn scope_independent_slot_inside_vfor_stays_stable() {
    let code = gen_vdom_template(
        r#"<template>
  <div v-for="item in items" :key="item.id">
    <Card><span>static</span></Card>
  </div>
</template>
<script setup>
import Card from './Card.vue'
const items = [{ id: 1 }]
</script>"#,
    );
    assert!(
        code.contains("_: 1 /* STABLE */"),
        "scope-independent slot content inside v-for stays STABLE (official refined check), got:\n{code}"
    );
    assert!(
        !code.contains("_: 2"),
        "no DYNAMIC flag without a scope reference, got:\n{code}"
    );
}

/// A descendant v-for ITERABLE referencing an outer slot param also counts
/// as a scope reference (expression position does not matter).
#[test]
fn descendant_vfor_iterable_referencing_outer_slot_param_is_dynamic() {
    let code = gen_vdom_template(
        r#"<template>
  <Outer v-slot="{ list }">
    <Inner><i v-for="x in list" :key="x">{{ x }}</i></Inner>
  </Outer>
</template>
<script setup>
import Outer from './Outer.vue'
import Inner from './Inner.vue'
</script>"#,
    );
    assert!(
        code.contains("_: 2 /* DYNAMIC */"),
        "iterable referencing outer slot param must make Inner DYNAMIC, got:\n{code}"
    );
}
