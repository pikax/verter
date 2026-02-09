// Generates an AST closer to Vue native AST structure

pub use crate::syntax::plugins::ast_vue::types::*;
use crate::{
    cursor::position::PositionResolver,
    syntax::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxResult},
        types::SyntaxEvent,
    },
    tokenizer::helpers::is_whitespace,
};

pub struct AstPlugin<'a> {
    allocator: &'a oxc_allocator::Allocator,
    stack: Vec<Node<'a>>,
    position_resolver: PositionResolver<'a>,
}

impl<'a> AstPlugin<'a> {
    pub fn new(
        alloc: &'a oxc_allocator::Allocator,
        position_resolver: PositionResolver<'a>,
    ) -> Self {
        Self {
            stack: Vec::with_capacity(32),
            allocator: alloc,
            position_resolver,
        }
    }

    pub fn take_root(&mut self) -> Option<RootNode<'a>> {
        match self.stack.pop() {
            Some(Node::Root(r)) => Some(r),
            _ => None,
        }
    }

    #[inline(always)]
    fn pop_and_store(&mut self) {
        if let Some(el) = self.stack.pop() {
            let parent = self.stack.last_mut().unwrap();
            match parent {
                Node::Root(r) => {
                    if let Node::Element(el) = el {
                        r.children.push(Node::Element(el));
                    }
                }
                Node::Element(parent_el) => {
                    if let Node::Element(el) = el {
                        parent_el.children.push(Node::Element(el));
                    }
                }
                _ => {
                    panic!("Unexpected parent node type");
                }
            }
        }
    }
}

impl<'a> SyntaxPlugin<'a> for AstPlugin<'a> {
    fn name(&self) -> &str {
        "ast_vue"
    }

    fn start(&mut self, ctx: &crate::syntax::plugin::SyntaxPluginContext<'a>) {
        self.stack.clear();

        let node = Node::Root(RootNode {
            loc: self
                .position_resolver
                .to_source_location(0, ctx.input.len() as u32),

            source: ctx.input,
            children: oxc_allocator::Vec::new_in(self.allocator),
        });

        self.stack.push(node);
    }

    fn process_event(
        &mut self,
        event: SyntaxEvent<'a>,
        ctx: &mut SyntaxPluginContext<'a>,
    ) -> SyntaxResult<SyntaxEvent<'a>> {
        match &event {
            SyntaxEvent::OpenTagStart(e) => {
                let start = e.start + 1;
                let name_bytes = &ctx.bytes[start as usize..e.name_end as usize];

                let tag_type = if name_bytes[0] >= b'A' && name_bytes[0] <= b'Z' {
                    ElementTypes::COMPONENT
                } else if name_bytes == b"template" {
                    ElementTypes::TEMPLATE
                } else if name_bytes == b"slot" {
                    ElementTypes::SLOT
                } else {
                    ElementTypes::ELEMENT
                };

                let node = Node::Element(ElementNode {
                    ns: Namespace::HTML,
                    tag: ctx.slice(start, e.name_end),
                    tag_type,
                    props: oxc_allocator::Vec::new_in(self.allocator),
                    children: oxc_allocator::Vec::new_in(self.allocator),
                    is_self_closing: false,
                    loc: None,

                    inner_loc: None,

                    open_tag_start: e.start,
                    open_tag_end: e.name_end,
                });

                self.stack.push(node)
            }
            SyntaxEvent::OpenTagEnd(e) => {
                if let Some(Node::Element(element_node)) = self.stack.last_mut() {
                    let loc = self
                        .position_resolver
                        .to_source_location(element_node.open_tag_start, e.end);

                    element_node.loc = Some(loc);
                    element_node.open_tag_end = e.end;

                    if e.self_closing || e.is_void_element {
                        self.pop_and_store();
                    }
                }
            }
            SyntaxEvent::CloseTag(e) => {
                if !e.is_void_element {
                    if let Some(Node::Element(element_node)) = self.stack.last_mut() {
                        let loc = self
                            .position_resolver
                            .to_source_location(element_node.open_tag_start, e.end);
                        element_node.loc = Some(loc);
                    }

                    self.pop_and_store();
                }
            }
            SyntaxEvent::Prop(e) => {
                let loc = self.position_resolver.to_source_location(e.start, e.end);

                if e.is_directive {
                    // if oxc_parser plugin is enabled, this will never execute.
                    let arg = if let Some(arg) = &e.arg {
                        Some(SimpleExpressionNode {
                            content: ctx.slice(arg.start, arg.end),
                            loc: self
                                .position_resolver
                                .to_source_location(arg.start, arg.end),
                            is_static: !arg.is_dynamic,
                            const_type: if arg.is_dynamic {
                                ConstantTypes::NotConstant
                            } else {
                                ConstantTypes::CanStringify
                            },
                        })
                    } else {
                        None
                    };

                    let exp = if let Some(value) = &e.value {
                        Some(SimpleExpressionNode {
                            content: ctx.slice(value.start, value.end),
                            loc: self
                                .position_resolver
                                .to_source_location(value.start, value.end),
                            is_static: false,
                            const_type: ConstantTypes::NotConstant,
                        })
                    } else {
                        None
                    };

                    let modifiers = if let Some(mods) = &e.modifiers {
                        Some(oxc_allocator::Vec::from_iter_in(
                            mods.iter().map(|m| SimpleExpressionNode {
                                content: ctx.slice(m.start, m.end),
                                loc: self.position_resolver.to_source_location(m.start, m.end),
                                is_static: true,
                                const_type: ConstantTypes::CanStringify,
                            }),
                            self.allocator,
                        ))
                    } else {
                        None
                    };

                    let raw_name = ctx.slice(e.start, e.name_end);
                    let bytes = &ctx.bytes[e.start as usize..e.name_end as usize];
                    let name = if bytes[0] == b'@' {
                        "on"
                    } else if bytes[0] == b':' || bytes[0] == b'.' {
                        "bind"
                    } else if bytes[0] == b'#' {
                        "slot"
                    } else if bytes.starts_with(b"v-") {
                        ctx.slice(e.start + 2, e.name_end)
                    } else {
                        ctx.slice(e.start, e.name_end)
                    };

                    let ev = DirectiveNode {
                        name,
                        loc,

                        arg,
                        modifiers,
                        exp,
                        for_parse_result: None,
                        raw_name,
                    };

                    if let Some(Node::Element(element_node)) = self.stack.last_mut() {
                        element_node.props.push(PropNode::Directive(ev));
                    }
                } else {
                    let name_loc = self
                        .position_resolver
                        .to_source_location(e.start, e.name_end);

                    // let value
                    let value = if let Some(value) = &e.value {
                        Some(TextNode {
                            content: ctx.slice(value.start, value.end),
                            loc: self
                                .position_resolver
                                .to_source_location(value.start, value.end),
                        })
                    } else {
                        None
                    };

                    let ev = AttributeNode {
                        name: ctx.slice(e.start, e.name_end),

                        loc,
                        name_loc,
                        value,
                    };
                    if let Some(Node::Element(element_node)) = self.stack.last_mut() {
                        element_node.props.push(PropNode::Attribute(ev));
                    }
                }
            }
            SyntaxEvent::OxcProp(_e) => {
                // TODO
            }

            SyntaxEvent::Interpolation(e) => {
                // content must remove all the whitespace around it
                let mut start_content = e.content_start as usize;
                let mut end_content = e.content_end as usize;

                while start_content < end_content && (is_whitespace(ctx.bytes[start_content])) {
                    if is_whitespace(ctx.bytes[start_content]) {
                        start_content += 1;
                    }
                }
                while end_content > start_content && (is_whitespace(ctx.bytes[end_content - 1])) {
                    if is_whitespace(ctx.bytes[end_content - 1]) {
                        end_content -= 1;
                    }
                }

                let ev = InterpolationNode {
                    content: SimpleExpressionNode {
                        content: ctx.slice(start_content as u32, end_content as u32),
                        loc: self
                            .position_resolver
                            .to_source_location(start_content as u32, end_content as u32),
                        is_static: false,
                        const_type: ConstantTypes::NotConstant,
                    },
                    loc: self.position_resolver.to_source_location(e.start, e.end),
                };
                if let Some(Node::Element(element_node)) = self.stack.last_mut() {
                    element_node.children.push(Node::Interpolation(ev));
                }
            }
            SyntaxEvent::OxcInterpolation(_e) => {
                // TODO
            }

            SyntaxEvent::Text(e) => {
                let loc = self.position_resolver.to_source_location(e.start, e.end);

                // find where the non-whitespace content starts and ends, if there's whitespace before just add one
                let mut start_content = e.start as usize;
                let mut end_content = e.end as usize;

                while start_content < end_content && (is_whitespace(ctx.bytes[start_content])) {
                    if is_whitespace(ctx.bytes[start_content]) {
                        start_content += 1;
                    }
                }
                while end_content > start_content && (is_whitespace(ctx.bytes[end_content - 1])) {
                    if is_whitespace(ctx.bytes[end_content - 1]) {
                        end_content -= 1;
                    }
                }

                // this will ensure that if there's leading or trailing whitespace, we add a single space instead
                // doing a start_content - 1 or end_content + 1 could get another "whitespace" char,
                // such as newline, tab, etc. so we just add a single space manually
                let ev = if start_content > e.start as usize || end_content < e.end as usize {
                    let mut builder = oxc_allocator::StringBuilder::new_in(self.allocator);

                    // Prepend space if start_content > e.start
                    if start_content > e.start as usize {
                        builder.push(' ');
                    }

                    // Add the actual content
                    builder.push_str(self.position_resolver.slice(start_content, end_content));

                    // Append space if end_content < e.end
                    if end_content < e.end as usize {
                        builder.push(' ');
                    }

                    // TODO confirm this is correct
                    let content = self.allocator.alloc_str(builder.as_str());
                    TextNode { content, loc }
                } else {
                    TextNode {
                        content: ctx.slice(e.start, e.end),
                        loc,
                    }
                };
                if let Some(Node::Element(element_node)) = self.stack.last_mut() {
                    element_node.children.push(Node::Text(ev));
                }
            }
            SyntaxEvent::Comment(e) => {
                let loc = self.position_resolver.to_source_location(e.start, e.end);

                let ev = CommentNode {
                    content: ctx.slice(e.start + 4, e.end - 3), // remove <!-- and -->
                    loc,
                };
                if let Some(Node::Element(element_node)) = self.stack.last_mut() {
                    element_node.children.push(Node::Comment(ev));
                }
            }

            // TODO implement
            _ => {}
        }

        SyntaxResult::Keep(event)
    }
}
