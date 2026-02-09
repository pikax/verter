use bumpalo::Bump;

use crate::{
    cursor::position::PositionResolver,
    plugin::translator::{
        plugin::{TranslatorOptions, TranslatorPlugin, TranslatorPluginContext},
        plugins::{
            ast::{ast::AstPlugin, types::RootNode},
            oxc_parser::oxc_parser::OxcParserPlugin,
        },
        translator::Translator,
    },
    runner::runner::RunnerContext,
    tokenizer::{
        byte::tokenize,
        plugin::{TokenizerOptions, TokenizerPluginContext},
    },
};

pub fn parse<'a, 'bump, 'alloc>(
    bump: &'bump Bump,
    input: &'a str,
    options: &'a TokenizerOptions,
    allocator: &'alloc oxc_allocator::Allocator,
) -> &'bump RootNode<'a, 'bump>
where
    'bump: 'a,
    'alloc: 'a,
    'a: 'alloc,
{
    let _bytes = input.as_bytes();

    let tokenizer_runner = RunnerContext::new(bump, input, input.as_bytes(), options);

    let _tokenizer_context = TokenizerPluginContext {
        runner: &tokenizer_runner,
        position: PositionResolver::new(input),
    };
    let translator_opts = bump.alloc(TranslatorOptions::default());

    // Allocate runner in bump so it lives for 'bump
    let runner = bump.alloc(RunnerContext::new(
        bump,
        input,
        input.as_bytes(),
        translator_opts,
    ));

    // Initialize plugins

    // Allocate plugins in bump so they live for 'bump
    let oxc_parser_plugin = bump.alloc(OxcParserPlugin::new(allocator));
    let ast_plugin = bump.alloc(AstPlugin::new(bump));

    // {
    let mut pipeline: Vec<&mut dyn TranslatorPlugin<'a, 'bump, 'alloc>> = vec![
        oxc_parser_plugin as &mut dyn TranslatorPlugin<'a, 'bump, 'alloc>,
        ast_plugin as &mut dyn TranslatorPlugin<'a, 'bump, 'alloc>,
    ];

    let mut ctx = TranslatorPluginContext {
        runner,
        parsed: std::collections::HashMap::new(),
    };

    // Call start on all plugins before moving them
    for plugin in pipeline.iter_mut() {
        plugin.start(&ctx);
    }
    let mut translator = Translator::new(bump, pipeline);

    tokenize(_bytes, |e| {
        translator.process_event(&e, &mut ctx);
    });

    // Call end on all plugins through translator's pipeline
    for plugin in translator.pipeline.iter_mut() {
        plugin.end(&ctx);
    }
    // }

    // Take root from ast_plugin and allocate in bump
    bump.alloc(
        ast_plugin
            .take_root()
            .expect(format!("Expected root node from AstPlugin, {}", input).as_str()),
    )
}
