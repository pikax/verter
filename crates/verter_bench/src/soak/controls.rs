//! The negative controls: each plants one defect in a smoke run and requires
//! the verdict to fail for that defect's own reason and no other.
//!
//! A control bites only when its mutation was applied (recorded from what the
//! run executed, not from the plan), the run's failure kinds are exactly the
//! control's expectation, and, for the retained-key leak, the leak figure the
//! planted root causes is among the violations.

use std::path::Path;

use serde::Serialize;

use super::manifest::{LifecycleClass, Mode};
use super::run::{run, Mutation, RunOptions, SoakError, SUMMARY_FILE};
use super::verify::{verify, SoakFailure};

/// One negative control.
#[derive(Debug, Clone, Copy)]
pub struct Control {
    pub name: &'static str,
    pub mutation: Mutation,
    /// The exact set of failure kinds the planted defect produces.
    pub kinds: &'static [&'static str],
    /// A drained figure the planted defect must be among the violations of.
    pub leak_figure: Option<&'static str>,
}

/// Every control, in execution order.
#[must_use]
pub fn all() -> Vec<Control> {
    vec![
        Control {
            name: "retained-key-leak",
            mutation: Mutation::RetainedKeyLeak,
            kinds: &["ownership-broken", "retained-leak"],
            leak_figure: Some("semantic_roots"),
        },
        Control {
            name: "omitted-workload-class",
            mutation: Mutation::OmitClass(LifecycleClass::UniqueOwnerChurn),
            kinds: &["inventory-mismatch", "missing-class"],
            leak_figure: None,
        },
        Control {
            name: "omitted-cancellation",
            mutation: Mutation::OmitCancellation,
            kinds: &["cancellation-not-observed"],
            leak_figure: None,
        },
        Control {
            name: "fake-overlap",
            mutation: Mutation::FakeOverlap,
            kinds: &["fake-overlap"],
            leak_figure: None,
        },
        Control {
            name: "wrong-project-reversion",
            mutation: Mutation::WrongProjectReversion,
            // The wrong configuration is also bytes the manifest does not
            // freeze for the restore.
            kinds: &["publication-mismatch", "wrong-project-reversion"],
            leak_figure: None,
        },
        Control {
            name: "missing-summary",
            mutation: Mutation::MissingSummary,
            kinds: &["missing-summary"],
            leak_figure: None,
        },
        Control {
            name: "unexpected-skip",
            mutation: Mutation::UnexpectedSkip,
            kinds: &["unexpected-skip"],
            leak_figure: None,
        },
        Control {
            name: "unfrozen-fixture",
            mutation: Mutation::UnfrozenFixture,
            kinds: &["publication-mismatch"],
            leak_figure: None,
        },
    ]
}

/// What running one control observed.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlReport {
    pub name: &'static str,
    /// The run recorded the mutation as applied to at least one action (a
    /// withheld summary is applied when the summary file is absent).
    pub applied: bool,
    pub kinds: Vec<String>,
    pub expected_kinds: Vec<String>,
    pub leak_figure_found: bool,
    /// The planted defect failed the run for its intended reason.
    pub bit: bool,
}

/// The kind tag of a failure.
#[must_use]
pub fn kind_of(failure: &SoakFailure) -> String {
    serde_json::to_value(failure).expect("a failure serialises")["kind"]
        .as_str()
        .expect("a tagged failure")
        .to_string()
}

/// Run `control` as a one-tranche normal-mode smoke into `dir`.
pub fn execute(control: &Control, dir: &Path) -> Result<ControlReport, SoakError> {
    let summary = run(&RunOptions {
        mode: Mode::Normal,
        tranches: 1,
        out_dir: dir.to_path_buf(),
        mutation: Some(control.mutation),
    })?;
    let failures = verify(Mode::Normal, 1, dir);
    let written = dir.join(SUMMARY_FILE).exists();
    let applied = match (&summary.mutation, control.mutation) {
        (Some(applied), Mutation::MissingSummary) => {
            applied.mutation == control.mutation && !written
        }
        (Some(applied), mutation) => {
            applied.mutation == mutation && !applied.affected_actions.is_empty()
        }
        (None, _) => false,
    };
    let mut kinds: Vec<String> = failures.iter().map(kind_of).collect();
    kinds.sort();
    kinds.dedup();
    let expected_kinds: Vec<String> = control.kinds.iter().map(|k| (*k).to_string()).collect();
    let leak_figure_found = control.leak_figure.is_none_or(|figure| {
        failures.iter().any(|f| match f {
            SoakFailure::RetainedLeak { violations } => {
                violations.iter().any(|v| v.figure == figure)
            }
            _ => false,
        })
    });
    Ok(ControlReport {
        name: control.name,
        applied,
        bit: applied && kinds == expected_kinds && leak_figure_found,
        kinds,
        expected_kinds,
        leak_figure_found,
    })
}
