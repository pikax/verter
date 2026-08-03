use std::sync::Arc;
use std::{fs, path::Path};

use quote::ToTokens;
use syn::visit::Visit;

use verter_language::carrier_grammar::{
    AcceptedRegisteredCarrierSource, CarrierGrammarAuthority, CarrierGrammarConfig,
    CarrierParserGrammarVersion, FrameworkAdapterSemanticVersion,
};
use verter_language::registered_source_authority::{
    CanonicalFileId, FileIncarnation, RegisteredSourceAuthority, SourceGeneration,
};
use verter_language::{CarrierAttribute, CarrierBlock, MarkupNodeKind};

use super::registered_carrier_projection::{
    project_registered_carrier_for_tests as project_registered_carrier, RegisteredCarrierProjection,
};
use super::registry::CarrierCompilerRegistry;

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
    project_registered_carrier(compiler.as_ref(), accepted)
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
    let template_host = inventory
        .blocks()
        .iter()
        .find_map(|block| match block {
            CarrierBlock::Section {
                id,
                role: verter_language::SectionRole::TemplateHost,
                ..
            } => Some(*id),
            _ => None,
        })
        .expect("template host");
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
    // Parser-bound limitation: `SvelteBlock` carries only its opening and full
    // spans, not a closing span or recovery fact. Pin the absent projection
    // channel here until the parser DTO can publish those authored facts.
    assert!(nodes.iter().all(|node| !matches!(
        node.kind(),
        MarkupNodeKind::SvelteControlBlock(block) if block.closing_span.is_some()
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
    assert_eq!(span_range(*promise), (then_start + 8, then_start + 15));
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
    assert_eq!(span_range(*promise), (catch_start + 8, catch_start + 15));
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
            family: DirectiveFamily::Vue(verter_language::VueDirectiveKind::Custom),
            local_name: None,
            argument: verter_language::DirectiveArgument::Static { name },
            ..
        } if inventory.slice(name.authored).expect("static argument") == "arg"
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

#[test]
fn registered_projector_signature_is_capability_only() {
    let _: fn(
        &dyn super::carrier_compiler::CarrierCompiler,
        &AcceptedRegisteredCarrierSource,
    ) -> RegisteredCarrierProjection = project_registered_carrier;
}

struct CapabilityCallVisitor {
    calls: Vec<String>,
    module_path: Vec<String>,
}

impl Default for CapabilityCallVisitor {
    fn default() -> Self {
        Self::for_module(["verter_compiler"])
    }
}

impl CapabilityCallVisitor {
    const PROJECTOR: [&'static str; 4] = [
        "verter_compiler",
        "framework_common",
        "registered_carrier_projection",
        "project_registered_carrier",
    ];

    fn for_module(parts: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            calls: Vec::new(),
            module_path: parts.into_iter().map(Into::into).collect(),
        }
    }

    fn resolve_path(&self, path: &syn::Path) -> Vec<String> {
        let segments = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>();
        let Some(first) = segments.first().map(String::as_str) else {
            return Vec::new();
        };
        if path.leading_colon.is_some() || first == "verter_compiler" {
            return segments;
        }
        if first == "crate" {
            let mut resolved = vec![self.module_path[0].clone()];
            resolved.extend(segments.into_iter().skip(1));
            return resolved;
        }

        let mut resolved = self.module_path.clone();
        let mut offset = 0;
        if first == "self" {
            offset = 1;
        } else {
            while segments.get(offset).is_some_and(|part| part == "super") {
                if resolved.len() > 1 {
                    resolved.pop();
                }
                offset += 1;
            }
        }
        resolved.extend(segments.into_iter().skip(offset));
        resolved
    }

    fn record_projector_path(&mut self, path: &syn::Path) {
        if self.resolve_path(path) == Self::PROJECTOR {
            self.calls.push(Self::PROJECTOR.join("::"));
        }
    }
}

impl<'ast> Visit<'ast> for CapabilityCallVisitor {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        if item_is_test_only(item_attrs(item)) {
            return;
        }
        if let syn::Item::Mod(module) = item {
            if let Some((_, items)) = &module.content {
                self.module_path.push(module.ident.to_string());
                for item in items {
                    self.visit_item(item);
                }
                self.module_path.pop();
                return;
            }
        }
        syn::visit::visit_item(self, item);
    }

    fn visit_expr_path(&mut self, expression: &'ast syn::ExprPath) {
        self.record_projector_path(&expression.path);
        syn::visit::visit_expr_path(self, expression);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        let mut targets = Vec::new();
        collect_use_targets(&item.tree, Vec::new(), &mut targets);
        for target in targets {
            let path = syn::Path {
                leading_colon: item.leading_colon,
                segments: target
                    .into_iter()
                    .map(|ident| {
                        syn::PathSegment::from(syn::Ident::new(&ident, item.use_token.span))
                    })
                    .collect(),
            };
            self.record_projector_path(&path);
        }
        syn::visit::visit_item_use(self, item);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if matches!(
            call.method.to_string().as_str(),
            "register_source" | "register_carrier_grammar" | "accept_registered_source"
        ) {
            self.calls.push(call.method.to_string());
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}

fn item_attrs(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(item) => &item.attrs,
        syn::Item::Enum(item) => &item.attrs,
        syn::Item::ExternCrate(item) => &item.attrs,
        syn::Item::Fn(item) => &item.attrs,
        syn::Item::ForeignMod(item) => &item.attrs,
        syn::Item::Impl(item) => &item.attrs,
        syn::Item::Macro(item) => &item.attrs,
        syn::Item::Mod(item) => &item.attrs,
        syn::Item::Static(item) => &item.attrs,
        syn::Item::Struct(item) => &item.attrs,
        syn::Item::Trait(item) => &item.attrs,
        syn::Item::TraitAlias(item) => &item.attrs,
        syn::Item::Type(item) => &item.attrs,
        syn::Item::Union(item) => &item.attrs,
        syn::Item::Use(item) => &item.attrs,
        _ => &[],
    }
}

fn cfg_possibilities(meta: &syn::Meta) -> (bool, bool) {
    match meta {
        syn::Meta::Path(path) if path.is_ident("test") => (false, true),
        syn::Meta::List(list)
            if list.path.is_ident("all")
                || list.path.is_ident("any")
                || list.path.is_ident("not") =>
        {
            let Ok(nested) = list.parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            ) else {
                return (true, true);
            };
            let values = nested.iter().map(cfg_possibilities).collect::<Vec<_>>();
            if list.path.is_ident("all") {
                (
                    values.iter().all(|(can_be_true, _)| *can_be_true),
                    values.iter().any(|(_, can_be_false)| *can_be_false),
                )
            } else if list.path.is_ident("any") {
                (
                    values.iter().any(|(can_be_true, _)| *can_be_true),
                    values.iter().all(|(_, can_be_false)| *can_be_false),
                )
            } else if let [value] = values.as_slice() {
                (value.1, value.0)
            } else {
                (true, true)
            }
        }
        _ => (true, true),
    }
}

fn item_is_test_only(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("test") {
            return true;
        }
        attr.path().is_ident("cfg")
            && attr
                .parse_args::<syn::Meta>()
                .is_ok_and(|meta| !cfg_possibilities(&meta).0)
    })
}

fn collect_use_targets(tree: &syn::UseTree, prefix: Vec<String>, output: &mut Vec<Vec<String>>) {
    match tree {
        syn::UseTree::Path(path) => {
            let mut next = prefix;
            next.push(path.ident.to_string());
            collect_use_targets(&path.tree, next, output);
        }
        syn::UseTree::Name(name) => {
            let mut target = prefix;
            target.push(name.ident.to_string());
            output.push(target);
        }
        syn::UseTree::Rename(rename) => {
            let mut target = prefix;
            target.push(rename.ident.to_string());
            output.push(target);
        }
        syn::UseTree::Group(group) => {
            for tree in &group.items {
                collect_use_targets(tree, prefix.clone(), output);
            }
        }
        syn::UseTree::Glob(_) => {}
    }
}

#[test]
fn capability_guard_rejects_aliased_indirect_projector_calls() {
    let syntax = syn::parse_file(
        r#"
        use crate::framework_common::registered_carrier_projection::project_registered_carrier as production_projector;

        fn planted(compiler: &Compiler, accepted: &Accepted) {
            let indirect = production_projector;
            let _ = indirect(compiler, accepted);
        }
        "#,
    )
    .expect("plant parses");
    let mut calls = CapabilityCallVisitor::default();
    calls.visit_file(&syntax);
    assert!(
        !calls.calls.is_empty(),
        "an aliased function pointer must retain the projector's canonical identity"
    );
}

#[test]
fn capability_guard_rejects_cfg_not_test_production_calls() {
    let syntax = syn::parse_file(
        r#"
        #[cfg(not(test))]
        fn planted_not_test(compiler: &Compiler, accepted: &Accepted) {
            let _ = crate::framework_common::registered_carrier_projection::project_registered_carrier(compiler, accepted);
        }

        #[cfg(any(test, feature = "production"))]
        fn planted_any_can_be_production(compiler: &Compiler, accepted: &Accepted) {
            let _ = crate::framework_common::registered_carrier_projection::project_registered_carrier(compiler, accepted);
        }

        #[cfg(test)]
        fn allowed_test_only(compiler: &Compiler, accepted: &Accepted) {
            let _ = crate::framework_common::registered_carrier_projection::project_registered_carrier(compiler, accepted);
        }

        #[cfg(all(test, feature = "extra"))]
        fn allowed_all_requires_test(compiler: &Compiler, accepted: &Accepted) {
            let _ = crate::framework_common::registered_carrier_projection::project_registered_carrier(compiler, accepted);
        }

        #[cfg(not(not(test)))]
        fn allowed_double_negation_requires_test(compiler: &Compiler, accepted: &Accepted) {
            let _ = crate::framework_common::registered_carrier_projection::project_registered_carrier(compiler, accepted);
        }
        "#,
    )
    .expect("plant parses");
    let mut calls = CapabilityCallVisitor::default();
    calls.visit_file(&syntax);
    assert_eq!(
        calls.calls.len(),
        2,
        "cfg(not(test)) and cfg(any(test, ...)) remain production-capable; test-required forms do not"
    );
}

fn rust_sources(path: &Path, output: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(path).expect("read source tree") {
        let entry = entry.expect("source entry");
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, output);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs")
            && !path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with("_tests.rs"))
        {
            output.push(path);
        }
    }
}

fn rust_source_module_path(workspace: &Path, path: &Path) -> Vec<String> {
    let relative = path
        .strip_prefix(workspace.join("crates"))
        .expect("source belongs to workspace crates");
    let parts = relative
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();
    let mut module = vec![parts[0].to_string()];
    for directory in &parts[2..parts.len().saturating_sub(1)] {
        module.push((*directory).to_string());
    }
    let file = Path::new(parts.last().expect("Rust source file"));
    let stem = file
        .file_stem()
        .and_then(|stem| stem.to_str())
        .expect("stem");
    if !matches!(stem, "lib" | "main" | "mod") {
        module.push(stem.to_string());
    }
    module
}

#[test]
fn registered_carrier_projection_is_capability_only() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root");
    let mut sources = Vec::new();
    for entry in fs::read_dir(workspace.join("crates")).expect("workspace crates") {
        let source = entry.expect("crate entry").path().join("src");
        if source.is_dir() {
            rust_sources(&source, &mut sources);
        }
    }
    for path in sources {
        let source = fs::read_to_string(&path).expect("Rust source");
        let syntax = syn::parse_file(&source).unwrap_or_else(|error| {
            panic!(
                "{} must parse for the capability guard: {error}",
                path.display()
            )
        });
        let mut production_calls =
            CapabilityCallVisitor::for_module(rust_source_module_path(workspace, &path));
        production_calls.visit_file(&syntax);
        assert!(
            production_calls.calls.is_empty(),
            "registered mint/accept/projector calls escaped tests in {}: {:?}",
            path.display(),
            production_calls.calls
        );
    }

    let projector_source = include_str!("registered_carrier_projection.rs");
    let projector = syn::parse_file(projector_source).expect("projector source parses");
    let functions = projector
        .items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Fn(function) if function.sig.ident == "project_registered_carrier" => {
                Some(function)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(functions.len(), 1, "exactly one registered projector");
    assert!(matches!(functions[0].vis, syn::Visibility::Inherited));
    let signature = functions[0].sig.to_token_stream().to_string();
    assert!(signature.contains("& dyn CarrierCompiler"));
    assert!(signature.contains("& AcceptedRegisteredCarrierSource"));
    assert!(!signature.contains("str"));
    assert!(!signature.contains("FrameworkArtifactId"));

    let bundle = projector
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Struct(item) if item.ident == "RegisteredCarrierProjection" => Some(item),
            _ => None,
        })
        .expect("projection bundle");
    assert!(bundle
        .fields
        .iter()
        .all(|field| matches!(field.vis, syn::Visibility::Inherited)));

    let suite = syn::parse_file(include_str!("registered_carrier_projection_tests.rs"))
        .expect("conformance suite parses");
    let mut suite_calls = CapabilityCallVisitor::default();
    syn::visit::visit_file(&mut suite_calls, &suite);
    for required in [
        "register_source",
        "register_carrier_grammar",
        "accept_registered_source",
    ] {
        assert!(
            suite_calls.calls.iter().any(|call| call == required),
            "suite must invoke {required}"
        );
    }
}
