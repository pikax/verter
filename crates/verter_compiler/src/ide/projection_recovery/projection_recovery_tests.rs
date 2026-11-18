//! Recovery state for script regions and the authored `generic` binder.

use super::*;

/// The checking text of `source` (which starts at carrier offset `base`)
/// after applying recovery's replacements.
fn checking_text(source: &str, base: u32, replacements: &[(u32, u32, String)]) -> String {
    let mut sorted: Vec<&(u32, u32, String)> = replacements.iter().collect();
    sorted.sort_by_key(|(start, end, _)| (*start, *end));
    let mut out = String::new();
    let mut cursor = 0usize;
    for (start, end, text) in sorted {
        let start = (*start - base) as usize;
        let end = (*end - base) as usize;
        out.push_str(&source[cursor..start]);
        out.push_str(text);
        cursor = end;
    }
    out.push_str(&source[cursor..]);
    out
}

fn setup_block(content: &str, content_start: u32) -> RecoveryBlock<'_> {
    RecoveryBlock {
        region: ProjectionRegion::InstanceScope,
        content,
        content_start,
        tsx: false,
    }
}

fn contribution(
    state: &IncompleteProjectionState,
    region: ProjectionRegion,
) -> &ProjectionRecoveryContribution {
    state
        .contributions
        .iter()
        .find(|contribution| contribution.region == region)
        .expect("recovered region contributes")
}

#[test]
fn clean_script_regions_are_complete_and_contribute_nothing() {
    let module = "export const shared = 1\n";
    let setup = "\nconst count = shared + 1\n";
    let state = recover_script_regions(
        &[
            RecoveryBlock {
                region: ProjectionRegion::ModuleScope,
                content: module,
                content_start: 10,
                tsx: false,
            },
            setup_block(setup, 80),
        ],
        Some(("T extends string", 60)),
    );
    assert!(state.is_complete());
    assert!(state.may_warm_complete_cache());
    assert!(state.contributions.is_empty());
    assert_eq!(
        state.region(ProjectionRegion::InstanceScope),
        Some(&RegionCompleteness {
            region: ProjectionRegion::InstanceScope,
            state: RegionState::Complete,
            start: 80,
            end: 80 + setup.len() as u32,
        })
    );
    assert_eq!(
        state
            .region(ProjectionRegion::GenericBinder)
            .map(|r| r.state),
        Some(RegionState::Complete)
    );
}

#[test]
fn dangling_member_access_recovers_with_the_receiver_kept() {
    let base = 50;
    let setup = "\nconst user = { name: 'Ada' }\nuser.\n";
    let state = recover_script_regions(&[setup_block(setup, base)], None);

    let region = state
        .region(ProjectionRegion::InstanceScope)
        .expect("setup region");
    assert_eq!(region.state, RegionState::Recovered);
    assert_eq!(
        (region.start, region.end),
        (base, base + setup.len() as u32)
    );
    assert!(!state.is_complete());

    let recovered = contribution(&state, ProjectionRegion::InstanceScope);
    assert!(!recovered.diagnostics.is_empty());
    for diagnostic in &recovered.diagnostics {
        assert!(base <= diagnostic.start && diagnostic.end <= region.end);
        assert!(!diagnostic.message.is_empty());
    }

    // The member placeholder sits right after `user.`; no replacement touches
    // the receiver's bytes.
    let dot_end = base + setup.find("user.").unwrap() as u32 + "user.".len() as u32;
    assert!(recovered
        .replacements
        .iter()
        .any(|(start, end, text)| *start == dot_end && *end == dot_end && !text.is_empty()));
    assert!(recovered
        .replacements
        .iter()
        .all(|(start, _, _)| *start >= dot_end));
    let text = checking_text(setup, base, &recovered.replacements);
    assert!(
        text.contains("user.valueOf"),
        "receiver keeps its member access: {text}"
    );
    // The recovered text ends at a statement boundary.
    assert!(text.trim_end().ends_with(';'), "{text}");
}

#[test]
fn open_brackets_close_at_the_end_of_the_recovered_region() {
    let setup = "\nconst run = () => {\n  call(\n";
    let state = recover_script_regions(&[setup_block(setup, 0)], None);
    let recovered = contribution(&state, ProjectionRegion::InstanceScope);
    let text = checking_text(setup, 0, &recovered.replacements);
    assert!(text.ends_with(")};"), "{text}");
}

#[test]
fn a_recovered_block_leaves_the_clean_block_complete() {
    let module = "export const shared = 1\n";
    let state = recover_script_regions(
        &[
            RecoveryBlock {
                region: ProjectionRegion::ModuleScope,
                content: module,
                content_start: 0,
                tsx: false,
            },
            setup_block("const a = shared +\n", 40),
        ],
        None,
    );
    assert_eq!(
        state.region(ProjectionRegion::ModuleScope).map(|r| r.state),
        Some(RegionState::Complete)
    );
    assert_eq!(
        state
            .region(ProjectionRegion::InstanceScope)
            .map(|r| r.state),
        Some(RegionState::Recovered)
    );
    assert_eq!(state.contributions.len(), 1);
    assert!(!state.syntax_diagnostics().is_empty());
}

#[test]
fn a_block_parses_under_its_own_grammar() {
    // An angle-bracket assertion is TypeScript, not TSX; a JSX element is TSX.
    let ts = recover_script_regions(&[setup_block("const n = <number>value\n", 0)], None);
    assert!(ts.is_complete());
    let tsx = recover_script_regions(
        &[RecoveryBlock {
            tsx: true,
            ..setup_block("const node = <div />\n", 0)
        }],
        None,
    );
    assert!(tsx.is_complete());
}

#[test]
fn an_unfinished_generic_binder_is_recovered_not_complete() {
    let offset = 40;
    let value = " T extends";
    let state = recover_script_regions(&[setup_block("const a = 1\n", 100)], Some((value, offset)));

    let binder = state
        .region(ProjectionRegion::GenericBinder)
        .expect("binder region");
    assert_eq!(binder.state, RegionState::Recovered);
    assert_eq!(
        (binder.start, binder.end),
        (offset, offset + value.len() as u32)
    );
    assert_eq!(
        state
            .region(ProjectionRegion::InstanceScope)
            .map(|r| r.state),
        Some(RegionState::Complete)
    );
    assert!(!state.is_complete());
    assert!(!state.may_warm_complete_cache());

    let recovered = contribution(&state, ProjectionRegion::GenericBinder);
    assert!(!recovered.diagnostics.is_empty());
    for diagnostic in &recovered.diagnostics {
        assert!(offset < diagnostic.start && diagnostic.end <= binder.end);
    }
    assert_eq!(
        recovered.replacements,
        vec![(binder.start, binder.end, String::new())]
    );
}

#[test]
fn a_blank_generic_attribute_is_a_complete_empty_binder() {
    let state = recover_script_regions(&[], Some(("  ", 7)));
    assert!(state.is_complete());
    assert_eq!(
        state
            .region(ProjectionRegion::GenericBinder)
            .map(|r| r.state),
        Some(RegionState::Complete)
    );
}

#[test]
fn the_complete_state_has_no_regions_and_warms() {
    let state = IncompleteProjectionState::complete();
    assert!(state.is_complete());
    assert!(state.may_warm_complete_cache());
    assert!(state.region(ProjectionRegion::Markup).is_none());
    assert!(state.syntax_diagnostics().is_empty());
}

#[test]
fn an_omitted_region_is_incomplete() {
    let mut state = IncompleteProjectionState::complete();
    state.regions.push(RegionCompleteness {
        region: ProjectionRegion::Markup,
        state: RegionState::Omitted,
        start: 0,
        end: 10,
    });
    assert!(!state.is_complete());
    assert!(!state.may_warm_complete_cache());
}

/// The module's public items name only neutral types: every field is
/// destructured exhaustively with its type, every variant is matched
/// exhaustively, and every function is coerced to its neutral signature, so
/// adding a framework-typed field, variant or parameter stops compiling here.
#[test]
fn public_items_name_no_framework_type() {
    fn region_name(region: ProjectionRegion) -> &'static str {
        match region {
            ProjectionRegion::ModuleScope => "module",
            ProjectionRegion::InstanceScope => "instance",
            ProjectionRegion::GenericBinder => "binder",
            ProjectionRegion::Markup => "markup",
            ProjectionRegion::Style => "style",
        }
    }
    fn state_name(state: RegionState) -> &'static str {
        match state {
            RegionState::Complete => "complete",
            RegionState::Recovered => "recovered",
            RegionState::Omitted => "omitted",
        }
    }

    type State = IncompleteProjectionState;
    type Recover = fn(&[RecoveryBlock<'_>], Option<(&str, u32)>) -> State;
    let recover: Recover = recover_script_regions;
    let _: fn() -> State = State::complete;
    let _: fn(&State) -> bool = State::is_complete;
    let _: fn(&State) -> bool = State::may_warm_complete_cache;
    type RegionOf = for<'s> fn(&'s State, ProjectionRegion) -> Option<&'s RegionCompleteness>;
    type DiagnosticsOf = for<'s> fn(&'s State) -> Vec<&'s RecoveredSyntaxDiagnostic>;
    let _: RegionOf = State::region;
    let _: DiagnosticsOf = State::syntax_diagnostics;

    let RecoveryBlock {
        region,
        content,
        content_start,
        tsx,
    }: RecoveryBlock<'_> = setup_block("a.\n", 3);
    let _: (ProjectionRegion, &str, u32, bool) = (region, content, content_start, tsx);

    let IncompleteProjectionState {
        regions,
        contributions,
    } = recover(&[setup_block("a.\n", 3)], Some(("T extends", 0)));
    let regions: Vec<RegionCompleteness> = regions;
    let contributions: Vec<ProjectionRecoveryContribution> = contributions;
    let mut seen = Vec::new();
    for RegionCompleteness {
        region,
        state,
        start,
        end,
    } in regions
    {
        let _: (u32, u32) = (start, end);
        seen.push((region_name(region), state_name(state)));
    }
    assert_eq!(
        seen,
        vec![("instance", "recovered"), ("binder", "recovered")]
    );
    for ProjectionRecoveryContribution {
        region,
        diagnostics,
        replacements,
    } in contributions
    {
        let _: ProjectionRegion = region;
        let _: Vec<(u32, u32, String)> = replacements;
        for RecoveredSyntaxDiagnostic {
            start,
            end,
            message,
        } in diagnostics
        {
            let _: (u32, u32, String) = (start, end, message);
        }
    }
}
