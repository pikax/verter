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
        custom_elements: None,
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
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, code, oxc_span::SourceType::tsx()).parse();
    assert!(
        !parsed.fatal_error && parsed.diagnostics.is_empty(),
        "generated template must be valid TSX: {:?}\n{code}",
        parsed.diagnostics
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
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, code, oxc_span::SourceType::tsx()).parse();
    assert!(
        !parsed.fatal_error && parsed.diagnostics.is_empty(),
        "generated template must be valid TSX: {:?}\n{code}",
        parsed.diagnostics
    );
    let mut scanner = Scanner {
        wanted_element,
        attributes: Vec::new(),
    };
    scanner.visit_program(&parsed.program);
    scanner.attributes
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
        custom_elements: None,
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

// ── Part H: JSX syntax validation for directive combinations ─────

/// Validate that the generated TSX template is parseable JSX/TSX.
/// Wraps the template output in a JSX fragment so IIFE expressions parse correctly.
fn assert_valid_jsx(source: &str, label: &str) {
    let result = gen_tsx_template(source);
    let wrapper = format!("const x = <>{}</>", result);
    let val_alloc = oxc_allocator::Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&val_alloc, &wrapper, oxc_span::SourceType::tsx())
            .parse();
    assert!(
        parsed.diagnostics.is_empty(),
        "[{}] TSX syntax errors: {:?}\n--- source ---\n{}\n--- output ---\n{}",
        label,
        parsed
            .diagnostics
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>(),
        source,
        result
    );
}

// ── v-for source mapping (#19) ──────────────────────────────────

/// Helper: generate TSX template with bindings AND return source map tokens.
/// Returns (output_string, Vec<(dst_line, dst_col, src_col)>).
///
/// Single-authored-line fixtures only. A fixture whose authored expression spans
/// several lines must use [`gen_tsx_template_with_line_map`], which also reports the
/// token's source LINE.
fn gen_tsx_template_with_map(
    source: &str,
    bindings: &[(&str, BindingType)],
) -> (String, Vec<(u32, u32, u32)>) {
    let (output, tokens) = gen_tsx_template_with_line_map(source, bindings);
    (
        output,
        tokens
            .into_iter()
            .map(|(dst_line, dst_col, _src_line, src_col)| (dst_line, dst_col, src_col))
            .collect(),
    )
}

/// Helper: generate TSX template with bindings AND return FULL source map tokens.
/// Returns (output_string, Vec<(dst_line, dst_col, src_line, src_col)>).
///
/// MAPPED tokens only. A run's EXTENT also depends on the UNMAPPED tokens that bound
/// it, so an extent assertion must go through
/// [`gen_tsx_template_with_raw_tokens`] instead.
fn gen_tsx_template_with_line_map(
    source: &str,
    bindings: &[(&str, BindingType)],
) -> (String, Vec<(u32, u32, u32, u32)>) {
    let (output, tokens) = gen_tsx_template_with_raw_tokens(source, bindings);
    (
        output,
        tokens
            .into_iter()
            .filter_map(|t| {
                t.src
                    .map(|(src_line, src_col)| (t.dst_line, t.dst_col, src_line, src_col))
            })
            .collect(),
    )
}

/// One emitted source-map token, mapped or not.
///
/// `src` is `None` for a token an UNMAPPED chunk emitted. Those tokens carry no
/// mapping of their own but they BOUND the preceding mapped run's extent, so they
/// are load-bearing for any extent assertion (see [`RunMap`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RawToken {
    dst_line: u32,
    dst_col: u32,
    src: Option<(u32, u32)>,
}

/// Helper: generate TSX template with bindings AND return EVERY emitted source-map
/// token, mapped or unmapped.
fn gen_tsx_template_with_raw_tokens(
    source: &str,
    bindings: &[(&str, BindingType)],
) -> (String, Vec<RawToken>) {
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
        custom_elements: None,
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
    let tokens: Vec<RawToken> = map
        .get_tokens()
        .map(|t| RawToken {
            dst_line: t.get_dst_line(),
            dst_col: t.get_dst_col(),
            src: t
                .get_source_id()
                .map(|_| (t.get_src_line(), t.get_src_col())),
        })
        .collect();

    (full, tokens)
}

/// One mapped run reconstructed from the emitted tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OracleRun {
    dst_line: u32,
    dst_col: u32,
    /// Exclusive generated-column end.
    dst_end: u32,
    src_line: u32,
    src_col: u32,
    /// Exclusive source-column end. `src_end - src_col == dst_end - dst_col` ALWAYS.
    src_end: u32,
    /// Compatibility-component label: only runs sharing one may compose a range.
    component: u32,
}

/// The mapped runs a conformant consumer builds from the emitted source-map tokens —
/// the test-side mirror of `verter_lsp::documents::position_map::PositionMapper`,
/// whose contract is documented on `MappedRun` there.
///
/// Three properties of that contract are what these tests exercise:
///
///  1. **A run maps 1:1 within itself.** A source-map token is a POINT mapping, so a
///     run's generated extent and its source extent are the SAME length
///     (`dst_end - dst_col == src_end - src_col`) and a position resolves by adding
///     the in-run offset to the other side. This is not a policy choice — it is the
///     limit of what one token can express, and it is why a LENGTH-CHANGING rewrite
///     cannot ride a single run.
///  2. **The extent is bounded, not open-ended.** It is
///     `min(next-token-column-on-this-generated-line - dst_col,
///     source-line-length - src_col)`. The first bound counts the next token of ANY
///     kind, so an UNMAPPED token right after a mapped run caps it — which is why
///     [`gen_tsx_template_with_raw_tokens`] must not filter unmapped tokens away.
///  3. **Lookups are strictly in-run and ranges must compose.** A query in a gap maps
///     to NOTHING (no extrapolation, no snap-to-nearest), and a RANGE resolves only
///     when both endpoints land in runs linked by an unbroken chain contiguous in
///     BOTH spaces (`prev.dst_end == cur.dst_col && prev.src_end == cur.src_col`).
///
/// Columns are UTF-16 code units on both sides, matching the emitter. Two deliberate
/// simplifications, both of which make this oracle STRICTER than the consumer (never
/// more permissive, so it cannot manufacture a false green):
///  - the multiline line-wrap contiguity arm is not modelled, so a run never joins
///    across a generated newline here;
///  - the content-less extent arms are not modelled, because the emitted map always
///    embeds the authored source.
struct RunMap {
    runs: Vec<OracleRun>,
}

impl RunMap {
    fn build(source: &str, tokens: &[RawToken]) -> Self {
        let src_line_lens: Vec<u32> = source
            .split('\n')
            .map(|line| line.chars().map(|c| c.len_utf16() as u32).sum())
            .collect();

        let mut runs: Vec<OracleRun> = Vec::new();
        let mut next_component = 0u32;
        for (i, tok) in tokens.iter().enumerate() {
            let Some((src_line, src_col)) = tok.src else {
                continue; // unmapped: no run of its own, it only BOUNDS the previous one
            };
            let next_dst_bound = tokens[i + 1..]
                .iter()
                .take_while(|t| t.dst_line == tok.dst_line)
                .find(|t| t.dst_col > tok.dst_col)
                .map(|t| t.dst_col - tok.dst_col);
            let src_remaining = src_line_lens
                .get(src_line as usize)
                .copied()
                .unwrap_or(0)
                .saturating_sub(src_col);
            let run_len = match next_dst_bound {
                Some(bound) => bound.min(src_remaining),
                None => src_remaining,
            };
            if run_len == 0 {
                continue;
            }
            let run = OracleRun {
                dst_line: tok.dst_line,
                dst_col: tok.dst_col,
                dst_end: tok.dst_col + run_len,
                src_line,
                src_col,
                src_end: src_col + run_len,
                component: 0,
            };
            let component = match runs.last() {
                Some(prev)
                    if prev.dst_line == run.dst_line
                        && prev.dst_end == run.dst_col
                        && prev.src_line == run.src_line
                        && prev.src_end == run.src_col =>
                {
                    prev.component
                }
                _ => {
                    let fresh = next_component;
                    next_component += 1;
                    fresh
                }
            };
            runs.push(OracleRun { component, ..run });
        }
        Self { runs }
    }

    fn run_at_dst(&self, line: u32, col: u32) -> Option<&OracleRun> {
        self.runs
            .iter()
            .find(|r| r.dst_line == line && col >= r.dst_col && col < r.dst_end)
    }

    fn run_at_src(&self, line: u32, col: u32) -> Option<&OracleRun> {
        self.runs
            .iter()
            .find(|r| r.src_line == line && col >= r.src_col && col < r.src_end)
    }

    /// generated (line, col) → authored (line, col). Strictly in-run.
    fn to_source(&self, line: u32, col: u32) -> Option<(u32, u32)> {
        let run = self.run_at_dst(line, col)?;
        Some((run.src_line, run.src_col + (col - run.dst_col)))
    }

    /// authored (line, col) → generated (line, col). Strictly in-run.
    fn to_generated(&self, line: u32, col: u32) -> Option<(u32, u32)> {
        let run = self.run_at_src(line, col)?;
        Some((run.dst_line, run.dst_col + (col - run.src_col)))
    }

    /// A half-open generated range `[start, end)` on one generated line → the authored
    /// range a provider edit derived from it would splice, or `None` when the range
    /// does not compose (the fail-closed answer — no edit is produced).
    fn range_to_source(&self, line: u32, start: u32, end: u32) -> Option<((u32, u32), (u32, u32))> {
        let start_run = self.run_at_dst(line, start)?;
        let start_src = (
            start_run.src_line,
            start_run.src_col + (start - start_run.dst_col),
        );
        // Half-open end exactly at a run's exclusive generated end: the last INCLUDED
        // column is `end - 1`, and the composed authored end is that run's mapped
        // exclusive end.
        if end > 0 {
            if let Some(end_run) = self.run_at_dst(line, end - 1) {
                if end_run.dst_end == end && end_run.component == start_run.component {
                    return Some((start_src, (end_run.src_line, end_run.src_end)));
                }
            }
        }
        let end_run = self.run_at_dst(line, end)?;
        if end_run.component != start_run.component {
            return None;
        }
        Some((
            start_src,
            (end_run.src_line, end_run.src_col + (end - end_run.dst_col)),
        ))
    }
}

/// The BYTE offset in `text` of 0-based `(line, col)` — the inverse of
/// [`gen_line_col`]. ASCII only (asserted), so a column is a byte within its line.
fn byte_of_line_col(text: &str, line: u32, col: u32) -> usize {
    assert!(
        text.is_ascii(),
        "byte_of_line_col assumes an ASCII fixture so a UTF-16 column is a byte offset"
    );
    let line_start = text
        .split_inclusive('\n')
        .take(line as usize)
        .map(str::len)
        .sum::<usize>();
    line_start + col as usize
}

/// The generated BYTE offset the authored byte `src_off` maps to, or `None` when that
/// authored byte has no generated correlate at all (the fail-closed answer).
///
/// ASCII single-authored-line fixtures only: the authored byte offset is then the
/// column on authored line 0, and a generated column is a byte within its line.
fn authored_byte_to_generated_byte(
    source: &str,
    output: &str,
    map: &RunMap,
    src_off: usize,
) -> Option<usize> {
    assert!(
        source.is_ascii() && !source.contains('\n'),
        "this helper takes the authored byte offset as a column on authored line 0"
    );
    let (line, col) = map.to_generated(0, src_off as u32)?;
    Some(byte_of_line_col(output, line, col))
}

/// The authored BYTE offset the generated byte `gen_off` maps back to, or `None`.
fn generated_byte_to_authored_byte(
    source: &str,
    output: &str,
    map: &RunMap,
    gen_off: usize,
) -> Option<usize> {
    assert!(
        source.is_ascii() && !source.contains('\n'),
        "this helper reports the authored byte offset as a column on authored line 0"
    );
    let (line, col) = gen_line_col(output, gen_off);
    let (src_line, src_col) = map.to_source(line, col)?;
    assert_eq!(src_line, 0, "single-authored-line fixture");
    Some(src_col as usize)
}

// ── Object literal in binding: prop name as key must not be rewritten ────

fn compile_full_sfc_tsx(source: &str, filename: &str) -> String {
    let alloc = Allocator::new();
    let options = crate::compile::legacy_test_support::CodegenOptions {
        filename: Some(filename.to_string()),
        target: crate::compile::CompileTarget::TSX,
        embed_ambient_types: false,
        ..Default::default()
    };
    let verter_opts = crate::compile::legacy_test_support::VerterCompileOptions::default();
    let result = crate::compile::legacy_test_support::compile(
        source,
        &options,
        &verter_opts,
        &crate::compile::VueMacroSemanticInput::Unavailable,
        &alloc,
    );
    let tsx = result.tsx.as_ref().expect("TSX should be generated");
    tsx.code.clone()
}

fn assert_valid_tsx(code: &str, label: &str) {
    let alloc = Allocator::new();
    let parsed =
        verter_parser::oxc_parse::Parser::new(&alloc, code, oxc_span::SourceType::tsx()).parse();
    for err in &parsed.diagnostics {
        eprintln!("[{label}] OXC ERROR: {err}");
    }
    assert!(
        parsed.diagnostics.is_empty(),
        "[{label}] TSX should have no parse errors. Got {} errors. Output:\n{code}",
        parsed.diagnostics.len()
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
        custom_elements: None,
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

// ── Custom-directive carrier mapping ───────────────────────
//
// A custom directive relocates its whole payload into a synthetic
// `___VERTER___runCustomDirective(...)` call. Every AUTHORED token inside that
// payload — the directive name, the value expression, the argument, and each
// modifier — must carry its own mapped run back to the authored span, or the
// resulting diagnostics/hover/definition land nowhere (an unmapped generated
// position has no original position and a conformant consumer fails closed).

/// Convert a byte offset in `s` into the 0-based `(line, col)` the source map uses.
fn gen_line_col(s: &str, byte_off: usize) -> (u32, u32) {
    let mut line = 0u32;
    let mut line_start = 0usize;
    for (i, b) in s.as_bytes().iter().enumerate().take(byte_off) {
        if *b == b'\n' {
            line += 1;
            line_start = i + 1;
        }
    }
    (line, (byte_off - line_start) as u32)
}

/// Assert `gen_token` occurs EXACTLY once in `output` and that a source-map token
/// is anchored at its first byte pointing back to `src_offset`.
///
/// Asserting the PAIR (generated position ↔ source offset) is what discriminates:
/// a `contains()` check stays green when the mapping drifts, and a bare
/// "some token has this src_col" check stays green when the run is anchored on the
/// wrong generated token.
fn assert_mapped_run(
    output: &str,
    tokens: &[(u32, u32, u32)],
    gen_token: &str,
    src_offset: usize,
    what: &str,
) {
    let occurrences = output.matches(gen_token).count();
    assert_eq!(
        occurrences, 1,
        "{what}: expected exactly one occurrence of {gen_token:?} in the generated \
         output so the mapping assertion is unambiguous, found {occurrences}: {output}"
    );
    let gen_off = output.find(gen_token).unwrap();
    let (line, col) = gen_line_col(output, gen_off);
    let found = tokens
        .iter()
        .any(|&(l, c, src)| l == line && c == col && src == src_offset as u32);
    assert!(
        found,
        "{what}: generated {gen_token:?} at {line}:{col} must carry a mapped run back \
         to authored offset {src_offset}. Tokens on that generated line: {:?}\n{output}",
        tokens
            .iter()
            .filter(|&&(l, _, _)| l == line)
            .collect::<Vec<_>>()
    );
}

/// Assert a source-map token is anchored exactly at generated byte `gen_off` and
/// points back to authored offset `src_offset`.
///
/// The PAIR is what discriminates: a token merely existing somewhere for that
/// source offset stays green when the run is anchored on the wrong generated byte,
/// and a token merely existing at that generated byte stays green when it points
/// at the wrong authored offset.
fn assert_token_at(
    output: &str,
    tokens: &[(u32, u32, u32)],
    gen_off: usize,
    src_offset: usize,
    what: &str,
) {
    let (line, col) = gen_line_col(output, gen_off);
    assert!(
        tokens
            .iter()
            .any(|&(l, c, src)| l == line && c == col && src == src_offset as u32),
        "{what}: generated {line}:{col} must carry a mapped run anchored at authored \
         offset {src_offset}. Tokens on that generated line: {:?}\n{output}",
        tokens
            .iter()
            .filter(|&&(l, _, _)| l == line)
            .collect::<Vec<_>>()
    );
}

/// Assert a source-map token is anchored exactly at generated byte `gen_off` and
/// points back to the authored `(line, col)` of `src_off` — the LINE-aware form.
///
/// [`assert_token_at`] compares the source COLUMN only, which cannot see a token
/// that lands on the right column of the WRONG authored line. A multiline
/// expression needs the full pair.
fn assert_token_at_lc(
    output: &str,
    source: &str,
    tokens: &[(u32, u32, u32, u32)],
    gen_off: usize,
    src_off: usize,
    what: &str,
) {
    let (dst_line, dst_col) = gen_line_col(output, gen_off);
    let (src_line, src_col) = gen_line_col(source, src_off);
    assert!(
        tokens.iter().any(|&(dl, dc, sl, sc)| dl == dst_line
            && dc == dst_col
            && sl == src_line
            && sc == src_col),
        "{what}: generated {dst_line}:{dst_col} must carry a mapped run anchored at \
         authored {src_line}:{src_col}. Tokens on that generated line: {:?}\n{output}",
        tokens
            .iter()
            .filter(|&&(dl, _, _, _)| dl == dst_line)
            .collect::<Vec<_>>()
    );
}

/// Assert the WHOLE kebab→camel projection of a directive name, column by column,
/// in BOTH directions.
///
/// `authored` is the authored `v-…` name and `generated` the camel identifier it
/// projects to. The correspondence is derived here from the two spellings rather
/// than restated per fixture: dropping each `-` and pairing what remains gives the
/// authored column each generated column stands for. Every non-hyphen authored
/// column must reach the generated character it actually became, and every hyphen
/// — which the transform DELETES, so it has no generated correlate — must map to
/// nothing.
///
/// This asserts the EXTENT, not an anchor. A test that checks only the run's first
/// column stays green while the run is six columns long over a seven-column authored
/// token; that is exactly how the truncated extent shipped.
fn assert_directive_name_projection(
    source: &str,
    output: &str,
    map: &RunMap,
    authored: &str,
    generated: &str,
    what: &str,
) {
    // The correspondence below pairs authored characters with generated characters
    // one for one, which holds only while every case change is length-preserving —
    // true for ASCII. A non-ASCII name whose initial expands (`ß` → `SS`) must be
    // asserted through the unit-level projection tests instead, so refuse it here
    // rather than derive a wrong expectation from it.
    assert!(
        authored.is_ascii() && generated.is_ascii(),
        "{what}: this helper derives the column correspondence from the two ASCII spellings"
    );
    let name_src = source
        .find(authored)
        .unwrap_or_else(|| panic!("{what}: fixture must contain the authored name {authored:?}"));
    assert_eq!(
        source.matches(authored).count(),
        1,
        "{what}: the authored name {authored:?} must occur once so the offsets are unambiguous"
    );
    assert_eq!(
        output.matches(generated).count(),
        1,
        "{what}: the generated identifier {generated:?} must occur once in the output so the \
         mapping assertions are unambiguous:\n{output}"
    );
    let name_gen = output.find(generated).expect("checked just above");

    // Pair each authored column with the generated column it became, skipping the
    // hyphens the transform deletes.
    let mut expected: Vec<(usize, Option<usize>)> = Vec::new();
    let mut gen_idx = 0usize;
    for (i, ch) in authored.char_indices() {
        if ch == '-' {
            expected.push((i, None));
        } else {
            expected.push((i, Some(gen_idx)));
            gen_idx += 1;
        }
    }
    assert_eq!(
        gen_idx,
        generated.len(),
        "{what}: {authored:?} minus its hyphens must have as many characters as {generated:?}"
    );

    for (src_rel, gen_rel) in expected {
        let src_off = name_src + src_rel;
        let want = gen_rel.map(|g| name_gen + g);
        let got = authored_byte_to_generated_byte(source, output, map, src_off);
        assert_eq!(
            got,
            want,
            "{what}: authored byte {src_off} ({:?}) must map to {}, got {got:?}. \
             The kebab→camel hop is LENGTH-CHANGING, so one linear run cannot carry it: \
             it covers only its own generated length of authored columns and shifts every \
             column past the deleted hyphen. Runs: {:?}\n{output}",
            &authored[src_rel..src_rel + 1],
            match want {
                Some(g) => format!("generated byte {g} ({:?})", &output[g..g + 1]),
                None => "NOTHING (the hyphen has no generated correlate)".to_string(),
            },
            map.runs,
        );

        // …and the reverse direction, which is what a provider-reported position and
        // the endpoints of a provider-reported RANGE resolve through.
        if let Some(gen_off) = want {
            let back = generated_byte_to_authored_byte(source, output, map, gen_off);
            assert_eq!(
                back,
                Some(src_off),
                "{what}: generated byte {gen_off} ({:?}) must map BACK to authored byte \
                 {src_off} ({:?}), got {back:?}. Runs: {:?}\n{output}",
                &output[gen_off..gen_off + 1],
                &authored[src_rel..src_rel + 1],
                map.runs,
            );
        }
    }

    // A provider edit derived from the generated identifier's full range must never
    // splice a STRICT PREFIX of the authored token: that is the corrupting partial
    // edit (rename `vColor` → `vHighlight` replacing `v-colo` and leaving a dangling
    // `r`). The kebab→camel hop deletes bytes, and a run chain contiguous in both
    // spaces preserves total length, so no token layout can compose the COMPLETE
    // authored token from the shorter generated one — the range must fail CLOSED.
    let (gen_line, gen_col) = gen_line_col(output, name_gen);
    let composed = map.range_to_source(gen_line, gen_col, gen_col + generated.len() as u32);
    assert_eq!(
        composed,
        None,
        "{what}: the generated identifier's range [{gen_col}, {}) must NOT compose an \
         authored range — it is shorter than the authored token, so anything it composes is a \
         PARTIAL token and an edit derived from it corrupts the source. It composed \
         {composed:?} (authored token is [{name_src}, {})). Runs: {:?}\n{output}",
        gen_col + generated.len() as u32,
        name_src + authored.len(),
        map.runs,
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
        custom_elements: None,
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
        custom_elements: None,
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

mod components;
mod directives;
mod elements;
mod events;
mod general;
mod slots;
mod sourcemaps;
mod typing;
#[cfg(test)]
mod scratch_print {
    use super::*;
    #[test]
    fn scratch_print_outputs() {
        for (src, b) in [
            (
                r#"<template><div v-if="show" @click="handler($event)">click</div></template>"#,
                vec![],
            ),
            (
                r#"<template><div v-if="a">A</div><div v-else-if="b" @click="handler($event)">B</div></template>"#,
                vec![],
            ),
            (
                r#"<template><div v-if="typeof msg === 'string'" :handler="function() { return msg.trim() }">hi</div></template>"#,
                vec![],
            ),
            (
                r#"<template><div v-if="ok" :onX="() => handle()"/></template>"#,
                vec![
                    ("ok", BindingType::SetupConst),
                    ("handle", BindingType::Props),
                ],
            ),
            (
                r#"<template><div v-if="ok" :onX="() => { handle() }"/></template>"#,
                vec![
                    ("ok", BindingType::SetupConst),
                    ("handle", BindingType::SetupConst),
                ],
            ),
            (
                r#"<template><div v-if="ok" @click="count++"/></template>"#,
                vec![
                    ("ok", BindingType::SetupConst),
                    ("count", BindingType::SetupRef),
                ],
            ),
            (
                r#"<template><button @click="count++" v-if="ready">x</button></template>"#,
                vec![
                    ("ready", BindingType::SetupConst),
                    ("count", BindingType::SetupConst),
                ],
            ),
        ] {
            println!("=====\n{}\n=====", gen_tsx_template_with_bindings(src, &b));
        }
    }
}
