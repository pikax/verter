//! The structural specifier inventory must find every REAL specifier and no
//! text that merely looks like one.

use super::*;
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

fn texts(source: &str) -> Vec<String> {
    collect_module_specifier_spans(source)
        .expect("source must parse")
        .into_iter()
        .map(|span| span.text)
        .collect()
}

#[test]
fn finds_every_module_specifier_position_a_carrier_uses() {
    let source = "import Child from './Child.vue'\n\
         import type { P } from \"./types\"\n\
         export { x } from './re-export'\n\
         export * from './star'\n\
         const lazy = () => import('./dynamic.vue')\n\
         type Widened = typeof import(\"./TypePosition.vue\")[\"default\"]\n\
         import legacy = require('./equals')\n\
         export type { Widened }\n\
         export default lazy\n";
    assert_eq!(
        texts(source),
        vec![
            "./Child.vue",
            "./types",
            "./re-export",
            "./star",
            "./dynamic.vue",
            "./TypePosition.vue",
            "./equals",
        ],
        "every specifier position a generated carrier can use must be found, \
         in source order — the TYPE position is the one the fallthrough \
         widening emits"
    );
}

/// The defect this inventory exists to make impossible.
///
/// An Options-API stub passes the user's authored script body through
/// verbatim, so authored text containing `import(` or `from ` reaches the
/// rewriter. A byte scan rewrites it; a structural walk cannot see it.
#[test]
fn never_reports_specifier_lookalikes_in_authored_text() {
    let source = "const marker = 'import(\"@/Child.vue\")' as const\n\
         const template = `from \"@/Template.vue\"`\n\
         // import Commented from \"@/Commented.vue\"\n\
         /* from \"@/Block.vue\" */\n\
         const re = /import\\(\"@\\/Regex.vue\"\\)/\n\
         const concatenated = 'imp' + 'ort(\"@/Split.vue\")'\n\
         export default marker\n";
    assert!(
        texts(source).is_empty(),
        "a string literal, template literal, comment, or regex that CONTAINS \
         specifier syntax is not a specifier; reporting one lets a rewriter \
         silently change the user's own literal type: {:?}",
        texts(source)
    );
}

/// Both halves at once, which is what the Options-API stub actually looks
/// like: a real import to rewrite, and a lookalike that must survive.
#[test]
fn separates_a_real_specifier_from_a_lookalike_in_the_same_source() {
    let source = "import Child from '@/Child.vue'\n\
         const marker = 'import(\"@/Child.vue\")' as const\n\
         export default { components: { Child }, marker }\n";
    let spans = collect_module_specifier_spans(source).expect("source must parse");
    assert_eq!(
        spans.len(),
        1,
        "exactly the import declaration's source, never the identical text \
         inside the authored literal: {spans:?}"
    );
    assert_eq!(spans[0].text, "@/Child.vue");
    // The recorded range is the WHOLE literal, quotes included, and the quote
    // character travels with it so a replacement keeps the carrier's style.
    assert_eq!(&source[spans[0].start..spans[0].end], "'@/Child.vue'");
    assert_eq!(spans[0].quote, '\'');
}

/// A source that does not parse CLEANLY reports NO ANSWER, distinctly from
/// "no specifiers".
///
/// Conflating the two is how a rewriter silently leaves every specifier in a
/// broken carrier alone while believing it did its job. `None` obliges the
/// caller to fail closed and say so; `Some(vec![])` would let it continue.
///
/// The RECOVERABLE half is the load-bearing one. `panicked` alone is not the
/// test: OXC recovers from plenty of malformed sources with `panicked == false`
/// and a non-empty error list, and a recovering parser may synthesise nodes at
/// positions the author never wrote — splicing on those corrupts the carrier.
#[test]
fn a_source_that_does_not_parse_cleanly_reports_no_answer() {
    // HARD failure: the parser gives up.
    let panicking = "import Child from './Child.vue'\n\
         const swallowed = (\n";
    assert!(
        collect_module_specifier_spans(panicking).is_none(),
        "a carrier the parser cannot handle must be distinguishable from one \
         that genuinely contains no specifier"
    );

    // RECOVERABLE failure: the parser does NOT give up, and reports an error.
    // A duplicate accessibility modifier is one such source; the assertions
    // below pin BOTH halves so this cannot silently become a hard failure and
    // stop testing recovery.
    let recoverable = "import Child from './Child.vue'\n\
         class C { public public m() {} }\n";
    {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, recoverable, SourceType::tsx()).parse();
        assert!(
            !parsed.panicked,
            "precondition: this source must RECOVER, or it tests the hard-failure \
             path over again"
        );
        assert!(
            !parsed.errors.is_empty(),
            "precondition: and it must report an error, or there is nothing to \
             discriminate"
        );
    }
    assert!(
        collect_module_specifier_spans(recoverable).is_none(),
        "a RECOVERED parse with diagnostics must also report no answer — \
         checking only `panicked` lets a malformed carrier be rewritten on \
         spans a recovering parser invented"
    );

    let empty_but_valid = "export const x = 1\n";
    assert_eq!(
        collect_module_specifier_spans(empty_but_valid),
        Some(Vec::new()),
        "a VALID source with no specifiers is an answer, and it is the empty one"
    );
}

/// A no-substitution template literal is an ordinary static specifier; one WITH
/// substitutions is not statically knowable and is left alone.
#[test]
fn reports_a_no_substitution_template_specifier_and_skips_an_interpolated_one() {
    assert_eq!(
        texts("const lazy = () => import(`./Child.vue`)\n"),
        vec!["./Child.vue"],
        "a backtick-written static specifier still needs rewriting when the \
         carrier moves directories"
    );

    let spans = collect_module_specifier_spans("const lazy = () => import(`./Child.vue`)\n")
        .expect("source must parse");
    // The recorded range covers the WHOLE template literal, backticks
    // included, and the replacement is emitted as a plain string literal.
    let rendered = quote_module_specifier(&spans[0].text, spans[0].quote);
    assert_eq!(rendered, "\"./Child.vue\"");

    assert!(
        texts("const name = 'Child'\nconst lazy = () => import(`./${name}.vue`)\n").is_empty(),
        "an INTERPOLATED specifier depends on runtime values and must not be \
         guessed at"
    );
}

/// Re-quoting escapes every character a JS/TS string literal cannot carry raw.
///
/// A raw line terminator does not merely produce a wrong specifier — it
/// produces an UNTERMINATED literal and a cascade of syntax errors through the
/// rest of the generated carrier.
#[test]
fn quoting_escapes_line_terminators_and_round_trips() {
    for specifier in [
        "./a\nb.vue",
        "./a\rb.vue",
        "./a\u{2028}b.vue",
        "./a\u{2029}b.vue",
        "./a\\b.vue",
        "./a'b.vue",
        "./a\"b.vue",
    ] {
        for quote in ['\'', '"'] {
            let literal = quote_module_specifier(specifier, quote);
            assert!(
                !literal[1..literal.len() - 1].contains(['\n', '\r', '\u{2028}', '\u{2029}']),
                "no raw line terminator may survive into the literal: {literal:?}"
            );
            let source = format!("import X from {literal}\n");
            assert_eq!(
                texts(&source),
                vec![specifier.to_string()],
                "and the escaped literal must still denote exactly {specifier:?}"
            );
        }
    }
}

/// An ESCAPED specifier is reported by its decoded value and stays rewritable.
///
/// Reporting the raw bytes instead would make `'..\\\\x'` — an ordinary
/// Windows-style relative specifier — either corrupt on rewrite or silently
/// skipped, so the carrier would keep a specifier that resolves against the
/// wrong directory.
#[test]
fn reports_an_escaped_specifier_by_its_decoded_value() {
    assert_eq!(
        texts("import Child from './Ch\\u0069ld.vue'\n"),
        vec!["./Child.vue"],
        "a unicode escape denotes an ordinary specifier"
    );
    assert_eq!(
        texts("import type { Foo } from '..\\\\x'\n"),
        vec!["..\\x"],
        "a Windows-style backslash specifier decodes to the path it denotes"
    );
}

/// Re-quoting round-trips: whatever a caller substitutes comes back out as a
/// literal denoting exactly that specifier.
#[test]
fn quoting_round_trips_through_the_decoded_value() {
    for (specifier, quote) in [
        ("./plain.vue", '\''),
        ("..\\x", '"'),
        ("C:\\proj\\Child.vue", '\''),
        ("has'quote", '\''),
    ] {
        let source = format!(
            "import X from {}\n",
            quote_module_specifier(specifier, quote)
        );
        assert_eq!(
            texts(&source),
            vec![specifier.to_string()],
            "re-quoting {specifier:?} with {quote:?} must denote the same specifier"
        );
    }
}
