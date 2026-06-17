//! Parser round-trip + matrix-coverage tests for the Svelte byte parser.
//!
//! Every SUPPORTED row's construct produces the expected AST node; every
//! OUT-OF-SCOPE row parses WITHOUT crash (the matrix's parse-without-crash
//! contract). A row's SUPPORTED/OUT-OF-SCOPE status is a PROJECTOR concern — the
//! parser accepts every current-docs construct uniformly. The discriminating
//! diagnostic assertions for the OUT-OF-SCOPE rows live in the session-side
//! resolver tests; here we only prove parse-without-crash + the AST shape.

use super::template_ast::*;
use super::tokenizer::parse_svelte;

/// Parse a Svelte source and assert it produced no fatal diagnostics (a clean
/// matrix-row fixture should never trip an `unterminated-*` recovery).
fn parse_clean(source: &str) -> ParsedSvelte {
    let parsed = parse_svelte(source);
    assert!(
        parsed
            .diagnostics
            .iter()
            .all(|d| !d.code.starts_with("unterminated")),
        "clean fixture produced unterminated diagnostics: {:?}",
        parsed.diagnostics
    );
    parsed
}

/// Recursively collect every template element by tag name.
fn elements<'a>(nodes: &'a [SvelteNode], out: &mut Vec<&'a SvelteElement>) {
    for node in nodes {
        match node {
            SvelteNode::Element(el) => {
                out.push(el);
                elements(&el.children, out);
            }
            SvelteNode::Block(block) => {
                elements(&block.children, out);
                for clause in &block.clauses {
                    elements(&clause.children, out);
                }
            }
            _ => {}
        }
    }
}

/// Recursively collect every block.
fn blocks<'a>(nodes: &'a [SvelteNode], out: &mut Vec<&'a SvelteBlock>) {
    for node in nodes {
        match node {
            SvelteNode::Element(el) => blocks(&el.children, out),
            SvelteNode::Block(block) => {
                out.push(block);
                blocks(&block.children, out);
                for clause in &block.clauses {
                    blocks(&clause.children, out);
                }
            }
            _ => {}
        }
    }
}

/// Recursively collect every standalone tag.
fn tags<'a>(nodes: &'a [SvelteNode], out: &mut Vec<&'a SvelteTag>) {
    for node in nodes {
        match node {
            SvelteNode::Tag(tag) => out.push(tag),
            SvelteNode::Element(el) => tags(&el.children, out),
            SvelteNode::Block(block) => {
                tags(&block.children, out);
                for clause in &block.clauses {
                    tags(&clause.children, out);
                }
            }
            _ => {}
        }
    }
}

// ── Scripts ────────────────────────────────────────────────────────────

#[test]
fn instance_and_module_scripts_are_separated() {
    let src =
        "<script module>export const x = 1;</script>\n<script lang=\"ts\">let a = 1;</script>";
    let p = parse_clean(src);
    let module = p.module_script.expect("module script present");
    assert!(module.is_module);
    assert_eq!(
        src[module.content.unwrap().start as usize..module.content.unwrap().end as usize].trim(),
        "export const x = 1;"
    );
    let instance = p.instance_script.expect("instance script present");
    assert!(!instance.is_module);
    assert_eq!(instance.lang.as_deref(), Some("ts"));
}

#[test]
fn runes_in_instance_script_are_opaque_to_the_parser() {
    // The parser records the script content span verbatim — it does NOT parse
    // runes (`$props`/`$state`/…); those are a projector/script-fact concern.
    let src =
        "<script lang=\"ts\">\n  let { name } = $props();\n  let count = $state(0);\n</script>";
    let p = parse_clean(src);
    let content = p.instance_content().expect("instance content");
    let body = &src[content.start as usize..content.end as usize];
    assert!(body.contains("$props()"));
    assert!(body.contains("$state(0)"));
}

// ── Interpolation + elements ───────────────────────────────────────────

#[test]
fn interpolation_records_inner_expression_span() {
    let src = "<p>{count + 1}</p>";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let interps: Vec<&SvelteNode> = els
        .iter()
        .flat_map(|e| e.children.iter())
        .filter(|n| matches!(n, SvelteNode::Interpolation(_)))
        .collect();
    assert_eq!(interps.len(), 1);
    if let SvelteNode::Interpolation(span) = interps[0] {
        assert_eq!(&src[span.start as usize..span.end as usize], "count + 1");
    }
}

#[test]
fn brace_in_string_literal_does_not_close_interpolation_early() {
    let src = "<p>{ obj['a}b'] }</p>";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let interp = els[0]
        .children
        .iter()
        .find_map(|n| match n {
            SvelteNode::Interpolation(s) => Some(*s),
            _ => None,
        })
        .expect("interpolation");
    assert_eq!(
        src[interp.start as usize..interp.end as usize].trim(),
        "obj['a}b']"
    );
}

#[test]
fn brace_in_comment_does_not_close_interpolation_early() {
    let src = "<p>{ /* } */ value }</p>";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let interp = els[0]
        .children
        .iter()
        .find_map(|n| match n {
            SvelteNode::Interpolation(s) => Some(*s),
            _ => None,
        })
        .expect("interpolation");
    assert_eq!(
        src[interp.start as usize..interp.end as usize].trim(),
        "/* } */ value"
    );
}

#[test]
fn brace_in_regex_does_not_close_interpolation_early() {
    let src = "<p>{ /[}]/.test(x) }</p>";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let interp = els[0]
        .children
        .iter()
        .find_map(|n| match n {
            SvelteNode::Interpolation(s) => Some(*s),
            _ => None,
        })
        .expect("interpolation");
    assert_eq!(
        src[interp.start as usize..interp.end as usize].trim(),
        "/[}]/.test(x)"
    );
}

#[test]
fn division_after_value_is_not_treated_as_regex() {
    // `a / b` and postfix `a++ / b` are DIVISION, not a regex — the brace must
    // close at the real `}`. DISCRIMINATING: a too-eager regex heuristic would
    // skip from the `/` to a later `/` and mis-span (or run past the brace).
    for src in [
        "<p>{ total / count }</p>",
        "<p>{ a / b / c }</p>",
        "<p>{ items.length / 2 }</p>",
    ] {
        let p = parse_clean(src);
        let mut els = Vec::new();
        elements(&p.template, &mut els);
        let interp = els[0]
            .children
            .iter()
            .find_map(|n| match n {
                SvelteNode::Interpolation(s) => Some(*s),
                _ => None,
            })
            .unwrap_or_else(|| panic!("interpolation for {src:?}"));
        let inner = &src[interp.start as usize..interp.end as usize];
        // The whole division expression is captured (the brace closed at `}`).
        assert!(
            inner.contains('/') && inner.trim_end().ends_with(|c: char| c != '}'),
            "division expression captured intact for {src:?}, got {inner:?}"
        );
    }
}

#[test]
fn component_vs_intrinsic_classification() {
    let src = "<div><MyComp /><Foo.Bar /></div>";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let div = els.iter().find(|e| e.name == "div").unwrap();
    assert_eq!(div.kind, SvelteElementKind::Intrinsic);
    assert!(els
        .iter()
        .any(|e| e.name == "MyComp" && e.kind == SvelteElementKind::Component));
    assert!(els
        .iter()
        .any(|e| e.name == "Foo.Bar" && e.kind == SvelteElementKind::Component));
}

// ── Events + attributes (SUPPORTED) ────────────────────────────────────

#[test]
fn svelte5_event_attribute_is_a_plain_lowercase_attribute() {
    // The PRIMARY event path is a plain lowercase `onclick={...}` attribute.
    let src = "<button onclick={handle}>x</button>";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let btn = els.iter().find(|e| e.name == "button").unwrap();
    let onclick = btn.attributes.iter().find_map(|a| match &a.kind {
        SvelteAttributeKind::Plain { name, value, .. } if name == "onclick" => Some(value.clone()),
        _ => None,
    });
    assert!(onclick.is_some(), "onclick parses as a plain attribute");
    assert!(matches!(
        onclick.unwrap(),
        Some(SvelteAttributeValue::Expression(_))
    ));
}

#[test]
fn shorthand_attribute_records_name_as_value() {
    let src = "<input {value} />";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let input = els.iter().find(|e| e.name == "input").unwrap();
    assert!(input.attributes.iter().any(|a| matches!(
        &a.kind,
        SvelteAttributeKind::Plain { name, .. } if name == "value"
    )));
}

#[test]
fn spread_attribute_parses() {
    let src = "<div {...rest} />";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let div = els.iter().find(|e| e.name == "div").unwrap();
    assert!(div
        .attributes
        .iter()
        .any(|a| matches!(a.kind, SvelteAttributeKind::Spread(_))));
}

#[test]
fn class_directive_parses() {
    let src = "<div class:active={isActive}>x</div>";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let div = els.iter().find(|e| e.name == "div").unwrap();
    let dir = div.attributes.iter().find_map(|a| match &a.kind {
        SvelteAttributeKind::Directive(d) if d.kind == SvelteDirectiveKind::Class => Some(d),
        _ => None,
    });
    assert_eq!(dir.unwrap().local, "active");
}

#[test]
fn css_custom_property_parses_as_plain_attribute() {
    // `--name={expr}` parses as a plain attribute (NOT a directive).
    let src = "<Comp --accent={color} --size=\"4px\" />";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let comp = els.iter().find(|e| e.name == "Comp").unwrap();
    assert!(comp.attributes.iter().any(|a| matches!(
        &a.kind, SvelteAttributeKind::Plain { name, .. } if name == "--accent"
    )));
    assert!(comp.attributes.iter().any(|a| matches!(
        &a.kind, SvelteAttributeKind::Plain { name, .. } if name == "--size"
    )));
}

// ── Bindings (SUPPORTED + OUT-OF-SCOPE) ────────────────────────────────

#[test]
fn bind_value_directive_parses() {
    let src = "<input bind:value={name} />";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let input = els.iter().find(|e| e.name == "input").unwrap();
    assert!(input.attributes.iter().any(|a| matches!(
        &a.kind, SvelteAttributeKind::Directive(d) if d.kind == SvelteDirectiveKind::Bind && d.local == "value"
    )));
}

#[test]
fn function_binding_two_expression_form_parses_without_crash() {
    // OUT-OF-SCOPE v1: `bind:x={get, set}` — the two-expression form must parse.
    let src = "<input bind:value={getValue, setValue} />";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let input = els.iter().find(|e| e.name == "input").unwrap();
    let bind = input.attributes.iter().find_map(|a| match &a.kind {
        SvelteAttributeKind::Directive(d) if d.kind == SvelteDirectiveKind::Bind => Some(d),
        _ => None,
    });
    let bind = bind.expect("bind directive");
    // The value span carries BOTH expressions (the projector splits them).
    match &bind.value {
        Some(SvelteAttributeValue::Expression(span)) => {
            assert_eq!(
                src[span.start as usize..span.end as usize].trim(),
                "getValue, setValue"
            );
        }
        other => panic!("expected expression value, got {other:?}"),
    }
}

// ── Directives (OUT-OF-SCOPE) ──────────────────────────────────────────

#[test]
fn use_transition_animate_and_legacy_on_parse_without_crash() {
    let src = "<div use:tooltip={opts} transition:fade={{ duration: 300 }} in:fly out:fade animate:flip on:click|preventDefault={handle}>x</div>";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let div = els.iter().find(|e| e.name == "div").unwrap();
    let kinds: Vec<SvelteDirectiveKind> = div
        .attributes
        .iter()
        .filter_map(|a| match &a.kind {
            SvelteAttributeKind::Directive(d) => Some(d.kind),
            _ => None,
        })
        .collect();
    assert!(kinds.contains(&SvelteDirectiveKind::Use));
    assert!(kinds.contains(&SvelteDirectiveKind::Transition));
    assert!(kinds.contains(&SvelteDirectiveKind::In));
    assert!(kinds.contains(&SvelteDirectiveKind::Out));
    assert!(kinds.contains(&SvelteDirectiveKind::Animate));
    assert!(kinds.contains(&SvelteDirectiveKind::On));
    // The legacy `on:click|preventDefault` records the modifier.
    let on = div
        .attributes
        .iter()
        .find_map(|a| match &a.kind {
            SvelteAttributeKind::Directive(d) if d.kind == SvelteDirectiveKind::On => Some(d),
            _ => None,
        })
        .unwrap();
    assert_eq!(on.local, "click");
    assert_eq!(on.modifiers, vec!["preventDefault".to_string()]);
}

#[test]
fn let_slot_prop_directive_is_classified_as_the_let_kind() {
    // The `let:` slot-prop directive is classified STRUCTURALLY as
    // `SvelteDirectiveKind::Let` (the parser is the directive-prefix authority) —
    // NOT lumped into `Unknown` for a downstream string-sniff. Both the aliased
    // form (`let:item={alias}`) and the shorthand (`let:item`) classify as `Let`.
    let src = "<C let:item={alias} let:row>x</C>";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let c = els.iter().find(|e| e.name == "C").unwrap();
    let lets: Vec<&SvelteDirective> = c
        .attributes
        .iter()
        .filter_map(|a| match &a.kind {
            SvelteAttributeKind::Directive(d) if d.kind == SvelteDirectiveKind::Let => Some(d),
            _ => None,
        })
        .collect();
    assert_eq!(
        lets.len(),
        2,
        "both `let:item={{alias}}` and `let:row` classify as `Let`: {:?}",
        c.attributes
    );
    // DISCRIMINATING: neither `let:` directive is classified as `Unknown`.
    let unknowns = c.attributes.iter().filter(|a| {
        matches!(&a.kind, SvelteAttributeKind::Directive(d) if d.kind == SvelteDirectiveKind::Unknown)
    });
    assert_eq!(
        unknowns.count(),
        0,
        "a `let:` directive must NOT classify as `Unknown` (the old string-sniff \
         premise): {:?}",
        c.attributes
    );
    assert_eq!(lets[0].local, "item");
    assert_eq!(lets[1].local, "row");
}

#[test]
fn style_directive_with_important_modifier_parses() {
    let src = "<div style:color|important={c}>x</div>";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let dir = els[0].attributes.iter().find_map(|a| match &a.kind {
        SvelteAttributeKind::Directive(d) if d.kind == SvelteDirectiveKind::Style => Some(d),
        _ => None,
    });
    let dir = dir.unwrap();
    assert_eq!(dir.local, "color");
    assert_eq!(dir.modifiers, vec!["important".to_string()]);
}

// ── Blocks ─────────────────────────────────────────────────────────────

#[test]
fn if_else_if_else_block_parses() {
    let src = "{#if a}A{:else if b}B{:else}C{/if}";
    let p = parse_clean(src);
    let mut bs = Vec::new();
    blocks(&p.template, &mut bs);
    let block = bs
        .iter()
        .find(|b| matches!(b.kind, SvelteBlockKind::If))
        .unwrap();
    assert_eq!(
        src[block.head_expr.unwrap().start as usize..block.head_expr.unwrap().end as usize],
        *"a"
    );
    let kinds: Vec<SvelteClauseKind> = block.clauses.iter().map(|c| c.kind).collect();
    assert_eq!(
        kinds,
        vec![SvelteClauseKind::ElseIf, SvelteClauseKind::Else]
    );
}

#[test]
fn each_keyed_with_index_and_key_parses() {
    let src = "{#each items as item, i (item.id)}<li>{item.name}</li>{/each}";
    let p = parse_clean(src);
    let mut bs = Vec::new();
    blocks(&p.template, &mut bs);
    let block = &bs[0];
    let SvelteBlockKind::Each { item, index, key } = &block.kind else {
        panic!("expected each block");
    };
    assert_eq!(
        src[block.head_expr.unwrap().start as usize..block.head_expr.unwrap().end as usize].trim(),
        "items"
    );
    assert_eq!(
        src[item.unwrap().start as usize..item.unwrap().end as usize].trim(),
        "item"
    );
    assert_eq!(
        src[index.unwrap().start as usize..index.unwrap().end as usize].trim(),
        "i"
    );
    assert_eq!(
        src[key.unwrap().start as usize..key.unwrap().end as usize].trim(),
        "item.id"
    );
}

#[test]
fn each_without_as_item_parses() {
    // SUPPORTED: `{#each {length: n}}` — no `as` item binding.
    let src = "{#each { length: count }}<div />{/each}";
    let p = parse_clean(src);
    let mut bs = Vec::new();
    blocks(&p.template, &mut bs);
    let block = &bs[0];
    let SvelteBlockKind::Each { item, .. } = &block.kind else {
        panic!("expected each block");
    };
    assert!(item.is_none(), "the no-item form has no `as` binding");
    assert_eq!(
        src[block.head_expr.unwrap().start as usize..block.head_expr.unwrap().end as usize].trim(),
        "{ length: count }"
    );
}

#[test]
fn await_then_catch_parses() {
    let src = "{#await promise}loading{:then value}{value}{:catch err}{err}{/await}";
    let p = parse_clean(src);
    let mut bs = Vec::new();
    blocks(&p.template, &mut bs);
    let block = &bs[0];
    let SvelteBlockKind::Await {
        then_binding,
        catch_binding,
    } = &block.kind
    else {
        panic!("expected await block");
    };
    assert_eq!(
        src[then_binding.unwrap().start as usize..then_binding.unwrap().end as usize].trim(),
        "value"
    );
    assert_eq!(
        src[catch_binding.unwrap().start as usize..catch_binding.unwrap().end as usize].trim(),
        "err"
    );
}

#[test]
fn key_block_parses() {
    let src = "{#key value}<Comp />{/key}";
    let p = parse_clean(src);
    let mut bs = Vec::new();
    blocks(&p.template, &mut bs);
    assert!(bs.iter().any(|b| matches!(b.kind, SvelteBlockKind::Key)));
}

#[test]
fn snippet_block_records_name_and_params() {
    let src = "{#snippet row(item, index)}<td>{item}</td>{/snippet}";
    let p = parse_clean(src);
    let mut bs = Vec::new();
    blocks(&p.template, &mut bs);
    let block = &bs[0];
    let SvelteBlockKind::Snippet {
        name_text, params, ..
    } = &block.kind
    else {
        panic!("expected snippet block");
    };
    assert_eq!(name_text, "row");
    assert_eq!(
        src[params.unwrap().start as usize..params.unwrap().end as usize].trim(),
        "item, index"
    );
}

// ── Tags ───────────────────────────────────────────────────────────────

#[test]
fn render_html_attach_debug_tags_parse() {
    let src = "<div>{@render row(item)}{@html content}{@attach myAttach}{@debug a, b}</div>";
    let p = parse_clean(src);
    let mut ts = Vec::new();
    tags(&p.template, &mut ts);
    let kinds: Vec<SvelteTagKind> = ts.iter().map(|t| t.kind).collect();
    assert!(kinds.contains(&SvelteTagKind::Render));
    assert!(kinds.contains(&SvelteTagKind::Html));
    assert!(kinds.contains(&SvelteTagKind::Attach));
    assert!(kinds.contains(&SvelteTagKind::Debug));
    let render = ts.iter().find(|t| t.kind == SvelteTagKind::Render).unwrap();
    assert_eq!(
        src[render.inner.start as usize..render.inner.end as usize].trim(),
        "row(item)"
    );
}

#[test]
fn declaration_tags_and_legacy_const_parse() {
    // 5.56 declaration tags `{const}` / `{let}` + legacy `{@const}`.
    let src = "<ul>{#each items as item}{const doubled = item * 2}{let label = item}{@const legacy = item}<li>{doubled}</li>{/each}</ul>";
    let p = parse_clean(src);
    let mut ts = Vec::new();
    tags(&p.template, &mut ts);
    let kinds: Vec<SvelteTagKind> = ts.iter().map(|t| t.kind).collect();
    assert!(
        kinds.contains(&SvelteTagKind::Const),
        "declaration {{const}}"
    );
    assert!(kinds.contains(&SvelteTagKind::Let), "declaration {{let}}");
    assert!(
        kinds.contains(&SvelteTagKind::LegacyConst),
        "legacy {{@const}}"
    );
    let decl_const = ts.iter().find(|t| t.kind == SvelteTagKind::Const).unwrap();
    assert_eq!(
        src[decl_const.inner.start as usize..decl_const.inner.end as usize].trim(),
        "doubled = item * 2"
    );
}

// ── Special elements ───────────────────────────────────────────────────

#[test]
fn supported_special_elements_parse() {
    let src = "<svelte:head><title>X</title></svelte:head><svelte:window /><svelte:element this={tag}>x</svelte:element><svelte:boundary><p>ok</p></svelte:boundary><svelte:options runes={true} />";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let specials: Vec<SvelteSpecialKind> = els
        .iter()
        .filter_map(|e| match e.kind {
            SvelteElementKind::Special(k) => Some(k),
            _ => None,
        })
        .collect();
    assert!(specials.contains(&SvelteSpecialKind::Head));
    assert!(specials.contains(&SvelteSpecialKind::Window));
    assert!(specials.contains(&SvelteSpecialKind::Element));
    assert!(specials.contains(&SvelteSpecialKind::Boundary));
    assert!(specials.contains(&SvelteSpecialKind::Options));
}

#[test]
fn dynamic_self_and_fragment_special_elements_parse() {
    let src = "<svelte:component this={Comp} /><svelte:self /><svelte:fragment slot=\"x\">y</svelte:fragment>";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let specials: Vec<SvelteSpecialKind> = els
        .iter()
        .filter_map(|e| match e.kind {
            SvelteElementKind::Special(k) => Some(k),
            _ => None,
        })
        .collect();
    assert!(specials.contains(&SvelteSpecialKind::Component));
    assert!(specials.contains(&SvelteSpecialKind::SelfRef));
    assert!(specials.contains(&SvelteSpecialKind::Fragment));
}

// ── Styling ────────────────────────────────────────────────────────────

#[test]
fn component_style_block_is_opaque_recorded_span() {
    let src =
        "<style>.foo { color: red; } :global(.bar) { color: blue; }</style>\n<div class=\"foo\" />";
    let p = parse_clean(src);
    assert_eq!(p.styles.len(), 1);
    let style = &p.styles[0];
    let content = style.content.unwrap();
    assert!(src[content.start as usize..content.end as usize].contains(":global(.bar)"));
}

#[test]
fn nested_style_element_inside_template_parses_without_crash() {
    let src = "<div><style>.x{}</style></div>";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    assert!(els
        .iter()
        .any(|e| matches!(e.kind, SvelteElementKind::NestedStyle)));
}

// ── Await expressions (OUT-OF-SCOPE) + comments-in-tags ──────────

#[test]
fn await_as_ordinary_expression_position_parses() {
    // OUT-OF-SCOPE v1 (async): markup `{await ...}` is an ordinary template
    // expression — it must parse without crash.
    let src = "<p>{await fetchUser()}</p>";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let interp = els[0].children.iter().find_map(|n| match n {
        SvelteNode::Interpolation(s) => Some(*s),
        _ => None,
    });
    assert_eq!(
        src[interp.unwrap().start as usize..interp.unwrap().end as usize].trim(),
        "await fetchUser()"
    );
}

#[test]
fn comments_in_element_open_tag_parse_without_crash() {
    // Comments inside element open tags — the carrier
    // parse-without-crash contract. DISCRIMINATING: the comment is INSIDE the
    // open tag, between attributes; the attributes AFTER it must still parse (a
    // naive scan would stop at the comment's `>` and lose `onclick`).
    let src = "<button class=\"x\" <!-- the click handler --> onclick={handle} disabled>x</button>";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let btn = els
        .iter()
        .find(|e| e.name == "button")
        .expect("button parses");
    assert!(
        btn.attributes.iter().any(|a| matches!(
            &a.kind, SvelteAttributeKind::Plain { name, .. } if name == "onclick"
        )),
        "the attribute after an in-tag comment must still parse: {:?}",
        btn.attributes
    );
    assert!(
        btn.attributes.iter().any(|a| matches!(
            &a.kind, SvelteAttributeKind::Plain { name, .. } if name == "disabled"
        )),
        "the trailing attribute must parse too"
    );
}

// ── Robustness: the full kitchen-sink fixture must never panic ──────────

#[test]
fn full_kitchen_sink_parses_without_panic() {
    let src = r#"<script module>export const meta = 1;</script>
<script lang="ts">
  let { items, title = "untitled" }: { items: string[]; title?: string } = $props();
  let count = $state(0);
  let doubled = $derived(count * 2);
  $effect(() => console.log(count));
</script>

<svelte:options runes={true} />
<svelte:head><title>{title}</title></svelte:head>

<h1 class:big={count > 10}>{title}</h1>
<button onclick={() => count++} use:tooltip transition:fade>+</button>

{#if count > 0}
  {#each items as item, i (item)}
    {@const label = `${i}: ${item}`}
    <li bind:this={el} style:--x="1px">{label}</li>
  {:else}
    <li>empty</li>
  {/each}
{:else}
  <p>none</p>
{/if}

{#await load()}
  loading
{:then value}
  {@html value}
{:catch e}
  {e.message}
{/await}

{#snippet footer(x)}
  <footer>{x}</footer>
{/snippet}
{@render footer("hi")}

<svelte:component this={Dynamic} />
<style>.big { font-weight: bold; }</style>
"#;
    // Must not panic and must record both scripts + style.
    let p = parse_svelte(src);
    assert!(p.instance_script.is_some());
    assert!(p.module_script.is_some());
    assert_eq!(p.styles.len(), 1);
}

#[test]
fn element_records_the_matching_close_tag_span() {
    // The parser is the close-tag authority — it records the matching `</name>`
    // close-tag span (start at `<`, end past `>`) on the constructed element.
    let src = "<div>x</div>";
    let p = parse_clean(src);
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    let div = els.iter().find(|e| e.name == "div").expect("div");
    let span = div
        .close_span
        .expect("close span recorded for a closed element");
    assert_eq!(&src[span.start as usize..span.end as usize], "</div>");
}

#[test]
fn self_closing_and_void_elements_have_no_close_span() {
    let p = parse_clean("<br /><img src=\"a.png\">");
    let mut els = Vec::new();
    elements(&p.template, &mut els);
    for el in &els {
        assert!(
            el.close_span.is_none(),
            "self-closing / void `{}` has no close span",
            el.name
        );
    }
}

#[test]
fn close_span_is_depth_aware_for_nested_same_name_elements() {
    // A nested same-name `<div>` must NOT steal the outer element's close — the
    // recorded span is the close that brings depth back to zero (the LAST
    // `</div>`).
    let src = "<div><div>inner</div></div>";
    let p = parse_clean(src);
    let outer = match &p.template[0] {
        SvelteNode::Element(el) => el,
        other => panic!("expected an element, got {other:?}"),
    };
    let span = outer.close_span.expect("outer close span");
    // The outer close is the FINAL `</div>` in the source (offset of the last
    // occurrence), not the inner one.
    let last_close = src.rfind("</div>").unwrap() as u32;
    assert_eq!(
        span.start, last_close,
        "outer close is the depth-zero close"
    );
    assert_eq!(&src[span.start as usize..span.end as usize], "</div>");
    // The inner element's close is the FIRST `</div>`.
    let inner = match &outer.children[0] {
        SvelteNode::Element(el) => el,
        other => panic!("expected inner element, got {other:?}"),
    };
    let inner_span = inner.close_span.expect("inner close span");
    assert_eq!(inner_span.start, src.find("</div>").unwrap() as u32);
}

#[test]
fn close_span_ignores_a_close_tag_inside_a_descendant_string_literal() {
    // The string/brace-aware child walk never sees the `</div>` inside a child
    // interpolation's string literal — the recorded close span is the REAL close
    // tag after the children.
    let src = "<div>{\"a </div> b\"}<span>x</span></div>";
    let p = parse_clean(src);
    let div = match &p.template[0] {
        SvelteNode::Element(el) => el,
        other => panic!("expected an element, got {other:?}"),
    };
    let span = div.close_span.expect("close span");
    // The REAL close is the FINAL `</div>` (after `<span>x</span>`), NOT the one
    // inside the string literal.
    assert_eq!(span.start, src.rfind("</div>").unwrap() as u32);
    assert_eq!(&src[span.start as usize..span.end as usize], "</div>");
}
