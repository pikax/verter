//! Prerequisite for Svelte-side lossless-span convergence: `:deep()` /
//! `:global()` / `:slotted()` must classify exactly like the already-shared
//! `is()`/`where()` selector-list pseudos — carrier-blind, i.e. the SAME
//! `SyntaxKind::PseudoSelectorList` produced regardless of which `CssDialect`
//! flag is active. A future Svelte-specific grammar convergence must build on
//! this shared classification rather than introduce a distinct carrier-named
//! kind.
//!
//! Two independent properties are asserted, since cross-dialect equality
//! alone cannot distinguish a real neutral kind from a hypothetical
//! carrier-named variant emitted identically everywhere:
//! (a) parsing each pseudo under all 5 `CssDialect` flags produces a
//!     byte-identical event stream (fingerprint AND `StartNode` kind
//!     sequence) — the tree shape does not vary by dialect;
//! (b) the pseudo-function's own node is EXACTLY `SyntaxKind::PseudoSelectorList`
//!     — the same closed variant already produced for `is()`/`where()`, never
//!     a distinct/new kind.

use std::sync::Arc;

use verter_css_syntax::{
    parse_with_sink, CssDialect, CssEntryPoint, CssParseMode, CssSource, CssStructureTooLarge,
    ParseEvent, ParseEventSink, SyntaxKind,
};

const DIALECTS: [CssDialect; 5] = [
    CssDialect::Css,
    CssDialect::Scss,
    CssDialect::Sass,
    CssDialect::Less,
    CssDialect::Stylus,
];

const PSEUDOS: [&str; 3] = ["deep", "global", "slotted"];

#[derive(Default)]
struct KindRecordingSink {
    fingerprint: u64,
    start_kinds: Vec<SyntaxKind>,
}

impl ParseEventSink for KindRecordingSink {
    fn event(&mut self, event: ParseEvent) -> Result<(), CssStructureTooLarge> {
        self.fingerprint = event.fold_fingerprint(self.fingerprint);
        if let ParseEvent::StartNode { kind, .. } = event {
            self.start_kinds.push(kind);
        }
        Ok(())
    }
}

fn parse_selector(dialect: CssDialect, text: &str) -> KindRecordingSink {
    let source = CssSource::new(Arc::from(text), 0).expect("valid source");
    let mut sink = KindRecordingSink::default();
    parse_with_sink(
        &source,
        dialect,
        CssEntryPoint::SelectorList,
        CssParseMode::Strict,
        &mut sink,
    )
    .unwrap_or_else(|err| panic!("{dialect:?} failed to parse `{text}`: {err:?}"));
    sink
}

#[test]
fn deep_global_slotted_classify_as_pseudo_selector_list_across_all_dialects() {
    for pseudo in PSEUDOS {
        let text = format!(".a:{pseudo}(.b)");
        let mut baseline: Option<(u64, Vec<SyntaxKind>)> = None;

        for dialect in DIALECTS {
            let sink = parse_selector(dialect, &text);

            // Property (b): exactly `PseudoSelectorList` — never a distinct
            // carrier-named kind, and never demoted to `UnknownPseudoFunction`
            // or misclassified as `NthSelector`.
            assert!(
                sink.start_kinds.contains(&SyntaxKind::PseudoSelectorList),
                "{dialect:?} `{text}` did not produce a PseudoSelectorList node: {:?}",
                sink.start_kinds
            );
            assert!(
                !sink.start_kinds.iter().any(|kind| matches!(
                    kind,
                    SyntaxKind::UnknownPseudoFunction | SyntaxKind::NthSelector
                )),
                "{dialect:?} `{text}` was classified as a different functional-pseudo kind: {:?}",
                sink.start_kinds
            );

            // Property (a): byte-identical tree shape across dialects — same
            // input text must fold to the same event fingerprint AND the same
            // `StartNode` kind sequence, whichever dialect flag is active.
            match &baseline {
                None => baseline = Some((sink.fingerprint, sink.start_kinds.clone())),
                Some((fingerprint, start_kinds)) => {
                    assert_eq!(
                        sink.fingerprint, *fingerprint,
                        "{dialect:?} produced a different event fingerprint for `{text}` \
                         than the baseline dialect — pseudo-list classification is not \
                         carrier-blind",
                    );
                    assert_eq!(
                        &sink.start_kinds, start_kinds,
                        "{dialect:?} produced a different node-kind sequence for `{text}`",
                    );
                }
            }
        }
    }
}

/// Discrimination companion: `is()`/`where()` already classify as
/// `PseudoSelectorList` (the established, ratified-neutral baseline). If this
/// failed, the test harness itself would be broken (the assertion helper
/// mis-detecting `PseudoSelectorList`), which would make the primary test
/// above pass vacuously.
#[test]
fn is_and_where_still_classify_as_pseudo_selector_list() {
    for pseudo in ["is", "where"] {
        let text = format!(".a:{pseudo}(.b)");
        let sink = parse_selector(CssDialect::Css, &text);
        assert!(
            sink.start_kinds.contains(&SyntaxKind::PseudoSelectorList),
            "`{text}` did not produce a PseudoSelectorList node: {:?}",
            sink.start_kinds
        );
    }
}
