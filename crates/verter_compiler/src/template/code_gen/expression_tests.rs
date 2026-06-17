//! Tests for the segmented expression-plan builder.
//!
//! These pin the core invariant: every byte of resolver scaffolding
//! (`__props.`, `.value`, keyword brackets) is an UNMAPPED segment, and only
//! authored identifiers carry a source offset. A naive builder that mapped the
//! whole resolved string to one offset fails the `.value` / `__props.`
//! "is None" assertions.

use super::{build_prefixed_expr_segments, resolve_simple_expr_segments};
use crate::common::RelativeSpan;
use crate::template::code_gen::binding::{BindingResolver, BindingType};
use crate::template::code_gen::types::MappedGeneratedText;
use crate::template::code_gen::vapor::interpolation::build_prefixed_expr;
use crate::template::oxc::types::OxcParsedExpression;
use crate::utils::oxc::{Binding, BindingExtractionResult, Dynamism};
use rustc_hash::FxHashMap;

fn make_resolver(
    entries: &[(&'static str, BindingType)],
    is_inline: bool,
) -> BindingResolver<'static> {
    let mut map = FxHashMap::default();
    for &(name, bt) in entries {
        map.insert(name as &str, bt);
    }
    BindingResolver::new(map, is_inline)
}

/// Extract `(segment_text, source_start)` pairs in order.
fn seg_pairs(mgt: &MappedGeneratedText) -> Vec<(&str, Option<u32>)> {
    mgt.segments
        .iter()
        .map(|s| {
            (
                &mgt.text[s.generated_start as usize..s.generated_end as usize],
                s.source_start,
            )
        })
        .collect()
}

/// Segments must tile the whole `text` contiguously with no gaps/overlaps, so
/// the concatenation of every segment slice reproduces `text` byte-for-byte
/// (the emitter relies on this to keep generated output identical).
fn assert_tiles(mgt: &MappedGeneratedText) {
    let mut cursor = 0u32;
    let mut rebuilt = String::new();
    for s in &mgt.segments {
        assert_eq!(
            s.generated_start, cursor,
            "segment must start where the previous ended; {:?}",
            mgt.segments
        );
        assert!(
            s.generated_end > s.generated_start,
            "no zero-length segments; {:?}",
            mgt.segments
        );
        rebuilt.push_str(&mgt.text[s.generated_start as usize..s.generated_end as usize]);
        cursor = s.generated_end;
    }
    assert_eq!(
        cursor as usize,
        mgt.text.len(),
        "segments cover the whole text"
    );
    assert_eq!(rebuilt, mgt.text, "concatenated segments equal text");
}

/// Discriminating — an inline `SetupRef` resolves `count` → `count.value`, and
/// the synthetic `.value` suffix MUST be an unmapped segment. Mapping the whole
/// resolved string would put `.value` under the `count` token (a mapping bleed).
#[test]
fn simple_setup_ref_keeps_value_suffix_unmapped() {
    let resolver = make_resolver(&[("count", BindingType::SetupRef)], true);
    // `count` lives at file byte 4 (e.g. `v-if="count"` value at 4).
    let mgt = resolve_simple_expr_segments(&resolver, "count", 4);

    assert_eq!(mgt.text, "count.value");
    assert_eq!(
        seg_pairs(&mgt),
        vec![("count", Some(4)), (".value", None)],
        "`.value` must be a SEPARATE unmapped segment, `count` mapped to its source"
    );
    assert_tiles(&mgt);
    // Byte-identical to the flat resolver path.
    assert_eq!(mgt.text, resolver.resolve_simple_expr("count"));
}

/// Discriminating — a prop literally named like a JS keyword (`class`) resolves
/// to bracket form `__props["class"]`; only `class` maps, the `__props["` and
/// `"]` scaffolding stays unmapped.
#[test]
fn keyword_prop_keeps_brackets_unmapped() {
    let resolver = make_resolver(&[("class", BindingType::Props)], true);
    let mgt = resolve_simple_expr_segments(&resolver, "class", 10);

    assert_eq!(mgt.text, "__props[\"class\"]");
    // `class` is the only mapped segment; everything else is None.
    let pairs = seg_pairs(&mgt);
    let class_seg = pairs.iter().find(|(t, _)| *t == "class");
    assert_eq!(
        class_seg,
        Some(&("class", Some(10))),
        "`class` must map to its source offset; pairs: {pairs:?}"
    );
    assert!(
        pairs.iter().all(|(t, src)| *t == "class" || src.is_none()),
        "every non-identifier byte (`__props[\"`, `\"]`) must be unmapped; pairs: {pairs:?}"
    );
    assert_tiles(&mgt);
    assert_eq!(mgt.text, resolver.resolve_simple_expr("class"));
}

/// Build an OXC expression with two prop bindings at the given file offsets.
fn two_prop_oxc(
    inner_start: u32,
    a: (&'static str, u32),
    b: (&'static str, u32),
) -> OxcParsedExpression<'static> {
    OxcParsedExpression {
        offset: inner_start,
        expression: None,
        errors: None,
        bindings: Some(BindingExtractionResult {
            bindings: vec![
                Binding {
                    name: a.0,
                    span: RelativeSpan::new(
                        a.1 - inner_start,
                        a.1 - inner_start + a.0.len() as u32,
                    ),
                    pos: a.1,
                    ignore: false,
                    is_shorthand: false,
                },
                Binding {
                    name: b.0,
                    span: RelativeSpan::new(
                        b.1 - inner_start,
                        b.1 - inner_start + b.0.len() as u32,
                    ),
                    pos: b.1,
                    ignore: false,
                    is_shorthand: false,
                },
            ],
            functions: vec![],
            literals: vec![],
            has_errors: false,
            dynamism: Dynamism::Dynamic,
        }),
        dynamism: Dynamism::Dynamic,
    }
}

/// Discriminating — a compound condition `foo && bar` (both props) resolves to
/// `__props.foo && __props.bar`; `foo` and `bar` each map to their own source
/// token and EVERY `__props.` run is unmapped. A flat single-token map would put
/// one token on the leading `__props.` and leave `bar` untokened.
#[test]
fn compound_props_map_each_identifier_not_the_prefix() {
    let resolver = make_resolver(
        &[("foo", BindingType::Props), ("bar", BindingType::Props)],
        true,
    );
    // `foo` at 4, `bar` at 11 (`foo && bar`: foo[0..3] + ` && `[3..7] + bar[7..10]).
    let oxc = two_prop_oxc(4, ("foo", 4), ("bar", 11));
    let mgt = build_prefixed_expr_segments("foo && bar", 4, &oxc, &resolver, &[]);

    assert_eq!(mgt.text, "__props.foo && __props.bar");
    let pairs = seg_pairs(&mgt);

    // foo and bar each map to their authored offsets.
    assert!(
        pairs.contains(&("foo", Some(4))),
        "`foo` must map to src 4; pairs: {pairs:?}"
    );
    assert!(
        pairs.contains(&("bar", Some(11))),
        "`bar` must map to its OWN src token (11), not be swallowed by the prefix; pairs: {pairs:?}"
    );
    // Both `__props.` runs are unmapped.
    let props_segs: Vec<_> = pairs.iter().filter(|(t, _)| *t == "__props.").collect();
    assert_eq!(props_segs.len(), 2, "two `__props.` runs; pairs: {pairs:?}");
    assert!(
        props_segs.iter().all(|(_, src)| src.is_none()),
        "every `__props.` run must be unmapped; pairs: {pairs:?}"
    );
    assert_tiles(&mgt);
    // Byte-identical to the flat builder.
    assert_eq!(
        mgt.text,
        build_prefixed_expr("foo && bar", 4, &oxc, &resolver, &[])
    );
}

/// A bare identifier (no prefix/suffix) is a single mapped segment.
#[test]
fn bare_setup_binding_is_single_mapped_segment() {
    // Inline setup const (no `.value`): bare, mapped.
    let resolver = make_resolver(&[("show", BindingType::SetupConst)], true);
    let mgt = resolve_simple_expr_segments(&resolver, "show", 7);
    assert_eq!(mgt.text, "show");
    assert_eq!(seg_pairs(&mgt), vec![("show", Some(7))]);
    assert_tiles(&mgt);
}

/// An unknown identifier under VDOM standalone mode gets a `_ctx.` prefix that
/// stays unmapped while the identifier maps.
#[test]
fn unknown_ident_keeps_ctx_prefix_unmapped() {
    let resolver = make_resolver(&[], false);
    let mgt = resolve_simple_expr_segments(&resolver, "msg", 3);
    assert_eq!(mgt.text, "_ctx.msg");
    assert_eq!(seg_pairs(&mgt), vec![("_ctx.", None), ("msg", Some(3))]);
    assert_tiles(&mgt);
    assert_eq!(mgt.text, resolver.resolve_simple_expr("msg"));
}
