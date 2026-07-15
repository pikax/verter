#![allow(dead_code)]
#![allow(clippy::large_enum_variant)]
//! Benchmark: inline `ElementNode` vs `Box<ElementNode>` in the AST arena.
//!
//! Measures both AST construction and DFS traversal for a realistic template
//! (~50 elements, mix of leaves and elements) under two layouts:
//!   A) `AstNodeKind::Element(ElementNode)` — current, large enum variant
//!   B) `AstNodeKind::Element(Box<ElementNode>)` — boxed, smaller enum
//!
//! The benchmark uses local mirror types so neither layout requires modifying
//! the real codebase.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::hint::black_box;

// ============================================================================
// Shared leaf types (identical for both layouts)
// ============================================================================

#[derive(Debug, Clone)]
struct TextNode {
    start: u32,
    end: u32,
    _is_entity: bool,
}

#[derive(Debug, Clone)]
struct CommentNode {
    start: u32,
    end: u32,
}

#[derive(Debug, Clone)]
struct InterpolationNode {
    start: u32,
    end: u32,
}

#[derive(Debug, Clone)]
struct NodeTag {
    start: u32,
    end: u32,
    _name_end: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NodeId(usize);

#[derive(Debug, Clone)]
struct ElementContent {
    _start: u32,
    _end: u32,
    children: Vec<NodeId>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct NodeProp {
    start: u32,
    name_end: u32,
    is_directive: bool,
    arg_start: Option<u32>,
    arg_end: Option<u32>,
    value_start: Option<u32>,
    value_end: Option<u32>,
    modifiers: Vec<Span>,
    is_dynamic: Option<bool>,
}

#[derive(Debug, Clone)]
struct Span {
    _start: u32,
    _end: u32,
}

#[derive(Debug, Clone)]
struct ElementNodeCondition {
    _kind: u8,
    _prop: NodeProp,
}

#[repr(u8)]
#[derive(Debug, Copy, Clone)]
enum TagType {
    Element = 0,
}

#[derive(Debug, Copy, Clone)]
struct PropFlag(u16);

#[derive(Debug, Copy, Clone)]
struct ChildrenFlag(u16);

#[derive(Debug, Copy, Clone)]
enum ChildrenMode {
    Empty,
}

// ============================================================================
// ElementNode — the large payload (mirrors real ElementNode exactly)
// ============================================================================

#[derive(Debug, Clone)]
struct ElementNode {
    tag_open: NodeTag,
    tag_close: Option<NodeTag>,
    tag_type: TagType,
    _is_self_closing: bool,
    props: Vec<NodeProp>,
    content: Option<ElementContent>,
    _v_condition: Option<ElementNodeCondition>,
    _v_for: Option<NodeProp>,
    _v_slot: Option<NodeProp>,
    _v_once: Option<NodeProp>,
    _prop_flag: PropFlag,
    _children_flag: ChildrenFlag,
    _children_mode: ChildrenMode,
}

impl ElementNode {
    fn new(start: u32, end: u32) -> Self {
        Self {
            tag_open: NodeTag {
                start,
                end,
                _name_end: end - 1,
            },
            tag_close: Some(NodeTag {
                start: end,
                end: end + 6,
                _name_end: end + 5,
            }),
            tag_type: TagType::Element,
            _is_self_closing: false,
            props: Vec::new(),
            content: None,
            _v_condition: None,
            _v_for: None,
            _v_slot: None,
            _v_once: None,
            _prop_flag: PropFlag(0),
            _children_flag: ChildrenFlag(0),
            _children_mode: ChildrenMode::Empty,
        }
    }

    fn with_props(mut self, count: usize) -> Self {
        self.props = (0..count)
            .map(|i| NodeProp {
                start: i as u32 * 10,
                name_end: i as u32 * 10 + 5,
                is_directive: i % 3 == 0,
                arg_start: if i % 3 == 0 {
                    Some(i as u32 * 10 + 6)
                } else {
                    None
                },
                arg_end: if i % 3 == 0 {
                    Some(i as u32 * 10 + 8)
                } else {
                    None
                },
                value_start: Some(i as u32 * 10 + 6),
                value_end: Some(i as u32 * 10 + 9),
                modifiers: Vec::new(),
                is_dynamic: if i % 3 == 0 { Some(false) } else { None },
            })
            .collect();
        self
    }

    fn with_children(mut self, children: Vec<NodeId>) -> Self {
        self.content = Some(ElementContent {
            _start: self.tag_open.end,
            _end: self.tag_close.as_ref().map_or(0, |t| t.start),
            children,
        });
        self
    }
}

// ============================================================================
// Layout A: Inline (current)
// ============================================================================

mod inline {
    use super::*;

    #[derive(Debug, Clone)]
    pub enum AstNodeKind {
        Element(ElementNode),
        Text(TextNode),
        Comment(CommentNode),
        Interpolation(InterpolationNode),
    }

    #[derive(Debug, Clone)]
    pub struct AstNode {
        pub kind: AstNodeKind,
        _parent: Option<NodeId>,
        _index_in_parent: usize,
    }

    pub struct Arena {
        pub nodes: Vec<AstNode>,
        pub root_children: Vec<NodeId>,
    }

    impl Arena {
        pub fn new() -> Self {
            Self {
                nodes: Vec::new(),
                root_children: Vec::new(),
            }
        }

        pub fn alloc_element(&mut self, el: ElementNode) -> NodeId {
            let id = NodeId(self.nodes.len());
            self.nodes.push(AstNode {
                kind: AstNodeKind::Element(el),
                _parent: None,
                _index_in_parent: 0,
            });
            id
        }

        pub fn alloc_text(&mut self, start: u32, end: u32) -> NodeId {
            let id = NodeId(self.nodes.len());
            self.nodes.push(AstNode {
                kind: AstNodeKind::Text(TextNode {
                    start,
                    end,
                    _is_entity: false,
                }),
                _parent: None,
                _index_in_parent: 0,
            });
            id
        }

        pub fn alloc_comment(&mut self, start: u32, end: u32) -> NodeId {
            let id = NodeId(self.nodes.len());
            self.nodes.push(AstNode {
                kind: AstNodeKind::Comment(CommentNode { start, end }),
                _parent: None,
                _index_in_parent: 0,
            });
            id
        }

        pub fn alloc_interpolation(&mut self, start: u32, end: u32) -> NodeId {
            let id = NodeId(self.nodes.len());
            self.nodes.push(AstNode {
                kind: AstNodeKind::Interpolation(InterpolationNode { start, end }),
                _parent: None,
                _index_in_parent: 0,
            });
            id
        }

        /// DFS traversal — visits all nodes reachable from `start`.
        pub fn dfs(&self, start: NodeId) -> usize {
            let mut count = 0usize;
            let mut stack = vec![start];
            while let Some(id) = stack.pop() {
                count += 1;
                let node = &self.nodes[id.0];
                if let AstNodeKind::Element(el) = &node.kind {
                    if let Some(content) = &el.content {
                        for &child in content.children.iter().rev() {
                            stack.push(child);
                        }
                    }
                }
            }
            count
        }
    }
}

// ============================================================================
// Layout B: Boxed ElementNode
// ============================================================================

mod boxed {
    use super::*;

    #[derive(Debug, Clone)]
    pub enum AstNodeKind {
        Element(Box<ElementNode>),
        Text(TextNode),
        Comment(CommentNode),
        Interpolation(InterpolationNode),
    }

    #[derive(Debug, Clone)]
    pub struct AstNode {
        pub kind: AstNodeKind,
        _parent: Option<NodeId>,
        _index_in_parent: usize,
    }

    pub struct Arena {
        pub nodes: Vec<AstNode>,
        pub root_children: Vec<NodeId>,
    }

    impl Arena {
        pub fn new() -> Self {
            Self {
                nodes: Vec::new(),
                root_children: Vec::new(),
            }
        }

        pub fn alloc_element(&mut self, el: ElementNode) -> NodeId {
            let id = NodeId(self.nodes.len());
            self.nodes.push(AstNode {
                kind: AstNodeKind::Element(Box::new(el)),
                _parent: None,
                _index_in_parent: 0,
            });
            id
        }

        pub fn alloc_text(&mut self, start: u32, end: u32) -> NodeId {
            let id = NodeId(self.nodes.len());
            self.nodes.push(AstNode {
                kind: AstNodeKind::Text(TextNode {
                    start,
                    end,
                    _is_entity: false,
                }),
                _parent: None,
                _index_in_parent: 0,
            });
            id
        }

        pub fn alloc_comment(&mut self, start: u32, end: u32) -> NodeId {
            let id = NodeId(self.nodes.len());
            self.nodes.push(AstNode {
                kind: AstNodeKind::Comment(CommentNode { start, end }),
                _parent: None,
                _index_in_parent: 0,
            });
            id
        }

        pub fn alloc_interpolation(&mut self, start: u32, end: u32) -> NodeId {
            let id = NodeId(self.nodes.len());
            self.nodes.push(AstNode {
                kind: AstNodeKind::Interpolation(InterpolationNode { start, end }),
                _parent: None,
                _index_in_parent: 0,
            });
            id
        }

        /// DFS traversal — visits all nodes reachable from `start`.
        pub fn dfs(&self, start: NodeId) -> usize {
            let mut count = 0usize;
            let mut stack = vec![start];
            while let Some(id) = stack.pop() {
                count += 1;
                let node = &self.nodes[id.0];
                if let AstNodeKind::Element(el) = &node.kind {
                    if let Some(content) = &el.content {
                        for &child in content.children.iter().rev() {
                            stack.push(child);
                        }
                    }
                }
            }
            count
        }
    }
}

// ============================================================================
// Shared tree-building helpers (trait-based to share logic)
// ============================================================================

trait ArenaApi {
    fn alloc_element(&mut self, el: ElementNode) -> NodeId;
    fn alloc_text(&mut self, start: u32, end: u32) -> NodeId;
    fn alloc_comment(&mut self, start: u32, end: u32) -> NodeId;
    fn alloc_interpolation(&mut self, start: u32, end: u32) -> NodeId;
    fn set_root_children(&mut self, children: Vec<NodeId>);
}

impl ArenaApi for inline::Arena {
    fn alloc_element(&mut self, el: ElementNode) -> NodeId {
        self.alloc_element(el)
    }
    fn alloc_text(&mut self, start: u32, end: u32) -> NodeId {
        self.alloc_text(start, end)
    }
    fn alloc_comment(&mut self, start: u32, end: u32) -> NodeId {
        self.alloc_comment(start, end)
    }
    fn alloc_interpolation(&mut self, start: u32, end: u32) -> NodeId {
        self.alloc_interpolation(start, end)
    }
    fn set_root_children(&mut self, children: Vec<NodeId>) {
        self.root_children = children;
    }
}

impl ArenaApi for boxed::Arena {
    fn alloc_element(&mut self, el: ElementNode) -> NodeId {
        self.alloc_element(el)
    }
    fn alloc_text(&mut self, start: u32, end: u32) -> NodeId {
        self.alloc_text(start, end)
    }
    fn alloc_comment(&mut self, start: u32, end: u32) -> NodeId {
        self.alloc_comment(start, end)
    }
    fn alloc_interpolation(&mut self, start: u32, end: u32) -> NodeId {
        self.alloc_interpolation(start, end)
    }
    fn set_root_children(&mut self, children: Vec<NodeId>) {
        self.root_children = children;
    }
}

/// Build a realistic template tree:
///   root
///   └── div.app (wrapper)
///       ├── span.item-0 → text
///       ├── input (self-closing, 3 props)
///       ├── button (2 props) → text + interpolation
///       ├── p (v-if, 2 props) → interpolation
///       ├── div (v-for, 2 props) → interpolation
///       ├── ... repeats for `element_count` elements
///       └── <!-- comment -->
///
/// Mix: ~60% elements, ~20% text, ~10% interpolation, ~10% comments
fn build_realistic_tree<A: ArenaApi>(arena: &mut A, element_count: usize) -> Vec<NodeId> {
    let mut pos: u32 = 0;
    let mut wrapper_children = Vec::new();

    for i in 0..element_count {
        match i % 5 {
            0 => {
                // <span class="item">text</span>
                let text = arena.alloc_text(pos + 6, pos + 10);
                pos += 10;
                let span = arena.alloc_element(
                    ElementNode::new(pos, pos + 6)
                        .with_props(2)
                        .with_children(vec![text]),
                );
                pos += 12;
                wrapper_children.push(span);
            }
            1 => {
                // <input v-model type placeholder /> (self-closing, 3 props)
                let input = arena.alloc_element(ElementNode::new(pos, pos + 8).with_props(3));
                pos += 16;
                wrapper_children.push(input);
            }
            2 => {
                // <button @click :disabled>Click {{ count }}</button>
                let text = arena.alloc_text(pos + 8, pos + 14);
                let interp = arena.alloc_interpolation(pos + 14, pos + 25);
                let btn = arena.alloc_element(
                    ElementNode::new(pos, pos + 8)
                        .with_props(2)
                        .with_children(vec![text, interp]),
                );
                pos += 35;
                wrapper_children.push(btn);
            }
            3 => {
                // <p v-if="show" :class="cls">{{ msg }}</p>
                let interp = arena.alloc_interpolation(pos + 6, pos + 15);
                let p = arena.alloc_element(
                    ElementNode::new(pos, pos + 6)
                        .with_props(2)
                        .with_children(vec![interp]),
                );
                pos += 22;
                wrapper_children.push(p);
            }
            4 => {
                // <!-- comment -->
                let comment = arena.alloc_comment(pos, pos + 16);
                pos += 16;
                wrapper_children.push(comment);
            }
            _ => unreachable!(),
        }
    }

    // Wrap everything in a <div class="app">...</div>
    let wrapper = arena.alloc_element(
        ElementNode::new(0, 5)
            .with_props(1)
            .with_children(wrapper_children.clone()),
    );

    let root_children = vec![wrapper];
    arena.set_root_children(root_children.clone());
    root_children
}

// ============================================================================
// Benchmarks
// ============================================================================

fn bench_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("ast_construction");

    for &count in &[10, 50, 100, 200, 1000] {
        group.bench_with_input(BenchmarkId::new("inline", count), &count, |b, &count| {
            b.iter(|| {
                let mut arena = inline::Arena::new();
                build_realistic_tree(&mut arena, count);
                black_box(&arena);
            });
        });

        group.bench_with_input(BenchmarkId::new("boxed", count), &count, |b, &count| {
            b.iter(|| {
                let mut arena = boxed::Arena::new();
                build_realistic_tree(&mut arena, count);
                black_box(&arena);
            });
        });
    }

    group.finish();
}

fn bench_dfs_traversal(c: &mut Criterion) {
    let mut group = c.benchmark_group("ast_dfs_traversal");

    for &count in &[10, 50, 100, 200, 1000] {
        // Pre-build arenas
        let mut inline_arena = inline::Arena::new();
        let inline_roots = build_realistic_tree(&mut inline_arena, count);

        let mut boxed_arena = boxed::Arena::new();
        let boxed_roots = build_realistic_tree(&mut boxed_arena, count);

        group.bench_with_input(BenchmarkId::new("inline", count), &count, |b, _| {
            b.iter(|| {
                let count = inline_arena.dfs(inline_roots[0]);
                black_box(count);
            });
        });

        group.bench_with_input(BenchmarkId::new("boxed", count), &count, |b, _| {
            b.iter(|| {
                let count = boxed_arena.dfs(boxed_roots[0]);
                black_box(count);
            });
        });
    }

    group.finish();
}

fn bench_enum_size(c: &mut Criterion) {
    // Not a real benchmark — just print sizes for reference.
    let mut group = c.benchmark_group("enum_sizes");

    group.bench_function("print_sizes", |b| {
        b.iter(|| {
            let inline_node_size = std::mem::size_of::<inline::AstNode>();
            let boxed_node_size = std::mem::size_of::<boxed::AstNode>();
            let inline_kind_size = std::mem::size_of::<inline::AstNodeKind>();
            let boxed_kind_size = std::mem::size_of::<boxed::AstNodeKind>();
            let element_size = std::mem::size_of::<ElementNode>();

            black_box((
                inline_node_size,
                boxed_node_size,
                inline_kind_size,
                boxed_kind_size,
                element_size,
            ))
        });
    });

    group.finish();

    // Print for human reference (shows once in terminal)
    eprintln!("\n=== Type Sizes ===");
    eprintln!(
        "  ElementNode:          {} bytes",
        std::mem::size_of::<ElementNode>()
    );
    eprintln!(
        "  inline::AstNodeKind:  {} bytes",
        std::mem::size_of::<inline::AstNodeKind>()
    );
    eprintln!(
        "  boxed::AstNodeKind:   {} bytes",
        std::mem::size_of::<boxed::AstNodeKind>()
    );
    eprintln!(
        "  inline::AstNode:      {} bytes",
        std::mem::size_of::<inline::AstNode>()
    );
    eprintln!(
        "  boxed::AstNode:       {} bytes",
        std::mem::size_of::<boxed::AstNode>()
    );
    eprintln!(
        "  TextNode:             {} bytes",
        std::mem::size_of::<TextNode>()
    );
    eprintln!(
        "  CommentNode:          {} bytes",
        std::mem::size_of::<CommentNode>()
    );
    eprintln!(
        "  InterpolationNode:    {} bytes",
        std::mem::size_of::<InterpolationNode>()
    );
    eprintln!("==================\n");
}

criterion_group!(
    benches,
    bench_enum_size,
    bench_construction,
    bench_dfs_traversal
);
criterion_main!(benches);
