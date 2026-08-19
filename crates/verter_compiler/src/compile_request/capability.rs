//! The compiler capability matrix — the sole table a canonical
//! [`crate::compile_request::CompileRequest`] construction/execution path
//! consults to decide whether a requested framework/backend/product
//! combination is supported, and if not, exactly which typed refusal it
//! carries.
//!
//! One variant per row of the framework conformance capability matrix.
//! Adding a matrix row means adding a variant here and extending every
//! exhaustive `match` over [`CapabilityCell`] — a silently-uncovered row is
//! a compile error, not a skipped test case.

/// One capability-matrix cell. Variant names mirror the matrix's
/// `cell_id` column exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapabilityCell {
    VueParseLocal,
    VueVdomClient,
    VueVaporClient,
    VueSsr,
    VueSsrVaporBackend,
    VueMacroLocal,
    VueMacroImported,
    VueScopedSlotted,
    VueCustomElement,
    VueTemplateOptions,
    VueAsyncSetup,
    VuePublicApi,
    VueTsc,
    VueDeclaration,
    VueCompatV2,
    VueOtherVersion,
    SveltePraseLocal,
    SvelteClientRunes,
    SvelteClientLegacy,
    SvelteServerRunes,
    SvelteServerLegacy,
    SvelteComponent,
    SvelteModule,
    SvelteSemanticCore,
    SvelteCustomElement,
    SvelteAsyncExperimental,
    SvelteHydration,
    SveltePublicApi,
    SvelteTsc,
    SvelteDeclaration,
    SvelteHmr,
    SvelteCompatApi4,
    SvelteOfficialAst,
    SvelteOtherVersion,
}

/// `target_disposition`, verbatim from `capability-matrix.tsv`. Six
/// distinct values occur in the committed matrix; a construction-time
/// request constructor only ever needs to distinguish
/// [`Self::admits_construction`], but every value is represented so the
/// table stays a faithful mirror rather than a lossy projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityDisposition {
    /// Request construction is admitted for this cell.
    Supported,
    /// A supported cell whose fulfilment additionally requires a project
    /// provider (imported types) — construction succeeds; the projection
    /// demand is a prerequisite the execution layer plans for.
    ProjectionRequired,
    /// Explicit-opt-in-only supported cell — construction is admitted, but
    /// only when the caller explicitly requests it; never enabled by an
    /// absent/unknown option defaulting on.
    ExplicitOptIn,
    /// Request construction fails closed for this cell — the typed
    /// [`crate::compile_request::CompileRequestError`] arm names it.
    UnsupportedFailClosed,
    /// The cell is not a Verter product at all (an official-only artifact
    /// shape with no established Verter product to publish it as); a
    /// request cannot ask for it because there is no field to ask with.
    NotApplicable,
    /// The cell names a framework/runtime version this compiler is not
    /// pinned to. Not a per-request option axis — the compiler is pinned
    /// to one exact version per framework, so this disposition is
    /// informational (a domain-scope fact about the pinned version), not a
    /// construction check `CompileRequest` itself performs.
    VersionIncompatible,
}

impl CapabilityCell {
    /// `target_disposition`, verbatim from `capability-matrix.tsv`.
    pub const fn disposition(self) -> CapabilityDisposition {
        use CapabilityCell::*;
        use CapabilityDisposition::*;
        match self {
            VueParseLocal => Supported,
            VueVdomClient => Supported,
            VueVaporClient => Supported,
            VueSsr => Supported,
            VueSsrVaporBackend => UnsupportedFailClosed,
            VueMacroLocal => Supported,
            VueMacroImported => ProjectionRequired,
            VueScopedSlotted => Supported,
            VueCustomElement => Supported,
            VueTemplateOptions => Supported,
            VueAsyncSetup => Supported,
            VuePublicApi => Supported,
            VueTsc => Supported,
            VueDeclaration => Supported,
            VueCompatV2 => UnsupportedFailClosed,
            VueOtherVersion => VersionIncompatible,
            SveltePraseLocal => Supported,
            SvelteClientRunes => Supported,
            SvelteClientLegacy => Supported,
            SvelteServerRunes => Supported,
            SvelteServerLegacy => Supported,
            SvelteComponent => Supported,
            SvelteModule => UnsupportedFailClosed,
            SvelteSemanticCore => Supported,
            SvelteCustomElement => Supported,
            SvelteAsyncExperimental => ExplicitOptIn,
            SvelteHydration => Supported,
            SveltePublicApi => Supported,
            SvelteTsc => Supported,
            SvelteDeclaration => Supported,
            SvelteHmr => UnsupportedFailClosed,
            SvelteCompatApi4 => UnsupportedFailClosed,
            SvelteOfficialAst => NotApplicable,
            SvelteOtherVersion => VersionIncompatible,
        }
    }

    /// Whether this cell admits request construction. `ProjectionRequired`
    /// and `ExplicitOptIn` count as admitted: the former's projection
    /// demand is a planned prerequisite, not a construction refusal; the
    /// latter is refused only by ABSENCE of the caller's explicit request,
    /// never by construction itself. `NotApplicable`/`VersionIncompatible`
    /// are not per-request refusals at all — see their doc comments.
    pub const fn admits_construction(self) -> bool {
        !matches!(
            self.disposition(),
            CapabilityDisposition::UnsupportedFailClosed
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_CELLS: [CapabilityCell; 34] = [
        CapabilityCell::VueParseLocal,
        CapabilityCell::VueVdomClient,
        CapabilityCell::VueVaporClient,
        CapabilityCell::VueSsr,
        CapabilityCell::VueSsrVaporBackend,
        CapabilityCell::VueMacroLocal,
        CapabilityCell::VueMacroImported,
        CapabilityCell::VueScopedSlotted,
        CapabilityCell::VueCustomElement,
        CapabilityCell::VueTemplateOptions,
        CapabilityCell::VueAsyncSetup,
        CapabilityCell::VuePublicApi,
        CapabilityCell::VueTsc,
        CapabilityCell::VueDeclaration,
        CapabilityCell::VueCompatV2,
        CapabilityCell::VueOtherVersion,
        CapabilityCell::SveltePraseLocal,
        CapabilityCell::SvelteClientRunes,
        CapabilityCell::SvelteClientLegacy,
        CapabilityCell::SvelteServerRunes,
        CapabilityCell::SvelteServerLegacy,
        CapabilityCell::SvelteComponent,
        CapabilityCell::SvelteModule,
        CapabilityCell::SvelteSemanticCore,
        CapabilityCell::SvelteCustomElement,
        CapabilityCell::SvelteAsyncExperimental,
        CapabilityCell::SvelteHydration,
        CapabilityCell::SveltePublicApi,
        CapabilityCell::SvelteTsc,
        CapabilityCell::SvelteDeclaration,
        CapabilityCell::SvelteHmr,
        CapabilityCell::SvelteCompatApi4,
        CapabilityCell::SvelteOfficialAst,
        CapabilityCell::SvelteOtherVersion,
    ];

    /// Non-vacuity + exact disposition-count proof against
    /// `capability-matrix.tsv`'s committed 34 rows.
    #[test]
    fn capability_matrix_disposition_counts_match_the_committed_table() {
        use CapabilityDisposition::*;
        let count = |d: CapabilityDisposition| {
            ALL_CELLS
                .iter()
                .filter(|c| std::mem::discriminant(&c.disposition()) == std::mem::discriminant(&d))
                .count()
        };
        assert_eq!(count(Supported), 24);
        assert_eq!(count(UnsupportedFailClosed), 5);
        assert_eq!(count(ProjectionRequired), 1);
        assert_eq!(count(ExplicitOptIn), 1);
        assert_eq!(count(NotApplicable), 1);
        assert_eq!(count(VersionIncompatible), 2);
    }

    #[test]
    fn unsupported_fail_closed_cells_never_admit_construction() {
        for cell in ALL_CELLS {
            let admits = cell.admits_construction();
            let is_unsupported = matches!(
                cell.disposition(),
                CapabilityDisposition::UnsupportedFailClosed
            );
            assert_eq!(
                admits, !is_unsupported,
                "{cell:?}: admits_construction() must be exactly the negation of \
                 UnsupportedFailClosed"
            );
        }
    }
}
