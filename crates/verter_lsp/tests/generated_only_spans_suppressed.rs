//! Architecture guard (span classification): a synthetic Verter-generated
//! helper-region span NEVER escapes to the user — it is classified `GeneratedOnly`
//! and SUPPRESSED. A span that maps back to the carrier source is `SourceMappable`
//! (returned, mapped); a span on a real on-disk `.ts` is `External` (returned
//! as-is). Framework-agnostic (Vue AND Svelte).
//!
//! This is the project-bound external-TS-engine guard
//! `generated_only_spans_suppressed`. It exercises the production span classifier
//! ([`verter_lsp::external_ts_sync::classify_provider_range`]) over BOTH a stub
//! mapper and the REAL [`ProviderPositionMapper`]. Classification is RANGE-based
//! (a TS result is a span/edit) and keyed on the TYPED [`SpanSubjectKind`] (NEVER a
//! path-suffix heuristic). It includes discriminating self-checks: a classifier
//! that did NOT suppress an unmapped companion span, that classified point-wise
//! (leaking a range straddling the generated boundary), or that derived
//! companion-ness from the path shape, would fail here.

use verter_lsp::documents::line_index::LineIndex;
use verter_lsp::documents::provider_projection::{ProviderPositionMapper, SelfFileProviderMapper};
use verter_lsp::external_ts_sync::{
    classify_provider_range, SpanClass, SpanMapperView, SpanSubjectKind,
};
use verter_session::external_ts::SnapshotRole;

/// A test mapper: a range maps as a whole iff BOTH endpoint lines are allow-listed.
struct AllowListMapper {
    mapped: Vec<u32>,
}

impl SpanMapperView for AllowListMapper {
    fn provider_range_maps_to_source(
        &self,
        start_line: u32,
        _start_char: u32,
        end_line: u32,
        _end_char: u32,
    ) -> bool {
        self.mapped.contains(&start_line) && self.mapped.contains(&end_line)
    }
}

#[test]
fn unmapped_companion_span_is_generated_only_and_suppressed_vue() {
    let mapper = AllowListMapper { mapped: vec![10] };
    let helper = classify_provider_range(&mapper, SpanSubjectKind::Companion, 0, 0, 0, 5);
    assert_eq!(helper, SpanClass::GeneratedOnly);
    assert!(
        helper.is_suppressed(),
        "a synthetic helper-region span inside a Vue carrier companion must be suppressed"
    );
    let real = classify_provider_range(&mapper, SpanSubjectKind::Companion, 10, 0, 10, 4);
    assert_eq!(real, SpanClass::SourceMappable);
    assert!(!real.is_suppressed());
}

#[test]
fn unmapped_companion_span_is_generated_only_and_suppressed_svelte() {
    let mapper = AllowListMapper { mapped: vec![20] };
    let helper = classify_provider_range(&mapper, SpanSubjectKind::Companion, 1, 0, 1, 3);
    assert_eq!(helper, SpanClass::GeneratedOnly);
    assert!(
        helper.is_suppressed(),
        "framework parity: a synthetic helper span inside a Svelte carrier companion is suppressed"
    );
}

#[test]
fn range_straddling_generated_boundary_is_suppressed() {
    // Range-based discriminator: start (line 10) maps, end (line 11) does NOT. A
    // point-based classifier would pass the start and leak generated content.
    let mapper = AllowListMapper { mapped: vec![10] };
    let straddle = classify_provider_range(&mapper, SpanSubjectKind::Companion, 10, 0, 11, 4);
    assert_eq!(
        straddle,
        SpanClass::GeneratedOnly,
        "a span straddling the generated/source boundary must be suppressed (range-based)"
    );
}

#[test]
fn external_real_ts_span_is_returned_as_is() {
    // Typed External subject: an unmapped span is External, never suppressed.
    let mapper = AllowListMapper { mapped: vec![] };
    let class = classify_provider_range(&mapper, SpanSubjectKind::External, 0, 0, 0, 3);
    assert_eq!(class, SpanClass::External);
    assert!(
        !class.is_suppressed(),
        "a span on a real on-disk .ts is external and returned as-is, never suppressed"
    );
}

#[test]
fn typed_subject_wins_over_path_shape() {
    // The companion-vs-real decision is the typed subject, NOT a path suffix. A
    // path-suffix classifier would get these backwards.
    let mapper = AllowListMapper { mapped: vec![] }; // nothing maps
    assert_eq!(
        classify_provider_range(&mapper, SpanSubjectKind::Companion, 0, 0, 0, 4),
        SpanClass::GeneratedOnly,
        "typed Companion suppresses its synthetic region regardless of path shape"
    );
    assert_eq!(
        classify_provider_range(&mapper, SpanSubjectKind::External, 0, 0, 0, 4),
        SpanClass::External,
        "typed External is never suppressed regardless of path shape"
    );
}

#[test]
fn span_subject_kind_is_derived_from_role_structurally() {
    for role in [
        SnapshotRole::CarrierIde,
        SnapshotRole::CarrierApi,
        SnapshotRole::CarrierBatch,
        SnapshotRole::Shadow,
    ] {
        assert_eq!(
            SpanSubjectKind::from_role(role),
            SpanSubjectKind::Companion,
            "{role:?} is a Verter companion surface"
        );
    }
    assert_eq!(
        SpanSubjectKind::from_role(SnapshotRole::Real),
        SpanSubjectKind::External,
        "Real is a genuine on-disk file"
    );
}

#[test]
fn suppression_rides_the_real_provider_position_mapper() {
    // A REAL SelfFile mapper (rune-module surface) with a 2-line synthetic prelude.
    // The production tsx_range_to_carrier returns None for the prelude region.
    let src = "export const x = 1;\nexport const y = 2;\n";
    let line_index = LineIndex::new_utf16(src);
    let mapper = ProviderPositionMapper::SelfFile(SelfFileProviderMapper::new(2, &[], &line_index));

    let prelude = classify_provider_range(&mapper, SpanSubjectKind::Companion, 0, 0, 0, 6);
    assert_eq!(
        prelude,
        SpanClass::GeneratedOnly,
        "the real mapper's synthetic prelude region must classify GeneratedOnly (suppressed)"
    );
    assert!(prelude.is_suppressed());

    let user = classify_provider_range(&mapper, SpanSubjectKind::Companion, 2, 0, 2, 5);
    assert_eq!(
        user,
        SpanClass::SourceMappable,
        "the real mapper's user-source region maps back (not suppressed)"
    );
}

/// Discriminating self-check: ONLY GeneratedOnly is suppressed.
#[test]
fn only_generated_only_is_suppressed() {
    assert!(SpanClass::GeneratedOnly.is_suppressed());
    assert!(!SpanClass::SourceMappable.is_suppressed());
    assert!(!SpanClass::External.is_suppressed());
}
