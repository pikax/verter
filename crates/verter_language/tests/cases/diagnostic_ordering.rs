use std::cmp::Ordering;

use verter_identity::identity::{CompatibilityDomainId, CompatibilityEpoch};
use verter_language::{
    compare_language_diagnostics, parse_key_for, sort_language_diagnostics, syntax_profile_id_for,
    DiagnosticArg, FileLanguage, LanguageDiagnostic, LanguageDiagnosticSeverity, ParseOptions,
};
use verter_span::Span;

fn source_id(source: &str) -> verter_identity::identity::ParseKey {
    let language = FileLanguage::svelte();
    let profile = syntax_profile_id_for(&language, &ParseOptions::default()).unwrap();
    parse_key_for(
        source,
        &language,
        CompatibilityDomainId("verter.test.syntax"),
        CompatibilityEpoch(0),
        &profile,
    )
    .unwrap()
}

fn diagnostic(message: &str, argument: &str) -> LanguageDiagnostic {
    LanguageDiagnostic {
        span: Span::new(3, 5),
        severity: LanguageDiagnosticSeverity::Error,
        code: "same-code",
        arguments: vec![DiagnosticArg::Text(argument.to_owned())],
        message: message.to_owned(),
        blocks_compile: true,
    }
}

#[test]
fn normative_order_uses_source_span_severity_code_and_typed_arguments() {
    let first_source = source_id("first");
    let second_source = source_id("second");
    let low_argument = diagnostic("message-z", "a");
    let high_argument = diagnostic("message-a", "z");

    assert_eq!(
        compare_language_diagnostics(&first_source, &low_argument, &first_source, &high_argument,),
        Ordering::Less,
        "typed arguments, not display text, must break an otherwise identical key"
    );
    assert_ne!(
        compare_language_diagnostics(&first_source, &low_argument, &second_source, &low_argument,),
        Ordering::Equal,
        "the canonical source identity is the first key dimension"
    );
}

#[test]
fn every_normative_key_dimension_discriminates_independently() {
    let source = source_id("same");
    let other_source = source_id("other");
    let base = diagnostic("display", "same");

    let mut later_start = base.clone();
    later_start.span = Span::new(4, 5);
    let mut later_end = base.clone();
    later_end.span = Span::new(3, 6);
    let mut warning = base.clone();
    warning.severity = LanguageDiagnosticSeverity::Warning;
    let mut later_code = base.clone();
    later_code.code = "z-code";
    let mut later_argument = base.clone();
    later_argument.arguments = vec![DiagnosticArg::Text("z".into())];

    assert_eq!(
        [
            compare_language_diagnostics(&source, &base, &other_source, &base) != Ordering::Equal,
            compare_language_diagnostics(&source, &base, &source, &later_start) == Ordering::Less,
            compare_language_diagnostics(&source, &base, &source, &later_end) == Ordering::Less,
            compare_language_diagnostics(&source, &base, &source, &warning) == Ordering::Less,
            compare_language_diagnostics(&source, &base, &source, &later_code) == Ordering::Less,
            compare_language_diagnostics(&source, &base, &source, &later_argument)
                == Ordering::Less,
        ],
        [true; 6],
        "source, start, end, severity, code, and typed arguments must each discriminate"
    );
}

#[test]
fn display_message_is_not_an_ordering_dimension() {
    let source = source_id("same");
    let left = diagnostic("aaa", "same");
    let right = diagnostic("zzz", "same");

    assert_eq!(
        compare_language_diagnostics(&source, &left, &source, &right),
        Ordering::Equal
    );
    assert_eq!(
        compare_language_diagnostics(&source, &right, &source, &left),
        Ordering::Equal
    );
}

#[test]
fn normative_sort_is_stable_across_input_permutations_when_keys_are_distinct() {
    let source = source_id("same");
    let a = diagnostic("later display text", "a");
    let b = diagnostic("earlier display text", "b");
    let mut forward = vec![a.clone(), b.clone()];
    let mut reverse = vec![b, a];

    sort_language_diagnostics(&source, &mut forward);
    sort_language_diagnostics(&source, &mut reverse);

    assert_eq!(forward, reverse);
    assert_eq!(forward[0].arguments, [DiagnosticArg::Text("a".into())]);
}
