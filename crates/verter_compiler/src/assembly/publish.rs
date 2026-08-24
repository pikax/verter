//! Atomic artifact-set publication. `publish` either returns EXACTLY the
//! planned artifact set with every contract-required mapping product
//! attached, or a typed [`AssemblyRefusal`] and nothing — no variant
//! carries a partial/publishable sibling artifact, and no artifact is
//! returned that was not in [`ProductPlan`].

use super::fragment::{DeclaredHelper, DeclaredImport, FragmentDialect, ValidatedFragment};
use super::plan::ProductPlan;
use crate::compile_request::ProductKind;

/// One artifact's finished composition, reported by the caller (the
/// framework-owned composer) together with the exact facts `publish` needs
/// to verify atomicity — never re-derived by scanning `code`.
///
/// `pub(crate)`: the ONLY way to mint an [`ArtifactSet`] from outside this
/// crate is [`crate::standalone::StandaloneCompiler::compile`] — no external
/// crate constructs a contribution and calls [`publish`] independently, so
/// the raw-source direct route is structurally closed.
pub(crate) struct ArtifactContribution<'a> {
    pub kind: ProductKind,
    /// Every fragment that contributed to this artifact — the declared
    /// helper/import union this artifact's own emitted imports are
    /// checked against, and the SAME collection `assemble_sequence`/
    /// `splice_into_hole` composed `code` from (not a second, separately
    /// maintained list).
    pub fragments: Vec<&'a ValidatedFragment>,
    pub code: String,
    /// The import statements the composer actually wrote into `code` —
    /// a fact the composer already knows because it is the one writing
    /// them, reported here rather than recovered by parsing `code` back.
    pub emitted_imports: Vec<DeclaredImport>,
    /// The exact dialect [`Self::code`] is written in — the SAME dialect
    /// every contributing fragment declared and validated under (a
    /// mismatch there is a producer bug, not this check's concern); used
    /// for the final-parse check below instead of a fixed permissive
    /// default.
    pub dialect: FragmentDialect,
    pub source_projection_map: Option<String>,
    pub runtime_source_map: Option<String>,
}

/// A published artifact. Fields are private — the only mint site is
/// [`publish`]'s own atomicity checks, so a caller cannot hand-build a
/// value that LOOKS validated without actually going through them (the
/// same sealing discipline [`ValidatedFragment`](super::fragment::ValidatedFragment)
/// already holds for a fragment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledArtifact {
    kind: ProductKind,
    code: String,
    dialect: FragmentDialect,
    source_projection_map: Option<String>,
    runtime_source_map: Option<String>,
}

impl AssembledArtifact {
    pub fn kind(&self) -> ProductKind {
        self.kind
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    /// The exact ECMAScript/TypeScript dialect [`Self::code`] is written in
    /// — the same [`FragmentDialect`] its publishing
    /// [`ArtifactContribution::dialect`] declared. A caller with no other
    /// route to this fact (a raw-source direct-core consumer has no
    /// [`crate::parser::types::ParsedSfc`] of its own to re-derive it from)
    /// reads it here rather than re-parsing the carrier a second time.
    pub fn dialect(&self) -> FragmentDialect {
        self.dialect
    }

    pub fn source_projection_map(&self) -> Option<&str> {
        self.source_projection_map.as_deref()
    }

    pub fn runtime_source_map(&self) -> Option<&str> {
        self.runtime_source_map.as_deref()
    }
}

/// No `Default` — an `ArtifactSet` exists only as [`publish`]'s return
/// value. A publishable-looking empty set that never went through
/// `publish`'s atomicity checks would be indistinguishable from a
/// genuinely empty (zero-product) publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactSet {
    artifacts: Vec<AssembledArtifact>,
}

impl ArtifactSet {
    pub fn artifacts(&self) -> &[AssembledArtifact] {
        &self.artifacts
    }

    pub fn artifact(&self, kind: ProductKind) -> Option<&AssembledArtifact> {
        self.artifacts.iter().find(|a| a.kind == kind)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssemblyRefusal {
    /// The plan named an artifact no contribution was supplied for.
    MissingPlannedArtifact { kind: ProductKind },
    /// A contribution was supplied for an artifact the plan never
    /// requested.
    UnplannedArtifactProduced { kind: ProductKind },
    /// An `IdeCompanion` (or any artifact whose plan entry requires one)
    /// was composed without its non-optional `SourceProjectionMap`.
    MissingRequiredSourceProjectionMap { kind: ProductKind },
    /// A `SourceProjectionMap` was attached to an artifact the plan did
    /// not mark as requiring one — never a default-constructed extra.
    UnrequestedSourceProjectionMap { kind: ProductKind },
    /// A runtime product requested `runtime_source_map` but its
    /// contribution carries none.
    MissingRequiredRuntimeSourceMap { kind: ProductKind },
    /// A `RuntimeSourceMapData` was attached without having been
    /// requested.
    UnrequestedRuntimeSourceMap { kind: ProductKind },
    /// An artifact emits an import no contributing fragment declared.
    UndeclaredHelper {
        kind: ProductKind,
        specifier: String,
        name: String,
    },
    /// More than one contribution was supplied for the same planned
    /// [`ProductKind`] — "exactly the planned set" means output
    /// cardinality matches the plan exactly, not "at least" it.
    DuplicateArtifactContribution { kind: ProductKind },
    /// A code-bearing artifact's final composed bytes do not parse as a
    /// complete ECMAScript/TypeScript module.
    FinalParseFailed { kind: ProductKind, reason: String },
}

/// Product kinds whose [`ArtifactContribution::code`] is required to be a
/// real ECMAScript/TypeScript module — the ones [`publish`]'s final-parse
/// check applies to. `PublicApi`/`Analysis` may carry a non-code payload
/// (facts/serialized data), so they are not parsed as JS here.
fn is_code_bearing(kind: ProductKind) -> bool {
    matches!(
        kind,
        ProductKind::RuntimeClient
            | ProductKind::RuntimeServer
            | ProductKind::IdeCompanion
            | ProductKind::Declarations
    )
}

fn declared_names<'a>(fragments: &[&'a ValidatedFragment], specifier: &str) -> Vec<&'a str> {
    fragments
        .iter()
        .flat_map(|fragment| {
            let f = fragment.fragment();
            let from_imports = f
                .imports
                .iter()
                .filter(|i: &&DeclaredImport| i.specifier == specifier)
                .flat_map(|i: &DeclaredImport| i.bound_names());
            let from_helpers = f.helpers.iter().map(|h: &DeclaredHelper| h.name.as_str());
            from_imports.chain(from_helpers)
        })
        .collect()
}

/// Validate `contributions` against `plan` and publish atomically. Every
/// check below runs to completion against `contributions` before any
/// [`ArtifactSet`] is constructed — a failure anywhere returns exactly one
/// [`AssemblyRefusal`] and builds no artifact at all.
///
/// `pub(crate)`: [`crate::assembly::vue_module::compose_main_module`] and
/// [`crate::standalone::StandaloneCompiler::compile`] are this crate's only
/// callers — no legacy alternate core outside `verter_compiler` may publish
/// an [`ArtifactSet`] directly.
pub(crate) fn publish(
    plan: &ProductPlan,
    contributions: Vec<ArtifactContribution<'_>>,
) -> Result<ArtifactSet, AssemblyRefusal> {
    // Exact cardinality: two contributions for the same kind would let one
    // shadow the other in the published set (`ArtifactSet::artifact` finds
    // the FIRST match) while the second's own atomicity checks silently
    // ran and passed for nothing — "exactly the planned set" means output
    // cardinality matches the plan exactly, never "at least."
    let mut seen_kinds = std::collections::HashSet::new();
    for contribution in &contributions {
        if !seen_kinds.insert(contribution.kind) {
            return Err(AssemblyRefusal::DuplicateArtifactContribution {
                kind: contribution.kind,
            });
        }
    }

    for planned in plan.artifacts() {
        if !contributions.iter().any(|c| c.kind == planned.kind) {
            return Err(AssemblyRefusal::MissingPlannedArtifact { kind: planned.kind });
        }
    }
    for contribution in &contributions {
        let Some(planned) = plan.artifact(contribution.kind) else {
            return Err(AssemblyRefusal::UnplannedArtifactProduced {
                kind: contribution.kind,
            });
        };

        match (
            planned.requires_source_projection_map,
            &contribution.source_projection_map,
        ) {
            (true, None) => {
                return Err(AssemblyRefusal::MissingRequiredSourceProjectionMap {
                    kind: contribution.kind,
                })
            }
            (false, Some(_)) => {
                return Err(AssemblyRefusal::UnrequestedSourceProjectionMap {
                    kind: contribution.kind,
                })
            }
            _ => {}
        }

        match (
            planned.requires_runtime_source_map,
            &contribution.runtime_source_map,
        ) {
            (true, None) => {
                return Err(AssemblyRefusal::MissingRequiredRuntimeSourceMap {
                    kind: contribution.kind,
                })
            }
            (false, Some(_)) => {
                return Err(AssemblyRefusal::UnrequestedRuntimeSourceMap {
                    kind: contribution.kind,
                })
            }
            _ => {}
        }

        for emitted in &contribution.emitted_imports {
            let declared: Vec<&str> = declared_names(&contribution.fragments, &emitted.specifier);
            for name in emitted.bound_names() {
                if !declared.contains(&name) {
                    return Err(AssemblyRefusal::UndeclaredHelper {
                        kind: contribution.kind,
                        specifier: emitted.specifier.clone(),
                        name: name.to_string(),
                    });
                }
            }
        }

        // Final assembly must parse as its declared ECMAScript/TypeScript
        // module — checked here, once, on the fully-composed bytes, not
        // inferred from any contributing fragment individually having
        // parsed. `contribution.dialect`, not a fixed permissive default —
        // a JS artifact must reject TypeScript-only syntax rather than
        // silently accept it under TSX.
        if is_code_bearing(contribution.kind) {
            if let Some(reason) =
                super::fragment::final_module_parse_errors(&contribution.code, contribution.dialect)
            {
                return Err(AssemblyRefusal::FinalParseFailed {
                    kind: contribution.kind,
                    reason,
                });
            }
        }
    }

    let artifacts = contributions
        .into_iter()
        .map(|c| AssembledArtifact {
            kind: c.kind,
            code: c.code,
            dialect: c.dialect,
            source_projection_map: c.source_projection_map,
            runtime_source_map: c.runtime_source_map,
        })
        .collect();
    Ok(ArtifactSet { artifacts })
}

#[cfg(test)]
#[path = "publish_tests.rs"]
mod tests;
