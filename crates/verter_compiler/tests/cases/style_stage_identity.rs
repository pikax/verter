//! Style bytes leaving the compiler carry the identity that makes them
//! addressable, and preprocessor bytes entering it must arrive with theirs.
//!
//! Each case names a boundary that a bare `String` + private lang table let
//! through: bytes whose byte space the consumer had to guess, a refusal whose
//! coordinate space was implicit, preprocessor output admitted anonymously, and
//! a `lang="…"` spelling that resolved on one route and failed closed on
//! another.

use std::sync::Arc;

use verter_compiler::compile::style_usage::extract_style_v_bind_usage_for_languages;
use verter_compiler::parser::types::StyleLang;
use verter_compiler::style_planner::{
    prepare_supplied_style, run_vue_style_authored_only, run_vue_style_cascade,
    transform_vue_css_modules, transform_vue_scoped_css, transform_vue_style, transform_vue_v_bind,
    AuthoredStyleInput, CascadeInput, PlainCssInput, StyleRewriteOutcome, VerifiedPlainCss,
    VueStyleCascadeOutcome,
};
use verter_css_syntax::{
    parse_style_ir, CssDialect, CssParseMode, CssSource, ExternalStyleProducer,
    QualifiedStyleResult, StyleDependencyKind, StyleProducer, StyleStage,
};

fn plain(code: &str) -> PlainCssInput<'_> {
    PlainCssInput::try_new(
        code,
        CssDialect::Css,
        "probe.style",
        "space:probe",
        "artifact:probe",
    )
    .expect("plain css")
}

fn sass_producer() -> StyleProducer {
    StyleProducer::External(
        ExternalStyleProducer::new("sass", Some("1.77.0"), None).expect("named producer"),
    )
}

fn authored(code: &str, dialect: CssDialect) -> AuthoredStyleInput<'_> {
    AuthoredStyleInput::new(
        code,
        dialect,
        "probe.style",
        "space:probe",
        "artifact:probe",
    )
}

/// A cascade that rewrote bytes and one that did not are different byte spaces.
/// Handing both back as a bare `String` made every consumer decide for itself
/// which one it was holding.
#[test]
fn cascade_output_names_its_stage_dialect_and_producer() {
    let rewritten = run_vue_style_authored_only(
        authored("$c: red;\n.a { color: v-bind(tone); }", CssDialect::Scss),
        "sc1",
        false,
    );
    assert_eq!(rewritten.result.stage(), StyleStage::FrameworkRewritten);
    assert_eq!(
        rewritten.result.dialect(),
        CssDialect::Scss,
        "a framework rewrite keeps the dialect it was handed; only preprocessing leaves it behind"
    );
    assert_eq!(rewritten.result.producer(), &StyleProducer::Verter);
    assert!(rewritten.code().contains("var(--sc1-tone)"));

    let untouched = run_vue_style_authored_only(
        authored(".a { color: red; }", CssDialect::Css),
        "sc1",
        false,
    );
    assert_eq!(
        untouched.result.stage(),
        StyleStage::Authored,
        "bytes no stage changed are still the authored bytes"
    );
    assert_eq!(untouched.code(), ".a { color: red; }");
}

/// A refusal's span is only meaningful against the bytes the refused stage was
/// handed. The cascade is the only thing that knows which those were, so it
/// records the space rather than leaving each consumer to assume one.
#[test]
fn a_rewrite_refusal_reaches_consumers_as_a_stage_qualified_diagnostic() {
    // CSS Modules and selector scoping are plain-CSS-only stages; SCSS input
    // refuses both.
    let outcome = run_vue_style_cascade(
        authored(".a { color: red; }", CssDialect::Scss),
        "sc1",
        true,
        true,
        false,
    );

    let diagnostics = outcome.result.diagnostics();
    assert!(
        !diagnostics.is_empty(),
        "a refused plain-CSS-only stage must surface as a diagnostic"
    );
    for diagnostic in diagnostics {
        assert_eq!(
            diagnostic.stage(),
            StyleStage::Authored,
            "no stage rewrote these bytes, so the refusal addresses the authored space"
        );
    }
    assert_eq!(
        diagnostics.len(),
        outcome.facts.refusals.len() + outcome.stage_failures.len(),
        "every refusal reaches the consumer exactly once"
    );
}

/// When an earlier stage rewrote the bytes, a later stage's refusal is
/// reported against the rewritten ones, and the diagnostic says so.
///
/// This is the branch a consumer's mapping choice turns on. A `v-bind()`
/// rewrite changes the length of the block, so the same offset means different
/// things before and after it: a consumer that runs the refusal's span through
/// the authored block's arithmetic lands past the construct the message names.
/// Nothing about the refusal itself distinguishes the two cases — only the
/// stage does.
#[test]
fn a_refusal_after_a_rewrite_addresses_the_rewritten_space() {
    // The stray `}` is refused by the CSS-Modules stage; it sits after the
    // `v-bind()` the authored stage rewrites into a longer `var()` call.
    let source = ".a { color: v-bind(tone); }\n}\n";
    let outcome =
        run_vue_style_cascade(authored(source, CssDialect::Css), "sc1", true, false, false);

    let [diagnostic] = outcome.result.diagnostics() else {
        panic!("one refusal, got {:?}", outcome.result.diagnostics());
    };
    assert_eq!(
        diagnostic.stage(),
        StyleStage::FrameworkRewritten,
        "the v-bind stage moved these bytes before the refusing stage saw them"
    );

    // The refusing stage clears the output, so the bytes the span addresses
    // are not on this outcome — they are what the v-bind stage alone produced.
    // That is precisely why the stage has to travel with the span: the
    // reported position outlives the text it addresses.
    let rewritten = run_vue_style_authored_only(authored(source, CssDialect::Css), "sc1", false);
    let span = diagnostic.span().expect("this refusal carries a position");
    assert_eq!(
        &rewritten.code()[span.start as usize..span.end as usize],
        "}",
        "the span addresses the rewritten bytes it was reported against"
    );
    assert_ne!(
        source.get(span.start as usize..span.end as usize),
        Some("}"),
        "and reading it out of the authored bytes lands somewhere else"
    );
}

/// Preprocessor bytes are admitted only through the preprocessed-stage
/// witness, and `as_preprocessed` is the single gate that mints one from a
/// result. A result at any other stage yields none, so authored SCSS cannot
/// reach a plain-CSS-only consumer by being called CSS.
///
/// That the consumer REFUSES to accept another stage is proven by the compiler
/// rather than here: `tests/cases/compile-fail/`
/// `prepare_supplied_style_rejects_unqualified_bytes.rs` fails to build when a
/// caller hands it bare bytes.
#[test]
fn only_the_preprocessed_stage_mints_the_supplied_style_witness() {
    let css = ".card { color: red; }";

    let named = QualifiedStyleResult::preprocessed(
        StyleProducer::External(
            verter_css_syntax::ExternalStyleProducer::new("sass", Some("1.77.0"), None)
                .expect("named producer"),
        ),
        css,
        Vec::new(),
        Vec::new(),
    );
    let witness = named
        .as_preprocessed()
        .expect("preprocessed css mints the witness");
    assert_eq!(witness.code(), css);
    let prepared = prepare_supplied_style(witness).expect("preprocessed css is admitted");
    assert_eq!(prepared.ir().source().text(), css);

    assert!(
        QualifiedStyleResult::authored(CssDialect::Css, css, Vec::new(), Vec::new())
            .as_preprocessed()
            .is_none(),
        "authored bytes are not preprocessor output, whatever they contain"
    );
    assert!(
        QualifiedStyleResult::framework_rewritten(CssDialect::Css, css, Vec::new(), Vec::new())
            .as_preprocessed()
            .is_none(),
        "Verter's own rewrite output is not an external supplied artifact"
    );
}

/// The `lang="…"` spelling → dialect identity has one owner. A private table
/// beside it drifts silently: the same spelling resolves on one route and fails
/// closed on another, and nothing reports the disagreement.
#[test]
fn style_lang_spellings_resolve_through_the_single_dialect_owner() {
    for (spelling, _) in CssDialect::LANG_SPELLINGS {
        let usage =
            extract_style_v_bind_usage_for_languages([(".a { color: v-bind(tone); }", spelling)]);
        assert!(
            usage.complete,
            "{spelling:?} is an addressable dialect spelling"
        );
        assert!(usage.used.contains("tone"), "{spelling:?}");
    }

    let unknown = extract_style_v_bind_usage_for_languages([(".a { color: red }", "nocss")]);
    assert!(
        !unknown.complete,
        "an unaddressable spelling must fail open, never claim an exhaustive inventory"
    );
}

/// The owner and the carrier parser's own `lang` classification must accept
/// EXACTLY the same spellings.
///
/// The failure this discriminates is the inverted drift: widen one side and a
/// `<style lang="…">` the SFC pipeline refuses outright still resolves on the
/// usage route, which then publishes an exhaustive-looking `v-bind` inventory
/// for a block that never compiles. Narrow one side and a block the pipeline
/// compiles fine has its bindings reported as unused. Neither side reports the
/// disagreement, so nothing but this catches it.
#[test]
fn the_dialect_owner_accepts_exactly_what_the_carrier_parser_classifies() {
    let owned: Vec<_> = CssDialect::LANG_SPELLINGS
        .into_iter()
        .map(|(spelling, _)| spelling)
        .collect();

    for spelling in &owned {
        assert_ne!(
            StyleLang::from_bytes(spelling.as_bytes()),
            StyleLang::Unknown,
            "{spelling:?} resolves in the dialect owner but not in the carrier parser"
        );
    }

    // Spellings a carrier can plausibly author that neither side may accept:
    // a preprocessor table keyed by exact bytes has no entry for them, so a
    // block spelled this way has nothing that can compile it.
    for spelling in ["SCSS", "Less", " stylus ", "sass ", "nocss", ""] {
        assert!(
            CssDialect::from_lang(spelling).is_none(),
            "{spelling:?} must fail closed in the dialect owner"
        );
        assert_eq!(
            StyleLang::from_bytes(spelling.as_bytes()),
            StyleLang::Unknown,
            "{spelling:?} must fail closed in the carrier parser too"
        );
    }

    // The carrier's classification is deliberately the wider universe: it
    // names dialects the native grammar set has no member for. "postcss" is
    // the live one — a mainstream Vue spelling with no native parser here, but
    // a real dialect the carrier must still name, because the consumers that
    // read the carrier's classification (the editor's CSS intelligence) serve
    // it and the consumers that read the dialect owner (the rewrite pipeline)
    // must refuse it. Collapsing it into "unrecognised" made both answer the
    // same way and the editor served such a block nothing at all.
    for spelling in [&b"postcss"[..], &b"pcss"[..]] {
        assert!(
            CssDialect::from_lang(std::str::from_utf8(spelling).unwrap()).is_none(),
            "postcss has no native grammar under either spelling"
        );
        assert_eq!(
            StyleLang::from_bytes(spelling),
            StyleLang::PostCss,
            "but the carrier parser names it rather than calling it unrecognised"
        );
    }
}

/// The same stylesheet reports the same inclusions whichever cascade entry
/// point it was routed through.
///
/// A route that parses the sheet but never reads the inclusions its own parse
/// observed publishes "no inclusions" for a sheet that plainly has one. The
/// answer then depends on which entry the caller happened to take, and a
/// consumer that asks "is this block's surface exhaustive?" inherits the
/// disagreement as a route-dependent yes/no.
#[test]
fn every_cascade_entry_reports_the_same_inclusion_inventory() {
    let source = "@import \"theme.css\";\n.a { color: v-bind(tone); }\n";

    let via_authored =
        run_vue_style_cascade(authored(source, CssDialect::Css), "sc1", false, true, false);
    let ir = parse_style_ir(
        CssSource::new(Arc::from(source), 0).expect("test stylesheet fits the parser"),
        CssDialect::Css,
        CssParseMode::Recover,
    )
    .expect("recover-mode parse");
    let via_verified = transform_vue_style(
        VerifiedPlainCss::from_parsed_native_css(&ir).expect("native-CSS provenance"),
        CascadeInput::Authored,
        "component.css",
        "space:component",
        "artifact:component",
        "sc1",
        false,
        true,
        false,
    );

    // The inventory is BOTH the list and the derived "does any of this pull in
    // bytes nothing here parsed" answer. Comparing only the list misses the
    // route dependence on the field consumers actually branch on: an entry can
    // publish the list and leave the derived answer at its default, and every
    // later stage's recorder then sees a populated list and does nothing.
    type Inventory = (Vec<StyleDependencyKind>, bool);
    let inventory = |facts: &verter_compiler::style_planner::VueStyleFacts| -> Inventory {
        (
            facts
                .dependencies
                .iter()
                .map(|dependency| dependency.kind())
                .collect(),
            facts.pulls_in_unparsed_bytes,
        )
    };
    // A finished cascade publishes its inclusion list on the RESULT; the
    // recorder's accumulator is moved into it rather than copied, so the list
    // is read where consumers read it and the derived answer where the
    // single-stage entries return it.
    let cascade_inventory = |outcome: &VueStyleCascadeOutcome| -> Inventory {
        assert!(
            outcome.facts.dependencies.is_empty(),
            "a finished cascade must not retain a second copy of the inclusion list"
        );
        (
            outcome
                .result
                .dependencies()
                .iter()
                .map(|dependency| dependency.kind())
                .collect(),
            outcome.facts.pulls_in_unparsed_bytes,
        )
    };
    let single_stage_inventory = |outcome: &StyleRewriteOutcome| -> Inventory {
        match outcome {
            StyleRewriteOutcome::Unchanged { facts }
            | StyleRewriteOutcome::Rewritten { facts, .. } => inventory(facts),
        }
    };

    let expected: Inventory = (vec![StyleDependencyKind::Import], true);
    assert_eq!(
        cascade_inventory(&via_authored),
        expected,
        "the authored entry's own parse saw the inclusion, and that it brings in foreign bytes"
    );
    assert_eq!(
        cascade_inventory(&via_verified),
        expected,
        "and so did the verified entry's — the route must not change the answer"
    );

    // The remaining entries parse the same bytes and must publish the same
    // inventory. Any one of them reporting "no inclusions", or reporting the
    // inclusion but not that it brings in unparsed bytes, is the
    // route-dependent answer this contract exists to close.
    assert_eq!(
        cascade_inventory(&run_vue_style_authored_only(
            authored(source, CssDialect::Css),
            "sc1",
            false
        )),
        expected,
        "the authored-only entry parsed the sheet, so it reports what its parse saw"
    );
    let modules_only = transform_vue_css_modules(plain(source), "sc1").expect("modules stage");
    let scoped_only = transform_vue_scoped_css(plain(source), "sc1").expect("scoped stage");
    let v_bind_only =
        transform_vue_v_bind(authored(source, CssDialect::Css), "sc1").expect("v-bind stage");
    assert_eq!(
        single_stage_inventory(&modules_only),
        expected,
        "the CSS-Modules entry parsed the sheet, so it reports what its parse saw"
    );
    assert_eq!(
        single_stage_inventory(&scoped_only),
        expected,
        "and so does the scoped-selector entry"
    );
    assert_eq!(
        single_stage_inventory(&v_bind_only),
        expected,
        "and so does the v-bind entry, which is the one the liveness reader takes"
    );

    // A sheet with no inclusions is not the same state as a parse that never
    // recorded one, and both entries must say so rather than leaving the
    // derived answer at whatever a default carries.
    let self_contained = ".a { color: v-bind(tone); }\n";
    assert_eq!(
        cascade_inventory(&run_vue_style_cascade(
            authored(self_contained, CssDialect::Css),
            "sc1",
            false,
            true,
            false
        )),
        (Vec::new(), false),
        "a self-contained sheet declares its whole surface"
    );
}

/// A Sass built-in module is not another stylesheet, and a block that only
/// `@use`s one still declares its whole v-bind() surface.
///
/// The failure this discriminates is the over-broad "any inclusion means the
/// surface is incomplete" rule: a built-in module import appears in a large
/// share of real SCSS blocks, and counting it as foreign bytes switched the
/// exhaustive inventory off for all of them, so every binding used only from
/// those blocks stopped being publishable as used.
#[test]
fn a_sass_builtin_module_does_not_make_the_v_bind_inventory_incomplete() {
    let builtin_only = extract_style_v_bind_usage_for_languages([(
        "@use \"sass:math\";\n.a { width: math.div(v-bind(span), 2); }",
        "scss",
    )]);
    assert!(builtin_only.used.contains("span"));
    assert!(
        builtin_only.complete,
        "a built-in Sass module emits no rules and can hold no v-bind()"
    );

    let real_sheet = extract_style_v_bind_usage_for_languages([(
        "@use \"./theme\";\n.a { color: v-bind(tone); }",
        "scss",
    )]);
    assert!(real_sheet.used.contains("tone"));
    assert!(
        !real_sheet.complete,
        "a use of another sheet still pulls in bytes nothing here parsed"
    );
}

/// A refusal from the scoped-selector stage, after the CSS-Modules stage
/// rewrote the bytes, addresses the REWRITTEN bytes.
///
/// The CSS-Modules stage rewrites on its own — no v-bind() need be involved —
/// and the scoped-selector stage below it then parses that output. Deciding a
/// later stage's coordinate space from "did v-bind() rewrite" therefore answers
/// for the wrong stage: it stamps this refusal as authored, and the one
/// consumer that trusts that label shifts the span by the authored block's
/// start offset. Class hashing changes byte lengths, so the reported range
/// lands past the construct the message names.
#[test]
fn a_refusal_after_a_modules_rewrite_addresses_the_rewritten_space() {
    // The module stage hashes .card, which lengthens every offset after it;
    // the scoped stage then refuses the empty :global() argument that follows.
    let source = ".card { color: red; }\n:global() { color: blue; }\n";
    let outcome =
        run_vue_style_cascade(authored(source, CssDialect::Css), "sc1", true, true, false);

    assert!(
        outcome.facts.rewrites.css_modules,
        "the modules stage must have rewritten the bytes for this to discriminate"
    );
    let [diagnostic] = outcome.result.diagnostics() else {
        panic!("one refusal, got {:?}", outcome.result.diagnostics());
    };
    assert_eq!(
        diagnostic.stage(),
        StyleStage::FrameworkRewritten,
        "the modules stage moved these bytes before the scoping stage saw them"
    );

    // And the span really does address the modules output. The refused
    // construct is an empty argument list, so the span is empty and what it
    // points AT is what identifies it: in the modules output those offsets sit
    // inside the refused ":global()", and in the authored bytes they do not.
    let modules_only =
        run_vue_style_cascade(authored(source, CssDialect::Css), "sc1", true, false, false);
    let span = diagnostic.span().expect("this refusal carries a position");
    let rewritten = modules_only.code();
    assert!(
        rewritten[..span.start as usize].ends_with(":global("),
        "the span addresses the rewritten bytes it was reported against: {:?}",
        &rewritten[..span.start as usize]
    );
    assert!(
        !source[..span.start as usize].ends_with(":global("),
        "and running the same offset through the authored block's arithmetic lands somewhere the refused construct is not"
    );
}

/// External preprocessor output is recorded as the tool's output, not as
/// Verter's own authored bytes.
///
/// The cascade cannot infer this: preprocessed plain CSS and authored plain CSS
/// are the same bytes. Only the entry point that was handed them knows, so it
/// says — and a run that rewrites nothing must still come out named after the
/// tool rather than silently claiming Verter produced it.
#[test]
fn preprocessed_input_keeps_its_producer_through_the_cascade() {
    let css = ".card { color: red; }";
    let ir = parse_style_ir(
        CssSource::new(Arc::from(css), 0).expect("test stylesheet fits the parser"),
        CssDialect::Css,
        CssParseMode::Recover,
    )
    .expect("recover-mode parse");

    let untouched = transform_vue_style(
        VerifiedPlainCss::from_parsed_native_css(&ir).expect("native-CSS provenance"),
        CascadeInput::Preprocessed(sass_producer()),
        "component.css",
        "space:component",
        "artifact:component",
        "sc1",
        false,
        false,
        false,
    );
    assert_eq!(untouched.result.stage(), StyleStage::Preprocessed);
    assert_eq!(untouched.result.producer(), &sass_producer());

    // The authored v-bind() stage still runs on preprocessed bytes: a
    // preprocessor leaves v-bind() in its output for exactly this stage.
    let with_v_bind = ".card { color: v-bind(tone); }";
    let v_bind_ir = parse_style_ir(
        CssSource::new(Arc::from(with_v_bind), 0).expect("test stylesheet fits the parser"),
        CssDialect::Css,
        CssParseMode::Recover,
    )
    .expect("recover-mode parse");
    let rewritten = transform_vue_style(
        VerifiedPlainCss::from_parsed_native_css(&v_bind_ir).expect("native-CSS provenance"),
        CascadeInput::Preprocessed(sass_producer()),
        "component.css",
        "space:component",
        "artifact:component",
        "sc1",
        false,
        false,
        false,
    );
    assert!(
        rewritten.code().contains("var(--sc1-tone)"),
        "preprocessed bytes still get their v-bind() lowered: {}",
        rewritten.code()
    );
    assert_eq!(
        rewritten.result.stage(),
        StyleStage::FrameworkRewritten,
        "bytes this cascade rewrote are its own output whatever produced the input"
    );
}

/// A block that pulls in another stylesheet cannot claim an exhaustive
/// `v-bind()` inventory: the included sheet may reference a binding nothing
/// here parsed, and publishing it as unused is the failure this guards.
#[test]
fn an_imported_stylesheet_makes_the_v_bind_inventory_incomplete() {
    let with_import = extract_style_v_bind_usage_for_languages([(
        "@import \"theme.css\";\n.a { color: v-bind(tone); }",
        "css",
    )]);
    assert!(with_import.used.contains("tone"));
    assert!(
        !with_import.complete,
        "an unresolved inclusion leaves bindings this parse never saw"
    );

    let without_import =
        extract_style_v_bind_usage_for_languages([(".a { color: v-bind(tone); }", "css")]);
    assert!(
        without_import.complete,
        "a self-contained block still reports an exhaustive inventory"
    );
}

/// A block whose parse had to skip input never publishes an exhaustive
/// `v-bind()` inventory.
///
/// The liveness consumer publishes "this binding is unused" from the absence
/// of a name in this inventory, so an inventory built from a parse that
/// skipped part of the sheet is unsound in exactly one direction. A recovery
/// window can swallow an `@import` whole — the inclusion never reaches the
/// at-rule frame, so the recorded inclusion list comes back EMPTY for a block
/// that pulls in a stylesheet, and a binding used only from that sheet gets
/// reported as unused.
#[test]
fn a_style_block_parsed_through_recovery_never_claims_a_complete_v_bind_surface() {
    // No inclusion anywhere in these bytes, and the `v-bind()` sits in a rule
    // the parse read cleanly — so the reported usage proves the extractor took
    // its `Ok` arm, and the only thing left that can withhold completeness is
    // the parse's own record that it discarded input. Without both halves the
    // assertion cannot tell "the parse skipped input" apart from "the rewrite
    // refused these bytes outright", which reports incomplete too.
    let recovered = extract_style_v_bind_usage_for_languages([(
        ".a { color: v-bind(tone); }
.b { content: \"unterminated
",
        "css",
    )]);
    assert!(
        recovered.used.contains("tone"),
        "the recovery arm still reports the rule it read cleanly: {recovered:?}"
    );
    assert!(
        !recovered.complete,
        "a block whose parse skipped input cannot claim an exhaustive surface: {recovered:?}"
    );

    // The motivating shape: a recovery window can swallow an `@import` whole,
    // so the recorded inclusion list comes back EMPTY for a block that pulls
    // in a stylesheet. Here the window also swallows the `v-bind()`, which is
    // why the discriminating case above keeps the two apart.
    let swallowed_import = extract_style_v_bind_usage_for_languages([(
        ".a { color: v-bind(tone); content: \"unterminated
@import \"theme.css\";
",
        "css",
    )]);
    assert!(
        !swallowed_import.complete,
        "an inclusion inside the skipped range is still an inclusion"
    );

    // Control: the same sheet without the recovery window, and with the same
    // inclusion, is equally incomplete — but for the reason the inclusion list
    // can actually show.
    let included = extract_style_v_bind_usage_for_languages([(
        "@import \"theme.css\";\n.a { color: v-bind(tone); }\n",
        "css",
    )]);
    assert!(!included.complete);

    // And a clean, inclusion-free block still publishes an exhaustive one, so
    // the fail-open above is not a blanket switch-off.
    let clean =
        extract_style_v_bind_usage_for_languages([(".a { color: v-bind(tone); }\n", "css")]);
    assert!(clean.complete && clean.used.contains("tone"), "{clean:?}");
}

/// A cleared output is a refusal, not a rewrite's product.
///
/// A stage that cannot run safely WIPES the output rather than exposing a
/// half-applied rewrite. Deriving "was this rewritten?" from "are there owned
/// bytes?" labelled those zero bytes as framework-rewritten and produced by
/// Verter — a provenance claim about bytes no rewrite ever made, on a result
/// whose whole content is that nothing was produced. Emptiness alone cannot
/// tell the two apart, because an authored `<style></style>` is empty too.
#[test]
fn a_refusal_that_cleared_the_output_claims_no_producer_for_it() {
    for (source, dialect, module, scoped) in [
        // The modules stage refuses an untrusted rewrite target and clears.
        (
            ".good { color: v-bind(c); } .bad { color red; }",
            CssDialect::Css,
            true,
            true,
        ),
        // The scoped-selector stage refuses non-plain-CSS input and clears.
        (".a { color: red; }", CssDialect::Scss, false, true),
    ] {
        let outcome = run_vue_style_cascade(authored(source, dialect), "sc1", module, scoped, true);
        assert!(
            !outcome.stage_failures.is_empty(),
            "sanity: a stage must have hard-failed for {source:?}"
        );
        assert_eq!(outcome.code(), "", "sanity: the output was cleared");
        assert!(
            outcome.result.is_refused(),
            "the wiped output must read as a refusal, not as produced bytes"
        );
        assert_eq!(
            outcome.result.stage(),
            StyleStage::Authored,
            "and must not claim the framework-rewritten space it never reached"
        );
    }

    // Control: a stage that really did rewrite still says so.
    let rewritten = run_vue_style_cascade(
        authored(".a { color: v-bind(tone); }", CssDialect::Css),
        "sc1",
        false,
        true,
        true,
    );
    assert!(rewritten.stage_failures.is_empty());
    assert!(!rewritten.result.is_refused());
    assert_eq!(rewritten.result.stage(), StyleStage::FrameworkRewritten);
    assert_eq!(rewritten.result.producer(), &StyleProducer::Verter);
}
