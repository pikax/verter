use std::any::Any;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use verter_language::carrier_grammar::{
    AcceptedRegisteredCarrierSource, CarrierGrammarAuthority, CarrierGrammarConfig,
    CarrierParserGrammarVersion, FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};
use verter_language::{
    CarrierAttribute, CarrierBlock, CarrierBlockInventory, CarrierStructureHash, MarkupNodeKind,
};

use super::carrier_compiler::{
    CarrierCompiler, CompileUnsupported, IdeCompileOptions, IdeOutput, ParseOptions,
    RuntimeCompileOptions, RuntimeCompileOutput, TemplateFacts,
};
use super::registered_carrier_projection::{
    materialize_registered_carrier_for_tests,
    project_registered_carrier_for_tests as project_registered_carrier, RegisteredCarrierPayload,
};
use super::registered_projector_seal::{
    mint_registered_projector_seal_for_tests, RegisteredProjectorSeal,
};
use super::registry::CarrierCompilerRegistry;

struct RegisteredCarrierProjection {
    carrier: RegisteredCarrierPayload,
    inventory: Arc<CarrierBlockInventory>,
    carrier_structure_hash: CarrierStructureHash,
}

type RegisteredProjectorForTests = fn(
    &dyn CarrierCompiler,
    &AcceptedRegisteredCarrierSource,
    &RegisteredProjectorSeal,
) -> (
    RegisteredCarrierPayload,
    Arc<CarrierBlockInventory>,
    CarrierStructureHash,
    bool,
);

struct CountingCarrierCompiler {
    inner: Arc<dyn CarrierCompiler>,
    parse_calls: AtomicUsize,
}

impl CountingCarrierCompiler {
    fn new(inner: Arc<dyn CarrierCompiler>) -> Self {
        Self {
            inner,
            parse_calls: AtomicUsize::new(0),
        }
    }

    fn parse_calls(&self) -> usize {
        self.parse_calls.load(Ordering::SeqCst)
    }
}

impl CarrierCompiler for CountingCarrierCompiler {
    fn __verter_as_any(&self) -> &dyn Any {
        // Keep the real framework projector selected while observing calls
        // independently of the projector and its returned carrier witness.
        self.inner.__verter_as_any()
    }

    fn adapter_id(&self) -> verter_language::FrameworkAdapterId {
        self.inner.adapter_id()
    }

    fn carrier_language_id(&self) -> verter_language::LanguageId {
        self.inner.carrier_language_id()
    }

    fn parse(
        &self,
        source: &str,
        opts: &ParseOptions,
    ) -> Arc<verter_language::FrameworkParseArtifact> {
        self.parse_calls.fetch_add(1, Ordering::SeqCst);
        self.inner.parse(source, opts)
    }

    fn eval_source(
        &self,
        source: &str,
        artifact: &verter_language::FrameworkParseArtifact,
    ) -> Arc<str> {
        self.inner.eval_source(source, artifact)
    }

    fn compile_ide(
        &self,
        source: &str,
        artifact: &verter_language::FrameworkParseArtifact,
        opts: &IdeCompileOptions,
    ) -> Result<IdeOutput, CompileUnsupported> {
        self.inner.compile_ide(source, artifact, opts)
    }

    fn template_data(
        &self,
        source: &str,
        artifact: &verter_language::FrameworkParseArtifact,
    ) -> TemplateFacts {
        self.inner.template_data(source, artifact)
    }

    fn compile_bundle(
        &self,
        source: &str,
        artifact: &verter_language::FrameworkParseArtifact,
        opts: &RuntimeCompileOptions,
        alloc: &oxc_allocator::Allocator,
    ) -> Result<RuntimeCompileOutput, CompileUnsupported> {
        self.inner.compile_bundle(source, artifact, opts, alloc)
    }
}

impl RegisteredCarrierProjection {
    fn carrier(&self) -> &RegisteredCarrierPayload {
        &self.carrier
    }

    fn inventory(&self) -> &Arc<CarrierBlockInventory> {
        &self.inventory
    }

    fn carrier_structure_hash(&self) -> CarrierStructureHash {
        self.carrier_structure_hash
    }
}

fn accepted(
    canonical: &str,
    language: verter_language::FileLanguage,
    source: &str,
) -> (
    RegisteredSourceAuthority,
    CarrierGrammarAuthority,
    AcceptedRegisteredCarrierSource,
) {
    let source_authority = RegisteredSourceAuthority::new().expect("source authority");
    let snapshot = source_authority
        .register_source(
            CanonicalFileId::new(canonical),
            FileIncarnation::new(1),
            SourceGeneration::new(1),
            language.clone(),
            Arc::from(source),
        )
        .expect("registered source");
    let grammar_authority = CarrierGrammarAuthority::new().expect("grammar authority");
    let config = if language.is_vue() {
        CarrierGrammarConfig::vue("{{", "}}", std::iter::empty::<&str>()).expect("Vue config")
    } else {
        CarrierGrammarConfig::Svelte
    };
    grammar_authority
        .register_carrier_grammar(
            language,
            FrameworkAdapterSemanticVersion::new(1).expect("adapter version"),
            CarrierParserGrammarVersion::new(1).expect("grammar version"),
            config.clone(),
        )
        .expect("grammar registration");
    let accepted = grammar_authority
        .accept_registered_source(&source_authority, &snapshot, &config)
        .expect("accepted source");
    (source_authority, grammar_authority, accepted)
}

fn project(accepted: &AcceptedRegisteredCarrierSource) -> RegisteredCarrierProjection {
    let registry = CarrierCompilerRegistry::built_in();
    let language = accepted.source().resolved_file_language();
    let compiler = registry
        .compiler_for_carrier_language(
            language.adapter_id().expect("carrier adapter"),
            language.carrier_language_id().expect("carrier language"),
        )
        .expect("registered compiler");
    project_with_compiler(compiler.as_ref(), accepted)
}

fn project_with_compiler(
    compiler: &dyn CarrierCompiler,
    accepted: &AcceptedRegisteredCarrierSource,
) -> RegisteredCarrierProjection {
    let seal = mint_registered_projector_seal_for_tests();
    let (carrier, inventory, carrier_structure_hash, same_carrier_arc) =
        project_registered_carrier(compiler, accepted, &seal);
    // This identity check complements the independent counting witness below;
    // exact-one parse enforcement does not rely on the projector's own witness.
    assert!(
        same_carrier_arc,
        "projection payload must retain the exact Arc produced by the sole parse"
    );
    RegisteredCarrierProjection {
        carrier,
        inventory,
        carrier_structure_hash,
    }
}

#[test]
fn b2_materialization_preserves_exact_carrier_and_inventory_arcs() {
    let registry = CarrierCompilerRegistry::built_in();
    for (path, language, source) in [
        (
            "file:///workspace/Materialize.vue",
            verter_language::FileLanguage::vue(),
            "<script setup>const x = 1</script><template>{{ x }}</template>",
        ),
        (
            "file:///workspace/Materialize.svelte",
            verter_language::FileLanguage::svelte(),
            "<script>let x = 1</script><p>{x}</p>",
        ),
    ] {
        let (_, _, accepted) = accepted(path, language, source);
        let resolved = accepted.source().resolved_file_language();
        let compiler = registry
            .compiler_for_carrier_language(
                resolved.adapter_id().unwrap(),
                resolved.carrier_language_id().unwrap(),
            )
            .unwrap();
        let seal = mint_registered_projector_seal_for_tests();
        assert_eq!(
            materialize_registered_carrier_for_tests(compiler.as_ref(), &accepted, &seal),
            (true, true)
        );
    }
}

#[test]
fn registered_projection_calls_parse_exactly_once_for_vue_and_svelte() {
    let registry = CarrierCompilerRegistry::built_in();
    let mut observed_parse_calls = Vec::new();
    for (path, language, source) in [
        (
            "file:///workspace/ParseCount.vue",
            verter_language::FileLanguage::vue(),
            "<template><div/></template>",
        ),
        (
            "file:///workspace/ParseCount.svelte",
            verter_language::FileLanguage::svelte(),
            "<div />",
        ),
    ] {
        let (_, _, accepted) = accepted(path, language, source);
        let resolved = accepted.source().resolved_file_language();
        let real_compiler = registry
            .compiler_for_carrier_language(
                resolved.adapter_id().expect("carrier adapter"),
                resolved.carrier_language_id().expect("carrier language"),
            )
            .expect("registered compiler");
        let counting_compiler = CountingCarrierCompiler::new(Arc::clone(real_compiler));

        let _projection = project_with_compiler(&counting_compiler, &accepted);
        observed_parse_calls.push((path, counting_compiler.parse_calls()));
    }
    assert_eq!(
        observed_parse_calls,
        [
            ("file:///workspace/ParseCount.vue", 1),
            ("file:///workspace/ParseCount.svelte", 1),
        ],
        "each registered projection must parse its carrier exactly once"
    );
}

#[test]
fn registered_projection_retains_the_sole_parse_carrier_arc_and_metadata() {
    for (path, language, source) in [
        (
            "file:///workspace/Identity.vue",
            verter_language::FileLanguage::vue(),
            "<template><div/></template>",
        ),
        (
            "file:///workspace/Identity.svelte",
            verter_language::FileLanguage::svelte(),
            "<div />",
        ),
    ] {
        let (_, _, accepted) = accepted(path, language, source);
        let projection = project(&accepted);
        let payload = projection.carrier();
        assert_eq!(
            payload.adapter_id(),
            accepted
                .source()
                .resolved_file_language()
                .adapter_id()
                .expect("carrier adapter")
        );
        assert_eq!(
            payload.language_id(),
            accepted
                .source()
                .resolved_file_language()
                .carrier_language_id()
                .expect("carrier language")
        );
        assert_ne!(payload.parser_version(), 0);
    }
}

#[test]
fn registered_vue_projection_uses_the_real_capability_and_preserves_order_and_duplicates() {
    let source = "<script setup lang=\"ts\">let x=1</script>\n<template><DIV a=\"&amp;\" a='two' :id=\"x\">{{x}}</DIV></template>\n<style scoped module>div{}</style>";
    let (_source_authority, _grammar_authority, accepted) = accepted(
        "file:///workspace/App.vue",
        verter_language::FileLanguage::vue(),
        source,
    );
    let projection = project(&accepted);
    let inventory = projection.inventory();

    inventory.validate().expect("valid projected inventory");
    assert_eq!(inventory.source_spaces()[0].bytes().as_ref(), source);
    assert!(inventory.blocks().windows(2).all(|pair| {
        inventory.block_start(&pair[0]).expect("first block")
            <= inventory.block_start(&pair[1]).expect("second block")
    }));
    let (template_host, template_syntax) = inventory
        .blocks()
        .iter()
        .find_map(|block| match block {
            CarrierBlock::Section {
                id,
                role: verter_language::SectionRole::TemplateHost,
                syntax,
            } => Some((*id, syntax)),
            _ => None,
        })
        .expect("template host");
    assert_eq!(
        inventory
            .slice(verter_language::SourceSlice::new(
                template_syntax.content_span,
            ))
            .expect("template content"),
        "<DIV a=\"&amp;\" a='two' :id=\"x\">{{x}}</DIV>"
    );
    assert_eq!(
        inventory
            .slice(verter_language::SourceSlice::new(
                template_syntax.closing_span.expect("template closing span"),
            ))
            .expect("template close"),
        "</template>"
    );
    let div = inventory
        .markup()
        .nodes()
        .iter()
        .find_map(|node| match node.kind() {
            MarkupNodeKind::Element(element)
                if inventory.slice(element.authored_name()).expect("name") == "DIV" =>
            {
                Some(element)
            }
            _ => None,
        })
        .expect("DIV element");
    assert!(inventory
        .markup()
        .roots()
        .iter()
        .all(|root| { inventory.markup().nodes()[root.0 as usize].root_block == template_host }));
    assert_eq!(
        inventory
            .normalized_name(div.normalized_name())
            .expect("normalised"),
        "div"
    );
    assert_eq!(div.attributes().len(), 3);
    assert!(matches!(
        div.attributes()[0],
        CarrierAttribute::Named { .. }
    ));
    assert_eq!(
        div.attributes()[1].duplicate_of(),
        Some(div.attributes()[0].id())
    );
    assert_eq!(
        inventory
            .decode_attribute_value(&div.attributes()[0])
            .expect("decode"),
        Some("&".into())
    );
}

#[test]
fn registered_svelte_projection_covers_closed_topology_families() {
    let source = "<script context=\"module\">export const m=1</script><script lang=\"ts\">let p=Promise.resolve(1)</script><svelte:head><title>x</title></svelte:head>{#each [1] as item, i (item)}<X {...item} bind:value={item} {@attach action}>{#await p then value}{:catch error}{error}{/await}{@render child(item)}{/each}<style>.x{}</style>";
    let (_source_authority, _grammar_authority, accepted) = accepted(
        "file:///workspace/App.svelte",
        verter_language::FileLanguage::svelte(),
        source,
    );
    let projection = project(&accepted);
    let inventory = projection.inventory();
    inventory.validate().expect("valid projected inventory");

    let kinds = inventory
        .markup()
        .nodes()
        .iter()
        .map(|node| node.kind().fragment_kind())
        .collect::<Vec<_>>();
    assert!(kinds.contains(&verter_language::MarkupFragmentKind::Element));
    assert!(kinds.contains(&verter_language::MarkupFragmentKind::SvelteControlBlock));
    assert!(kinds.contains(&verter_language::MarkupFragmentKind::SvelteClause));
    assert!(kinds.contains(&verter_language::MarkupFragmentKind::SvelteStandaloneTag));
    assert!(inventory
        .markup()
        .nodes()
        .iter()
        .flat_map(|node| node.kind().attributes())
        .any(|attr| matches!(attr, CarrierAttribute::Spread { .. })));
    assert!(inventory
        .markup()
        .nodes()
        .iter()
        .flat_map(|node| node.kind().attributes())
        .any(|attr| matches!(attr, CarrierAttribute::Directive { .. })));
    assert!(inventory
        .markup()
        .nodes()
        .iter()
        .flat_map(|node| node.kind().attributes())
        .any(|attr| matches!(attr, CarrierAttribute::Attach { .. })));
}

#[test]
fn registered_projector_closed_variant_matrix_is_exhaustive_for_live_parsers() {
    use verter_language::{
        AttributeValue, AttributeValuePart, DirectiveFamily, MarkupElementKind,
        SvelteAwaitInlineBranch, SvelteClauseHead, SvelteControlBlockHead, SvelteDirectiveKind,
        SvelteSpecialElementKind, SvelteStandaloneTagFamily, SyntaxTermination,
    };

    let svelte = r#"<svelte:head/><svelte:window/><svelte:document/><svelte:body/><svelte:element this="div"/><svelte:boundary/><svelte:options/><svelte:component this={C}/><svelte:self/><svelte:fragment/><svelte:mystery/>
<div a="x{value}y" {value} {...rest} bind:value={value} class:on={value} style:color|important={value} use:action transition:fade in:fly out:fly animate:flip on:click={go} let:item odd:name={value} {@attach action}><Widget/><style>.x{}</style></div>
{#if a}a{:else if b}b{:else}c{/if}{#each rows as row, i (row.id)}x{:else}e{/each}{#await promise then value}v{:catch error}e{/await}{#await promise catch error}e{:then value}v{/await}{#key key}k{/key}{#snippet row(item)}s{/snippet}
{@render row(value)}{@html html}{@const a = 1}{const b = 2}{let c = 3}{@debug a}{@attach action}{@mystery value}"#;
    let (_, _, accepted_svelte) = accepted(
        "file:///workspace/Matrix.svelte",
        verter_language::FileLanguage::svelte(),
        svelte,
    );
    let projection = project(&accepted_svelte);
    let inventory = projection.inventory();
    inventory.validate().expect("matrix inventory");
    let nodes = inventory.markup().nodes();

    let element_kinds = nodes
        .iter()
        .filter_map(|node| match node.kind() {
            MarkupNodeKind::Element(element) => Some(&element.kind),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(element_kinds
        .iter()
        .any(|kind| matches!(kind, MarkupElementKind::Native)));
    assert!(element_kinds
        .iter()
        .any(|kind| matches!(kind, MarkupElementKind::Component)));
    assert!(element_kinds
        .iter()
        .any(|kind| matches!(kind, MarkupElementKind::SvelteNestedStyle)));
    for special in [
        SvelteSpecialElementKind::Head,
        SvelteSpecialElementKind::Window,
        SvelteSpecialElementKind::Document,
        SvelteSpecialElementKind::Body,
        SvelteSpecialElementKind::Element,
        SvelteSpecialElementKind::Boundary,
        SvelteSpecialElementKind::Options,
        SvelteSpecialElementKind::Component,
        SvelteSpecialElementKind::SelfRef,
        SvelteSpecialElementKind::Fragment,
    ] {
        assert!(element_kinds.iter().any(
            |kind| matches!(kind, MarkupElementKind::SvelteSpecial(actual) if *actual == special)
        ));
    }
    assert!(element_kinds.iter().any(|kind| matches!(
        kind,
        MarkupElementKind::SvelteSpecial(SvelteSpecialElementKind::Unknown { .. })
    )));

    let heads = nodes
        .iter()
        .filter_map(|node| match node.kind() {
            MarkupNodeKind::SvelteControlBlock(block) => Some(&block.head),
            _ => None,
        })
        .collect::<Vec<_>>();
    // The parser records the consumed `{/keyword}` close-tag span; every
    // CLOSED control block in this matrix carries its authored closing span
    // and a truthful Closed termination.
    assert!(nodes.iter().all(|node| !matches!(
        node.kind(),
        MarkupNodeKind::SvelteControlBlock(block)
            if block.closing_span.is_none()
                || block.termination != SyntaxTermination::Closed
    )));
    assert!(heads
        .iter()
        .any(|head| matches!(head, SvelteControlBlockHead::If { .. })));
    assert!(heads.iter().any(|head| matches!(
        head,
        SvelteControlBlockHead::Each {
            item: Some(_),
            index: Some(_),
            key: Some(_),
            ..
        }
    )));
    assert!(heads.iter().any(|head| matches!(
        head,
        SvelteControlBlockHead::Await {
            inline_branch: SvelteAwaitInlineBranch::Then {
                binding: Some(_),
                ..
            },
            ..
        }
    )));

    let span_range = |span: verter_language::SourceSpan| (span.start, span.end);
    let await_node = |opening: &str| {
        let start = svelte.find(opening).expect("authored await block") as u32;
        nodes
            .iter()
            .find(|node| {
                matches!(
                    node.kind(),
                    MarkupNodeKind::SvelteControlBlock(block)
                        if block.full_span.start == start
                )
            })
            .expect("projected await block")
    };
    let projected_children = |node: &verter_language::MarkupSyntaxNode| {
        inventory.markup().child_ids()[node.children.start as usize..node.children.end as usize]
            .iter()
            .map(|id| &nodes[id.0 as usize])
            .collect::<Vec<_>>()
    };

    let then_opening = "{#await promise then value}";
    let then_start = svelte.find(then_opening).expect("inline then") as u32;
    let then_node = await_node(then_opening);
    let MarkupNodeKind::SvelteControlBlock(then_block) = then_node.kind() else {
        unreachable!("await node kind")
    };
    let SvelteControlBlockHead::Await {
        promise,
        inline_branch:
            SvelteAwaitInlineBranch::Then {
                marker_span,
                head_span,
                binding: Some(binding),
            },
    } = &then_block.head
    else {
        panic!("inline then head")
    };
    assert_eq!(
        span_range(promise.expect("authored promise span")),
        (then_start + 8, then_start + 15)
    );
    assert_eq!(span_range(*marker_span), (then_start + 16, then_start + 20));
    assert_eq!(span_range(*head_span), (then_start + 16, then_start + 26));
    assert_eq!(span_range(*binding), (then_start + 21, then_start + 26));
    let then_children = projected_children(then_node);
    assert_eq!(then_children.len(), 2, "body plus authored catch clause");
    assert!(matches!(
        then_children[0].kind(),
        MarkupNodeKind::Text { content_span }
            if span_range(*content_span) == (then_start + 27, then_start + 28)
    ));
    assert!(matches!(
        then_children[1].kind(),
        MarkupNodeKind::SvelteClause(clause)
            if matches!(clause.head, SvelteClauseHead::Catch { binding: Some(binding) }
                if span_range(binding) == (then_start + 36, then_start + 41))
                && span_range(clause.marker_span) == (then_start + 28, then_start + 42)
    ));

    let catch_opening = "{#await promise catch error}";
    let catch_start = svelte.find(catch_opening).expect("inline catch") as u32;
    let catch_node = await_node(catch_opening);
    let MarkupNodeKind::SvelteControlBlock(catch_block) = catch_node.kind() else {
        unreachable!("await node kind")
    };
    let SvelteControlBlockHead::Await {
        promise,
        inline_branch:
            SvelteAwaitInlineBranch::Catch {
                marker_span,
                head_span,
                binding: Some(binding),
            },
    } = &catch_block.head
    else {
        panic!("inline catch head")
    };
    assert_eq!(
        span_range(promise.expect("authored promise span")),
        (catch_start + 8, catch_start + 15)
    );
    assert_eq!(
        span_range(*marker_span),
        (catch_start + 16, catch_start + 21)
    );
    assert_eq!(span_range(*head_span), (catch_start + 16, catch_start + 27));
    assert_eq!(span_range(*binding), (catch_start + 22, catch_start + 27));
    let catch_children = projected_children(catch_node);
    assert_eq!(catch_children.len(), 2, "body plus authored then clause");
    assert!(matches!(
        catch_children[0].kind(),
        MarkupNodeKind::Text { content_span }
            if span_range(*content_span) == (catch_start + 28, catch_start + 29)
    ));
    assert!(matches!(
        catch_children[1].kind(),
        MarkupNodeKind::SvelteClause(clause)
            if matches!(clause.head, SvelteClauseHead::Then { binding: Some(binding) }
                if span_range(binding) == (catch_start + 36, catch_start + 41))
                && span_range(clause.marker_span) == (catch_start + 29, catch_start + 42)
    ));
    assert!(heads.iter().any(|head| matches!(
        head,
        SvelteControlBlockHead::Await {
            inline_branch: SvelteAwaitInlineBranch::Catch {
                binding: Some(_),
                ..
            },
            ..
        }
    )));
    assert!(heads
        .iter()
        .any(|head| matches!(head, SvelteControlBlockHead::Key { .. })));
    assert!(heads.iter().any(|head| matches!(
        head,
        SvelteControlBlockHead::Snippet {
            params_span: Some(_),
            ..
        }
    )));

    let clauses = nodes
        .iter()
        .filter_map(|node| match node.kind() {
            MarkupNodeKind::SvelteClause(clause) => Some(&clause.head),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(clauses
        .iter()
        .any(|head| matches!(head, SvelteClauseHead::ElseIf { .. })));
    assert!(clauses
        .iter()
        .any(|head| matches!(head, SvelteClauseHead::Else)));
    assert!(clauses
        .iter()
        .any(|head| matches!(head, SvelteClauseHead::Then { .. })));
    assert!(clauses
        .iter()
        .any(|head| matches!(head, SvelteClauseHead::Catch { .. })));

    let tags = nodes
        .iter()
        .filter_map(|node| match node.kind() {
            MarkupNodeKind::SvelteStandaloneTag(tag) => Some(&tag.family),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(tags
        .iter()
        .any(|tag| matches!(tag, SvelteStandaloneTagFamily::Render)));
    assert!(tags
        .iter()
        .any(|tag| matches!(tag, SvelteStandaloneTagFamily::Html)));
    assert!(tags
        .iter()
        .any(|tag| matches!(tag, SvelteStandaloneTagFamily::LegacyConst)));
    assert!(tags
        .iter()
        .any(|tag| matches!(tag, SvelteStandaloneTagFamily::Const)));
    assert!(tags
        .iter()
        .any(|tag| matches!(tag, SvelteStandaloneTagFamily::Let)));
    assert!(tags
        .iter()
        .any(|tag| matches!(tag, SvelteStandaloneTagFamily::Debug)));
    assert!(tags
        .iter()
        .any(|tag| matches!(tag, SvelteStandaloneTagFamily::Attach)));
    assert!(tags
        .iter()
        .any(|tag| matches!(tag, SvelteStandaloneTagFamily::Unknown { .. })));

    let attrs = nodes
        .iter()
        .flat_map(|node| node.kind().attributes())
        .collect::<Vec<_>>();
    assert!(attrs.iter().any(|attr| matches!(
        attr,
        CarrierAttribute::Named {
            syntax: verter_language::NamedAttributeSyntax::SvelteShorthand,
            ..
        }
    )));
    assert!(attrs
        .iter()
        .any(|attr| matches!(attr, CarrierAttribute::Spread { .. })));
    assert!(attrs
        .iter()
        .any(|attr| matches!(attr, CarrierAttribute::Attach { .. })));
    for expected in [
        SvelteDirectiveKind::Bind,
        SvelteDirectiveKind::On,
        SvelteDirectiveKind::Use,
        SvelteDirectiveKind::Class,
        SvelteDirectiveKind::Style,
        SvelteDirectiveKind::Let,
        SvelteDirectiveKind::Transition,
        SvelteDirectiveKind::In,
        SvelteDirectiveKind::Out,
        SvelteDirectiveKind::Animate,
    ] {
        assert!(attrs.iter().any(|attr| matches!(attr, CarrierAttribute::Directive { family: DirectiveFamily::Svelte(actual), .. } if *actual == expected)));
    }
    assert!(attrs.iter().all(|attr| !matches!(
        attr,
        CarrierAttribute::Directive {
            family: DirectiveFamily::Svelte(_),
            local_name: None,
            ..
        }
    )));
    assert!(attrs.iter().all(|attr| !matches!(
        attr,
        CarrierAttribute::Directive {
            family: DirectiveFamily::Svelte(_),
            argument: verter_language::DirectiveArgument::Static { .. }
                | verter_language::DirectiveArgument::Dynamic { .. },
            ..
        }
    )));
    assert!(attrs.iter().any(|attr| matches!(
        attr,
        CarrierAttribute::Directive {
            family: DirectiveFamily::Svelte(SvelteDirectiveKind::Unknown { .. }),
            ..
        }
    )));
    assert!(attrs.iter().any(|attr| matches!(attr, CarrierAttribute::Named { value: AttributeValue::Mixed { parts, .. }, .. } if matches!(parts.as_ref(), [AttributeValuePart::Static { .. }, AttributeValuePart::Expression { .. }, AttributeValuePart::Static { .. }]))));
    assert!(nodes.iter().any(|node| matches!(node.kind(), MarkupNodeKind::Element(element) if element.termination == SyntaxTermination::SelfClosing)));

    let vue = r#"<template><svg><path/></svg><math><mi/></math><MyWidget Foo="&copy;"><!-- c -->{{ value }}text</MyWidget><component :is="which"/><br><div :id="x" v-bind:[key].camel="value" v-custom:arg="x"/><div v-bind:[key].camel="value" @click.stop="go" v-model="value" v-show="ok" v-if="ok" v-else-if="other" v-else v-for="x in xs" v-slot="slot" v-pre v-cloak v-once v-memo="[x]" v-html="html" v-text="text" v-custom:arg="x"/></div></template>"#;
    let (_, _, accepted) = accepted(
        "file:///workspace/Matrix.vue",
        verter_language::FileLanguage::vue(),
        vue,
    );
    let projection = project(&accepted);
    let inventory = projection.inventory();
    inventory.validate().expect("Vue matrix inventory");
    let vue_attrs = inventory
        .markup()
        .nodes()
        .iter()
        .flat_map(|node| node.kind().attributes())
        .collect::<Vec<_>>();
    assert!(vue_attrs.iter().all(|attr| !matches!(
        attr,
        CarrierAttribute::Directive {
            family: DirectiveFamily::Vue(_),
            local_name: Some(_),
            ..
        }
    )));
    assert!(
        vue_attrs.iter().any(|attr| matches!(
            attr,
            CarrierAttribute::Directive {
                family: DirectiveFamily::Vue(verter_language::VueDirectiveKind::Bind),
                local_name: None,
                argument: verter_language::DirectiveArgument::Dynamic {
                    full_span,
                    open_span,
                    expression_span,
                    close_span: Some(close_span),
                    ..
                },
                ..
            } if inventory.slice_span(*full_span).expect("dynamic argument full") == "[key]"
                && inventory.slice_span(*open_span).expect("dynamic argument open") == "["
                && inventory.slice_span(*expression_span).expect("dynamic argument") == "key"
                && inventory.slice_span(*close_span).expect("dynamic argument close") == "]"
        )),
        "Vue attributes: {vue_attrs:#?}"
    );
    assert!(vue_attrs.iter().any(|attr| matches!(
        attr,
        CarrierAttribute::Directive {
            family:
                DirectiveFamily::Vue(verter_language::VueDirectiveKind::Custom {
                    authored,
                    normalized,
                }),
            local_name: None,
            argument: verter_language::DirectiveArgument::Static { name },
            ..
        } if inventory.slice(name.authored).expect("static argument") == "arg"
            && inventory.slice(*authored).expect("custom family") == "v-custom"
            && inventory.normalized_name(*normalized).expect("custom family name") == "v-custom"
    )));
    assert!(inventory.markup().nodes().iter().any(|node| matches!(node.kind(), MarkupNodeKind::Element(element) if element.namespace == verter_language::MarkupNamespace::Svg)));
    assert!(inventory.markup().nodes().iter().any(|node| matches!(node.kind(), MarkupNodeKind::Element(element) if element.namespace == verter_language::MarkupNamespace::MathMl)));
    assert!(inventory.markup().nodes().iter().any(|node| matches!(node.kind(), MarkupNodeKind::Element(element) if element.kind == MarkupElementKind::DynamicComponent)));
    assert!(inventory
        .markup()
        .nodes()
        .iter()
        .any(|node| matches!(node.kind(), MarkupNodeKind::Comment { .. })));
    assert!(inventory
        .markup()
        .nodes()
        .iter()
        .any(|node| matches!(node.kind(), MarkupNodeKind::Interpolation { .. })));
    assert!(inventory
        .markup()
        .nodes()
        .iter()
        .any(|node| matches!(node.kind(), MarkupNodeKind::Text { .. })));
    assert!(inventory.markup().nodes().iter().any(|node| matches!(node.kind(), MarkupNodeKind::Element(element) if element.termination == SyntaxTermination::Void)));
}

#[test]
fn carrier_structure_hash_ignores_offset_motion_but_discriminates_meaning() {
    let (_, _, first) = accepted(
        "file:///workspace/A.vue",
        verter_language::FileLanguage::vue(),
        "<template><div a=\"x\"/></template>",
    );
    let (_, _, moved) = accepted(
        "file:///workspace/B.vue",
        verter_language::FileLanguage::vue(),
        "\n\n<template><div a=\"x\"/></template>",
    );
    let (_, _, changed) = accepted(
        "file:///workspace/C.vue",
        verter_language::FileLanguage::vue(),
        "<template><span a=\"x\"/></template>",
    );
    assert_eq!(
        project(&first).carrier_structure_hash(),
        project(&moved).carrier_structure_hash()
    );
    assert_ne!(
        project(&first).carrier_structure_hash(),
        project(&changed).carrier_structure_hash()
    );
}

#[test]
fn projected_inventory_mutations_discriminate_without_reading_source_again() {
    use verter_language::parse_artifact::carrier_inventory::ScriptSourceType as CarrierScriptSourceType;
    use verter_language::{compute_carrier_structure_hash, CarrierBlockInventory, SectionRole};

    let (_, _, accepted) = accepted(
        "file:///workspace/Mutation.vue",
        verter_language::FileLanguage::vue(),
        "<script>let x=1</script><template><div/></template>",
    );
    let projection = project(&accepted);
    let inventory = projection.inventory();
    let rebuild = |blocks: Vec<CarrierBlock>| {
        CarrierBlockInventory::new(
            Arc::from(inventory.source_spaces().to_vec()),
            Arc::new(inventory.normalized_names().clone()),
            Arc::from(blocks),
            Arc::new(inventory.markup().clone()),
        )
        .expect("valid semantic mutation")
    };

    let mut dialect = inventory.blocks().to_vec();
    let CarrierBlock::Section { role, .. } = &mut dialect[0] else {
        panic!("script section");
    };
    let replacement = SectionRole::Script {
        role: verter_language::ScriptRole::Module,
        dialect: CarrierScriptSourceType::TypeScript,
    };
    *role = if *role == replacement {
        SectionRole::Script {
            role: verter_language::ScriptRole::Instance,
            dialect: CarrierScriptSourceType::JavaScript,
        }
    } else {
        replacement
    };
    assert_ne!(
        projection.carrier_structure_hash(),
        compute_carrier_structure_hash(&rebuild(dialect))
    );

    let mut recovery = inventory.blocks().to_vec();
    let CarrierBlock::Section { syntax, .. } = &mut recovery[0] else {
        panic!("script section");
    };
    syntax.termination = verter_language::SyntaxTermination::Recovered {
        reason: verter_language::BlockRecoveryReason::ParserRejectedSyntax,
        recovery_span: None,
    };
    assert_ne!(
        projection.carrier_structure_hash(),
        compute_carrier_structure_hash(&rebuild(recovery))
    );
}

#[test]
fn projected_inventory_validation_rejects_identity_span_and_cardinality_mutations() {
    use verter_language::{CarrierBlockInventory, InventoryValidationError};

    let (_, _, accepted) = accepted(
        "file:///workspace/Validation.vue",
        verter_language::FileLanguage::vue(),
        "<template><div a='x'/></template>",
    );
    let projection = project(&accepted);
    let inventory = projection.inventory();

    let mut spaces = inventory.source_spaces().to_vec();
    spaces[0].byte_len += 1;
    assert!(matches!(
        CarrierBlockInventory::new(
            Arc::from(spaces),
            Arc::new(inventory.normalized_names().clone()),
            Arc::from(inventory.blocks().to_vec()),
            Arc::new(inventory.markup().clone()),
        ),
        Err(InventoryValidationError::SourceLengthMismatch(_))
    ));

    let mut blocks = inventory.blocks().to_vec();
    let CarrierBlock::Section { syntax, .. } = &mut blocks[0] else {
        panic!("template section");
    };
    syntax.full_span.end = u32::MAX;
    assert!(matches!(
        CarrierBlockInventory::new(
            Arc::from(inventory.source_spaces().to_vec()),
            Arc::new(inventory.normalized_names().clone()),
            Arc::from(blocks),
            Arc::new(inventory.markup().clone()),
        ),
        Err(InventoryValidationError::InvalidSpan(_))
    ));

    let mut arena = inventory.markup().clone();
    arena.roots = Arc::from([arena.roots()[0], arena.roots()[0]]);
    assert!(matches!(
        CarrierBlockInventory::new(
            Arc::from(inventory.source_spaces().to_vec()),
            Arc::new(inventory.normalized_names().clone()),
            Arc::from(inventory.blocks().to_vec()),
            Arc::new(arena),
        ),
        Err(InventoryValidationError::InvalidRootOwnership(_))
    ));
}

#[test]
fn malformed_registered_sources_preserve_parser_owned_termination() {
    use verter_language::SyntaxTermination;

    for (path, language, source) in [
        (
            "file:///workspace/Broken.vue",
            verter_language::FileLanguage::vue(),
            "<template><div>",
        ),
        (
            "file:///workspace/Broken.svelte",
            verter_language::FileLanguage::svelte(),
            "<div>",
        ),
    ] {
        let (_, _, accepted) = accepted(path, language, source);
        let projection = project(&accepted);
        assert!(projection.inventory().markup().nodes().iter().any(|node| {
            matches!(
                node.kind(),
                MarkupNodeKind::Element(element)
                    if element.termination == SyntaxTermination::UnclosedEof
            )
        }));
    }
}

/// An unclosed dynamic directive argument at EOF (`<div v-bind:[key`) reaches
/// the projector: the tokenizer's EOF recovery still emits a dynamic
/// `DirArg` event (`X_MISSING_DYNAMIC_DIRECTIVE_ARGUMENT_END`). The projector
/// must emit a TYPED recovery argument — `close_span: None` +
/// `SyntaxTermination::UnclosedEof` — never panic and never fabricate a
/// closed bracket.
#[test]
fn unclosed_dynamic_directive_argument_at_eof_projects_typed_recovery() {
    use verter_language::{DirectiveArgument, DirectiveFamily, SyntaxTermination};

    let vue = "<template><div v-bind:[key";
    let (_, _, accepted) = accepted(
        "file:///workspace/UnclosedDynamicArg.vue",
        verter_language::FileLanguage::vue(),
        vue,
    );
    let projection = project(&accepted);
    let inventory = projection.inventory();
    inventory.validate().expect("recovered inventory");

    let attrs = inventory
        .markup()
        .nodes()
        .iter()
        .flat_map(|node| node.kind().attributes())
        .collect::<Vec<_>>();

    assert!(
        attrs.iter().any(|attr| matches!(
            attr,
            CarrierAttribute::Directive {
                family: DirectiveFamily::Vue(verter_language::VueDirectiveKind::Bind),
                argument: DirectiveArgument::Dynamic {
                    open_span,
                    expression_span,
                    close_span: None,
                    termination: SyntaxTermination::UnclosedEof,
                    ..
                },
                ..
            } if inventory.slice_span(*open_span).expect("open bracket") == "["
                && inventory.slice_span(*expression_span).expect("expression") == "key"
        )),
        "unclosed dynamic argument must project typed recovery: {attrs:#?}"
    );
    // Negative: NO fabricated closed bracket — no dynamic argument in this
    // document may carry a close span or a `Closed` termination.
    assert!(attrs.iter().all(|attr| !matches!(
        attr,
        CarrierAttribute::Directive {
            argument: DirectiveArgument::Dynamic {
                close_span: Some(_),
                ..
            },
            ..
        }
    )));
    assert!(attrs.iter().all(|attr| !matches!(
        attr,
        CarrierAttribute::Directive {
            argument: DirectiveArgument::Dynamic {
                termination: SyntaxTermination::Closed,
                ..
            },
            ..
        }
    )));
}

#[test]
fn registered_projector_signature_requires_authority_seal() {
    let _: RegisteredProjectorForTests = project_registered_carrier;
}

// ── FL2-E TE-C-12 / T-B1-D03: Svelte style dialect derives from `lang` ──

#[test]
fn registered_svelte_style_dialect_derives_from_authored_lang() {
    use verter_language::{SectionRole, StyleDialect, StyleModule};
    let cases: [(&str, StyleDialect); 5] = [
        ("<style>.x{}</style>", StyleDialect::Css),
        ("<style lang=\"css\">.x{}</style>", StyleDialect::Css),
        ("<style lang=\"scss\">.x{}</style>", StyleDialect::Scss),
        ("<style lang=\"less\">.x{}</style>", StyleDialect::Less),
        ("<style lang=\"stylus\">.x{}</style>", StyleDialect::Stylus),
    ];
    for (source, expected) in cases {
        let (_, _, accepted) = accepted(
            "file:///workspace/StyleLang.svelte",
            verter_language::FileLanguage::svelte(),
            source,
        );
        let projection = project(&accepted);
        let inventory = projection.inventory();
        inventory.validate().expect("style-lang inventory");
        let role = inventory
            .blocks()
            .iter()
            .find_map(|block| match block {
                CarrierBlock::Section {
                    role:
                        SectionRole::Style {
                            dialect,
                            scoped,
                            module,
                        },
                    ..
                } => Some((dialect.clone(), *scoped, module.clone())),
                _ => None,
            })
            .expect("style section");
        assert_eq!(role.0, expected, "dialect for {source}");
        // Svelte has no authored `scoped` / `module` attributes; the projection
        // must not fabricate them.
        assert!(!role.1, "scoped for {source}");
        assert_eq!(role.2, StyleModule::None, "module for {source}");
    }
}

#[test]
fn registered_svelte_style_dialect_mutation_discriminates_structure_hash() {
    let (_, _, scss) = accepted(
        "file:///workspace/A.svelte",
        verter_language::FileLanguage::svelte(),
        "<style lang=\"scss\">.x{}</style>",
    );
    let (_, _, less) = accepted(
        "file:///workspace/B.svelte",
        verter_language::FileLanguage::svelte(),
        "<style lang=\"less\">.x{}</style>",
    );
    assert_ne!(
        project(&scss).carrier_structure_hash(),
        project(&less).carrier_structure_hash(),
        "distinct authored style dialects must hash differently"
    );
}

// ── FL2-E TE-B-06 / T-B1-D02: custom-name hash closure ──

#[test]
fn custom_block_name_mutation_discriminates_structure_hash() {
    let (_, _, docs) = accepted(
        "file:///workspace/A.vue",
        verter_language::FileLanguage::vue(),
        "<docs>x</docs>",
    );
    let (_, _, spec) = accepted(
        "file:///workspace/B.vue",
        verter_language::FileLanguage::vue(),
        "<spec>x</spec>",
    );
    let (_, _, moved) = accepted(
        "file:///workspace/C.vue",
        verter_language::FileLanguage::vue(),
        "\n\n<docs>x</docs>",
    );
    assert_ne!(
        project(&docs).carrier_structure_hash(),
        project(&spec).carrier_structure_hash(),
        "distinct custom block names must hash differently"
    );
    assert_eq!(
        project(&docs).carrier_structure_hash(),
        project(&moved).carrier_structure_hash(),
        "custom block offset motion must not change the hash"
    );
}

#[test]
fn custom_directive_family_name_mutation_discriminates_structure_hash() {
    let (_, _, foo) = accepted(
        "file:///workspace/A.vue",
        verter_language::FileLanguage::vue(),
        "<template><div v-foo=\"x\"/></template>",
    );
    let (_, _, bar) = accepted(
        "file:///workspace/B.vue",
        verter_language::FileLanguage::vue(),
        "<template><div v-bar=\"x\"/></template>",
    );
    assert_ne!(
        project(&foo).carrier_structure_hash(),
        project(&bar).carrier_structure_hash(),
        "distinct custom directive family names must hash differently"
    );
}

// ── FL2-E TE-B-08 / T-B1-D05: parser-produced golden depth ──

#[test]
fn vue_nested_same_name_template_blocks_close_at_the_outermost_marker() {
    use verter_language::{SectionRole, SyntaxTermination};
    let source = "<template><div><template v-if=\"x\"><span/></template></div></template>";
    let (_, _, accepted) = accepted(
        "file:///workspace/Nested.vue",
        verter_language::FileLanguage::vue(),
        source,
    );
    let projection = project(&accepted);
    let inventory = projection.inventory();
    inventory.validate().expect("nested inventory");
    let sections = inventory
        .blocks()
        .iter()
        .filter_map(|block| match block {
            CarrierBlock::Section { role, syntax, .. } => Some((role, syntax)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(sections.len(), 1, "exactly one template HOST block");
    let (role, syntax) = sections[0];
    assert!(matches!(role, SectionRole::TemplateHost));
    let outer_close = source.rfind("</template>").expect("outer close") as u32;
    let close = syntax.closing_span.expect("outer template close span");
    assert_eq!(close.start, outer_close, "block closes at the LAST marker");
    assert_eq!(syntax.termination, SyntaxTermination::Closed);

    let inner_close = source.find("</template>").expect("inner close") as u32;
    let inner = inventory
        .markup()
        .nodes()
        .iter()
        .find_map(|node| match node.kind() {
            MarkupNodeKind::Element(element)
                if inventory
                    .normalized_name(element.normalized_name)
                    .expect("name")
                    == "template" =>
            {
                Some(element)
            }
            _ => None,
        })
        .expect("nested template element");
    assert_eq!(inner.termination, SyntaxTermination::Closed);
    assert_eq!(
        inner.closing_span.expect("inner close span").start,
        inner_close,
        "nested same-name element closes at the FIRST marker"
    );
}

#[test]
fn vue_recovered_unclosed_element_projects_truthful_termination() {
    use verter_language::SyntaxTermination;
    let source = "<template><div>text</template>";
    let (_, _, accepted) = accepted(
        "file:///workspace/Unclosed.vue",
        verter_language::FileLanguage::vue(),
        source,
    );
    let projection = project(&accepted);
    let inventory = projection.inventory();
    inventory.validate().expect("unclosed inventory");
    let div = inventory
        .markup()
        .nodes()
        .iter()
        .find_map(|node| match node.kind() {
            MarkupNodeKind::Element(element)
                if inventory
                    .normalized_name(element.normalized_name)
                    .expect("name")
                    == "div" =>
            {
                Some(element)
            }
            _ => None,
        })
        .expect("unclosed div");
    // The parser recovered an unclosed element: no fabricated Closed, no
    // fabricated closing span.
    assert!(div.closing_span.is_none(), "no fabricated closing span");
    assert!(
        !matches!(div.termination, SyntaxTermination::Closed),
        "no fabricated Closed termination: {:?}",
        div.termination
    );
}

#[test]
fn svelte_nested_same_name_blocks_and_elements_close_at_their_own_markers() {
    use verter_language::SyntaxTermination;
    let source = "{#if a}{#if b}<div><div>x</div></div>{/if}{/if}";
    let (_, _, accepted) = accepted(
        "file:///workspace/Nested.svelte",
        verter_language::FileLanguage::svelte(),
        source,
    );
    let projection = project(&accepted);
    let inventory = projection.inventory();
    inventory.validate().expect("nested inventory");
    let nodes = inventory.markup().nodes();

    let inner_close = source.find("{/if}").expect("inner if close") as u32;
    let outer_close = source.rfind("{/if}").expect("outer if close") as u32;
    let inner_start = source.find("{#if b}").expect("inner if") as u32;
    let blocks = nodes
        .iter()
        .filter_map(|node| match node.kind() {
            MarkupNodeKind::SvelteControlBlock(block) => Some(block),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(blocks.len(), 2, "two nested if blocks");
    for block in &blocks {
        assert_eq!(
            block.termination,
            SyntaxTermination::Closed,
            "closed control block"
        );
    }
    let inner = blocks
        .iter()
        .find(|block| block.full_span.start == inner_start)
        .expect("inner block");
    let outer = blocks
        .iter()
        .find(|block| block.full_span.start == 0)
        .expect("outer block");
    assert_eq!(
        inner.closing_span.expect("inner close span").start,
        inner_close,
        "inner block closes at the FIRST close marker"
    );
    assert_eq!(
        outer.closing_span.expect("outer close span").start,
        outer_close,
        "outer block closes at the LAST close marker"
    );

    let inner_div_close = source.find("</div>").expect("inner div close") as u32;
    let outer_div_close = source.rfind("</div>").expect("outer div close") as u32;
    let divs = nodes
        .iter()
        .filter_map(|node| match node.kind() {
            MarkupNodeKind::Element(element)
                if inventory
                    .normalized_name(element.normalized_name)
                    .expect("name")
                    == "div" =>
            {
                Some(element)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(divs.len(), 2, "two nested divs");
    let closes = divs
        .iter()
        .map(|element| element.closing_span.expect("div close").start)
        .collect::<Vec<_>>();
    assert!(closes.contains(&inner_div_close));
    assert!(closes.contains(&outer_div_close));
}

#[test]
fn svelte_unclosed_control_block_projects_recovered_not_fabricated_closed() {
    use verter_language::{BlockRecoveryReason, SyntaxTermination};
    let source = "{#if a}txt";
    let (_, _, accepted) = accepted(
        "file:///workspace/UnclosedBlock.svelte",
        verter_language::FileLanguage::svelte(),
        source,
    );
    let projection = project(&accepted);
    let inventory = projection.inventory();
    inventory.validate().expect("unclosed inventory");
    let block = inventory
        .markup()
        .nodes()
        .iter()
        .find_map(|node| match node.kind() {
            MarkupNodeKind::SvelteControlBlock(block) => Some(block),
            _ => None,
        })
        .expect("unclosed if block");
    assert!(block.closing_span.is_none(), "no fabricated closing span");
    assert_eq!(
        block.termination,
        SyntaxTermination::Recovered {
            reason: BlockRecoveryReason::MissingCloseTag,
            recovery_span: None,
        },
        "a missing close marker must project recovery, not a fabricated Closed"
    );
}

/// R2-C-01: parser-producible EMPTY block heads (`{#if}` and friends — the
/// ordinary mid-typing states) must project typed recovery (`None` head
/// expression), never panic the projector.
fn svelte_control_block_head(source: &str) -> verter_language::SvelteControlBlockHead {
    let (_, _, accepted) = accepted(
        "file:///workspace/EmptyHead.svelte",
        verter_language::FileLanguage::svelte(),
        source,
    );
    let projection = project(&accepted);
    let inventory = projection.inventory();
    inventory.validate().expect("empty-head inventory");
    inventory
        .markup()
        .nodes()
        .iter()
        .find_map(|node| match node.kind() {
            MarkupNodeKind::SvelteControlBlock(block) => Some(block.head.clone()),
            _ => None,
        })
        .expect("control block node")
}

#[test]
fn svelte_empty_if_head_projects_typed_recovery_not_panic() {
    use verter_language::SvelteControlBlockHead;
    for source in ["{#if}{/if}", "{#if }{/if}"] {
        let head = svelte_control_block_head(source);
        assert!(
            matches!(head, SvelteControlBlockHead::If { condition: None }),
            "an empty `{{#if}}` head must project `condition: None`, got: {head:?}"
        );
    }
    // Negative control: a non-empty head still projects its authored span.
    let head = svelte_control_block_head("{#if cond}{/if}");
    assert!(
        matches!(
            head,
            verter_language::SvelteControlBlockHead::If { condition: Some(_) }
        ),
        "a non-empty `{{#if cond}}` head must keep its condition span, got: {head:?}"
    );
}

#[test]
fn svelte_empty_each_head_projects_typed_recovery_not_panic() {
    use verter_language::SvelteControlBlockHead;
    let head = svelte_control_block_head("{#each}{/each}");
    assert!(
        matches!(
            head,
            SvelteControlBlockHead::Each {
                iterable: None,
                item: None,
                index: None,
                key: None,
            }
        ),
        "an empty `{{#each}}` head must project `iterable: None`, got: {head:?}"
    );
}

#[test]
fn svelte_empty_await_head_projects_typed_recovery_not_panic() {
    use verter_language::{SvelteAwaitInlineBranch, SvelteControlBlockHead};
    let head = svelte_control_block_head("{#await}{/await}");
    assert!(
        matches!(
            head,
            SvelteControlBlockHead::Await {
                promise: None,
                inline_branch: SvelteAwaitInlineBranch::None,
            }
        ),
        "an empty `{{#await}}` head must project `promise: None`, got: {head:?}"
    );
}

#[test]
fn svelte_empty_key_head_projects_typed_recovery_not_panic() {
    use verter_language::SvelteControlBlockHead;
    let head = svelte_control_block_head("{#key}{/key}");
    assert!(
        matches!(head, SvelteControlBlockHead::Key { expression: None }),
        "an empty `{{#key}}` head must project `expression: None`, got: {head:?}"
    );
}

#[test]
fn svelte_empty_else_if_condition_projects_typed_recovery_not_panic() {
    use verter_language::SvelteClauseHead;
    let source = "{#if x}a{:else if}b{/if}";
    let (_, _, accepted) = accepted(
        "file:///workspace/EmptyElseIf.svelte",
        verter_language::FileLanguage::svelte(),
        source,
    );
    let projection = project(&accepted);
    let inventory = projection.inventory();
    inventory.validate().expect("empty else-if inventory");
    let head = inventory
        .markup()
        .nodes()
        .iter()
        .find_map(|node| match node.kind() {
            MarkupNodeKind::SvelteClause(clause) => Some(clause.head.clone()),
            _ => None,
        })
        .expect("clause node");
    assert!(
        matches!(head, SvelteClauseHead::ElseIf { condition: None }),
        "an empty `{{:else if}}` condition must project `condition: None`, got: {head:?}"
    );
}

#[test]
fn svelte_unknown_block_and_tag_project_unknown_node_kinds() {
    use verter_language::{SvelteStandaloneTagFamily, SyntaxTermination, UnknownMarkupReason};
    let source = "{#wat q}body{/wat}{@wat expr}";
    let (_, _, accepted) = accepted(
        "file:///workspace/Unknown.svelte",
        verter_language::FileLanguage::svelte(),
        source,
    );
    let projection = project(&accepted);
    let inventory = projection.inventory();
    inventory.validate().expect("unknown inventory");
    let nodes = inventory.markup().nodes();

    // An unknown `{#wat}` block is an UNKNOWN node kind (parser-owned
    // classification), never silently reshaped into a known block family.
    let unknown = nodes
        .iter()
        .find_map(|node| match node.kind() {
            MarkupNodeKind::Unknown {
                termination,
                authored_head,
                reason,
                closing_span,
                full_span,
                ..
            } => Some((termination, authored_head, reason, closing_span, full_span)),
            _ => None,
        })
        .expect("unknown block node");
    assert_eq!(*unknown.2, UnknownMarkupReason::ParserUnknownVariant);
    let head = unknown.1.expect("authored head");
    assert_eq!(
        inventory.slice(head).expect("head slice"),
        "wat",
        "authored head names the unknown keyword"
    );
    assert_eq!(
        *unknown.0,
        SyntaxTermination::Closed,
        "the close marker was consumed"
    );
    assert_eq!(
        unknown.3.expect("unknown block close span").start,
        source.find("{/wat}").expect("close") as u32
    );
    assert_eq!(unknown.4.start, 0);

    // No control-block node may absorb the unknown keyword.
    assert!(nodes
        .iter()
        .all(|node| !matches!(node.kind(), MarkupNodeKind::SvelteControlBlock(_))));

    // `{@wat}` stays a standalone tag with an UNKNOWN family.
    assert!(nodes.iter().any(|node| matches!(
        node.kind(),
        MarkupNodeKind::SvelteStandaloneTag(tag)
            if matches!(&tag.family, SvelteStandaloneTagFamily::Unknown { authored_name, reason }
                if *reason == UnknownMarkupReason::ParserUnknownVariant
                    && inventory.slice(*authored_name).expect("tag name").starts_with("@wat"))
    )));
}
