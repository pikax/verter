use crate::coordinates::{AuthoredFormatByteOffset as O, AuthoredFormatByteRange as R};
use crate::doc::{Doc, Line};
use crate::provenance::{AuthoredProvenance, FormatProvenanceTable, Provenanced};
use crate::render::{render, RenderOptions, Segment};

#[test]
fn source_segments_retain_exact_bindings_in_emission_order() {
    let mut table = FormatProvenanceTable::new(17_u128);
    let first = R::new(O::from_raw(0), O::from_raw(2)).unwrap();
    let second = R::new(O::from_raw(2), O::from_raw(6)).unwrap();
    let a = AuthoredProvenance::new(table.bind_authored(first), first);
    let b = AuthoredProvenance::new(table.bind_authored(second), second);
    let mut doc = Doc::new();
    let suffix = doc.source_line_suffix(Provenanced::new("🙂".into(), b));
    let text = doc.source(Provenanced::new("é".into(), a));
    let line = doc.line(Line::Hard);
    let root = doc.concat([suffix, text, line]);
    let output = render(&doc, root, RenderOptions::default()).unwrap();
    assert_eq!(output.text, "é🙂\n");
    let bindings: Vec<_> = output
        .segments
        .iter()
        .filter_map(|s| match s {
            Segment::Source(s) => Some((*s.provenance(), s.value().range)),
            _ => None,
        })
        .collect();
    assert_eq!(bindings.len(), 2);
    assert_eq!(bindings[0].0, a);
    assert_eq!(bindings[1].0, b);
    assert_eq!(
        (bindings[0].1.start().get(), bindings[0].1.end().get()),
        (0, 2)
    );
    assert_eq!(
        (bindings[1].1.start().get(), bindings[1].1.end().get()),
        (2, 6)
    );
    for (binding, _) in bindings {
        table.validate(&17, &binding).unwrap();
    }
    for _ in 0..8 {
        assert_eq!(
            render(&doc, root, RenderOptions::default()).unwrap(),
            output
        );
    }
}
