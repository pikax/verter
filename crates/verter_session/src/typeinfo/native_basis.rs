//! Fact-scoped native input basis for an issued typeinfo fact.
//!
//! A [`NativeFactBasis`] is **the committed native input observations
//! supporting one fact under one captured view**, never the whole workspace
//! snapshot. It is committed at request execution, from what the request
//! actually observed:
//!
//! - every fact the traced resolution and terminal projection read, warm
//!   evidence included: a consumed result's receipt is expanded to its
//!   leaves, and a terminal domain aggregate keeps its full population and
//!   stamp ([`native_observations`]);
//! - negative probes through their producer-committed resolution key and
//!   version (a recorded absent probe is an observed fact; an unrecorded
//!   path is never assumed absent);
//! - every owner's source whole hash and source environment, observed into
//!   the trace even when the fact's semantic evidence is narrower;
//! - for every participating canonical, its effective parse / resolve /
//!   type / lib environment and project identity, read root-relative
//!   through the captured view;
//! - the captured view's effective overlay population.
//!
//! [`commit_native_fact_basis`] requires a cleanly finalised trace, checks
//! that the request's validation port over the captured view vouches for
//! every observed fact (evidence it cannot vouch for cannot authorise
//! issuance), and then calls [`InputBasis::commit`], which mints the
//! [`verter_identity::identity::InputBasisId`]. The cost is proportional to
//! the observed metadata, receipt evidence included; it performs no
//! semantic traversal and no disk read, and it never enumerates the
//! host's owners.
//!
//! [`validate_native_fact_basis`] re-checks a committed basis against a
//! later view. Success certifies the SAME basis: newly observed inputs
//! never silently replace it, and any change to a committed observation is
//! a stale basis.
//!
//! The typeinfo graph executors are this module's production callers:
//! issuance through [`crate::typeinfo::node_refs`], and expand-around
//! admission of an issued reference.

use std::sync::Arc;

use verter_analysis_inputs::{InputBasis, LoadWave, Observation, SnapshotFence};
use verter_identity::encoding::CanonicalDigest;
use verter_session_query::facts::fact_cache::{FactVersionRef, ReadSetSignature};
use verter_session_query::facts::fact_read_set::FactReadSetFinalise;
use verter_session_query::facts::fact_set::FactsRef;
use verter_session_query::facts::native_observation::{
    native_observations, view_population_bytes, NativeAuthority, NativeWriter,
};
use verter_session_query::facts::receipt::ReceiptWalk;
use verter_session_query::facts::store_view::StoreView;

use crate::resolver_store::HostStoreView;
use crate::VerterHost;

/// The committed native observations supporting one fact under one
/// captured view.
#[derive(Debug, Clone)]
pub(crate) struct NativeFactBasis {
    basis: InputBasis,
    signature: ReadSetSignature,
    scopes: Arc<[Arc<str>]>,
    fence: SnapshotFence,
}

impl NativeFactBasis {
    /// The immutable committed observations.
    pub(crate) fn basis(&self) -> &InputBasis {
        &self.basis
    }

    /// The fence bound to [`Self::basis`] at commit. Expand-around
    /// admission is its production reader.
    pub(crate) fn fence(&self) -> &SnapshotFence {
        &self.fence
    }

    /// Metadata this basis retains: every committed row (one per fact
    /// leaf, receipts expanded, so the observed evidence itself), every
    /// scope name, and the traced signature's direct entries. The receipts
    /// those entries share are charged to the retention account by their
    /// own owners.
    pub(crate) fn retained_bytes(&self) -> usize {
        let rows: usize = self
            .basis
            .observations()
            .map(|row| row.canonical().len() + row.value_bytes().map_or(0, <[u8]>::len) + 32)
            .sum();
        let scopes: usize = self.scopes.iter().map(|scope| scope.len()).sum();
        let signature = self.signature.entry_count() * std::mem::size_of::<FactVersionRef>();
        std::mem::size_of::<Self>() + rows + scopes + signature
    }
}

/// Why a traced fact could not commit a native basis. Every arm means no
/// reference may be issued for the fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeBasisRefusal {
    /// An owner's source whole hash or source environment could not be
    /// observed.
    OwnerUnobserved,
    /// The trace consumed a read its facts cannot represent.
    NotRepresentable,
    /// A compacting domain moved while the trace ran.
    MutationUnstable,
    /// The captured view does not vouch for an observed fact.
    NotVouched,
    /// The evidence carries a terminal witness scoped to this request's
    /// completion overlay, which no later view can validate.
    RequestLocal,
    /// An observation has no canonical encoding, or two observations of
    /// one key disagree.
    Uncommittable,
}

/// A committed basis no longer describes the inputs a view observes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeBasisStale {
    /// A committed fact no longer validates.
    Facts,
    /// A committed environment or overlay observation changed.
    Observation,
}

/// A finalised fact trace, with whether every owner's source was observed.
#[derive(Debug, Clone)]
pub(crate) struct NativeTrace {
    finalise: FactReadSetFinalise,
    owners_observed: bool,
}

/// Run `f` under a fresh fact tracer bound to `ctx`'s request basis, and
/// finalise it. Observations made inside `f` reach every enclosing tracer
/// as well, so tracing a fact never hides evidence from an outer scope.
///
/// Before `f`, the tracer observes every owner's source whole hash and
/// source environment as the request sees them, materialising the owner
/// when it is cold, even when the fact's own semantic evidence is
/// narrower: an issued fact is bound to its owners' exact sources. An owner
/// whose source cannot be observed leaves the trace uncommittable.
pub(crate) fn trace_native_fact<M: verter_audit::observe::ObserveExec, R>(
    ctx: &crate::resolver_core::HostResolverContext<'_, M>,
    owners: &[&str],
    f: impl FnOnce() -> R,
) -> (R, NativeTrace) {
    let ((owners_observed, value), finalise) =
        verter_type_engine::fact_signature_helpers::install_fact_tracer(
            &verter_type_engine::fact_signature_helpers::FactTracerBasisSource::from_ctx(ctx),
            || {
                let owners_observed = owners.iter().all(|owner| observe_owner_source(ctx, owner));
                (owners_observed, f())
            },
        );
    (
        value,
        NativeTrace {
            finalise,
            owners_observed,
        },
    )
}

/// Record `owner`'s current source whole hash and source environment into
/// the active tracers, materialising the owner's indexed source when it is
/// cold. `false` when either cannot be observed.
fn observe_owner_source<M: verter_audit::observe::ObserveExec>(
    ctx: &crate::resolver_core::HostResolverContext<'_, M>,
    owner: &str,
) -> bool {
    use verter_type_engine::resolver_core::fact_validation_port::FactValidation;
    use verter_type_engine::resolver_core::request_ports::IndexedInputs;
    let Some(serve) = ctx.ensure_indexed_ready_serve(owner) else {
        return false;
    };
    let Some(key) = ctx.artifact_key_for_current_content(owner) else {
        return false;
    };
    let env = ctx.source_environment(&key);
    ctx.observe_borrowed_signature(&[
        FactVersionRef::FileWholeHash {
            canonical_id: Arc::from(owner),
            hash: serve.indexed.whole_hash,
        },
        FactVersionRef::FileSourceEnv {
            canonical_id: Arc::from(owner),
            parse_env_hash: env.parse_env_hash,
            parse_key: env.parse_key,
            file_language_id: env.file_language_id,
        },
    ]);
    true
}

/// Commit the native basis of one traced fact, observed under `view`.
///
/// `view` must be the one captured view the fact was resolved and exported
/// under, and `port` the request's validation port over it (the view plus
/// the request's completion overlay): the trace must validate through the
/// port the request itself validated through, while environment rows are
/// read root-relative from `view`. `entry_scopes` are the
/// canonicals the request named (the signature contributes every canonical
/// its evidence reaches).
pub(crate) fn commit_native_fact_basis(
    host: &VerterHost,
    view: &HostStoreView,
    port: &dyn StoreView,
    trace: NativeTrace,
    entry_scopes: &[&str],
) -> Result<NativeFactBasis, NativeBasisRefusal> {
    if !trace.owners_observed {
        return Err(NativeBasisRefusal::OwnerUnobserved);
    }
    let signature = match trace.finalise {
        FactReadSetFinalise::Ok(facts) => ReadSetSignature::new(facts),
        FactReadSetFinalise::NonCacheable(_) => {
            return Err(NativeBasisRefusal::NotRepresentable);
        }
        FactReadSetFinalise::MutationUnstable => {
            return Err(NativeBasisRefusal::MutationUnstable);
        }
    };
    if !port.validates_fact_signature(FactsRef::Set(&signature.facts)) {
        return Err(NativeBasisRefusal::NotVouched);
    }
    if carries_request_completion_witness(&signature) {
        return Err(NativeBasisRefusal::RequestLocal);
    }
    let authority = NativeAuthority::new(host.instance_id);
    let scopes = participating_scopes(&signature, entry_scopes);
    let mut rows = fact_rows(&signature, authority)?;
    rows.extend(environment_rows(view, &scopes));
    let wave = LoadWave::from_keys(rows.iter().map(|row| Arc::<str>::from(row.canonical())));
    let basis =
        InputBasis::commit(wave, rows, []).map_err(|_| NativeBasisRefusal::Uncommittable)?;
    let fence = SnapshotFence::bind(&basis);
    Ok(NativeFactBasis {
        basis,
        signature,
        scopes,
        fence,
    })
}

/// Check that `view` still observes exactly what `basis` committed: every
/// committed fact validates through `port` (a request validation port over
/// `view`), and every committed environment and overlay observation
/// re-reads identically from `view`. Success certifies the same basis; it
/// never adopts newly observed inputs.
pub(crate) fn validate_native_fact_basis(
    view: &HostStoreView,
    port: &dyn StoreView,
    basis: &NativeFactBasis,
) -> Result<(), NativeBasisStale> {
    if !port.validates_fact_signature(FactsRef::Set(&basis.signature.facts)) {
        return Err(NativeBasisStale::Facts);
    }
    for row in &environment_rows(view, &basis.scopes) {
        match basis.basis.observe(row.canonical()) {
            Ok(committed) if committed == row => {}
            _ => return Err(NativeBasisStale::Observation),
        }
    }
    Ok(())
}

/// Whether any leaf the signature reaches, receipts expanded, is a terminal
/// witness scoped to one request's completion overlay.
fn carries_request_completion_witness(signature: &ReadSetSignature) -> bool {
    let mut walk = ReceiptWalk::default();
    signature.entries().any(|fact| match fact {
        FactVersionRef::Receipt(receipt) => {
            !walk.leaves(receipt, |leaf| !leaf.is_request_completion_witness())
        }
        leaf => leaf.is_request_completion_witness(),
    })
}

/// The request's own canonicals plus every canonical the evidence names,
/// sorted and deduplicated.
fn participating_scopes(signature: &ReadSetSignature, entry_scopes: &[&str]) -> Arc<[Arc<str>]> {
    let mut scopes: Vec<&str> = entry_scopes.to_vec();
    scopes.extend(signature.canonical_id_refs());
    scopes.sort_unstable();
    scopes.dedup();
    scopes.into_iter().map(Arc::from).collect()
}

/// One value row per distinct observed fact leaf. The row is named by the
/// digest of the fact's canonical key and carries the key and revision
/// whole, so a digest collision cannot alias two keys: it surfaces as a
/// conflicting commit instead.
fn fact_rows(
    signature: &ReadSetSignature,
    authority: NativeAuthority,
) -> Result<Vec<Observation>, NativeBasisRefusal> {
    let observations = native_observations(signature.entries(), authority)
        .map_err(|_| NativeBasisRefusal::Uncommittable)?;
    Ok(observations
        .into_iter()
        .map(|observation| {
            let name = CanonicalDigest::of_bytes(observation.key()).to_hex();
            let mut value = NativeWriter::new("verter.native_basis.fact.v1");
            value.bytes(observation.key()).bytes(observation.revision());
            Observation::value(format!("fact:{name}"), value.finish())
        })
        .collect())
}

/// The root-relative effective environment row of every participating
/// canonical (parse / resolve / type / lib environment and project
/// identity), plus the view's overlay population.
fn environment_rows(view: &HostStoreView, scopes: &[Arc<str>]) -> Vec<Observation> {
    let mut rows = Vec::with_capacity(scopes.len() + 1);
    rows.push(Observation::value(
        "view:population",
        view_population_bytes(view.view_population()),
    ));
    for scope in scopes {
        let (env_hashes, project) = view.native_environment_for(scope);
        let mut env = NativeWriter::new("verter.native_basis.environment.v1");
        env.bytes(&env_hashes.parse_env_hash)
            .bytes(&env_hashes.resolve_env_hash)
            .bytes(&env_hashes.type_env_hash)
            .bytes(&env_hashes.lib_env_hash)
            .bytes(&project.0);
        rows.push(Observation::value(format!("env:{scope}"), env.finish()));
    }
    rows
}

#[cfg(test)]
#[path = "native_basis_tests.rs"]
mod tests;
