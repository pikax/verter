//! Workspace-owned resolver driving and configuration ingress.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[cfg(test)]
use std::cell::Cell;

use crate::canonical_path::CanonicalPath;
use crate::membership::configured_membership_match_all_under_root;
#[cfg(test)]
use crate::membership::ProjectMembership;
use verter_semantic::resolver_core::{
    AttemptFailure, AttemptOutcome, AttemptOutput, ConsumedResolutionObservationKey,
    IdeProjectCompilerOptions, IdeProjectConfig, InputKey, InputLoadIntegrityReason,
    InputResolutionBudgetExhaustion, InputResolutionBudgetMeter, InputResolutionBudgets,
    InputResolutionRetention, KernelAttempt, LoadSet, ModuleResolverCore, ResolutionBasis,
    ResolutionContext, ResolutionObservationSnapshot, ResolutionPackageManifest, ResolveRequest,
    ResolveResult, ResolverAttemptView, ResolverObservationKind,
};

/// One exact bounded preflight entry. Payload-bearing package content is not
/// present here; only its authoritative reservation is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolutionInputReservation {
    PathProbe {
        key: InputKey,
        value: verter_semantic::resolver_core::PathProbe,
        directories: Vec<String>,
    },
    RealPath {
        key: InputKey,
        value: Option<String>,
        directories: Vec<String>,
    },
    PackageManifest {
        key: InputKey,
        manifest_path: String,
        present: bool,
        raw_bytes: u64,
        directories: Vec<String>,
    },
}

impl ResolutionInputReservation {
    #[must_use]
    pub fn key(&self) -> &InputKey {
        match self {
            Self::PathProbe { key, .. }
            | Self::RealPath { key, .. }
            | Self::PackageManifest { key, .. } => key,
        }
    }

    fn matches_key_variant(&self) -> bool {
        matches!(
            (self, self.key()),
            (Self::PathProbe { .. }, InputKey::PathProbe { .. })
                | (Self::RealPath { .. }, InputKey::RealPath { .. })
                | (
                    Self::PackageManifest { .. },
                    InputKey::PackageManifest { .. }
                )
        )
    }

    fn reserved_bytes(&self) -> Option<u64> {
        let metadata = match self {
            Self::PathProbe { directories, .. } | Self::RealPath { directories, .. } => {
                spelling_bytes(directories.iter().map(String::as_str))?
            }
            Self::PackageManifest {
                manifest_path,
                directories,
                ..
            } => spelling_bytes(
                directories
                    .iter()
                    .map(String::as_str)
                    .chain(std::iter::once(manifest_path.as_str())),
            )?,
        };
        let payload = match self {
            Self::PathProbe { .. } => 1,
            Self::RealPath { value, .. } => value.as_ref().map_or(0, |value| value.len() as u64),
            Self::PackageManifest { raw_bytes, .. } => *raw_bytes,
        };
        metadata.checked_add(payload)
    }
}

/// Exact normalized keys and basis reserved by bounded preflight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolutionInputReservationBatch {
    keys: Vec<InputKey>,
    basis: ResolutionBasis,
    entries: Vec<ResolutionInputReservation>,
    reserved_bytes: u64,
}

impl ResolutionInputReservationBatch {
    pub fn new(
        keys: Vec<InputKey>,
        basis: ResolutionBasis,
        entries: Vec<ResolutionInputReservation>,
    ) -> Option<Self> {
        let reserved_bytes = checked_reservation_byte_total(
            entries
                .iter()
                .map(ResolutionInputReservation::reserved_bytes),
        )?;
        Some(Self {
            keys,
            basis,
            entries,
            reserved_bytes,
        })
    }

    #[must_use]
    pub fn keys(&self) -> &[InputKey] {
        &self.keys
    }

    #[must_use]
    pub const fn basis(&self) -> ResolutionBasis {
        self.basis
    }

    #[must_use]
    pub const fn reserved_bytes(&self) -> u64 {
        self.reserved_bytes
    }

    #[must_use]
    pub fn entries(&self) -> &[ResolutionInputReservation] {
        &self.entries
    }

    #[cfg(test)]
    pub(crate) fn with_reserved_bytes_for_test(mut self, reserved_bytes: u64) -> Self {
        self.reserved_bytes = reserved_bytes;
        self
    }
}

/// One value returned by the bounded load phase.
#[derive(Debug, Clone)]
pub enum LoadedResolutionInput {
    PathProbe {
        key: InputKey,
        value: verter_semantic::resolver_core::PathProbe,
        directories: Vec<String>,
    },
    RealPath {
        key: InputKey,
        value: Option<String>,
        directories: Vec<String>,
    },
    PackageManifest {
        key: InputKey,
        value: Option<crate::types::PackageManifest>,
        manifest_path: String,
        directories: Vec<String>,
    },
}

impl LoadedResolutionInput {
    #[must_use]
    pub fn key(&self) -> &InputKey {
        match self {
            Self::PathProbe { key, .. }
            | Self::RealPath { key, .. }
            | Self::PackageManifest { key, .. } => key,
        }
    }

    fn matches_key_variant(&self) -> bool {
        matches!(
            (self, self.key()),
            (Self::PathProbe { .. }, InputKey::PathProbe { .. })
                | (Self::RealPath { .. }, InputKey::RealPath { .. })
                | (
                    Self::PackageManifest { .. },
                    InputKey::PackageManifest { .. }
                )
        )
    }

    fn actual_bytes(&self) -> Option<u64> {
        let metadata = match self {
            Self::PathProbe { directories, .. } | Self::RealPath { directories, .. } => {
                spelling_bytes(directories.iter().map(String::as_str))?
            }
            Self::PackageManifest {
                manifest_path,
                directories,
                ..
            } => spelling_bytes(
                directories
                    .iter()
                    .map(String::as_str)
                    .chain(std::iter::once(manifest_path.as_str())),
            )?,
        };
        let payload = match self {
            Self::PathProbe { .. } => 1,
            Self::RealPath { value, .. } => value.as_ref().map_or(0, |value| value.len() as u64),
            Self::PackageManifest { value, .. } => value
                .as_ref()
                .and_then(|manifest| manifest.raw.as_ref())
                .map_or(0, |raw| raw.len() as u64),
        };
        metadata.checked_add(payload)
    }
}

/// Complete output of one bounded load flight.
#[derive(Debug, Clone)]
pub struct LoadedResolutionInputBatch {
    keys: Vec<InputKey>,
    basis: ResolutionBasis,
    entries: Vec<LoadedResolutionInput>,
    actual_bytes: u64,
    complete: bool,
}

impl LoadedResolutionInputBatch {
    pub fn new(
        keys: Vec<InputKey>,
        basis: ResolutionBasis,
        entries: Vec<LoadedResolutionInput>,
        complete: bool,
    ) -> Option<Self> {
        let actual_bytes =
            checked_actual_byte_total(entries.iter().map(LoadedResolutionInput::actual_bytes))?;
        Some(Self {
            keys,
            basis,
            entries,
            actual_bytes,
            complete,
        })
    }

    #[must_use]
    pub fn keys(&self) -> &[InputKey] {
        &self.keys
    }

    #[must_use]
    pub const fn basis(&self) -> ResolutionBasis {
        self.basis
    }

    #[must_use]
    pub const fn actual_bytes(&self) -> u64 {
        self.actual_bytes
    }

    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.complete
    }

    #[must_use]
    pub fn entries(&self) -> &[LoadedResolutionInput] {
        &self.entries
    }
}

pub(crate) fn checked_reservation_byte_total(
    charges: impl IntoIterator<Item = Option<u64>>,
) -> Option<u64> {
    charges
        .into_iter()
        .try_fold(0_u64, |total, charge| total.checked_add(charge?))
}

pub(crate) fn checked_actual_byte_total(
    charges: impl IntoIterator<Item = Option<u64>>,
) -> Option<u64> {
    charges
        .into_iter()
        .try_fold(0_u64, |total, charge| total.checked_add(charge?))
}

fn spelling_bytes<'a>(mut spellings: impl Iterator<Item = &'a str>) -> Option<u64> {
    spellings.try_fold(0_u64, |sum, spelling| {
        sum.checked_add(spelling.len() as u64)
    })
}

pub(crate) fn unsupported_input_failure(key: &InputKey) -> Option<AttemptFailure> {
    let observation = match key {
        InputKey::FileContent { .. } => ResolverObservationKind::WholeHash,
        InputKey::DeclBody { space, .. } => match space {
            verter_semantic::resolver_core::DeclarationSpace::Type => {
                ResolverObservationKind::TypeDecl
            }
            verter_semantic::resolver_core::DeclarationSpace::Value => {
                ResolverObservationKind::ValueDecl
            }
        },
        InputKey::ModuleAugmentationIndex { .. } => {
            ResolverObservationKind::ModuleAugmentationIndex
        }
        InputKey::FlowFunctionSkeleton { .. } => ResolverObservationKind::FunctionBodySkeleton,
        InputKey::PathProbe { .. }
        | InputKey::RealPath { .. }
        | InputKey::PackageManifest { .. } => return None,
    };
    Some(AttemptFailure::ObservationUnavailable { observation })
}

pub(crate) fn preflight_supported_resolution_inputs(
    keys: &[InputKey],
    basis: ResolutionBasis,
    mut path_probe: impl FnMut(
        &str,
    ) -> Result<
        (verter_semantic::resolver_core::PathProbe, Vec<String>),
        AttemptFailure,
    >,
    mut realpath: impl FnMut(&str) -> Result<(Option<String>, Vec<String>), AttemptFailure>,
    mut package_manifest: impl FnMut(
        &str,
        &InputKey,
    ) -> Result<(bool, u64, Vec<String>), AttemptFailure>,
) -> Result<ResolutionInputReservationBatch, AttemptFailure> {
    let mut entries = Vec::with_capacity(keys.len());
    for key in keys {
        let entry = match key {
            InputKey::PathProbe { path } => {
                let (value, directories) = path_probe(path)?;
                ResolutionInputReservation::PathProbe {
                    key: key.clone(),
                    value,
                    directories,
                }
            }
            InputKey::RealPath { path } => {
                let (value, directories) = realpath(path)?;
                ResolutionInputReservation::RealPath {
                    key: key.clone(),
                    value,
                    directories,
                }
            }
            InputKey::PackageManifest { directory } => {
                let manifest_path =
                    verter_semantic::resolver_core::join_paths(directory, "package.json");
                let (present, raw_bytes, directories) = package_manifest(&manifest_path, key)?;
                ResolutionInputReservation::PackageManifest {
                    key: key.clone(),
                    manifest_path,
                    present,
                    raw_bytes,
                    directories,
                }
            }
            _ => {
                return Err(unsupported_input_failure(key)
                    .expect("the supported variants were matched exhaustively"));
            }
        };
        entries.push(entry);
    }
    ResolutionInputReservationBatch::new(keys.to_vec(), basis, entries).ok_or_else(|| {
        AttemptFailure::InputResolutionByteLimit {
            unresolved: keys.to_vec(),
            bytes: u64::MAX,
        }
    })
}

pub(crate) fn load_supported_resolution_inputs(
    reservation: &ResolutionInputReservationBatch,
    mut package_manifest: impl FnMut(
        &str,
        bool,
        u64,
        &InputKey,
    )
        -> Result<Option<crate::types::PackageManifest>, AttemptFailure>,
) -> Result<LoadedResolutionInputBatch, AttemptFailure> {
    let mut entries = Vec::with_capacity(reservation.entries().len());
    for entry in reservation.entries() {
        entries.push(match entry {
            ResolutionInputReservation::PathProbe {
                key,
                value,
                directories,
            } => LoadedResolutionInput::PathProbe {
                key: key.clone(),
                value: *value,
                directories: directories.clone(),
            },
            ResolutionInputReservation::RealPath {
                key,
                value,
                directories,
            } => LoadedResolutionInput::RealPath {
                key: key.clone(),
                value: value.clone(),
                directories: directories.clone(),
            },
            ResolutionInputReservation::PackageManifest {
                key,
                manifest_path,
                present,
                raw_bytes,
                directories,
            } => LoadedResolutionInput::PackageManifest {
                key: key.clone(),
                value: package_manifest(manifest_path, *present, *raw_bytes, key)?,
                manifest_path: manifest_path.clone(),
                directories: directories.clone(),
            },
        });
    }
    LoadedResolutionInputBatch::new(
        reservation.keys().to_vec(),
        reservation.basis(),
        entries,
        true,
    )
    .ok_or_else(|| AttemptFailure::InputLoadIntegrity {
        unresolved: reservation.keys().to_vec(),
        reason: InputLoadIntegrityReason::ActualOverReservation,
    })
}

#[cfg(test)]
thread_local! {
    static DRIVER_TERMINAL_KEY_COPIES: Cell<usize> = const { Cell::new(0) };
    static DRIVER_DELTA_MATERIALIZATIONS: Cell<usize> = const { Cell::new(0) };
    static INPUT_RESOLUTION_BUDGET_EVENTS: std::cell::RefCell<Vec<InputResolutionBudgetExhaustion>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
pub(crate) fn reset_resolution_driver_churn_for_test() {
    DRIVER_TERMINAL_KEY_COPIES.set(0);
    DRIVER_DELTA_MATERIALIZATIONS.set(0);
}

#[cfg(test)]
pub(crate) fn take_input_resolution_budget_events_for_test() -> Vec<InputResolutionBudgetExhaustion>
{
    INPUT_RESOLUTION_BUDGET_EVENTS.take()
}

#[cfg(test)]
pub(crate) fn resolution_driver_churn_for_test() -> (usize, usize) {
    (
        DRIVER_TERMINAL_KEY_COPIES.get(),
        DRIVER_DELTA_MATERIALIZATIONS.get(),
    )
}

/// Config-ingress constructor for the semantic-owned project DTO.
#[must_use]
pub fn ide_project_config(
    root: String,
    workspace_root: String,
    tsconfig_path: Option<String>,
) -> IdeProjectConfig {
    let provider_root = root.clone();
    let membership = configured_membership_match_all_under_root(&CanonicalPath::new(&root));
    IdeProjectConfig {
        root,
        workspace_root,
        tsconfig_path,
        provider_root,
        workspace_aliases: Vec::new(),
        compiler_options: IdeProjectCompilerOptions::default(),
        references: Vec::new(),
        membership,
    }
}

#[derive(Default)]
pub(crate) struct ResolutionInputs {
    snapshot: Arc<ResolutionObservationSnapshot>,
    observation_inputs: HashMap<InputKey, SnapshotInput>,
}

#[cfg(test)]
impl ResolutionInputs {
    pub(crate) fn metadata_key_shares_input_arc_for_test(&self, key: &InputKey) -> bool {
        self.observation_inputs
            .keys()
            .find(|stored| *stored == key)
            .is_some_and(|stored| input_key_arc_ptr_eq(stored, key))
    }
}

#[cfg(test)]
fn input_key_arc_ptr_eq(left: &InputKey, right: &InputKey) -> bool {
    match (left, right) {
        (InputKey::PathProbe { path: left }, InputKey::PathProbe { path: right })
        | (InputKey::RealPath { path: left }, InputKey::RealPath { path: right }) => {
            Arc::ptr_eq(left, right)
        }
        (
            InputKey::PackageManifest { directory: left },
            InputKey::PackageManifest { directory: right },
        ) => Arc::ptr_eq(left, right),
        _ => false,
    }
}

enum SnapshotInput {
    PathProbe {
        directories: Vec<String>,
    },
    RealPath {
        directories: Vec<String>,
    },
    PackageManifest {
        fingerprint: Option<[u8; 16]>,
        directories: Vec<String>,
        manifest_path: String,
    },
}

fn attempt_view(
    inputs: &ResolutionInputs,
    basis: ResolutionBasis,
    budgets: InputResolutionBudgets,
    retention: &InputResolutionRetention,
) -> ResolverAttemptView {
    ResolverAttemptView::from_resolution_snapshot_with_operation_retention(
        Arc::clone(&inputs.snapshot),
        basis,
        budgets,
        retention.clone(),
    )
}

#[cfg(test)]
fn load_requested_inputs(
    inputs: &mut ResolutionInputs,
    keys: &[InputKey],
    mut snapshot_path_probe: impl FnMut(
        &str,
    ) -> (verter_semantic::resolver_core::PathProbe, Vec<String>),
    mut snapshot_realpath: impl FnMut(&str) -> (Option<String>, Vec<String>),
    mut snapshot_package_manifest: impl FnMut(
        &str,
    )
        -> (Option<crate::types::PackageManifest>, Vec<String>),
) -> Result<bool, Box<InputKey>> {
    let mut progressed = false;
    for key in keys {
        match key {
            InputKey::PathProbe { path } => {
                let (value, directories) = snapshot_path_probe(path);
                progressed |= !inputs.snapshot.contains_path_probe(path);
                Arc::make_mut(&mut inputs.snapshot).insert_path_probe(path.to_string(), value);
                inputs
                    .observation_inputs
                    .insert(key.clone(), SnapshotInput::PathProbe { directories });
            }
            InputKey::RealPath { path } => {
                let (value, directories) = snapshot_realpath(path);
                progressed |= !inputs.snapshot.contains_real_path(path);
                Arc::make_mut(&mut inputs.snapshot)
                    .insert_real_path(path.to_string(), value.map(Arc::from));
                inputs
                    .observation_inputs
                    .insert(key.clone(), SnapshotInput::RealPath { directories });
            }
            InputKey::PackageManifest { directory } => {
                let manifest_path =
                    verter_semantic::resolver_core::join_paths(directory, "package.json");
                let (manifest, directories) = snapshot_package_manifest(&manifest_path);
                let fingerprint = manifest
                    .as_ref()
                    .map(crate::resolution_currency::manifest_fingerprint_of);
                let value = manifest.map(|manifest| {
                    Arc::new(ResolutionPackageManifest {
                        main: manifest.main,
                        module: manifest.module,
                        types: manifest.types,
                        typings: manifest.typings,
                        exports: manifest.exports,
                        imports: manifest.imports,
                    })
                });
                progressed |= !inputs.snapshot.contains_package_manifest(directory);
                Arc::make_mut(&mut inputs.snapshot)
                    .insert_package_manifest(directory.to_string(), value);
                inputs.observation_inputs.insert(
                    key.clone(),
                    SnapshotInput::PackageManifest {
                        fingerprint,
                        directories,
                        manifest_path,
                    },
                );
            }
            _ => return Err(Box::new(key.clone())),
        }
    }
    Ok(progressed)
}

#[cfg(test)]
pub(crate) fn load_requested_workspace_inputs(
    reader: &dyn crate::traits::WorkspaceRead,
    inputs: &mut ResolutionInputs,
    keys: &[InputKey],
) -> Result<bool, Box<InputKey>> {
    load_requested_inputs(
        inputs,
        keys,
        |path| (reader.probe_path(path), Vec::new()),
        |path| (reader.realpath(path), Vec::new()),
        |path| (reader.read_package_manifest(path), Vec::new()),
    )
}

#[cfg(test)]
pub(crate) fn preflight_workspace_inputs_for_test(
    reader: &dyn crate::traits::WorkspaceRead,
    keys: &[InputKey],
    basis: ResolutionBasis,
) -> Result<ResolutionInputReservationBatch, AttemptFailure> {
    preflight_supported_resolution_inputs(
        keys,
        basis,
        |path| {
            let _ = reader.take_resolution_directory_observations();
            let value = reader.probe_path(path);
            Ok((value, reader.take_resolution_directory_observations()))
        },
        |path| {
            let _ = reader.take_resolution_directory_observations();
            let value = reader.realpath(path);
            Ok((value, reader.take_resolution_directory_observations()))
        },
        |manifest_path, _| {
            let _ = reader.take_resolution_directory_observations();
            let manifest = reader.read_package_manifest(manifest_path);
            Ok((
                manifest.is_some(),
                manifest
                    .as_ref()
                    .and_then(|manifest| manifest.raw.as_ref())
                    .map_or(0, |raw| raw.len() as u64),
                reader.take_resolution_directory_observations(),
            ))
        },
    )
}

#[cfg(test)]
pub(crate) fn load_workspace_inputs_for_test(
    reader: &dyn crate::traits::WorkspaceRead,
    reservation: &ResolutionInputReservationBatch,
) -> Result<LoadedResolutionInputBatch, AttemptFailure> {
    load_supported_resolution_inputs(
        reservation,
        |manifest_path, expected_present, reserved_raw_bytes, key| {
            let manifest = reader.read_package_manifest(manifest_path);
            if manifest.is_some() != expected_present {
                return Err(AttemptFailure::InputLoadIntegrity {
                    unresolved: vec![key.clone()],
                    reason: InputLoadIntegrityReason::IncompleteBoundedCapture,
                });
            }
            if manifest
                .as_ref()
                .and_then(|manifest| manifest.raw.as_ref())
                .is_some_and(|raw| raw.len() as u64 > reserved_raw_bytes)
            {
                return Err(AttemptFailure::InputLoadIntegrity {
                    unresolved: vec![key.clone()],
                    reason: InputLoadIntegrityReason::ActualOverReservation,
                });
            }
            Ok(manifest)
        },
    )
}

fn apply_attempt_output(
    reader: &crate::resolution_currency::TransactionReader<'_>,
    inputs: &ResolutionInputs,
    output: &AttemptOutput,
) -> bool {
    verter_debug_assert::verter_debug_assert!(output.observed_facts().is_empty());
    verter_debug_assert::verter_debug_assert!(output.ambient_dependencies().is_empty());

    for observation in output.consumed_resolution_observations() {
        match observation {
            ConsumedResolutionObservationKey::PathProbe { path } => {
                let key = InputKey::PathProbe {
                    path: Arc::clone(path),
                };
                let Some(SnapshotInput::PathProbe { directories }) =
                    inputs.observation_inputs.get(&key)
                else {
                    return false;
                };
                let Some(value) = inputs.snapshot.path_probe(path) else {
                    return false;
                };
                reader.record_snapshot_directories(directories);
                reader.record_snapshot_canonical_path(path, value);
            }
            ConsumedResolutionObservationKey::RealPath { path } => {
                let key = InputKey::RealPath {
                    path: Arc::clone(path),
                };
                let Some(SnapshotInput::RealPath { directories }) =
                    inputs.observation_inputs.get(&key)
                else {
                    return false;
                };
                let Some(value) = inputs.snapshot.real_path(path) else {
                    return false;
                };
                reader.record_snapshot_directories(directories);
                reader.record_snapshot_canonical_realpath(path, value.as_deref());
            }
            ConsumedResolutionObservationKey::PackageManifest { directory } => {
                let key = InputKey::PackageManifest {
                    directory: Arc::clone(directory),
                };
                let Some(SnapshotInput::PackageManifest {
                    fingerprint,
                    directories,
                    manifest_path,
                }) = inputs.observation_inputs.get(&key)
                else {
                    return false;
                };
                reader.record_snapshot_directories(directories);
                reader.record_snapshot_canonical_manifest(manifest_path, *fingerprint);
            }
            ConsumedResolutionObservationKey::RecoveryScope { canonical_prefix } => {
                reader.record_snapshot_canonical_recovery_scope(canonical_prefix);
            }
        }
    }
    true
}

#[cfg(test)]
fn canonical_manifest_path(directory: &str) -> String {
    if directory.is_empty() {
        "/package.json".to_string()
    } else if directory.ends_with('/') {
        format!("{directory}package.json")
    } else {
        format!("{directory}/package.json")
    }
}

pub(crate) fn drive_attempt<T>(
    reader: &dyn crate::traits::WorkspaceRead,
    ledger: &mut InputResolutionLedger,
    mut apply_attempt_output: impl FnMut(&ResolutionInputs, &AttemptOutput) -> bool,
    mut run: impl FnMut(&ResolverAttemptView, ResolutionBasis) -> KernelAttempt<T>,
) -> Result<T, Box<AttemptFailure>> {
    let result = drive_attempt_with_bounded_io(
        reader,
        ledger,
        |keys, basis| reader.preflight_resolution_inputs_bounded(keys, basis),
        |reservation| reader.load_preflighted_resolution_inputs(reservation),
        &mut apply_attempt_output,
        &mut run,
    );
    if result.is_err() {
        ledger.release_applied_outputs();
    }
    result
}

pub(crate) struct InputResolutionLedger {
    budgets: InputResolutionBudgets,
    attempts: u32,
    unique_keys: HashSet<InputKey>,
    bytes: u64,
    depth: u32,
    churn: u32,
    last_load_set: Option<LoadSet>,
    staged_loaded_inputs: Vec<LoadedResolutionInput>,
    applied_outputs: Vec<AttemptOutput>,
    retention: InputResolutionRetention,
}

impl InputResolutionLedger {
    pub(crate) fn new(budgets: InputResolutionBudgets) -> Self {
        Self {
            budgets,
            attempts: 0,
            unique_keys: HashSet::new(),
            bytes: 0,
            depth: 0,
            churn: 0,
            last_load_set: None,
            staged_loaded_inputs: Vec::new(),
            applied_outputs: Vec::new(),
            retention: InputResolutionRetention::new(budgets),
        }
    }

    pub(crate) fn charge_churn(
        &mut self,
        reader: &dyn crate::traits::WorkspaceRead,
        unresolved: &[InputKey],
    ) -> Result<(), Box<AttemptFailure>> {
        self.staged_loaded_inputs.clear();
        let prospective = self.churn.checked_add(1).unwrap_or(u32::MAX);
        if prospective > self.budgets.churn() {
            return Err(limit_failure(
                reader,
                InputResolutionBudgetMeter::Churn,
                u64::from(self.churn),
                u64::from(self.churn) + 1,
                u64::from(self.budgets.churn()),
                AttemptFailure::InputResolutionChurnLimit {
                    unresolved: copy_terminal_keys(unresolved),
                    churn: self.churn,
                },
            ));
        }
        self.churn = prospective;
        Ok(())
    }

    pub(crate) fn charge_outer_restart(
        &mut self,
        reader: &dyn crate::traits::WorkspaceRead,
    ) -> Result<(), Box<AttemptFailure>> {
        self.applied_outputs.clear();
        let unresolved = self
            .last_load_set
            .as_ref()
            .map_or_else(Vec::new, |load_set| load_set.keys().to_vec());
        self.charge_churn(reader, &unresolved)
    }

    pub(crate) fn commit_loaded_inputs(&mut self, reader: &dyn crate::traits::WorkspaceRead) {
        reader.commit_loaded_resolution_inputs(&self.staged_loaded_inputs);
        self.staged_loaded_inputs.clear();
    }

    pub(crate) fn release_applied_outputs(&mut self) {
        self.applied_outputs.clear();
    }

    #[cfg(test)]
    pub(crate) fn applied_output_count_for_test(&self) -> usize {
        self.applied_outputs.len()
    }

    /// Drop pass-local payloads and the actual completed outputs applied by a
    /// ReturnOnly discovery pass. The validated replay owns fresh outputs;
    /// retaining discovery output after discard would retain witnesses with no
    /// live transaction.
    pub(crate) fn discard_staged_loaded_inputs(&mut self) {
        self.staged_loaded_inputs.clear();
        self.release_applied_outputs();
    }

    fn ensure_transient_retry_fits(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        unresolved: &[InputKey],
        reservation_bytes: u64,
    ) -> Result<(), Box<AttemptFailure>> {
        let prospective_attempts = self.attempts.checked_add(1).map(u64::from);
        if prospective_attempts.is_none_or(|attempts| attempts > u64::from(self.budgets.attempts()))
        {
            return Err(limit_failure(
                reader,
                InputResolutionBudgetMeter::Attempts,
                u64::from(self.attempts),
                prospective_attempts.unwrap_or(u64::MAX),
                u64::from(self.budgets.attempts()),
                AttemptFailure::InputResolutionAttemptLimit {
                    unresolved: copy_terminal_keys(unresolved),
                    attempts: self.attempts,
                },
            ));
        }

        let prospective_bytes = self.bytes.checked_add(reservation_bytes);
        if prospective_bytes.is_none_or(|bytes| bytes > self.budgets.input_bytes()) {
            return Err(limit_failure(
                reader,
                InputResolutionBudgetMeter::InputBytes,
                self.bytes,
                prospective_bytes.unwrap_or(u64::MAX),
                self.budgets.input_bytes(),
                AttemptFailure::InputResolutionByteLimit {
                    unresolved: copy_terminal_keys(unresolved),
                    bytes: self.bytes,
                },
            ));
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn consumed_for_test(&self) -> (u32, usize, u64, u32, u32) {
        (
            self.attempts,
            self.unique_keys.len(),
            self.bytes,
            self.depth,
            self.churn,
        )
    }
}

impl Default for InputResolutionLedger {
    fn default() -> Self {
        Self::new(InputResolutionBudgets::default())
    }
}

pub(crate) fn drive_attempt_with_bounded_io<T>(
    reader: &dyn crate::traits::WorkspaceRead,
    ledger: &mut InputResolutionLedger,
    mut preflight: impl FnMut(
        &[InputKey],
        ResolutionBasis,
    ) -> Result<ResolutionInputReservationBatch, AttemptFailure>,
    mut load: impl FnMut(
        &ResolutionInputReservationBatch,
    ) -> Result<LoadedResolutionInputBatch, AttemptFailure>,
    mut apply_attempt_output: impl FnMut(&ResolutionInputs, &AttemptOutput) -> bool,
    mut run: impl FnMut(&ResolverAttemptView, ResolutionBasis) -> KernelAttempt<T>,
) -> Result<T, Box<AttemptFailure>> {
    let mut active_basis = None;
    let mut inputs = ResolutionInputs::default();
    let mut requested = Vec::new();
    let mut delta = Vec::new();
    let mut last_load_set: Option<LoadSet> = None;

    loop {
        let basis = crate::resolution_currency::resolution_basis_for_reader(reader)
            .unwrap_or_else(ResolutionBasis::unbound_placeholder);
        if active_basis != Some(basis) {
            if active_basis.is_some() {
                ledger.charge_churn(reader, last_load_set.as_ref().map_or(&[], LoadSet::keys))?;
            }
            active_basis = Some(basis);
            inputs = ResolutionInputs::default();
            requested.clear();
            last_load_set = None;
        }
        let prospective_attempts = ledger.attempts.checked_add(1).unwrap_or(u32::MAX);
        if prospective_attempts > ledger.budgets.attempts() {
            return Err(limit_failure(
                reader,
                InputResolutionBudgetMeter::Attempts,
                u64::from(ledger.attempts),
                u64::from(ledger.attempts) + 1,
                u64::from(ledger.budgets.attempts()),
                AttemptFailure::InputResolutionAttemptLimit {
                    unresolved: last_load_set
                        .as_ref()
                        .map_or_else(Vec::new, |load_set| copy_terminal_keys(load_set.keys())),
                    attempts: ledger.attempts,
                },
            ));
        }
        ledger.attempts = prospective_attempts;
        let view = attempt_view(&inputs, basis, ledger.budgets, &ledger.retention);
        let outcome = run(&view, basis);
        drop(view);
        match outcome {
            AttemptOutcome::Complete(completed) => {
                #[cfg(test)]
                crate::engine::resolution_test_hooks::fire(
                    crate::engine::resolution_test_hooks::ResolutionPhase::ProviderProjection,
                );
                if !apply_attempt_output(&inputs, &completed.output) {
                    return Err(Box::new(AttemptFailure::InputResolutionNoProgress {
                        unresolved: last_load_set
                            .as_ref()
                            .map_or_else(Vec::new, |load_set| copy_terminal_keys(load_set.keys())),
                    }));
                }
                let verter_semantic::resolver_core::CompletedAttempt { value, output } = completed;
                ledger.applied_outputs.push(output);
                ledger.last_load_set = last_load_set;
                return Ok(value);
            }
            AttemptOutcome::NeedInputs(load_set) => {
                if load_set.basis() != basis {
                    ledger.charge_churn(reader, load_set.keys())?;
                    last_load_set = Some(load_set);
                    active_basis = None;
                    continue;
                }
                delta.clear();
                #[cfg(test)]
                let had_delta_capacity = delta.capacity() != 0;
                delta.extend(
                    load_set
                        .keys()
                        .iter()
                        .filter(|key| !requested.contains(*key))
                        .cloned(),
                );
                #[cfg(test)]
                if !had_delta_capacity && !delta.is_empty() {
                    DRIVER_DELTA_MATERIALIZATIONS.set(DRIVER_DELTA_MATERIALIZATIONS.get() + 1);
                }
                if delta.is_empty() {
                    return Err(Box::new(AttemptFailure::InputResolutionNoProgress {
                        unresolved: copy_terminal_keys(load_set.keys()),
                    }));
                }

                if let Some(failure) = delta.iter().find_map(unsupported_input_failure) {
                    reader.note_input_resolution_terminal_failure();
                    return Err(Box::new(failure));
                }

                let prospective_depth = ledger.depth.checked_add(1).unwrap_or(u32::MAX);
                if prospective_depth > ledger.budgets.driver_depth() {
                    return Err(limit_failure(
                        reader,
                        InputResolutionBudgetMeter::DriverDepth,
                        u64::from(ledger.depth),
                        u64::from(ledger.depth) + 1,
                        u64::from(ledger.budgets.driver_depth()),
                        AttemptFailure::InputResolutionDepthLimit {
                            unresolved: copy_terminal_keys(&delta),
                            depth: ledger.depth,
                        },
                    ));
                }

                let mut new_keys = Vec::new();
                let mut new_key_bytes = 0_u64;
                for key in &delta {
                    if !ledger.unique_keys.contains(key) {
                        new_keys.push(key.clone());
                        new_key_bytes = new_key_bytes
                            .checked_add(input_key_spelling_bytes(key))
                            .unwrap_or(u64::MAX);
                    }
                }
                let prospective_unique = ledger
                    .unique_keys
                    .len()
                    .checked_add(new_keys.len())
                    .and_then(|value| u32::try_from(value).ok())
                    .unwrap_or(u32::MAX);
                if prospective_unique > ledger.budgets.unique_keys() {
                    return Err(limit_failure(
                        reader,
                        InputResolutionBudgetMeter::UniqueKeys,
                        ledger.unique_keys.len() as u64,
                        ledger.unique_keys.len() as u64 + new_keys.len() as u64,
                        u64::from(ledger.budgets.unique_keys()),
                        AttemptFailure::InputResolutionUniqueKeyLimit {
                            unresolved: copy_terminal_keys(&delta),
                            unique_keys: u32::try_from(ledger.unique_keys.len())
                                .unwrap_or(u32::MAX),
                        },
                    ));
                }
                let prospective_key_bytes = ledger.bytes.checked_add(new_key_bytes);
                if prospective_key_bytes.is_none_or(|bytes| bytes > ledger.budgets.input_bytes()) {
                    return Err(limit_failure(
                        reader,
                        InputResolutionBudgetMeter::InputBytes,
                        ledger.bytes,
                        prospective_key_bytes.unwrap_or(u64::MAX),
                        ledger.budgets.input_bytes(),
                        AttemptFailure::InputResolutionByteLimit {
                            unresolved: copy_terminal_keys(&delta),
                            bytes: ledger.bytes,
                        },
                    ));
                }
                ledger.unique_keys.extend(new_keys);
                ledger.bytes = prospective_key_bytes.expect("checked above");

                let reservation = match preflight(&delta, basis) {
                    Ok(reservation) => reservation,
                    Err(AttemptFailure::TransientInputLoadFailure { .. }) => {
                        // Preflight has not produced a payload/metadata
                        // reservation yet, so only the next kernel attempt
                        // needs proving here. The repeated preflight must
                        // still produce a bounded reservation, which is
                        // charged before its load below.
                        ledger.ensure_transient_retry_fits(reader, &delta, 0)?;
                        last_load_set = Some(load_set);
                        continue;
                    }
                    Err(failure) => {
                        reader.note_input_resolution_terminal_failure();
                        return Err(Box::new(failure));
                    }
                };
                if reservation.keys() != delta.as_slice()
                    || reservation.basis() != basis
                    || reservation.entries().len() != delta.len()
                    || !reservation
                        .entries()
                        .iter()
                        .zip(delta.iter())
                        .all(|(entry, key)| entry.matches_key_variant() && entry.key() == key)
                {
                    reader.note_input_load_integrity_failure();
                    return Err(Box::new(AttemptFailure::InputLoadIntegrity {
                        unresolved: copy_terminal_keys(&delta),
                        reason: if reservation.basis() != basis {
                            InputLoadIntegrityReason::BasisMismatch
                        } else {
                            InputLoadIntegrityReason::KeySetMismatch
                        },
                    }));
                }
                let prospective_bytes = ledger.bytes.checked_add(reservation.reserved_bytes());
                if prospective_bytes.is_none_or(|bytes| bytes > ledger.budgets.input_bytes()) {
                    return Err(limit_failure(
                        reader,
                        InputResolutionBudgetMeter::InputBytes,
                        ledger.bytes,
                        prospective_bytes.unwrap_or(u64::MAX),
                        ledger.budgets.input_bytes(),
                        AttemptFailure::InputResolutionByteLimit {
                            unresolved: copy_terminal_keys(&delta),
                            bytes: ledger.bytes,
                        },
                    ));
                }
                ledger.bytes = prospective_bytes.expect("checked above");

                let loaded = match load(&reservation) {
                    Ok(loaded) => loaded,
                    Err(AttemptFailure::TransientInputLoadFailure { .. }) => {
                        ledger.ensure_transient_retry_fits(
                            reader,
                            &delta,
                            reservation.reserved_bytes(),
                        )?;
                        last_load_set = Some(load_set);
                        continue;
                    }
                    Err(failure @ AttemptFailure::InputLoadIntegrity { .. }) => {
                        reader.note_input_load_integrity_failure();
                        return Err(Box::new(failure));
                    }
                    Err(failure) => {
                        reader.note_input_resolution_terminal_failure();
                        return Err(Box::new(failure));
                    }
                };
                let integrity_reason = if loaded.basis() != basis {
                    Some(InputLoadIntegrityReason::BasisMismatch)
                } else if loaded.keys() != delta.as_slice()
                    || loaded.entries().len() != delta.len()
                    || !loaded
                        .entries()
                        .iter()
                        .zip(delta.iter())
                        .all(|(entry, key)| entry.matches_key_variant() && entry.key() == key)
                {
                    Some(InputLoadIntegrityReason::KeySetMismatch)
                } else if !loaded.is_complete() {
                    Some(InputLoadIntegrityReason::IncompleteBoundedCapture)
                } else if loaded.actual_bytes() > reservation.reserved_bytes() {
                    Some(InputLoadIntegrityReason::ActualOverReservation)
                } else {
                    None
                };
                if let Some(reason) = integrity_reason {
                    reader.note_input_load_integrity_failure();
                    return Err(Box::new(AttemptFailure::InputLoadIntegrity {
                        unresolved: copy_terminal_keys(&delta),
                        reason,
                    }));
                }
                ledger
                    .staged_loaded_inputs
                    .extend(loaded.entries().iter().cloned());
                let progressed = apply_loaded_resolution_inputs(&mut inputs, loaded);
                if progressed {
                    requested.append(&mut delta);
                    ledger.depth = prospective_depth;
                }
                last_load_set = Some(load_set);
            }
            AttemptOutcome::Terminal(failure) => match failure {
                AttemptFailure::InputResolutionAliasGeometryRetentionLimit {
                    retained,
                    prospective,
                    maximum,
                } => {
                    return Err(limit_failure(
                        reader,
                        InputResolutionBudgetMeter::AliasGeometryRetention,
                        u64::from(retained),
                        u64::from(prospective),
                        u64::from(maximum),
                        AttemptFailure::InputResolutionAliasGeometryRetentionLimit {
                            retained,
                            prospective,
                            maximum,
                        },
                    ));
                }
                AttemptFailure::InputResolutionCompletedWitnessRetentionLimit {
                    retained,
                    prospective,
                    maximum,
                } => {
                    return Err(limit_failure(
                        reader,
                        InputResolutionBudgetMeter::CompletedWitnessRetention,
                        u64::from(retained),
                        u64::from(prospective),
                        u64::from(maximum),
                        AttemptFailure::InputResolutionCompletedWitnessRetentionLimit {
                            retained,
                            prospective,
                            maximum,
                        },
                    ));
                }
                failure if failure.is_input_resolution_limit() => {
                    reader.note_resolution_budget_exhausted();
                    return Err(Box::new(failure));
                }
                failure => return Err(Box::new(failure)),
            },
        }
    }
}

fn limit_failure(
    reader: &dyn crate::traits::WorkspaceRead,
    meter: InputResolutionBudgetMeter,
    consumed: u64,
    prospective: u64,
    maximum: u64,
    failure: AttemptFailure,
) -> Box<AttemptFailure> {
    let event = InputResolutionBudgetExhaustion {
        meter,
        consumed,
        prospective,
        maximum,
    };
    #[cfg(test)]
    INPUT_RESOLUTION_BUDGET_EVENTS.with_borrow_mut(|events| events.push(event));
    reader.note_input_resolution_budget_exhausted(event);
    Box::new(failure)
}

fn input_key_spelling_bytes(key: &InputKey) -> u64 {
    match key {
        InputKey::PathProbe { path } | InputKey::RealPath { path } => path.len() as u64,
        InputKey::PackageManifest { directory } => directory.len() as u64,
        _ => 0,
    }
}

fn apply_loaded_resolution_inputs(
    inputs: &mut ResolutionInputs,
    loaded: LoadedResolutionInputBatch,
) -> bool {
    let mut progressed = false;
    for entry in loaded.entries {
        match entry {
            LoadedResolutionInput::PathProbe {
                key,
                value,
                directories,
            } => {
                let InputKey::PathProbe { path } = &key else {
                    return false;
                };
                progressed |= !inputs.snapshot.contains_path_probe(path);
                Arc::make_mut(&mut inputs.snapshot).insert_path_probe(path.to_string(), value);
                inputs
                    .observation_inputs
                    .insert(key, SnapshotInput::PathProbe { directories });
            }
            LoadedResolutionInput::RealPath {
                key,
                value,
                directories,
            } => {
                let InputKey::RealPath { path } = &key else {
                    return false;
                };
                progressed |= !inputs.snapshot.contains_real_path(path);
                Arc::make_mut(&mut inputs.snapshot)
                    .insert_real_path(path.to_string(), value.map(Arc::from));
                inputs
                    .observation_inputs
                    .insert(key, SnapshotInput::RealPath { directories });
            }
            LoadedResolutionInput::PackageManifest {
                key,
                value,
                manifest_path,
                directories,
            } => {
                let InputKey::PackageManifest { directory } = &key else {
                    return false;
                };
                let fingerprint = value
                    .as_ref()
                    .map(crate::resolution_currency::manifest_fingerprint_of);
                let value = value.map(|manifest| {
                    Arc::new(ResolutionPackageManifest {
                        main: manifest.main,
                        module: manifest.module,
                        types: manifest.types,
                        typings: manifest.typings,
                        exports: manifest.exports,
                        imports: manifest.imports,
                    })
                });
                progressed |= !inputs.snapshot.contains_package_manifest(directory);
                Arc::make_mut(&mut inputs.snapshot)
                    .insert_package_manifest(directory.to_string(), value);
                inputs.observation_inputs.insert(
                    key,
                    SnapshotInput::PackageManifest {
                        fingerprint,
                        directories,
                        manifest_path,
                    },
                );
            }
        }
    }
    progressed
}

fn copy_terminal_keys(keys: &[InputKey]) -> Vec<InputKey> {
    #[cfg(test)]
    DRIVER_TERMINAL_KEY_COPIES.set(DRIVER_TERMINAL_KEY_COPIES.get() + keys.len());
    keys.to_vec()
}

pub(crate) fn resolve_tracked(
    resolver: &ModuleResolverCore,
    _capability: &crate::engine::TrackedResolutionCapability,
    reader: &crate::resolution_currency::TransactionReader<'_>,
    ledger: &mut InputResolutionLedger,
    request: &ResolveRequest,
) -> Result<Option<ResolveResult>, Box<AttemptFailure>> {
    let frame = resolver.resolve_frame(request);
    let result = drive_attempt(
        reader,
        ledger,
        |inputs, output| apply_attempt_output(reader, inputs, output),
        |view, basis| frame.attempt(view, basis),
    );
    match result {
        Err(failure)
            if failure.is_input_resolution_limit()
                || matches!(
                    failure.as_ref(),
                    AttemptFailure::InputLoadIntegrity { .. }
                        | AttemptFailure::ObservationUnavailable { .. }
                ) =>
        {
            Ok(None)
        }
        result => result,
    }
}

pub(crate) fn resolve_for_project_tracked(
    resolver: &ModuleResolverCore,
    _capability: &crate::engine::TrackedResolutionCapability,
    reader: &crate::resolution_currency::TransactionReader<'_>,
    ledger: &mut InputResolutionLedger,
    owner: &verter_semantic::resolver_core::ProjectOwnership,
    specifier: &str,
    context: ResolutionContext,
) -> Result<Option<ResolveResult>, Box<AttemptFailure>> {
    let frame = resolver.resolve_for_project_frame(owner, specifier, context);
    let result = drive_attempt(
        reader,
        ledger,
        |inputs, output| apply_attempt_output(reader, inputs, output),
        |view, basis| frame.attempt(view, basis),
    );
    match result {
        Err(failure)
            if failure.is_input_resolution_limit()
                || matches!(
                    failure.as_ref(),
                    AttemptFailure::InputLoadIntegrity { .. }
                        | AttemptFailure::ObservationUnavailable { .. }
                ) =>
        {
            Ok(None)
        }
        result => result,
    }
}

#[cfg(test)]
trait ModuleResolverCoreTestExt {
    fn resolve_with_reader(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        request: &ResolveRequest,
    ) -> Option<ResolveResult>;

    fn resolve_for_project_with_reader(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        owner: &verter_semantic::resolver_core::ProjectOwnership,
        specifier: &str,
        context: ResolutionContext,
    ) -> Option<ResolveResult>;
}

#[cfg(test)]
impl ModuleResolverCoreTestExt for ModuleResolverCore {
    fn resolve_with_reader(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        request: &ResolveRequest,
    ) -> Option<ResolveResult> {
        let frame = self.resolve_frame(request);
        let mut ledger = InputResolutionLedger::default();
        drive_attempt_with_bounded_io(
            reader,
            &mut ledger,
            |keys, basis| preflight_workspace_inputs_for_test(reader, keys, basis),
            |reservation| load_workspace_inputs_for_test(reader, reservation),
            |_, _| true,
            |view, basis| frame.attempt(view, basis),
        )
        .unwrap_or_else(|failure| {
            if failure.is_input_resolution_limit() {
                None
            } else {
                panic!("resolution driver failed unexpectedly: {failure:?}")
            }
        })
    }

    fn resolve_for_project_with_reader(
        &self,
        reader: &dyn crate::traits::WorkspaceRead,
        owner: &verter_semantic::resolver_core::ProjectOwnership,
        specifier: &str,
        context: ResolutionContext,
    ) -> Option<ResolveResult> {
        let frame = self.resolve_for_project_frame(owner, specifier, context);
        let mut ledger = InputResolutionLedger::default();
        drive_attempt_with_bounded_io(
            reader,
            &mut ledger,
            |keys, basis| preflight_workspace_inputs_for_test(reader, keys, basis),
            |reservation| load_workspace_inputs_for_test(reader, reservation),
            |_, _| true,
            |view, basis| frame.attempt(view, basis),
        )
        .unwrap_or_else(|failure| {
            if failure.is_input_resolution_limit() {
                None
            } else {
                panic!("resolution driver failed unexpectedly: {failure:?}")
            }
        })
    }
}

#[cfg(test)]
#[path = "resolver_tests.rs"]
mod resolver_tests;
