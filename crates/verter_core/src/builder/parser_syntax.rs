use crate::{
    cursor::position::PositionResolver,
    syntax::{
        plugin::{SyntaxPlugin, SyntaxPluginContext, SyntaxPluginOptions},
        plugins::ast_vue::{ast_vue::AstPlugin, types::RootNode},
        syntax::Syntax,
    },
    tokenizer::byte::tokenize,
};

pub fn parse<'a>(
    input: &'a str,
    options: &'a SyntaxPluginOptions,
    allocator: &'a oxc_allocator::Allocator,
) -> &'a RootNode<'a> {
    let bytes = input.as_bytes();

    let mut context = SyntaxPluginContext::new(input, input.as_bytes(), options);

    let position_resolver = PositionResolver::new(input);

    let mut ast_plugin = AstPlugin::new(allocator, position_resolver);
    // let mut analysis = Analysis::new(vec![]);

    {
        let pipeline: Vec<&mut dyn SyntaxPlugin<'a>> = vec![&mut ast_plugin];
        let mut syntax = Syntax::new(pipeline);

        syntax.start(&mut context);
        tokenize(bytes, |e| {
            syntax.handle(&e, &mut context);
        });
        syntax.end(&mut context);
    }

    allocator.alloc(
        ast_plugin
            .take_root()
            .unwrap_or_else(|| panic!("Expected root node from AstPlugin, {}", input)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test parsing a complex real-world Vue component that was causing panics
    /// This file contains multiple nested v-if/v-for directives, dynamic attributes,
    /// and complex template expressions
    #[test]
    fn test_parse_complex_vue_component() {
        let source = include_str!("../../examples/ast/source/test.vue");
        let options = SyntaxPluginOptions::default();
        let allocator = oxc_allocator::Allocator::new();

        let root = parse(source, &options, &allocator);

        // Basic assertions to verify the file was parsed successfully
        assert_eq!(root.source, source);
        assert!(!root.children.is_empty(), "Root should have children");

        // The test.vue file starts with a <template> tag containing a <div> with v-if
        // We just verify the parse completes without panicking
        println!(
            "Successfully parsed complex Vue component with {} root children",
            root.children.len()
        );
    }

    #[test]
    fn test_parse_empty_void() {
        let source = "<img>";
        let options = SyntaxPluginOptions::default();
        let allocator = oxc_allocator::Allocator::new();

        let root = parse(source, &options, &allocator);
        assert_eq!(root.source, source);
    }

    #[test]
    fn test_parse_lowercase_closing_tag() {
        let source = "<Div></div>";
        let options = SyntaxPluginOptions::default();
        let allocator = oxc_allocator::Allocator::new();

        let root = parse(source, &options, &allocator);
        assert_eq!(root.source, source);
    }

    #[test]
    fn test_parse_empty_template() {
        let source = "";
        let options = SyntaxPluginOptions::default();
        let allocator = oxc_allocator::Allocator::new();

        let root = parse(source, &options, &allocator);
        assert_eq!(root.source, source);
        assert!(
            root.children.is_empty(),
            "Empty template should have no children"
        );
    }

    #[test]
    fn test_parse_simple_element() {
        let source = "<div>Hello</div>";
        let options = SyntaxPluginOptions::default();
        let allocator = oxc_allocator::Allocator::new();

        let root = parse(source, &options, &allocator);
        assert_eq!(root.source, source);
        assert_eq!(root.children.len(), 1);
    }
}
