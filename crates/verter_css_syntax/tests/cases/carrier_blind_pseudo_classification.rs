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
//! (b) the pseudo-function's OWN node — the node whose `StartNode` offset is
//!     the pseudo's own `:` byte — is EXACTLY `SyntaxKind::PseudoSelectorList`,
//!     and no OTHER node opens at that offset.
//!
//! Property (b) is positional, not presence-based. A mere
//! `start_kinds.contains(&PseudoSelectorList)` never associates the kind with
//! the tested pseudo's own node: a carrier-named variant emitted for `:deep`
//! itself, sitting beside an unrelated `PseudoSelectorList` marker opened
//! anywhere else in the tree, satisfies `contains` while violating the
//! property. The offset join, plus whole-sequence equality against the
//! ratified-neutral `is()`/`where()` baseline (which replaces an exclusion
//! list naming two known-wrong kinds — that list can never anticipate a kind
//! introduced later), pins the association structurally.

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

/// The carrier-shaped pseudos under test.
const PSEUDOS: [&str; 3] = ["deep", "global", "slotted"];

/// The ratified carrier-NEUTRAL baseline this charter requires the three
/// above to be indistinguishable from.
const NEUTRAL_BASELINE: [&str; 2] = ["is", "where"];

#[derive(Default)]
struct KindRecordingSink {
    fingerprint: u64,
    /// Every `StartNode`, as `(kind, start-offset)` — the offset is what
    /// associates a kind with the node it actually belongs to.
    starts: Vec<(SyntaxKind, u32)>,
}

impl KindRecordingSink {
    fn kinds(&self) -> Vec<SyntaxKind> {
        self.starts.iter().map(|(kind, _)| *kind).collect()
    }

    /// Every node kind opening exactly at `offset`. For a functional pseudo
    /// that is the pseudo's own node and nothing else.
    fn kinds_starting_at(&self, offset: u32) -> Vec<SyntaxKind> {
        self.starts
            .iter()
            .filter(|(_, start)| *start == offset)
            .map(|(kind, _)| *kind)
            .collect()
    }
}

impl ParseEventSink for KindRecordingSink {
    fn event(&mut self, event: ParseEvent) -> Result<(), CssStructureTooLarge> {
        self.fingerprint = event.fold_fingerprint(self.fingerprint);
        if let ParseEvent::StartNode { kind, start, .. } = event {
            self.starts.push((kind, start));
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

/// `.a:<name>(.b)` — the pseudo's own `:` is at byte 2 in every spelling, so
/// the offset that identifies the pseudo's node is name-independent.
fn fixture(name: &str) -> (String, u32) {
    (format!(".a:{name}(.b)"), 2)
}

#[test]
fn deep_global_slotted_classify_as_pseudo_selector_list_across_all_dialects() {
    // The ratified-neutral reference sequence, taken from `is()` under the
    // plain-CSS dialect. Every carrier-shaped pseudo must produce this exact
    // node-kind sequence — an added, removed, or substituted node anywhere in
    // the tree is a divergence, whether or not it is named after a carrier.
    let (is_text, is_pseudo_offset) = fixture("is");
    let neutral_reference = parse_selector(CssDialect::Css, &is_text).kinds();
    assert_eq!(
        parse_selector(CssDialect::Css, &is_text).kinds_starting_at(is_pseudo_offset),
        vec![SyntaxKind::PseudoSelectorList],
        "harness sanity: the ratified-neutral `is()` baseline must itself open \
         exactly one node — a PseudoSelectorList — at its own `:`"
    );

    for pseudo in PSEUDOS {
        let (text, pseudo_offset) = fixture(pseudo);
        let mut baseline: Option<(u64, Vec<(SyntaxKind, u32)>)> = None;

        for dialect in DIALECTS {
            let sink = parse_selector(dialect, &text);

            // Property (b), positional: the node opening at the pseudo's OWN
            // `:` byte is exactly `PseudoSelectorList`, and it is the ONLY
            // node opening there. A carrier-named kind minted for `:deep`
            // fails here even if an unrelated `PseudoSelectorList` is opened
            // elsewhere in the same tree, which is precisely what a
            // `contains(&PseudoSelectorList)` presence check cannot see.
            assert_eq!(
                sink.kinds_starting_at(pseudo_offset),
                vec![SyntaxKind::PseudoSelectorList],
                "{dialect:?} `{text}`: the node at the pseudo's own `:` (offset \
                 {pseudo_offset}) must be exactly PseudoSelectorList and nothing \
                 else; full start list: {:?}",
                sink.starts,
            );

            // Property (b), whole-tree: the kind SEQUENCE is identical to the
            // ratified-neutral `is()` reference. This replaces an exclusion
            // list naming two known-wrong kinds (`UnknownPseudoFunction` /
            // `NthSelector`), which by construction cannot exclude a kind
            // introduced after the list was written.
            assert_eq!(
                sink.kinds(),
                neutral_reference,
                "{dialect:?} `{text}`: node-kind sequence diverges from the \
                 carrier-neutral `is()` reference",
            );

            // Property (a): byte-identical tree shape across dialects — same
            // input text must fold to the same event fingerprint AND the same
            // `(kind, offset)` start sequence, whichever dialect flag is active.
            match &baseline {
                None => baseline = Some((sink.fingerprint, sink.starts.clone())),
                Some((fingerprint, starts)) => {
                    assert_eq!(
                        sink.fingerprint, *fingerprint,
                        "{dialect:?} produced a different event fingerprint for `{text}` \
                         than the baseline dialect — pseudo-list classification is not \
                         carrier-blind",
                    );
                    assert_eq!(
                        &sink.starts, starts,
                        "{dialect:?} produced a different node-kind/offset sequence for `{text}`",
                    );
                }
            }
        }
    }
}

/// Discrimination companion: the ratified-neutral `is()`/`where()` pseudos
/// classify positionally the same way. If this failed, the offset join above
/// would be measuring a broken harness rather than the parser, which would
/// make the primary test pass (or fail) vacuously.
#[test]
fn is_and_where_still_classify_as_pseudo_selector_list() {
    for pseudo in NEUTRAL_BASELINE {
        let (text, pseudo_offset) = fixture(pseudo);
        let sink = parse_selector(CssDialect::Css, &text);
        assert_eq!(
            sink.kinds_starting_at(pseudo_offset),
            vec![SyntaxKind::PseudoSelectorList],
            "`{text}`: the node at the pseudo's own `:` must be exactly \
             PseudoSelectorList; full start list: {:?}",
            sink.starts,
        );
    }
}
