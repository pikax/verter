//! The factored product of a template literal type.
//!
//! A template literal type distributes over each union among its spans
//! (`getTemplateLiteralType`): its value is the cross product of the spans'
//! constituents, each concatenation joined with the template's texts. A
//! [`TemplatePlan`] holds that product FACTORED — the texts and, per span,
//! its normalized constituents — and answers what the product requires
//! before any concatenation exists:
//!
//! - its [`cardinality`](TemplatePlan::cardinality), the constituents the
//!   checker counts (`getCrossProductUnionSize`), with checked arithmetic
//!   and an empty span winning wherever it sits, so a product that would
//!   overflow before a later `never` span is still the empty product;
//! - its [`construction_bytes`](TemplatePlan::construction_bytes), the bytes
//!   every concatenation holds once built, a different quantity from the
//!   count: long literals cost bytes, not constituents.
//!
//! Construction is charged on both before it begins and then streams: a
//! [`TemplateCursor`] walks the product in order, building each
//! concatenation alone into a buffer sized exactly for it. No prefix of the
//! product is ever held.

use crate::semantic_query::SemanticNodeId;

/// One piece of a distributed template concatenation: literal text, or a
/// hole type the template literal type keeps (`string`, `number`, …).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum TemplatePiece {
    Text(String),
    Hole(SemanticNodeId),
}

/// One constituent a template, or one of its interpolated expressions,
/// distributes into: its pieces, and — for a literal type that is not a
/// string (a number, bigint or boolean literal, `null`, `undefined`, an enum
/// member) — that type. The checker counts constituents, not texts: `"0"`
/// and `0` are two constituents of `"0" | 0` though they spell the same
/// text, while `"a0"` and the `"a0"` a nested template forms are one.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct TemplateConstituent {
    pub(super) pieces: Vec<TemplatePiece>,
    pub(super) spelled: Option<SemanticNodeId>,
}

impl TemplateConstituent {
    /// A string literal or template literal constituent of `pieces`.
    pub(super) fn string(pieces: Vec<TemplatePiece>) -> Self {
        Self {
            pieces,
            spelled: None,
        }
    }

    /// The text bytes and holes this constituent contributes.
    fn weight(&self) -> (usize, usize) {
        self.pieces
            .iter()
            .fold((0, 0), |(text, holes), piece| match piece {
                TemplatePiece::Text(piece) => (text + piece.len(), holes),
                TemplatePiece::Hole(_) => (text, holes + 1),
            })
    }
}

/// The bytes one hole adds to a built template literal type: its operand id
/// and the text that follows it.
const HOLE_BYTES: usize =
    std::mem::size_of::<SemanticNodeId>() + std::mem::size_of::<verter_type_expr::OwnedStr>();

/// A template literal type's product, factored: its `texts` (one more than
/// its spans) and each span's normalized constituents, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TemplatePlan {
    texts: Vec<String>,
    spans: Vec<Vec<TemplateConstituent>>,
}

impl TemplatePlan {
    /// The plan of `texts` around `spans`; a missing text is empty.
    pub(super) fn new(mut texts: Vec<String>, spans: Vec<Vec<TemplateConstituent>>) -> Self {
        texts.resize(spans.len() + 1, String::new());
        Self { texts, spans }
    }

    /// Each span's constituent count, as the checker's cross-product rule
    /// weighs it.
    pub(super) fn span_widths(&self) -> impl Iterator<Item = usize> + '_ {
        self.spans.iter().map(Vec::len)
    }

    /// The number of concatenations the product holds; `None` when it does
    /// not fit a `usize`. Any empty span makes the product empty, wherever
    /// it sits: an overflow before it is not an overflow of the product.
    pub(super) fn cardinality(&self) -> Option<usize> {
        if self.spans.iter().any(Vec::is_empty) {
            return Some(0);
        }
        self.spans
            .iter()
            .try_fold(1usize, |size, span| size.checked_mul(span.len()))
    }

    /// The bytes the product holds once built: every concatenation as its
    /// own type node, summed in factored form (each constituent of a span
    /// sits in `cardinality / width` concatenations), and, for more than one
    /// concatenation, the union they join while it is canonicalized. `None`
    /// when the sum does not fit a `usize`.
    pub(super) fn construction_bytes(&self) -> Option<usize> {
        let cardinality = self.cardinality()?;
        if cardinality == 0 {
            return Some(0);
        }
        let texts = self
            .texts
            .iter()
            .try_fold(0usize, |total, text| total.checked_add(text.len()))?;
        let mut total =
            cardinality.checked_mul(super::connected_demand::derived_node_bytes(texts))?;
        for span in &self.spans {
            let repeats = cardinality / span.len();
            let weight = span.iter().try_fold(0usize, |total, constituent| {
                let (text, holes) = constituent.weight();
                total.checked_add(text.checked_add(holes.checked_mul(HOLE_BYTES)?)?)
            })?;
            total = total.checked_add(repeats.checked_mul(weight)?)?;
        }
        if cardinality > 1 {
            total = total
                .checked_add(cardinality.checked_mul(super::connected_demand::UNION_ARM_BYTES)?)?;
        }
        Some(total)
    }

    /// The product's concatenations, one at a time, in order (the first
    /// span varies slowest).
    pub(super) fn cursor(&self) -> TemplateCursor<'_> {
        TemplateCursor {
            plan: self,
            choice: vec![0; self.spans.len()],
            done: self.spans.iter().any(Vec::is_empty),
        }
    }
}

/// One concatenation of a product: its texts (one more than its holes) and
/// its holes. A concatenation with no hole is a single text.
#[derive(Debug, Default)]
pub(super) struct TemplateConcatenation {
    pub(super) texts: Vec<String>,
    pub(super) holes: Vec<SemanticNodeId>,
}

impl TemplateConcatenation {
    /// The pieces of this concatenation: a text-only one is the single text
    /// a string literal of it holds, so the two are one constituent.
    pub(super) fn into_pieces(self) -> Vec<TemplatePiece> {
        if self.holes.is_empty() {
            return self.texts.into_iter().map(TemplatePiece::Text).collect();
        }
        let mut pieces = Vec::with_capacity(self.texts.len() + self.holes.len());
        let mut holes = self.holes.into_iter();
        for text in self.texts {
            if !text.is_empty() {
                pieces.push(TemplatePiece::Text(text));
            }
            if let Some(hole) = holes.next() {
                pieces.push(TemplatePiece::Hole(hole));
            }
        }
        pieces
    }
}

/// A walk over a [`TemplatePlan`]'s product that builds one concatenation
/// per step: an odometer over the spans' constituents, so the walk holds a
/// choice per span and never a prefix of the product.
pub(super) struct TemplateCursor<'plan> {
    plan: &'plan TemplatePlan,
    choice: Vec<usize>,
    done: bool,
}

impl TemplateCursor<'_> {
    /// The constituents chosen for the current concatenation.
    fn chosen(&self) -> impl Iterator<Item = &TemplateConstituent> {
        self.plan
            .spans
            .iter()
            .zip(&self.choice)
            .map(|(span, choice)| &span[*choice])
    }

    /// Build the current concatenation and advance; `None` past the end.
    /// Each text is allocated once at its exact length.
    pub(super) fn next_concatenation(&mut self) -> Option<TemplateConcatenation> {
        if self.done {
            return None;
        }
        // Size each text before building it: the text before the first
        // hole, between holes, and after the last.
        let mut lengths: Vec<usize> = vec![self.plan.texts[0].len()];
        let mut holes = 0usize;
        for (index, constituent) in self.chosen().enumerate() {
            for piece in &constituent.pieces {
                match piece {
                    TemplatePiece::Text(text) => {
                        *lengths.last_mut().expect("a text") += text.len();
                    }
                    TemplatePiece::Hole(_) => {
                        holes += 1;
                        lengths.push(0);
                    }
                }
            }
            *lengths.last_mut().expect("a text") += self.plan.texts[index + 1].len();
        }
        let mut texts: Vec<String> = Vec::with_capacity(lengths.len());
        texts.push(String::with_capacity(lengths[0]));
        let mut concatenation = TemplateConcatenation {
            texts: Vec::new(),
            holes: Vec::with_capacity(holes),
        };
        texts[0].push_str(&self.plan.texts[0]);
        for (index, constituent) in self.chosen().enumerate() {
            for piece in &constituent.pieces {
                match piece {
                    TemplatePiece::Text(text) => {
                        texts.last_mut().expect("a text").push_str(text);
                    }
                    TemplatePiece::Hole(hole) => {
                        concatenation.holes.push(*hole);
                        texts.push(String::with_capacity(lengths[texts.len()]));
                    }
                }
            }
            texts
                .last_mut()
                .expect("a text")
                .push_str(&self.plan.texts[index + 1]);
        }
        concatenation.texts = texts;
        self.advance();
        Some(concatenation)
    }

    /// The current concatenation as one constituent of an enclosing span.
    pub(super) fn next_constituent(&mut self) -> Option<TemplateConstituent> {
        self.next_concatenation()
            .map(|concatenation| TemplateConstituent::string(concatenation.into_pieces()))
    }

    fn advance(&mut self) {
        for (span, choice) in self.plan.spans.iter().zip(self.choice.iter_mut()).rev() {
            *choice += 1;
            if *choice < span.len() {
                return;
            }
            *choice = 0;
        }
        self.done = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> TemplateConstituent {
        TemplateConstituent::string(vec![TemplatePiece::Text(value.to_owned())])
    }

    fn span(values: &[&str]) -> Vec<TemplateConstituent> {
        values.iter().map(|value| text(value)).collect()
    }

    fn texts(plan: &TemplatePlan) -> Vec<String> {
        let mut cursor = plan.cursor();
        std::iter::from_fn(|| cursor.next_concatenation())
            .map(|concatenation| {
                assert!(concatenation.holes.is_empty());
                concatenation.texts.concat()
            })
            .collect()
    }

    /// The cursor walks the product in order, the first span slowest, and
    /// joins each choice with the template's texts.
    #[test]
    fn the_cursor_walks_the_product_in_order() {
        let plan = TemplatePlan::new(
            vec!["<".into(), "-".into(), ">".into()],
            vec![span(&["a", "b"]), span(&["0", "1", "2"])],
        );
        assert_eq!(plan.cardinality(), Some(6));
        assert_eq!(
            texts(&plan),
            ["<a-0>", "<a-1>", "<a-2>", "<b-0>", "<b-1>", "<b-2>"]
        );
    }

    /// An empty span empties the product wherever it sits, even past a
    /// prefix whose count would already overflow, and the cursor builds
    /// nothing.
    #[test]
    fn an_empty_span_empties_an_overflowing_product() {
        let wide = TemplatePlan::new(Vec::new(), vec![span(&["a", "b"]); 70]);
        assert_eq!(wide.cardinality(), None, "2^70 overflows");
        assert_eq!(wide.construction_bytes(), None);
        for at in [0, 35, 70] {
            let mut spans = vec![span(&["a", "b"]); 70];
            spans.insert(at, Vec::new());
            let plan = TemplatePlan::new(Vec::new(), spans);
            assert_eq!(plan.cardinality(), Some(0), "empty span at {at}");
            assert_eq!(plan.construction_bytes(), Some(0));
            assert!(plan.cursor().next_concatenation().is_none());
        }
    }

    /// The bytes count text, not constituents: two products of the same
    /// count differ by their literals' lengths, and the factored sum is the
    /// sum over every concatenation built.
    #[test]
    fn construction_bytes_count_text_not_constituents() {
        let short = TemplatePlan::new(Vec::new(), vec![span(&["a", "b"]), span(&["c", "d"])]);
        let long_text = "x".repeat(1000);
        let long = TemplatePlan::new(
            Vec::new(),
            vec![span(&["a", "b"]), span(&["c", long_text.as_str()])],
        );
        assert_eq!(short.cardinality(), long.cardinality());
        let node = super::super::connected_demand::derived_node_bytes(0);
        let union = super::super::connected_demand::UNION_ARM_BYTES;
        let sum = |plan: &TemplatePlan| {
            texts(plan)
                .iter()
                .map(|text| node + union + text.len())
                .sum::<usize>()
        };
        assert_eq!(short.construction_bytes(), Some(sum(&short)));
        assert_eq!(long.construction_bytes(), Some(sum(&long)));
        assert!(long.construction_bytes() > short.construction_bytes());
    }

    /// A hole stays a hole, with the texts around it joined.
    #[test]
    fn holes_split_the_texts_around_them() {
        let hole = SemanticNodeId(7);
        let plan = TemplatePlan::new(
            vec!["a".into(), "b".into(), "c".into()],
            vec![
                vec![TemplateConstituent::string(vec![
                    TemplatePiece::Text("x".into()),
                    TemplatePiece::Hole(hole),
                ])],
                span(&["y"]),
            ],
        );
        let mut cursor = plan.cursor();
        let only = cursor.next_concatenation().expect("one concatenation");
        assert_eq!(only.texts, ["ax", "byc"]);
        assert_eq!(only.holes, [hole]);
        assert!(cursor.next_concatenation().is_none());
        let node = super::super::connected_demand::derived_node_bytes(0);
        assert_eq!(plan.construction_bytes(), Some(node + 5 + HOLE_BYTES));
    }
}
