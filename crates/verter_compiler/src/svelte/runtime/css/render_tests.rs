//! Unit tests for the scoped-CSS renderer — each construct pinned against the
//! official `svelte@5.56.3` `phases/3-transform/css/index.js` behavior:
//! scope-class application (with the `:where(...)` specificity bump and the
//! per-rule reset), `:global(...)` unwrapping, `:global { … }` block
//! comment-wrapping, unused/empty rule pruning (whole-rule and per-selector),
//! comment-close escaping, and the `@keyframes` rename plus
//! `animation`/`animation-name` token rewrite. The render-parity anchor
//! byte-compares the committed `scoped_styles` fixture against its committed
//! golden `css.code` (hash-masked, line-ending-normalized).

use super::build_style_scope_plan;
use super::types::{CssMode, ProvenStyleScopePlan};
use crate::svelte::parser::parse_svelte;
use crate::svelte::runtime::{lower_parsed_svelte_to_ir, SvelteRuntimeOptions};
use oxc_allocator::Allocator;
use verter_span::Span;

/// Body span of the lone `<style>` block in `source`.
fn body_span(source: &str) -> Span {
    let start = source.find("<style>").expect("open tag") + "<style>".len();
    let end = source.rfind("</style>").expect("close tag");
    Span::new(start as u32, end as u32)
}

/// Lower the component and build its scope plan (the production wiring:
/// parse → analyze → match → render). An `Ok` plan is PROVEN by construction.
fn plan_for(source: &str, filename: Option<&str>) -> ProvenStyleScopePlan {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: filename.map(str::to_string),
        ..Default::default()
    };
    let ir = lower_parsed_svelte_to_ir(source, &parsed, &opts, &alloc).expect("lowering succeeds");
    build_style_scope_plan(
        source,
        body_span(source),
        filename,
        CssMode::External,
        &ir,
        false,
    )
    .expect("a clean body plans")
}

/// Render the css body of `<div class="card">…</div>` + `css` and return
/// `(rendered css.code, scope hash)`.
fn rendered(css: &str) -> (String, String) {
    let source =
        format!("<div class=\"card\"><p class=\"title\">x</p></div>\n<style>{css}</style>");
    let plan = plan_for(&source, None);
    (plan.css_code, plan.hash)
}

#[test]
fn scope_class_appends_to_matched_static_element_selector() {
    let (code, hash) = rendered(".card { color: red; }");
    assert_eq!(code, format!(".card.{hash} {{ color: red; }}"));
    // Should-NOT: a lone scoped compound takes the plain class selector —
    // never the `:where` bump — and a used rule is never comment-pruned.
    assert!(!code.contains(":where"));
    assert!(!code.contains("(unused)"));
    assert!(!code.contains("(empty)"));
}

#[test]
fn later_scoped_compound_in_same_complex_selector_gets_where_specificity_bump() {
    let (code, hash) = rendered(".card .title { color: red; }");
    // First scoped compound takes `.hash` (+0-1-0 bump), every later one in
    // the SAME complex selector takes the specificity-neutral `:where(.hash)`.
    assert_eq!(
        code,
        format!(".card.{hash} .title:where(.{hash}) {{ color: red; }}")
    );
    // Should-NOT: the later compound never takes the bare class form.
    assert!(!code.contains(&format!(".title.{hash}")));
}

#[test]
fn sibling_rules_reset_specificity_per_rule() {
    let (code, hash) = rendered(".card { color: red; }\n.title { color: blue; }");
    // The bump state resets per rule-level selector list: each rule's first
    // scoped compound takes the plain class selector.
    assert_eq!(
        code,
        format!(".card.{hash} {{ color: red; }}\n.title.{hash} {{ color: blue; }}")
    );
    // Should-NOT: no `:where` anywhere — the bump never leaks across rules.
    assert!(!code.contains(":where"));
}

#[test]
fn global_pseudo_unwraps_and_is_never_scope_classed() {
    let (code, hash) = rendered(":global(.x) { color: red; }");
    // `:global(` and `)` are removed; the inner selector is emitted bare.
    assert_eq!(code, ".x { color: red; }");
    // Should-NOT: a global selector never receives the scope class and the
    // `:global` wrapper never survives.
    assert!(!code.contains(&hash));
    assert!(!code.contains(":global"));
}

#[test]
fn mid_compound_global_pseudo_is_stripped_and_compound_still_scoped() {
    let (code, hash) = rendered(".card:global(.x) { color: red; }");
    // The mid-compound `:global(...)` wrapper is stripped; the scope class
    // lands on the last non-pseudo simple selector (`.card`).
    assert_eq!(code, format!(".card.{hash}.x {{ color: red; }}"));
    assert!(!code.contains(":global"));
}

#[test]
fn global_block_comment_wraps_wrapper_and_keeps_body() {
    let (code, hash) = rendered(":global {\n.x { color: red; }\n}");
    // The `:global` prelude + braces are comment-wrapped; the body is kept.
    assert_eq!(code, "/* :global {*/\n.x { color: red; }\n/*}*/");
    // Should-NOT: rules inside a global block are neither scope-classed nor
    // pruned as unused.
    assert!(!code.contains(&hash));
    assert!(!code.contains("(unused)"));
}

#[test]
fn unused_rule_is_comment_wrapped_and_used_rule_is_not() {
    let (code, hash) = rendered(".card { color: red; }\n.missing { color: blue; }");
    assert_eq!(
        code,
        format!(".card.{hash} {{ color: red; }}\n/* (unused) .missing {{ color: blue; }}*/")
    );
    // Should-NOT: the used rule stays live (never wrapped), and an unused
    // rule's selectors never receive the scope class.
    assert!(code.starts_with(&format!(".card.{hash} {{")));
    assert!(!code.contains(&format!(".missing.{hash}")));
}

#[test]
fn empty_rule_is_comment_wrapped_as_empty_not_unused() {
    let (code, hash) = rendered(".card {}");
    assert_eq!(code, "/* (empty) .card {}*/");
    // Should-NOT: the empty wrap wins over the unused wrap, and an empty
    // rule's selectors are never visited (no scope class).
    assert!(!code.contains("(unused)"));
    assert!(!code.contains(&hash));
}

#[test]
fn local_keyframes_renamed_and_animation_shorthand_token_rewritten() {
    let (code, hash) = rendered(
        "@keyframes spin { from { opacity: 0; } to { opacity: 1; } }\n.card { animation: spin 1s linear; }",
    );
    assert_eq!(
        code,
        format!(
            "@keyframes {hash}-spin {{ from {{ opacity: 0; }} to {{ opacity: 1; }} }}\n.card.{hash} {{ animation: {hash}-spin 1s linear; }}"
        )
    );
    // Should-NOT: the bare local name never survives in the at-rule or the
    // declaration value, and nothing WITHIN the keyframes block is scoped.
    assert!(!code.contains("@keyframes spin"));
    assert!(!code.contains("animation: spin"));
    assert!(!code.contains(&format!("from.{hash}")));
}

#[test]
fn animation_name_declaration_token_rewritten() {
    let (code, hash) =
        rendered("@keyframes spin { from { opacity: 0; } }\n.card { animation-name: spin; }");
    assert!(code.contains(&format!("animation-name: {hash}-spin;")));
    assert!(!code.contains("animation-name: spin;"));
}

#[test]
fn global_prefixed_keyframes_strips_prefix_without_rename() {
    let (code, hash) =
        rendered("@keyframes -global-foo { from { opacity: 0; } }\n.card { animation-name: foo; }");
    assert_eq!(
        code,
        format!(
            "@keyframes foo {{ from {{ opacity: 0; }} }}\n.card.{hash} {{ animation-name: foo; }}"
        )
    );
    // Should-NOT: the `-global-` prefix is gone, the name is NOT hash-renamed,
    // and references to a global keyframe are NOT rewritten.
    assert!(!code.contains("-global-"));
    assert!(!code.contains(&format!("{hash}-foo")));
}

#[test]
fn selector_list_prunes_leading_unused_run_up_to_comma() {
    let (code, hash) = rendered(".missing, .card { color: red; }");
    assert_eq!(
        code,
        format!("/* (unused) .missing,*/ .card.{hash} {{ color: red; }}")
    );
    // Should-NOT: the used selector stays outside the comment and the rule
    // itself is not wrapped.
    assert!(!code.starts_with("/* (unused) .missing, .card"));
}

#[test]
fn selector_list_prunes_trailing_unused_run_after_comma() {
    let (code, hash) = rendered(".card, .missing { color: red; }");
    assert_eq!(
        code,
        format!(".card.{hash} /* (unused) .missing*/ {{ color: red; }}")
    );
    // Should-NOT: the scope class stays on the used selector, outside the
    // pruned run.
    assert!(code.starts_with(&format!(".card.{hash} /*")));
}

#[test]
fn comment_close_inside_unused_rule_is_escaped() {
    let (code, _hash) = rendered(".missing { /* x*/ color: red; }");
    // The wrapping unused-comment must survive an interior `*/` — the
    // official renderer escapes the close to `*\/`.
    assert_eq!(code, "/* (unused) .missing { /* x*\\/ color: red; }*/");
}

#[test]
fn star_type_selector_is_replaced_by_scope_class() {
    // Official: a scoped `*` TypeSelector is REPLACED by the scope class
    // (`update(selector.start, selector.end, modifier)`), never appended-to.
    let (code, hash) = rendered("* { color: red; }");
    assert_eq!(code, format!(".{hash} {{ color: red; }}"));
    // Should-NOT: the `*` token never survives, and the replacement is the
    // plain class form (first scoped compound — no `:where` bump).
    assert!(!code.contains('*'));
    assert!(!code.contains(":where"));
}

#[test]
fn closing_unused_comment_survives_adjacent_bare_global_unwrap() {
    // `.missing,:global x` — the prune closes its `/* (unused) ` comment with
    // an `appendRight`-affinity `*/` at the EXACT byte where the bare
    // `:global` token's content-only `update(start, end, "")` range begins.
    // Official output (svelte@5.56.3): the comment close SURVIVES the update.
    let (code, hash) = rendered(".missing,:global x { color: red; }");
    assert_eq!(code, "/* (unused) .missing,*/ x { color: red; }");
    // Should-NOT: a regression to `overwrite` clears the `*/` and the whole
    // rule dies inside an unclosed comment; the global selector is never
    // scope-classed.
    assert!(code.contains("*/"));
    assert!(!code.contains(&hash));
}

#[test]
fn nested_bare_global_amp_prefix_composes_with_preserved_comment_close() {
    // The nested `&`-prefix (`prependRight`) stacks in FRONT of the preserved
    // `*/` on the same boundary chunk — official emits the `&` inside the
    // unused comment and keeps `.x` as the live nested selector.
    let (code, hash) = rendered(".card { .missing,:global.x { color: red; } }");
    assert_eq!(
        code,
        format!(".card.{hash} {{ /* (unused) .missing,&*/.x {{ color: red; }} }}")
    );
}

#[test]
fn prune_boundary_comment_at_global_args_form_is_cleared_by_remove() {
    // The ARGUMENT form `:global(...)` unwraps through `remove`, which CLEARS
    // boundary insertions — official output loses the `*/` here (the comment
    // stays unclosed). Byte-parity means porting that too: the contrast with
    // the bare-`:global` update case above is the two-sided discriminator.
    let (code, _hash) = rendered(".missing,:global(.x) { color: red; }");
    assert_eq!(code, "/* (unused) .missing,.x { color: red; }");
    // Should-NOT: no comment close survives the removed range.
    assert!(!code.contains("*/"));
}

#[test]
fn scoped_styles_fixture_render_matches_committed_golden_css_code() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/svelte_oracle_corpus");
    let fixture = std::fs::read_to_string(root.join("fixtures/css/scoped_styles.svelte"))
        .expect("the committed scoped_styles fixture reads");
    let golden: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(root.join("goldens/css/scoped_styles.client.json"))
            .expect("the committed scoped_styles golden reads"),
    )
    .expect("the golden parses as JSON");

    // The oracle compiled with the fixture-relative filename — the hash
    // input. A constructed plan is proven by construction (an unprovable
    // fixture would have failed the build).
    let plan = plan_for(&fixture, Some("css/scoped_styles.svelte"));

    let golden_hash = golden["css"]["hash"].as_str().expect("golden css.hash");
    let golden_code = golden["css"]["code"].as_str().expect("golden css.code");
    assert_eq!(plan.hash, golden_hash);

    // Mask the live hash exactly as the golden generator does
    // (`maskScopeHash`: every `svelte-<djb2>` token → `svelte-<scoped>`) and
    // byte-compare, line-ending-normalized on both sides.
    let normalize = |s: &str| s.replace("\r\n", "\n");
    let masked = normalize(&plan.css_code).replace(plan.hash.as_str(), "svelte-<scoped>");
    assert_eq!(masked, normalize(golden_code));
}

#[test]
fn plan_populates_css_code_for_proven_template_and_unprovable_never_plans() {
    // Proven: the rendered stylesheet lands on the plan.
    let (code, hash) = rendered(".card { color: red; }");
    assert!(!code.is_empty());
    assert!(code.contains(&hash));

    // Unprovable (a `<svelte:head>` `<title>`, decomposed out of the runtime
    // IR fragment): NO plan exists — the build fails with the typed
    // selector-unprovable failure (fail-closed, never a guessed scope, never a
    // sentinel plan).
    let source =
        "<svelte:head><title>t</title></svelte:head>\n<style>.card { color: red; }</style>";
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions::default();
    let ir = lower_parsed_svelte_to_ir(source, &parsed, &opts, &alloc).expect("lowering succeeds");
    let err = build_style_scope_plan(
        source,
        body_span(source),
        None,
        CssMode::External,
        &ir,
        false,
    )
    .expect_err("a head-`<title>` template never constructs a plan");
    assert_eq!(
        err.class,
        super::StylePlanFailureClass::SelectorUnprovable,
        "the failure names the selector-unprovable class: {err:?}"
    );
}

// ─── fail-closed hardening: malformed spans refuse, never panic ─────────────
//
// `render_stylesheet` runs on every PROVEN styled component during refusal
// classification; a pathological AST/span must surface as a typed refusal
// (the caller keeps the component refused), never a host panic. Each test
// below panicked at a distinct render site before the hardening.

/// Parse + analyze `source`'s style body — a WELL-FORMED base AST the
/// pathological tests then corrupt one span/flag at a time.
fn analyzed_ast(source: &str) -> super::types::StyleSheet {
    let mut ast = super::parse::parse_style_body(source, body_span(source)).expect("body parses");
    super::analyze::analyze_stylesheet(source, &mut ast).expect("body analyzes");
    ast
}

#[test]
fn render_fails_closed_on_out_of_range_selector_span() {
    // The prune comma back-scan starts past the source end.
    let source = "<style>.a,.b { color: red; }</style>";
    let mut ast = analyzed_ast(source);
    let super::types::StyleChild::Rule(rule) = &mut ast.children[0] else {
        panic!("the sheet's child is a rule");
    };
    rule.prelude.children[0].metadata.used = false;
    rule.prelude.children[1].metadata.used = true;
    rule.prelude.children[1].span = Span::new(10_000, 10_002);
    let result =
        super::render::render_stylesheet(source, &ast, "svelte-x", &[], false, None, false);
    assert_eq!(
        result,
        Err(super::render::RenderError {
            span: Span::new(10_000, 10_002)
        })
    );
}

#[test]
fn render_fails_closed_when_comma_scan_reaches_offset_zero() {
    // A pruned-run close whose span sits BEFORE any comma: the official
    // back-scan would walk past offset 0 (the `comma -= 1` underflow).
    let source = "<style>.a,.b { color: red; }</style>";
    let mut ast = analyzed_ast(source);
    let super::types::StyleChild::Rule(rule) = &mut ast.children[0] else {
        panic!("the sheet's child is a rule");
    };
    rule.prelude.children[0].metadata.used = false;
    rule.prelude.children[1].metadata.used = true;
    rule.prelude.children[1].span = Span::new(1, 3);
    let result =
        super::render::render_stylesheet(source, &ast, "svelte-x", &[], false, None, false);
    assert_eq!(
        result,
        Err(super::render::RenderError {
            span: Span::new(1, 3)
        })
    );
}

#[test]
fn render_fails_closed_on_global_marker_without_leading_pseudo_class() {
    // `is_global` set on a compound that does NOT lead with a pseudo-class —
    // the inconsistent-metadata shape (previously an `unreachable!`).
    let source = "<style>.a { color: red; }</style>";
    let mut ast = analyzed_ast(source);
    let super::types::StyleChild::Rule(rule) = &mut ast.children[0] else {
        panic!("the sheet's child is a rule");
    };
    rule.prelude.children[0].metadata.used = true;
    rule.prelude.children[0].children[0].metadata.is_global = true;
    let result =
        super::render::render_stylesheet(source, &ast, "svelte-x", &[], false, None, false);
    assert!(result.is_err(), "an inconsistent is_global marker refuses");
}

#[test]
fn render_fails_closed_on_out_of_range_stylesheet_span() {
    // The final trim's remove range extends past the source end (the
    // checked-transform refusal path).
    let source = "<style>.a { color: red; }</style>";
    let mut ast = analyzed_ast(source);
    ast.span = Span::new(ast.span.start, source.len() as u32 + 40);
    let result =
        super::render::render_stylesheet(source, &ast, "svelte-x", &[], false, None, false);
    assert!(result.is_err(), "an out-of-range sheet span refuses");
}

#[test]
fn valid_css_still_renders_ok_through_the_fallible_signature() {
    // The hardening must not change the faithful output for valid CSS: the
    // same analyzed AST renders Ok with the exact pre-hardening bytes.
    let source = "<style>.a { color: red; }</style>";
    let mut ast = analyzed_ast(source);
    let super::types::StyleChild::Rule(rule) = &mut ast.children[0] else {
        panic!("the sheet's child is a rule");
    };
    rule.prelude.children[0].metadata.used = true;
    rule.prelude.children[0].children[0].metadata.scoped = true;
    let code = super::render::render_stylesheet(source, &ast, "svelte-x", &[], false, None, false)
        .expect("valid css renders")
        .code;
    assert_eq!(code, ".a.svelte-x { color: red; }");
}

// ─── fail-closed hardening: out-of-range span-SCAN anchors refuse (never a
// silent skip / partial stylesheet) ──────────────────────────────────────────

/// The `animation: spin` declaration inside `.card`, with `spin` a LOCAL
/// keyframe — the base shape whose declaration span the tests then
/// mis-anchor. Returns `(ast, keyframes)` ready for a direct render.
fn keyframed_ast(source: &str) -> (super::types::StyleSheet, Vec<super::types::KeyframeName>) {
    let mut ast = super::parse::parse_style_body(source, body_span(source)).expect("body parses");
    let analysis = super::analyze::analyze_stylesheet(source, &mut ast).expect("body analyzes");
    // Mark everything used/scoped so the render walks the declaration.
    fn mark(children: &mut [super::types::StyleChild]) {
        for child in children {
            match child {
                super::types::StyleChild::Rule(rule) => {
                    for sel in &mut rule.prelude.children {
                        sel.metadata.used = true;
                    }
                }
                super::types::StyleChild::Atrule(_) => {}
            }
        }
    }
    mark(&mut ast.children);
    (ast, analysis.keyframes)
}

/// Reach the `animation` declaration of the LAST rule in the sheet.
fn animation_declaration(ast: &mut super::types::StyleSheet) -> &mut super::types::Declaration {
    let super::types::StyleChild::Rule(rule) = ast.children.last_mut().expect("a last child")
    else {
        panic!("the last child is the .card rule");
    };
    let super::types::BlockChild::Declaration(decl) =
        rule.block.children.first_mut().expect("a declaration")
    else {
        panic!("the rule's first block child is the animation declaration");
    };
    decl
}

#[test]
fn render_fails_closed_on_past_eof_animation_declaration_span() {
    // THE out-of-range span-scan leak: a declaration span whose derived scan
    // anchor (`span.start + property.len() + 1`) lands PAST the source end.
    // Without the fail-closed guard the token scan silently does not execute
    // and the render returns a PARTIAL stylesheet — the keyframes rename to
    // `<hash>-spin` while the `animation: spin` reference stays UNREWRITTEN
    // (a broken animation at runtime). The guard turns it into a refusal.
    let source =
        "<style>@keyframes spin { from { opacity: 0 } }\n.card { animation: spin; }</style>";
    let (mut ast, keyframes) = keyframed_ast(source);
    assert_eq!(keyframes.len(), 1, "spin is a local keyframe");
    let decl = animation_declaration(&mut ast);
    assert_eq!(decl.property, "animation");
    // Mis-anchor the declaration START past EOF (the span end follows suit —
    // only the derived scan anchor matters).
    decl.span = Span::new(10_000, 10_010);
    let mis_anchored = decl.span;
    let result =
        super::render::render_stylesheet(source, &ast, "svelte-x", &keyframes, false, None, false);
    assert_eq!(
        result,
        Err(super::render::RenderError { span: mis_anchored }),
        "a past-EOF animation scan anchor must refuse the WHOLE render — never \
         a partial stylesheet with an unrewritten animation reference"
    );
}

#[test]
fn render_fails_closed_on_at_eof_anchor_with_nonempty_animation_value() {
    // The boundary case: the derived anchor lands EXACTLY at the source end
    // while the declaration still claims a NON-EMPTY value — its tokens are
    // unreachable, so a silent no-scan would drop the rewrite. Refuse.
    let source =
        "<style>@keyframes spin { from { opacity: 0 } }\n.card { animation: spin; }</style>";
    let (mut ast, keyframes) = keyframed_ast(source);
    let decl = animation_declaration(&mut ast);
    // `span.start + "animation".len() + 1 == source.len()`.
    let start = (source.len() - "animation".len() - 1) as u32;
    decl.span = Span::new(start, source.len() as u32);
    let mis_anchored = decl.span;
    let result =
        super::render::render_stylesheet(source, &ast, "svelte-x", &keyframes, false, None, false);
    assert_eq!(
        result,
        Err(super::render::RenderError { span: mis_anchored })
    );
}

#[test]
fn well_anchored_animation_declaration_still_rewrites() {
    // The guard must not change the faithful path: the untouched AST renders
    // Ok with BOTH the keyframes rename AND the animation token rewrite.
    let source =
        "<style>@keyframes spin { from { opacity: 0 } }\n.card { animation: spin; }</style>";
    let (ast, keyframes) = keyframed_ast(source);
    let code =
        super::render::render_stylesheet(source, &ast, "svelte-x", &keyframes, false, None, false)
            .expect("valid css renders")
            .code;
    assert!(code.contains("@keyframes svelte-x-spin"), "{code}");
    assert!(code.contains("animation: svelte-x-spin;"), "{code}");
    assert!(!code.contains("animation: spin"), "{code}");
}

#[test]
fn render_fails_closed_on_past_eof_minify_declaration_span() {
    // The minify colon-collapse shares the same derived-anchor shape
    // (`span.start + property.len() + 1`) for NON-animation declarations — a
    // past-EOF anchor refuses there too, never a silent skip.
    let source = "<style>.card { color: red; }</style>";
    let mut ast = analyzed_ast(source);
    let super::types::StyleChild::Rule(rule) = &mut ast.children[0] else {
        panic!("the sheet's child is a rule");
    };
    rule.prelude.children[0].metadata.used = true;
    let super::types::BlockChild::Declaration(decl) =
        rule.block.children.first_mut().expect("a declaration")
    else {
        panic!("the rule's first block child is the color declaration");
    };
    decl.span = Span::new(10_000, 10_010);
    let result = super::render::render_stylesheet(source, &ast, "svelte-x", &[], true, None, false);
    assert!(
        result.is_err(),
        "a past-EOF minify collapse anchor must refuse: {result:?}"
    );
}

// ─── the `*` TypeSelector scope path: content-only `update`, never append ────

#[test]
fn star_selector_update_preserves_prune_close_at_its_start_boundary() {
    // `.unused,*` with NO whitespace after the comma: the pruned-run close
    // `*/` is appendRight'ed at `comma + 1` — exactly the `*` selector's
    // START boundary, so it lands on the `*` chunk's INTRO. The scope
    // application must REPLACE `*` via the content-only `update` (official
    // css/index.js), which PRESERVES that boundary insertion:
    //
    //   /* (unused) .unused,*/.svelte-<hash> { color: red; }
    //
    // (oracle-pinned against svelte@5.56.3). An `overwrite` would clear the
    // intro and LOSE the closing `*/` (an unbalanced comment swallowing the
    // whole rule); an `append` would keep the `*` (`*.svelte-<hash>`,
    // double-applying universal matching). Both mutations go RED here.
    let (code, hash) = rendered(".unused,* { color: red; }");
    assert_eq!(
        code,
        format!("/* (unused) .unused,*/.{hash} {{ color: red; }}")
    );
    // Negative: the `*` token is REPLACED, and the comment is balanced.
    assert!(
        !code.contains('*') || code.matches("*/").count() == 1,
        "{code}"
    );
    assert!(!code.contains(&format!("*.{hash}")), "{code}");
}

// ─── the css source map: produced on demand from the SAME shared transform ───

/// An analyzed AST with the lone `.card` selector marked used + scoped — the
/// minimal PROVEN shape for a direct render (the matcher verdicts set by
/// hand, exactly like the fail-closed fixtures above).
fn scoped_card_ast(source: &str) -> super::types::StyleSheet {
    let mut ast = analyzed_ast(source);
    let super::types::StyleChild::Rule(rule) = &mut ast.children[0] else {
        panic!("the sheet's child is a rule");
    };
    rule.prelude.children[0].metadata.used = true;
    rule.prelude.children[0].children[0].metadata.scoped = true;
    ast
}

#[test]
fn source_map_is_produced_only_on_demand_with_identical_code() {
    // A/B on `want_source_map`: OFF ⇒ `None`; ON ⇒ `Some(valid JSON)` whose
    // sources/sourcesContent reference the ORIGINAL component source — and
    // the rendered css bytes are IDENTICAL either way (the map demand is
    // map-only, never a render input).
    let source = "<div class=\"card\">x</div>\n<style>\n.card { color: red; }\n</style>";
    let ast = scoped_card_ast(source);
    let off = super::render::render_stylesheet(
        source,
        &ast,
        "svelte-x",
        &[],
        false,
        Some("App.svelte"),
        false,
    )
    .expect("valid css renders");
    assert_eq!(off.source_map, None, "no demand, no map");
    let on = super::render::render_stylesheet(
        source,
        &ast,
        "svelte-x",
        &[],
        false,
        Some("App.svelte"),
        true,
    )
    .expect("valid css renders");
    assert_eq!(
        on.code, off.code,
        "the map demand never changes the rendered bytes"
    );
    let json = on.source_map.expect("a map on demand");
    let map =
        oxc_sourcemap::OwnedSourceMap::from_json_string(&json).expect("valid source-map JSON");
    assert_eq!(
        map.get_sources().collect::<Vec<_>>(),
        ["App.svelte"],
        "the map names the component source file"
    );
    assert_eq!(
        map.get_source_content(0),
        Some(source),
        "sourcesContent embeds the ORIGINAL component source"
    );
}

#[test]
fn source_map_names_fall_back_to_unknown_and_basename_like_official() {
    // Official svelte@5.56.3 first-hand: the css map rides magic-string's
    // `generateMap({ source: options.filename, file: options.filename })`
    // over VALIDATED options (a missing filename defaults to `"(unknown)"`),
    // and magic-string emits the BASENAME (`file.split(/[/\\]/).pop()`) for
    // `file` and resolves `sources[0]` relative to `file`'s directory — with
    // source === file that is exactly the basename. So: NO filename ⇒
    // `map.file == "(unknown)"`, `map.sources == ["(unknown)"]`, with REAL
    // mappings and the FULL component source embedded; `src/Foo.svelte` (or
    // the Windows spelling) ⇒ `"Foo.svelte"` for both.
    let source = "<div class=\"card\">x</div>\n<style>\n.card { color: red; }\n</style>";
    let ast = scoped_card_ast(source);
    let render = |filename: Option<&str>| {
        let rendered =
            super::render::render_stylesheet(source, &ast, "svelte-x", &[], false, filename, true)
                .expect("valid css renders");
        let json = rendered.source_map.expect("a map on demand");
        (
            rendered.code,
            oxc_sourcemap::OwnedSourceMap::from_json_string(&json).expect("valid source-map JSON"),
        )
    };

    // NO filename ⇒ `(unknown)` for BOTH `file` and `sources[0]` — the map
    // still carries real mappings pointing the rendered css back to source.
    let (code, map) = render(None);
    assert_eq!(
        map.get_file(),
        Some("(unknown)"),
        "official: a missing filename maps `file` to `(unknown)`"
    );
    assert_eq!(
        map.get_sources().collect::<Vec<_>>(),
        ["(unknown)"],
        "official: a missing filename still names ONE source, `(unknown)`"
    );
    assert_eq!(
        map.get_source_content(0),
        Some(source),
        "sourcesContent still embeds the ORIGINAL component source"
    );
    assert!(
        map.get_tokens().next().is_some(),
        "the `(unknown)` source still carries REAL mappings"
    );
    // A rendered css token maps back to its source offset through the
    // `(unknown)` source (the mapping rail is name-independent).
    let lookup = crate::framework_common::sourcemap_e2e_helpers::build_lookup_table(&map);
    crate::framework_common::sourcemap_e2e_helpers::assert_token_maps_to_source(
        &map, &lookup, &code, source, ".card", 0,
    );

    // A PATH filename ⇒ the BASENAME for both fields (magic-string
    // semantics), under BOTH separators.
    let (_, map) = render(Some("src/Foo.svelte"));
    assert_eq!(map.get_file(), Some("Foo.svelte"));
    assert_eq!(map.get_sources().collect::<Vec<_>>(), ["Foo.svelte"]);
    let (_, map) = render(Some("src\\win\\Foo.svelte"));
    assert_eq!(map.get_file(), Some("Foo.svelte"));
    assert_eq!(map.get_sources().collect::<Vec<_>>(), ["Foo.svelte"]);
    // A bare filename is its own basename (the existing `App.svelte` case
    // stays byte-identical).
    let (_, map) = render(Some("Foo.svelte"));
    assert_eq!(map.get_file(), Some("Foo.svelte"));
    assert_eq!(map.get_sources().collect::<Vec<_>>(), ["Foo.svelte"]);
}

#[test]
fn source_map_tokens_point_css_code_back_to_the_style_source_spans() {
    // The mapping correctness bar: each rendered selector token maps back to
    // its ORIGINAL `<style>` source span (the render edits by source
    // position, so the surviving source chunks carry exact offsets). Driven
    // through the REAL pipeline (parse → analyze → match → render) so the
    // map is the one a production compile publishes.
    let source = "<div class=\"card\"><p class=\"title\">x</p></div>\n<style>\n.card { color: red; }\n.title { padding: 0; }\n</style>";
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    let ir = lower_parsed_svelte_to_ir(source, &parsed, &opts, &alloc).expect("lowering succeeds");
    let plan = build_style_scope_plan(
        source,
        body_span(source),
        Some("App.svelte"),
        CssMode::External,
        &ir,
        true,
    )
    .expect("a clean body plans");
    let json = plan
        .source_map
        .as_deref()
        .expect("the plan carries the demanded css map");
    let map = oxc_sourcemap::OwnedSourceMap::from_json_string(json).expect("valid source-map JSON");
    let lookup = crate::framework_common::sourcemap_e2e_helpers::build_lookup_table(&map);
    // Both scoped selectors survive the render (the scope class APPENDS after
    // each) — each generated token maps to the selector's source offset.
    for selector in [".card", ".title"] {
        crate::framework_common::sourcemap_e2e_helpers::assert_token_maps_to_source(
            &map,
            &lookup,
            &plan.css_code,
            source,
            selector,
            0,
        );
    }
}

#[test]
fn css_source_map_emitted_mappings_are_all_correct_though_coarser_than_svelte() {
    // Hard-checked first-hand against the official svelte@5.56.3 css.map for
    // this same fixture: the official css transform registers every CSS AST
    // node start/end (`addSourcemapLocation(node.start/end)` in
    // 3-transform/css/index.js), so its map carries EXTRA segments at
    // block/declaration boundaries (e.g. the `{`, the `color` declaration
    // start and end) that Verter's chunk-start map does not emit. Verter's
    // map is COARSER (fewer segments) but every segment it DOES emit is
    // CORRECT — per-node granularity parity is a sourcemap-hardening
    // completeness follow-up tracked in `docs/arch/svelte-native-compiler-plan.md`. The
    // correctness FLOOR this test pins (must hold now and after that
    // hardening):
    //   (a) every MAPPED segment points at source text IDENTICAL to the
    //       generated text it covers (segment-by-segment text equality, not a
    //       bounds check) — no emitted mapping may point at a wrong source
    //       position;
    //   (b) every UNMAPPED segment covers exactly an inserted scope class
    //       (`.svelte-hash`) — inserted content never fabricates a source
    //       claim (the official map instead lets insertions extend the
    //       preceding segment);
    //   (c) the selector tokens map EXACTLY, and the surviving post-insertion
    //       declaration chunk (` { color: red; }` after the inserted scope
    //       class) maps back to the source position immediately after
    //       `.card` — the chunk-start mapping for the declaration region.
    let source = "<div class=\"card\"><p class=\"title\">x</p></div>\n<style>\n.card { color: red; }\n.title { padding: 0; }\n</style>";
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions {
        filename: Some("App.svelte".to_string()),
        ..Default::default()
    };
    let ir = lower_parsed_svelte_to_ir(source, &parsed, &opts, &alloc).expect("lowering succeeds");
    let plan = build_style_scope_plan(
        source,
        body_span(source),
        Some("App.svelte"),
        CssMode::External,
        &ir,
        true,
    )
    .expect("a clean body plans");
    let code = &plan.css_code;
    let json = plan
        .source_map
        .as_deref()
        .expect("the plan carries the demanded css map");
    let map = oxc_sourcemap::OwnedSourceMap::from_json_string(json).expect("valid source-map JSON");
    use crate::framework_common::sourcemap_e2e_helpers as helpers;

    let mut tokens: Vec<_> = map.get_tokens().collect();
    tokens.sort_by_key(|t| (t.get_dst_line(), t.get_dst_col()));
    assert!(!tokens.is_empty(), "the css map carries emitted mappings");

    let inserted_scope_class = format!(".{}", plan.hash);
    let mut mapped_count = 0usize;
    let mut unmapped_count = 0usize;
    for (i, token) in tokens.iter().enumerate() {
        // The generated segment this token covers: from the token to the next
        // token on the SAME generated line, else to that line's end (tokens
        // are emitted per chunk start and per line within a chunk, so no
        // segment spans a newline).
        let gen_start =
            helpers::line_col_to_byte_offset(code, token.get_dst_line(), token.get_dst_col())
                .expect("generated token position is in bounds");
        let gen_end = match tokens.get(i + 1) {
            Some(next) if next.get_dst_line() == token.get_dst_line() => {
                helpers::line_col_to_byte_offset(code, next.get_dst_line(), next.get_dst_col())
                    .expect("next generated token position is in bounds")
            }
            _ => code[gen_start..]
                .find('\n')
                .map_or(code.len(), |nl| gen_start + nl),
        };
        let gen_segment = &code[gen_start..gen_end];

        match token.get_source_id() {
            Some(_) => {
                mapped_count += 1;
                let src_offset = helpers::line_col_to_byte_offset(
                    source,
                    token.get_src_line(),
                    token.get_src_col(),
                )
                .unwrap_or_else(|| {
                    panic!(
                        "mapped token gen {}:{} -> src {}:{} is OUT OF BOUNDS in the source",
                        token.get_dst_line(),
                        token.get_dst_col(),
                        token.get_src_line(),
                        token.get_src_col()
                    )
                });
                if gen_segment.is_empty() {
                    // A token at a generated line end (the leading body `\n`
                    // chunk): the mapped source position must sit at a line
                    // end too, so the next generated char (`\n`) corresponds.
                    let src_rest_of_line = source[src_offset..].split('\n').next().unwrap_or("");
                    assert!(
                        src_rest_of_line.is_empty(),
                        "line-end token gen {}:{} maps to src {}:{}, which is NOT at a line end \
                         (rest of source line: {src_rest_of_line:?})",
                        token.get_dst_line(),
                        token.get_dst_col(),
                        token.get_src_line(),
                        token.get_src_col()
                    );
                } else {
                    let src_end = src_offset + gen_segment.len();
                    let src_segment = source.get(src_offset..src_end).unwrap_or_else(|| {
                        panic!(
                            "mapped token gen {}:{} -> src {}:{}: source slice out of \
                             bounds/off-boundary for generated segment {gen_segment:?}",
                            token.get_dst_line(),
                            token.get_dst_col(),
                            token.get_src_line(),
                            token.get_src_col()
                        )
                    });
                    assert_eq!(
                        src_segment,
                        gen_segment,
                        "MIS-MAP: token gen {}:{} -> src {}:{} points at source text \
                         {src_segment:?}, but the generated segment is {gen_segment:?}",
                        token.get_dst_line(),
                        token.get_dst_col(),
                        token.get_src_line(),
                        token.get_src_col()
                    );
                }
            }
            None => {
                unmapped_count += 1;
                // The scoped render's ONLY insertions are the appended scope
                // classes: an unmapped segment covering anything else would
                // mean original source text lost its provenance.
                assert_eq!(
                    gen_segment,
                    inserted_scope_class,
                    "unmapped segment at gen {}:{} is not the inserted scope class",
                    token.get_dst_line(),
                    token.get_dst_col()
                );
            }
        }
    }
    // The fixture's two scoped rules survive whole: chunk-start + per-line
    // mappings for the original chunks, one unmapped insertion per selector.
    assert!(
        mapped_count >= 4,
        "expected the two rules' chunk mappings (got {mapped_count} mapped tokens)"
    );
    assert_eq!(
        unmapped_count, 2,
        "exactly the two inserted scope classes are unmapped"
    );

    // (c) positive floor — the selector tokens map EXACTLY…
    let lookup = helpers::build_lookup_table(&map);
    for selector in [".card", ".title"] {
        helpers::assert_token_maps_to_source(&map, &lookup, code, source, selector, 0);
    }
    // …and the surviving post-insertion declaration chunk maps back to the
    // source position immediately after `.card` (source line 2, the ` {`
    // right after the selector) — the M1-cited region.
    let decl = " { color: red; }";
    let gen_decl_offset = code.find(decl).expect("the declaration chunk survives");
    let (gen_line, gen_col) = helpers::byte_offset_to_line_col(code, gen_decl_offset);
    let token = map
        .lookup_token(&lookup, gen_line, gen_col)
        .expect("a token covers the post-insertion declaration chunk");
    assert_eq!(
        (
            token.get_dst_line(),
            token.get_dst_col(),
            token.get_src_line(),
            token.get_src_col()
        ),
        (gen_line, gen_col, 2, 5),
        "the surviving declaration chunk's own chunk-start mapping points at the source \
         ` {{ color: red; }}` right after `.card`"
    );
    let src_decl_offset =
        helpers::line_col_to_byte_offset(source, 2, 5).expect("src 2:5 in bounds");
    assert!(
        source[src_decl_offset..].starts_with(decl),
        "src 2:5 carries the declaration text"
    );
}

#[test]
fn nbsp_between_property_and_colon_still_renames_animation_keyframes() {
    // Oracle-confirmed against svelte@5.56.3:
    // `.x{animation\u{a0}: spin 1s}@keyframes spin{}` parses the property as
    // `animation` (the `/[\s:]/` property scan stops at the NBSP), so the
    // Declaration visitor renames BOTH the value reference and the keyframes
    // rule: `.x.<hash>{animation\u{a0}: <hash>-spin 1s}@keyframes <hash>-spin{}`.
    // The official value-scan anchor (`node.start + node.property.length + 1`)
    // skips ONE source char (UTF-16 unit) after the property — with the NBSP
    // there, the scan starts at the `:` and still finds `spin`.
    let source =
        "<div class=\"x\">y</div>\n<style>.x{animation\u{a0}: spin 1s}@keyframes spin{}</style>";
    let plan = plan_for(source, None);
    let hash = &plan.hash;
    assert_eq!(
        plan.css_code,
        format!(
            ".x.{hash}{{animation\u{a0}: {hash}-spin 1s}}@keyframes {hash}-spin{}",
            "{}"
        )
    );
    // Should-NOT: the un-renamed value reference must not survive (`: spin `)
    // and the render must not have treated `animation\u{a0}` as a foreign
    // property (which would leave the reference bare).
    assert!(!plan.css_code.contains(": spin"));
}

#[test]
fn type_selector_match_uses_unicode_case_fold_like_official() {
    // Oracle-confirmed vs svelte@5.56.3: `<k>` + `\u{212A}{color:red}` (the
    // KELVIN SIGN selector) → `\u{212A}.svelte-hash{color:red}` — the
    // official TypeSelector compare is `element.name.toLowerCase() !==
    // name.toLowerCase()` (css-prune.js, FULL Unicode fold), and
    // `'\u{212A}'.toLowerCase() === 'k'`, so the rule is used + scoped. An
    // ASCII-only fold wrongly prunes it `(unused)`. (The selector side is the
    // reachable non-ASCII carrier: `read_identifier` accepts any cp ≥ 160.)
    let source = "<k>x</k>\n<style>\u{212A}{color:red}</style>";
    let plan = plan_for(source, None);
    let hash = &plan.hash;
    assert_eq!(plan.css_code, format!("\u{212A}.{hash}{{color:red}}"));
    assert!(!plan.css_code.contains("(unused)"));
}

/// Build a plan and return the `Result` (unlike `plan_for`, which unwraps) so a
/// fail-closed refusal can be asserted.
fn try_plan(
    source: &str,
) -> Result<ProvenStyleScopePlan, crate::svelte::runtime::css::StylePlanFailure> {
    let alloc = Allocator::default();
    let parsed = parse_svelte(source);
    let opts = SvelteRuntimeOptions::default();
    let ir = lower_parsed_svelte_to_ir(source, &parsed, &opts, &alloc).expect("lowering succeeds");
    build_style_scope_plan(
        source,
        body_span(source),
        None,
        CssMode::External,
        &ir,
        false,
    )
}

#[test]
fn escaped_global_keyword_fails_closed_not_wrong_offset_splice() {
    // `:\67 lobal(.x)` is a CSS-escaped `:global(`. The `:global(` removal
    // anchor adds the BYTE length of the literal keyword, but the escaped form
    // is longer than 8 bytes, so a byte splice lands mid-token and mangles the
    // output. svelte@5.56.3 ITSELF mangles this (emits `al(.x{color:red}`), so
    // there is no correct output to match — Verter fails closed instead of
    // emitting a wrong-offset splice. Against the pre-guard code this planned
    // Ok with a mangled `css_code`, so this discriminates.
    let source = "<div class=\"x\">y</div>\n<style>:\\67 lobal(.x){color:red}</style>";
    assert!(
        try_plan(source).is_err(),
        "an escaped :global keyword must fail closed, not emit a wrong-offset splice"
    );
    // Control: the LITERAL `:global(.x)` still plans + renders (the guard only
    // trips on the non-literal keyword).
    let literal = "<div class=\"x\">y</div>\n<style>:global(.x){color:red}</style>";
    assert!(try_plan(literal).is_ok(), "a literal :global still plans");
}

#[test]
fn escaped_keyframes_keyword_fails_closed_not_wrong_offset_splice() {
    // `@\6b eyframes spin` is a CSS-escaped `@keyframes` KEYWORD. The rename
    // anchor is a byte offset off the raw name span, but the official anchor is
    // a UTF-16/decoded offset — they desync on the escaped keyword (svelte
    // itself mangles the `@keyframes` rule so it no longer matches the renamed
    // `animation` reference). Verter fails closed rather than emit a mismatched
    // rename. Against the pre-guard code this planned Ok with mangled output.
    let source =
        "<div class=\"x\">y</div>\n<style>.x{animation:spin}@\\6b eyframes spin{from{opacity:0}}</style>";
    assert!(
        try_plan(source).is_err(),
        "an escaped @keyframes keyword must fail closed, not emit a mismatched rename"
    );
    // Control: the LITERAL `@keyframes spin` (even with a NON-ASCII keyframe
    // NAME) still plans — the guard is on the KEYWORD span only.
    let literal =
        "<div class=\"x\">y</div>\n<style>.x{animation:café}@keyframes café{from{opacity:0}}</style>";
    assert!(
        try_plan(literal).is_ok(),
        "a literal @keyframes with a non-ASCII name still plans"
    );
}
