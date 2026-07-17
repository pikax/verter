use super::*;
use crate::code_transform::CodeTransform;

/// Read a `.vue` file from an OPT-IN external corpus, located via an
/// environment variable rather than a hardcoded machine path. Returns
/// `None` (so the caller skips) ONLY when the corpus env var is UNSET —
/// these tests exercise real third-party SFCs that are not vendored into
/// the repo, so they are off by default and run only when a developer
/// points the corpus env var at a local checkout. When the env var IS set
/// but the referenced file is missing/unreadable, this PANICS rather than
/// skipping: an explicitly-configured-but-broken corpus root is a real
/// error, not a silent green pass. NOTE: these remain external-corpus
/// tests; full testing-hermeticity (vendored fixtures or a dedicated
/// `external-corpus` feature gate excluding them from the default run) is
/// tracked separately and intentionally NOT addressed here.
fn read_external_corpus_vue(corpus_root_env: &str, relative_path: &str) -> Option<String> {
    let root = match std::env::var(corpus_root_env) {
        Ok(r) => r,
        // Genuinely unset → skip (corpus off by default).
        Err(std::env::VarError::NotPresent) => return None,
        // Set but not valid Unicode → the corpus IS configured, just
        // broken; fail loud rather than silently skip (same posture as a
        // set-but-unreadable file below).
        Err(std::env::VarError::NotUnicode(v)) => panic!(
            "external corpus env ${corpus_root_env} is set but not valid \
             Unicode ({v:?}); fix the value or unset ${corpus_root_env} to \
             skip these tests"
        ),
    };
    let full = std::path::Path::new(&root).join(relative_path);
    match std::fs::read_to_string(&full) {
        Ok(s) => Some(s),
        Err(e) => panic!(
            "external corpus env ${corpus_root_env} is set to `{root}`, but \
             `{}` could not be read ({e}); fix the corpus root or unset \
             ${corpus_root_env} to skip these tests",
            full.display()
        ),
    }
}

/// Helper: compile a full SFC with TSX template generation.
/// Returns the template portion of the TSX output.
fn gen_tsx_template(source: &str) -> String {
    let alloc = Allocator::new();
    let bytes = source.as_bytes();

    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });

    let template_ast = match syntax.take_template_ast() {
        Some(ast) => ast,
        None => return String::new(),
    };

    let source_type = oxc_span::SourceType::tsx();
    let oxc_ast = crate::template::oxc::parse_template_expressions(
        &template_ast,
        source,
        &alloc,
        source_type,
        true,
    );

    let mut tpl_ct = CodeTransform::new(source, &alloc);
    let mut out = CodeGenOutput::new(&alloc);
    let bindings = FxHashMap::default();
    let options = IdeTemplateOptions {
        self_name: "App",
        comments: true,
        is_jsx: false,
        strict_slots: false,
    };

    generate_ide_template(
        &template_ast,
        &oxc_ast,
        source,
        &mut out,
        &alloc,
        &bindings,
        &options,
        &TemplateComponentBindings::default(),
    );
    out.apply_to(&mut tpl_ct);

    let full = tpl_ct.build_string();

    let tpl_start = template_ast.root.tag_open.start as usize;
    let tpl_end = template_ast
        .root
        .tag_close
        .as_ref()
        .map(|tc| tc.end as usize)
        .unwrap_or(full.len());
    let suffix_len = source.len() - tpl_end;
    full[tpl_start..full.len() - suffix_len].to_string()
}

#[derive(Debug, Default)]
struct JsxElementBodyFacts {
    element_count: usize,
    non_empty_elements: Vec<String>,
    explicit_children_attributes: usize,
    definitely_invalid_attributes: usize,
    template_only_references: usize,
    hello_string_literals: usize,
    panel_open_name_offsets: Vec<u32>,
    panel_close_name_offsets: Vec<u32>,
}

/// Parse generated TSX and inspect the JSX AST rather than matching rendered
/// source text. Vue template bodies are slots, not React-style `children`
/// attributes, so the IDE carrier must leave every concrete JSX element empty
/// while retaining the body expressions in surrounding JSX fragments.
fn jsx_element_body_facts(code: &str) -> JsxElementBodyFacts {
    use oxc_ast::ast::{
        IdentifierReference, JSXAttributeItem, JSXAttributeName, JSXElement, JSXElementName,
        StringLiteral,
    };
    use oxc_ast_visit::{walk, Visit};

    struct Scanner {
        facts: JsxElementBodyFacts,
    }

    impl<'a> Visit<'a> for Scanner {
        fn visit_jsx_element(&mut self, element: &JSXElement<'a>) {
            self.facts.element_count += 1;
            if !element.children.is_empty() {
                let name = match &element.opening_element.name {
                    JSXElementName::Identifier(name) => name.name.to_string(),
                    JSXElementName::IdentifierReference(name) => name.name.to_string(),
                    JSXElementName::NamespacedName(_) => "<namespaced>".to_string(),
                    JSXElementName::MemberExpression(_) => "<member>".to_string(),
                    JSXElementName::ThisExpression(_) => "this".to_string(),
                };
                self.facts.non_empty_elements.push(name);
            }
            if matches!(
                &element.opening_element.name,
                JSXElementName::IdentifierReference(name) if name.name == "Panel"
            ) {
                let JSXElementName::IdentifierReference(name) = &element.opening_element.name
                else {
                    unreachable!();
                };
                self.facts.panel_open_name_offsets.push(name.span.start);
                if let Some(closing) = &element.closing_element {
                    let JSXElementName::IdentifierReference(name) = &closing.name else {
                        panic!("Panel closing tag must remain an identifier reference");
                    };
                    self.facts.panel_close_name_offsets.push(name.span.start);
                }
            }

            self.facts.explicit_children_attributes += element
                .opening_element
                .attributes
                .iter()
                .filter(|attribute| {
                    matches!(
                        attribute,
                        JSXAttributeItem::Attribute(attribute)
                            if matches!(
                                &attribute.name,
                                JSXAttributeName::Identifier(name) if name.name == "children"
                            )
                    )
                })
                .count();
            self.facts.definitely_invalid_attributes += element
                .opening_element
                .attributes
                .iter()
                .filter(|attribute| {
                    matches!(
                        attribute,
                        JSXAttributeItem::Attribute(attribute)
                            if matches!(
                                &attribute.name,
                                JSXAttributeName::Identifier(name)
                                    if name.name == "definitelyInvalid"
                            )
                    )
                })
                .count();

            walk::walk_jsx_element(self, element);
        }

        fn visit_identifier_reference(&mut self, reference: &IdentifierReference<'a>) {
            if reference.name == "templateOnly" {
                self.facts.template_only_references += 1;
            }
        }

        fn visit_string_literal(&mut self, literal: &StringLiteral<'a>) {
            if literal.value == "hello" {
                self.facts.hello_string_literals += 1;
            }
        }
    }

    let alloc = Allocator::default();
    let parsed = oxc_parser::Parser::new(&alloc, code, oxc_span::SourceType::tsx()).parse();
    assert!(
        !parsed.panicked && parsed.errors.is_empty(),
        "generated template must be valid TSX: {:?}\n{code}",
        parsed.errors
    );

    let mut scanner = Scanner {
        facts: JsxElementBodyFacts::default(),
    };
    scanner.visit_program(&parsed.program);
    scanner.facts
}

#[derive(Debug, PartialEq, Eq)]
struct JsxAttributeFact {
    name: String,
    start: u32,
}

fn jsx_attributes_for_element(code: &str, wanted_element: &str) -> Vec<JsxAttributeFact> {
    use oxc_ast::ast::{JSXAttributeItem, JSXAttributeName, JSXElement, JSXElementName};
    use oxc_ast_visit::{walk, Visit};

    struct Scanner<'wanted> {
        wanted_element: &'wanted str,
        attributes: Vec<JsxAttributeFact>,
    }

    impl<'a> Visit<'a> for Scanner<'_> {
        fn visit_jsx_element(&mut self, element: &JSXElement<'a>) {
            let is_wanted = match &element.opening_element.name {
                JSXElementName::Identifier(name) => name.name == self.wanted_element,
                JSXElementName::IdentifierReference(name) => name.name == self.wanted_element,
                _ => false,
            };
            if is_wanted {
                self.attributes
                    .extend(
                        element
                            .opening_element
                            .attributes
                            .iter()
                            .filter_map(|attribute| match attribute {
                                JSXAttributeItem::Attribute(attribute) => match &attribute.name {
                                    JSXAttributeName::Identifier(name) => Some(JsxAttributeFact {
                                        name: name.name.to_string(),
                                        start: name.span.start,
                                    }),
                                    JSXAttributeName::NamespacedName(name) => {
                                        Some(JsxAttributeFact {
                                            name: format!(
                                                "{}:{}",
                                                name.namespace.name, name.name.name
                                            ),
                                            start: name.span.start,
                                        })
                                    }
                                },
                                JSXAttributeItem::SpreadAttribute(_) => None,
                            }),
                    );
            }
            walk::walk_jsx_element(self, element);
        }
    }

    let alloc = Allocator::default();
    let parsed = oxc_parser::Parser::new(&alloc, code, oxc_span::SourceType::tsx()).parse();
    assert!(
        !parsed.panicked && parsed.errors.is_empty(),
        "generated template must be valid TSX: {:?}\n{code}",
        parsed.errors
    );
    let mut scanner = Scanner {
        wanted_element,
        attributes: Vec::new(),
    };
    scanner.visit_program(&parsed.program);
    scanner.attributes
}

#[test]
fn vue_slot_bodies_are_fragment_siblings_not_jsx_element_children() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><Panel children="explicit" definitelyInvalid="bad"><div class="card">{{ templateOnly }}</div></Panel></template>"#,
        &[
            ("Panel", BindingType::SetupConst),
            ("templateOnly", BindingType::SetupConst),
        ],
    );
    let facts = jsx_element_body_facts(&result);

    assert_eq!(
        facts.element_count, 2,
        "Panel and div must remain typed JSX elements"
    );
    assert!(
        facts.non_empty_elements.is_empty(),
        "Vue slot bodies must not become React JSX children: {facts:?}\n{result}"
    );
    assert_eq!(
        facts.explicit_children_attributes, 1,
        "an authored children prop must remain on the typed element"
    );
    assert_eq!(
        facts.definitely_invalid_attributes, 1,
        "an invalid authored prop must remain on the typed element for the TypeScript diagnostic"
    );
    assert_eq!(
        facts.template_only_references, 1,
        "detaching slot content must preserve the authored template reference"
    );
}

#[test]
fn isolated_empty_pair_preserves_both_component_tag_mappings() {
    let source = r#"<template><Panel><span>{{ templateOnly }}</span></Panel></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("Panel", BindingType::SetupConst),
            ("templateOnly", BindingType::SetupConst),
        ],
    );
    let facts = jsx_element_body_facts(&output);
    assert!(
        facts.non_empty_elements.is_empty(),
        "mapped empty-pair carrier must also isolate JSX children: {facts:?}\n{output}"
    );

    assert_eq!(
        facts.panel_open_name_offsets.len(),
        1,
        "the typed empty pair must retain one opening Panel tag: {facts:?}\n{output}"
    );
    assert_eq!(
        facts.panel_close_name_offsets.len(),
        1,
        "the typed empty pair must retain one closing Panel tag: {facts:?}\n{output}"
    );

    let source_open = source.find("Panel").unwrap() as u32;
    let source_close = source.rfind("Panel").unwrap() as u32;
    assert!(
        tokens.iter().any(|&(line, generated, original)| line == 0
            && generated == facts.panel_open_name_offsets[0]
            && original == source_open),
        "opening Panel must map to the authored opening name; tokens={tokens:?}\n{output}"
    );
    assert!(
        tokens
            .iter()
                .any(|&(line, generated, original)| line == 0
                && generated == facts.panel_close_name_offsets[0]
                && original == source_close),
        "generated empty-pair closing Panel must map to the authored closing name; tokens={tokens:?}\n{output}"
    );
}

fn gen_tsx_template_with_bindings(source: &str, bindings: &[(&str, BindingType)]) -> String {
    gen_tsx_template_with_components(source, bindings, &[])
}

/// Like [`gen_tsx_template_with_bindings`], but also seeds the GlobalComponents fallback
/// inventory with `fallback_consts` — the PascalCase const names a `<script setup>` would
/// emit for globally-registered components. Drives the global-component event-typing rows.
fn gen_tsx_template_with_components(
    source: &str,
    bindings: &[(&str, BindingType)],
    fallback_consts: &[&str],
) -> String {
    let alloc = Allocator::new();
    let bytes = source.as_bytes();

    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });

    let template_ast = match syntax.take_template_ast() {
        Some(ast) => ast,
        None => return String::new(),
    };

    let source_type = oxc_span::SourceType::tsx();
    let oxc_ast = crate::template::oxc::parse_template_expressions(
        &template_ast,
        source,
        &alloc,
        source_type,
        true,
    );

    let tpl_alloc = Allocator::new();
    let mut tpl_ct = CodeTransform::new(source, &tpl_alloc);
    let mut out = CodeGenOutput::new(&tpl_alloc);

    let mut binding_map: FxHashMap<&str, BindingType> = FxHashMap::default();
    for &(name, bt) in bindings {
        binding_map.insert(tpl_alloc.alloc_str(name), bt);
    }

    let options = IdeTemplateOptions {
        self_name: "App",
        comments: true,
        is_jsx: false,
        strict_slots: false,
    };

    let components =
        TemplateComponentBindings::new(fallback_consts.iter().map(|s| s.to_string()).collect());

    generate_ide_template(
        &template_ast,
        &oxc_ast,
        source,
        &mut out,
        &tpl_alloc,
        &binding_map,
        &options,
        &components,
    );
    out.apply_to(&mut tpl_ct);

    let full = tpl_ct.build_string();
    let tpl_start = template_ast.root.tag_open.start as usize;
    let tpl_end = template_ast
        .root
        .tag_close
        .as_ref()
        .map(|tc| tc.end as usize)
        .unwrap_or(full.len());
    let suffix_len = source.len() - tpl_end;
    full[tpl_start..full.len() - suffix_len].to_string()
}

// ── Basic nodes ────────────────────────────────────────────

#[test]
fn basic_div() {
    let result = gen_tsx_template("<template><div></div></template>");
    assert!(result.contains("<div></div>"), "got: {}", result);
}

#[test]
fn text_content() {
    let result = gen_tsx_template("<template><div>hello</div></template>");
    let facts = jsx_element_body_facts(&result);
    assert_eq!(facts.element_count, 1);
    assert!(
        facts.non_empty_elements.is_empty(),
        "got: {facts:?}\n{result}"
    );
    assert_eq!(
        facts.hello_string_literals, 1,
        "the detached template text must remain a JSX string expression: {facts:?}\n{result}"
    );
}

#[test]
fn text_content_with_lt_wrapped() {
    let result = gen_tsx_template("<template>2 < 1</template>");
    assert!(
        result.contains("{\"2 < 1\"}")
            || (result.contains("{\"2\"}") && result.contains("{\"< 1\"}")),
        "got: {}",
        result
    );
}

#[test]
fn text_content_escapes_quote() {
    let result = gen_tsx_template("<template>\"</template>");
    assert!(result.contains("{\"\\\"\"}"), "got: {}", result);
}

#[test]
fn interpolation_basic() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ msg }}</div></template>",
        &[("msg", BindingType::SetupRef)],
    );
    assert!(
        result.contains("{ msg }"),
        "{{ msg }} should become bare identifier in TSX mode, got: {}",
        result
    );
}

#[test]
fn interpolation_expression() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ a + b }}</div></template>",
        &[("a", BindingType::SetupRef), ("b", BindingType::SetupRef)],
    );
    assert!(result.contains("{ a + b }"), "got: {}", result);
}

#[test]
fn interpolation_partial_known_binding_stays_bare_for_completion() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ cou }}</div></template>",
        &[("count", BindingType::SetupRef)],
    );
    assert!(
        result.contains("{ cou }") || result.contains("{cou}"),
        "partial binding should stay bare for completion context, got: {}",
        result
    );
    assert!(
        !result.contains("___VERTER___instance.cou"),
        "partial binding must not get instance prefix, got: {}",
        result
    );
}

#[test]
fn comment_preserved() {
    let result = gen_tsx_template("<template><!-- hello --></template>");
    assert!(
        result.contains("{/* hello */}"),
        "Comment should be converted to JSX, got: {}",
        result
    );
}

#[test]
fn self_closing_element() {
    let result = gen_tsx_template("<template><br/></template>");
    assert!(result.contains("<br/>"), "got: {}", result);
}

/// @ai-generated - Guards Vue component prop normalization without weakening native JSX attrs.
///
/// Mutation recipe: at the component-only normalization call site, replace the normalized
/// name with the authored name. This test must fail while `basic_div` remains green; restore
/// the call site, verify a clean worktree, then rerun both tests green.
#[test]
fn component_kebab_props_are_camelized_but_native_attributes_are_not() {
    let source = r#"<template><DirectChild contract-prop="literal" :optional-flag="enabled"/><div aria-label="label" :data-test="enabled"/></template>"#;
    let output = gen_tsx_template_with_bindings(
        source,
        &[
            ("DirectChild", BindingType::SetupConst),
            ("enabled", BindingType::SetupConst),
        ],
    );

    assert_eq!(
        jsx_attributes_for_element(&output, "DirectChild")
            .into_iter()
            .map(|attribute| attribute.name)
            .collect::<Vec<_>>(),
        vec!["contractProp", "optionalFlag"],
        "Vue component props must use the public camel-case JSX contract: {output}"
    );
    assert_eq!(
        jsx_attributes_for_element(&output, "div")
            .into_iter()
            .map(|attribute| attribute.name)
            .collect::<Vec<_>>(),
        vec!["aria-label", "data-test"],
        "native DOM attributes retain their authored JSX spelling: {output}"
    );

    let mapped_source = r#"<template><DirectChild :contract-prop="enabled"/></template>"#;
    let (mapped_output, tokens) = gen_tsx_template_with_map(
        mapped_source,
        &[
            ("DirectChild", BindingType::SetupConst),
            ("enabled", BindingType::SetupConst),
        ],
    );
    let generated_prop = jsx_attributes_for_element(&mapped_output, "DirectChild")
        .into_iter()
        .find(|attribute| attribute.name == "contractProp")
        .expect("normalized component prop must remain a parsed JSX attribute");
    let authored_prop = mapped_source
        .find("contract-prop")
        .expect("fixture contains authored component prop") as u32;
    assert!(
        tokens.iter().any(|&(line, generated, original)| {
            line == 0 && generated == generated_prop.start && original == authored_prop
        }),
        "normalized component prop must map to the authored kebab-name start; attr={generated_prop:?}, tokens={tokens:?}, output={mapped_output}"
    );
}

#[test]
fn void_element_without_self_closing_slash() {
    // HTML void elements like <br> (no slash) must become self-closing in JSX
    let result = gen_tsx_template("<template><br></template>");
    // Must be self-closing in JSX output (either <br/> or <br />)
    assert!(
        result.contains("<br/>") || result.contains("<br />"),
        "void element <br> must be self-closing in JSX: {result}"
    );
    // Must NOT have unclosed <br> (which is invalid JSX)
    assert!(
        !result.contains("<br>"),
        "raw <br> must not appear in JSX output: {result}"
    );

    // Multiple adjacent void elements
    let result2 = gen_tsx_template("<template><br><br></template>");
    assert!(
        !result2.contains("<br>"),
        "adjacent void <br><br> must both be self-closing: {result2}"
    );

    // <input> with attributes
    let result3 = gen_tsx_template(r#"<template><input type="text"></template>"#);
    assert!(
        !result3.contains("<input type=\"text\">"),
        "void <input> with attrs must be self-closing: {result3}"
    );
}

#[test]
fn multiline_text_escapes_newlines_in_string_literal() {
    let result = gen_tsx_template("<template><p>\n  Hello\n  World\n</p></template>");
    // Text IS wrapped in {"..."} — but newlines must be escaped as \n
    assert!(
        result.contains("{\""),
        "text should be wrapped in string literal: {result}"
    );
    // Must contain escaped newlines, not raw newlines inside the string
    assert!(
        result.contains("\\n"),
        "newlines in text must be escaped as \\n: {result}"
    );
    // The {"..."} expression must be on a single line (no raw newlines)
    for line in result.lines() {
        if line.contains("{\"") {
            assert!(
                line.contains("\"}"),
                "text string literal must be on single line (no raw newlines): {result}"
            );
        }
    }
}

#[test]
fn nested_elements() {
    let result = gen_tsx_template("<template><div><span></span></div></template>");
    let facts = jsx_element_body_facts(&result);
    assert_eq!(
        facts.element_count, 2,
        "div and span must both remain typed"
    );
    assert!(
        facts.non_empty_elements.is_empty(),
        "nested Vue elements must become fragment siblings, not JSX element children: {facts:?}\n{result}"
    );
}

#[test]
fn multiple_root_elements() {
    let result = gen_tsx_template("<template><div></div><span></span></template>");
    assert!(
        result.contains("<>") && result.contains("</>"),
        "Multiple root elements should be wrapped in fragment, got: {}",
        result
    );
}

// ── Interpolation with bindings ────────────────────────────

#[test]
fn interpolation_with_setup_ref() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ count }}</div></template>",
        &[("count", BindingType::SetupRef)],
    );
    // In TSX mode, SetupRef gets no prefix and no .value suffix (block scope handles unwrapping)
    assert!(
        result.contains("{ count }") && !result.contains("count.value"),
        "SetupRef should be bare identifier in TSX mode (no .value), got: {}",
        result
    );
}

#[test]
fn interpolation_with_setup_const() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ msg }}</div></template>",
        &[("msg", BindingType::SetupConst)],
    );
    // SetupConst in inline mode: no prefix, no suffix
    assert!(
        result.contains("{ msg }"),
        "SetupConst should have no prefix/suffix, got: {}",
        result
    );
}

#[test]
fn interpolation_with_props() {
    let result = gen_tsx_template_with_bindings(
        "<template><div>{{ title }}</div></template>",
        &[("title", BindingType::Props)],
    );
    // Props in inline mode: __props. prefix
    assert!(
        result.contains("__props.title"),
        "Props should get __props. prefix, got: {}",
        result
    );
}

// ── Structural directive removal (v-if, v-for, v-slot) ───

#[test]
fn v_if_attribute_removed_from_output() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="show">hello</div></template>"#,
        &[("show", BindingType::SetupRef)],
    );
    // Positive: IIFE if-block should be present
    assert!(
        result.contains("if(show)"),
        "v-if condition should produce IIFE if-block, got: {}",
        result
    );
    // Negative: v-if attribute must NOT appear in output
    assert!(
        !result.contains("v-if"),
        "v-if attribute must be removed from JSX output, got: {}",
        result
    );
}

#[test]
fn v_if_compound_expr_attribute_removed() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="a || b" class="foo">hello</div></template>"#,
        &[("a", BindingType::SetupRef), ("b", BindingType::SetupRef)],
    );
    assert!(
        !result.contains("v-if"),
        "v-if attribute must not appear in output, got: {}",
        result
    );
    // The condition should be in the ternary
    assert!(
        result.contains("a || b"),
        "resolved condition should be in ternary, got: {}",
        result
    );
    // The class attribute should still be present
    assert!(
        result.contains(r#"class="foo""#),
        "class attribute should be preserved, got: {}",
        result
    );
}

#[test]
fn v_if_with_props_binding_attribute_removed() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="show" class="active">content</div></template>"#,
        &[("show", BindingType::Props)],
    );
    assert!(
        !result.contains("v-if"),
        "v-if must be removed from output, got: {}",
        result
    );
    assert!(
        result.contains("if(__props.show)"),
        "should have __props.show in if-condition, got: {}",
        result
    );
    // v-if value should NOT appear as string attribute value
    assert!(
        !result.contains(r#"="show""#) && !result.contains(r#"="__props.show""#),
        "v-if value should not be in attribute quotes, got: {}",
        result
    );
}

#[test]
fn v_else_if_attribute_removed() {
    let result = gen_tsx_template(
        r#"<template><div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div></template>"#,
    );
    assert!(
        !result.contains("v-if"),
        "v-if must not appear in output, got: {}",
        result
    );
    assert!(
        !result.contains("v-else-if"),
        "v-else-if must not appear in output, got: {}",
        result
    );
    assert!(
        !result.contains("v-else"),
        "v-else must not appear in output, got: {}",
        result
    );
}

#[test]
fn v_for_attribute_removed_from_output() {
    let result = gen_tsx_template(
        r#"<template><div v-for="item in items" :key="item.id">{{ item.name }}</div></template>"#,
    );
    assert!(
        !result.contains("v-for"),
        "v-for attribute must be removed from JSX output, got: {}",
        result
    );
    // Positive: .map() wrapper should be present
    assert!(
        result.contains(".map("),
        "v-for should produce .map() wrapper, got: {}",
        result
    );
    // The " in " separator should not appear as raw text
    assert!(
        !result.contains(r#""item in items""#),
        "v-for expression should not appear as attribute value string, got: {}",
        result
    );
}

#[test]
fn v_for_with_props_binding_attribute_removed() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><li v-for="item in list">{{ item }}</li></template>"#,
        &[("list", BindingType::Props)],
    );
    assert!(
        !result.contains("v-for"),
        "v-for must be removed from output, got: {}",
        result
    );
    assert!(
        result.contains("__props.list).map("),
        "iterable should get __props. prefix, got: {}",
        result
    );
}

#[test]
fn v_slot_attribute_removed_from_output() {
    let result = gen_tsx_template(
        r#"<template><MyComp><template #default="{ item }"><span>{{ item }}</span></template></MyComp></template>"#,
    );
    assert!(
        !result.contains("v-slot") && !result.contains("#default"),
        "v-slot/#default must be removed from output, got: {}",
        result
    );
}

#[test]
fn v_once_attribute_removed_from_output() {
    let result = gen_tsx_template(r#"<template><div v-once>static content</div></template>"#);
    assert!(
        !result.contains("v-once"),
        "v-once must be removed from JSX output, got: {}",
        result
    );
    assert!(
        result.contains("<div>"),
        "element should still be present, got: {}",
        result
    );
}

#[test]
fn multiple_directives_all_removed() {
    let result =
        gen_tsx_template(r#"<template><div v-if="show" v-once class="box">hello</div></template>"#);
    assert!(
        !result.contains("v-if"),
        "v-if must be removed, got: {}",
        result
    );
    assert!(
        !result.contains("v-once"),
        "v-once must be removed, got: {}",
        result
    );
    assert!(
        result.contains(r#"class="box""#),
        "regular attributes should be preserved, got: {}",
        result
    );
}

#[test]
fn v_if_and_v_for_on_same_element_both_removed() {
    let result = gen_tsx_template(
        r#"<template><div v-for="item in items" v-if="item.active">{{ item.name }}</div></template>"#,
    );
    assert!(
        !result.contains("v-for"),
        "v-for must be removed, got: {}",
        result
    );
    assert!(
        !result.contains("v-if"),
        "v-if must be removed, got: {}",
        result
    );
    assert!(
        result.contains(".map("),
        "should have .map() wrapper, got: {}",
        result
    );
    assert!(
        result.contains("?"),
        "should have ternary from v-if (not IIFE), got: {}",
        result
    );
    assert!(
        result.contains(": null"),
        "should have ternary null branch, got: {}",
        result
    );
}

// ── v-for comprehensive tests ────────────────────────────────

#[test]
fn v_for_destructured_params() {
    let result = gen_tsx_template(
        r#"<template><li v-for="(item, index) in items" :key="index">{{ item }}</li></template>"#,
    );
    assert!(
        !result.contains("v-for"),
        "v-for attribute must be removed, got: {}",
        result
    );
    assert!(
        result.contains(".map((item, index)"),
        "destructured params should be in .map() callback, got: {}",
        result
    );
    // " in " separator must not appear as raw text
    assert!(
        !result.contains("\" in \"") && !result.contains(" in items"),
        "v-for separator must not appear in output, got: {}",
        result
    );
}

#[test]
fn v_for_object_destructure() {
    let result = gen_tsx_template(
        r#"<template><div v-for="(value, key, index) in obj">{{ key }}: {{ value }}</div></template>"#,
    );
    assert!(
        !result.contains("v-for"),
        "v-for must be removed, got: {}",
        result
    );
    assert!(
        result.contains(".map((value, key, index)"),
        "triple destructure should be in .map(), got: {}",
        result
    );
}

#[test]
fn v_for_of_variant() {
    let result =
        gen_tsx_template(r#"<template><span v-for="item of items">{{ item }}</span></template>"#);
    assert!(
        !result.contains("v-for"),
        "v-for must be removed, got: {}",
        result
    );
    assert!(
        result.contains(".map("),
        "should produce .map() wrapper, got: {}",
        result
    );
    // "of" separator must not leak
    assert!(
        !result.contains(" of items"),
        "v-for 'of' separator must not appear in output, got: {}",
        result
    );
    // Simple identifier iterable should get type annotation
    assert!(
        result.contains(": (typeof items)[number]"),
        "single param with simple iterable should get type annotation, got: {}",
        result
    );
}

#[test]
fn v_for_simple_param_has_type_annotation() {
    let result =
        gen_tsx_template(r#"<template><div v-for="item in items">{{ item }}</div></template>"#);
    assert!(
        result.contains(": (typeof items)[number]"),
        "single param with simple iterable should get type annotation, got: {}",
        result
    );
}

#[test]
fn v_for_destructured_param_has_type_annotation() {
    let result = gen_tsx_template(
        r#"<template><div v-for="{ name, email } in users">{{ name }}</div></template>"#,
    );
    // Destructured pattern without comma in the params (commas are inside braces but
    // the top-level params string is "{ name, email }" which contains commas)
    // This should NOT get annotation because the params contain a comma
    assert!(
        !result.contains("(typeof users)[number]"),
        "destructured params with commas should not get type annotation, got: {}",
        result
    );
}

#[test]
fn v_for_multi_param_no_annotation() {
    let result = gen_tsx_template(
        r#"<template><li v-for="(item, index) in items" :key="index">{{ item }}</li></template>"#,
    );
    assert!(
        !result.contains("(typeof items)[number]"),
        "multi-param v-for should not get type annotation, got: {}",
        result
    );
}

#[test]
fn v_for_complex_iterable_no_annotation() {
    let result = gen_tsx_template(
        r#"<template><span v-for="item in getItems()">{{ item }}</span></template>"#,
    );
    assert!(
        !result.contains("(typeof"),
        "complex iterable (function call) should not get type annotation, got: {}",
        result
    );
}

#[test]
fn v_for_numeric_range() {
    let result = gen_tsx_template(r#"<template><span v-for="n in 10">{{ n }}</span></template>"#);
    assert!(
        !result.contains("v-for"),
        "v-for must be removed, got: {}",
        result
    );
    // Numeric range must be wrapped in Array.from() — calling .map() directly on
    // a number literal (e.g., `10.map(...)`) is invalid JavaScript.
    assert!(
        result.contains("Array.from({length: 10}"),
        "numeric range should use Array.from(), got: {}",
        result
    );
    assert!(
        result.contains(".map("),
        "numeric range should be iterable in .map(), got: {}",
        result
    );
}

#[test]
fn v_for_complex_iterable_expression() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-for="item in items.filter(x => x.active)" :key="item.id">{{ item.name }}</div></template>"#,
        &[("items", BindingType::SetupConst)],
    );
    assert!(
        !result.contains("v-for"),
        "v-for must be removed, got: {}",
        result
    );
    assert!(
        result.contains(".filter("),
        "complex iterable expression should be preserved, got: {}",
        result
    );
    assert!(
        result.contains(".map("),
        "should have .map() wrapper, got: {}",
        result
    );
}

#[test]
fn v_for_setup_ref_iterable_binding() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><li v-for="item in todos">{{ item.text }}</li></template>"#,
        &[("todos", BindingType::SetupRef)],
    );
    assert!(
        !result.contains("v-for"),
        "v-for must be removed, got: {}",
        result
    );
    assert!(
        result.contains("todos).map(") && !result.contains("todos.value"),
        "SetupRef iterable should be bare identifier in TSX mode (no .value), got: {}",
        result
    );
}

/// Discriminating regression guard — an iterable whose only identifier is also
/// the v-for loop local (`v-for="item in item"`) parses with an OXC v-for present
/// but ZERO in-range references (the local is filtered out of the reference set).
/// That sub-case must emit the iterable VERBATIM (`{(item)`), exactly as a
/// reference-bearing resolver patch over an empty reference set would: the
/// resolver prefix is never applied because there is no reference to apply it to.
/// Routing this through the resolver-only simple-expression path instead prefixes
/// the bare identifier to `___VERTER___instance.item`, a generated-byte regression.
#[test]
fn v_for_iterable_only_loop_local_emits_verbatim_not_resolver_prefixed() {
    let result =
        gen_tsx_template(r#"<template><div v-for="item in item">{{ item }}</div></template>"#);
    assert!(
        !result.contains("v-for"),
        "v-for must be removed, got: {}",
        result
    );
    // The iterable identifier is the v-for local → OXC yields zero in-range refs.
    // Parent behavior: emit the iterable verbatim, wrapped `{(item).map(`.
    assert!(
        result.contains("{(item).map("),
        "iterable with only the loop local must stay verbatim `{{(item).map(`, got: {}",
        result
    );
    // It must NOT route through the resolver-only path (which prefixes the bare
    // identifier as an instance member access).
    assert!(
        !result.contains("___VERTER___instance.item"),
        "iterable must not be resolver-prefixed to `___VERTER___instance.item`, got: {}",
        result
    );
}

#[test]
fn v_for_closing_structure() {
    let result = gen_tsx_template(
        r#"<template><div v-for="item in items" :key="item.id">text</div></template>"#,
    );
    assert!(
        result.contains(") })}"),
        "v-for closing should produce CloseParen+CloseBrace+CloseParen+CloseBrace for .map() statement-body closure, got: {}",
        result
    );
}

// ── ref attribute tests ──────────────────────────────────────

#[test]
fn ref_static_converts_to_jsx_expression() {
    let result = gen_tsx_template(r#"<template><div ref="myRef">content</div></template>"#);
    // Should convert to ref={"myRef"} (JSX expression with string literal)
    assert!(
        result.contains(r#"ref={"myRef"}"#),
        "static ref should become ref={{\"myRef\"}}, got: {}",
        result
    );
    // Must NOT have bare ref="myRef" (Vue syntax, not valid JSX expression)
    assert!(
        !result.contains(r#"ref="myRef""#),
        "bare ref=\"myRef\" must not appear in JSX output, got: {}",
        result
    );
}

#[test]
fn ref_dynamic_binding_converts_to_jsx_expression() {
    let result =
        gen_tsx_template(r#"<template><div :ref="el => (myRef = el)">content</div></template>"#);
    assert!(
        result.contains("ref={"),
        "dynamic :ref should become ref={{expr}}, got: {}",
        result
    );
    // The :ref prefix must be removed
    assert!(
        !result.contains(":ref"),
        ":ref prefix must not appear in output, got: {}",
        result
    );
}

#[test]
fn ref_with_other_attrs_preserved() {
    let result = gen_tsx_template(
        r#"<template><input ref="inputRef" type="text" class="field" /></template>"#,
    );
    assert!(
        result.contains(r#"ref={"inputRef"}"#),
        "ref should be converted, got: {}",
        result
    );
    assert!(
        result.contains(r#"type="text""#),
        "type attribute should be preserved, got: {}",
        result
    );
    assert!(
        result.contains(r#"class="field""#),
        "class attribute should be preserved, got: {}",
        result
    );
}

// ── v-if IIFE structure tests ─────────────────────────────

#[test]
fn v_if_iife_structure() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="visible">hello</div></template>"#,
        &[("visible", BindingType::SetupRef)],
    );
    // Must have IIFE pattern: {()=>{if(cond){...}}}
    assert!(
        result.contains("{()=>{if(visible){"),
        "v-if should open with IIFE if-block, got: {}",
        result
    );
    // Must close with }}} (block close + arrow body close + JSX expression close)
    assert!(
        result.contains("}}}"),
        "v-if standalone should close with }}}}, got: {}",
        result
    );
    // Must NOT have ternary pattern
    assert!(
        !result.contains("? ("),
        "should not use ternary pattern, got: {}",
        result
    );
    assert!(
        !result.contains(": null}"),
        "should not have null fallback, got: {}",
        result
    );
}

#[test]
fn v_if_else_chain_iife_structure() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div></template>"#,
        &[("a", BindingType::SetupRef), ("b", BindingType::SetupRef)],
    );
    // Should have IIFE if/else-if/else chain
    assert!(
        result.contains("{()=>{if(a){"),
        "should have IIFE if-block, got: {}",
        result
    );
    assert!(
        result.contains("else if(b){"),
        "should have else-if block, got: {}",
        result
    );
    assert!(
        result.contains("else{"),
        "should have else block, got: {}",
        result
    );
    // Should close with }}} at the end (else block close + arrow body + JSX)
    assert!(
        result.contains("}}}"),
        "chain should close properly, got: {}",
        result
    );
    // Should NOT have standalone "v-else" text
    assert!(
        !result.contains("v-else"),
        "v-else must not appear as attribute, got: {}",
        result
    );
}

#[test]
fn v_if_else_if_without_else_closes() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="a">A</div><div v-else-if="b">B</div></template>"#,
        &[("a", BindingType::SetupRef), ("b", BindingType::SetupRef)],
    );
    assert!(
        result.contains("{()=>{if(a){"),
        "should have IIFE if-block, got: {}",
        result
    );
    assert!(
        result.contains("else if(b){"),
        "should have else-if block, got: {}",
        result
    );
    // Without v-else, parent loop adds }}
    assert!(
        result.contains("}}}"),
        "chain without else should close with }}}}, got: {}",
        result
    );
}

#[test]
fn v_if_with_binding_prefix_iife() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="show">content</div></template>"#,
        &[("show", BindingType::Props)],
    );
    assert!(
        result.contains("{()=>{if(__props.show){"),
        "should use __props.show in if-condition, got: {}",
        result
    );
}

// ── v-if prop narrowing guard tests ──────────────────────────

#[test]
fn v_if_event_handler_gets_guard() {
    let result = gen_tsx_template(
        r#"<template><div v-if="show" @click="handler($event)">click</div></template>"#,
    );
    // Event handler with $event should have guard: if (!(...)) { return undefined; }
    assert!(
        result.contains("return undefined"),
        "event handler in v-if should have narrowing guard, got: {}",
        result
    );
    assert!(
        result.contains("show"),
        "guard should reference the condition, got: {}",
        result
    );
    // Positive: still has the event handler
    assert!(
        result.contains("onClick={"),
        "should have onClick handler, got: {}",
        result
    );
    // Negative: v-if should not appear
    assert!(
        !result.contains("v-if"),
        "v-if must be removed, got: {}",
        result
    );
}

#[test]
fn v_else_if_event_handler_gets_combined_guard() {
    let result = gen_tsx_template(
        r#"<template><div v-if="a">A</div><div v-else-if="b" @click="handler($event)">B</div></template>"#,
    );
    // Guard should negate prior siblings: !((a)) and include own condition (b)
    assert!(
        result.contains("!(("),
        "guard should have negation of prior condition, got: {}",
        result
    );
}

#[test]
fn v_if_non_function_prop_no_guard() {
    let result =
        gen_tsx_template(r#"<template><div v-if="show" :class="myClass">content</div></template>"#);
    // Non-function bindings should NOT have guards
    assert!(
        !result.contains("?undefined:"),
        "non-function prop should not have ternary guard, got: {}",
        result
    );
}

// ── v-if nested IIFE tests ──────────────────────────────────

#[test]
fn v_if_nested_gets_block_guard() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="parent"><span v-if="child">nested</span></div></template>"#,
        &[
            ("parent", BindingType::SetupRef),
            ("child", BindingType::SetupRef),
        ],
    );
    // Nested v-if should have block guard: if(!(condText)) return;
    let has_guard = result.contains("return;") && result.contains("if(!(");
    assert!(
        has_guard,
        "nested v-if should have block guard from parent, got: {}",
        result
    );
    // Should still have the nested if-condition
    assert!(
        result.contains("if(child)"),
        "nested v-if should have its own if-condition, got: {}",
        result
    );
}

// ── Part F: Comment repositioning ────────────────────────────────

#[test]
fn v_if_comment_before_repositioned_inside_iife() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><!-- @ts-expect-error --><div v-if="show">hello</div></template>"#,
        &[("show", BindingType::SetupRef)],
    );
    // Comment should appear INSIDE the IIFE, after the if(cond){ line
    // Pattern: {()=>{if(cond){ {/* @ts-expect-error */} <div>...
    assert!(
        result.contains("if(show)"),
        "should have IIFE condition, got:\n{}",
        result
    );
    // Comment must be AFTER the IIFE open, not before it
    let iife_pos = result.find("{()=>{").expect("should have IIFE open");
    let comment_pos = result
        .find("{/* @ts-expect-error */}")
        .expect("comment should be preserved");
    assert!(
        comment_pos > iife_pos,
        "comment should appear AFTER IIFE open, got:\n{}",
        result
    );
    // Negative: comment should NOT appear before the IIFE
    let before_iife = &result[..iife_pos];
    assert!(
        !before_iife.contains("@ts-expect-error"),
        "comment must not appear before IIFE, got:\n{}",
        result
    );
}

#[test]
fn v_if_without_preceding_comment_no_change() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-if="show">hello</div></template>"#,
        &[("show", BindingType::SetupRef)],
    );
    // No comment to reposition — should work normally
    assert!(
        result.contains("{()=>{if(show){"),
        "should have IIFE pattern, got:\n{}",
        result
    );
    assert!(
        !result.contains("{/*"),
        "should not have any comments, got:\n{}",
        result
    );
}

// ── Part F2: v-if/v-else with whitespace between elements ────────

#[test]
fn v_if_else_with_whitespace_between_elements() {
    // Simulates formatted template: <img v-if="cond" />\n  <span v-else>fallback</span>
    let result = gen_tsx_template_with_bindings(
        "<template>\n  <img v-if=\"show\" />\n  <span v-else>fallback</span>\n</template>",
        &[("show", BindingType::SetupRef)],
    );

    // Positive: must have complete IIFE chain with if/else
    assert!(
        result.contains("{()=>{if(show){"),
        "should have IIFE if-block, got:\n{}",
        result
    );
    assert!(
        result.contains("else{"),
        "should have else block in same IIFE, got:\n{}",
        result
    );

    // Structural: IIFE must NOT close before else — no }}} between IIFE start and else
    let iife_start = result.find("{()=>{if(").unwrap();
    let else_pos = result.find("else{").unwrap();
    let between = &result[iife_start..else_pos];
    assert!(
        !between.contains("}}}"),
        "IIFE must not close before else: premature close found between IIFE start and else, got:\n{}",
        result
    );

    // Negative: v-if/v-else attributes must not appear in output
    assert!(
        !result.contains("v-if"),
        "v-if attribute must be removed from JSX, got:\n{}",
        result
    );
    assert!(
        !result.contains("v-else"),
        "v-else attribute must be removed from JSX, got:\n{}",
        result
    );

    // Validate JSX syntax: the full template output must parse
    let wrapper = format!("const x = {}", result);
    let val_alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&val_alloc, &wrapper, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "TSX template output has syntax errors: {:?}\n--- output ---\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        result
    );
}

#[test]
fn v_if_else_if_else_with_whitespace() {
    let result = gen_tsx_template_with_bindings(
        "<template>\n  <div v-if=\"a\">A</div>\n  <div v-else-if=\"b\">B</div>\n  <div v-else>C</div>\n</template>",
        &[("a", BindingType::SetupRef), ("b", BindingType::SetupRef)],
    );

    // Positive: complete IIFE chain
    assert!(
        result.contains("{()=>{if(a){"),
        "should have IIFE if-block, got:\n{}",
        result
    );
    assert!(
        result.contains("else if(b){"),
        "should have else-if block, got:\n{}",
        result
    );
    assert!(
        result.contains("else{"),
        "should have else block, got:\n{}",
        result
    );

    // Structural: IIFE must NOT close before else-if or else
    let iife_start = result.find("{()=>{if(").unwrap();
    let else_if_pos = result.find("else if(").unwrap();
    let else_pos = result.find("else{").unwrap();
    let between_if_and_else_if = &result[iife_start..else_if_pos];
    assert!(
        !between_if_and_else_if.contains("}}}"),
        "IIFE must not close before else-if, got:\n{}",
        result
    );
    let between_else_if_and_else = &result[else_if_pos..else_pos];
    assert!(
        !between_else_if_and_else.contains("}}}"),
        "IIFE must not close before else, got:\n{}",
        result
    );

    // Negative: directive attributes must not appear
    assert!(
        !result.contains("v-if"),
        "v-if must be removed, got:\n{}",
        result
    );
    assert!(
        !result.contains("v-else"),
        "v-else must be removed, got:\n{}",
        result
    );

    // Validate JSX syntax: the full template output must parse
    let wrapper = format!("const x = {}", result);
    let val_alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&val_alloc, &wrapper, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "TSX template output has syntax errors: {:?}\n--- output ---\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        result
    );
}

// ── Part G: <template v-slot> with v-if ──────────────────────────

#[test]
fn template_v_if_v_slot_skips_iife() {
    // <template v-if v-slot> should NOT get IIFE wrapping (slot handles conditions)
    let result = gen_tsx_template(
        r#"<template><MyComp><template v-if="show" #default>content</template></MyComp></template>"#,
    );
    // The IIFE pattern should NOT wrap the slot template
    assert!(
        !result.contains("{()=>{if(show){"),
        "template with v-if + v-slot should not get IIFE wrapping, got:\n{}",
        result
    );
}

// ── v-bind function prop guards ──────────────────

#[test]
fn v_bind_arrow_expr_gets_ternary_guard() {
    // Arrow expression body: `:handler="() => msg.trim()"` inside v-if
    // → handler={() => !(guard)?undefined:msg.trim()}
    let result = gen_tsx_template(
        r#"<template><div v-if="typeof msg === 'string'" :handler="() => msg.trim()">hi</div></template>"#,
    );
    let norm: String = result.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        norm.contains("?undefined:"),
        "arrow expression prop should get ternary guard, got:\n{}",
        result
    );
    assert!(
        !norm.contains("if(!(") || norm.contains("{()=>{if("),
        "arrow expression should use ternary guard, not block guard in handler, got:\n{}",
        result
    );
}

#[test]
fn v_bind_arrow_block_gets_block_guard() {
    // Arrow block body: `:handler="() => { return msg.trim() }"` inside v-if
    // → handler={() => {if(!(guard))return; return msg.trim() }}
    let result = gen_tsx_template(
        r#"<template><div v-if="typeof msg === 'string'" :handler="() => { return msg.trim() }">hi</div></template>"#,
    );
    let norm: String = result.chars().filter(|c| !c.is_whitespace()).collect();
    // The handler value should contain a block guard
    // Find the handler= part and check for block guard inside it
    let handler_pos = norm.find("handler={").expect("should have handler prop");
    let after_handler = &norm[handler_pos..];
    assert!(
        after_handler.contains("if(!(") && after_handler.contains(")return;"),
        "arrow block prop should get block guard inside handler, got:\n{}",
        result
    );
}

#[test]
fn v_bind_function_expr_gets_block_guard() {
    // Function expression: `:handler="function() { return msg.trim() }"` inside v-if
    // → handler={function() {if(!(guard))return; return msg.trim() }}
    let result = gen_tsx_template(
        r#"<template><div v-if="typeof msg === 'string'" :handler="function() { return msg.trim() }">hi</div></template>"#,
    );
    let norm: String = result.chars().filter(|c| !c.is_whitespace()).collect();
    let handler_pos = norm.find("handler={").expect("should have handler prop");
    let after_handler = &norm[handler_pos..];
    assert!(
        after_handler.contains("if(!(") && after_handler.contains(")return;"),
        "function expression prop should get block guard, got:\n{}",
        result
    );
}

#[test]
fn v_if_guarded_function_value_tsx_is_byte_equivalent() {
    // Characterization: the v-if narrowing-guard injection for function-typed value
    // props produces EXACTLY the same generated TSX after the EmitOp in-place
    // migration as the pre-migration baked-overwrite form. Only the SOURCE MAP
    // improves; the bytes (narrowing structure + accessor prefixes) are identical.
    // SetupConst → bare identifier; Props → `__props.` accessor prefix.

    // Arrow-expression body, SetupConst (no prefix).
    let arrow_expr = gen_tsx_template_with_bindings(
        r#"<template><div v-if="ok" :onX="() => handle()"/></template>"#,
        &[
            ("ok", BindingType::SetupConst),
            ("handle", BindingType::SetupConst),
        ],
    );
    assert!(
        arrow_expr.contains("onX={() => !((ok))?undefined:handle()}"),
        "arrow-expr guard must be byte-identical `onX={{() => !((ok))?undefined:handle()}}`: {arrow_expr}"
    );

    // Arrow-expression body, Props (accessor prefix on the body identifier; the v-if
    // condition is resolved independently by the directive layer).
    let arrow_expr_props = gen_tsx_template_with_bindings(
        r#"<template><div v-if="ok" :onX="() => handle()"/></template>"#,
        &[
            ("ok", BindingType::SetupConst),
            ("handle", BindingType::Props),
        ],
    );
    assert!(
        arrow_expr_props.contains("?undefined:__props.handle()"),
        "arrow-expr Props body must keep its `__props.` accessor prefix after the guard: {arrow_expr_props}"
    );

    // Arrow-block body, SetupConst.
    let arrow_block = gen_tsx_template_with_bindings(
        r#"<template><div v-if="ok" :onX="() => { handle() }"/></template>"#,
        &[
            ("ok", BindingType::SetupConst),
            ("handle", BindingType::SetupConst),
        ],
    );
    assert!(
        arrow_block.contains("onX={() => {if(!((ok))) return; handle() }}"),
        "arrow-block guard must be byte-identical `() => {{if(!((ok))) return; handle() }}`: {arrow_block}"
    );

    // Function-expression body, SetupConst.
    let fn_expr = gen_tsx_template_with_bindings(
        r#"<template><div v-if="ok" :onX="function() { handle() }"/></template>"#,
        &[
            ("ok", BindingType::SetupConst),
            ("handle", BindingType::SetupConst),
        ],
    );
    assert!(
        fn_expr.contains("onX={function() {if(!((ok))) return; handle() }}"),
        "fn-expr guard must be byte-identical `function() {{if(!((ok))) return; handle() }}`: {fn_expr}"
    );

    // Negative (all shapes): the guarded expression must NOT be doubled or mangled —
    // exactly ONE guard per prop.
    assert_eq!(
        arrow_expr.matches("?undefined:").count(),
        1,
        "exactly one ternary guard per arrow-expr prop: {arrow_expr}"
    );
    assert_eq!(
        arrow_block.matches("if(!((ok))) return;").count(),
        1,
        "exactly one block guard per arrow-block prop: {arrow_block}"
    );
}

#[test]
fn v_bind_non_function_no_guard() {
    // Non-function props should NOT get any guard
    let result = gen_tsx_template(r#"<template><div v-if="show" :class="msg">hi</div></template>"#);
    let norm: String = result.chars().filter(|c| !c.is_whitespace()).collect();
    // Find the class prop
    let class_pos = norm.find("class={").expect("should have class prop");
    let after_class = &norm[class_pos..];
    // Should NOT have any guard
    assert!(
        !after_class.starts_with("class={()=>") && !after_class.contains("?undefined:"),
        "non-function prop should not get guard, got:\n{}",
        result
    );
}

// ── Part H: JSX syntax validation for directive combinations ─────

/// Validate that the generated TSX template is parseable JSX/TSX.
/// Wraps the template output in a JSX fragment so IIFE expressions parse correctly.
fn assert_valid_jsx(source: &str, label: &str) {
    let result = gen_tsx_template(source);
    let wrapper = format!("const x = <>{}</>", result);
    let val_alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&val_alloc, &wrapper, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "[{}] TSX syntax errors: {:?}\n--- source ---\n{}\n--- output ---\n{}",
        label,
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        source,
        result
    );
}

#[test]
fn jsx_valid_v_if_alone() {
    assert_valid_jsx(
        r#"<template><div v-if="show">content</div></template>"#,
        "v-if alone",
    );
}

#[test]
fn jsx_valid_v_if_else() {
    assert_valid_jsx(
        r#"<template><div v-if="show">A</div><div v-else>B</div></template>"#,
        "v-if/v-else inline",
    );
}

#[test]
fn jsx_valid_v_if_else_if_else() {
    assert_valid_jsx(
        r#"<template><div v-if="a">A</div><div v-else-if="b">B</div><div v-else>C</div></template>"#,
        "v-if/v-else-if/v-else inline",
    );
}

#[test]
fn jsx_valid_v_if_else_whitespace() {
    assert_valid_jsx(
        "<template>\n  <div v-if=\"show\">A</div>\n  <div v-else>B</div>\n</template>",
        "v-if/v-else with whitespace",
    );
}

#[test]
fn jsx_valid_v_if_else_if_else_whitespace() {
    assert_valid_jsx(
        "<template>\n  <div v-if=\"a\">A</div>\n  <div v-else-if=\"b\">B</div>\n  <div v-else>C</div>\n</template>",
        "v-if/v-else-if/v-else with whitespace",
    );
}

#[test]
fn jsx_valid_v_for_alone() {
    assert_valid_jsx(
        r#"<template><div v-for="item in items" :key="item.id">{{ item.name }}</div></template>"#,
        "v-for alone",
    );
}

#[test]
fn jsx_valid_v_for_with_index() {
    assert_valid_jsx(
        r#"<template><div v-for="(item, index) in items" :key="index">{{ item }}</div></template>"#,
        "v-for with index",
    );
}

#[test]
fn jsx_valid_v_slot_component() {
    assert_valid_jsx(
        r#"<template><MyComp v-slot="{ data }"><span>{{ data }}</span></MyComp></template>"#,
        "v-slot on component",
    );
}

#[test]
fn jsx_valid_named_slot() {
    assert_valid_jsx(
        r#"<template><MyComp><template #header>Header</template><template #default>Body</template></MyComp></template>"#,
        "named slots with template",
    );
}

#[test]
fn jsx_valid_v_if_v_for_same_element() {
    assert_valid_jsx(
        r#"<template><div v-if="show" v-for="item in items" :key="item">{{ item }}</div></template>"#,
        "v-if + v-for same element",
    );
}

#[test]
fn jsx_valid_v_for_with_v_if_children() {
    assert_valid_jsx(
        r#"<template><ul><li v-for="item in items" :key="item.id"><span v-if="item.active">active</span><span v-else>inactive</span></li></ul></template>"#,
        "v-for with v-if/v-else children",
    );
}

#[test]
fn jsx_valid_v_for_with_v_if_children_whitespace() {
    assert_valid_jsx(
        "<template>\n  <ul>\n    <li v-for=\"item in items\" :key=\"item.id\">\n      <span v-if=\"item.active\">active</span>\n      <span v-else>inactive</span>\n    </li>\n  </ul>\n</template>",
        "v-for with v-if/v-else children whitespace",
    );
}

#[test]
fn jsx_valid_v_if_with_v_slot() {
    assert_valid_jsx(
        r#"<template><MyComp v-if="show" v-slot="{ data }"><span>{{ data }}</span></MyComp></template>"#,
        "v-if + v-slot on component",
    );
}

#[test]
fn jsx_valid_v_for_with_v_slot() {
    assert_valid_jsx(
        r#"<template><MyComp v-for="item in items" :key="item.id" v-slot="{ data }"><span>{{ data }}</span></MyComp></template>"#,
        "v-for + v-slot",
    );
}

#[test]
fn jsx_valid_nested_v_if() {
    assert_valid_jsx(
        r#"<template><div v-if="a"><span v-if="b">B</span><span v-else>not B</span></div></template>"#,
        "nested v-if chains",
    );
}

#[test]
fn jsx_valid_v_if_with_template_v_for() {
    assert_valid_jsx(
        "<template>\n  <div v-if=\"show\">\n    <span v-for=\"item in items\" :key=\"item\">{{ item }}</span>\n  </div>\n  <div v-else>empty</div>\n</template>",
        "v-if with v-for inside + v-else",
    );
}

#[test]
fn jsx_valid_multiple_v_if_chains() {
    assert_valid_jsx(
        "<template>\n  <div v-if=\"a\">A</div>\n  <div v-else>not A</div>\n  <div v-if=\"b\">B</div>\n  <div v-else>not B</div>\n</template>",
        "multiple separate v-if chains with whitespace",
    );
}

#[test]
fn jsx_valid_all_directives_combined() {
    assert_valid_jsx(
        "<template>\n  <div v-if=\"hasItems\">\n    <MyComp v-for=\"item in items\" :key=\"item.id\" v-slot=\"{ row }\">\n      <span v-if=\"row.active\">{{ row.name }}</span>\n      <span v-else>inactive</span>\n    </MyComp>\n  </div>\n  <div v-else>no items</div>\n</template>",
        "v-if + v-for + v-slot + nested v-if/v-else",
    );
}

// ===================================================================
// ===================================================================

#[test]
fn v_show_with_ref_binding_gets_prefix() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-show="visible">hi</div></template>"#,
        &[("visible", BindingType::SetupRef)],
    );
    assert!(
        result.contains("visible") && !result.contains("visible.value"),
        "v-show ref binding should be bare identifier in TSX mode (no .value). Got: {}",
        result
    );
    assert!(
        result.contains("display:"),
        "v-show should produce style display. Got: {}",
        result
    );
    assert!(
        !result.contains("v-show"),
        "v-show attribute must be removed. Got: {}",
        result
    );
}

#[test]
fn v_show_with_props_binding_gets_prefix() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-show="isVisible">hi</div></template>"#,
        &[("isVisible", BindingType::Props)],
    );
    assert!(
        result.contains("__props.isVisible"),
        "v-show props binding should have __props. prefix. Got: {}",
        result
    );
}

#[test]
fn v_show_compound_expr_resolves_all_bindings() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-show="isAdmin && visible">hi</div></template>"#,
        &[
            ("isAdmin", BindingType::Props),
            ("visible", BindingType::SetupRef),
        ],
    );
    assert!(
        result.contains("__props.isAdmin"),
        "v-show should resolve isAdmin as props. Got: {}",
        result
    );
    assert!(
        result.contains("visible") && !result.contains("visible.value"),
        "v-show should resolve visible as bare identifier in TSX mode (no .value). Got: {}",
        result
    );
}

#[test]
fn v_show_with_existing_style_no_duplicate_attributes() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-show="ready" :style="itemStyle">hi</div></template>"#,
        &[
            ("ready", BindingType::SetupRef),
            ("itemStyle", BindingType::SetupConst),
        ],
    );
    // Should NOT produce duplicate `style` attributes
    let style_count = result.matches("style=").count();
    assert_eq!(
        style_count, 1,
        "v-show + :style should merge into one style attribute, not produce {} style= occurrences. Got: {}",
        style_count, result
    );
    // Should include both the v-show display logic and the existing style
    assert!(
        result.contains("display:"),
        "merged style should include v-show display logic. Got: {}",
        result
    );
    // Should NOT have v-show attribute
    assert!(
        !result.contains("v-show"),
        "v-show attribute must be removed. Got: {}",
        result
    );
}

// ── v-model in TSX ────────────────────────────────────────────

#[test]
fn v_model_basic_component() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><Comp v-model="count" /></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    assert!(
        result.contains("modelValue={count}"),
        "v-model should produce modelValue prop. Got: {}",
        result
    );
    assert!(
        result.contains("\"onUpdate:modelValue\""),
        "v-model should produce onUpdate:modelValue handler. Got: {}",
        result
    );
    // Must use spread syntax (bare quoted attribute is invalid JSX)
    assert!(
        !result.contains("\"onUpdate:modelValue\"={"),
        "onUpdate handler must NOT be a bare JSX attribute. Got: {}",
        result
    );
    assert!(
        !result.contains("v-model"),
        "v-model attribute must be removed from JSX. Got: {}",
        result
    );
}

#[test]
fn v_model_named() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><Comp v-model:title="title" /></template>"#,
        &[("title", BindingType::SetupRef)],
    );
    assert!(
        result.contains("title={title}"),
        "named v-model should produce named prop. Got: {}",
        result
    );
    assert!(
        result.contains("\"onUpdate:title\""),
        "named v-model should produce onUpdate:title handler. Got: {}",
        result
    );
    // Must use spread syntax (bare quoted attribute is invalid JSX)
    assert!(
        !result.contains("\"onUpdate:title\"={"),
        "named onUpdate handler must NOT be a bare JSX attribute. Got: {}",
        result
    );
}

/// `v-model:show` on a COMPONENT — the generated `show=` prop NAME token must map
/// back to the source `show` arg span so a TypeProvider can resolve the child
/// component's `$props['show']` and hover lands on the directive arg. Pre-change
/// the static-arg prop name was emitted as unmapped synthetic text
/// (`Piece::Syn("show={")`) and the whole `v-model:show="x"` span was overwritten,
/// so the arg token had ZERO source→TSX mapping. Baseline against the working
/// `:show` bind name mapping (`v_bind_shorthand_title_source_map_accuracy`).
#[test]
fn vmodel_named_component_prop_name_maps_to_arg() {
    let source = r#"<template><Comp v-model:show="x" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("x", BindingType::SetupConst)]);

    assert!(
        output.contains("show={"),
        "named v-model should emit `show={{...}}`: {output}"
    );

    // Source col of the `show` arg token (`v-model:show` → after the colon).
    let arg_src_col = source.find("v-model:show").unwrap() as u32 + "v-model:".len() as u32;
    // The generated prop NAME `show=` — locate the prop-name occurrence (the one
    // immediately followed by `={`).
    let name_gen_col = output.find("show={").unwrap() as u32;
    let (name_gl, name_gc) = gen_offset_to_line_col(&output, name_gen_col as usize);

    let has_correct = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == name_gl && dc == name_gc && sc == arg_src_col);
    assert!(
        has_correct,
        "generated `show` prop-name (gen {name_gl}:{name_gc}) must map back to the \
         source arg col {arg_src_col} (the `s` in `show`). Tokens: {tokens:?}, output: {output}"
    );
}

/// `v-model:my-arg` on a COMPONENT — the generated prop name is the camelCased
/// `myArg` (≠ the source `my-arg`), so preserve-in-place is impossible. The mapped
/// piece lets the generated token be `myArg` while owning the source `arg_span`.
/// The mapped token (at the prop-name start) must point back into the `my-arg`
/// source token. Pre-change there was no mapping at all.
#[test]
fn vmodel_named_component_kebab_prop_name_maps_to_arg() {
    let source = r#"<template><Comp v-model:my-arg="x" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("x", BindingType::SetupConst)]);

    assert!(
        output.contains("myArg={"),
        "named v-model:my-arg should camelCase to `myArg={{...}}`: {output}"
    );

    let arg_src_col = source.find("v-model:my-arg").unwrap() as u32 + "v-model:".len() as u32;
    let name_gen_col = output.find("myArg={").unwrap() as u32;
    let (name_gl, name_gc) = gen_offset_to_line_col(&output, name_gen_col as usize);

    // The mapped prop-name token starts at the generated `myArg` and points back
    // into the `my-arg` source token (InsertMapped is linear-run, so it anchors at
    // the arg start; char-perfect kebab→camel is not required — the whole source
    // token is covered by the source-owned hover).
    let has_correct = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == name_gl && dc == name_gc && sc == arg_src_col);
    assert!(
        has_correct,
        "generated `myArg` prop-name (gen {name_gl}:{name_gc}) must map back to the \
         source arg col {arg_src_col} (the `m` in `my-arg`). Tokens: {tokens:?}, output: {output}"
    );
}

/// REGRESSION: the bound VALUE (`x`) and the `onUpdate:show` handler emissions
/// must be UNCHANGED by the mapped prop-name piece. The value still maps back to
/// its source span, and the synthetic `onUpdate:show` event key must NOT carry a
/// duplicate mapping to the arg span (which would make hover land on the event
/// key instead of the prop).
#[test]
fn vmodel_named_component_value_and_onupdate_unchanged() {
    let source = r#"<template><Comp v-model:show="x" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("x", BindingType::SetupConst)]);

    // The bound value `x` still maps back to source.
    let value_src_col = source.find("\"x\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, value_src_col),
        "bound value `x` must still map back to source col {value_src_col}. Tokens: {tokens:?}"
    );

    // The onUpdate handler key is still synthetic/unmapped.
    assert!(
        output.contains("\"onUpdate:show\""),
        "named v-model should still produce onUpdate:show handler: {output}"
    );
    let onupdate_gen = output.find("onUpdate:show").unwrap();
    let (ul, uc) = gen_offset_to_line_col(&output, onupdate_gen);
    assert!(
        !has_token_at_gen(&tokens, ul, uc),
        "onUpdate:show event key (gen {ul}:{uc}) must NOT carry a mapping (no duplicate \
         arg provenance). Tokens: {tokens:?}"
    );

    // The arg span must be mapped EXACTLY ONCE (only the prop-name piece) — never a
    // second time onto the onUpdate key. Count distinct generated positions that map
    // to the arg source col.
    let arg_src_col = source.find("v-model:show").unwrap() as u32 + "v-model:".len() as u32;
    let arg_mappings = tokens
        .iter()
        .filter(|&&(_, _, sc)| sc == arg_src_col)
        .count();
    assert_eq!(
        arg_mappings, 1,
        "the source arg col {arg_src_col} must be mapped exactly once (the prop name), \
         not duplicated onto onUpdate/modifier keys. Tokens: {tokens:?}"
    );
}

/// REGRESSION: native `<input v-model:foo="x">` must NOT map the static arg to the
/// generated DOM prop (`value`/`checked`) — native v-model DOM props are
/// compiler-synthesized, not a `$props` surface, so mapping would be false
/// provenance. The DOM-prop name must carry NO mapping to the arg span.
#[test]
fn vmodel_native_arg_not_mapped_to_dom_prop() {
    // Native input with a NAMED arg (`v-model:foo`): the named-arg-on-native branch
    // is the actual risk the `!is_native` guard defends — a default `v-model` (no
    // arg) never had a name to map, so it passes trivially. With a named arg, the
    // arg `foo` is meaningless on a native element (the DOM prop is the synthesized
    // `value`/`checked`, NOT a `$props` surface), so `static_arg_span` stays `None`
    // and the codegen must NOT fabricate a mapping onto the synthesized DOM prop.
    let source = r#"<template><input v-model:foo="x"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("x", BindingType::SetupConst)]);

    // A native element ignores the named arg: the DOM prop is still `value`, NOT a
    // camelCased `foo` prop. The arg name must not leak into a `$props`-style prop.
    assert!(
        output.contains("value={"),
        "native v-model:foo should still emit the synthesized DOM prop value={{...}}: {output}"
    );
    assert!(
        !output.contains("foo={"),
        "native v-model:foo must NOT synthesize a `foo` component prop: {output}"
    );

    // The generated DOM prop name (`value`) must NOT carry a mapped token — it is
    // compiler-synthesized, not a source token, so the `!is_native` guard must keep
    // `static_arg_span = None` and emit no `MappedStaticModelPropName` piece.
    let value_gen = output.find("value={").unwrap();
    let (vl, vc) = gen_offset_to_line_col(&output, value_gen);
    assert!(
        !has_token_at_gen(&tokens, vl, vc),
        "native DOM prop `value` (gen {vl}:{vc}) must NOT carry a mapped token \
         (false provenance). Tokens: {tokens:?}, output: {output}"
    );

    // Strongest: the source `foo` arg span must NOT be mapped anywhere in the
    // generated TSX (no `MappedStaticModelPropName` emission for a native element).
    let arg_src_col = source.find("v-model:foo").unwrap() as u32 + "v-model:".len() as u32;
    assert!(
        !has_token_for_src(&tokens, arg_src_col),
        "native v-model:foo arg (source col {arg_src_col}) must carry NO mapped token \
         — `!is_native` keeps `static_arg_span = None`. Tokens: {tokens:?}, output: {output}"
    );
}

/// MULTIPLE `v-model` on one COMPONENT — each generated prop NAME must map
/// INDEPENDENTLY back to its OWN source arg span. The `v_model_hover` loop walks
/// every directive and the per-directive mapped prop-name piece must anchor at
/// the matching arg, so `v-model:a` → source `a` and `v-model:b` → source `b`
/// (never both onto one arg, never a cross-wired mapping).
#[test]
fn vmodel_multiple_named_args_map_independently() {
    let source = r#"<template><Comp v-model:a="x" v-model:b="y" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("x", BindingType::SetupConst),
            ("y", BindingType::SetupConst),
        ],
    );

    assert!(
        output.contains("a={") && output.contains("b={"),
        "both named v-models should emit `a={{...}}` and `b={{...}}`: {output}"
    );

    // Source cols of each arg (`v-model:a` / `v-model:b` → the char after the colon).
    let a_src_col = source.find("v-model:a").unwrap() as u32 + "v-model:".len() as u32;
    let b_src_col = source.find("v-model:b").unwrap() as u32 + "v-model:".len() as u32;
    assert_ne!(
        a_src_col, b_src_col,
        "the two args must be distinct source spans"
    );

    // Generated prop-name positions: the `a={` / `b={` prop-name tokens.
    let a_gen = output.find("a={").unwrap();
    let b_gen = output.find("b={").unwrap();
    let (a_gl, a_gc) = gen_offset_to_line_col(&output, a_gen);
    let (b_gl, b_gc) = gen_offset_to_line_col(&output, b_gen);

    // `a` prop name maps to the `a` arg span (and ONLY that one).
    assert!(
        tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == a_gl && dc == a_gc && sc == a_src_col),
        "generated `a` prop-name (gen {a_gl}:{a_gc}) must map to source arg col {a_src_col}. \
         Tokens: {tokens:?}, output: {output}"
    );
    // `b` prop name maps to the `b` arg span (and ONLY that one).
    assert!(
        tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == b_gl && dc == b_gc && sc == b_src_col),
        "generated `b` prop-name (gen {b_gl}:{b_gc}) must map to source arg col {b_src_col}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // Each arg span is mapped EXACTLY ONCE (no cross-wiring onto the other prop name
    // and no duplicate onto an onUpdate/modifier key).
    let a_mappings = tokens.iter().filter(|&&(_, _, sc)| sc == a_src_col).count();
    let b_mappings = tokens.iter().filter(|&&(_, _, sc)| sc == b_src_col).count();
    assert_eq!(
        a_mappings, 1,
        "arg `a` (col {a_src_col}) must be mapped exactly once. Tokens: {tokens:?}"
    );
    assert_eq!(
        b_mappings, 1,
        "arg `b` (col {b_src_col}) must be mapped exactly once. Tokens: {tokens:?}"
    );
}

/// MODIFIER on a named component `v-model` — the prop NAME still maps to the arg
/// span, and the modifier name (`trim`) falls OUTSIDE the source-owned prop-name
/// hover range and emits through the UNCHANGED `Piece::Modifier` path (its own
/// mapped token in the `showModifiers={{ ... }}` prop).
#[test]
fn vmodel_named_component_with_modifier_maps_prop_name_and_modifier() {
    let source = r#"<template><Comp v-model:show.trim="x" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("x", BindingType::SetupConst)]);

    assert!(
        output.contains("show={"),
        "named v-model with modifier should still emit `show={{...}}`: {output}"
    );
    assert!(
        output.contains("showModifiers={{"),
        "the `.trim` modifier should emit a `showModifiers` prop: {output}"
    );
    assert!(
        output.contains("trim"),
        "the modifiers prop should contain `trim`: {output}"
    );

    // The prop NAME still maps to the source `show` arg span.
    let arg_src_col = source.find("v-model:show").unwrap() as u32 + "v-model:".len() as u32;
    let name_gen = output.find("show={").unwrap();
    let (name_gl, name_gc) = gen_offset_to_line_col(&output, name_gen);
    assert!(
        tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == name_gl && dc == name_gc && sc == arg_src_col),
        "generated `show` prop-name (gen {name_gl}:{name_gc}) must map to source arg col \
         {arg_src_col}. Tokens: {tokens:?}, output: {output}"
    );

    // The modifier name `trim` maps through the UNCHANGED `Piece::Modifier` path to
    // its OWN source span (the `trim` after the dot) — outside the prop-name token.
    let trim_src = source.find(".trim").unwrap() as u32 + 1;
    assert_ne!(
        trim_src, arg_src_col,
        "the modifier span must be distinct from the arg span"
    );
    assert!(
        has_token_for_src(&tokens, trim_src),
        "modifier `trim` must map to its own source col {trim_src} (unchanged \
         Piece::Modifier path). Tokens: {tokens:?}, output: {output}"
    );
}

/// KEBAB component tag + KEBAB arg — `<my-comp v-model:my-arg="x"/>`. The generated
/// prop name camelCases to `myArg`, and that token must map back to the SOURCE
/// `my-arg` arg span (a PascalCase/kebab component tag is still a component, so the
/// `!is_native` branch emits the mapped prop-name piece).
#[test]
fn vmodel_kebab_component_named_arg_maps_to_arg() {
    let source = r#"<template><my-comp v-model:my-arg="x"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("x", BindingType::SetupConst)]);

    assert!(
        output.contains("myArg={"),
        "kebab component v-model:my-arg should camelCase to `myArg={{...}}`: {output}"
    );

    let arg_src_col = source.find("v-model:my-arg").unwrap() as u32 + "v-model:".len() as u32;
    let name_gen = output.find("myArg={").unwrap();
    let (name_gl, name_gc) = gen_offset_to_line_col(&output, name_gen);
    assert!(
        tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == name_gl && dc == name_gc && sc == arg_src_col),
        "generated `myArg` prop-name (gen {name_gl}:{name_gc}) must map back to source arg \
         col {arg_src_col} (the `m` in `my-arg`). Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn v_model_with_binding_resolution() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><Comp v-model="count" /></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    assert!(
        result.contains("modelValue={count}") && !result.contains("count.value"),
        "v-model on ref should resolve to bare identifier in TSX mode (no .value). Got: {}",
        result
    );
}

#[test]
fn v_model_on_native_element() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><input v-model="msg" /></template>"#,
        &[("msg", BindingType::SetupRef)],
    );
    // Native input should use `value` (not `modelValue`) and native event handler
    assert!(
        result.contains("value={msg}"),
        "v-model on native input should produce value prop. Got: {}",
        result
    );
    assert!(
        !result.contains("modelValue"),
        "v-model on native input must NOT use modelValue. Got: {}",
        result
    );
    assert!(
        result.contains("onInput={"),
        "v-model on native input should use onInput event. Got: {}",
        result
    );
    // Must not have any quoted attribute names (invalid JSX)
    assert!(
        !result.contains(r#""onUpdate:"#),
        "native input must not have quoted onUpdate attribute. Got: {}",
        result
    );
    assert!(
        !result.contains("v-model"),
        "v-model attribute must be removed. Got: {}",
        result
    );
}

#[test]
fn v_model_with_explicit_change_handler_no_duplicate() {
    // v-model on <input type="checkbox"> + explicit @change should not produce
    // duplicate onChange attributes (TS17001).
    let result = gen_tsx_template_with_bindings(
        r#"<template><input v-model="model" type="checkbox" @change="handleChange" /></template>"#,
        &[
            ("model", BindingType::SetupRef),
            ("handleChange", BindingType::SetupConst),
        ],
    );
    let on_change_count = result.matches("onChange=").count()
        + result.matches("onChange:").count()
        + result.matches("\"onChange\"").count();
    assert_eq!(
        on_change_count, 1,
        "v-model + @change on native input should produce exactly one onChange. Got {} in: {}",
        on_change_count, result
    );
    assert!(
        !result.contains("v-model"),
        "v-model attribute must be removed. Got: {}",
        result
    );
}

#[test]
fn v_model_with_explicit_input_handler_no_duplicate() {
    // v-model on text <input> + explicit @input should not produce
    // duplicate onInput attributes.
    let result = gen_tsx_template_with_bindings(
        r#"<template><input v-model="text" @input="onInput" /></template>"#,
        &[
            ("text", BindingType::SetupRef),
            ("onInput", BindingType::SetupConst),
        ],
    );
    let on_input_count = result.matches("onInput=").count();
    assert_eq!(
        on_input_count, 1,
        "v-model + @input on text input should produce exactly one onInput. Got {} in: {}",
        on_input_count, result
    );
    // v-model should still produce the value prop
    assert!(
        result.contains("value={text}"),
        "v-model should still produce value prop. Got: {}",
        result
    );
}

#[test]
fn v_model_with_explicit_checked_prop_no_duplicate() {
    // v-model on <input type="radio"> + explicit :checked + @change should not
    // produce duplicate checked or onChange attributes.
    let result = gen_tsx_template_with_bindings(
        r#"<template><input v-model="modelValue" type="radio" :checked="modelValue === val" @change="handleChange" /></template>"#,
        &[
            ("modelValue", BindingType::SetupRef),
            ("val", BindingType::SetupConst),
            ("handleChange", BindingType::SetupConst),
        ],
    );
    let checked_count = result.matches("checked=").count();
    let on_change_count = result.matches("onChange=").count();
    assert_eq!(
        checked_count, 1,
        "v-model + :checked on radio should produce one checked attr. Got {} in: {}",
        checked_count, result
    );
    assert_eq!(
        on_change_count, 1,
        "v-model + @change on radio should produce one onChange. Got {} in: {}",
        on_change_count, result
    );
}

#[test]
fn duplicate_keydown_handlers_use_spread_for_second() {
    // @keydown.space + @keydown.enter both map to onKeyDown —
    // the second must use spread syntax to avoid TS17001.
    let result = gen_tsx_template_with_bindings(
        r#"<template><td @keydown.space.prevent.stop="handleClick" @keydown.enter.prevent.stop="handleClick" /></template>"#,
        &[("handleClick", BindingType::SetupConst)],
    );
    let on_keydown_attr = result.matches("onKeyDown={").count();
    assert!(
        on_keydown_attr <= 1,
        "should have at most one onKeyDown= attribute (rest as spread). Got {} in: {}",
        on_keydown_attr,
        result
    );
    // Should still reference both handlers somehow
    assert!(
        result.contains("handleClick"),
        "handler reference should be present. Got: {}",
        result
    );
}

#[test]
fn self_closing_template_v_if_produces_valid_jsx() {
    // <template v-if="..." /> is self-closing with no children.
    // The IIFE wrapping must produce valid JSX (empty fragment or null).
    let result = gen_tsx_template_with_bindings(
        r#"<template><template v-if="noFooter" /><template v-else><div>footer</div></template></template>"#,
        &[("noFooter", BindingType::SetupConst)],
    );
    // Positive: should have the v-if condition
    assert!(
        result.contains("noFooter"),
        "v-if condition should be present. Got: {}",
        result
    );
    // Negative: no unclosed fragments — count <> and </> should match
    let open_frags = result.matches("<>").count();
    let close_frags = result.matches("</>").count();
    assert_eq!(
        open_frags, close_frags,
        "fragment open/close count should match. Got {} opens and {} closes in: {}",
        open_frags, close_frags, result
    );
}

#[test]
fn multiline_static_style_merged_with_dynamic_no_unterminated_string() {
    // When static style has newlines and is merged with :style, the static value
    // must not produce an unterminated JS string literal inside normalizeStyle.
    let result = gen_tsx_template_with_bindings(
        "<template><div style=\"\n  position: absolute;\n  top: 0;\n\" :style=\"{ height: h + 'px' }\">hi</div></template>",
        &[("h", BindingType::SetupConst)],
    );
    // Positive: should have normalizeStyle call
    assert!(
        result.contains("normalizeStyle"),
        "merged style should use normalizeStyle. Got: {}",
        result
    );
    // Negative: the static string inside normalizeStyle must NOT have literal newlines
    // (which would be unterminated string literal TS1002)
    let norm_idx = result.find("normalizeStyle").unwrap();
    let after_norm = &result[norm_idx..];
    // Find the string literal inside the normalizeStyle call
    if let Some(quote_idx) = after_norm.find(",\"") {
        let after_quote = &after_norm[quote_idx + 2..];
        let end_quote = after_quote.find('"').unwrap_or(after_quote.len());
        let static_str = &after_quote[..end_quote];
        assert!(
            !static_str.contains('\n'),
            "static style string must not contain newlines. Got: {}",
            static_str
        );
    }
}

// ── Slot outlets in TSX ────────────────────────────────────────

#[test]
fn slot_outlet_default() {
    let result = gen_tsx_template(r#"<template><slot /></template>"#);
    assert!(
        result.contains("___VERTER___instance.$slots.default?.()"),
        "Default slot outlet should produce ___VERTER___instance.$slots.default?.(). Got: {}",
        result
    );
    assert!(
        !result.contains("<slot"),
        "<slot> tag must be replaced. Got: {}",
        result
    );
    assert!(
        !result.contains("{ $slots.default"),
        "Bare $slots without instance prefix must not appear. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_named() {
    let result = gen_tsx_template(r#"<template><slot name="header" /></template>"#);
    assert!(
        result.contains("___VERTER___instance.$slots.header?.()"),
        "Named slot outlet should produce ___VERTER___instance.$slots.header?.(). Got: {}",
        result
    );
    assert!(
        !result.contains("{ $slots.header"),
        "Bare $slots without instance prefix must not appear. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_with_props() {
    let result = gen_tsx_template(r#"<template><slot name="item" :data="itemData" /></template>"#);
    assert!(
        result.contains("___VERTER___instance.$slots.item"),
        "Slot call should reference ___VERTER___instance.$slots.item. Got: {}",
        result
    );
    assert!(
        result.contains("data: ___VERTER___instance.itemData")
            || result.contains("data:___VERTER___instance.itemData"),
        "Slot props should include data binding with instance prefix (unresolved). Got: {}",
        result
    );
}

#[test]
fn slot_outlet_with_fallback() {
    let result = gen_tsx_template(r#"<template><slot>fallback</slot></template>"#);
    assert!(
        result.contains("___VERTER___instance.$slots.default?.()"),
        "Slot with fallback should have ___VERTER___instance.$slots call. Got: {}",
        result
    );
    assert!(
        result.contains("??"),
        "Slot with fallback should use ?? operator. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_hyphenated_name() {
    let result = gen_tsx_template(r#"<template><slot name="overlay-content" /></template>"#);
    assert!(
        result.contains("$slots['overlay-content']"),
        "Hyphenated slot name must use bracket notation. Got: {}",
        result
    );
    assert!(
        !result.contains("$slots.overlay-content"),
        "Must NOT use dot notation for hyphenated names (parses as subtraction). Got: {}",
        result
    );
    assert!(
        !result.contains("<slot"),
        "<slot> tag must be replaced. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_hyphenated_name_with_props() {
    let result = gen_tsx_template(r#"<template><slot name="item-data" :value="x" /></template>"#);
    assert!(
        result.contains("$slots['item-data']"),
        "Hyphenated slot name with props must use bracket notation. Got: {}",
        result
    );
    assert!(
        result.contains("value:") || result.contains("value :"),
        "Slot props should be present. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_dotted_name() {
    let result = gen_tsx_template(r#"<template><slot name="foo.bar" /></template>"#);
    assert!(
        result.contains("$slots['foo.bar']"),
        "Dotted slot name must use bracket notation. Got: {}",
        result
    );
    assert!(
        !result.contains("$slots.foo.bar"),
        "Must NOT use dot notation for dotted names. Got: {}",
        result
    );
}

#[test]
fn slot_outlet_hyphenated_name_with_fallback() {
    let result =
        gen_tsx_template(r#"<template><slot name="overlay-content">fallback</slot></template>"#);
    assert!(
        result.contains("$slots['overlay-content']"),
        "Hyphenated slot name with fallback must use bracket notation. Got: {}",
        result
    );
    assert!(
        result.contains("??"),
        "Slot with fallback should use ?? operator. Got: {}",
        result
    );
}

// ── Instance property resolution in TSX ─────────────────────────

#[test]
fn tsx_unresolved_dollar_emit_gets_instance_prefix() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ $emit('click') }}</div></template>"#,
        &[],
    );
    assert!(
        result.contains("___VERTER___instance.$emit"),
        "Unresolved $emit should get instance prefix. Got: {}",
        result
    );
    assert!(
        !result.contains("{ $emit(") && !result.contains("{$emit("),
        "Bare $emit without prefix must not appear. Got: {}",
        result
    );
}

#[test]
fn tsx_unresolved_dollar_attrs_gets_instance_prefix() {
    let result =
        gen_tsx_template_with_bindings(r#"<template><div>{{ $attrs }}</div></template>"#, &[]);
    assert!(
        result.contains("___VERTER___instance.$attrs"),
        "Unresolved $attrs should get instance prefix. Got: {}",
        result
    );
}

#[test]
fn tsx_known_setup_binding_stays_bare() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ count }}</div></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    assert!(
        !result.contains("___VERTER___instance.count"),
        "Known binding should NOT get instance prefix. Got: {}",
        result
    );
}

#[test]
fn tsx_props_binding_stays_dunder_props() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ msg }}</div></template>"#,
        &[("msg", BindingType::Props)],
    );
    assert!(
        result.contains("__props.msg"),
        "Props binding should use __props. Got: {}",
        result
    );
    assert!(
        !result.contains("___VERTER___instance.msg"),
        "Props binding should NOT get instance prefix. Got: {}",
        result
    );
}

// ── Dynamic event names in TSX ────────────────────────────────

#[test]
fn dynamic_event_name() {
    let result = gen_tsx_template(r#"<template><div @[eventName]="handler" /></template>"#);
    assert!(
        result.contains("eventName") || result.contains("_ctx.eventName"),
        "Dynamic event should reference eventName. Got: {}",
        result
    );
    assert!(
        !result.contains("@["),
        "Dynamic event syntax must be removed. Got: {}",
        result
    );
}

#[test]
fn dynamic_event_name_with_binding() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @[eventName]="handler" /></template>"#,
        &[("eventName", BindingType::SetupRef)],
    );
    assert!(
        result.contains("eventName") && !result.contains("eventName.value"),
        "Dynamic event name on ref should be bare identifier in TSX mode (no .value). Got: {}",
        result
    );
}

// ── v-for source mapping (#19) ──────────────────────────────────

/// Helper: generate TSX template with bindings AND return source map tokens.
/// Returns (output_string, Vec<(dst_line, dst_col, src_col)>).
fn gen_tsx_template_with_map(
    source: &str,
    bindings: &[(&str, BindingType)],
) -> (String, Vec<(u32, u32, u32)>) {
    let alloc = Allocator::new();
    let bytes = source.as_bytes();

    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });

    let template_ast = match syntax.take_template_ast() {
        Some(ast) => ast,
        None => return (String::new(), Vec::new()),
    };

    let source_type = oxc_span::SourceType::tsx();
    let oxc_ast = crate::template::oxc::parse_template_expressions(
        &template_ast,
        source,
        &alloc,
        source_type,
        true,
    );

    let tpl_alloc = Allocator::new();
    let mut tpl_ct = CodeTransform::new(source, &tpl_alloc);
    let mut out = CodeGenOutput::new(&tpl_alloc);
    let binding_map: FxHashMap<&str, BindingType> = bindings.iter().copied().collect();
    let options = IdeTemplateOptions {
        self_name: "App",
        comments: true,
        is_jsx: false,
        strict_slots: false,
    };

    generate_ide_template(
        &template_ast,
        &oxc_ast,
        source,
        &mut out,
        &tpl_alloc,
        &binding_map,
        &options,
        &TemplateComponentBindings::default(),
    );
    out.apply_to(&mut tpl_ct);

    let full = tpl_ct.build_string();
    let map =
        tpl_ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("test.vue"));
    let tokens: Vec<(u32, u32, u32)> = map
        .get_tokens()
        .filter(|t| t.get_source_id().is_some())
        .map(|t| (t.get_dst_line(), t.get_dst_col(), t.get_src_col()))
        .collect();

    (full, tokens)
}

#[test]
fn v_for_iterable_is_source_mapped() {
    // v-for="item in items" — the iterable `items` in the .map() wrapper
    // should have a source map token pointing back to the original `items` position.
    let source = r#"<template><div v-for="item in items">{{ item }}</div></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Verify output shape
    assert!(
        output.contains(".map("),
        "v-for should produce .map() wrapper: {output}"
    );

    // Find the byte offset of "items" in the v-for attribute value
    let items_src_offset = source.find("item in items").unwrap() + "item in ".len();

    // There should be a source map token pointing to the iterable position
    let has_iterable_token = tokens
        .iter()
        .any(|&(_, _, src_col)| src_col == items_src_offset as u32);
    assert!(
        has_iterable_token,
        "v-for iterable should have source map token at src col {}. Tokens: {:?}",
        items_src_offset, tokens
    );
}

#[test]
fn v_for_param_is_source_mapped() {
    // The iteration parameter `item` in .map((item) => ...) should map back
    // to the parameter position in the v-for attribute value.
    let source = r#"<template><div v-for="item in items">{{ item }}</div></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    assert!(
        output.contains(".map((item"),
        "v-for should produce .map((item...) => ...): {output}"
    );

    // "item" starts right after the opening quote of v-for="
    let param_src_offset = source.find("item in items").unwrap();

    let has_param_token = tokens
        .iter()
        .any(|&(_, _, src_col)| src_col == param_src_offset as u32);
    assert!(
        has_param_token,
        "v-for parameter should have source map token at src col {}. Tokens: {:?}",
        param_src_offset, tokens
    );
}

#[test]
fn component_is_dynamic_expr_is_source_mapped() {
    // <component :is="currentView"> should emit a source-mapped temp variable
    // so TSGO can provide hover info on `currentView`.
    let source = r#"<template><component :is="currentView">hello</component></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("currentView", BindingType::SetupRef)]);

    // The output should contain the temp variable with the expression
    assert!(
        output.contains("currentView"),
        "output should contain `currentView`: {output}"
    );

    // Find the byte offset of "currentView" in the :is attribute value
    let expr_src_offset = source.find("currentView").unwrap();

    // There should be a source map token pointing back to the expression
    let has_expr_token = tokens
        .iter()
        .any(|&(_, _, src_col)| src_col == expr_src_offset as u32);
    assert!(
        has_expr_token,
        "component :is expression should have source map token at src col {}. Tokens: {:?}",
        expr_src_offset, tokens
    );
}

#[test]
fn component_is_dynamic_resolves_bindings() {
    // <component :is="currentView"> with SetupRef binding should resolve
    // the expression through the BindingResolver (e.g., `currentView.value`
    // for refs in non-inline mode, or just `currentView` for inline).
    let source = r#"<template><component :is="currentView">hello</component></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("currentView", BindingType::SetupRef)]);

    // With inline mode (default for TSX), SetupRef bindings are used directly.
    // The expression should be present in the output (not _ctx. prefixed since inline).
    assert!(
        output.contains("currentView"),
        "output should contain resolved `currentView`: {output}"
    );
    // The `:is` attribute itself should be removed
    assert!(
        !output.contains(":is="),
        "`:is` attribute should be removed from output: {output}"
    );
    // The `component` tag should be rewritten
    assert!(
        !output.contains("<component"),
        "`<component` tag should be rewritten: {output}"
    );
}

#[test]
fn component_is_dynamic_resolves_data_binding() {
    // In TSX mode, Data bindings use ___VERTER___instance. prefix (no _ctx. prefix).
    let source = r#"<template><component :is="currentView">hello</component></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("currentView", BindingType::Data)]);

    assert!(
        output.contains("___VERTER___instance.currentView") && !output.contains("_ctx.currentView"),
        "Data binding should use instance prefix in TSX mode: {output}"
    );
    assert!(
        !output.contains(":is="),
        "`:is` attribute should be removed from output: {output}"
    );
}

// ── Data/Options binding instance prefix in TSX mode ─────────────

#[test]
fn data_binding_uses_instance_prefix() {
    let source = r#"<template><div>{{ count }}</div></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("count", BindingType::Data)]);

    // Positive: Data bindings should use ___VERTER___instance. prefix
    assert!(
        output.contains("___VERTER___instance.count"),
        "Data binding should use instance prefix in TSX mode: {output}"
    );
    // Negative: should NOT contain bare `{count}` without instance prefix
    assert!(
        !output.contains("{count}"),
        "Data binding should not be bare — must use instance prefix: {output}"
    );
}

#[test]
fn options_binding_uses_instance_prefix() {
    let source = r#"<template><div>{{ total }}</div></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("total", BindingType::Options)]);

    // Positive: Options bindings should use ___VERTER___instance. prefix
    assert!(
        output.contains("___VERTER___instance.total"),
        "Options binding should use instance prefix in TSX mode: {output}"
    );
}

#[test]
fn event_handler_simple_ident_is_source_mapped() {
    // @click="handler" — the handler identifier should have a source map token.
    let source = r#"<template><button @click="handler">click</button></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("handler", BindingType::SetupConst)]);

    assert!(
        output.contains("onClick={handler}"),
        "should emit onClick={{handler}}: {output}"
    );

    // Find the byte offset of "handler" in the @click value
    let handler_src_offset = source.find("handler").unwrap();

    let has_handler_token = tokens
        .iter()
        .any(|&(_, _, src_col)| src_col == handler_src_offset as u32);
    assert!(
        has_handler_token,
        "event handler should have source map token at src col {}. Tokens: {:?}",
        handler_src_offset, tokens
    );
}

#[test]
fn event_handler_fn_expr_is_source_mapped() {
    // @click="(e) => doSomething(e)" — the expression should be source-mapped.
    let source = r#"<template><button @click="(e) => doSomething(e)">click</button></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("doSomething", BindingType::SetupConst)]);

    assert!(
        output.contains("onClick={(e) => doSomething(e)}"),
        "should emit onClick with fn expr: {output}"
    );

    // Find the byte offset of the expression in the @click value
    let expr_src_offset = source.find("(e) => doSomething").unwrap();

    let has_expr_token = tokens
        .iter()
        .any(|&(_, _, src_col)| src_col == expr_src_offset as u32);
    assert!(
        has_expr_token,
        "fn expression should have source map token at src col {}. Tokens: {:?}",
        expr_src_offset, tokens
    );
}

#[test]
fn event_handler_inline_expr_is_source_mapped() {
    // @click="count++" — the inline expression should be source-mapped.
    // Using SetupConst to avoid .value transformation changing the text.
    let source = r#"<template><button @click="count++">click</button></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("count", BindingType::SetupConst)]);

    assert!(
        output.contains("count++"),
        "should contain the expression: {output}"
    );

    // Find byte offset of "count++" in the @click value
    let expr_src_offset = source.find("count++").unwrap();

    let has_expr_token = tokens
        .iter()
        .any(|&(_, _, src_col)| src_col == expr_src_offset as u32);
    assert!(
        has_expr_token,
        "inline expression should have source map token at src col {}. Tokens: {:?}",
        expr_src_offset, tokens
    );
}

/// The synthetic closing suffix of an in-place v-on handler (the `}}` wrapper
/// close for an inline expression, the `}` JSX-container close for a simple
/// handler) is compiler-synthesized scaffolding with no source token, so it must
/// map to None — consistent with the rest of the decomposed handler boundary
/// (prefix delete, scaffold-after-event, guard are all unmapped). A MAPPED
/// overwrite of the closing span would point the synthetic braces at the body
/// end (the close quote), which would land go-to-definition / hover on a
/// synthetic brace.
///
/// Discriminating: pre-fix the closing suffix was emitted via a mapped
/// `out.overwrite(trimmed_ve, prop_end, suffix)`, producing a token at the
/// suffix's generated column → this assertion FAILS. Post-fix the suffix is an
/// unmapped inserted chunk → no token at that column → PASSES. The generated TSX
/// text is byte-identical across the fix (pinned by
/// `event_handler_inline_expr_is_source_mapped` /
/// `event_handler_simple_ident_is_source_mapped`).
#[test]
fn v_on_handler_closing_suffix_maps_to_none() {
    // Inline expression: `@click="count++"` → `onClick={() => {count++}}`. The
    // trailing `}}` is the wrapper + container close — synthetic.
    let inline = r#"<template><button @click="count++">click</button></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(inline, &[("count", BindingType::SetupConst)]);
    assert!(
        output.contains("onClick={() => {count++}}"),
        "inline handler output must be byte-stable: {output}"
    );
    // The synthetic suffix is the FINAL `}}` run before the closing `>` of the tag.
    let gt = output.find("}}>").expect("expected `}}>` in output");
    let suffix_col = gt as u32; // generated column of the first `}` of the synthetic suffix
    assert!(
        !tokens.iter().any(|&(_, dst_col, _)| dst_col == suffix_col),
        "synthetic v-on closing suffix (gen col {suffix_col}) must NOT carry a source \
         map token (synthetic → None). Tokens: {tokens:?}"
    );

    // Simple handler: `@click="handler"` → `onClick={handler}`. The trailing `}`
    // is the JSX-container close — synthetic.
    let simple = r#"<template><button @click="handler">click</button></template>"#;
    let (o2, t2) = gen_tsx_template_with_map(simple, &[("handler", BindingType::SetupConst)]);
    assert!(
        o2.contains("onClick={handler}"),
        "simple handler output must be byte-stable: {o2}"
    );
    let gt2 = o2.find("}>").expect("expected `}>` in simple output");
    let suffix_col2 = gt2 as u32; // generated column of the synthetic `}` close
    assert!(
        !t2.iter().any(|&(_, dst_col, _)| dst_col == suffix_col2),
        "synthetic v-on closing `}}` (gen col {suffix_col2}) must NOT carry a source \
         map token (synthetic → None). Tokens: {t2:?}"
    );
}

// ── Bug 1: Dynamic <component :is> uses extractRenderComponent ──

#[test]
fn component_dynamic_is_uses_extract_render_component() {
    let source = r#"<template><component :is="'div'"></component></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("___VERTER___extractRenderComponent"),
        "should use extractRenderComponent wrapper: {output}"
    );
    assert!(
        output.contains("___VERTER___component_render"),
        "should use ___VERTER___component_render temp name: {output}"
    );
    assert!(
        output.contains("const ___VERTER___component_render=___VERTER___extractRenderComponent("),
        "should declare const with extractRenderComponent wrapper: {output}"
    );
    // Negative: old format should not appear
    assert!(
        !output.contains("__verter_component_render"),
        "old format __verter_component_render should not appear: {output}"
    );
    assert!(
        !output.contains("<component"),
        "<component tag should be rewritten: {output}"
    );
}

#[test]
fn component_dynamic_is_expression() {
    let source = r#"<template><component :is="as || 'div'"></component></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("as", BindingType::SetupRef)]);

    assert!(
        output.contains("___VERTER___extractRenderComponent("),
        "should use extractRenderComponent: {output}"
    );
    assert!(
        output.contains("<___VERTER___component_render"),
        "should rewrite opening tag: {output}"
    );
    assert!(
        output.contains("</___VERTER___component_render>"),
        "should rewrite closing tag: {output}"
    );
}

#[test]
fn component_static_is_unchanged() {
    let source = r#"<template><component is="div" tabindex="1"></component></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("<div"),
        "static is should rewrite to target tag: {output}"
    );
    assert!(
        !output.contains("extractRenderComponent"),
        "static is should not use extractRenderComponent: {output}"
    );
    assert!(
        !output.contains("<component"),
        "<component tag should be rewritten: {output}"
    );
}

#[test]
fn component_dynamic_is_removes_is_directive() {
    let source = r#"<template><component :is="tag" class="foo"></component></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("tag", BindingType::SetupRef)]);

    assert!(
        output.contains("class=\"foo\""),
        "class attribute should be preserved: {output}"
    );
    assert!(
        !output.contains(":is="),
        ":is= directive should be removed: {output}"
    );
}

// ── Bug 2: Class/Style merge ──

#[test]
fn class_merge_static_and_dynamic() {
    let source = r#"<template><div class="foo" :class="{bar: true}"/></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass: {output}"
    );
    assert!(
        output.contains("{bar: true}") && output.contains("\"foo\""),
        "should contain both class expressions: {output}"
    );
    // Count class= occurrences — should be exactly 1
    let class_count = output.matches("class=").count();
    assert_eq!(
        class_count, 1,
        "should have exactly 1 class= attribute, got {class_count}: {output}"
    );
}

#[test]
fn class_merge_with_prop_in_between() {
    let source =
        r#"<template><div class="foo" my-random-prop="true" :class="{bar: true}"/></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass: {output}"
    );
    assert!(
        output.contains("my-random-prop"),
        "should preserve other props: {output}"
    );
    let class_count = output.matches("class=").count();
    assert_eq!(
        class_count, 1,
        "should have exactly 1 class= attribute, got {class_count}: {output}"
    );
}

#[test]
fn style_merge_static_and_dynamic() {
    let source = r#"<template><div style="color:red" :style="{ bg: 'blue' }"/></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("normalizeStyle"),
        "should use normalizeStyle: {output}"
    );
    let style_count = output.matches("style=").count();
    assert_eq!(
        style_count, 1,
        "should have exactly 1 style= attribute, got {style_count}: {output}"
    );
}

#[test]
fn class_and_style_merge_combined() {
    let source = r#"<template><div class="a" :class="b" style="c" :style="d"/></template>"#;
    let output = gen_tsx_template_with_bindings(
        source,
        &[("b", BindingType::SetupRef), ("d", BindingType::SetupRef)],
    );

    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass: {output}"
    );
    assert!(
        output.contains("normalizeStyle"),
        "should use normalizeStyle: {output}"
    );
    let class_count = output.matches("class=").count();
    assert_eq!(
        class_count, 1,
        "should have exactly 1 class= attribute: {output}"
    );
    let style_count = output.matches("style=").count();
    assert_eq!(
        style_count, 1,
        "should have exactly 1 style= attribute: {output}"
    );
}

#[test]
fn style_object_literal_gets_css_properties_satisfies() {
    let source = r#"<template><div :style="{ color: 'red' }"/></template>"#;
    let output = gen_tsx_template(source);
    // Positive: object literal style should get CSSProperties satisfies annotation
    assert!(
        output.contains("satisfies") && output.contains("CSSProperties"),
        "object literal :style should have satisfies CSSProperties: {output}"
    );
    // Negative: non-object-literal style should NOT get satisfies
    let source2 = r#"<template><div :style="myVar"/></template>"#;
    let output2 = gen_tsx_template(source2);
    assert!(
        !output2.contains("satisfies"),
        "non-object-literal :style should NOT have satisfies: {output2}"
    );
}

#[test]
fn class_only_static_no_merge() {
    let source = r#"<template><div class="foo"/></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("class=\"foo\""),
        "static class should be unchanged: {output}"
    );
    assert!(
        !output.contains("normalizeClass"),
        "should not use normalizeClass for static-only: {output}"
    );
}

#[test]
fn class_only_dynamic_no_merge() {
    let source = r#"<template><div :class="{bar: true}"/></template>"#;
    let output = gen_tsx_template(source);

    assert!(
        output.contains("class={{bar: true}}"),
        "dynamic-only class should be simple binding: {output}"
    );
    assert!(
        !output.contains("normalizeClass"),
        "should not use normalizeClass for dynamic-only: {output}"
    );
}

#[test]
fn class_merge_no_extra_closing_brace() {
    // Bug: `<span :class="$attrs.class" class="ns-popover--wrapper">` generated `])}}`
    // (double closing brace) instead of `])}`
    let source =
        r#"<template><span :class="$attrs.class" class="ns-popover--wrapper">hi</span></template>"#;
    let output = gen_tsx_template(source);

    // Positive: should contain merged normalizeClass with static value
    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass for merged class: {output}"
    );
    assert!(
        output.contains("\"ns-popover--wrapper\""),
        "should contain static class value: {output}"
    );

    // Negative: must NOT have double closing brace `])}}` — only `])}`
    let double_brace = "])}}";
    assert!(
        !output.contains(double_brace),
        "must not have extra closing brace: {output}"
    );
    // Positive: should have exactly `])}`
    let single_brace = "])}";
    assert!(
        output.contains(single_brace),
        "should have correct single closing brace: {output}"
    );
}

#[test]
fn class_merge_static_before_dynamic_no_extra_brace() {
    // Popover.vue pattern: static `class` BEFORE dynamic `:class`
    let source =
        r#"<template><span class="ns-popover--wrapper" :class="$attrs.class">hi</span></template>"#;
    let output = gen_tsx_template(source);

    eprintln!("=== OUTPUT ===\n{}\n=== END ===", output);

    // Positive: should contain normalizeClass
    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass: {output}"
    );

    // Negative: must NOT have double closing brace
    let double_brace = "])}}";
    assert!(
        !output.contains(double_brace),
        "must not have extra closing brace: {output}"
    );
}

#[test]
fn class_merge_dynamic_before_static_no_extra_brace() {
    // Original Bug 2 pattern: dynamic `:class` BEFORE static `class`
    let source =
        r#"<template><span :class="$attrs.class" class="ns-popover--wrapper">hi</span></template>"#;
    let output = gen_tsx_template(source);

    eprintln!("=== OUTPUT ===\n{}\n=== END ===", output);

    // Positive: should contain normalizeClass
    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass: {output}"
    );

    // Negative: must NOT have double closing brace
    let double_brace = "])}}";
    assert!(
        !output.contains(double_brace),
        "must not have extra closing brace: {output}"
    );
}

#[test]
fn popover_vue_template_generates_valid_tsx() {
    // Full Popover.vue template pattern that user reports as broken
    let source = r#"<script setup lang="ts">
import { computed, ref, useTemplateRef, watch } from 'vue'
const show = ref(false)
const onClickWrapper = () => {}
const floatingStyles = ref({})
const showArrow = ref(false)
const arrowPos = ref({})
</script>
<template>
  <span
    ref="wrapperElm"
    class="ns-popover--wrapper"
    :class="$attrs.class"
    :style="$attrs.style as any"
    @click="onClickWrapper"
  >
    <slot name="reference" />
  </span>
  <Popup
    ref="popupElm"
    v-model:show="show"
    class="ns-popover"
    :style="[floatingStyles, $attrs.style]"
    position=""
  >
    <div v-if="showArrow" ref="arrowElm" class="ns-popover__arrow" :style="[arrowPos]"></div>
    <div
      role="menu"
      class="ns-popover__content"
      :class="{
        'ns-popover__content--horizontal': true,
      }"
    >
      <slot />
    </div>
  </Popup>
</template>"#;
    let output = gen_tsx_template(source);

    eprintln!("=== POPOVER OUTPUT ===\n{}\n=== END ===", output);

    // Must not have any double closing braces from normalizeClass
    let double_brace = "])}}";
    assert!(
        !output.contains(double_brace),
        "must not have extra closing brace: {output}"
    );

    // normalizeClass should be present for merged class attrs
    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass for merged class: {output}"
    );

    // v-if should NOT appear in JSX
    assert!(
        !output.contains("v-if"),
        "v-if attribute must be removed from JSX: {output}"
    );
}

#[test]
fn class_merge_with_script_attrs_and_generic() {
    // Regression: Popover.vue with attrs="{ class: string, style: string }" on
    // <script setup> produces duplicate class/style attributes in JSX.
    let source = r#"<script setup lang="ts" attrs="{ class: string, style: string }" generic="T extends object">
import { ref } from 'vue'
const show = ref(false)
const onClickWrapper = () => {}
</script>
<template>
  <span
    ref="wrapperElm"
    class="ns-popover--wrapper"
    :class="$attrs.class"
    :style="$attrs.style as any"
    @click="onClickWrapper"
  >
    <slot name="reference" />
  </span>
</template>"#;
    let output = gen_tsx_template(source);

    eprintln!("=== ATTRS+GENERIC OUTPUT ===\n{}\n=== END ===", output);

    // Positive: should use normalizeClass for merged class
    assert!(
        output.contains("normalizeClass"),
        "should use normalizeClass for merged class: {output}"
    );

    // Critical: must have exactly 1 class= attribute (no duplicates → ts(17001))
    let class_count = output.matches("class=").count();
    assert_eq!(
        class_count, 1,
        "should have exactly 1 class= attribute, got {class_count}: {output}"
    );

    // Critical: must have exactly 1 style= attribute (no duplicates)
    let style_count = output.matches("style=").count();
    assert_eq!(
        style_count, 1,
        "should have exactly 1 style= attribute, got {style_count}: {output}"
    );

    // Negative: must not have double closing brace from normalizeClass
    assert!(
        !output.contains("])}}"),
        "must not have extra closing brace: {output}"
    );
}

// ── Split overwrite tests for source map accuracy ────────────────

/// `v-bind="$attrs"` must produce `{...___VERTER___instance.$attrs}` using split
/// overwrites so that `$attrs` retains its original source position in the source map.
/// Without the split, TSGO hover lands on `___VERTER___instance` instead.
#[test]
fn v_bind_spread_attrs_source_map_accuracy() {
    let source = r#"<template><div v-bind="$attrs"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Positive: spread with instance prefix
    assert!(
        output.contains("{...___VERTER___instance.$attrs}"),
        "v-bind=\"$attrs\" should produce spread with instance prefix: {output}"
    );
    // Negative: no raw v-bind
    assert!(
        !output.contains("v-bind"),
        "v-bind attribute must be removed from JSX: {output}"
    );

    // Source map: find the source column of `$attrs` in the original
    let source_attrs_col = source.find("$attrs").expect("$attrs in source") as u32;

    // Tokens are (dst_line, dst_col, src_col) for line 0 tokens with source_id.
    // With the split overwrite, there should be a token mapping generated $attrs
    // back to source col of $attrs. Without the split, only prop.start is mapped.
    let has_attrs_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == source_attrs_col);
    assert!(
        has_attrs_token,
        "source map must have a token mapping to the original $attrs position (col {}), \
         but only found source columns: {:?}",
        source_attrs_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );
}

/// `:data="$attrs"` (static key with instance prefix) must use split overwrite
/// so `$attrs` retains its source map position.
#[test]
fn static_prop_with_prefix_source_map_accuracy() {
    let source = r#"<template><div :data="$attrs"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Positive: the prop should be converted to JSX binding
    assert!(
        output.contains("data={___VERTER___instance.$attrs}"),
        ":data=\"$attrs\" should produce data={{instance.$attrs}}: {output}"
    );
    // Negative: no raw `:data` or `v-bind`
    assert!(
        !output.contains(":data"),
        ":data directive must be removed from JSX: {output}"
    );

    // Source map: verify $attrs maps to its original source position
    let source_attrs_col = source.find("$attrs").expect("$attrs in source") as u32;
    let has_attrs_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == source_attrs_col);
    assert!(
        has_attrs_token,
        "source map must have a token mapping to the original $attrs position (col {}), \
         but only found source columns: {:?}",
        source_attrs_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );
}

/// `:rows="d_rows"` with Data binding (PrimeVue-shaped case) — the prefix-only
/// rewrite must use split overwrite so `d_rows` retains its source map position.
/// Without the split, TSGO hover lands on the synthetic `___VERTER___instance` prefix.
#[test]
fn data_prop_binding_source_map_accuracy() {
    let source = r#"<template><DataTable :rows="d_rows"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("d_rows", BindingType::Data)]);

    // Positive: prop should use instance prefix
    assert!(
        output.contains("rows={___VERTER___instance.d_rows}"),
        ":rows=\"d_rows\" should produce rows={{___VERTER___instance.d_rows}}: {output}"
    );
    // Negative: no raw :rows
    assert!(
        !output.contains(":rows"),
        ":rows directive must be removed from JSX: {output}"
    );

    // Source map: d_rows should map to its original source position
    let source_col = source.find("d_rows").expect("d_rows in source") as u32;
    let has_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == source_col);
    assert!(
        has_token,
        "source map must have a token mapping to the original d_rows position (col {}), \
         but only found source columns: {:?}",
        source_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );
}

/// `:class="{ 'active': visible }"` with Props binding — patch-based approach must
/// preserve source map tokens for identifiers so TSGO hover works on sub-expressions.
/// With Props binding, `visible` gets `__props.` prefix, which previously used a single
/// overwrite destroying source map tokens.
#[test]
fn class_binding_with_props_source_map_accuracy() {
    let source = r#"<template><div :class="{ 'active': visible }"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("visible", BindingType::Props)]);

    // Positive: should produce JSX class binding with __props prefix
    assert!(
        output.contains("class={{ 'active': __props.visible }}"),
        "should convert :class to JSX class binding with props prefix: {output}"
    );
    // Negative: no raw :class
    assert!(
        !output.contains(":class"),
        ":class directive must be removed from JSX: {output}"
    );

    // Source map: `visible` identifier should have a token at its original source position
    // (patch-based approach preserves it via collect_binding_patches)
    let visible_src_col = source.find("visible").expect("visible in source") as u32;
    let has_visible_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == visible_src_col);
    assert!(
        has_visible_token,
        "source map must have a token mapping to the original visible position (col {}), \
         but only found source columns: {:?}",
        visible_src_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );
}

/// `:class` with merged static+dynamic class — source map tokens preserved via patch-based.
#[test]
fn merged_class_binding_source_map_accuracy() {
    let source = r#"<template><div class="base" :class="{ 'active': isActive }"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("isActive", BindingType::Props)]);

    // Positive: should use normalizeClass with merged static value and __props prefix
    assert!(
        output.contains("___VERTER___normalizeClass"),
        "merged class should use normalizeClass: {output}"
    );
    assert!(
        output.contains("__props.isActive"),
        "should apply __props prefix to isActive: {output}"
    );

    // Negative: no raw :class
    assert!(
        !output.contains(":class"),
        ":class directive must be removed from JSX: {output}"
    );

    // Source map: `isActive` identifier should have a token at its original source position
    let src_col = source.find("isActive").expect("isActive in source") as u32;
    let has_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == src_col);
    assert!(
        has_token,
        "source map must have a token mapping to the original isActive position (col {}), \
         but only found source columns: {:?}",
        src_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );
}

// ========================================================================
// Fix 5: Sourcemap coverage for member access and $props (Bugs 7, 11)
// ========================================================================

/// Member access in v-bind: `:prop="obj.field"` — verify sourcemap interpolation covers `.field`.
///
/// The PositionMapper uses interpolation between tokens, so we only need a token at `obj`
/// and the offset to `field` will be computed automatically. Verify the token exists for `obj`
/// and that the output preserves the expression unchanged.
#[test]
fn member_access_in_v_bind_source_map() {
    let source = r#"<template><Comp :prop="obj.field"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("obj", BindingType::SetupConst)]);

    // Positive: should emit prop={obj.field}
    assert!(
        output.contains("obj.field"),
        "should preserve obj.field: {output}"
    );

    // Sourcemap: verify `obj` has a token — interpolation covers `.field` from this token
    let obj_src_col = source.find("obj.field").unwrap() as u32;
    let has_obj_token = tokens.iter().any(|&(_, _, sc)| sc == obj_src_col);
    assert!(
        has_obj_token,
        "source map must have token for `obj` at col {}, tokens: {:?}",
        obj_src_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );

    // Verify no overwrite breaks the linear mapping between obj and field:
    // Both must be on the same generated line and same source line with matching offsets.
    let field_src_col = source.find("field").unwrap() as u32;
    let obj_offset = field_src_col - obj_src_col; // 4 chars ("obj.")

    // Find the generated column of the `obj` token
    let obj_gen = tokens
        .iter()
        .find(|&&(_, _, sc)| sc == obj_src_col)
        .map(|&(dl, dc, _)| (dl, dc));
    if let Some((_obj_line, obj_col)) = obj_gen {
        // Verify the generated output has `field` at obj_col + 4
        // (i.e., no inserted/removed text between obj and field)
        let gen_out = &output;
        let lines: Vec<&str> = gen_out.lines().collect();
        if let Some(line_str) = lines.first() {
            let gen_field_expected_col = obj_col + obj_offset;
            if (gen_field_expected_col as usize) < line_str.len() {
                let actual = &line_str[gen_field_expected_col as usize..];
                assert!(
                    actual.starts_with("field"),
                    "interpolation check: expected 'field' at generated col {}, but got '{}'",
                    gen_field_expected_col,
                    &actual[..actual.len().min(10)]
                );
            }
        }
    }
}

/// `$props` member access: `{{ $props.msg }}` — verify sourcemap token for `$props`.
///
/// The PositionMapper interpolates from `$props` token to `.msg`. If the expression is
/// rewritten (e.g., `$props` → `__props`), the original source token should still map
/// correctly. The `.msg` part needs the linear offset from the `$props` token to be intact.
#[test]
fn dollar_props_member_access_source_map() {
    let source = r#"<template><div>{{ $props.msg }}</div></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Positive: should contain $props.msg or a prefixed version
    assert!(
        output.contains("$props") || output.contains("__props"),
        "should contain $props reference: {output}"
    );

    // Sourcemap: verify `$props` has a token
    let props_src_col = source.find("$props").unwrap() as u32;
    let has_props_token = tokens.iter().any(|&(_, _, sc)| sc == props_src_col);
    assert!(
        has_props_token,
        "source map must have token for `$props` at col {}, tokens: {:?}",
        props_src_col,
        tokens.iter().map(|t| t.2).collect::<Vec<_>>()
    );

    // Check: if $props is rewritten to something longer (e.g., __props or ___VERTER___.instance.$props),
    // the interpolation from the $props token to .msg won't work because the generated text
    // is longer than the source. Log the output for diagnosis.
    let msg_src_col = source.find("msg").unwrap() as u32;
    let props_to_msg_src_offset = msg_src_col - props_src_col; // 7 chars ("$props.")

    // Find generated position of $props token
    let props_gen = tokens
        .iter()
        .find(|&&(_, _, sc)| sc == props_src_col)
        .map(|&(dl, dc, _)| (dl, dc));

    if let Some((_gen_line, gen_col)) = props_gen {
        // In the generated output, check what's at gen_col + 7 (the interpolated .msg position)
        let gen_msg_expected = gen_col + props_to_msg_src_offset;
        let lines: Vec<&str> = output.lines().collect();
        if let Some(line_str) = lines.first() {
            if (gen_msg_expected as usize) < line_str.len() {
                let at_expected = &line_str[gen_msg_expected as usize..];
                if !at_expected.starts_with("msg") {
                    // Interpolation broken — $props was rewritten to something longer.
                    // This is the root cause: the generated text between $props and .msg
                    // has different length than the source, breaking linear interpolation.
                    eprintln!(
                        "DIAGNOSIS: $props interpolation broken. At gen col {}: '{}'. \
                         Output: '{}'",
                        gen_msg_expected,
                        &at_expected[..at_expected.len().min(20)],
                        output,
                    );
                }
            }
        }
    }
}

/// Props binding prefix sourcemap accuracy: `:title="myProp"` with Props binding.
/// The generated output has `__props.myProp`. The source map token for `myProp`
/// should point to the generated position of `myProp` (AFTER `__props.`), not to
/// `__props.` itself. This ensures hover at `myProp` in the Vue SFC resolves to the
/// correct prop type rather than the full `__props` object type.
#[test]
fn prop_binding_prefix_source_map_accuracy() {
    let source = r#"<template><div :title="myProp"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("myProp", BindingType::Props)]);

    // Positive: output should contain __props.myProp
    assert!(
        output.contains("__props.myProp"),
        "should apply __props prefix: {output}"
    );

    // Find source column of `myProp` in the :title attribute value
    let src_col = source.find("myProp").unwrap() as u32;

    // There should be a source map token whose source column points to `myProp`
    let token = tokens.iter().find(|&&(_, _, sc)| sc == src_col);
    assert!(
        token.is_some(),
        "source map must have a token for myProp at src col {src_col}. Tokens: {:?}",
        tokens
    );

    // The generated column of that token should point to `myProp` (after `__props.`),
    // not to `__props.` itself.
    let &(gen_line, gen_col, _) = token.unwrap();
    let lines: Vec<&str> = output.lines().collect();
    if let Some(line_str) = lines.get(gen_line as usize) {
        let at_gen = &line_str[gen_col as usize..];
        assert!(
            at_gen.starts_with("myProp"),
            "generated column {gen_col} should point to 'myProp', not '__props.'. \
             At gen col {gen_col}: '{}'. Full output: {output}",
            &at_gen[..at_gen.len().min(20)]
        );
    }
}

/// Props binding in template literal: `:class="\`prefix--${closeIconPosition}\`"`.
/// Same issue as above but within a template literal expression.
#[test]
fn prop_in_template_literal_source_map_accuracy() {
    let source = r#"<template><div :class="`prefix--${closeIconPosition}`"></div></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("closeIconPosition", BindingType::Props)]);

    // Positive: should apply __props prefix
    assert!(
        output.contains("__props.closeIconPosition"),
        "should apply __props prefix: {output}"
    );

    // Find source column of `closeIconPosition` in the template literal
    let src_col = source.find("closeIconPosition").unwrap() as u32;

    // There should be a source map token for closeIconPosition
    let token = tokens.iter().find(|&&(_, _, sc)| sc == src_col);
    assert!(
        token.is_some(),
        "source map must have a token for closeIconPosition at src col {src_col}. Tokens: {:?}",
        tokens
    );

    // The generated column should point to 'closeIconPosition', not '__props.'
    let &(gen_line, gen_col, _) = token.unwrap();
    let lines: Vec<&str> = output.lines().collect();
    if let Some(line_str) = lines.get(gen_line as usize) {
        let at_gen = &line_str[gen_col as usize..];
        assert!(
            at_gen.starts_with("closeIconPosition"),
            "generated column should point to 'closeIconPosition', not '__props.'. \
             At gen col {gen_col}: '{}'. Full output: {output}",
            &at_gen[..at_gen.len().min(30)]
        );
    }
}

// ── v-bind shorthand `:` off-by-one source map tests ────────────

/// v-bind shorthand `:prop="expr"` — the source map token for the prop name
/// must point to the prop name itself (e.g., `class`), NOT to the `:` prefix.
///
/// Previously, `out.overwrite(prop.start, ...)` used `prop.start` which includes
/// the `:`, making all diagnostics off by 1 column.
#[test]
fn v_bind_shorthand_prop_name_source_map_accuracy() {
    let source = r#"<template><div :class="foo"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("foo", BindingType::SetupConst)]);

    // Positive: should emit class={foo}
    assert!(
        output.contains("class={foo}"),
        "should convert :class to class={{foo}}: {output}"
    );
    // Negative: no raw :class in output
    assert!(
        !output.contains(":class"),
        ":class must be removed from JSX output: {output}"
    );

    // Source map: the `class` prop name token should map to `class` in source,
    // not to the `:` that precedes it.
    let colon_src_col = source.find(":class").unwrap() as u32;
    let class_src_col = colon_src_col + 1; // `class` starts after `:`

    // Find the generated position of `class` in the output
    let class_gen_col = output.find("class={").unwrap() as u32;

    // There must be a token mapping generated `class` back to source `class` (not `:`)
    let has_correct_token = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == class_gen_col && sc == class_src_col);
    let has_wrong_token = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == class_gen_col && sc == colon_src_col);
    assert!(
        has_correct_token,
        "source map token for `class` should point to source col {} (the `c` in `class`), \
         not col {} (the `:`). Tokens: {:?}",
        class_src_col,
        colon_src_col,
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );
    assert!(
        !has_wrong_token,
        "source map must NOT map generated `class` to the `:` position (col {}). \
         Tokens: {:?}",
        colon_src_col,
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );
}

/// Same as above but for a longer prop name to confirm it's not just `class`.
/// `:title="msg"` — token for `title` should map to `t` not `:`.
#[test]
fn v_bind_shorthand_title_source_map_accuracy() {
    let source = r#"<template><Comp :title="msg"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("msg", BindingType::SetupConst)]);

    // Positive: should emit title={msg}
    assert!(
        output.contains("title={msg}"),
        "should convert :title to title={{msg}}: {output}"
    );

    let colon_src_col = source.find(":title").unwrap() as u32;
    let title_src_col = colon_src_col + 1;
    let title_gen_col = output.find("title={").unwrap() as u32;

    let has_correct_token = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == title_gen_col && sc == title_src_col);
    assert!(
        has_correct_token,
        "source map token for `title` should point to source col {} (the `t` in `title`), \
         not col {} (the `:`). Tokens: {:?}",
        title_src_col,
        colon_src_col,
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );
}

/// v-bind shorthand without value: `:foo` → `foo={foo}`.
/// The prop name token should map to `foo`, not the `:`.
#[test]
fn v_bind_shorthand_no_value_source_map_accuracy() {
    let source = r#"<template><Comp :foo/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("foo", BindingType::SetupConst)]);

    // Positive: should emit foo={foo}
    assert!(
        output.contains("foo={foo}"),
        "should convert :foo to foo={{foo}}: {output}"
    );

    let colon_src_col = source.find(":foo").unwrap() as u32;
    let foo_src_col = colon_src_col + 1;
    let foo_gen_col = output.find("foo={").unwrap() as u32;

    let has_correct_token = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == foo_gen_col && sc == foo_src_col);
    assert!(
        has_correct_token,
        "source map token for `foo` should point to source col {} (the `f` in `foo`), \
         not col {} (the `:`). Tokens: {:?}",
        foo_src_col,
        colon_src_col,
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );
}

/// v-bind shorthand without value: `:foo` ≡ `:foo="foo"`. The generated VALUE
/// identifier (inside `foo={…}`) must map back to the source `foo` arg token so
/// go-to-definition on the binding-resolved value lands on the template `foo`
/// (whose binding resolves to the declaration). Distinct from
/// `v_bind_shorthand_no_value_source_map_accuracy`, which pins the NAME (LHS)
/// mapping. Pre-fix the value was baked into a single `out.overwrite(arg_end, …,
/// "={foo}")` whose `Overwritten` chunk maps the whole run back to `arg_end`, so
/// the value identifier had NO token at the source `foo` start — this test fails
/// against that tree and passes once the value routes through the `EmitOp`
/// substrate.
#[test]
fn v_bind_shorthand_no_value_value_maps_to_source() {
    let source = r#"<template><Comp :foo/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("foo", BindingType::SetupConst)]);

    assert!(
        output.contains("foo={foo}"),
        "should convert :foo to foo={{foo}}: {output}"
    );

    let colon_src_col = source.find(":foo").unwrap() as u32;
    let foo_src_col = colon_src_col + 1; // the `f` of the arg token

    // The VALUE identifier is the `foo` INSIDE the braces: `foo={foo}` → value at
    // `+ "foo={".len()`. (The first `foo` is the NAME / LHS.)
    let pair_gen_col = output.find("foo={foo}").unwrap() as u32;
    let value_gen_col = pair_gen_col + "foo={".len() as u32;

    // Post-fix: a token at the value's generated column maps to the source `foo`
    // arg start. Pre-fix: the baked overwrite maps the run to `arg_end`, so no
    // token at `value_gen_col` points to `foo_src_col`.
    let value_maps_to_source = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == value_gen_col && sc == foo_src_col);
    assert!(
        value_maps_to_source,
        "the generated VALUE identifier `foo` (gen col {value_gen_col}) must map to source col \
         {foo_src_col} (the `f` in the `:foo` arg). Pre-fix it was baked into a mapped overwrite \
         anchored at arg_end and had no such token. Tokens: {:?}",
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );

    // Negative: the value identifier must NOT collapse to the prop start (`:`).
    let value_maps_to_colon = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == value_gen_col && sc == colon_src_col);
    assert!(
        !value_maps_to_colon,
        "the generated VALUE identifier must not map to the `:` (col {colon_src_col}). \
         Tokens: {:?}",
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );
}

/// `.foo` v-bind prop-modifier shorthand without value: `.foo` ≡ `.foo="foo"`.
/// The generated VALUE identifier (inside `foo={…}`) must map back to the source
/// `foo` key token (after the `.`). Pre-fix the WHOLE prop span was overwritten
/// with `format!("{}={{{}}}", key, resolved)`, baking both name and value into one
/// `Overwritten` chunk anchored at `prop.start` (the `.`), so the value
/// identifier had NO token at the source `foo` start.
#[test]
fn dot_prop_shorthand_no_value_value_maps_to_source() {
    let source = r#"<template><Comp .foo/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("foo", BindingType::SetupConst)]);

    assert!(
        output.contains("foo={foo}"),
        "should convert .foo to foo={{foo}}: {output}"
    );

    let dot_src_col = source.find(".foo").unwrap() as u32;
    let key_src_col = dot_src_col + 1; // the `f` of the key token (after `.`)

    let pair_gen_col = output.find("foo={foo}").unwrap() as u32;
    let value_gen_col = pair_gen_col + "foo={".len() as u32;

    let value_maps_to_source = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == value_gen_col && sc == key_src_col);
    assert!(
        value_maps_to_source,
        "the generated VALUE identifier `foo` (gen col {value_gen_col}) must map to source col \
         {key_src_col} (the `f` in the `.foo` key). Pre-fix the whole `.foo` span was baked into a \
         mapped overwrite anchored at the `.` and had no such token. Tokens: {:?}",
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );

    // Negative: the value identifier must NOT collapse to the prop start (`.`).
    let value_maps_to_dot = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == value_gen_col && sc == dot_src_col);
    assert!(
        !value_maps_to_dot,
        "the generated VALUE identifier must not map to the `.` (col {dot_src_col}). \
         Tokens: {:?}",
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );
}

/// Long-form `v-bind:prop="expr"` — the prop name token should map to `prop`, not `v`.
#[test]
fn v_bind_longform_prop_name_source_map_accuracy() {
    let source = r#"<template><div v-bind:class="foo"/></template>"#;

    let (output, tokens) = gen_tsx_template_with_map(source, &[("foo", BindingType::SetupConst)]);

    // Positive: should emit class={foo}
    assert!(
        output.contains("class={foo}"),
        "should convert v-bind:class to class={{foo}}: {output}"
    );

    let vbind_src_col = source.find("v-bind:class").unwrap() as u32;
    let class_src_col = source.find(":class").unwrap() as u32 + 1; // after `:` in `v-bind:class`
    let class_gen_col = output.find("class={").unwrap() as u32;

    let has_correct_token = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == class_gen_col && sc == class_src_col);
    assert!(
        has_correct_token,
        "source map token for `class` should point to source col {} (the `c` in `class`), \
         not col {} (the `v` in `v-bind`). Tokens: {:?}",
        class_src_col,
        vbind_src_col,
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );
}

// ── Slot outlet source map accuracy ─────────────────────────────

#[test]
fn slot_outlet_tag_name_source_mapped_to_slots() {
    // Hovering on `slot` in `<slot name="reference" />` should map to `$slots`
    // in the generated TSX, NOT to `?.()` or other synthetic regions.
    let source = r#"<template><slot name="reference" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Verify output shape
    assert!(output.contains("$slots"), "should contain $slots: {output}");
    assert!(
        output.contains(".reference"),
        "should contain .reference: {output}"
    );

    // Find source position of `s` in `<slot`
    let slot_src_col = source.find("<slot").unwrap() as u32 + 1; // position of `s`

    // Find the generated position of `$slots`
    let gen_slots_pos = output.find("$slots").unwrap() as u32;

    // The source map token at `s` should map to `$slots` in generated output,
    // NOT to positions past `$slots` (like `?.()`)
    let token_for_slot = tokens.iter().find(|&&(_, _, sc)| sc == slot_src_col);
    assert!(
        token_for_slot.is_some(),
        "should have source map token for `slot` tag name at src col {}. Tokens: {:?}",
        slot_src_col,
        tokens
    );

    let &(_, dst_col, _) = token_for_slot.unwrap();
    // dst_col should be within the `$slots` region, not past it
    assert!(
        dst_col >= gen_slots_pos && dst_col < gen_slots_pos + 6,
        "slot tag name should map to `$slots` region (gen cols {}..{}), got gen col {}. Output: {}",
        gen_slots_pos,
        gen_slots_pos + 6,
        dst_col,
        output
    );
}

#[test]
fn slot_outlet_name_attr_does_not_map_to_call_site() {
    // Positions within the `name="reference"` attribute should NOT map to `?.()`.
    // The slot name value `reference` should map to `.reference` in generated output.
    let source = r#"<template><slot name="reference" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Find source position of `reference` value (inside quotes)
    let ref_src_col = source.find("reference").unwrap() as u32;

    // Find generated position of `reference` (in `.reference`)
    let gen_ref_text = ".reference";
    let gen_ref_pos = output.find(gen_ref_text).unwrap() as u32;
    let gen_ref_start = gen_ref_pos + 1; // skip the `.`

    // The token for `reference` should map to the `.reference` region
    let token_for_ref = tokens.iter().find(|&&(_, _, sc)| sc == ref_src_col);
    assert!(
        token_for_ref.is_some(),
        "should have source map token for `reference` at src col {}. Tokens: {:?}",
        ref_src_col,
        tokens
    );

    let &(_, dst_col, _) = token_for_ref.unwrap();
    assert!(
        dst_col >= gen_ref_start && dst_col < gen_ref_start + 9,
        "reference should map to `.reference` region (gen cols {}..{}), got gen col {}. Output: {}",
        gen_ref_start,
        gen_ref_start + 9,
        dst_col,
        output
    );
}

#[test]
fn slot_outlet_no_interpolation_past_mapped_content() {
    // Simulates vue_to_tsx interpolation for the meaningful parts of the slot tag:
    // tag name (`slot`), attribute name (`name`), and attribute value (`reference`).
    // These positions must NOT land on the `(` of `?.()` — that causes `() any` hover.
    // Structural syntax (closing `"`, ` />`) may map to the `?.` operator, which is fine.
    let source = r#"<template><slot name="reference" /></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Find the generated position of `(` in `?.()` — this is where TSGO shows `() any`
    let call_paren_pos = output.find("?.()").unwrap() as u32 + 2; // position of `(`

    // Meaningful source positions: `<slot name="reference`
    // (excludes closing `"` and ` />` which are structural syntax)
    let tag_start = source.find("<slot").unwrap() as u32;
    let ref_end = source.find("reference").unwrap() as u32 + "reference".len() as u32;

    // Simulate vue_to_tsx for meaningful positions
    for query_col in tag_start..ref_end {
        let best = tokens
            .iter()
            .filter(|&&(_, _, sc)| sc <= query_col)
            .max_by_key(|&&(_, _, sc)| sc);

        if let Some(&(_, dst_col, src_col)) = best {
            let delta = query_col - src_col;
            let interpolated_dst = dst_col + delta;

            assert!(
                interpolated_dst < call_paren_pos,
                "source col {} interpolates to gen col {} (token src={} dst={} + delta={}), \
                 which is at/past `(` in `?.()` (gen col {}). This causes `() any` hover. Output: {}",
                query_col, interpolated_dst, src_col, dst_col, delta,
                call_paren_pos, output
            );
        }
    }
}

// ── Class/style merge source map accuracy ───────────────────────

#[test]
fn class_merge_dynamic_class_position_is_mapped() {
    // When both `class="foo"` and `:class="bar"` exist, the `:class` directive's
    // argument position should have a source map token pointing to the merged
    // `class={normalizeClass(...)}` attribute. The static `class` position is NOT
    // mapped in the codegen (the static attribute is removed from TSX); hover for
    // the static `class` is handled by the LSP hover handler which redirects the
    // TSGO query to the `:class` directive's position.
    let source = r#"<template><div class="foo" :class="bar"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // Find source position of the `:` in `:class` (the directive start / overwrite origin)
    let colon_class_col = source.find(":class").unwrap() as u32;

    // Find generated position of the merged `class=` attribute
    let gen_class_pos = output.find("class=").unwrap() as u32;

    // The `:class` directive start should have a source map token mapping
    // to the merged `class=` in generated TSX. This is the redirect target
    // used by the hover handler for the static `class` attribute.
    let token_for_colon = tokens.iter().find(|&&(_, _, sc)| sc == colon_class_col);
    assert!(
        token_for_colon.is_some(),
        "`:class` at src col {} should have a source map token. \
         Generated output: {}. Tokens: {:?}",
        colon_class_col,
        output,
        tokens
    );

    let &(_, dst_col, _) = token_for_colon.unwrap();
    assert!(
        dst_col >= gen_class_pos && dst_col < gen_class_pos + 6,
        "`:class` should map to merged `class=` region (gen cols {}..{}), got gen col {}. Output: {}",
        gen_class_pos, gen_class_pos + 6, dst_col, output
    );
}

// ── v-for body member access (regression test) ──────────────────

/// v-for iteration variables must NOT get the `___VERTER___instance.` prefix
/// in TSX output. They are locally scoped via `.map((param) => ...)`.
#[test]
fn v_for_body_member_access_no_instance_prefix() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><button v-for="action in actions" :disabled="action.disabled">{{ action.label }}</button></template>"#,
        &[("actions", BindingType::SetupConst)],
    );
    eprintln!("TSX output:\n{}", result);

    // Positive: .map() wrapper present
    assert!(
        result.contains(".map((action"),
        "should have .map((action...) wrapper, got: {}",
        result
    );

    // Positive: member access expressions preserved bare
    assert!(
        result.contains("action.disabled"),
        "prop expression should contain bare action.disabled, got: {}",
        result
    );
    assert!(
        result.contains("action.label"),
        "interpolation should contain bare action.label, got: {}",
        result
    );

    // NEGATIVE: v-for locals must NOT get instance prefix
    assert!(
        !result.contains("___VERTER___instance.action"),
        "v-for param must NOT get ___VERTER___instance. prefix, got: {}",
        result
    );
}

/// Source map test: verify that `action.disabled` inside v-for body is source-mapped
/// back to its original position, enabling TSGO/tsserver to resolve member access.
#[test]
fn v_for_body_member_access_source_mapped() {
    let source = r#"<template><button v-for="action in actions" :disabled="action.disabled">text</button></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("actions", BindingType::SetupConst)]);
    eprintln!("TSX output:\n{}", output);
    eprintln!("Tokens (dst_line, dst_col, src_col):");
    for &(_dl, dc, sc) in &tokens {
        eprintln!("  gen_col={}, src_col={}", dc, sc);
    }

    // Find "action.disabled" in the generated output
    let gen_action_pos = output
        .find("action.disabled")
        .expect("action.disabled should be in output");
    let gen_dot_pos = gen_action_pos + "action".len();

    // Find "action.disabled" in the source
    let src_action_pos = source
        .find("action.disabled")
        .expect("action.disabled should be in source");
    let src_dot_pos = src_action_pos + "action".len();

    eprintln!(
        "gen 'action' at col={}, gen '.' at col={}",
        gen_action_pos, gen_dot_pos
    );
    eprintln!(
        "src 'action' at col={}, src '.' at col={}",
        src_action_pos, src_dot_pos
    );

    // Find the best token: the one closest to (but not after) the source position,
    // mimicking the PositionMapper::vue_to_tsx algorithm.
    let best_token = tokens
        .iter()
        .filter(|&&(dl, _, sc)| dl == 0 && (sc as usize) <= src_dot_pos)
        .max_by_key(|&&(_, _, sc)| sc);

    assert!(
        best_token.is_some(),
        "Should have a source map token at or before src_col={}. Tokens: {:?}",
        src_dot_pos,
        tokens
    );

    let &(_, base_dc, base_sc) = best_token.unwrap();
    let delta = src_dot_pos as u32 - base_sc;
    let interpolated_gen_dot = base_dc + delta;
    eprintln!(
        "best token: gen_col={}, src_col={}, delta={}, interpolated gen_dot={}",
        base_dc, base_sc, delta, interpolated_gen_dot
    );
    assert_eq!(
        interpolated_gen_dot as usize, gen_dot_pos,
        "Position interpolation for '.' should map src_col {} to gen_col {} (actual gen_dot={}). \
         This ensures completion at 'action.' maps to the correct TSX offset.",
        src_dot_pos, interpolated_gen_dot, gen_dot_pos
    );
}

/// Nested v-for: both outer and inner iteration variables must be bare.
#[test]
fn nested_v_for_body_no_instance_prefix() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-for="user in users" :key="user.id"><span v-for="item in user.items" :key="item.id">{{ user.name }}: {{ item.text }}</span></div></template>"#,
        &[("users", BindingType::SetupConst)],
    );
    eprintln!("TSX output:\n{}", result);

    // Positive: both .map() wrappers
    assert!(
        result.contains(".map((user"),
        "outer .map((user...) expected, got: {}",
        result
    );

    // NEGATIVE: neither v-for local should get instance prefix
    assert!(
        !result.contains("___VERTER___instance.user"),
        "outer v-for param must NOT get instance prefix, got: {}",
        result
    );
    assert!(
        !result.contains("___VERTER___instance.item"),
        "inner v-for param must NOT get instance prefix, got: {}",
        result
    );
}

/// Destructured v-for params should remain bare.
#[test]
fn v_for_destructured_no_instance_prefix() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-for="{ name, email } in users" :key="email">{{ name }} ({{ email }})</div></template>"#,
        &[("users", BindingType::SetupConst)],
    );
    eprintln!("TSX output:\n{}", result);

    // NEGATIVE: destructured params must NOT get instance prefix
    assert!(
        !result.contains("___VERTER___instance.name"),
        "destructured v-for param 'name' must NOT get instance prefix, got: {}",
        result
    );
    assert!(
        !result.contains("___VERTER___instance.email"),
        "destructured v-for param 'email' must NOT get instance prefix, got: {}",
        result
    );
}

// ── Bug fix tests: verter-tsc false errors ──────────────────────────

#[test]
fn template_v_if_v_slot_no_orphan_iife_close() {
    // Bug: <template v-if v-slot> skips IIFE open but walker adds orphan }} close
    let result = gen_tsx_template(
        r#"<template><MyComp><template v-if="hasSlot" #indicator="bind"><slot name="indicator" /></template></MyComp></template>"#,
    );
    eprintln!("TSX output:\n{}", result);

    // Should not have orphan `}}` (IIFE close without matching open)
    // The JSX should be well-structured
    assert!(
        !result.contains("</>}}"),
        "should not have orphan IIFE close after slot template, got: {}",
        result
    );
}

#[test]
fn dynamic_component_closing_tag_no_attributes() {
    // Bug: </component :is="as"> leaks attributes onto JSX closing tag
    let result = gen_tsx_template(
        r#"<template><component :is="tag">child</component :is="tag"></template>"#,
    );
    eprintln!("TSX output:\n{}", result);

    // POSITIVE: should have the component render variable
    assert!(
        result.contains("___VERTER___component_render"),
        "should use component_render for dynamic :is, got: {}",
        result
    );

    // NEGATIVE: closing tag must NOT contain attributes
    assert!(
        !result.contains("</___VERTER___component_render :is"),
        "closing tag must not have :is attribute, got: {}",
        result
    );
    assert!(
        !result.contains(r#"</___VERTER___component_render "#),
        "closing tag must not have trailing content after tag name, got: {}",
        result
    );
}

#[test]
fn v_for_numeric_range_valid_tsx() {
    // Bug: v-for="i in 12" generates 12.map(...) which is invalid JS
    let result =
        gen_tsx_template(r#"<template><i v-for="i in 12" :key="i" class="line" /></template>"#);
    eprintln!("TSX output:\n{}", result);

    // POSITIVE: should have a .map() call
    assert!(
        result.contains(".map("),
        "should generate a .map() call, got: {}",
        result
    );

    // NEGATIVE: must NOT call .map() directly on a numeric literal
    assert!(
        !result.contains("12.map("),
        "must not call .map() on numeric literal, got: {}",
        result
    );
    // Also check that we don't get 12 followed by .map without space
    assert!(
        !result.contains("12 .map("),
        "must not call .map() on numeric literal with space, got: {}",
        result
    );
}

#[test]
fn v_for_numeric_expression_range_valid_tsx() {
    // Bug: v-for="i in count + 1" where count+1 might be a numeric expression
    let result = gen_tsx_template_with_bindings(
        r#"<template><div v-for="i in count" :key="i">{{ i }}</div></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    eprintln!("TSX output:\n{}", result);

    // Non-literal iterables should still work with .map()
    assert!(
        result.contains(".map("),
        "should generate .map() for non-literal iterables, got: {}",
        result
    );
}

#[test]
fn comment_between_v_if_v_else_valid_tsx() {
    // Bug: HTML comments between v-if/v-else become JSX comments that break if/else chain
    let result = gen_tsx_template(
        r#"<template><div v-if="a">A</div><!-- comment --><div v-else>B</div></template>"#,
    );
    eprintln!("TSX output:\n{}", result);

    // POSITIVE: should have both if and else branches
    assert!(
        result.contains("if("),
        "should have if condition, got: {}",
        result
    );
    assert!(
        result.contains("else"),
        "should have else branch, got: {}",
        result
    );

    // NEGATIVE: JSX comment must NOT appear between } and else
    // Valid: }else{  or }\nelse{
    // Invalid: }{/* comment */}\nelse{
    let cleaned = result.replace(char::is_whitespace, "");
    assert!(
        !cleaned.contains("}{/*"),
        "JSX comment must not appear between if-closing and else, got: {}",
        result
    );
}

#[test]
fn dynamic_component_inside_v_for_valid_tsx() {
    // Bug: <component :is> IS the v-for element — puts const statement in arrow expression
    // Real pattern from VirtualListItem.vue:
    // <component v-for="(c, index) in children" :key="index" :is="c" />
    let result = gen_tsx_template_with_bindings(
        r#"<template><component v-for="(c, index) in children" :key="index" :is="c" /></template>"#,
        &[("children", BindingType::SetupConst)],
    );
    eprintln!("TSX output:\n{}", result);

    // POSITIVE: should have component_render or extractRenderComponent
    assert!(
        result.contains("___VERTER___component_render")
            || result.contains("extractRenderComponent"),
        "should handle dynamic :is component, got: {}",
        result
    );

    // NEGATIVE: const statement must NOT appear inside .map(() => (...))
    // The arrow function with parens only allows expressions, not statements
    assert!(
        !result.contains("=> (const "),
        "const statement must not appear in arrow expression body, got: {}",
        result
    );
}

#[test]
fn dynamic_component_inside_jsx_children_valid_tsx() {
    // Bug: <component :is> inside another element puts const in JSX children
    let result = gen_tsx_template_with_bindings(
        r#"<template><div><component :is="tag" /></div></template>"#,
        &[("tag", BindingType::SetupConst)],
    );
    eprintln!("TSX output:\n{}", result);

    // The const statement for extractRenderComponent must be in valid JS context,
    // not inside JSX element children where it would be treated as text
    // Valid patterns:
    //   {(() => { const comp = ...; return <comp />; })()}
    //   Block scope before JSX
    // Invalid: <div>const comp = ...; <comp /></div>
    assert!(
        !result.contains(">const ___VERTER___component_render"),
        "const statement must not appear as JSX text children, got: {}",
        result
    );
}

#[test]
fn slot_props_kebab_case_quoted() {
    // Bug: slot scope props with kebab-case names generate unquoted property names
    // e.g., { item-class: "value" } which is invalid JS (item minus class)
    let result = gen_tsx_template(
        r#"<template><MyComp><template #default="{ itemClass }"><slot :item-class="itemClass" /></template></MyComp></template>"#,
    );
    eprintln!("TSX output:\n{}", result);

    // If slot props contain kebab-case keys, they must be quoted
    // This test verifies we don't generate unquoted hyphenated property names
    if result.contains("item-class") {
        assert!(
            result.contains(r#""item-class""#) || result.contains("'item-class'"),
            "kebab-case slot prop key must be quoted in JS object literal, got: {}",
            result
        );
    }
}

/// Regression: v-show + :style on the same element must not leak binding prefixes.
///
/// When `v-show="message"` and `:style="!!title ? undefined : { margin: 0 }"` are
/// both on the same element, the v-show handler merges both into a single `style` attribute.
/// But `process_v_bind` also processes `:style` and calls `collect_binding_patches` which
/// adds prepends at source positions of identifiers. These prepends survive the v-show
/// overwrite and leak as stray text (e.g., `___VERTER___instance.` after the style attribute).
#[test]
fn v_show_with_style_binding_no_leaked_prefix() {
    let source = r#"<template><div v-show="message" :style="!!title ? undefined : { margin: 0 }">hi</div></template>"#;
    let result = gen_tsx_template(source);
    eprintln!("TSX output:\n{}", result);

    // Positive: merged style should include both the v-show display logic and the existing style
    assert!(
        result.contains("display:"),
        "merged style should include v-show display logic. Got: {}",
        result
    );
    assert!(
        result.contains("title"),
        "merged style should include :style expression. Got: {}",
        result
    );

    // Negative: no stray binding prefixes leaked outside the style attribute
    let style_end = result
        .find("}}")
        .expect("should have closing }} for style object");
    let after_style = &result[style_end + 2..];
    assert!(
        !after_style.contains("___VERTER___instance."),
        "binding prefix must not leak after style attribute. After '}}': {:?}",
        after_style
    );

    // Parse the result with OXC to check for syntax errors
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::tsx();
    let wrapped = format!("import {{}} from 'vue';\n{}", result);
    let parsed = oxc_parser::Parser::new(&alloc, &wrapped, source_type).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Generated TSX should have no parse errors. Got {} errors. Output:\n{}",
        parsed.errors.len(),
        result
    );
}

/// Regression: complex template (notification-like) with transition, v-show, component :is,
/// v-text, and v-html must produce valid TSX without syntax errors.
#[test]
fn notification_template_complex_no_syntax_errors() {
    let source = r#"<template>
  <transition
    :name="ns.b('fade')"
    @before-leave="onClose"
    @after-leave="$emit('destroy')"
  >
    <div
      v-show="visible"
      :id="id"
      :class="[ns.b(), customClass, horizontalClass]"
      :style="positionStyle"
      role="alert"
      @mouseenter="clearTimer"
      @mouseleave="startTimer"
      @click="onClick"
    >
      <el-icon v-if="iconComponent" :class="[ns.e('icon'), typeClass]">
        <component :is="iconComponent" />
      </el-icon>
      <div :class="ns.e('group')">
        <h2 :class="ns.e('title')" v-text="title" />
        <div
          v-show="message"
          :class="ns.e('content')"
          :style="!!title ? undefined : { margin: 0 }"
        >
          <slot>
            <p v-if="!dangerouslyUseHTMLString">{{ message }}</p>
            <!-- Caution here, message could've been compromised, never use user's input as message -->
            <p v-else v-html="message" />
          </slot>
        </div>
        <el-icon v-if="showClose" :class="ns.e('closeBtn')" @click.stop="close">
          <component :is="closeIcon" />
        </el-icon>
      </div>
    </div>
  </transition>
</template>"#;
    let result = gen_tsx_template(source);
    eprintln!("=== NOTIFICATION TEMPLATE TSX ===\n{}\n=== END ===", result);

    // Negative: no stray leaked binding prefixes
    assert!(
        !result.contains("}}___VERTER___instance."),
        "binding prefix must not leak after style closing braces. Got:\n{}",
        result
    );

    // Parse the result with OXC to check for syntax errors
    let alloc = Allocator::new();
    let source_type = oxc_span::SourceType::tsx();
    let wrapped = format!("import {{}} from 'vue';\n{}", result);
    let parsed = oxc_parser::Parser::new(&alloc, &wrapped, source_type).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Generated TSX should have no parse errors. Got {} errors. Output:\n{}",
        parsed.errors.len(),
        result
    );
}

/// Regression: `<component :is="tag" v-if="cond" v-text="expr" />` must produce
/// valid TSX — the combination of dynamic :is IIFE + v-if + v-text was causing
/// syntax errors (TS1005: ';' expected).
#[test]
fn component_is_with_v_if_and_v_text_produces_valid_jsx() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><component :is="titleTag" v-if="!!title" v-text="title" /></template>"#,
        &[
            ("titleTag", BindingType::SetupConst),
            ("title", BindingType::SetupConst),
        ],
    );
    eprintln!(
        "=== COMPONENT :IS + V-IF + V-TEXT ===\n{}\n=== END ===",
        result
    );

    // Must contain v-text → textContent conversion
    assert!(
        result.contains("textContent"),
        "v-text should generate textContent prop"
    );

    // Must not have raw v-text in output
    assert!(
        !result.contains("v-text"),
        "v-text directive must be removed from JSX"
    );

    // Parse with OXC to verify valid TSX
    let alloc = Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, &result, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Generated TSX should have no parse errors. Got {} errors. Output:\n{}",
        parsed.errors.len(),
        result
    );
}

#[test]
fn component_is_v_text_options_api_full_sfc() {
    let source = r#"<template>
  <div>
    <component :is="titleTag" v-if="!!title" v-text="title" />
  </div>
</template>

<script lang="ts">
export default defineComponent({
  props: {
    title: { type: String, default: '' },
    titleTag: { type: String, default: 'h4' },
  },
  setup(props) {
    return {};
  },
});
</script>"#;
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("BalCard.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== FULL SFC TSX ===\n{}\n=== END ===", tsx.code);

    // Parse with OXC to verify valid TSX
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Full SFC TSX should have no parse errors. Got {} errors. Output:\n{}",
        parsed.errors.len(),
        tsx.code
    );
}

#[test]
fn balcard_vue_full_sfc_produces_valid_tsx() {
    let Some(source) = read_external_corpus_vue(
        "VERTER_TEST_REPOS_ROOT",
        "balancer-frontend-v2/src/components/_global/BalCard/BalCard.vue",
    ) else {
        return;
    };
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("BalCard.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions {
        source_map: true,
        ..Default::default()
    };
    let result = crate::compile::compile(&source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== BALCARD FULL TSX ===\n{}\n=== END ===", tsx.code);

    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "BalCard TSX should have no parse errors. Got {} errors",
        parsed.errors.len(),
    );
}

#[test]
fn custom_docs_block_before_template_produces_valid_tsx() {
    let source = r#"<docs>
---
order: 0
title:
  zh-CN: 基本用法
---
## Notes
</docs>

<template>
  <div>hello</div>
</template>
<script lang="ts" setup>
import { ref } from 'vue';
const checked = ref<boolean>(false);
</script>"#;
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("Basic.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== DOCS BLOCK TSX ===\n{}\n=== END ===", tsx.code);

    // Custom block content should not appear in TSX
    assert!(
        !tsx.code.contains("order: 0"),
        "Custom block content should not leak into TSX"
    );

    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "TSX with custom block should have no parse errors. Got {} errors.\nOutput:\n{}",
        parsed.errors.len(),
        tsx.code
    );
}

#[test]
fn ant_design_switch_basic_produces_valid_tsx() {
    let Some(source) = read_external_corpus_vue(
        "VERTER_TEST_REPOS_ROOT",
        "ant-design-vue/components/switch/demo/basic.vue",
    ) else {
        return;
    };
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("basic.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(&source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== ANT BASIC TSX ===\n{}\n=== END ===", tsx.code);

    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Got {} errors",
        parsed.errors.len(),
    );
}

#[test]
fn activist_card_topic_selection_produces_valid_tsx() {
    let Some(source) = read_external_corpus_vue(
        "VERTER_TEST_REPOS_ROOT",
        "activist-org-activist/frontend/app/components/card/CardTopicSelection.vue",
    ) else {
        return;
    };
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("CardTopicSelection.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(&source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== ACTIVIST TSX ===\n{}\n=== END ===", tsx.code);
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Got {} errors",
        parsed.errors.len()
    );
}

#[test]
fn activist_machine_steps_produces_valid_tsx() {
    let Some(source) = read_external_corpus_vue(
        "VERTER_TEST_REPOS_ROOT",
        "activist-org-activist/frontend/app/components/MachineStepsCreateEventTime.vue",
    ) else {
        return;
    };
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("MachineStepsCreateEventTime.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(&source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== MACHINE STEPS TSX ===\n{}\n=== END ===", tsx.code);
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Got {} errors",
        parsed.errors.len()
    );
}

/// <component :is="..."> should not generate a ___VERTER___Comp function with
/// `instantiateComponent(component, {})` — `component` is not a valid variable.
#[test]
fn component_is_dynamic_no_comp_function() {
    let source = r#"<template>
  <component :is="tag" />
</template>
<script setup lang="ts">
const tag = 'div';
</script>"#;
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("App.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");

    // Must NOT contain instantiateComponent(component, ...)
    assert!(
        !tsx.code.contains("instantiateComponent(component"),
        "Should not emit Comp function for <component :is>. Got:\n{}",
        tsx.code
    );

    // Parse to ensure valid TSX
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "Got {} errors",
        parsed.errors.len()
    );
}

#[test]
fn nexus_notification_produces_valid_tsx() {
    let Some(source) = read_external_corpus_vue(
        "VERTER_NEXUS_UI_ROOT",
        "packages/ui/src/components/Notifications/components/Notification.vue",
    ) else {
        return;
    };
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("Notification.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(&source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    // ___VERTER___props must be declared (not just referenced)
    assert!(
        tsx.code.contains("const ___VERTER___props"),
        "Destructured defineProps should declare ___VERTER___props. Got:\n{}",
        tsx.code
    );

    // Parse with OXC to verify valid TSX
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Got {} errors",
        parsed.errors.len(),
    );
}

/// Destructured defineProps (`const { foo } = defineProps<{...}>()`) should
/// declare ___VERTER___props so that `const __props = ___VERTER___props` resolves.
#[test]
fn destructured_define_props_declares_verter_props() {
    let source = r#"<script setup lang="ts">
const { msg, count } = defineProps<{
  msg: string
  count: number
}>()
</script>
<template>
  <div>{{ msg }} {{ count }}</div>
</template>"#;
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("App.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");

    // ___VERTER___props must be declared, not just referenced
    assert!(
        tsx.code.contains("const ___VERTER___props"),
        "Should declare ___VERTER___props for destructured defineProps. Got:\n{}",
        tsx.code
    );

    // Original destructured pattern should NOT remain
    assert!(
        !tsx.code.contains("const { msg, count }"),
        "Destructuring pattern should be rewritten. Got:\n{}",
        tsx.code
    );

    // Parse to ensure valid TSX
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Got {} errors",
        parsed.errors.len()
    );
}

#[test]
fn nexus_bloc_produces_valid_tsx() {
    let Some(source) = read_external_corpus_vue(
        "VERTER_NEXUS_UI_ROOT",
        "packages/ui/src/components/atom/Bloc/Bloc.vue",
    ) else {
        return;
    };
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("Bloc.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(&source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== BLOC TSX ===\n{}\n=== END ===", tsx.code);
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("OXC ERROR: {}", err);
    }
    assert!(
        parsed.errors.is_empty(),
        "Got {} errors",
        parsed.errors.len()
    );
}

#[test]
fn runtime_define_props_in_template_scope() {
    // Runtime defineProps({...}) without assignment should expose prop names
    // in the template scope. TS2304 "Cannot find name" if they're not.
    let source = r#"<template>
  <div v-if="showBoard">
    <router-link :to="`/boards/${url}`">{{ name }}</router-link>
  </div>
</template>

<script setup lang="ts">
defineProps({
  name: { type: String, required: true },
  url: { type: String, required: true },
  showBoard: { type: Boolean, required: true },
});
</script>"#;
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("BoardBadge.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    eprintln!("=== RUNTIME PROPS TSX ===\n{}\n=== END ===", tsx.code);

    // Positive: props should be accessible via __props in template
    assert!(
        tsx.code.contains("__props.showBoard"),
        "showBoard should be accessed via __props in template, got:\n{}",
        tsx.code
    );
    assert!(
        tsx.code.contains("__props.url") || tsx.code.contains("__props.name"),
        "url/name should be accessed via __props in template, got:\n{}",
        tsx.code
    );

    // Negative: Comp function condition guards must also use __props
    // (TS2304 "Cannot find name 'showBoard'" if bare)
    assert!(
        !tsx.code.contains("if(!((showBoard)))"),
        "Comp function guard must NOT use bare 'showBoard' — should be __props.showBoard, got:\n{}",
        tsx.code
    );

    // OXC validation
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "Full TSX should parse without errors. Got {} errors:\n{}",
        parsed.errors.len(),
        tsx.code
    );
}

#[test]
fn closing_tag_case_mismatch_component() {
    // Vue is case-insensitive for closing tags: <Button>...</button> is valid.
    // JSX is case-sensitive: the closing tag must match the opening tag.
    // Verter must rewrite the closing tag to match the opening tag.
    let result = gen_tsx_template_with_bindings(
        r#"<template>
  <Button class="btn">Click</Button>
  <Button class="btn2">Click2</button>
</template>"#,
        &[("Button", BindingType::SetupConst)],
    );
    eprintln!("=== CASE MISMATCH ===\n{}\n=== END ===", result);

    // Positive: both buttons should have matching closing tags
    let close_count = result.matches("</Button>").count();
    assert!(
        close_count == 2,
        "should have 2 </Button> closing tags (case-corrected), got {} in:\n{}",
        close_count,
        result
    );

    // Negative: lowercase </button> should not appear
    assert!(
        !result.contains("</button>"),
        "lowercase </button> should be rewritten to </Button>, got:\n{}",
        result
    );
}

// ── Kebab-case event handling in spread syntax ─────────────────────────────

#[test]
fn kebab_event_with_dollar_event_emits_typed_payload_param() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @click-overlay="emit('clickOverlay', $event)" /></template>"#,
        &[("emit", BindingType::SetupConst)],
    );
    // Hyphenated JSX name can't be a bare attribute → spread key.
    assert!(
        result.contains(r#""onClick-overlay""#),
        "should preserve kebab-case event name as spread key: {result}"
    );
    // `$event` is bound as the handler's sole parameter, EXPLICITLY annotated with
    // an ambient DOM event-payload type — JSX contextual typing cannot flow through a
    // spread, so a bare parameter would be `any`. A non-standard native event
    // (`click-overlay`) is not a known DOM event, so the intersected `Event` index
    // fallback types it as the base `Event` (never a `ts(2339)` error).
    assert!(
        result.contains("($event:"),
        "spread $event must be an explicitly-typed parameter: {result}"
    );
    assert!(
        result.contains(r#"(GlobalEventHandlersEventMap & { [___VERTER___EventKey: string]: Event })["click-overlay"]"#),
        "spread $event type must be the ambient DOM event-map type keyed by the event name: {result}"
    );
    // Negative: the `import('vue')` indexed formula is NOT used for native spread
    // `$event` — it does not resolve under the tsgo TypeProvider.
    assert!(
        !result.contains("IntrinsicElementAttributes"),
        "native spread $event must not use the import('vue') formula: {result}"
    );
    // Negative: the generic `eventCallbacks<TArgs extends Array<any>>` helper that
    // forced `$event` to `any` is gone.
    assert!(
        !result.contains("___VERTER___eventCallbacks"),
        "spread $event must NOT use the eventCallbacks wrapper: {result}"
    );
    assert!(
        !result.contains("...___VERTER___eventArgs"),
        "spread $event must NOT use the generic event-args rest param: {result}"
    );
}

#[test]
fn kebab_event_arrow_function_satisfies_native_payload() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @click-overlay="($event) => doSomething($event)" /></template>"#,
        &[("doSomething", BindingType::SetupConst)],
    );
    // JSX contextual typing cannot flow through a spread, so the arrow handler's
    // parameter would be implicit-`any`. The arrow is wrapped in a `satisfies` clause
    // whose target is the native event-handler signature, so `$event` is contextually
    // typed against the ambient DOM payload tuple. The user arrow stays source-mapped.
    assert!(
        result.contains(
            r#""onClick-overlay": (($event) => doSomething($event)) satisfies (...___VERTER___eventArgs: "#
        ),
        "arrow handler must be `satisfies`-wrapped on spread: {result}"
    );
    assert!(
        result.contains(
            r#"[(GlobalEventHandlersEventMap & { [___VERTER___EventKey: string]: Event })["click-overlay"]]) => unknown"#
        ),
        "satisfies target must be the native DOM payload tuple: {result}"
    );
    // Negative: the `import('vue')` indexed formula is NOT used (it does not resolve
    // under the tsgo TypeProvider).
    assert!(
        !result.contains("IntrinsicElementAttributes"),
        "native spread arrow must not use the import('vue') formula: {result}"
    );
    // Negative: should NOT double-wrap the arrow body into a synthetic block.
    assert!(
        !result.contains("($event) => {($event)"),
        "should NOT double-wrap arrow function: {result}"
    );
}

#[test]
fn kebab_event_function_expr_satisfies_native_payload() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @click-overlay="function($event) { doSomething($event) }" /></template>"#,
        &[("doSomething", BindingType::SetupConst)],
    );
    // Function-expression handler is `satisfies`-wrapped exactly like the arrow case.
    assert!(
        result.contains(r#""onClick-overlay": (function($event) { doSomething($event) }) satisfies (...___VERTER___eventArgs: "#),
        "function expression must be `satisfies`-wrapped on spread: {result}"
    );
    assert!(
        result.contains(r#") => unknown"#),
        "satisfies clause must close with `=> unknown`: {result}"
    );
}

#[test]
fn kebab_event_inline_expr_no_dollar_event_wraps_with_no_param() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @click-overlay="count++" /></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    // Inline expression without $event → () => { ... }
    assert!(
        result.contains("() => {"),
        "should wrap with () => for inline expr without $event: {result}"
    );
    assert!(
        result.contains(r#""onClick-overlay""#),
        "should preserve kebab-case event name: {result}"
    );
}

// ── Fix 1: Broken interpolation recovery ──────────────────────────

#[test]
fn broken_interpolation_preserves_identifiers() {
    // Broken expression: {{ count + }} — OXC can't parse it, but identifiers must survive
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ count + }}</div></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    eprintln!("broken interpolation output: {}", result);
    // Positive: identifiers preserved
    assert!(
        result.contains("count"),
        "broken expression should preserve identifiers: {result}"
    );
    // Positive: mustache delimiters converted
    assert!(
        result.contains('{') && result.contains('}') && result.contains("count"),
        "mustache should be converted to a JSX expression with preserved identifiers: {result}"
    );
    // Negative: no raw mustache delimiters
    assert!(
        !result.contains("{{") && !result.contains("}}"),
        "mustache delimiters must be converted to JSX: {result}"
    );
    assert_valid_tsx(&result, "broken-interpolation");
}

#[test]
fn broken_interpolation_keeps_identifier_source_map_anchor() {
    let source = r#"<template><div>{{ count + }}</div></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("count", BindingType::SetupConst)]);

    let count_src_col = source.find("count").unwrap() as u32;
    let anchor = tokens
        .iter()
        .filter(|&&(_, _, src_col)| src_col <= count_src_col)
        .max_by_key(|&&(_, _, src_col)| src_col)
        .copied();

    assert!(
        anchor.is_some(),
        "broken interpolation should retain a usable source-map anchor before 'count', tokens: {:?}",
        tokens
    );

    let (_gen_line, gen_col, anchor_src_col) = anchor.unwrap();
    let mapped_col = gen_col + (count_src_col - anchor_src_col);
    let first_line = output.lines().next().unwrap_or("");
    assert!(
        first_line
            .get(mapped_col as usize..)
            .is_some_and(|suffix| suffix.starts_with("count")),
        "broken interpolation should keep linear mapping from the nearest anchor to 'count', got output: {output}, anchor={anchor:?}"
    );
}

#[test]
fn valid_interpolations_unaffected_by_broken_expr_handling() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ count }}</div></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    // Valid expression should still work normally (whitespace from source is preserved)
    assert!(
        result.contains("{ count }") || result.contains("{count}"),
        "valid interpolation should produce {{count}}: {result}"
    );
    assert!(
        !result.contains("{{") && !result.contains("}}"),
        "no raw mustache delimiters: {result}"
    );
}

#[test]
fn mixed_broken_and_valid_interpolations() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div>{{ count + }}<span>{{ count }}</span></div></template>"#,
        &[("count", BindingType::SetupRef)],
    );
    eprintln!("mixed output: {}", result);
    // Valid expression should be fully patched (whitespace from source is preserved)
    assert!(
        result.contains("{ count }") || result.contains("{count}"),
        "valid interpolation should be patched: {result}"
    );
    // Broken expression: identifiers preserved
    assert!(
        result.contains("count"),
        "broken expression should still preserve identifiers: {result}"
    );
    // No raw mustache delimiters anywhere
    assert!(
        !result.contains("{{") && !result.contains("}}"),
        "no raw mustache delimiters: {result}"
    );
    assert_valid_tsx(&result, "mixed-broken-and-valid-interpolations");
}

// ── Fix 3: v-slot scoped parameter typing ─────────────────────────

#[test]
fn v_slot_params_arrow_wrapper() {
    // Component v-slot with params: should generate IIFE with extractArgumentsFromRenderSlot
    let result = gen_tsx_template(
        r#"<template><MyComp v-slot="{ slotItem }"><span>{{ slotItem }}</span></MyComp></template>"#,
    );
    eprintln!("v-slot params output: {}", result);
    // Positive: should have arrow function wrapper for slot params
    assert!(
        result.contains("{ slotItem }") || result.contains("{slotItem}"),
        "should contain slot params in arrow function: {result}"
    );
    assert!(
        result.contains("extractArgumentsFromRenderSlot"),
        "should use extractArgumentsFromRenderSlot for slot typing: {result}"
    );
    assert!(
        result.contains("instantiateComponent"),
        "should use instantiateComponent for component instance: {result}"
    );
    assert!(
        result.contains(r#""default""#),
        "should reference default slot name: {result}"
    );
    // Negative: v-slot attribute must not appear
    assert!(
        !result.contains("v-slot"),
        "v-slot attribute must be removed: {result}"
    );
    assert!(
        result.contains("const { slotItem } = ___VERTER___extractArgumentsFromRenderSlot")
            || result.contains("const {slotItem} = ___VERTER___extractArgumentsFromRenderSlot"),
        "slot params should bind from the typed slot extract result, got: {result}"
    );
    assert!(
        !result.contains("function({ slotItem })") && !result.contains("function({slotItem})"),
        "slot params should not be introduced as untyped function parameters, got: {result}"
    );
}

#[test]
fn v_slot_named_template_params() {
    // <template #header="{ title }"> should generate typed wrapper with "header"
    let result = gen_tsx_template(
        r#"<template><MyComp><template #header="{ title }"><span>{{ title }}</span></template></MyComp></template>"#,
    );
    eprintln!("named template v-slot output: {}", result);
    assert!(
        result.contains("extractArgumentsFromRenderSlot"),
        "should use extractArgumentsFromRenderSlot: {result}"
    );
    assert!(
        result.contains(r#""header""#),
        "should reference header slot name: {result}"
    );
    // Negative
    assert!(
        !result.contains("#header") && !result.contains("v-slot:header"),
        "v-slot directive must be removed: {result}"
    );
}

#[test]
fn v_slot_default_template_params() {
    // <template v-slot="{ data }"> should use "default" slot name
    let result = gen_tsx_template(
        r#"<template><MyComp><template v-slot="{ data }"><span>{{ data }}</span></template></MyComp></template>"#,
    );
    eprintln!("default template v-slot output: {}", result);
    assert!(
        result.contains("extractArgumentsFromRenderSlot"),
        "should use extractArgumentsFromRenderSlot: {result}"
    );
    assert!(
        result.contains(r#""default""#),
        "should use default slot name: {result}"
    );
}

#[test]
fn v_slot_multiple_named_templates() {
    // Multiple named slots — each gets independent IIFE, params don't leak
    let result = gen_tsx_template(
        r#"<template><MyComp><template #header="{ x }"><span>{{ x }}</span></template><template #footer="{ y }"><span>{{ y }}</span></template></MyComp></template>"#,
    );
    eprintln!("multi-slot output: {}", result);
    assert!(
        result.contains(r#""header""#) && result.contains(r#""footer""#),
        "should reference both slot names: {result}"
    );
    // Count extractArgumentsFromRenderSlot calls — should be 2
    let count = result.matches("extractArgumentsFromRenderSlot").count();
    assert_eq!(
        count, 2,
        "should have 2 extractArgumentsFromRenderSlot calls: {result}"
    );
}

#[test]
fn v_slot_no_params_unchanged() {
    // v-slot without params: no wrapper needed
    let result =
        gen_tsx_template(r#"<template><MyComp v-slot><span>content</span></MyComp></template>"#);
    eprintln!("v-slot no params output: {}", result);
    assert!(
        !result.contains("extractArgumentsFromRenderSlot"),
        "no wrapper for v-slot without params: {result}"
    );
    assert!(
        !result.contains("v-slot"),
        "v-slot attribute must be removed: {result}"
    );
}

#[test]
fn v_slot_params_no_instance_prefix() {
    // Slot params must NOT get ___VERTER___instance. prefix
    let result = gen_tsx_template_with_bindings(
        r#"<template><MyComp v-slot="{ slotItem }"><span>{{ slotItem }}</span></MyComp></template>"#,
        &[("slotItem", BindingType::SetupConst)], // Even if in bindings, slot takes priority
    );
    assert!(
        !result.contains("___VERTER___instance.slotItem"),
        "slot param must NOT get instance prefix: {result}"
    );
}

#[test]
fn partial_v_slot_param_stays_bare_for_completion() {
    let result = gen_tsx_template(
        r#"<template><MyComp v-slot="{ slotItem, slotIndex, slotTotal }"><span>{{ sl }}</span></MyComp></template>"#,
    );
    assert!(
        result.contains("{ sl }") || result.contains("{sl}"),
        "partial slot param should stay bare for completion context, got: {result}"
    );
    assert!(
        !result.contains("___VERTER___instance.sl"),
        "partial slot param must not get instance prefix, got: {result}"
    );
}

#[test]
fn v_slot_with_v_for() {
    // v-for wraps element, slot wraps children — both should work
    let result = gen_tsx_template_with_bindings(
        r#"<template><MyComp v-for="item in items" :key="item.id" v-slot="{ data }"><span>{{ data }}</span></MyComp></template>"#,
        &[("items", BindingType::SetupConst)],
    );
    eprintln!("v-for + v-slot output: {}", result);
    // Both v-for map and v-slot IIFE should be present
    assert!(
        result.contains(".map("),
        "v-for should produce .map(): {result}"
    );
    assert!(
        result.contains("extractArgumentsFromRenderSlot"),
        "v-slot should produce extractArgumentsFromRenderSlot: {result}"
    );
}

// ── Object literal in binding: prop name as key must not be rewritten ────

fn compile_full_sfc_tsx(source: &str, filename: &str) -> String {
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some(filename.to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    tsx.code.clone()
}

fn assert_valid_tsx(code: &str, label: &str) {
    let alloc = Allocator::new();
    let parsed = oxc_parser::Parser::new(&alloc, code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.errors {
        eprintln!("[{label}] OXC ERROR: {err}");
    }
    assert!(
        parsed.errors.is_empty(),
        "[{label}] TSX should have no parse errors. Got {} errors. Output:\n{code}",
        parsed.errors.len()
    );
}

#[test]
fn object_literal_binding_prop_key_not_rewritten() {
    // Bug: `:overlay-style="{ zIndex: zIndex - 2 }"` where `zIndex` is a prop
    // causes `resolve_all_prop_refs_in_expr` to produce `__props.zIndex: __props.zIndex - 2`
    // which is invalid JS (can't have dots in object keys without quotes).
    let source = r#"<script setup lang="ts">
import MyComp from './MyComp.vue'
const props = defineProps<{ zIndex: number }>()
</script>
<template>
  <MyComp :overlay-style="{ zIndex: zIndex - 2 }" />
</template>"#;
    let code = compile_full_sfc_tsx(source, "Test.vue");
    eprintln!("Object key test TSX:\n{code}");

    // Should parse without errors (the core assertion)
    assert_valid_tsx(&code, "object-key-prop");

    // Negative: should NOT have __props.zIndex: (invalid object key)
    assert!(
        !code.contains("__props.zIndex:"),
        "object key must NOT be prefixed with __props.: {code}"
    );
}

// ── JSX helper ─────────────────────────────────────────────

fn gen_jsx_template(source: &str) -> String {
    let alloc = Allocator::new();
    let bytes = source.as_bytes();

    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });

    let template_ast = match syntax.take_template_ast() {
        Some(ast) => ast,
        None => return String::new(),
    };

    let source_type = oxc_span::SourceType::tsx();
    let oxc_ast = crate::template::oxc::parse_template_expressions(
        &template_ast,
        source,
        &alloc,
        source_type,
        true,
    );

    let mut tpl_ct = CodeTransform::new(source, &alloc);
    let mut out = CodeGenOutput::new(&alloc);
    let bindings = FxHashMap::default();
    let options = IdeTemplateOptions {
        self_name: "App",
        comments: true,
        is_jsx: true,
        strict_slots: false,
    };

    generate_ide_template(
        &template_ast,
        &oxc_ast,
        source,
        &mut out,
        &alloc,
        &bindings,
        &options,
        &TemplateComponentBindings::default(),
    );
    out.apply_to(&mut tpl_ct);

    let full = tpl_ct.build_string();
    let tpl_start = template_ast.root.tag_open.start as usize;
    let tpl_end = template_ast
        .root
        .tag_close
        .as_ref()
        .map(|tc| tc.end as usize)
        .unwrap_or(full.len());
    let suffix_len = source.len() - tpl_end;
    full[tpl_start..full.len() - suffix_len].to_string()
}

// ── Custom directive type checking ─────────────────────────

#[test]
fn custom_directive_basic_no_args() {
    let result = gen_tsx_template(r#"<template><div v-focus /></template>"#);
    eprintln!("custom_directive_basic_no_args:\n{result}");

    // Positive: should emit v-directive callback with vFocus
    assert!(
        result.contains("v-directive="),
        "should emit v-directive prop: {result}"
    );
    assert!(
        result.contains(r#"directiveAccessor["vFocus"]"#),
        "should reference vFocus from accessor: {result}"
    );
    assert!(
        result.contains("true,undefined,{}"),
        "no-value directive should use true,undefined,{{}}: {result}"
    );

    // Negative: v-focus should NOT appear as raw attribute
    assert!(
        !result.contains("v-focus"),
        "v-focus raw attribute must be removed: {result}"
    );
}

#[test]
fn custom_directive_with_value() {
    let result = gen_tsx_template(r#"<template><div v-test="val" /></template>"#);
    eprintln!("custom_directive_with_value:\n{result}");

    assert!(
        result.contains(r#"directiveAccessor["vTest"]"#),
        "should reference vTest: {result}"
    );
    // Value should be the expression "val"
    assert!(
        result.contains("val,undefined,{}"),
        "should have val as value expression: {result}"
    );
}

#[test]
fn custom_directive_static_arg() {
    let result = gen_tsx_template(r#"<template><div v-test:foo="val" /></template>"#);
    eprintln!("custom_directive_static_arg:\n{result}");

    assert!(
        result.contains(r#"directiveAccessor["vTest"]"#),
        "should reference vTest: {result}"
    );
    assert!(
        result.contains(r#"val,"foo","#),
        "should have static arg 'foo' (quoted): {result}"
    );
}

#[test]
fn custom_directive_dynamic_arg() {
    let result = gen_tsx_template(r#"<template><div v-test:[dyn]="val" /></template>"#);
    eprintln!("custom_directive_dynamic_arg:\n{result}");

    assert!(
        result.contains(r#"directiveAccessor["vTest"]"#),
        "should reference vTest: {result}"
    );
    // Dynamic arg: dyn resolved as expression (no quotes)
    assert!(
        result.contains("instance.dyn,"),
        "dynamic arg should be resolved unquoted expression: {result}"
    );
}

#[test]
fn custom_directive_modifiers() {
    let result = gen_tsx_template(r#"<template><div v-test.bar.baz="val" /></template>"#);
    eprintln!("custom_directive_modifiers:\n{result}");

    assert!(
        result.contains(r#"directiveAccessor["vTest"]"#),
        "should reference vTest: {result}"
    );
    assert!(
        result.contains(r#""bar":true"#),
        "should have bar modifier: {result}"
    );
    assert!(
        result.contains(r#""baz":true"#),
        "should have baz modifier: {result}"
    );
}

#[test]
fn custom_directive_multiple() {
    let result = gen_tsx_template(r#"<template><div v-a v-b="x" /></template>"#);
    eprintln!("custom_directive_multiple:\n{result}");

    // Should have single v-directive= with both calls
    assert!(
        result.contains(r#"directiveAccessor["vA"]"#),
        "should reference vA: {result}"
    );
    assert!(
        result.contains(r#"directiveAccessor["vB"]"#),
        "should reference vB: {result}"
    );
    // Only one v-directive= prop
    assert_eq!(
        result.matches("v-directive=").count(),
        1,
        "should have exactly one v-directive prop: {result}"
    );
}

#[test]
fn custom_directive_hyphenated_name() {
    let result = gen_tsx_template(r#"<template><div v-click-outside="fn" /></template>"#);
    eprintln!("custom_directive_hyphenated_name:\n{result}");

    assert!(
        result.contains(r#"directiveAccessor["vClickOutside"]"#),
        "should camelCase hyphenated name: {result}"
    );

    // Negative: raw attribute must not appear
    assert!(
        !result.contains("v-click-outside"),
        "raw v-click-outside must be removed: {result}"
    );
}

#[test]
fn custom_directive_builtins_not_captured() {
    let result = gen_tsx_template(r#"<template><div v-show="x" /></template>"#);
    eprintln!("custom_directive_builtins_not_captured:\n{result}");

    // v-show is a built-in — should NOT produce v-directive
    assert!(
        !result.contains("v-directive="),
        "built-in v-show should NOT produce v-directive: {result}"
    );
}

#[test]
fn custom_directive_jsx_mode_skips() {
    let result = gen_jsx_template(r#"<template><div v-focus /></template>"#);
    eprintln!("custom_directive_jsx_mode_skips:\n{result}");

    // JSX mode should NOT emit v-directive (TS-only feature)
    assert!(
        !result.contains("v-directive="),
        "JSX mode should not emit v-directive: {result}"
    );
}

#[test]
fn custom_directive_full_combo() {
    // v-test:foo.bar="baz" — value + static arg + modifier
    let result = gen_tsx_template(r#"<template><div v-test:foo.bar="baz" /></template>"#);
    eprintln!("custom_directive_full_combo:\n{result}");

    assert!(
        result.contains(r#"directiveAccessor["vTest"]"#),
        "should reference vTest: {result}"
    );
    assert!(
        result.contains(r#"baz,"foo",{"bar":true}"#),
        "should have value, static arg, and modifier object: {result}"
    );

    // Negative: raw directive must not appear
    assert!(
        !result.contains("v-test:foo"),
        "raw v-test:foo must be removed: {result}"
    );
}

#[test]
fn custom_directive_on_component() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><MyComp v-focus /></template>"#,
        &[("MyComp", BindingType::SetupConst)],
    );
    eprintln!("custom_directive_on_component:\n{result}");

    // Should work the same on components
    assert!(
        result.contains("v-directive="),
        "should emit v-directive on component: {result}"
    );
    assert!(
        result.contains(r#"directiveAccessor["vFocus"]"#),
        "should reference vFocus: {result}"
    );
}

// ── Script preamble: directive accessor ────────────────────

#[test]
fn script_preamble_directive_accessor() {
    let source = r#"<script setup lang="ts">
const x = 1
</script>
<template><div v-focus /></template>"#;
    let code = compile_full_sfc_tsx(source, "Test.vue");
    eprintln!("script_preamble_directive_accessor:\n{code}");

    assert!(
        code.contains("___VERTER___directiveAccessor"),
        "should emit directiveAccessor declaration: {code}"
    );
    assert!(
        code.contains("retrieveSetupDirectives"),
        "should import retrieveSetupDirectives: {code}"
    );
    assert!(
        code.contains("runCustomDirective"),
        "should import runCustomDirective: {code}"
    );
    assert!(
        code.contains("ExtractLeafElement"),
        "should import ExtractLeafElement type: {code}"
    );
}

#[test]
fn script_preamble_directive_accessor_valid_tsx() {
    let source = r#"<script setup lang="ts">
const x = 1
</script>
<template><div v-focus v-test:foo.bar="baz" /></template>"#;
    let code = compile_full_sfc_tsx(source, "Test.vue");
    eprintln!("script_preamble_directive_accessor_valid_tsx:\n{code}");

    // The output should be valid TSX
    assert_valid_tsx(&code, "directive-accessor-preamble");
}

// ── @ts-expect-error / @ts-ignore in template comments ──────────────────────

#[test]
fn ts_expect_error_before_component() {
    let result = gen_tsx_template(r#"<template><!-- @ts-expect-error --><MyComp/></template>"#);
    // Comment should appear as JSX comment before the component
    assert!(
        result.contains("{/* @ts-expect-error */}"),
        "should have TS directive comment, got:\n{}",
        result
    );
    // Comment must appear before the component tag
    let comment_pos = result.find("{/* @ts-expect-error */}").unwrap();
    let comp_pos = result.find("<MyComp").unwrap();
    assert!(
        comment_pos < comp_pos,
        "comment should appear before component, got:\n{}",
        result
    );
    // No raw HTML comment markers in output
    assert!(
        !result.contains("<!--"),
        "should not have raw HTML comment markers, got:\n{}",
        result
    );
    assert!(
        !result.contains("-->"),
        "should not have raw HTML comment close, got:\n{}",
        result
    );
}

#[test]
fn ts_expect_error_before_v_for() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><!-- @ts-expect-error --><div v-for="x in xs">{{ x }}</div></template>"#,
        &[("xs", BindingType::SetupRef)],
    );
    // v-for wraps in .map() — the comment must be INSIDE the map callback
    assert!(
        result.contains(".map("),
        "should have .map() wrapper, got:\n{}",
        result
    );
    let map_pos = result.find(".map(").unwrap();
    // Comment must be present (as JSX comment with TS directive)
    assert!(
        result.contains("@ts-expect-error"),
        "TS directive comment should be present, got:\n{}",
        result
    );
    let comment_pos = result.find("@ts-expect-error").unwrap();
    assert!(
        comment_pos > map_pos,
        "comment should be inside .map() callback, not before it, got:\n{}",
        result
    );
    // No raw HTML comment markers
    assert!(
        !result.contains("<!--"),
        "no raw HTML markers, got:\n{}",
        result
    );
}

#[test]
fn ts_expect_error_before_component_is() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><!-- @ts-expect-error --><component :is="comp"/></template>"#,
        &[("comp", BindingType::SetupRef)],
    );
    // <component :is> wraps in IIFE — comment must be inside the IIFE
    assert!(
        result.contains("extractRenderComponent"),
        "should have extractRenderComponent IIFE, got:\n{}",
        result
    );
    // Comment should be inside the IIFE (after the IIFE open)
    let iife_pos = result.find("(() =>").unwrap();
    // Check that a TS directive comment appears somewhere after the IIFE open
    let after_iife = &result[iife_pos..];
    assert!(
        after_iife.contains("@ts-expect-error"),
        "TS directive comment should be inside component :is IIFE, got:\n{}",
        result
    );
    // No raw HTML comment markers
    assert!(
        !result.contains("<!--"),
        "no raw HTML markers, got:\n{}",
        result
    );
}

#[test]
fn ts_expect_error_v_if_component_is() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><!-- @ts-expect-error --><component :is="c" v-if="ok"/></template>"#,
        &[("c", BindingType::SetupRef), ("ok", BindingType::SetupRef)],
    );
    // v-if wraps in IIFE, <component :is> creates nested IIFE
    // The comment should end up inside the component :is IIFE (before `return`)
    assert!(
        result.contains("extractRenderComponent"),
        "should have extractRenderComponent IIFE, got:\n{}",
        result
    );
    // Comment should be somewhere in the output
    assert!(
        result.contains("@ts-expect-error"),
        "TS directive comment should be present, got:\n{}",
        result
    );
    assert!(
        !result.contains("<!--"),
        "no raw HTML markers, got:\n{}",
        result
    );
}

#[test]
fn ts_expect_error_v_for_v_if() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><!-- @ts-expect-error --><div v-for="x in xs" v-if="ok">{{ x }}</div></template>"#,
        &[("xs", BindingType::SetupRef), ("ok", BindingType::SetupRef)],
    );
    // v-for + v-if: v-for is outer (.map), v-if uses ternary inside
    assert!(
        result.contains(".map("),
        "should have .map() wrapper, got:\n{}",
        result
    );
    let map_pos = result.find(".map(").unwrap();
    assert!(
        result.contains("@ts-expect-error"),
        "TS directive comment should be present, got:\n{}",
        result
    );
    let comment_pos = result.find("@ts-expect-error").unwrap();
    assert!(
        comment_pos > map_pos,
        "comment should be inside .map() callback, got:\n{}",
        result
    );
    assert!(
        !result.contains("<!--"),
        "no raw HTML markers, got:\n{}",
        result
    );
}

#[test]
fn ts_ignore_same_behavior() {
    let result = gen_tsx_template(r#"<template><!-- @ts-ignore --><MyComp/></template>"#);
    assert!(
        result.contains("{/* @ts-ignore */}"),
        "should have @ts-ignore comment, got:\n{}",
        result
    );
    let comment_pos = result.find("{/* @ts-ignore */}").unwrap();
    let comp_pos = result.find("<MyComp").unwrap();
    assert!(
        comment_pos < comp_pos,
        "@ts-ignore should appear before component, got:\n{}",
        result
    );
    assert!(
        !result.contains("<!--"),
        "no raw HTML markers, got:\n{}",
        result
    );
}

#[test]
fn regular_comment_not_repositioned_for_v_for() {
    let result = gen_tsx_template(
        r#"<template><!-- hello --><div v-for="x in xs">{{ x }}</div></template>"#,
    );
    // Regular (non-TS-directive) comment should NOT be repositioned inside .map()
    assert!(
        result.contains("{/* hello */}"),
        "regular comment should be converted to JSX, got:\n{}",
        result
    );
    // Comment should stay at its original position (before .map)
    let comment_pos = result.find("{/* hello */}").unwrap();
    let map_pos = result.find(".map(").unwrap();
    assert!(
        comment_pos < map_pos,
        "regular comment should stay before .map(), not be repositioned inside, got:\n{}",
        result
    );
    assert!(
        !result.contains("<!--"),
        "no raw HTML markers, got:\n{}",
        result
    );
}

#[test]
fn existing_v_if_comment_repositioning_not_regressed() {
    // The existing v-if comment repositioning should still work
    let result = gen_tsx_template_with_bindings(
        r#"<template><!-- @ts-expect-error --><div v-if="show">hello</div></template>"#,
        &[("show", BindingType::SetupRef)],
    );
    assert!(
        result.contains("if(show)"),
        "should have IIFE condition, got:\n{}",
        result
    );
    let iife_pos = result.find("{()=>{").expect("should have IIFE open");
    let comment_pos = result
        .find("{/* @ts-expect-error */}")
        .expect("comment should be preserved");
    assert!(
        comment_pos > iife_pos,
        "comment should appear AFTER IIFE open (inside), got:\n{}",
        result
    );
    assert!(
        !result.contains("<!--"),
        "no raw HTML markers, got:\n{}",
        result
    );
}

// ── Strict slot children type checking ──────────────────────

/// Helper: compile a template with strict_slots enabled.
/// Returns the template portion of the TSX output.
#[allow(dead_code)]
fn gen_tsx_template_strict_slots(source: &str) -> String {
    gen_tsx_template_strict_slots_with_bindings(source, &[])
}

fn gen_tsx_template_strict_slots_with_bindings(
    source: &str,
    bindings: &[(&str, BindingType)],
) -> String {
    let alloc = Allocator::new();
    let bytes = source.as_bytes();

    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });

    let template_ast = match syntax.take_template_ast() {
        Some(ast) => ast,
        None => return String::new(),
    };

    let source_type = oxc_span::SourceType::tsx();
    let oxc_ast = crate::template::oxc::parse_template_expressions(
        &template_ast,
        source,
        &alloc,
        source_type,
        true,
    );

    let tpl_alloc = Allocator::new();
    let mut tpl_ct = CodeTransform::new(source, &tpl_alloc);
    let mut out = CodeGenOutput::new(&tpl_alloc);

    let mut binding_map: FxHashMap<&str, BindingType> = FxHashMap::default();
    for &(name, bt) in bindings {
        binding_map.insert(tpl_alloc.alloc_str(name), bt);
    }

    let options = IdeTemplateOptions {
        self_name: "App",
        comments: true,
        is_jsx: false,
        strict_slots: true,
    };

    generate_ide_template(
        &template_ast,
        &oxc_ast,
        source,
        &mut out,
        &tpl_alloc,
        &binding_map,
        &options,
        &TemplateComponentBindings::default(),
    );
    out.apply_to(&mut tpl_ct);

    let full = tpl_ct.build_string();
    let tpl_start = template_ast.root.tag_open.start as usize;
    let tpl_end = template_ast
        .root
        .tag_close
        .as_ref()
        .map(|tc| tc.end as usize)
        .unwrap_or(full.len());
    let suffix_len = source.len() - tpl_end;
    full[tpl_start..full.len() - suffix_len].to_string()
}

#[test]
fn strict_slots_component_children() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs><TabItem /><TabItem /></Tabs></template>",
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
        ],
    );
    // Positive: strictRenderSlot call with default slot and TabItem children
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot call, got:\n{}",
        result
    );
    assert!(
        result.contains("$slots"),
        "should reference $slots, got:\n{}",
        result
    );
    assert!(
        result.contains("'default'"),
        "should reference default slot, got:\n{}",
        result
    );
    assert!(
        result.contains("TabItem"),
        "should reference TabItem constructor, got:\n{}",
        result
    );
    // Negative: no v-if or v-for artifacts in slot check
    assert!(
        !result.contains("v-if"),
        "v-if should not appear in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_html_children() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs><input /><span></span></Tabs></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Positive: strictRenderSlot with HTML element type references
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot call, got:\n{}",
        result
    );
    assert!(
        result.contains("HTMLElementTagNameMap"),
        "should reference HTMLElementTagNameMap, got:\n{}",
        result
    );
    assert!(
        result.contains("\"input\""),
        "should reference input element, got:\n{}",
        result
    );
    assert!(
        result.contains("\"span\""),
        "should reference span element, got:\n{}",
        result
    );
    // Negative
    assert!(
        !result.contains("v-slot"),
        "no v-slot in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_text_children() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs>hello world</Tabs></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Positive: strictRenderSlot with string type for text
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot for text, got:\n{}",
        result
    );
    assert!(
        result.contains("as string"),
        "should have string type for text node, got:\n{}",
        result
    );
    // Negative
    assert!(
        !result.contains("HTMLElementTagNameMap"),
        "should not have HTMLElementTagNameMap for text, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_named_slot() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs><template #header><input /></template></Tabs></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Positive: strictRenderSlot referencing named slot 'header'
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("'header'"),
        "should reference header slot name, got:\n{}",
        result
    );
    // Negative: should NOT have 'default' slot call
    assert!(
        !result.contains("'default'"),
        "should not have default slot (only named), got:\n{}",
        result
    );
}

#[test]
fn strict_slots_mixed_named_default() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs><template #header><input /></template><template #default><span /></template></Tabs></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Positive: two separate strictRenderSlot calls
    assert!(
        result.contains("'header'"),
        "should have header slot, got:\n{}",
        result
    );
    assert!(
        result.contains("'default'"),
        "should have default slot, got:\n{}",
        result
    );
    // Count occurrences of strictRenderSlot
    let count = result.matches("strictRenderSlot").count();
    assert!(
        count >= 2,
        "should have at least 2 strictRenderSlot calls, got {}, output:\n{}",
        count,
        result
    );
}

#[test]
fn strict_slots_no_children() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs /></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Negative: no strictRenderSlot for self-closing components
    assert!(
        !result.contains("strictRenderSlot"),
        "should NOT emit strictRenderSlot for self-closing, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_whitespace_only() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs>   \n   </Tabs></template>",
        &[("Tabs", BindingType::SetupImport)],
    );
    // Negative: no strictRenderSlot for whitespace-only children
    assert!(
        !result.contains("strictRenderSlot"),
        "should NOT emit strictRenderSlot for whitespace-only children, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_dynamic_component() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><component :is="comp"><span /></component></template>"#,
        &[("comp", BindingType::SetupRef)],
    );
    // Negative: no strictRenderSlot for dynamic <component :is>
    assert!(
        !result.contains("strictRenderSlot"),
        "should NOT emit strictRenderSlot for dynamic component, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_disabled() {
    let result = gen_tsx_template_with_bindings(
        "<template><Tabs><TabItem /></Tabs></template>",
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
        ],
    );
    // Negative: no strictRenderSlot when strict_slots is false (default helper)
    assert!(
        !result.contains("strictRenderSlot"),
        "should NOT emit strictRenderSlot when disabled, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_v_if_child() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs><TabItem v-if="show" /></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
            ("show", BindingType::SetupRef),
        ],
    );
    // Positive: TabItem still in the strict slot check (v-if doesn't change the type)
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("TabItem"),
        "should contain TabItem in slot check, got:\n{}",
        result
    );
    // Negative
    assert!(
        !result.contains("v-if"),
        "v-if should not appear in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_v_for_child() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs><TabItem v-for="i in 3" /></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
        ],
    );
    // Positive: TabItem still in the strict slot check
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("TabItem"),
        "should contain TabItem, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_interpolation_child() {
    let result = gen_tsx_template_strict_slots_with_bindings(
        "<template><Tabs>{{ msg }}</Tabs></template>",
        &[
            ("Tabs", BindingType::SetupImport),
            ("msg", BindingType::SetupRef),
        ],
    );
    // Positive: strictRenderSlot with string type for interpolation
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot for interpolation, got:\n{}",
        result
    );
    assert!(
        result.contains("as string"),
        "should have string type for interpolation, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_with_v_slot_params() {
    // When a component has v-slot params, BOTH extractArgumentsFromRenderSlot
    // (for slot props typing) AND strictRenderSlot (for children checking) should appear.
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs v-slot="{ item }"><TabItem /></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
        ],
    );
    // Positive: both helpers present
    assert!(
        result.contains("extractArgumentsFromRenderSlot"),
        "should have extractArgumentsFromRenderSlot for slot params, got:\n{}",
        result
    );
    assert!(
        result.contains("strictRenderSlot"),
        "should have strictRenderSlot for children, got:\n{}",
        result
    );
    assert!(
        result.contains("TabItem"),
        "should reference TabItem in slot check, got:\n{}",
        result
    );
    // Negative: no raw v-slot
    assert!(
        !result.contains("v-slot"),
        "v-slot directive should not appear in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_v_if_narrowing() {
    // v-if/v-else branches produce different element types — both should be in the array
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs><div v-if="isA" /><span v-else /></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("isA", BindingType::SetupRef),
        ],
    );
    // Positive: both div and span in the slot check array
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("\"div\""),
        "should reference div element, got:\n{}",
        result
    );
    assert!(
        result.contains("\"span\""),
        "should reference span element, got:\n{}",
        result
    );
    // Negative
    assert!(
        !result.contains("v-if"),
        "v-if should not appear in output, got:\n{}",
        result
    );
    assert!(
        !result.contains("v-else"),
        "v-else should not appear in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_v_for_nested() {
    // <template v-for> with a slot name should still collect children correctly
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs><template v-for="item in items" #default><TabItem /></template></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
            ("items", BindingType::SetupRef),
        ],
    );
    // Positive: TabItem in default slot check
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("'default'"),
        "should reference default slot, got:\n{}",
        result
    );
    assert!(
        result.contains("TabItem"),
        "should reference TabItem, got:\n{}",
        result
    );
}

// ── Strict slot sourcemap test ──────────────────────────────

/// Helper: generate TSX template with strict_slots AND return source map tokens.
fn gen_tsx_template_strict_slots_with_map(
    source: &str,
    bindings: &[(&str, BindingType)],
) -> (String, Vec<(u32, u32, u32)>) {
    let alloc = Allocator::new();
    let bytes = source.as_bytes();

    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });

    let template_ast = match syntax.take_template_ast() {
        Some(ast) => ast,
        None => return (String::new(), Vec::new()),
    };

    let source_type = oxc_span::SourceType::tsx();
    let oxc_ast = crate::template::oxc::parse_template_expressions(
        &template_ast,
        source,
        &alloc,
        source_type,
        true,
    );

    let tpl_alloc = Allocator::new();
    let mut tpl_ct = CodeTransform::new(source, &tpl_alloc);
    let mut out = CodeGenOutput::new(&tpl_alloc);
    let binding_map: FxHashMap<&str, BindingType> = bindings
        .iter()
        .map(|&(name, bt)| (tpl_alloc.alloc_str(name) as &str, bt))
        .collect();
    let options = IdeTemplateOptions {
        self_name: "App",
        comments: true,
        is_jsx: false,
        strict_slots: true,
    };

    generate_ide_template(
        &template_ast,
        &oxc_ast,
        source,
        &mut out,
        &tpl_alloc,
        &binding_map,
        &options,
        &TemplateComponentBindings::default(),
    );
    out.apply_to(&mut tpl_ct);

    let full = tpl_ct.build_string();
    let map =
        tpl_ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("test.vue"));
    let tokens: Vec<(u32, u32, u32)> = map
        .get_tokens()
        .filter(|t| t.get_source_id().is_some())
        .map(|t| (t.get_dst_line(), t.get_dst_col(), t.get_src_col()))
        .collect();

    (full, tokens)
}

#[test]
fn strict_slots_sourcemap_component_child() {
    // Verify that the source map has a token mapping the child constructor
    // name back to its position in the template.
    let source = "<template><Tabs><TabItem /></Tabs></template>";
    let (output, tokens) = gen_tsx_template_strict_slots_with_map(
        source,
        &[
            ("Tabs", BindingType::SetupImport),
            ("TabItem", BindingType::SetupImport),
        ],
    );

    // Find `TabItem` position in the source (after `<`)
    let tab_item_src_col = source.find("<TabItem").unwrap() as u32 + 1; // skip `<`

    // The strictRenderSlot call should contain TabItem with a mapped token
    assert!(
        output.contains("strictRenderSlot"),
        "should have strictRenderSlot in output: {}",
        output
    );

    // Find a token that maps to the TabItem source position
    let has_tab_item_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == tab_item_src_col);
    assert!(
        has_tab_item_token,
        "should have a source map token at TabItem position (col {}), tokens: {:?}\noutput: {}",
        tab_item_src_col, tokens, output
    );
}

#[test]
fn strict_slots_sourcemap_html_child() {
    // Verify sourcemap mapping for HTML element children
    let source = "<template><Tabs><input /></Tabs></template>";
    let (output, tokens) =
        gen_tsx_template_strict_slots_with_map(source, &[("Tabs", BindingType::SetupImport)]);

    // `input` position in source (after `<`)
    let input_src_col = source.find("<input").unwrap() as u32 + 1;

    assert!(
        output.contains("HTMLElementTagNameMap[\"input\"]"),
        "should have HTMLElementTagNameMap in output: {}",
        output
    );

    // Find a token that maps to the input source position
    let has_input_token = tokens.iter().any(|&(_dl, _dc, sc)| sc == input_src_col);
    assert!(
        has_input_token,
        "should have a source map token at input position (col {}), tokens: {:?}\noutput: {}",
        input_src_col, tokens, output
    );
}

#[test]
fn strict_slots_v_for_component_var() {
    // v-for introduces a component variable — the strict slot check should use
    // the loop variable name as the constructor reference.
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Tabs v-for="Comp in components"><Comp /></Tabs></template>"#,
        &[
            ("Tabs", BindingType::SetupImport),
            ("components", BindingType::SetupRef),
        ],
    );
    // Positive: strictRenderSlot referencing v-for variable Comp
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    assert!(
        result.contains("'default'"),
        "should reference default slot, got:\n{}",
        result
    );
    // The child constructor should be "Comp" — the v-for loop variable
    // It appears in the strictRenderSlot array
    let slot_call_start = result.find("strictRenderSlot").unwrap();
    let slot_call = &result[slot_call_start..];
    assert!(
        slot_call.contains("Comp"),
        "strictRenderSlot array should contain Comp (v-for variable), got:\n{}",
        slot_call
    );
    // Negative: no raw v-for in output
    assert!(
        !result.contains("v-for"),
        "v-for should not appear in output, got:\n{}",
        result
    );
}

#[test]
fn strict_slots_v_slot_component_var() {
    // v-slot destructures a component — the strict slot check on the inner
    // component should reference the slot variable name.
    let result = gen_tsx_template_strict_slots_with_bindings(
        r#"<template><Provider v-slot="{ Child }"><Tabs><Child /></Tabs></Provider></template>"#,
        &[
            ("Provider", BindingType::SetupImport),
            ("Tabs", BindingType::SetupImport),
        ],
    );
    // Positive: strictRenderSlot on Tabs with Child in the array
    assert!(
        result.contains("strictRenderSlot"),
        "should emit strictRenderSlot, got:\n{}",
        result
    );
    // Find the Tabs strict slot call — it should reference Child
    let slot_call_start = result.find("strictRenderSlot").unwrap();
    let slot_call = &result[slot_call_start..];
    assert!(
        slot_call.contains("Child"),
        "strictRenderSlot array should contain Child (v-slot variable), got:\n{}",
        slot_call
    );
    assert!(
        slot_call.contains("'default'"),
        "should reference default slot, got:\n{}",
        slot_call
    );
    // Negative: raw v-slot should not be in output
    assert!(
        !result.contains("v-slot"),
        "v-slot should not appear in output, got:\n{}",
        result
    );
    // Provider also has children (Tabs) so it should also get a strictRenderSlot call
    let second_call = result.match_indices("strictRenderSlot").nth(1);
    assert!(
        second_call.is_some(),
        "Provider should also get a strictRenderSlot call for its default slot, got:\n{}",
        result
    );
}

// ── Options API component alias resolution ────────────────────────────────

#[test]
fn options_api_component_alias_emits_binding() {
    let source = r#"<script lang="ts">
import { defineComponent } from 'vue'
import SomeComp from './SomeComp.vue'

export default defineComponent({
  components: { MyAlias: SomeComp },
  setup() { return {} }
})
</script>
<template>
  <MyAlias />
</template>"#;
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("Test.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");

    // Template should use <MyAlias> and MyAlias must be in scope
    assert!(
        tsx.code.contains("<MyAlias"),
        "template should contain <MyAlias> JSX tag:\n{}",
        tsx.code
    );
    // There must be a const alias that assigns SomeComp to MyAlias
    assert!(
        tsx.code.contains("const MyAlias = SomeComp"),
        "should emit 'const MyAlias = SomeComp' for the component alias:\n{}",
        tsx.code
    );
    // Must be valid TSX
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "TSX should have no parse errors. Got {} errors:\n{}",
        parsed.errors.len(),
        tsx.code
    );
}

#[test]
fn options_api_component_shorthand_no_alias_needed() {
    // Shorthand: components: { SomeComp } — SomeComp is already imported, no alias needed
    let source = r#"<script lang="ts">
import { defineComponent } from 'vue'
import SomeComp from './SomeComp.vue'

export default defineComponent({
  components: { SomeComp },
  setup() { return {} }
})
</script>
<template>
  <SomeComp />
</template>"#;
    let alloc = Allocator::new();
    let options = crate::compile::CodegenOptions {
        filename: Some("Test.vue".to_string()),
        target: crate::compile::CompileTarget::TSX,
        ..Default::default()
    };
    let verter_opts = crate::compile::VerterCompileOptions::default();
    let result = crate::compile::compile(source, &options, &verter_opts, &alloc);
    let tsx = result.tsx.as_ref().expect("TSX should be generated");

    // SomeComp is already imported — no extra alias declaration needed
    // (it shouldn't break if one IS emitted, but it's unnecessary)
    assert!(
        tsx.code.contains("<SomeComp"),
        "template should contain <SomeComp> JSX tag:\n{}",
        tsx.code
    );
    // Must be valid TSX
    let parsed = oxc_parser::Parser::new(&alloc, &tsx.code, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "TSX should have no parse errors. Got {} errors:\n{}",
        parsed.errors.len(),
        tsx.code
    );
}

// ── Issue #48: $event must not be prefixed with instance ─────────────────

#[test]
fn dollar_event_standalone_not_prefixed() {
    let result = gen_tsx_template(r#"<template><div @click="$event">click</div></template>"#);
    // Positive: $event should appear bare inside the callback
    assert!(
        result.contains("$event"),
        "should contain $event in output: {result}"
    );
    // Negative: $event must NOT be prefixed with ___VERTER___instance.
    assert!(
        !result.contains("___VERTER___instance.$event"),
        "$event must NOT be prefixed with instance, got: {result}"
    );
}

#[test]
fn dollar_event_in_inline_expr_not_prefixed() {
    let result = gen_tsx_template_with_bindings(
        r#"<template><div @click="handleClick($event)">click</div></template>"#,
        &[("handleClick", BindingType::SetupConst)],
    );
    // Positive: handleClick and $event should both be present
    assert!(
        result.contains("handleClick"),
        "should contain handleClick: {result}"
    );
    assert!(result.contains("$event"), "should contain $event: {result}");
    // Negative: $event must NOT be prefixed
    assert!(
        !result.contains("___VERTER___instance.$event"),
        "$event must NOT be prefixed with instance, got: {result}"
    );
}

// ── Issue #46: bare @click (no value) must not emit broken binding ───────

#[test]
fn bare_event_no_value_removed() {
    let result = gen_tsx_template(r#"<template><div @click>click</div></template>"#);
    // Negative: must NOT contain onClick or any broken click binding
    assert!(
        !result.contains("onClick"),
        "bare @click should be removed, must not contain onClick: {result}"
    );
    assert!(
        !result.contains("___VERTER___ctx.click"),
        "bare @click must not produce ctx.click binding: {result}"
    );
    assert!(
        !result.contains("___VERTER___instance.click"),
        "bare @click must not produce instance.click binding: {result}"
    );
}

// ── $event type inference ────────────────────────────────────────────────

#[test]
fn event_handler_native_event_param_is_contextually_typed() {
    // Native `$event` is bound as the handler's sole parameter so it is
    // contextually typed by the JSX event prop (`onClick`) — the same mechanism
    // that types an inline-arrow parameter, so `$event` resolves to the real
    // event type rather than `any`.
    let result =
        gen_tsx_template(r#"<template><div @click="handleClick($event)">click</div></template>"#);
    assert!(
        result.contains("onClick={($event) => {"),
        "native $event handler should bind $event as the contextually-typed parameter: {result}"
    );
    assert!(
        result.contains("handleClick($event)"),
        "should still contain the $event handler body: {result}"
    );
    // Negative: native $event no longer relies on the generic eventCallbacks wrapper.
    assert!(
        !result.contains("___VERTER___eventCallbacks"),
        "native $event should NOT use the generic eventCallbacks wrapper: {result}"
    );
    assert!(
        !result.contains("...___VERTER___eventArgs"),
        "native $event should NOT use the generic event-args rest param: {result}"
    );
}

#[test]
fn event_handler_component_event_param_is_contextually_typed() {
    // A component `$event` is bound the same way — contextually typed by the
    // component's JSX event prop (`onCustom`).
    let result =
        gen_tsx_template(r#"<template><MyComp @custom="handleCustom($event)" /></template>"#);
    assert!(
        result.contains("onCustom={($event) => {"),
        "component $event handler should bind $event as the contextually-typed parameter: {result}"
    );
    assert!(
        !result.contains("___VERTER___eventCallbacks"),
        "component in-place $event should NOT use the generic eventCallbacks wrapper: {result}"
    );
}

#[test]
fn event_handler_without_event_param_no_event_callbacks() {
    // Simple identifier — no eventCallbacks needed
    let result = gen_tsx_template(r#"<template><div @click="handleClick">click</div></template>"#);
    assert!(
        !result.contains("___VERTER___eventCallbacks"),
        "simple ident handler should NOT use eventCallbacks: {result}"
    );

    // Inline expression without $event — no eventCallbacks needed
    let result2 = gen_tsx_template(r#"<template><div @click="count++">click</div></template>"#);
    assert!(
        !result2.contains("___VERTER___eventCallbacks"),
        "inline expr without $event should NOT use eventCallbacks: {result2}"
    );
}

#[test]
fn dollar_event_inside_string_literal_is_not_treated_as_event_param() {
    // Typed-IR detection: `$event` inside a STRING LITERAL is not an identifier
    // reference, so the handler must NOT be wrapped as `($event) => …`. The former
    // `resolved_expr.contains("$event")` substring check wrongly matched here, which
    // would shadow a real outer `$event` and mis-type the handler.
    let result = gen_tsx_template_with_bindings(
        r#"<template><button @click="log('save $event now')" /></template>"#,
        &[("log", BindingType::SetupConst)],
    );
    assert!(
        !result.contains("($event)"),
        "string-literal $event must NOT trigger the $event parameter wrapper: {result}"
    );
    // A plain inline expression with no real $event → wrapped as () => { … }.
    assert!(
        result.contains("onClick={() => {"),
        "inline expression handler should use the () => {{ }} wrapper: {result}"
    );
}

#[test]
fn event_handler_spread_with_event_param_emits_typed_payload() {
    // A duplicate `@click` routes the SECOND handler through the spread path (the
    // first stays an in-place `onClick={…}`). The spread `$event` must be an
    // explicitly-typed parameter — JSX contextual typing does not flow through a
    // spread attribute, so the old generic `eventCallbacks<TArgs extends Array<any>>`
    // wrapper left it `any`. For a native element the annotation is the ambient DOM
    // event-map type keyed by the event name (`click`), which resolves under every
    // TypeProvider (unlike the `import('vue')` formula).
    let result = gen_tsx_template_with_bindings(
        r#"<template><button @click="a($event)" @click="b($event)" /></template>"#,
        &[
            ("a", BindingType::SetupConst),
            ("b", BindingType::SetupConst),
        ],
    );
    // Positive: the spread branch binds `$event` as a typed parameter (the colon is
    // exclusive to the explicitly-annotated spread param; the in-place handler emits
    // a bare `($event) =>`).
    assert!(
        result.contains("($event:"),
        "spread $event must be an explicitly-typed parameter: {result}"
    );
    assert!(
        result.contains(r#"(GlobalEventHandlersEventMap & { [___VERTER___EventKey: string]: Event })["click"]"#),
        "spread $event type must be the ambient DOM event-map type keyed by the event name: {result}"
    );
    // Negative: the `import('vue')` indexed formula is NOT used for native spread
    // `$event` — it does not resolve under the tsgo TypeProvider.
    assert!(
        !result.contains("IntrinsicElementAttributes"),
        "native spread $event must not use the import('vue') formula: {result}"
    );
    // Negative: the generic eventCallbacks helper / rest-args are gone.
    assert!(
        !result.contains("___VERTER___eventCallbacks"),
        "spread event with $event must NOT use the eventCallbacks wrapper: {result}"
    );
    assert!(
        !result.contains("...___VERTER___eventArgs"),
        "spread event must NOT use the generic event-args rest param: {result}"
    );
}

// ── v-if/v-else + v-for lifted chain tests ───────────────────────

#[test]
fn v_if_v_for_followed_by_v_else_v_for() {
    // The primary bug case: sibling elements with v-if+v-for and v-else+v-for
    let result = gen_tsx_template(
        r#"<template><div v-if="show" v-for="item in items" :key="item.id">{{ item.name }}</div><div v-else v-for="item in others" :key="item.id">{{ item.label }}</div></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_followed_by_v_else_v_for ===\n{}\n=== END ===",
        result
    );
    // Positive: should have lifted ternary with condition outside map
    assert!(
        result.contains("show ?") || result.contains("show?"),
        "should have lifted condition outside: {result}"
    );
    // Positive: both branches should have .map()
    let map_count = result.matches(".map(").count();
    assert!(
        map_count >= 2,
        "should have two .map() calls (one per branch), found {map_count}: {result}"
    );
    // Negative: should NOT have bare `else` keyword (IIFE style)
    assert!(
        !result.contains("else{") && !result.contains("else {"),
        "should NOT use IIFE else (should be lifted ternary): {result}"
    );
    // Must be valid TSX
    assert_valid_jsx(
        r#"<template><div v-if="show" v-for="item in items" :key="item.id">{{ item.name }}</div><div v-else v-for="item in others" :key="item.id">{{ item.label }}</div></template>"#,
        "v-if+v-for followed by v-else+v-for",
    );
}

#[test]
fn v_if_v_for_chain_three_branches() {
    let result = gen_tsx_template(
        r#"<template><div v-if="mode === 'a'" v-for="item in listA">{{ item }}</div><div v-else-if="mode === 'b'" v-for="item in listB">{{ item }}</div><div v-else v-for="item in listC">{{ item }}</div></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_chain_three_branches ===\n{}\n=== END ===",
        result
    );
    // Should have 3 .map() calls
    let map_count = result.matches(".map(").count();
    assert!(
        map_count >= 3,
        "should have three .map() calls, found {map_count}: {result}"
    );
    // Should have ternary structure, not IIFE
    assert!(
        !result.contains("else{") && !result.contains("else {"),
        "should NOT use IIFE: {result}"
    );
    // Must be valid TSX
    assert_valid_jsx(
        r#"<template><div v-if="mode === 'a'" v-for="item in listA">{{ item }}</div><div v-else-if="mode === 'b'" v-for="item in listB">{{ item }}</div><div v-else v-for="item in listC">{{ item }}</div></template>"#,
        "three-branch v-if/v-else-if/v-else + v-for chain",
    );
}

#[test]
fn v_if_v_for_mixed_chain_some_with_for_some_without() {
    // v-if+v-for followed by plain v-else (no v-for)
    let result = gen_tsx_template(
        r#"<template><div v-if="show" v-for="item in items">{{ item }}</div><span v-else>fallback</span></template>"#,
    );
    eprintln!("=== v_if_v_for_mixed_chain ===\n{}\n=== END ===", result);
    // Lifted ternary: condition outside
    assert!(
        result.contains("show ?") || result.contains("show?"),
        "should have lifted condition: {result}"
    );
    // First branch has .map(), second doesn't
    assert!(
        result.contains(".map("),
        "first branch should have .map(): {result}"
    );
    assert!(
        result.contains("<span"),
        "second branch should have plain <span>: {result}"
    );
    // Must be valid TSX
    assert_valid_jsx(
        r#"<template><div v-if="show" v-for="item in items">{{ item }}</div><span v-else>fallback</span></template>"#,
        "mixed chain: v-if+v-for then plain v-else",
    );
}

#[test]
fn v_if_v_for_solo_lifts_condition() {
    // Solo v-if + v-for: condition should be lifted outside .map()
    // Vue 3 precedence: v-if has higher precedence, runs before v-for
    let result = gen_tsx_template(
        r#"<template><div v-if="show" v-for="item in list">{{ item }}</div></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_solo_lifts_condition ===\n{}\n=== END ===",
        result
    );
    // Should have lifted ternary: `show ? list.map(...) : null`
    assert!(
        result.contains("show ?") || result.contains("show?"),
        "should have lifted condition outside map: {result}"
    );
    assert!(
        result.contains(": null"),
        "solo lifted should have : null fallback: {result}"
    );
    assert!(result.contains(".map("), "should have .map(): {result}");
}

#[test]
fn v_if_v_for_solo_lifts_condition_before_normal_sibling() {
    // Same as the solo case, but followed by a normal sibling. This still
    // needs the lifted `cond ? map(...) : null` shape.
    let result = gen_tsx_template(
        r#"<template><div v-if="show" v-for="item in list">{{ item }}</div><p>after</p></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_solo_lifts_condition_before_normal_sibling ===\n{}\n=== END ===",
        result
    );
    assert!(
        result.contains("show ?") || result.contains("show?"),
        "should keep the lifted condition even with a following sibling: {result}"
    );
    assert!(
        result.contains(": null"),
        "solo lifted branch should still fall back to null before the next sibling: {result}"
    );
    assert!(
        result.contains("<p"),
        "following sibling should remain present: {result}"
    );
    assert_valid_jsx(
        r#"<template><div v-if="show" v-for="item in list">{{ item }}</div><p>after</p></template>"#,
        "solo v-if+v-for before normal sibling",
    );
}

#[test]
fn v_if_v_for_iife_chain_regression() {
    // Standard v-if/v-else chain WITHOUT any v-for should still use IIFE
    let result =
        gen_tsx_template(r#"<template><div v-if="show">A</div><div v-else>B</div></template>"#);
    // Should use IIFE (if/else), NOT ternary
    assert!(
        result.contains("if(") || result.contains("if ("),
        "no-v-for chain should use IIFE with if(): {result}"
    );
    assert!(
        result.contains("else"),
        "no-v-for chain should have else: {result}"
    );
    // Must be valid TSX
    assert_valid_jsx(
        r#"<template><div v-if="show">A</div><div v-else>B</div></template>"#,
        "IIFE chain regression (no v-for)",
    );
}

#[test]
fn v_if_v_for_statement_body() {
    // v-for should use statement-body callbacks: `=> { return (...) }`
    let result =
        gen_tsx_template(r#"<template><div v-for="item in items">{{ item }}</div></template>"#);
    eprintln!("=== v_if_v_for_statement_body ===\n{}\n=== END ===", result);
    // Should have statement body with return
    assert!(
        result.contains("=> { return"),
        "v-for should use statement body `=> {{ return (...)  }}`, got: {result}"
    );
    // Negative: should NOT have expression body `=> (`
    assert!(
        !result.contains("=> ("),
        "v-for should NOT use expression body `=> (`, got: {result}"
    );
}

#[test]
fn v_if_v_for_numeric_in_lifted_chain() {
    // Numeric v-for in a lifted chain should use Array.from without leading {
    let result = gen_tsx_template(
        r#"<template><div v-if="show" v-for="n in 5">{{ n }}</div><div v-else>none</div></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_numeric_in_lifted_chain ===\n{}\n=== END ===",
        result
    );
    assert!(
        result.contains("Array.from("),
        "numeric v-for should use Array.from: {result}"
    );
    // Must be valid TSX
    assert_valid_jsx(
        r#"<template><div v-if="show" v-for="n in 5">{{ n }}</div><div v-else>none</div></template>"#,
        "numeric v-for in lifted chain",
    );
}

#[test]
fn v_if_v_for_adjacent_chains_independent() {
    // Two separate chains with a <p> separator
    let result = gen_tsx_template(
        "<template><div v-if=\"a\" v-for=\"x in xs\">{{ x }}</div><div v-else>no A</div><p>separator</p><div v-if=\"b\" v-for=\"y in ys\">{{ y }}</div><div v-else>no B</div></template>",
    );
    eprintln!(
        "=== v_if_v_for_adjacent_chains_independent ===\n{}\n=== END ===",
        result
    );
    // Should have 2 separate lifted ternaries
    let map_count = result.matches(".map(").count();
    assert!(
        map_count >= 2,
        "should have at least two .map() calls: {result}"
    );
    assert!(
        result.contains("<p"),
        "separator <p> should be preserved: {result}"
    );
    // Must be valid TSX
    assert_valid_jsx(
        "<template><div v-if=\"a\" v-for=\"x in xs\">{{ x }}</div><div v-else>no A</div><p>separator</p><div v-if=\"b\" v-for=\"y in ys\">{{ y }}</div><div v-else>no B</div></template>",
        "two independent chains",
    );
}

#[test]
fn v_if_v_for_inside_nested_element() {
    // Chain inside a parent div (ElementContent chains, not root)
    let result = gen_tsx_template(
        r#"<template><div><span v-if="show" v-for="item in items">{{ item }}</span><span v-else>none</span></div></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_inside_nested_element ===\n{}\n=== END ===",
        result
    );
    assert!(
        result.contains("show ?") || result.contains("show?"),
        "nested chain should be lifted: {result}"
    );
    // Must be valid TSX
    assert_valid_jsx(
        r#"<template><div><span v-if="show" v-for="item in items">{{ item }}</span><span v-else>none</span></div></template>"#,
        "chain inside nested element",
    );
}

#[test]
fn v_if_v_for_with_comments_between_branches() {
    // Comments between chain members should be suppressed
    let result = gen_tsx_template(
        "<template><div v-if=\"show\" v-for=\"item in items\">{{ item }}</div><!-- separator comment --><div v-else v-for=\"item in others\">{{ item }}</div></template>",
    );
    eprintln!(
        "=== v_if_v_for_with_comments_between_branches ===\n{}\n=== END ===",
        result
    );
    // Must be valid TSX (comments between ternary branches would break)
    assert_valid_jsx(
        "<template><div v-if=\"show\" v-for=\"item in items\">{{ item }}</div><!-- separator comment --><div v-else v-for=\"item in others\">{{ item }}</div></template>",
        "comments between v-if+v-for branches",
    );
}

#[test]
fn v_if_v_for_with_entity_whitespace_between_branches() {
    // Entity-backed whitespace should be treated like ignorable formatting
    // whitespace for v-if / v-else adjacency.
    let result = gen_tsx_template(
        r#"<template><div v-if="show" v-for="item in items">{{ item }}</div>&nbsp;<div v-else>fallback</div></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_with_entity_whitespace_between_branches ===\n{}\n=== END ===",
        result
    );
    assert!(
        result.contains("show ?") || result.contains("show?"),
        "entity-backed whitespace should not break the lifted chain: {result}"
    );
    assert_valid_jsx(
        r#"<template><div v-if="show" v-for="item in items">{{ item }}</div>&nbsp;<div v-else>fallback</div></template>"#,
        "entity whitespace between v-if+v-for branches",
    );
}

#[test]
fn v_if_v_for_v_else_slot_outlet_plain_branch() {
    // Lifted ternary where v-else branch is a plain slot outlet
    let result = gen_tsx_template(
        r#"<template><MyComp><div v-if="show" v-for="item in items">{{ item }}</div><slot v-else name="fallback"/></MyComp></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_v_else_slot_outlet ===\n{}\n=== END ===",
        result
    );
    // Must be valid TSX
    assert_valid_jsx(
        r#"<template><MyComp><div v-if="show" v-for="item in items">{{ item }}</div><slot v-else name="fallback"/></MyComp></template>"#,
        "v-if+v-for then slot v-else",
    );
}

#[test]
fn v_if_v_for_v_else_component_is_plain_branch() {
    // Lifted ternary where v-else is a dynamic component
    let result = gen_tsx_template(
        r#"<template><div v-if="show" v-for="item in items">{{ item }}</div><component v-else :is="fallbackComp"/></template>"#,
    );
    eprintln!(
        "=== v_if_v_for_v_else_component_is ===\n{}\n=== END ===",
        result
    );
    // Must be valid TSX
    assert_valid_jsx(
        r#"<template><div v-if="show" v-for="item in items">{{ item }}</div><component v-else :is="fallbackComp"/></template>"#,
        "v-if+v-for then component :is v-else",
    );
}

// ============================================================================
// Typed EmitOp substrate — IDE-only prefixed-expression emission.
//
// These tests pin the four previously-desynced sites (v-html, v-text,
// dynamic-key bind `:[key]`, native v-model) plus the v-model repeated-
// occurrence contract. Every test asserts BOTH that the user identifier maps
// back to its source byte offset AND that the synthetic prefix/punctuation
// maps to None (no token covers the synthetic generated column).
// ============================================================================

/// Convert a generated byte offset into a (line, col) pair in the generated
/// output, matching the source-map token coordinate space (0-based line, col
/// in UTF-16 code units — ASCII fixtures keep byte==utf16).
fn gen_offset_to_line_col(output: &str, byte_offset: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut col = 0u32;
    for (i, ch) in output.char_indices() {
        if i >= byte_offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += ch.len_utf16() as u32;
        }
    }
    (line, col)
}

/// True iff some mapped token starts exactly at the generated `(line, col)`.
fn has_token_at_gen(tokens: &[(u32, u32, u32)], line: u32, col: u32) -> bool {
    tokens.iter().any(|&(dl, dc, _)| dl == line && dc == col)
}

/// True iff some mapped token maps back to source byte offset `src` (single-line
/// fixtures only — `src_col` equals the byte offset on line 0).
fn has_token_for_src(tokens: &[(u32, u32, u32)], src: u32) -> bool {
    tokens.iter().any(|&(_, _, sc)| sc == src)
}

#[test]
fn v_html_identifier_maps_to_source() {
    // <div v-html="msg"/> → innerHTML={msg}. `msg` maps back; innerHTML=/{/} → None.
    let source = r#"<template><div v-html="msg"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("msg", BindingType::SetupConst)]);

    assert!(
        output.contains("innerHTML={msg}"),
        "v-html should emit innerHTML={{msg}}: {output}"
    );
    assert!(
        !output.contains("v-html"),
        "v-html directive must be removed: {output}"
    );

    // Positive: `msg` maps to its source byte offset.
    let msg_src = source.find("\"msg\"").unwrap() as u32 + 1; // inside quotes
    assert!(
        has_token_for_src(&tokens, msg_src),
        "msg must map to source col {msg_src}. Tokens: {tokens:?}"
    );

    // Negative: the start of `innerHTML=` carries no source mapping.
    let innerhtml_gen = output.find("innerHTML=").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, innerhtml_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "innerHTML= start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}, output: {output}"
    );
    // The `{` immediately before `msg` and the `}` after must also be unmapped.
    let brace_open = output.find("innerHTML={").unwrap() + "innerHTML=".len();
    let (ol, oc) = gen_offset_to_line_col(&output, brace_open);
    assert!(
        !has_token_at_gen(&tokens, ol, oc),
        "innerHTML opening brace (gen {ol}:{oc}) must map to None. Tokens: {tokens:?}"
    );
}

#[test]
fn v_text_identifier_maps_to_source() {
    // <div v-text="content"/> → textContent={content}.
    let source = r#"<template><div v-text="content"/></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("content", BindingType::SetupConst)]);

    assert!(
        output.contains("textContent={content}"),
        "v-text should emit textContent={{content}}: {output}"
    );
    assert!(
        !output.contains("v-text"),
        "v-text directive must be removed: {output}"
    );

    let content_src = source.find("\"content\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, content_src),
        "content must map to source col {content_src}. Tokens: {tokens:?}"
    );

    let textcontent_gen = output.find("textContent=").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, textcontent_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "textContent= start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}"
    );
}

#[test]
fn dynamic_key_bind_both_identifiers_map() {
    // <div :[key]="val"/> → {...{[key]: val}}. Both `key` and `val` map back;
    // `{...{[`, `]: `, `}}}` map to None.
    let source = r#"<template><div :[key]="val"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("key", BindingType::SetupConst),
            ("val", BindingType::SetupConst),
        ],
    );

    assert!(
        output.contains("{...{[key]: val}}"),
        ":[key]=\"val\" should emit {{...{{[key]: val}}}}: {output}"
    );
    // Exact closing — no extra brace (regression: the spread+object closes with
    // exactly `}}`, not `}}}`).
    assert!(
        !output.contains("{...{[key]: val}}}"),
        ":[key] emission must close with exactly `}}}}` (no extra brace): {output}"
    );

    // Positive: both identifiers map back.
    let key_src = source.find("[key]").unwrap() as u32 + 1; // inside the [ ]
    let val_src = source.find("\"val\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, key_src),
        "key must map to source col {key_src}. Tokens: {tokens:?}"
    );
    assert!(
        has_token_for_src(&tokens, val_src),
        "val must map to source col {val_src}. Tokens: {tokens:?}"
    );

    // Negative: the `{...{[` boundary start maps to None.
    let boundary_gen = output.find("{...{[").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, boundary_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "{{...{{[ start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}"
    );
    // The `]: ` separator between key and val maps to None.
    let sep_gen = output.find("]: ").unwrap();
    let (sl, sc) = gen_offset_to_line_col(&output, sep_gen);
    assert!(
        !has_token_at_gen(&tokens, sl, sc),
        "]: separator (gen {sl}:{sc}) must map to None. Tokens: {tokens:?}"
    );
}

#[test]
fn native_vmodel_every_occurrence_maps_back() {
    // <input v-model="count"/> on a native element emits `count` 2-3 times:
    //   value={count} onInput={($event:any) => ((count) = $event)}
    // Every generated occurrence of `count` must map back to the single source
    // span; the assignment punctuation (=>, ($event, =) maps to None.
    let source = r#"<template><input v-model="count"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("count", BindingType::SetupRef)]);

    assert!(
        output.contains("value={"),
        "native v-model should emit value={{...}}: {output}"
    );
    assert!(
        output.contains("onInput={"),
        "native v-model should emit onInput handler: {output}"
    );

    let count_src = source.find("\"count\"").unwrap() as u32 + 1;

    // SetupRef bindings are emitted bare (no prefix) but with a `.value` suffix
    // appended; the identifier text `count` therefore appears at each occurrence.
    // Enumerate ALL generated occurrences of the identifier `count` and assert
    // each one is covered by a token mapping back to the source span.
    let mut occurrence_starts = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel) = output[search_from..].find("count") {
        let at = search_from + rel;
        occurrence_starts.push(at);
        search_from = at + "count".len();
    }
    assert!(
        occurrence_starts.len() >= 2,
        "expected >=2 generated `count` occurrences (read + write), found {}: {output}",
        occurrence_starts.len()
    );

    for at in &occurrence_starts {
        let (gl, gc) = gen_offset_to_line_col(&output, *at);
        let covered = tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == gl && dc == gc && sc == count_src);
        assert!(
            covered,
            "generated `count` occurrence at gen {gl}:{gc} must map back to source col {count_src}. Tokens: {tokens:?}, output: {output}"
        );
    }

    // Negative: the arrow `=>` of the handler maps to None.
    let arrow_gen = output.find("=>").unwrap();
    let (al, ac) = gen_offset_to_line_col(&output, arrow_gen);
    assert!(
        !has_token_at_gen(&tokens, al, ac),
        "arrow => (gen {al}:{ac}) must map to None. Tokens: {tokens:?}"
    );
    // The `($event` parameter list maps to None.
    let ev_gen = output.find("($event").unwrap();
    let (el, ec) = gen_offset_to_line_col(&output, ev_gen);
    assert!(
        !has_token_at_gen(&tokens, el, ec),
        "($event param (gen {el}:{ec}) must map to None. Tokens: {tokens:?}"
    );
}

#[test]
fn vmodel_source_to_generated_selects_read_occurrence() {
    // P2-A: one source span → multiple generated occurrences. The FIRST covering
    // mapped run in generated byte order is the value-binding (read) occurrence,
    // emitted before the assignment LHS. Discriminating: an LHS-first or
    // non-deterministic selection picks the occurrence inside `((count) = $event)`.
    let source = r#"<template><input v-model="count"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("count", BindingType::SetupRef)]);

    let count_src = source.find("\"count\"").unwrap() as u32 + 1;

    // The value-binding occurrence is the one inside `value={...count...}`.
    let value_eq = output.find("value={").expect("value={ in output");
    let assign_lhs = output.find("((").expect("(( assignment LHS in output");
    assert!(
        value_eq < assign_lhs,
        "value binding must be emitted before the assignment LHS: {output}"
    );

    // Collect all tokens that map to count_src, sorted by generated position.
    let mut covering: Vec<(u32, u32)> = tokens
        .iter()
        .filter(|&&(_, _, sc)| sc == count_src)
        .map(|&(dl, dc, _)| (dl, dc))
        .collect();
    covering.sort_unstable();
    assert!(
        !covering.is_empty(),
        "count must have at least one mapped token. Tokens: {tokens:?}"
    );

    // The first covering run (deterministic strict first-covering lookup in
    // generated order) must fall within the value-binding occurrence, NOT the
    // assignment LHS inside `((count) = $event)`.
    let (fl, fc) = covering[0];
    let first_byte = {
        // recover byte offset of (fl, fc) — single fixture, find nth line break
        let mut idx = 0usize;
        let mut line = 0u32;
        let mut col = 0u32;
        for (i, ch) in output.char_indices() {
            if line == fl && col == fc {
                idx = i;
                break;
            }
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += ch.len_utf16() as u32;
            }
            idx = i + ch.len_utf8();
        }
        idx
    };
    assert!(
        first_byte >= value_eq && first_byte < assign_lhs,
        "first covering run (gen {fl}:{fc}, byte {first_byte}) must be the value-binding occurrence \
         in [{value_eq}, {assign_lhs}), not the assignment LHS. Output: {output}"
    );
}

#[test]
fn vmodel_modifier_maps_to_source() {
    // <input v-model.trim="x"/> → the `trim` modifier token maps to its source span.
    let source = r#"<template><input v-model.trim="x"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("x", BindingType::SetupRef)]);

    assert!(
        output.contains("Modifiers={{"),
        "v-model.trim should emit a modifiers prop: {output}"
    );
    assert!(
        output.contains("trim"),
        "modifiers prop should contain `trim`: {output}"
    );

    let trim_src = source.find(".trim").unwrap() as u32 + 1; // the `trim` after the dot
    assert!(
        has_token_for_src(&tokens, trim_src),
        "modifier `trim` must map to source col {trim_src}. Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn vmodel_prefix_not_double_shifted() {
    // P1-B: with a Data binding the identifier is prefixed by `___VERTER___instance.`.
    // The identifier token must map to the FIRST byte of the identifier in
    // generated output (the byte right after the prefix), not shifted into the
    // prefix and not leaving the identifier interior unmapped.
    let source = r#"<template><MyComp v-model="d_val"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("d_val", BindingType::Data)]);

    let needle = "___VERTER___instance.d_val";
    assert!(
        output.contains(needle),
        "Data v-model should emit the instance prefix: {output}"
    );

    let src_col = source.find("\"d_val\"").unwrap() as u32 + 1;

    // The generated `d_val` (after the FIRST prefix) must carry a token that maps
    // to the source identifier, anchored exactly at the identifier start.
    let prefix_pos = output.find(needle).unwrap();
    let ident_gen = prefix_pos + "___VERTER___instance.".len();
    let (il, ic) = gen_offset_to_line_col(&output, ident_gen);
    let anchored = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == il && dc == ic && sc == src_col);
    assert!(
        anchored,
        "d_val must map to source col {src_col} anchored at the identifier start (gen {il}:{ic}), \
         no double shift. Tokens: {tokens:?}, output: {output}"
    );

    // Negative: the prefix start must NOT carry the identifier's mapping.
    let (pl, pc) = gen_offset_to_line_col(&output, prefix_pos);
    let prefix_carries = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == pl && dc == pc && sc == src_col);
    assert!(
        !prefix_carries,
        "the ___VERTER___instance. prefix start (gen {pl}:{pc}) must NOT carry d_val's mapping. \
         Tokens: {tokens:?}"
    );
}

#[test]
fn synthetic_boundary_start_maps_to_none() {
    // P1-C: the generated column at the start of an OverwriteSyntheticBoundary
    // (`innerHTML=` for v-html) maps to None. Discriminating: a Chunk::Overwritten
    // lowering would map that column back to the prop start.
    let source = r#"<template><div v-html="msg"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("msg", BindingType::SetupConst)]);

    let boundary_gen = output.find("innerHTML=").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, boundary_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "innerHTML= boundary start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}, output: {output}"
    );

    // And specifically: no token at that generated column maps to the prop start
    // (the old Chunk::Overwritten bug).
    let prop_start = source.find("v-html").unwrap() as u32;
    let maps_to_prop_start = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == bl && dc == bc && sc == prop_start);
    assert!(
        !maps_to_prop_start,
        "innerHTML= start must NOT map to the prop start (col {prop_start}) — the desync bug. \
         Tokens: {tokens:?}"
    );
}

#[test]
fn vmodel_does_not_emit_single_overwritten_chunk() {
    // The chunk list must contain NO Overwritten chunk spanning both the synthetic
    // prefix and a user identifier. Asserted via the map: the prop-start generated
    // column must NOT carry the identifier's source mapping (a single
    // overwrite(prop.start, prop_end, "value={count}...") would map the whole run
    // — including `value={` — back to prop.start).
    let source = r#"<template><input v-model="count"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("count", BindingType::SetupRef)]);

    let count_src = source.find("\"count\"").unwrap() as u32 + 1;
    let prop_start = source.find("v-model").unwrap() as u32;

    // No token may map a generated position back to the prop start.
    let any_prop_start = tokens.iter().any(|&(_, _, sc)| sc == prop_start);
    assert!(
        !any_prop_start,
        "no generated token may map back to the v-model prop start (col {prop_start}). \
         Tokens: {tokens:?}, output: {output}"
    );

    // The generated `value={` prefix must NOT carry count's mapping.
    let value_gen = output.find("value={").unwrap();
    let (vl, vc) = gen_offset_to_line_col(&output, value_gen);
    let value_carries_count = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == vl && dc == vc && sc == count_src);
    assert!(
        !value_carries_count,
        "the `value={{` synthetic prefix (gen {vl}:{vc}) must NOT carry count's mapping. \
         Tokens: {tokens:?}"
    );
}

#[test]
fn emit_codegen_crlf_and_tabs() {
    // P2-B: a CRLF, tab-indented fixture still maps identifiers exactly.
    let source = "<template>\r\n\t<div v-html=\"msg\" />\r\n</template>";
    let (output, tokens) = gen_tsx_template_with_map(source, &[("msg", BindingType::SetupConst)]);

    assert!(
        output.contains("innerHTML={msg}"),
        "CRLF/tab fixture should still emit innerHTML={{msg}}: {output:?}"
    );

    // `msg` source position: byte offset of the `m` in "msg" (the file has CRLF
    // and a leading tab, so compute the absolute byte offset directly).
    let msg_src = source.find("\"msg\"").unwrap() as u32 + 1;
    // The token's src_col is the column on its source LINE; `msg` is on line 1
    // (0-based), so match by src_col == column within that line.
    let line_start = source[..msg_src as usize]
        .rfind('\n')
        .map(|i| i + 1)
        .unwrap_or(0) as u32;
    let msg_src_col = msg_src - line_start;
    let has_msg = tokens.iter().any(|&(_, _, sc)| sc == msg_src_col);
    assert!(
        has_msg,
        "msg must map to source col {msg_src_col} (line-relative) even with CRLF/tabs. \
         Tokens: {tokens:?}, output: {output:?}"
    );
}

#[test]
fn vmodel_dynamic_arg_modifier_maps_and_is_valid() {
    // <input v-model:[eventName].trim="val"/> — dynamic arg + modifier.
    // The modifiers prop name must be the COMPUTED `[`${...}Modifiers`]` name with
    // the arg expression embedded, NOT an empty JSX attribute name (` ={{`), which
    // is invalid TSX. The embedded arg `eventName` must map back to its source span.
    let source = r#"<template><input v-model:[eventName].trim="val"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("eventName", BindingType::SetupConst),
            ("val", BindingType::SetupRef),
        ],
    );

    // Positive: a computed `[`${...}Modifiers`]` prop name is present.
    assert!(
        output.contains("Modifiers`]"),
        "dynamic-arg v-model with a modifier must emit a computed `[`${{...}}Modifiers`]` \
         prop name: {output}"
    );
    // Negative: the empty-attribute-name shape ` ={{` (the regression) must NOT appear.
    assert!(
        !output.contains(" ={{"),
        "dynamic-arg v-model must NOT emit an empty JSX attribute name ` ={{` (invalid TSX). \
         The computed modifiers name was dropped: {output}"
    );

    // The arg identifier `eventName` must map back to its source span. The arg
    // appears multiple times (computed prop name, event key, modifiers name); at
    // least one occurrence maps back.
    let arg_src = source.find("[eventName]").unwrap() as u32 + 1; // inside the [ ]
    assert!(
        has_token_for_src(&tokens, arg_src),
        "v-model dynamic arg `eventName` must map to source col {arg_src}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // The whole emission must be valid TSX (no empty attribute name, balanced
    // braces). Wrap as a JSX element attribute list and parse.
    let wrapper = format!("const x = <input {} />", output_attrs(&output));
    let val_alloc = oxc_allocator::Allocator::new();
    let parsed = oxc_parser::Parser::new(&val_alloc, &wrapper, oxc_span::SourceType::tsx()).parse();
    assert!(
        parsed.errors.is_empty(),
        "dynamic-arg v-model + modifier must produce valid TSX. Errors: {:?}\n--- output ---\n{}",
        parsed
            .errors
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        output
    );
}

/// Extract the attribute portion of a generated single-element template
/// (`<input ...attrs.../>`) for re-parsing as a JSX attribute list.
fn output_attrs(output: &str) -> String {
    // Strip the leading `<input` / `<tag` and trailing `/>` or `>` so the inner
    // attribute list can be re-wrapped in a fresh element for syntax validation.
    let after_tag = output
        .find(char::is_whitespace)
        .map(|i| &output[i..])
        .unwrap_or(output);
    let trimmed = after_tag.trim();
    let body = trimmed
        .strip_suffix("/>")
        .or_else(|| trimmed.strip_suffix('>'))
        .unwrap_or(trimmed);
    body.trim().to_string()
}

#[test]
fn v_on_object_spread_handler_maps_to_source() {
    // <div v-on="{ mousedown: doThis }"/> → {...{ mousedown: doThis }}.
    // The handler identifier `doThis` is a navigable user expression — it MUST map
    // back to its source span. The object punctuation (`{...{`, `: `, `}}`) and the
    // event key map to None.
    let source = r#"<template><div v-on="{ mousedown: doThis }"/></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("doThis", BindingType::SetupConst)]);

    assert!(
        output.contains("{...{"),
        "v-on object literal should emit a spread `{{...{{ ... }}}}`: {output}"
    );
    assert!(
        !output.contains("v-on"),
        "v-on directive must be removed: {output}"
    );

    // Positive: `doThis` maps to its source byte offset.
    let handler_src = source.find("doThis").unwrap() as u32;
    assert!(
        has_token_for_src(&tokens, handler_src),
        "v-on handler `doThis` must map to source col {handler_src}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // Negative: the `{...{` spread boundary start maps to None (the old baked
    // overwrite mapped the whole run — including the handler — back to prop.start).
    let boundary_gen = output.find("{...{").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, boundary_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "{{...{{ spread boundary start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}"
    );

    // Negative: no generated token may map back to the v-on prop start (the desync).
    let prop_start = source.find("v-on").unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == prop_start),
        "no generated token may map back to the v-on prop start (col {prop_start}). \
         Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn v_on_dynamic_event_name_expr_maps() {
    // <div @[event]="handler"/> → {...{[`on${event}` as any]: handler}}.
    // BOTH the dynamic event-name expression `event` and the handler `handler` are
    // navigable user expressions — each must map back to its source span. The
    // computed-key template literal and object punctuation map to None.
    let source = r#"<template><div @[event]="handler"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("event", BindingType::SetupConst),
            ("handler", BindingType::SetupConst),
        ],
    );

    assert!(
        output.contains("as any]:"),
        "dynamic event name should emit the computed-key spread `[`on${{...}}` as any]: ...`: {output}"
    );
    assert!(
        !output.contains("@["),
        "dynamic event syntax must be removed: {output}"
    );

    // Positive: both `event` (arg) and `handler` (value) map back to source.
    let event_src = source.find("[event]").unwrap() as u32 + 1; // inside the [ ]
    let handler_src = source.find("\"handler\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, event_src),
        "dynamic event-name expr `event` must map to source col {event_src}. \
         Tokens: {tokens:?}, output: {output}"
    );
    assert!(
        has_token_for_src(&tokens, handler_src),
        "dynamic event handler `handler` must map to source col {handler_src}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // Negative: the `{...{[` boundary start maps to None.
    let boundary_gen = output.find("{...{[").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, boundary_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "{{...{{[ boundary start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}"
    );

    // Negative: no generated token may map back to the prop start (the desync).
    let prop_start = source.find('@').unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == prop_start),
        "no generated token may map back to the @[event] prop start (col {prop_start}). \
         Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn v_show_condition_maps_to_source() {
    // <div v-show="visible"/> → style={{display: visible ? undefined : 'none'}}.
    // The condition `visible` is a navigable user expression relocated into the
    // synthetic style attribute; it MUST map back. The `style={{display: ` prefix
    // and ` ? undefined : 'none'}}` suffix map to None.
    let source = r#"<template><div v-show="visible"/></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("visible", BindingType::SetupConst)]);

    assert!(
        output.contains("display:"),
        "v-show should emit a display style: {output}"
    );
    assert!(
        !output.contains("v-show"),
        "v-show directive must be removed: {output}"
    );

    // Positive: `visible` maps to its source byte offset.
    let visible_src = source.find("\"visible\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, visible_src),
        "v-show condition `visible` must map to source col {visible_src}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // Negative: the `style={{display: ` boundary start maps to None (the old baked
    // overwrite mapped the whole run — including `visible` — back to prop.start).
    let boundary_gen = output.find("style={{display:").unwrap();
    let (bl, bc) = gen_offset_to_line_col(&output, boundary_gen);
    assert!(
        !has_token_at_gen(&tokens, bl, bc),
        "style={{{{display: boundary start (gen {bl}:{bc}) must map to None. Tokens: {tokens:?}"
    );

    // Negative: no generated token may map back to the v-show prop start.
    let prop_start = source.find("v-show").unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == prop_start),
        "no generated token may map back to the v-show prop start (col {prop_start}). \
         Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn v_if_guarded_value_binding_maps_to_source() {
    // <div v-if="ok" :onSomething="() => handle()"/> exercises the props.rs
    // `guarded.is_some()` branch: a function-typed value prop under a v-if narrowing
    // guard. The guard `!((ok))?undefined:` is injected into the MIDDLE of the
    // expression (after `=>`). Pre-fix the WHOLE expression — guard plus the user
    // body `handle()` — was baked into one mapped `out.overwrite(arg_end, prop_end,
    // &close)`, so the body identifier `handle` mapped to the foreign overwrite
    // start (arg_end) instead of its own source span → ctrl+click failed. Post-fix
    // the value is preserved in place (each identifier mapped) and ONLY the guard
    // text is unmapped.
    let source = r#"<template><div v-if="ok" :onSomething="() => handle()"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("ok", BindingType::SetupConst),
            ("handle", BindingType::SetupConst),
        ],
    );

    // The narrowing guard must still be present (semantics unchanged).
    assert!(
        output.contains("?undefined:"),
        "function-typed prop under v-if must still get the ternary narrowing guard: {output}"
    );
    assert!(
        !output.contains("v-if"),
        "v-if directive must be removed: {output}"
    );

    // Positive: the user body identifier `handle` maps back to its source byte offset.
    let handle_src = source.find("handle()").unwrap() as u32;
    assert!(
        has_token_for_src(&tokens, handle_src),
        "the guarded value body `handle` must map to source col {handle_src} (NOT the foreign \
         overwrite start). Pre-fix the baked overwrite mapped the whole run to arg_end. \
         Tokens: {:?}, output: {output}",
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );

    // Negative: the injected guard text maps to None. Locate the generated `?undefined:`
    // (the ternary guard tail) and assert no token starts there.
    let guard_gen = output.find("?undefined:").unwrap();
    let (gl, gc) = gen_offset_to_line_col(&output, guard_gen);
    assert!(
        !has_token_at_gen(&tokens, gl, gc),
        "the injected guard `?undefined:` (gen {gl}:{gc}) must map to None. Tokens: {tokens:?}"
    );

    // Negative: no generated token may map back to the prop start (`:` of :onSomething)
    // — that was the desync anchor.
    let prop_start = source.find(":onSomething").unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == prop_start),
        "no generated token may map back to the :onSomething prop start (col {prop_start}). \
         Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn v_if_guarded_inline_handler_maps_to_source() {
    // <div v-if="ok" @click="count++"/> — an inline v-on handler under a v-if
    // narrowing guard. The guard `() => {if (!((ok))) { return undefined; } ` wraps
    // the handler body. The user body `count++` must map back to source; the `() => {`
    // wrapper and the guard map to None. (The von.rs handler emission already
    // preserves the body in place via the prefix/suffix boundary split; this test
    // pins that invariant so a regression that bakes the body would be caught.)
    let source = r#"<template><div v-if="ok" @click="count++"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("ok", BindingType::SetupConst),
            ("count", BindingType::SetupRef),
        ],
    );

    assert!(
        output.contains("return undefined"),
        "inline handler under v-if must still get the narrowing guard: {output}"
    );
    assert!(
        output.contains("() => {"),
        "inline handler must be wrapped in an arrow body: {output}"
    );

    // Positive: the user body identifier `count` maps back to its OWN source byte
    // offset, AND the generated `count` token sits at the generated body position
    // (preserved in place, not relocated to a foreign anchor).
    let count_src = source.find("count++").unwrap() as u32;
    let count_gen = output.find("count++").unwrap() as u32;
    let (cgl, cgc) = gen_offset_to_line_col(&output, count_gen as usize);
    assert!(
        tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == cgl && dc == cgc && sc == count_src),
        "the guarded inline handler body `count` (gen {cgl}:{cgc}) must map to its own source \
         col {count_src}. Tokens: {:?}, output: {output}",
        tokens.iter().map(|t| (t.1, t.2)).collect::<Vec<_>>()
    );

    // Negative: the `() => {` arrow wrapper start maps to None.
    let wrapper_gen = output.find("() => {").unwrap();
    let (wl, wc) = gen_offset_to_line_col(&output, wrapper_gen);
    assert!(
        !has_token_at_gen(&tokens, wl, wc),
        "the `() => {{` arrow wrapper (gen {wl}:{wc}) must map to None. Tokens: {tokens:?}"
    );

    // Negative: the generated body identifier `count` must NOT map to the @click prop
    // start (the desync anchor). (The synthetic `onClick` prop NAME legitimately maps
    // near the event arg — this assertion targets the BODY identifier specifically.)
    let prop_start = source.find("@click").unwrap() as u32;
    assert!(
        !tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == cgl && dc == cgc && sc == prop_start),
        "the body identifier `count` (gen {cgl}:{cgc}) must not map to the @click prop start \
         (col {prop_start}). Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn v_show_merged_style_both_expressions_map() {
    // <div v-show="ready" :style="itemStyle"/> → the v-show condition merges into
    // the existing :style. BOTH `itemStyle` and `ready` are navigable and must map
    // back; the synthetic `style={{...(`, `), display: `, ` ? undefined ...}}` is None.
    let source = r#"<template><div v-show="ready" :style="itemStyle"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("ready", BindingType::SetupConst),
            ("itemStyle", BindingType::SetupConst),
        ],
    );

    assert!(
        output.matches("style=").count() == 1,
        "v-show + :style must merge into one style attribute: {output}"
    );
    assert!(
        output.contains("display:"),
        "merged style should include the display condition: {output}"
    );

    let ready_src = source.find("\"ready\"").unwrap() as u32 + 1;
    let item_src = source.find("\"itemStyle\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, ready_src),
        "v-show condition `ready` must map to source col {ready_src}. \
         Tokens: {tokens:?}, output: {output}"
    );
    assert!(
        has_token_for_src(&tokens, item_src),
        ":style binding `itemStyle` must map to source col {item_src}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // Negative: neither the v-show nor :style prop start carries a mapping.
    let show_start = source.find("v-show").unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == show_start),
        "no generated token may map back to the v-show prop start (col {show_start}). \
         Tokens: {tokens:?}, output: {output}"
    );
}

#[test]
fn migrated_sites_binding_notation_characterization() {
    // P3 characterization — pin the prop-accessor notation the migrated relocated
    // emitters produce, so it is INTENTIONAL, not accidental:
    //
    // 1. A keyword-named prop accessed as a bindingless SIMPLE identifier (e.g.
    //    `v-show="class"`) → BRACKET notation (`__props["class"]`). `emit_relocated_value`
    //    routes a bindingless simple identifier through `resolve_simple_expr`, which
    //    emits the bracket form for keywords (dot notation `__props.class` is valid TS
    //    too, but bracket matches the pre-migration shared-helper behaviour).
    let v_show_kw = gen_tsx_template_with_bindings(
        r#"<template><div v-show="class"/></template>"#,
        &[("class", BindingType::Props)],
    );
    assert!(
        v_show_kw.contains(r#"__props["class"]"#),
        "v-show keyword prop must use bracket notation `__props[\"class\"]`: {v_show_kw}"
    );
    assert!(
        !v_show_kw.contains("__props.class"),
        "v-show keyword prop must NOT use dot notation `__props.class`: {v_show_kw}"
    );

    // 2. A non-keyword prop emitted through the v-on object-spread substrate (where
    //    OXC DOES extract the binding) → DOT notation (`__props.handler`), identical to
    //    the in-place `@click="handler"` form. The migration keeps the two consistent.
    let v_on_obj = gen_tsx_template_with_bindings(
        r#"<template><div v-on="{ click: handler }"/></template>"#,
        &[("handler", BindingType::Props)],
    );
    assert!(
        v_on_obj.contains("__props.handler"),
        "v-on object-spread Props handler must use dot notation `__props.handler`: {v_on_obj}"
    );
    let at_click = gen_tsx_template_with_bindings(
        r#"<template><div @click="handler"/></template>"#,
        &[("handler", BindingType::Props)],
    );
    assert!(
        at_click.contains("__props.handler"),
        "@click Props handler uses dot notation (the spread form must match it): {at_click}"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// Unified user-expression emission — discriminating tests.
//
// These pin that every IDE user-expression emission routes through the single
// `plan_user_expr` planner + InPlace/Relocated sinks (and the object-literal layer
// above it): v-on object shorthand emits value-only (no doubled key), non-object
// `v-on="obj"` spreads a mapped expr, relocated ignored locals stay mapped, `:ref`
// values stay navigable, and a redundant native v-model still emits its modifiers.
// Each fails against the pre-unification hand-rolled emission.
// ─────────────────────────────────────────────────────────────────────────────

/// Q1 — `v-on="{ click }"` shorthand object property. The object-literal layer
/// emits the synthetic event key `onClick: `, then the shorthand VALUE in
/// value-only mode → `onClick: __props.click`. Pre-refactor the value was emitted
/// through `emit_relocated_value`, whose `emit_one_occurrence` saw the binding's
/// `is_shorthand=true` flag and re-expanded the key → the broken double
/// `onClick: click: __props.click` (or `onClick: click` when no prefix applies).
#[test]
fn v_on_object_shorthand_emits_value_only() {
    // Props binding → `__props.` prefix so the shorthand re-expansion is visible.
    let source = r#"<template><div v-on="{ click }"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("click", BindingType::Props)]);

    // Positive: exactly the value-only form. The event key `onClick:` is emitted
    // once by the object layer; the value is the resolved `__props.click`.
    assert!(
        output.contains("onClick: __props.click"),
        "v-on shorthand must emit `onClick: __props.click` (value-only): {output}"
    );
    // Negative: NO double key. The pre-refactor bug emitted the key twice.
    assert!(
        !output.contains("onClick: click: "),
        "v-on shorthand must NOT double the key (`onClick: click: __props.click`): {output}"
    );
    assert!(
        !output.contains("click: __props.click: "),
        "v-on shorthand must NOT emit a stray `click:` shorthand expansion: {output}"
    );

    // The VALUE identifier `click` maps back to its source span.
    let click_src = source.find("{ click }").unwrap() as u32 + "{ ".len() as u32;
    assert!(
        has_token_for_src(&tokens, click_src),
        "v-on shorthand value `click` must map to source col {click_src}. Tokens: {tokens:?}, output: {output}"
    );
}

/// Q1 — `v-on="handlers"` where the value is NOT an object literal. The whole
/// expression spreads as `{...<mapped user expr>}` and the identifier maps back to
/// source. Pre-refactor the non-object branch resolved through
/// `rewrite_v_on_object_literal_expr` + a flat UNMAPPED insert, so `handlers` had
/// no source mapping (ctrl+click failed).
#[test]
fn v_on_non_object_expr_spread_maps() {
    let source = r#"<template><div v-on="handlers"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("handlers", BindingType::Props)]);

    assert!(
        output.contains("{...__props.handlers}"),
        "v-on non-object expr must spread the mapped user expr `{{...__props.handlers}}`: {output}"
    );

    // Positive: `handlers` maps to its source span.
    let handlers_src = source.find("\"handlers\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, handlers_src),
        "v-on non-object expr `handlers` must map to source col {handlers_src}. Tokens: {tokens:?}, output: {output}"
    );

    // Negative: nothing maps back to the prop start.
    let prop_start = source.find("v-on").unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == prop_start),
        "no generated token may map back to the v-on prop start (col {prop_start}). Tokens: {tokens:?}, output: {output}"
    );
}

/// Q1 — an IGNORED local binding (a v-slot scoped local) used in a RELOCATED value
/// (here a v-on object handler) must emit the MAPPED bare identifier — no accessor
/// prefix/suffix, but a source-map token so ctrl+click lands on the local. The
/// relocated-value path must map ignored locals, not emit them unmapped.
#[test]
fn relocated_value_ignored_local_binding_maps() {
    // `item` is a v-slot scoped local; inside `v-on="{ click: item }"` it is an
    // ignored binding. It stays bare (no prefix) AND maps back to its source span.
    let source =
        r#"<template><Comp v-slot="{ item }"><div v-on="{ click: item }"/></Comp></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    // The handler value `item` is a scoped local → bare (no `__props.` / `_ctx.`).
    assert!(
        output.contains("onClick: item"),
        "ignored local handler must stay bare `onClick: item`: {output}"
    );
    assert!(
        !output.contains("onClick: _ctx.item") && !output.contains("onClick: __props.item"),
        "ignored local must NOT be prefixed: {output}"
    );

    // The `item` INSIDE the v-on object must map to its source span. Two `item`
    // tokens exist (the v-slot destructure + the handler); assert a token maps to
    // the handler occurrence specifically.
    let handler_item_src = source.find("click: item").unwrap() as u32 + "click: ".len() as u32;
    assert!(
        has_token_for_src(&tokens, handler_item_src),
        "ignored local handler `item` must map to source col {handler_item_src} (emitted MAPPED, not unmapped). Tokens: {tokens:?}, output: {output}"
    );
}

/// Q3 — dynamic `:ref="expr"` → `ref={expr}` IN PLACE. The parser routes a dynamic
/// `:ref` through `el.props` → `process_v_bind` (static-key path), which preserves
/// the value expression in place: the VALUE identifier `myRef` maps to its source
/// span, and (the desync check) it must NOT collapse to the foreign prop start.
/// The `ref` JSX attribute NAME is the preserved `ref` arg token and legitimately
/// maps to source — that is NOT the desync (the desync was a baked VALUE).
#[test]
fn ref_expr_value_maps_to_source() {
    let source = r#"<template><div :ref="myRef"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("myRef", BindingType::SetupRef)]);

    assert!(
        output.contains("ref={myRef}"),
        ":ref should emit ref={{myRef}} in place: {output}"
    );
    assert!(
        !output.contains(":ref"),
        ":ref directive must be removed: {output}"
    );

    // Positive: the VALUE identifier `myRef` maps to its source byte offset.
    let myref_src = source.find("\"myRef\"").unwrap() as u32 + 1;
    let value_gen_col = output.find("ref={myRef}").unwrap() as u32 + "ref={".len() as u32;
    let value_maps_to_source = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == value_gen_col && sc == myref_src);
    assert!(
        value_maps_to_source,
        ":ref VALUE `myRef` (gen col {value_gen_col}) must map to source col {myref_src}. \
         Tokens: {tokens:?}, output: {output}"
    );

    // Negative (the desync): the value identifier must NOT collapse to the `:` prop
    // start (a baked `out.overwrite(prop.start, .., &format!(\"ref={{{}}}\", value))`
    // would map the value back to the prop start).
    let prop_start = source.find(":ref").unwrap() as u32;
    let value_maps_to_prop_start = tokens
        .iter()
        .any(|&(_dl, dc, sc)| dc == value_gen_col && sc == prop_start);
    assert!(
        !value_maps_to_prop_start,
        ":ref VALUE must not map to the prop start (col {prop_start}). Tokens: {tokens:?}, output: {output}"
    );
}

/// Q3 — static `ref="myRef"` → `ref={"myRef"}` is a STRING LITERAL ref (non-navigable).
/// It must be emitted as an UNMAPPED synthetic replacement (delete + unmapped insert),
/// NOT a mapped `out.overwrite` whose `ref={"…"}` run maps back to the prop start.
#[test]
fn static_ref_emits_unmapped_string_literal() {
    let source = r#"<template><div ref="myRef"/></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[]);

    assert!(
        output.contains(r#"ref={"myRef"}"#),
        "static ref should emit ref={{\"myRef\"}}: {output}"
    );

    // The `ref={"…"}` synthetic run is unmapped — no generated token maps back to
    // the `ref` prop start (the string literal carries no navigation).
    let prop_start = source.find("ref=").unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == prop_start),
        "static ref's synthetic `ref={{\"…\"}}` must not map back to the prop start (col {prop_start}). \
         Tokens: {tokens:?}, output: {output}"
    );
}

/// Q4 — native v-model whose value/event generation is redundant
/// (`has_explicit_prop && has_explicit_handler`) but which carries MODIFIERS. The
/// modifiers prop MUST still be emitted. Pre-refactor `empty_replacement = true`
/// suppressed the whole emission INCLUDING modifiers — the modifier was silently
/// dropped.
#[test]
fn vmodel_redundant_still_emits_modifiers() {
    // <input v-model.trim="x" :value="..." @input="..."> — both the value prop and
    // the input handler are explicitly present, so value/event generation is
    // redundant; only the `.trim` modifier prop should be emitted.
    let source = r#"<template><input v-model.trim="x" :value="x" @input="e => x = e.target.value"/></template>"#;
    let (output, _tokens) = gen_tsx_template_with_map(source, &[("x", BindingType::SetupRef)]);

    // The modifiers prop must survive even though value/event are suppressed.
    assert!(
        output.contains("modelModifiers={{"),
        "redundant native v-model with modifiers MUST still emit the modifiers prop: {output}"
    );
    assert!(
        output.contains("trim: true"),
        "the `.trim` modifier must be emitted as `trim: true`: {output}"
    );
}

/// Q2 — the broken-interpolation recovery path's keyword-bracket case
/// (`SynthesizedResolved`) routes through the unified synthesized-shorthand emission
/// instead of a baked `out.overwrite(ident.start, ident.end, &resolved)`. `class` is
/// a JS keyword used as a Props member → `__props["class"]`; the `class` core must
/// map to its source token and must NOT collapse the whole bracket form onto the
/// prop start.
#[test]
fn broken_interpolation_keyword_member_maps_via_synthesized() {
    let source = r#"<template><div>{{ class + }}</div></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(source, &[("class", BindingType::Props)]);

    // Keyword member → bracket notation (dot would be a syntax error).
    assert!(
        output.contains(r#"__props["class"]"#),
        "broken-interpolation keyword member must resolve to bracket notation: {output}"
    );

    // The `class` core inside the brackets maps to its source token.
    let class_src = source.find("class").unwrap() as u32;
    let bracket_gen = output.find(r#"__props["class"]"#).unwrap();
    let core_gen = bracket_gen + r#"__props[""#.len();
    let (cl, cc) = gen_offset_to_line_col(&output, core_gen);
    let core_maps = tokens
        .iter()
        .any(|&(dl, dc, sc)| dl == cl && dc == cc && sc == class_src);
    assert!(
        core_maps,
        "the `class` core (gen {cl}:{cc}) must map to source col {class_src}. Tokens: {tokens:?}, output: {output}"
    );

    // The `__props["` prefix start must be unmapped (a baked overwrite would map it
    // back to the identifier/prop position).
    let (pl, pc) = gen_offset_to_line_col(&output, bracket_gen);
    assert!(
        !has_token_at_gen(&tokens, pl, pc),
        "the `__props[\"` prefix start (gen {pl}:{pc}) must map to None. Tokens: {tokens:?}, output: {output}"
    );
}

/// The v-on SPREAD path (hyphenated event name → `{...{"onMy-event": …}}`) keeps
/// the event NAME navigable: the `onMy-event` string key maps to the source event
/// token so hover / go-to-definition on a component `@my-event` resolves the child's
/// `onMyEvent` payload. The handler value also maps. Byte-identical output to the
/// pre-unification baked spread (only the source map gains the event-name token).
#[test]
fn v_on_spread_event_name_and_handler_map_to_source() {
    let source = r#"<template><MyComp @my-event="handler"/></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("handler", BindingType::SetupConst)]);

    // Hyphenated JSX event name forces the spread form.
    assert!(
        output.contains(r#"{...{"onMy-event": handler}}"#),
        "hyphenated @my-event must spread as {{...{{\"onMy-event\": handler}}}}: {output}"
    );

    // The event-name key maps to the source `my-event` token.
    let event_src = source.find("my-event").unwrap() as u32;
    let key_gen = output.find(r#""onMy-event""#).unwrap() + 1; // past the opening quote
    let (kl, kc) = gen_offset_to_line_col(&output, key_gen);
    assert!(
        tokens.iter().any(|&(dl, dc, sc)| dl == kl && dc == kc && sc == event_src),
        "the `onMy-event` key (gen {kl}:{kc}) must map to the source event token col {event_src}. Tokens: {tokens:?}, output: {output}"
    );

    // The handler value maps to its source span.
    let handler_src = source.find("\"handler\"").unwrap() as u32 + 1;
    assert!(
        has_token_for_src(&tokens, handler_src),
        "spread handler `handler` must map to source col {handler_src}. Tokens: {tokens:?}, output: {output}"
    );
}

/// Q1 — atomicity / unification sanity: the v-on object-spread handler still maps
/// to source through the unified planner (kept green across the refactor).
#[test]
fn v_on_object_spread_handler_still_maps_after_unify() {
    let source = r#"<template><div v-on="{ mousedown: doThis }"/></template>"#;
    let (output, tokens) =
        gen_tsx_template_with_map(source, &[("doThis", BindingType::SetupConst)]);
    let handler_src = source.find("doThis").unwrap() as u32;
    assert!(
        has_token_for_src(&tokens, handler_src),
        "v-on object handler `doThis` must still map after unify. Tokens: {tokens:?}, output: {output}"
    );
}

/// An inline v-on handler under a v-if narrowing guard. The handler
/// boundary was baked into a single flat `boundary_prefix` string and emitted via
/// one mapped `out.overwrite(prop_start, trimmed_vs, boundary_prefix)`, so the
/// generated `onClick={() => {if (!((ready))) { return undefined; } ` run — the
/// event NAME plus all the synthetic wrapper/guard scaffolding — mapped back to the
/// `@click` prop start (a foreign anchor). Post-fix the boundary is decomposed
/// through the typed `EmitOp` substrate:
///   - the synthetic JSX wrapper (`onClick={`, `() => {`) and the v-if narrowing
///     guard (`if (!((ready))) { return undefined; }`) map to None. The guard is a
///     COMPOSED, span-erased compiler-synthesized narrowing scaffold (own positive +
///     sibling negations from OTHER elements + ancestor scopes, already flattened to
///     a string and joined with synthetic `!(…) && (…)`); it has no single source
///     span, so it is synthetic text → None (consistent with the sibling
///     `process_v_bind` guarded-value path, whose `?undefined:` also maps to None).
///   - the event NAME `onClick` maps to the SOURCE event token (`click` arg), NOT
///     the `@click` prop start.
///   - the handler BODY `count++` stays in place, mapped to its own source span.
#[test]
fn v_if_guarded_inline_handler_guard_maps_to_none() {
    let source = r#"<template><button @click="count++" v-if="ready">x</button></template>"#;
    let (output, tokens) = gen_tsx_template_with_map(
        source,
        &[
            ("ready", BindingType::SetupConst),
            ("count", BindingType::SetupConst),
        ],
    );

    // Semantics unchanged: the narrowing guard is still emitted.
    assert!(
        output.contains("return undefined"),
        "inline handler under v-if must still get the narrowing guard: {output}"
    );
    assert!(
        output.contains("onClick={() => {"),
        "inline handler must emit the onClick arrow wrapper: {output}"
    );

    // Positive: the handler BODY identifier `count` maps to its OWN source span, at
    // the generated body position (preserved in place, not a foreign anchor).
    let count_src = source.find("count++").unwrap() as u32;
    let count_gen = output.find("count++").unwrap();
    let (cgl, cgc) = gen_offset_to_line_col(&output, count_gen);
    assert!(
        tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == cgl && dc == cgc && sc == count_src),
        "guarded inline handler body `count` (gen {cgl}:{cgc}) must map to its own source col \
         {count_src}. Tokens: {:?}, output: {output}",
        tokens.iter().map(|t| (t.0, t.1, t.2)).collect::<Vec<_>>()
    );

    // Positive: the event NAME `onClick` maps to the SOURCE event arg token (`click`),
    // exactly like the v-on spread branch maps `arg_start`.
    let event_arg_src = source.find("click").unwrap() as u32; // `click` arg of `@click`
    let onclick_gen = output.find("onClick={").unwrap();
    let (ekl, ekc) = gen_offset_to_line_col(&output, onclick_gen);
    assert!(
        tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == ekl && dc == ekc && sc == event_arg_src),
        "event name `onClick` (gen {ekl}:{ekc}) must map to the source event token col \
         {event_arg_src}. Tokens: {tokens:?}, output: {output}"
    );

    // Negative (the desync): NO generated token may map back to the `@click` prop
    // start (the desync anchor). Pre-fix the baked `boundary_prefix` overwrite mapped
    // the whole `onClick={() => {…guard…} ` run to the prop start.
    let prop_start = source.find("@click").unwrap() as u32;
    assert!(
        !tokens.iter().any(|&(_, _, sc)| sc == prop_start),
        "no generated token may map back to the @click prop start (col {prop_start}) — the \
         baked-boundary desync. Tokens: {tokens:?}, output: {output}"
    );

    // Negative: the injected guard text maps to None. The `if (!((ready)))` narrowing
    // scaffold is compiler-synthesized → unmapped.
    let guard_gen = output.find("if (!((ready)))").unwrap();
    let (gl, gc) = gen_offset_to_line_col(&output, guard_gen);
    assert!(
        !has_token_at_gen(&tokens, gl, gc),
        "the injected guard `if (!((ready)))` (gen {gl}:{gc}) must map to None. \
         Tokens: {tokens:?}, output: {output}"
    );

    // Negative: the `() => {` arrow wrapper start maps to None.
    let wrapper_gen = output.find("() => {").unwrap();
    let (wl, wc) = gen_offset_to_line_col(&output, wrapper_gen);
    assert!(
        !has_token_at_gen(&tokens, wl, wc),
        "the `() => {{` arrow wrapper (gen {wl}:{wc}) must map to None. Tokens: {tokens:?}"
    );
}

/// `emit_synthesized_shorthand_value`'s no-core fallback. When the
/// derived value `core` is NOT a substring of the resolver output `resolved` (the
/// resolver rewrote the expression so the core token is absent), the value is not
/// precisely mappable. Pre-fix the fallback mapped the ENTIRE synthetic `resolved`
/// string to the user source token — violating "synthetic text maps to None". Per
/// the prove-or-drop principle a feature drop (None mapping) is acceptable, a mismap
/// is not. Post-fix the no-core fallback emits the synthetic text UNMAPPED.
#[test]
fn synthesized_core_not_found_falls_back_unmapped() {
    use super::emit::emit_synthesized_shorthand_value;
    use verter_span::SourceByteOffset;

    let alloc = Allocator::new();
    // The CodeTransform source is a single original char `x` so the inserted synthetic
    // text is the only thing that could carry a (wrong) mapping.
    let mut ct = CodeTransform::new("x", &alloc);
    let mut out = CodeGenOutput::new(&alloc);

    // `resolved` = `$setup.bar` (a resolver rewrite), `core` = `zzz` is NOT a
    // substring of it → the no-core fallback path. `core_source_start` points at the
    // user token (offset 0). Pre-fix the WHOLE `$setup.bar` mapped to source col 0.
    emit_synthesized_shorthand_value(
        &mut out,
        SourceByteOffset(0),
        "$setup.bar",
        "zzz",
        SourceByteOffset(0),
    );
    out.apply_to(&mut ct);

    let built = ct.build_string();
    // The synthetic text is still emitted verbatim (semantics preserved).
    assert!(
        built.starts_with("$setup.bar"),
        "the synthetic value text must still be emitted: {built:?}"
    );

    let map = ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("t.vue"));
    let tokens: Vec<(u32, u32, u32)> = map
        .get_tokens()
        .filter(|t| t.get_source_id().is_some())
        .map(|t| (t.get_dst_line(), t.get_dst_col(), t.get_src_col()))
        .collect();

    // The inserted synthetic `$setup.bar` starts at generated (0, 0). It must map to
    // None — no token may anchor the synthetic insert to the user source token.
    assert!(
        !has_token_at_gen(&tokens, 0, 0),
        "the no-core synthetic fallback `$setup.bar` (gen 0:0) must map to None, not to the \
         user source token. Tokens: {tokens:?}, built: {built:?}"
    );
    // Discriminating: NO token at all may map back to source col 0 from the synthetic
    // insert (the pre-fix bug mapped the whole run to src col 0).
    assert!(
        !tokens
            .iter()
            .any(|&(dl, dc, sc)| dl == 0 && dc == 0 && sc == 0),
        "the no-core fallback must not map the synthetic string to source col 0. \
         Tokens: {tokens:?}, built: {built:?}"
    );
}

#[test]
fn slot_summary_memoized_warm_requery_builds_zero_extra() {
    // The overlay builds a component's slot summary on the FIRST demand and
    // serves every later demand for the same component warm from its memoized
    // cell. A second query for an already-built component must trigger ZERO
    // additional builds. Bypassing the cell (rebuilding per query) would make the
    // second query bump the build count to 2 and fail here.
    use crate::ast::types::{AstNodeKind, TagType};
    use crate::template::oxc::{reset_slot_summary_counts, slot_summary_build_count};

    let source = r#"<template>
  <Card><Panel /></Card>
</template>"#;
    let alloc = Allocator::new();
    let bytes = source.as_bytes();
    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });
    let template_ast = syntax.take_template_ast().expect("template ast");
    let oxc_ast = crate::template::oxc::parse_template_expressions(
        &template_ast,
        source,
        &alloc,
        oxc_span::SourceType::tsx(),
        true,
    );

    // First slot-checkable component in source order (`Card`).
    let comp_id = template_ast
        .nodes
        .iter()
        .enumerate()
        .find_map(|(idx, node)| match &node.kind {
            AstNodeKind::Element(el) if el.tag_type == TagType::Component => {
                Some(crate::types::NodeId(idx))
            }
            _ => None,
        })
        .expect("a component node");

    reset_slot_summary_counts();

    // Cold demand: builds exactly one summary.
    let first = oxc_ast.slot_summary(comp_id, &template_ast, source);
    assert!(first.is_some(), "Card is a slot-checkable component");
    assert_eq!(
        slot_summary_build_count(),
        1,
        "the first demand for a component must build its summary once"
    );

    // Warm demand: same component, served from the memoized cell — ZERO rebuilds.
    let second = oxc_ast.slot_summary(comp_id, &template_ast, source);
    assert!(second.is_some(), "the warm summary must still resolve");
    assert_eq!(
        slot_summary_build_count(),
        1,
        "a warm re-query of an already-built component must build ZERO additional summaries"
    );
}

// ── Standalone mapped resolver-prefixed expression heads ─────

/// Discriminating — the dynamic `<component :is="expr">` emitter routes its
/// resolved expression through the shared segmented producer. With an INLINE
/// resolver, a setup ref `currentView` resolves to `currentView.value`; the
/// `currentView` identifier maps to its source while the injected `.value` stays
/// UNMAPPED. The old single-chunk fold mapped the whole `currentView.value` run
/// at `iife_prefix.len()`, leaving no unmapped token at the `.value` boundary —
/// the `value_gen` assertion fails on that fold.
#[test]
fn dynamic_component_is_setup_ref_keeps_value_unmapped() {
    let alloc = Allocator::new();
    let source = r#"<template><component :is="currentView" /></template>"#;
    let bytes = source.as_bytes();

    let mut syntax = crate::parser::Syntax::new(false);
    crate::tokenizer::byte::tokenize_sfc(bytes, |e| {
        syntax.handle(
            &e,
            &crate::diagnostics::SyntaxPluginContext {
                input: source,
                bytes,
                options: &crate::diagnostics::SyntaxPluginOptions::default(),
                diagnostics: Vec::new(),
            },
        )
    });
    let template_ast = syntax.take_template_ast().expect("template ast");
    let oxc_ast = crate::template::oxc::parse_template_expressions(
        &template_ast,
        source,
        &alloc,
        oxc_span::SourceType::tsx(),
        true,
    );

    // Locate the dynamic `<component>` element and its OXC data.
    let (el, oxc_el) = template_ast
        .nodes
        .iter()
        .enumerate()
        .find_map(|(i, node)| match &node.kind {
            AstNodeKind::Element(el) if el.tag_type == TagType::Component => {
                let oxc_el = match &oxc_ast.data[i] {
                    OxcNodeData::Element(b) => Some(b.as_ref()),
                    _ => None,
                };
                Some((el.as_ref(), oxc_el))
            }
            _ => None,
        })
        .expect("dynamic <component> element");

    // Inline (non-TSX) resolver so the setup ref takes the `.value` suffix.
    let mut binding_map: FxHashMap<&str, BindingType> = FxHashMap::default();
    binding_map.insert("currentView", BindingType::SetupRef);
    let resolver = BindingResolver::new(binding_map, true);

    let mut out = CodeGenOutput::new(&alloc);
    let rewrite = rewrite_component_is(
        el,
        oxc_el,
        source,
        &mut out,
        &resolver,
        &[],
        EmitContext::JsxChildren,
    );
    let rewrite = rewrite.expect("dynamic :is must be handled");
    assert_eq!(rewrite.tag_name, "___VERTER___component_render");
    assert!(rewrite.needs_iife_close);

    let mut ct = CodeTransform::new(source, &alloc);
    out.apply_to(&mut ct);
    let built = ct.build_string();

    // The resolved expression keeps its `.value` (inline setup ref); bytes unchanged.
    assert!(
        built.contains("___VERTER___extractRenderComponent(currentView.value)"),
        "got: {built}"
    );

    let iife_prefix =
        "{(() => { const ___VERTER___component_render=___VERTER___extractRenderComponent(";
    let cv_gen = el.tag_open.start + iife_prefix.len() as u32;
    let cv_src = source.find("currentView").unwrap() as u32;

    let map = ct.generate_map(crate::code_transform::SourceMapOptions::new().with_source("t.vue"));
    let tokens: Vec<_> = map.get_tokens().collect();
    let dump: Vec<_> = tokens
        .iter()
        .map(|t| {
            (
                t.get_dst_col(),
                t.get_src_col(),
                t.get_source_id().is_some(),
            )
        })
        .collect();

    // `currentView` maps at `iife_prefix.len()` (from the prepend anchor) → its source col.
    let cv = tokens
        .iter()
        .find(|t| t.get_dst_col() == cv_gen && t.get_source_id().is_some());
    assert!(cv.is_some(), "`currentView` must map; tokens: {dump:?}");
    assert_eq!(
        cv.unwrap().get_src_col(),
        cv_src,
        "`currentView` must map to its source col, not the synthetic `.value`"
    );

    // The synthetic `.value` begins its own UNMAPPED segment at iife_prefix.len() + 11.
    let value_gen = cv_gen + "currentView".len() as u32;
    assert!(
        tokens
            .iter()
            .any(|t| t.get_dst_col() == value_gen && t.get_source_id().is_none()),
        "synthetic `.value` must start an unmapped segment at col {value_gen}; tokens: {dump:?}"
    );
    // No source token inside the `.value` region.
    assert!(
        !tokens.iter().any(|t| t.get_dst_col() >= value_gen
            && t.get_dst_col() < value_gen + ".value".len() as u32
            && t.get_source_id().is_some()),
        "`.value` region must carry no source token; tokens: {dump:?}"
    );
}

/// IDE generation always runs in TSX mode, where `resolve_suffix` returns `""`.
/// A setup ref iterable therefore never gains a `.value`: `v-for="todo in todos"`
/// resolves the iterable to bare `(todos)`. End-state invariant guarding the
/// producer routing in production (output bytes are invariant by design).
#[test]
fn ide_v_for_iterable_tsx_setup_ref_never_emits_value_suffix() {
    let source = r#"<template><div v-for="todo in todos">{{ todo }}</div></template>"#;
    let output = gen_tsx_template_with_bindings(source, &[("todos", BindingType::SetupRef)]);
    assert!(
        output.contains("todos).map("),
        "TSX v-for iterable must read `todos).map(`: {output}"
    );
    assert!(
        !output.contains("todos.value"),
        "TSX mode must not emit a `.value` suffix on the iterable: {output}"
    );
}

/// Structural guard — the standalone mapped resolver-prefixed expression heads
/// (IDE `v-for` iterable, dynamic `:is`) route through the shared segmented
/// producer (`build_prefixed_expr_segments` / `resolve_simple_expr_segments` →
/// `prepend_mapped_generated_text`). No standalone emitter may reintroduce the
/// per-identifier fold, keep an independent flat references mapper, or fold a
/// resolved resolver-prefixed expression into one mapped `:is` content chunk.
#[test]
fn standalone_mapped_emitters_carry_no_resolver_prefix_fold() {
    let directives_src = include_str!("directives.rs");
    let mod_src = include_str!("mod.rs");

    // (1) The v-for per-identifier `prefix+gap+bind_prefix+name+bind_suffix` fold
    //     must be gone — the iterable resolves through the segmented producer.
    assert!(
        !directives_src.contains(concat!(
            "format!(\"{}{}{}{}{}\", ",
            "prefix, gap, bind_prefix, name, bind_suffix)"
        )),
        "IDE v-for iterable must not fold prefix+gap+bind_prefix+name+bind_suffix into one mapped \
         chunk; route through build_prefixed_expr_segments / resolve_simple_expr_segments instead"
    );

    // (2) The independent flat references-walking resolver-prefix/suffix mapper
    //     must be deleted, not kept alongside the producer.
    assert!(
        !directives_src.contains("fn resolve_v_for_iterable"),
        "resolve_v_for_iterable (the independent resolver-prefix/suffix mapper) must be deleted; \
         the v-for iterable resolves through the shared segmented producer"
    );

    // (3) The dynamic `:is` resolved expression must not fold into one mapped
    //     content chunk; it routes through the producer + wrapped().
    assert!(
        !mod_src.contains(concat!("iife_prefix, resolved_expr, ", "ts_comment_text")),
        "dynamic :is must not fold iife_prefix + resolved_expr into one mapped chunk; route the \
         resolved expression through the shared segmented producer"
    );

    // Positive: both emitters lower through the single segmented carrier.
    assert!(
        directives_src.contains("prepend_mapped_generated_text"),
        "IDE v-for iterable must lower via prepend_mapped_generated_text"
    );
    assert!(
        mod_src.contains("prepend_mapped_generated_text"),
        "dynamic :is must lower via prepend_mapped_generated_text"
    );
}

// ── Spread-path event-typing closed matrix ────────────────────────────────
//
// The full spread-event typing matrix: {native, local component, global
// component} × {$event inline, arrow/function param} × {duplicate event key, hyphenated
// event key}. Native hyphenated rows are covered above by `kebab_event_*`. Every spread
// surface types its handler from the typed-IR — native via the ambient DOM payload,
// components via the shared `InstanceType<typeof Binding>["$props"]` inventory (local
// binding OR GlobalComponents fallback const) — never `$event: any`, never the retired
// `eventCallbacks` helper, never `import('vue').GlobalComponents[...]`.
mod spread_event_typing_matrix {
    use super::*;

    /// The component event-handler payload tuple for `Binding` and JSX event prop `onX`.
    fn component_params_tuple(binding: &str, on_event: &str) -> String {
        format!(
            r#"Parameters<NonNullable<Required<InstanceType<typeof {binding}>["$props"]>["{on_event}"]>>"#
        )
    }

    fn native_payload(event: &str) -> String {
        format!(
            r#"(GlobalEventHandlersEventMap & {{ [___VERTER___EventKey: string]: Event }})["{event}"]"#
        )
    }

    fn assert_no_untyped_leaks(result: &str) {
        assert!(
            !result.contains("$event: any"),
            "spread $event must be precisely typed, never `$event: any`: {result}"
        );
        assert!(
            !result.contains("___VERTER___eventCallbacks"),
            "retired generic eventCallbacks helper must be absent: {result}"
        );
    }

    // ── Native element ────────────────────────────────────────────────────

    #[test]
    fn duplicate_native_dollar_event_ambient_payload() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div @click="handle($event.clientY)" @click="handle($event.screenX)" /></template>"#,
            &[("handle", BindingType::SetupConst)],
        );
        // The SECOND @click routes through the spread path (duplicate key).
        assert!(
            result.contains(&format!(
                r#""onClick": ($event: {}) =>"#,
                native_payload("click")
            )),
            "duplicate native $event must be typed via the ambient DOM payload: {result}"
        );
        assert!(
            !result.contains("IntrinsicElementAttributes"),
            "native spread must not use the import('vue') formula: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    #[test]
    fn duplicate_native_arrow_satisfies_ambient_payload() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><div @click="(e) => handle(e)" @click="(e) => other(e)" /></template>"#,
            &[
                ("handle", BindingType::SetupConst),
                ("other", BindingType::SetupConst),
            ],
        );
        assert!(
            result.contains(&format!(
                r#""onClick": ((e) => other(e)) satisfies (...___VERTER___eventArgs: [{}]) => unknown"#,
                native_payload("click")
            )),
            "duplicate native arrow must be satisfies-wrapped against the ambient payload tuple: {result}"
        );
        assert!(
            !result.contains("IntrinsicElementAttributes"),
            "native spread arrow must not use the import('vue') formula: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    // ── Local (script-bound) component ────────────────────────────────────

    #[test]
    fn hyphenated_local_component_dollar_event_instance_type() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><LocalComp @my-event="handle($event)" /></template>"#,
            &[
                ("LocalComp", BindingType::SetupConst),
                ("handle", BindingType::SetupConst),
            ],
        );
        assert!(
            result.contains(&format!(
                r#""onMy-event": ($event: {}[0]) =>"#,
                component_params_tuple("LocalComp", "onMy-event")
            )),
            "hyphenated local-component $event must be typed via InstanceType<typeof LocalComp>: {result}"
        );
        assert!(
            !result.contains("GlobalComponents"),
            "local component must not use the GlobalComponents indexed type: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    #[test]
    fn duplicate_local_component_dollar_event_instance_type() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><LocalComp @click="handle($event)" @click="other($event)" /></template>"#,
            &[
                ("LocalComp", BindingType::SetupConst),
                ("handle", BindingType::SetupConst),
                ("other", BindingType::SetupConst),
            ],
        );
        assert!(
            result.contains(&format!(
                r#""onClick": ($event: {}[0]) =>"#,
                component_params_tuple("LocalComp", "onClick")
            )),
            "duplicate local-component $event must be typed via InstanceType<typeof LocalComp>: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    #[test]
    fn hyphenated_local_component_arrow_satisfies_instance_type() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><LocalComp @my-event="(e) => handle(e)" /></template>"#,
            &[
                ("LocalComp", BindingType::SetupConst),
                ("handle", BindingType::SetupConst),
            ],
        );
        assert!(
            result.contains(&format!(
                r#""onMy-event": ((e) => handle(e)) satisfies (...___VERTER___eventArgs: {}) => unknown"#,
                component_params_tuple("LocalComp", "onMy-event")
            )),
            "hyphenated local-component arrow must satisfies-wrap the InstanceType<typeof LocalComp> tuple: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    #[test]
    fn duplicate_local_component_arrow_satisfies_instance_type() {
        let result = gen_tsx_template_with_bindings(
            r#"<template><LocalComp @click="(e) => handle(e)" @click="(e) => other(e)" /></template>"#,
            &[
                ("LocalComp", BindingType::SetupConst),
                ("handle", BindingType::SetupConst),
                ("other", BindingType::SetupConst),
            ],
        );
        assert!(
            result.contains(&format!(
                r#""onClick": ((e) => other(e)) satisfies (...___VERTER___eventArgs: {}) => unknown"#,
                component_params_tuple("LocalComp", "onClick")
            )),
            "duplicate local-component arrow must satisfies-wrap the InstanceType<typeof LocalComp> tuple: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    // ── Global (GlobalComponents fallback) component ──────────────────────

    #[test]
    fn hyphenated_global_component_dollar_event_fallback_const() {
        let result = gen_tsx_template_with_components(
            r#"<template><GlobalComp @my-event="handle($event)" /></template>"#,
            &[("handle", BindingType::SetupConst)],
            &["GlobalComp"],
        );
        assert!(
            result.contains(&format!(
                r#""onMy-event": ($event: {}[0]) =>"#,
                component_params_tuple("GlobalComp", "onMy-event")
            )),
            "hyphenated global-component $event must resolve via the fallback const InstanceType<typeof GlobalComp>: {result}"
        );
        // Never the direct GlobalComponents indexed type (tsgo cannot resolve it).
        assert!(
            !result.contains("GlobalComponents"),
            "global component $event must NOT use import('vue').GlobalComponents[...]: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    #[test]
    fn duplicate_global_component_dollar_event_fallback_const() {
        let result = gen_tsx_template_with_components(
            r#"<template><GlobalComp @click="handle($event)" @click="other($event)" /></template>"#,
            &[
                ("handle", BindingType::SetupConst),
                ("other", BindingType::SetupConst),
            ],
            &["GlobalComp"],
        );
        assert!(
            result.contains(&format!(
                r#""onClick": ($event: {}[0]) =>"#,
                component_params_tuple("GlobalComp", "onClick")
            )),
            "duplicate global-component $event must resolve via the fallback const InstanceType<typeof GlobalComp>: {result}"
        );
        assert!(
            !result.contains("GlobalComponents"),
            "global component $event must NOT use import('vue').GlobalComponents[...]: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    #[test]
    fn hyphenated_global_component_arrow_satisfies_fallback_const() {
        let result = gen_tsx_template_with_components(
            r#"<template><GlobalComp @my-event="(e) => handle(e)" /></template>"#,
            &[("handle", BindingType::SetupConst)],
            &["GlobalComp"],
        );
        assert!(
            result.contains(&format!(
                r#""onMy-event": ((e) => handle(e)) satisfies (...___VERTER___eventArgs: {}) => unknown"#,
                component_params_tuple("GlobalComp", "onMy-event")
            )),
            "hyphenated global-component arrow must satisfies-wrap the fallback-const InstanceType<typeof GlobalComp> tuple: {result}"
        );
        assert!(
            !result.contains("GlobalComponents"),
            "global component arrow must NOT use import('vue').GlobalComponents[...]: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    #[test]
    fn duplicate_global_component_arrow_satisfies_fallback_const() {
        let result = gen_tsx_template_with_components(
            r#"<template><GlobalComp @click="(e) => handle(e)" @click="(e) => other(e)" /></template>"#,
            &[
                ("handle", BindingType::SetupConst),
                ("other", BindingType::SetupConst),
            ],
            &["GlobalComp"],
        );
        assert!(
            result.contains(&format!(
                r#""onClick": ((e) => other(e)) satisfies (...___VERTER___eventArgs: {}) => unknown"#,
                component_params_tuple("GlobalComp", "onClick")
            )),
            "duplicate global-component arrow must satisfies-wrap the fallback-const InstanceType<typeof GlobalComp> tuple: {result}"
        );
        assert!(
            !result.contains("GlobalComponents"),
            "global component arrow must NOT use import('vue').GlobalComponents[...]: {result}"
        );
        assert_no_untyped_leaks(&result);
    }

    // ── Unresolved component: explicit `any`, never implicit ──

    #[test]
    fn unresolved_component_dollar_event_explicit_any_not_implicit() {
        // A component with no local binding and no fallback const (not in the inventory).
        let result = gen_tsx_template_with_bindings(
            r#"<template><UnknownComp @click="handle($event)" @click="other($event)" /></template>"#,
            &[
                ("handle", BindingType::SetupConst),
                ("other", BindingType::SetupConst),
            ],
        );
        // Explicit `$event: any` (never a bare implicit-any parameter).
        assert!(
            result.contains(r#""onClick": ($event: any) =>"#),
            "unresolved component $event must be EXPLICIT any, never implicit: {result}"
        );
    }
}
