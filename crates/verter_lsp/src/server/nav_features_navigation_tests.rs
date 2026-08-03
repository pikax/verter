//! Tests for the cross-file Vue-prop rename synthesis + completeness gate in
//! `nav_features_navigation`. Extracted to a sibling per the test-organization
//! convention (inline `#[cfg(test)]` over ~400 lines moves to a `*_tests.rs`
//! sibling). Wired from the parent via `#[cfg(test)] #[path = …] mod …;`, so the
//! two inner modules reach the parent's private items through `super::super::`.

#[cfg(test)]
mod synthesized_rename_injection_tests {
    use super::super::inject_synthesized_carrier_rename_location;
    use crate::type_provider::protocol::RenameLocation;

    const API: &str = "/src/MyComp.vue.ts";

    fn loc(path: &str, start: u32, end: u32) -> RenameLocation {
        RenameLocation {
            path: path.to_string(),
            start,
            end,
        }
    }

    fn count_matching(locs: &[RenameLocation], path: &str, start: u32, end: u32) -> usize {
        locs.iter()
            .filter(|l| l.path == path && l.start == start && l.end == end)
            .count()
    }

    #[test]
    fn dedups_provider_location_for_same_prop_decl_to_exactly_one() {
        // The provider (tsserver) ALSO returned the carrier location for the SAME
        // prop declaration the synthesis targets.
        let mut locs = vec![loc(API, 40, 43)];
        inject_synthesized_carrier_rename_location(&mut locs, API, 40, 43);
        // EXACTLY one — discriminating: WITHOUT the dedup `retain` this is 2
        // (the provider's + the synthesized), a duplicate child edit.
        assert_eq!(
            count_matching(&locs, API, 40, 43),
            1,
            "the child-declaration carrier edit must appear exactly once (one deterministic origin)"
        );
    }

    #[test]
    fn preserves_other_provider_locations() {
        // The provider returned the matching carrier decl AND other valid locations
        // (the parent usage in App.vue.tsx, and a DIFFERENT-range carrier ref the
        // Vue-prop synthesis does not model).
        let app = "/src/App.vue.tsx";
        let mut locs = vec![
            loc(app, 1000, 1003), // parent usage — must survive
            loc(API, 40, 43),     // same prop decl — deduped against synthesis
            loc(API, 80, 83),     // a different carrier ref — must survive
        ];
        inject_synthesized_carrier_rename_location(&mut locs, API, 40, 43);

        assert_eq!(
            count_matching(&locs, API, 40, 43),
            1,
            "the synthesized prop-decl edit is the single origin"
        );
        assert_eq!(
            count_matching(&locs, app, 1000, 1003),
            1,
            "an unrelated provider location (parent usage) must be preserved"
        );
        assert_eq!(
            count_matching(&locs, API, 80, 83),
            1,
            "a different-range provider carrier location must be preserved (not broadly dropped)"
        );
    }

    #[test]
    fn injects_when_provider_did_not_report_the_child_decl() {
        // tsgo: the provider did NOT enumerate the child-declaration leg, so the
        // synthesized location is the ONLY one for the prop decl — it must be added.
        let mut locs: Vec<RenameLocation> = vec![loc("/src/App.vue.tsx", 1000, 1003)];
        inject_synthesized_carrier_rename_location(&mut locs, API, 40, 43);
        assert_eq!(
            count_matching(&locs, API, 40, 43),
            1,
            "the synthesized child-declaration leg must be injected when the provider omits it"
        );
    }

    #[test]
    fn prunes_same_start_different_end_provider_location_by_overlap() {
        // The provider returned a carrier location for the SAME prop declaration but
        // with a DIFFERENT end (it ranged `foo: string`, bytes 40..51, where the
        // synthesis ranges only the name `foo`, 40..43). The downstream merge dedups
        // carrier edits by mapped-`.vue` `range.start`; a same-start provider edit
        // left in the set would SUPPRESS the synthesized one (whichever lands first
        // wins the start slot) → the child decl could map to a wrong/over-covering
        // range. The overlap-prune must drop the provider's overlapping location.
        let mut locs = vec![loc(API, 40, 51)];
        inject_synthesized_carrier_rename_location(&mut locs, API, 40, 43);
        // DISCRIMINATING: with the OLD exact-only `retain` this is 2 (the provider's
        // 40..51 survives alongside the synthesized 40..43); with overlap-prune it is 1.
        assert_eq!(
            locs.len(),
            1,
            "a same-start-different-end provider carrier location must be pruned by overlap"
        );
        assert_eq!(
            count_matching(&locs, API, 40, 43),
            1,
            "only the synthesized exact-name range survives"
        );
        assert_eq!(
            count_matching(&locs, API, 40, 51),
            0,
            "the provider's overlapping (wider) range must be dropped, not kept"
        );
    }

    #[test]
    fn prunes_partial_overlap_provider_location() {
        // A provider location that PARTIALLY overlaps the synthesized range (44..47
        // vs synthesized 40..46 — provider start inside the synthesized range) must
        // also be pruned: it would map into the same declaration region.
        let mut locs = vec![loc(API, 44, 47)];
        inject_synthesized_carrier_rename_location(&mut locs, API, 40, 46);
        assert_eq!(
            locs.len(),
            1,
            "a partially-overlapping provider carrier location must be pruned"
        );
        assert_eq!(count_matching(&locs, API, 40, 46), 1);
    }

    #[test]
    fn keeps_adjacent_non_overlapping_provider_location() {
        // A provider carrier location that is ADJACENT but does NOT overlap (43..46,
        // touching the synthesized 40..43 at the half-open boundary) is a DIFFERENT
        // reference the synthesis does not model — it must be PRESERVED (the narrowing
        // is overlap-only, never broader).
        let mut locs = vec![loc(API, 43, 46)];
        inject_synthesized_carrier_rename_location(&mut locs, API, 40, 43);
        assert_eq!(
            locs.len(),
            2,
            "an adjacent, non-overlapping provider carrier location must be preserved"
        );
        assert_eq!(count_matching(&locs, API, 40, 43), 1);
        assert_eq!(count_matching(&locs, API, 43, 46), 1);
    }
}

#[cfg(test)]
mod cross_file_rename_gate_tests {
    use super::super::nav_features_navigation_rename_gate::gate_cross_file_child_prop_rename;
    use crate::server::child_prop_rename::{
        ChildPropDeclarationProof, ChildPropRenameClass, ChildPropUsage, ConfirmedChildPropRename,
    };
    use std::collections::HashMap;
    use tower_lsp_server::ls_types::{Position, Range, TextEdit, Uri, WorkspaceEdit};

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    /// The declaration file's URI. For the INLINE case this is the child `.vue`; for
    /// the IMPORTED case it is the THIRD file (the imported type's `.ts`). The gate
    /// keys on the resolved declaration URI uniformly, so the same helper serves both.
    fn decl_uri() -> Uri {
        uri("file:///src/MyComp.vue")
    }

    /// The IMPORTED-type member declaration file (a THIRD file).
    fn imported_decl_uri() -> Uri {
        uri("file:///src/importedProps.ts")
    }

    fn parent_uri() -> Uri {
        uri("file:///src/App.vue")
    }

    fn rng(line: u32, start_ch: u32, end_ch: u32) -> Range {
        Range {
            start: Position {
                line,
                character: start_ch,
            },
            end: Position {
                line,
                character: end_ch,
            },
        }
    }

    /// The decl is at line 5:11..5:14, the parent usage at App.vue 3:9..3:12.
    fn decl_range() -> Range {
        rng(5, 11, 14)
    }
    fn parent_usage_range() -> Range {
        rng(3, 9, 12)
    }

    /// A `Confirmed` rename whose declaration is `Known` (the INLINE child-`.vue`
    /// macro case, carrying an `inline_decl_span`). Both expected ranges present.
    fn confirmed_known_inline() -> ChildPropRenameClass {
        ChildPropRenameClass::Confirmed(Box::new(ConfirmedChildPropRename {
            usage: ChildPropUsage {
                parent_uri: parent_uri(),
                parent_prop_name: "foo".to_string(),
                parent_prop_name_span: verter_span::Span { start: 0, end: 3 },
                parent_is_shorthand: false,
                child_carrier_api_path: "/src/MyComp.vue.ts".to_string(),
            },
            expected_parent_usage_range: Some(parent_usage_range()),
            declaration: ChildPropDeclarationProof::Known {
                uri: decl_uri(),
                range: Some(decl_range()),
                inline_decl_span: Some(verter_span::Span {
                    start: 100,
                    end: 103,
                }),
            },
        }))
    }

    /// A `Confirmed` rename whose declaration is `Known` from the IMPORTED-type hop
    /// (a THIRD file, NO inline synthesis span — the provider's own rename edits it).
    fn confirmed_known_imported() -> ChildPropRenameClass {
        ChildPropRenameClass::Confirmed(Box::new(ConfirmedChildPropRename {
            usage: ChildPropUsage {
                parent_uri: parent_uri(),
                parent_prop_name: "foo".to_string(),
                parent_prop_name_span: verter_span::Span { start: 0, end: 3 },
                parent_is_shorthand: false,
                child_carrier_api_path: "/src/MyComp.vue.ts".to_string(),
            },
            expected_parent_usage_range: Some(parent_usage_range()),
            declaration: ChildPropDeclarationProof::Known {
                uri: imported_decl_uri(),
                range: Some(decl_range()),
                inline_decl_span: None,
            },
        }))
    }

    /// A `Confirmed` rename whose declaration is `Unknown` — the imported-type case
    /// whose provider `get_definition` hop could NOT resolve a declaration target.
    /// MUST fail closed: no usage-only partial.
    fn confirmed_unknown() -> ChildPropRenameClass {
        ChildPropRenameClass::Confirmed(Box::new(ConfirmedChildPropRename {
            usage: ChildPropUsage {
                parent_uri: parent_uri(),
                parent_prop_name: "foo".to_string(),
                parent_prop_name_span: verter_span::Span { start: 0, end: 3 },
                parent_is_shorthand: false,
                child_carrier_api_path: "/src/MyComp.vue.ts".to_string(),
            },
            expected_parent_usage_range: Some(parent_usage_range()),
            declaration: ChildPropDeclarationProof::Unknown,
        }))
    }

    fn edit_with(entries: Vec<(Uri, Vec<TextEdit>)>) -> WorkspaceEdit {
        let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
        for (u, edits) in entries {
            changes.insert(u, edits);
        }
        WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }
    }

    fn te(range: Range, new_text: &str) -> TextEdit {
        TextEdit {
            range,
            new_text: new_text.to_string(),
        }
    }

    // ── The load-bearing fail-closed discriminators ────────────────────────────

    #[test]
    fn confirmed_usage_only_merge_fails_closed_to_none() {
        // A CONFIRMED child-prop rename whose merged edit contains ONLY the parent
        // usage leg (the tsgo synthesis-failure shape: declaration leg dropped,
        // provider did not enumerate it). The gate MUST fail closed → None, never the
        // usage-only partial.
        //
        // DISCRIMINATING: without the gate (make `gate_cross_file_child_prop_rename`
        // always return `merged`) this would return the usage-only edit instead of
        // None.
        let merged = edit_with(vec![(
            parent_uri(),
            vec![te(parent_usage_range(), "fooRenamed")],
        )]);
        let result = gate_cross_file_child_prop_rename(
            Some(merged),
            &confirmed_known_inline(),
            "fooRenamed",
        );
        assert!(
            result.is_none(),
            "a Confirmed rename with a usage-ONLY merged edit must fail closed (None), \
             not ship a usage-only partial"
        );
    }

    #[test]
    fn confirmed_unknown_declaration_usage_only_fails_closed() {
        // The canonical imported-type gap: a CONFIRMED child-prop rename whose
        // declaration could NOT be resolved (`Unknown` — e.g. `defineProps<Imported>()`
        // whose provider get_definition hop failed) and whose merged edit is
        // usage-only MUST fail closed → None. This is the EXACT case an
        // unresolved-declaration-does-not-gate policy would leak (returning the merged
        // provider result untouched). DISCRIMINATING: against that leaky policy the
        // outcomes diverge (leaky: Some(usage-only); gated: None).
        let merged = edit_with(vec![(
            parent_uri(),
            vec![te(parent_usage_range(), "fooRenamed")],
        )]);
        let result =
            gate_cross_file_child_prop_rename(Some(merged), &confirmed_unknown(), "fooRenamed");
        assert!(
            result.is_none(),
            "a Confirmed rename with an UNKNOWN declaration must fail closed (None), never \
             ship a usage-only partial"
        );
    }

    #[test]
    fn confirmed_unknown_declaration_with_both_files_still_fails_closed() {
        // Even when the merged edit touches a SECOND file, an `Unknown` declaration
        // has no resolved target to PROVE the edit lands the declaration — so the gate
        // still fails closed. Guards against "any 2-file edit passes".
        let merged = edit_with(vec![
            (parent_uri(), vec![te(parent_usage_range(), "fooRenamed")]),
            (imported_decl_uri(), vec![te(decl_range(), "fooRenamed")]),
        ]);
        let result =
            gate_cross_file_child_prop_rename(Some(merged), &confirmed_unknown(), "fooRenamed");
        assert!(
            result.is_none(),
            "an Unknown-declaration rename has no resolved target to prove, so even a 2-file \
             merged edit must fail closed (None)"
        );
    }

    #[test]
    fn confirmed_decl_only_merge_fails_closed_to_none() {
        // A declaration-ONLY merged edit (parent usage leg missing) is ALSO
        // incomplete → fail closed.
        let merged = edit_with(vec![(decl_uri(), vec![te(decl_range(), "fooRenamed")])]);
        let result = gate_cross_file_child_prop_rename(
            Some(merged),
            &confirmed_known_inline(),
            "fooRenamed",
        );
        assert!(
            result.is_none(),
            "a Confirmed rename with a declaration-ONLY merged edit must fail closed (None)"
        );
    }

    #[test]
    fn confirmed_inline_both_legs_present_still_refuses_without_sibling_proof() {
        // Declaration + initiating usage are both present, but neither proves
        // that no sibling parent usage exists.
        let merged = edit_with(vec![
            (parent_uri(), vec![te(parent_usage_range(), "fooRenamed")]),
            (decl_uri(), vec![te(decl_range(), "fooRenamed")]),
        ]);
        let result = gate_cross_file_child_prop_rename(
            Some(merged.clone()),
            &confirmed_known_inline(),
            "fooRenamed",
        );
        assert!(
            result.is_none(),
            "declaration + initiating parent are not a complete workspace usage proof"
        );
    }

    #[test]
    fn confirmed_imported_both_legs_present_still_refuses_without_sibling_proof() {
        // A provider-native edit of the third-file declaration plus the
        // initiating usage still says nothing about sibling parents.
        let merged = edit_with(vec![
            (parent_uri(), vec![te(parent_usage_range(), "fooRenamed")]),
            (imported_decl_uri(), vec![te(decl_range(), "fooRenamed")]),
        ]);
        let result = gate_cross_file_child_prop_rename(
            Some(merged),
            &confirmed_known_imported(),
            "fooRenamed",
        );
        assert!(
            result.is_none(),
            "an imported declaration + initiating parent still lacks complete usage proof"
        );
    }

    #[test]
    fn confirmed_imported_usage_only_fails_closed() {
        // IMPORTED case, provider rename did NOT edit the resolved third-file member
        // (project membership / it renamed only the local alias) → usage-only → fail
        // closed. Even when `get_definition` resolved the declaration target, a rename
        // that does not edit it → Ok(None), never a usage-only partial.
        let merged = edit_with(vec![(
            parent_uri(),
            vec![te(parent_usage_range(), "fooRenamed")],
        )]);
        let result = gate_cross_file_child_prop_rename(
            Some(merged),
            &confirmed_known_imported(),
            "fooRenamed",
        );
        assert!(
            result.is_none(),
            "an imported-type rename whose resolved declaration is NOT edited by the provider \
             must fail closed (None), never a usage-only partial"
        );
    }

    #[test]
    fn confirmed_child_leg_from_provider_still_refuses_without_sibling_proof() {
        // A provider-supplied declaration edit is a positive location, not
        // proof that every parent usage was enumerated.
        let merged = edit_with(vec![
            (parent_uri(), vec![te(parent_usage_range(), "fooRenamed")]),
            (decl_uri(), vec![te(decl_range(), "fooRenamed")]),
        ]);
        let result = gate_cross_file_child_prop_rename(
            Some(merged),
            &confirmed_known_inline(),
            "fooRenamed",
        );
        assert!(
            result.is_none(),
            "a provider declaration leg cannot prove sibling-parent completeness"
        );
    }

    #[test]
    fn confirmed_refusal_is_independent_of_edit_span_and_text() {
        // Every `Confirmed` result is refused before span/text details could
        // authorize it. Keep representative payload variants to prove those
        // details cannot weaken the unconditional defense-in-depth refusal.
        let variants = [
            (
                "declaration wrong end",
                edit_with(vec![
                    (parent_uri(), vec![te(parent_usage_range(), "fooRenamed")]),
                    (decl_uri(), vec![te(rng(5, 11, 99), "fooRenamed")]),
                ]),
            ),
            (
                "parent usage wrong end",
                edit_with(vec![
                    (parent_uri(), vec![te(rng(3, 9, 99), "fooRenamed")]),
                    (decl_uri(), vec![te(decl_range(), "fooRenamed")]),
                ]),
            ),
            (
                "declaration wrong replacement text",
                edit_with(vec![
                    (parent_uri(), vec![te(parent_usage_range(), "fooRenamed")]),
                    (decl_uri(), vec![te(decl_range(), "WRONG")]),
                ]),
            ),
        ];
        for (label, merged) in variants {
            let result = gate_cross_file_child_prop_rename(
                Some(merged),
                &confirmed_known_inline(),
                "fooRenamed",
            );
            assert!(
                result.is_none(),
                "a Confirmed result must refuse regardless of {label}"
            );
        }
    }

    // ── NotChildProp: never over-gate a non-child rename ────────────────────────

    #[test]
    fn not_child_prop_does_not_gate_usage_only_result() {
        // A NotChildProp rename (e.g. a local binding) is NOT a confirmed cross-file
        // child-prop rename: the gate must NOT touch the provider's own merged result,
        // even a single-file one. DISCRIMINATING against an over-broad gate.
        let merged = edit_with(vec![(
            parent_uri(),
            vec![te(parent_usage_range(), "renamed")],
        )]);
        let result = gate_cross_file_child_prop_rename(
            Some(merged),
            &ChildPropRenameClass::NotChildProp,
            "renamed",
        );
        assert!(
            result.is_some(),
            "a NotChildProp rename's merged result must be returned untouched (no over-gating)"
        );
    }

    #[test]
    fn confirmed_unmappable_decl_range_fails_closed() {
        // If the declaration's range could not be computed (`Known { range: None }`),
        // the gate cannot prove the leg precisely and FAILS CLOSED — the fail-closed
        // boundary for an unmappable edit. Even a both-files-touched edit fails.
        let mut class = confirmed_known_inline();
        if let ChildPropRenameClass::Confirmed(target) = &mut class {
            if let ChildPropDeclarationProof::Known { range, .. } = &mut target.declaration {
                *range = None;
            }
        }
        let merged = edit_with(vec![
            (parent_uri(), vec![te(parent_usage_range(), "fooRenamed")]),
            (decl_uri(), vec![te(decl_range(), "fooRenamed")]),
        ]);
        let result = gate_cross_file_child_prop_rename(Some(merged), &class, "fooRenamed");
        assert!(
            result.is_none(),
            "a Confirmed rename with no precise declaration range must fail closed (None)"
        );
    }
}

/// The SAME-FILE completeness gate: the emitted transaction must overwrite every
/// authored occurrence Verter's own typed analysis proved, or the rename fails
/// closed. A non-emptiness check cannot see a partial — this can.
#[cfg(test)]
mod same_file_completeness_gate_tests {
    use super::super::nav_features_navigation_rename_gate::workspace_edit_covers_same_file_ranges;
    use std::collections::HashMap;
    use tower_lsp_server::ls_types::{Position, Range, TextEdit, Uri, WorkspaceEdit};

    fn uri(s: &str) -> Uri {
        s.parse().unwrap()
    }

    fn app() -> Uri {
        uri("file:///src/App.vue")
    }

    fn rng(line: u32, start_ch: u32, end_ch: u32) -> Range {
        Range {
            start: Position {
                line,
                character: start_ch,
            },
            end: Position {
                line,
                character: end_ch,
            },
        }
    }

    fn te(range: Range, new_text: &str) -> TextEdit {
        TextEdit {
            range,
            new_text: new_text.to_string(),
        }
    }

    #[allow(clippy::mutable_key_type)]
    fn edit_with(entries: Vec<(Uri, Vec<TextEdit>)>) -> WorkspaceEdit {
        let mut changes: HashMap<Uri, Vec<TextEdit>> = HashMap::new();
        for (uri, edits) in entries {
            changes.entry(uri).or_default().extend(edits);
        }
        WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }
    }

    /// The declaration + script usage + the two template occurrences.
    fn expected() -> Vec<Range> {
        vec![
            rng(1, 6, 11),
            rng(2, 12, 17),
            rng(6, 13, 18),
            rng(6, 23, 28),
        ]
    }

    #[test]
    fn complete_edit_set_passes() {
        let complete = edit_with(vec![(
            app(),
            expected().into_iter().map(|r| te(r, "renamed")).collect(),
        )]);
        assert!(workspace_edit_covers_same_file_ranges(
            Some(&complete),
            &app(),
            &expected(),
            "renamed"
        ));
    }

    #[test]
    fn script_only_subset_fails_closed() {
        // The exact shipped defect: the declaration + the script usage, with both
        // template occurrences missing. Non-empty, so a non-emptiness check passes
        // it; this gate must not.
        let partial = edit_with(vec![(
            app(),
            vec![te(rng(1, 6, 11), "renamed"), te(rng(2, 12, 17), "renamed")],
        )]);
        assert!(
            !workspace_edit_covers_same_file_ranges(Some(&partial), &app(), &expected(), "renamed"),
            "a 2-of-4 edit set is a partial that corrupts the template — it must not pass"
        );
    }

    #[test]
    fn the_same_ranges_in_a_different_file_do_not_cover_this_one() {
        // Ranges are proved PER FILE: an identical edit set recorded against
        // another document leaves this file's occurrences untouched.
        let elsewhere = edit_with(vec![(
            uri("file:///src/Other.vue"),
            expected().into_iter().map(|r| te(r, "renamed")).collect(),
        )]);
        assert!(
            !workspace_edit_covers_same_file_ranges(
                Some(&elsewhere),
                &app(),
                &expected(),
                "renamed"
            ),
            "edits in another file cannot satisfy this file's occurrence proof"
        );
    }

    #[test]
    fn wrong_new_text_at_an_expected_range_fails_closed() {
        let mut edits: Vec<TextEdit> = expected().into_iter().map(|r| te(r, "renamed")).collect();
        edits[3] = te(rng(6, 23, 28), "SOMETHING_ELSE");
        let wrong = edit_with(vec![(app(), edits)]);
        assert!(
            !workspace_edit_covers_same_file_ranges(Some(&wrong), &app(), &expected(), "renamed"),
            "an edit at the right range writing the wrong text does not cover the occurrence"
        );
    }

    #[test]
    fn cross_file_additions_do_not_affect_the_same_file_proof() {
        // The provider legitimately adds edits in OTHER files; the gate constrains
        // only this file's coverage.
        let with_other_file = edit_with(vec![
            (
                app(),
                expected().into_iter().map(|r| te(r, "renamed")).collect(),
            ),
            (
                uri("file:///src/Other.vue"),
                vec![te(rng(9, 0, 5), "renamed")],
            ),
        ]);
        assert!(workspace_edit_covers_same_file_ranges(
            Some(&with_other_file),
            &app(),
            &expected(),
            "renamed"
        ));
    }

    #[test]
    fn no_edit_at_all_fails_closed_when_occurrences_were_proved() {
        assert!(
            !workspace_edit_covers_same_file_ranges(None, &app(), &expected(), "renamed"),
            "proved occurrences with no emitted edit is the emptiest partial of all"
        );
    }

    #[test]
    fn nothing_proved_imposes_no_constraint() {
        // A cursor with no Verter-owned occurrence set (a provider-only cross-file
        // rename anchor) must not be gated at all.
        let provider_only = edit_with(vec![(
            uri("file:///src/Other.vue"),
            vec![te(rng(9, 0, 5), "renamed")],
        )]);
        assert!(workspace_edit_covers_same_file_ranges(
            Some(&provider_only),
            &app(),
            &[],
            "renamed"
        ));
    }
}
