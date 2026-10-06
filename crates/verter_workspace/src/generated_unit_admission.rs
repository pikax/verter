//! Generated-unit admission: is a SET of off-disk generated units a member of
//! ONE configured project?
//!
//! A generated unit reaches an engine as a member of the configured project
//! that admits its MEMBERSHIP BASIS — the carrier source the unit projects
//! from, or the unit itself when it derives from no carrier. Serving a unit
//! whose basis that project does not admit roots it in an inferred project —
//! wrong compiler options, and extra load on whoever owns that engine — so
//! anything that writes generated units into an engine it does not own must
//! hold THIS proof first; observing that some ownership record names the
//! source proves nothing about the units.
//!
//! The query CONSUMES the existing membership authority — it never builds a
//! second membership model. A unit is a member when the owning project's
//! compiled [`StaticMembershipSpec`](verter_session_query::resolution::StaticMembershipSpec)
//! matches the unit DIRECTLY (`files` exact and exclude-immune; `include`
//! minus `exclude`; extension-specific globs match only their extension; the
//! JS family is present only under `allowJs`/`checkJs`), or — when the unit
//! itself matches no include — matches the unit's basis: an extension-specific
//! `src/**/*.vue` owns `Foo.vue` and thereby admits the `Foo.vue.tsx`
//! projected from it, because the engine maps a companion through its source's
//! project. A config that EXCLUDES the unit's own form (`exclude:
//! ["src/**/*.vue.tsx"]`) refuses the unit even when its basis is admitted.
//! Each unit's default configured owner is then resolved through the SAME
//! tsgo-faithful walk carrier sources use
//! ([`WorkspaceSnapshot::default_configured_owner_for_generated_unit`]), so a
//! second configured project that would win the unit is a non-admission.
//!
//! Admission is ALL-OR-NOTHING over the proposed set: one unadmitted unit
//! refuses the whole set, because a partially-written generated family is a
//! Program whose imports resolve into an inferred project.
//!
//! Every result is ordered deterministically — units are sorted and
//! de-duplicated before evaluation, and no hash-set iteration decides anything.

use std::hash::{Hash, Hasher};

use rustc_hash::FxHasher;

use crate::canonical_path::CanonicalPath;
use crate::workspace_snapshot::{ProjectPayload, WorkspaceSnapshot};
use verter_session_query::resolution::ProjectId;

/// Why ONE generated unit is not admitted to the intended configured project.
/// A closed set: every non-admission is exactly one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum GeneratedUnitNonAdmissionReason {
    /// The snapshot holds no configured project with the intended tsconfig.
    NoSuchConfiguredProject,
    /// The unit is neither a `files` entry nor matched by any `include` glob of
    /// the intended project — directly NOR through its membership basis (the
    /// carrier source it projects from; the extension-specific-include shape
    /// where neither `src/**/*.vue.tsx` nor its `src/**/*.vue` basis matches).
    NotMatchedByIncludeOrFiles,
    /// An `include` glob matches the unit (or its basis) and an `exclude` glob
    /// removes it — including a config that excludes the unit's own companion
    /// form while admitting its carrier source.
    Excluded,
    /// The intended project's spec admits the unit, but the default-owner walk
    /// resolves it to a DIFFERENT configured project — an engine would serve it
    /// under that project's options, not the intended one's.
    OwnedByDifferentProject,
}

/// The stable identity of one admission: the intended project, the sorted unit
/// set, and the membership inputs that decided it. Two admissions with equal
/// fingerprints were decided over the same project, the same units, and the same
/// determining configuration; any change to `include`/`files`/`exclude`, to the
/// unit set, or to which projects claim a unit changes it.
///
/// In-process only (it keys in-memory caches); never persisted. A separate
/// snapshot pointer on the admitted proof ties it to the immutable membership
/// snapshot for provider-hub admission; the fingerprint alone is not authority.
///
/// A fingerprint is an IDENTITY, not a proof: it names which admission a cached
/// decision was made under. The proof that units may be written is
/// [`AdmittedGeneratedUnits`], which only the admission query mints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GeneratedUnitAdmissionFingerprint(u64);

impl GeneratedUnitAdmissionFingerprint {
    /// Rebuild a fingerprint from its raw value (a cache-key dimension carried
    /// across a layer boundary).
    #[must_use]
    pub fn from_raw(value: u64) -> Self {
        Self(value)
    }

    /// The raw fingerprint value (diagnostics / logging).
    #[must_use]
    pub fn value(self) -> u64 {
        self.0
    }
}

/// The proof that EVERY unit of a proposed write set is admitted to one
/// configured project. Only [`decide_generated_unit_admission`] mints it, so
/// holding one means the query ran and every unit passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedGeneratedUnits {
    tsconfig_path: CanonicalPath,
    /// Sorted, de-duplicated.
    units: Vec<CanonicalPath>,
    fingerprint: GeneratedUnitAdmissionFingerprint,
    /// In-process identity of the immutable membership snapshot that minted
    /// this proof. A retained publication prevents address reuse while the
    /// proof is used to admit a provider write.
    snapshot_identity: usize,
}

impl AdmittedGeneratedUnits {
    /// The configured project the units are admitted to.
    #[must_use]
    pub fn tsconfig_path(&self) -> &CanonicalPath {
        &self.tsconfig_path
    }

    /// The admitted units, sorted and de-duplicated.
    #[must_use]
    pub fn units(&self) -> &[CanonicalPath] {
        &self.units
    }

    /// Whether `unit` is one of the units this proof covers.
    #[must_use]
    pub fn covers(&self, unit: &CanonicalPath) -> bool {
        self.units.binary_search(unit).is_ok()
    }

    /// The stable identity of this admission.
    #[must_use]
    pub fn fingerprint(&self) -> GeneratedUnitAdmissionFingerprint {
        self.fingerprint
    }

    #[must_use]
    pub fn snapshot_identity(&self) -> usize {
        self.snapshot_identity
    }
}

/// A refused write set: every offending unit with its reason, sorted by unit
/// path. Never empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedUnitNonAdmission {
    offending: Vec<(CanonicalPath, GeneratedUnitNonAdmissionReason)>,
}

impl GeneratedUnitNonAdmission {
    /// Every offending unit with its reason, sorted by unit path.
    #[must_use]
    pub fn offending(&self) -> &[(CanonicalPath, GeneratedUnitNonAdmissionReason)] {
        &self.offending
    }

    /// The reason of the FIRST offending unit in path order — the single
    /// deterministic reason a caller reports for the whole set.
    #[must_use]
    pub fn reason(&self) -> GeneratedUnitNonAdmissionReason {
        self.offending[0].1
    }
}

/// The outcome of [`decide_generated_unit_admission`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GeneratedUnitAdmission {
    /// Every unit is admitted to the intended configured project.
    Admitted(AdmittedGeneratedUnits),
    /// At least one unit is not.
    NotAdmitted(GeneratedUnitNonAdmission),
}

impl GeneratedUnitAdmission {
    /// The fail-closed admission for units whose owning configured project
    /// could not be resolved (no bound owner): nothing is admitted, every
    /// unit offends with [`GeneratedUnitNonAdmissionReason::NoSuchConfiguredProject`].
    /// The same answer the membership query gives for a tsconfig the snapshot
    /// does not hold; a caller that resolved NO owner uses it to fail its
    /// write closed rather than fabricate a proof.
    #[must_use]
    pub fn unresolved_owner(units: &[CanonicalPath]) -> Self {
        Self::NotAdmitted(GeneratedUnitNonAdmission {
            offending: units
                .iter()
                .map(|unit| {
                    (
                        unit.clone(),
                        GeneratedUnitNonAdmissionReason::NoSuchConfiguredProject,
                    )
                })
                .collect(),
        })
    }
}

/// Decide whether EVERY unit of `units` is admitted to the configured project
/// whose tsconfig is `owning_tsconfig`.
///
/// A unit is admitted when (a) that project's compiled membership spec matches
/// the unit directly, or — when it does not — matches the unit's membership
/// BASIS (`membership_basis_of(unit)`: the carrier source a companion projects
/// from, or the unit itself for a unit that derives from no carrier), and (b)
/// the default-owner walk over the matched path resolves it to that SAME
/// project. `units` are off-disk paths; order and duplicates in the input do
/// not affect the result. An empty `units` is vacuously admitted — a caller
/// that needs a non-empty write set checks that itself.
#[must_use]
pub fn decide_generated_unit_admission(
    snapshot: &WorkspaceSnapshot,
    owning_tsconfig: &CanonicalPath,
    units: &[CanonicalPath],
) -> GeneratedUnitAdmission {
    decide_generated_unit_admission_with_basis(snapshot, owning_tsconfig, units, |unit| {
        unit.clone()
    })
}

/// [`decide_generated_unit_admission`] with the membership-basis derivation
/// supplied by the caller: the lower workspace layer cannot see the companion
/// naming that maps a generated unit back to its carrier source, so the
/// verter_lsp call sites hand that mapping in as data (the typed neutral facts
/// the hub consumes). The proof still names and covers exactly `units`.
#[must_use]
pub fn decide_generated_unit_admission_with_basis(
    snapshot: &WorkspaceSnapshot,
    owning_tsconfig: &CanonicalPath,
    units: &[CanonicalPath],
    membership_basis_of: impl Fn(&CanonicalPath) -> CanonicalPath,
) -> GeneratedUnitAdmission {
    let mut units: Vec<CanonicalPath> = units.to_vec();
    units.sort();
    units.dedup();

    let owner = snapshot
        .projects
        .iter()
        .find_map(|project| match &project.payload {
            ProjectPayload::Configured {
                tsconfig_path,
                membership,
                ..
            } if tsconfig_path == owning_tsconfig => Some((project.id, membership)),
            _ => None,
        });
    let Some((owner_id, membership)) = owner else {
        if units.is_empty() {
            // Nothing is proposed, so nothing can be refused: a refusal always
            // names the unit that caused it.
            let mut hasher = FxHasher::default();
            owning_tsconfig.as_str().hash(&mut hasher);
            snapshot.generation.hash(&mut hasher);
            return GeneratedUnitAdmission::Admitted(AdmittedGeneratedUnits {
                tsconfig_path: owning_tsconfig.clone(),
                units,
                fingerprint: GeneratedUnitAdmissionFingerprint(hasher.finish()),
                snapshot_identity: std::ptr::from_ref(snapshot) as usize,
            });
        }
        return GeneratedUnitAdmission::NotAdmitted(GeneratedUnitNonAdmission {
            offending: units
                .into_iter()
                .map(|unit| {
                    (
                        unit,
                        GeneratedUnitNonAdmissionReason::NoSuchConfiguredProject,
                    )
                })
                .collect(),
        });
    };

    let mut hasher = FxHasher::default();
    owning_tsconfig.as_str().hash(&mut hasher);
    snapshot.generation.hash(&mut hasher);
    hash_spec(&membership.spec, &mut hasher);

    let mut offending = Vec::new();
    for unit in &units {
        let basis = membership_basis_of(unit);
        unit.as_str().hash(&mut hasher);
        basis.as_str().hash(&mut hasher);
        // Membership precedence: the unit's own form first (a config that
        // includes — or excludes — the companion path decides for itself),
        // then the carrier-source basis the engine maps the unit through.
        let (probe, refusal) = match spec_refusal(&membership.spec, unit) {
            None => (unit, None),
            Some(GeneratedUnitNonAdmissionReason::Excluded) => {
                (unit, Some(GeneratedUnitNonAdmissionReason::Excluded))
            }
            Some(GeneratedUnitNonAdmissionReason::NotMatchedByIncludeOrFiles) => {
                (&basis, spec_refusal(&membership.spec, &basis))
            }
            // `spec_refusal` never returns the remaining two variants for a
            // path the spec already refused to match.
            Some(reason) => (unit, Some(reason)),
        };
        if let Some(reason) = refusal {
            offending.push((unit.clone(), reason));
            continue;
        }
        let resolved = snapshot.default_configured_owner_for_generated_unit(probe);
        hash_owner(snapshot, resolved, &mut hasher);
        if resolved != Some(owner_id) {
            offending.push((
                unit.clone(),
                GeneratedUnitNonAdmissionReason::OwnedByDifferentProject,
            ));
        }
    }

    if offending.is_empty() {
        GeneratedUnitAdmission::Admitted(AdmittedGeneratedUnits {
            tsconfig_path: owning_tsconfig.clone(),
            units,
            fingerprint: GeneratedUnitAdmissionFingerprint(hasher.finish()),
            snapshot_identity: std::ptr::from_ref(snapshot) as usize,
        })
    } else {
        GeneratedUnitAdmission::NotAdmitted(GeneratedUnitNonAdmission { offending })
    }
}

/// Why the intended project's spec does not admit `unit`, or `None` when it
/// does. Mirrors `StaticMembershipSpec::matches` step for step so the two can
/// never disagree; it only names WHICH step refused.
fn spec_refusal(
    spec: &verter_session_query::resolution::StaticMembershipSpec,
    unit: &CanonicalPath,
) -> Option<GeneratedUnitNonAdmissionReason> {
    if spec.matches(unit) {
        return None;
    }
    // `matches` refused: `files` missed, so either no include hit or an exclude
    // removed an include hit.
    if spec.include.iter().any(|glob| glob.matches(unit)) {
        Some(GeneratedUnitNonAdmissionReason::Excluded)
    } else {
        Some(GeneratedUnitNonAdmissionReason::NotMatchedByIncludeOrFiles)
    }
}

/// Fold the membership inputs that decide admission into the fingerprint, in
/// their stored (deterministic) order.
fn hash_spec(spec: &verter_session_query::resolution::StaticMembershipSpec, hasher: &mut FxHasher) {
    spec.files.len().hash(hasher);
    for file in &spec.files {
        file.as_str().hash(hasher);
    }
    spec.include.len().hash(hasher);
    for glob in &spec.include {
        glob.as_str().hash(hasher);
    }
    spec.exclude.len().hash(hasher);
    for glob in spec.exclude.iter() {
        glob.as_str().hash(hasher);
    }
}

/// Fold the resolved owner's tsconfig identity (not its positional id, which
/// shifts when an unrelated project appears) into the fingerprint.
fn hash_owner(snapshot: &WorkspaceSnapshot, owner: Option<ProjectId>, hasher: &mut FxHasher) {
    match owner.and_then(|id| snapshot.tsconfig_path(id)) {
        Some(tsconfig) => tsconfig.as_str().hash(hasher),
        None => 0u8.hash(hasher),
    }
}

#[cfg(test)]
#[path = "generated_unit_admission_tests.rs"]
mod tests;
