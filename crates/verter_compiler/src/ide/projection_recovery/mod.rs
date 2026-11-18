#![allow(dead_code)]
//! Framework-neutral recovery state for a projection built over a tolerant parse.
//!
//! No framework's template type appears in this module's signatures.
//!
//! A script region is `Complete` when its block parses cleanly under the
//! block's own grammar. Otherwise it is `Recovered`: the parser's syntax errors
//! become recovery-owned diagnostics, and the single tolerant token scan of the
//! real source ([`ScriptTokenScanner::recover_plan`]) supplies the checking
//! text inserted at its holes and at the end of the block. A `generic` binder
//! that does not parse as type parameters is `Recovered` the same way, and is
//! checked as an empty binder.

use oxc_allocator::Allocator;
use oxc_diagnostics::{LabeledSpan, OxcDiagnostic};
use oxc_span::SourceType;
use verter_parser::oxc_parse::{parse_refusal, Parser};

use crate::ide::script_recover::ScriptTokenScanner;
use crate::utils::oxc::vue::parse_generic;
use crate::utils::oxc::vue::GENERIC_WRAPPER_PREFIX;

/// A region of a projection, named without any framework's types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectionRegion {
    ModuleScope,
    InstanceScope,
    GenericBinder,
    Markup,
    Style,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionState {
    /// Projected from a clean parse.
    Complete,
    /// Projected from recovered syntax; checking context is minimal but valid.
    Recovered,
    /// Not projected.
    Omitted,
}

/// Completeness of one region of one product.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionCompleteness {
    pub region: ProjectionRegion,
    pub state: RegionState,
    /// Carrier byte range of the region.
    pub start: u32,
    pub end: u32,
}

/// A syntax diagnostic owned by recovery, separate from semantic observations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredSyntaxDiagnostic {
    pub start: u32,
    pub end: u32,
    pub message: String,
}

/// What recovery contributes for one region: the syntax diagnostics and the
/// text to check in place of the broken bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionRecoveryContribution {
    pub region: ProjectionRegion,
    pub diagnostics: Vec<RecoveredSyntaxDiagnostic>,
    /// Replacement checking text for a carrier range; the rest of the region
    /// is projected verbatim.
    pub replacements: Vec<(u32, u32, String)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IncompleteProjectionState {
    pub regions: Vec<RegionCompleteness>,
    pub contributions: Vec<ProjectionRecoveryContribution>,
}

impl IncompleteProjectionState {
    /// State of a carrier that parsed cleanly.
    #[must_use]
    pub fn complete() -> Self {
        Self::default()
    }

    /// True when every region was projected from a clean parse and recovery
    /// contributed nothing.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.contributions.is_empty()
            && self
                .regions
                .iter()
                .all(|region| region.state == RegionState::Complete)
    }

    /// Only a complete state may warm a complete-result cache.
    #[must_use]
    pub fn may_warm_complete_cache(&self) -> bool {
        self.is_complete()
    }

    #[must_use]
    pub fn region(&self, region: ProjectionRegion) -> Option<&RegionCompleteness> {
        self.regions.iter().find(|entry| entry.region == region)
    }

    #[must_use]
    pub fn syntax_diagnostics(&self) -> Vec<&RecoveredSyntaxDiagnostic> {
        self.contributions
            .iter()
            .flat_map(|contribution| contribution.diagnostics.iter())
            .collect()
    }

    /// Record `region`'s state over a carrier range, for a region whose
    /// completeness a consumer decided from its own parse (a template
    /// region the topology marks, for instance).
    pub fn mark_region(&mut self, region: ProjectionRegion, state: RegionState, range: (u32, u32)) {
        self.settle(region, state, range);
    }

    /// Checking-text insertions for the recovered script regions:
    /// `(offset, text)`. A replacement of the generic binder removes bytes
    /// and is not an insertion.
    #[must_use]
    pub fn script_insertions(&self) -> Vec<(u32, &str)> {
        self.contributions
            .iter()
            .filter(|contribution| contribution.region != ProjectionRegion::GenericBinder)
            .flat_map(|contribution| contribution.replacements.iter())
            .map(|(start, _, text)| (*start, text.as_str()))
            .collect()
    }

    fn settle(&mut self, region: ProjectionRegion, state: RegionState, (start, end): (u32, u32)) {
        self.regions.push(RegionCompleteness {
            region,
            state,
            start,
            end,
        });
    }

    fn recover(&mut self, region: ProjectionRegion, range: (u32, u32), parts: Contribution) {
        self.settle(region, RegionState::Recovered, range);
        let (diagnostics, replacements) = parts;
        self.contributions.push(ProjectionRecoveryContribution {
            region,
            diagnostics,
            replacements,
        });
    }
}

/// A recovered region's syntax diagnostics and replacements.
type Contribution = (Vec<RecoveredSyntaxDiagnostic>, Vec<(u32, u32, String)>);

/// One script-like block to recover.
#[derive(Debug, Clone, Copy)]
pub struct RecoveryBlock<'s> {
    pub region: ProjectionRegion,
    pub content: &'s str,
    pub content_start: u32,
    pub tsx: bool,
}

/// Recover the script regions of a carrier. Consumes the existing tolerant
/// recovery in `ide::script_recover`; adds no second recovery snapshot.
///
/// `generic` is the authored `generic` attribute value and its carrier offset.
pub fn recover_script_regions(
    blocks: &[RecoveryBlock<'_>],
    generic: Option<(&str, u32)>,
) -> IncompleteProjectionState {
    let mut state = IncompleteProjectionState::complete();
    for block in blocks {
        recover_block(block, &mut state);
    }
    if let Some((value, offset)) = generic {
        recover_binder(value, offset, &mut state);
    }
    state
}

fn recover_block(block: &RecoveryBlock<'_>, state: &mut IncompleteProjectionState) {
    let range = (
        block.content_start,
        end_of(block.content_start, block.content),
    );
    let source_type = if block.tsx {
        SourceType::tsx()
    } else {
        SourceType::ts()
    };
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, block.content, source_type).parse();
    if parse_refusal(&parsed).is_some() {
        // The parse did not run: that is operational incompleteness, not the
        // source's syntax, so nothing is projected or diagnosed.
        state.settle(block.region, RegionState::Omitted, range);
        return;
    }
    if !parsed.diagnostics.has_errors() {
        state.settle(block.region, RegionState::Complete, range);
        return;
    }
    let diagnostics = syntax_diagnostics(parsed.diagnostics.errors(), 0, range);
    let plan = ScriptTokenScanner::new(block.content, block.content_start).recover_plan();
    let mut replacements: Vec<(u32, u32, String)> = plan
        .inserts
        .iter()
        .map(|insert| (insert.at(), insert.at(), insert.placeholder().to_string()))
        .collect();
    replacements.push((range.1, range.1, plan.boundary_completion()));
    state.recover(block.region, range, (diagnostics, replacements));
}

fn recover_binder(value: &str, offset: u32, state: &mut IncompleteProjectionState) {
    let range = (offset, end_of(offset, value));
    let text = value.trim();
    if text.is_empty() {
        state.settle(
            ProjectionRegion::GenericBinder,
            RegionState::Complete,
            range,
        );
        return;
    }
    // `parse_generic` parses the trimmed text behind its wrapper prefix; its
    // diagnostics index the wrapped text.
    let text_start = offset + (text.as_ptr() as usize - value.as_ptr() as usize) as u32;
    let allocator = Allocator::default();
    let parsed = parse_generic(&allocator, text, text_start);
    if parsed.is_ok() {
        state.settle(
            ProjectionRegion::GenericBinder,
            RegionState::Complete,
            range,
        );
        return;
    }
    let diagnostics = syntax_diagnostics(
        parsed.errors.iter(),
        GENERIC_WRAPPER_PREFIX.len() as u32,
        (text_start, end_of(text_start, text)),
    );
    // A binder that does not parse is checked as no binder at all.
    let replacements = vec![(range.0, range.1, String::new())];
    state.recover(
        ProjectionRegion::GenericBinder,
        range,
        (diagnostics, replacements),
    );
}

/// Recovery-owned diagnostics for parser errors over `range`'s text, whose
/// label offsets are relative to that text after `lead` wrapper bytes.
/// Offsets that fall in the wrapper are clamped into `range`.
fn syntax_diagnostics<'d>(
    errors: impl Iterator<Item = &'d OxcDiagnostic>,
    lead: u32,
    range: (u32, u32),
) -> Vec<RecoveredSyntaxDiagnostic> {
    errors
        .map(|error| {
            let (start, end) = error
                .labels
                .iter()
                .find(|label| label.primary())
                .or_else(|| error.labels.first())
                .map_or(range, |label| label_range(label, lead, range));
            RecoveredSyntaxDiagnostic {
                start,
                end,
                message: error.message.to_string(),
            }
        })
        .collect()
}

fn label_range(label: &LabeledSpan, lead: u32, (start, end): (u32, u32)) -> (u32, u32) {
    let carrier = |relative: u32| start.saturating_add(relative.saturating_sub(lead)).min(end);
    (
        carrier(label.offset()),
        carrier(label.offset().saturating_add(label.len())),
    )
}

fn end_of(start: u32, text: &str) -> u32 {
    start + u32::try_from(text.len()).unwrap_or(u32::MAX - start)
}

#[cfg(test)]
mod projection_recovery_tests;
