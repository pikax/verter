//! The Vue structural-conformance DISCRIMINATOR guard — the positive proof
//! that the comparator discriminates cosmetic from behavioral differences.
//!
//! Every recipe below is a REVERSIBLE in-memory mutation on a committed
//! golden (plant → require the expected verdict → restore = the retained
//! original string → unplanted control stays GREEN), per the mutation-recipe
//! rule in `.claude/skills/testing/SKILL.md` §1a. Each mutation is asserted
//! INDEPENDENTLY against the pristine golden, and each plant is proven to
//! have applied (`assert_ne` / occurrence-count checks) before its verdict is
//! trusted.
//!
//! - COSMETIC mutations (waived dims: formatting/line numbers, redundant
//!   parens, ordinary comments, private-binding renames incl. helper
//!   aliases) must PASS.
//! - BEHAVIORAL mutations (helper family, patch flags, block topology,
//!   exported/source-authored/member names, effect/setter topology, template
//!   payload, event delegation routing, import source, diagnostics order,
//!   semantic comments) must FAIL — each on its own detection axis so one
//!   axis cannot mask another.
//!
//! Source maps are NOT a conformance dimension (a source map maps its own
//! compiler's output, and generated positions/line numbers are cosmetic), so
//! there are no map recipes here; Verter's source-map correctness is covered
//! by the separate position-encoding tests.
//!
//! Fixtures (committed goldens, never modified on disk):
//! - `vdom/v-for/array` — block/fragment/flags/key topology.
//! - `vdom/elements-text/static-element` — template-only `export function render`.
//! - `vapor/v-on/inline` — delegation, invoker, effect, template.
//! - `vapor/v-bind/static-dynamic` — two ordered prop setters in one effect.
//! - `vdom/script-setup/props-type-withdefaults` — `/* @__PURE__ */` semantic comment.

use verter_vue_conformance::compare::{compare_modules, DiagnosticRow, DiffDim, ModuleInput};

use crate::common::{authored, compare_code, golden_code, plant, plant_all};

const VDOM: &str = "vdom";
const VAPOR: &str = "vapor";

fn pass(name: &str, verter: &str, golden: &str, authored_set: &std::collections::BTreeSet<String>) {
    let comparison = compare_code(verter, golden, authored_set);
    assert!(
        comparison.passed(),
        "cosmetic mutation `{name}` must PASS, got {} reasons: {:?}",
        comparison.total,
        comparison
            .reasons
            .iter()
            .map(|r| r.summary())
            .collect::<Vec<_>>()
    );
}

fn fail(
    name: &str,
    verter: &str,
    golden: &str,
    authored_set: &std::collections::BTreeSet<String>,
    expected: DiffDim,
) {
    let comparison = compare_code(verter, golden, authored_set);
    assert!(
        !comparison.passed(),
        "behavioral mutation `{name}` must FAIL ({expected:?}), got PASS"
    );
    assert!(
        comparison.reasons.iter().any(|r| r.dim == expected),
        "behavioral mutation `{name}` must fail on {expected:?}, got: {:?}",
        comparison
            .reasons
            .iter()
            .map(|r| r.summary())
            .collect::<Vec<_>>()
    );
}

#[test]
fn vue_structural_conformance_discriminates_cosmetic_from_behavioral_diffs() {
    // ---- Non-vacuity: identical modules compare equal on every fixture. ----
    for (backend, case) in [
        (VDOM, "v-for/array"),
        (VDOM, "elements-text/static-element"),
        (VAPOR, "v-on/inline"),
        (VAPOR, "v-bind/static-dynamic"),
        (VDOM, "script-setup/props-type-withdefaults"),
    ] {
        let golden = golden_code(backend, case);
        let authored = authored(case);
        let comparison = compare_code(&golden, &golden, &authored);
        assert!(
            comparison.passed(),
            "non-vacuity: {backend}/{case} golden vs itself must PASS, got: {:?}",
            comparison
                .reasons
                .iter()
                .map(|r| r.summary())
                .collect::<Vec<_>>()
        );
    }

    // =======================================================================
    // COSMETIC mutations — must PASS.
    // =======================================================================
    {
        let case = "v-for/array";
        let golden = golden_code(VDOM, case);
        let authored = authored(case);

        // Reformat: reindent (2 -> 4 spaces) and drop blank lines — pure
        // whitespace trivia, never ASI-relevant.
        let reformatted = golden.replace("\n  ", "\n    ").replace("\n\n", "\n");
        assert_ne!(reformatted, golden, "reformat recipe failed to apply");
        pass(
            "cosmetic: reformat whitespace",
            &reformatted,
            &golden,
            &authored,
        );

        // LINE-NUMBER shift: prepend blank lines and insert more between
        // statements — generated positions/line numbers are cosmetic and
        // must not move the verdict (ASI is safe: blank lines only extend
        // existing line boundaries).
        let shifted = format!("\n\n\n{golden}").replace("\nimport", "\n\n\nimport");
        assert_ne!(shifted, golden, "line-shift recipe failed to apply");
        pass("cosmetic: line-number shift", &shifted, &golden, &authored);

        // Behavior-preserving parens around a call expression.
        let parens = plant(
            &golden,
            "_toDisplayString(item), 1 /* TEXT */",
            "(_toDisplayString(item)), 1 /* TEXT */",
            "parens",
        );
        pass("cosmetic: redundant parens", &parens, &golden, &authored);

        // Ordinary (non-semantic) comment inserted.
        let commented = plant(
            &golden,
            "const items = ref(",
            "/* conformance note: ordinary comment */\nconst items = ref(",
            "ordinary comment",
        );
        pass("cosmetic: ordinary comment", &commented, &golden, &authored);

        // Alpha-rename ABI-local parameters (`_ctx`, `_cache`).
        let renamed = plant_all(&golden, "_ctx", "_ctxRenamed", "rename _ctx");
        let renamed = plant_all(&renamed, "_cache", "_cacheRenamed", "rename _cache");
        pass(
            "cosmetic: alpha-rename ABI params",
            &renamed,
            &golden,
            &authored,
        );

        // Alpha-rename a helper alias across its import specifier AND call
        // sites (helper family identity — the imported name — is untouched).
        let renamed = plant_all(&golden, "_renderList", "_rL", "rename helper alias");
        pass(
            "cosmetic: alpha-rename helper alias",
            &renamed,
            &golden,
            &authored,
        );
    }
    {
        let case = "v-on/inline";
        let golden = golden_code(VAPOR, case);
        let authored = authored(case);

        // Alpha-rename vapor-private locals (node/text/template bindings).
        let renamed = plant_all(&golden, "n0", "n99", "rename n0");
        let renamed = plant_all(&renamed, "x0", "x99", "rename x0");
        let renamed = plant_all(&renamed, "t0", "t99", "rename t0");
        pass(
            "cosmetic: alpha-rename vapor locals",
            &renamed,
            &golden,
            &authored,
        );
    }

    // =======================================================================
    // BEHAVIORAL mutations — VDOM — must FAIL.
    // =======================================================================
    {
        let case = "v-for/array";
        let golden = golden_code(VDOM, case);
        let authored = authored(case);

        // Helper family swap: createElementBlock -> createElementVNode
        // (imported name only; alias untouched).
        let mutated = plant(
            &golden,
            "createElementBlock as",
            "createElementVNode as",
            "helper family",
        );
        fail(
            "vdom: helper family swap",
            &mutated,
            &golden,
            &authored,
            DiffDim::Import,
        );

        // Patch-flag mutation: 128 (KEYED_FRAGMENT) -> 127.
        let mutated = plant(
            &golden,
            ", 128 /* KEYED_FRAGMENT */",
            ", 127 /* KEYED_FRAGMENT */",
            "patch flag",
        );
        fail(
            "vdom: patch flag",
            &mutated,
            &golden,
            &authored,
            DiffDim::Literal,
        );

        // Drop an openBlock call from a sequence.
        let mutated = plant(&golden, "_openBlock(true), ", "", "drop openBlock");
        fail(
            "vdom: drop openBlock",
            &mutated,
            &golden,
            &authored,
            DiffDim::Structure,
        );

        // Rename a source-authored member property ($setup.items -> $setup.things).
        let mutated = plant(&golden, "$setup.items", "$setup.things", "member property");
        fail(
            "vdom: rename member property",
            &mutated,
            &golden,
            &authored,
            DiffDim::Identifier,
        );

        // Rename a source-authored binding (items -> things, all occurrences).
        let mutated = plant_all(&golden, "items", "things", "rename authored binding");
        fail(
            "vdom: rename source-authored binding",
            &mutated,
            &golden,
            &authored,
            DiffDim::Identifier,
        );

        // Import source change.
        let mutated = plant(&golden, "from \"vue\"", "from \"vue-x\"", "import source");
        fail(
            "vdom: import source",
            &mutated,
            &golden,
            &authored,
            DiffDim::Import,
        );

        // Imported-helper name change.
        let mutated = plant(&golden, "openBlock as", "openBlock2 as", "imported helper");
        fail(
            "vdom: imported helper change",
            &mutated,
            &golden,
            &authored,
            DiffDim::Import,
        );
    }
    {
        // Non-inline topology: the module's PUBLIC surface is the default
        // export — renaming the exported `_sfc_main` binding is contract
        // (the `render` fn itself is a private binding, renamed below as a
        // cosmetic recipe).
        let case = "elements-text/static-element";
        let golden = golden_code(VDOM, case);
        let authored = authored(case);

        let mutated = plant_all(
            &golden,
            "_sfc_main",
            "_sfc_other",
            "rename exported default",
        );
        fail(
            "vdom: rename exported default binding",
            &mutated,
            &golden,
            &authored,
            DiffDim::Identifier,
        );

        // Cosmetic: consistently renaming the private `render` binding AND
        // its attach reference passes (alpha equivalence).
        let renamed = plant(
            &golden,
            "function render(",
            "function renderVdom(",
            "rename render decl",
        );
        let renamed = plant(
            &renamed,
            "_sfc_main.render = render",
            "_sfc_main.render = renderVdom",
            "rename render attach ref",
        );
        pass(
            "cosmetic: alpha-rename render binding",
            &renamed,
            &golden,
            &authored,
        );
    }

    // =======================================================================
    // BEHAVIORAL mutations — Vapor — must FAIL.
    // =======================================================================
    {
        let case = "v-on/inline";
        let golden = golden_code(VAPOR, case);
        let authored = authored(case);

        // Helper family swap: setText -> setHtml (imported name only).
        let mutated = plant(&golden, "setText as", "setHtml as", "setText family");
        fail(
            "vapor: setText->setHtml",
            &mutated,
            &golden,
            &authored,
            DiffDim::Import,
        );

        // Move _setText OUT of the _renderEffect closure.
        let mutated = plant(
            &golden,
            "  _renderEffect(() => _setText(x0, \"Count: \" + _toDisplayString(_ctx.count)))",
            "  _setText(x0, \"Count: \" + _toDisplayString(_ctx.count))\n  _renderEffect(() => {})",
            "move setter out of effect",
        );
        fail(
            "vapor: setter moved out of effect",
            &mutated,
            &golden,
            &authored,
            DiffDim::Structure,
        );

        // Change the setter's target binding (x0 -> n0: another private
        // binding — alpha keys differ).
        let mutated = plant(&golden, "_setText(x0,", "_setText(n0,", "retarget setter");
        fail(
            "vapor: retarget setter binding",
            &mutated,
            &golden,
            &authored,
            DiffDim::Identifier,
        );

        // Mutate the _template static payload flag.
        let mutated = plant(
            &golden,
            "_template(\"<button> \", 1)",
            "_template(\"<button> \", 2)",
            "template flag",
        );
        fail(
            "vapor: template flag",
            &mutated,
            &golden,
            &authored,
            DiffDim::Literal,
        );

        // Mutate the _template static payload bytes.
        let mutated = plant(
            &golden,
            "_template(\"<button> \", 1)",
            "_template(\"<button></button>\", 1)",
            "template payload",
        );
        fail(
            "vapor: template payload",
            &mutated,
            &golden,
            &authored,
            DiffDim::Literal,
        );

        // Remove event delegation (delegated setup deleted).
        let mutated = plant(
            &golden,
            "_delegateEvents(\"click\")\n",
            "",
            "drop delegation",
        );
        fail(
            "vapor: drop event delegation",
            &mutated,
            &golden,
            &authored,
            DiffDim::Structure,
        );

        // Reroute the delegated handler registration ($evtclick ABI name).
        let mutated = plant(&golden, "n0.$evtclick", "n0.$evtfoo", "$evtclick ABI");
        fail(
            "vapor: delegated handler ABI name",
            &mutated,
            &golden,
            &authored,
            DiffDim::Identifier,
        );
    }
    {
        // Reorder the two prop setters inside one render effect.
        let case = "v-bind/static-dynamic";
        let golden = golden_code(VAPOR, case);
        let authored = authored(case);
        let mutated = plant(
            &golden,
            "    _setProp(n0, \"title\", _ctx.title)\n    _setProp(n0, \"disabled\", _ctx.disabled)",
            "    _setProp(n0, \"disabled\", _ctx.disabled)\n    _setProp(n0, \"title\", _ctx.title)",
            "reorder setters",
        );
        fail(
            "vapor: reorder effect setters",
            &mutated,
            &golden,
            &authored,
            DiffDim::Literal,
        );
    }

    // =======================================================================
    // BEHAVIORAL mutations — common — must FAIL.
    // =======================================================================
    {
        // Semantic comment (PURE annotation) drop + move.
        let case = "script-setup/props-type-withdefaults";
        let golden = golden_code(VDOM, case);
        let authored = authored(case);

        let dropped = plant(
            &golden,
            "const _sfc_main = /* @__PURE__ */ _defineComponent({",
            "const _sfc_main = _defineComponent({",
            "drop PURE comment",
        );
        fail(
            "common: drop semantic comment",
            &dropped,
            &golden,
            &authored,
            DiffDim::Comment,
        );

        let moved = plant(
            &golden,
            "const _sfc_main = /* @__PURE__ */ _defineComponent({",
            "const _sfc_main = _defineComponent({",
            "move PURE comment (strip)",
        );
        let moved = plant(
            &moved,
            "import { defineComponent as _defineComponent } from \"vue\";",
            "import { defineComponent as _defineComponent } from \"vue\";\n/* @__PURE__ */",
            "move PURE comment (replant at top)",
        );
        fail(
            "common: move semantic comment",
            &moved,
            &golden,
            &authored,
            DiffDim::Comment,
        );
    }
    {
        // Diagnostics ORDER is in-contract.
        let case = "v-for/array";
        let golden = golden_code(VDOM, case);
        let authored = authored(case);
        let row = |message: &str| DiagnosticRow {
            kind: "error".to_string(),
            code: Some("X_TEST".to_string()),
            message: message.to_string(),
        };
        let input = |diagnostics: Vec<DiagnosticRow>| ModuleInput {
            code: golden.clone(),
            diagnostics,
        };
        let ordered = compare_modules(
            &input(vec![row("first"), row("second")]),
            &input(vec![row("first"), row("second")]),
            &authored,
            64,
        )
        .expect("compare");
        assert!(ordered.passed(), "identical diagnostic sequences must PASS");
        let reordered = compare_modules(
            &input(vec![row("second"), row("first")]),
            &input(vec![row("first"), row("second")]),
            &authored,
            64,
        )
        .expect("compare");
        assert!(
            !reordered.passed()
                && reordered
                    .reasons
                    .iter()
                    .any(|r| r.dim == DiffDim::Diagnostics),
            "reordered diagnostics must FAIL on the diagnostics dim: {:?}",
            reordered
                .reasons
                .iter()
                .map(|r| r.summary())
                .collect::<Vec<_>>()
        );
    }
}
