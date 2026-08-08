//! The v4 `relation_verdict` consumption driver — the comparison half of the
//! relation-tuple-wire capture family
//! (`docs/arch/ri0-relation-verdict-oracle-addendum.md` §5).
//!
//! CAPTURE-ONLY posture: this driver consumes the CHECKED-IN relation
//! snapshots (tsgo NEVER launches in default tests — like the v3 driver, it
//! re-derives the `snapshot_id` from REGISTRY-ONLY inputs, loads the snapshot
//! via runtime `std::fs::read`, strictly decodes it, and validates env pins +
//! corpus), then observes what Verter's relation engine ACTUALLY answers for
//! the spec's supported key through the ONE normalized
//! [`relation_probe::RelationVerdictValue`] boundary (the
//! `ObservedRelationVerdict` of the addendum — the SAME boundary the oracle
//! DTO and the wire decoder share).
//!
//! The observation adapter calls the SOLE relation authority
//! (`execute(SemanticQueryKey::Relate)` via the full-key constructor
//! `execute_relate_pair`) ONLY for the engine's actually supported
//! identity (assignable, default policy, regular source freshness, NO
//! inference context) and REJECTS broader keys (a binder-carrying target
//! pattern IS an inference context — `EngineObservation::UnsupportedKey`).
//! `Unknown` / `Miss` / `BudgetExceeded` are ENGINE FAILURES, never oracle
//! verdicts. The adapter swap is itself guarded: zero remaining
//! oracle-consumer calls to the deleted `relate_nodes` entry point (the
//! retired-symbol guard in `typeinfo_tests/relation_verdict_oracle.rs`).
//!
//! Parity enforcement is DISABLED for the known-mismatch ledger rows (the
//! registry's `engine_pin`): a pinned row asserts the engine observation
//! against the PIN instead of the oracle, so a future engine fix flips the row
//! loudly. The honest state is captured-records-with-known-mismatch-ledger —
//! this family is NOT M=0.

use serde_json::Value;

use super::driver::{self, DriverError};
use super::identity::{self, PinnedEnv};
use super::query_specs::{
    EngineObservationPin, RelationEngineVerdict, RelationQuerySpec, RELATION_QUERY_SPECS,
};
use super::relation_probe::{self, RelationVerdict, RelationVerdictValue};
use super::snapshot::{self, OracleSnapshot};

/// Why a relation spec could not be validated against its snapshot + the
/// engine observation. [`run_relation_rows`] turns any error into a panic (the
/// test-failure surface) with row attribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RelationDriverError {
    /// The registry-derivable identity derivation failed (a bad operand text
    /// or an inconsistent binder layout).
    Spec(String),
    /// The snapshot file could not be read / parsed.
    SnapshotIo(String),
    /// The snapshot failed strict decode.
    Decode(snapshot::SnapshotDecodeError),
    /// A stored env-pin / `snapshot_id` / `row_ref` field did not match the
    /// registry-derived expectation.
    EnvPinMismatch {
        field: String,
        expected: String,
        found: String,
    },
    /// The vendored-corpus validation failed (the v3 driver's rail).
    Corpus(DriverError),
    /// The engine could not answer a PARITY row (an `Unknown` / a resolution
    /// miss / a rejected key on a row with no ledger pin) — an engine failure,
    /// never an oracle verdict.
    EngineFailure {
        row_function: String,
        detail: String,
    },
    /// A parity row's engine observation did not equal the captured oracle
    /// value — a REAL relation divergence (not a ledger row: parity is
    /// enforced here).
    ParityMismatch {
        row_function: String,
        engine: String,
        oracle: String,
    },
    /// A ledger-pinned row's engine observation no longer matches the pin —
    /// either the engine fix landed (REMOVE the pin and enforce parity) or the
    /// engine drifted further (UPDATE the pin with the new proven answer).
    LedgerFlip {
        row_function: String,
        detail: String,
    },
}

/// What the relation engine ACTUALLY answers for a spec's key, observed
/// through the normalized boundary. `Unknown` / `Miss` / `BudgetExceeded` have
/// NO representation as verdicts — they are engine failures.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum EngineObservation {
    /// The adapter REJECTED the key: the target pattern carries `infer`
    /// binders (an inference context), outside the engine's supported
    /// identity this block.
    UnsupportedKey,
    /// `execute(Relate)` returned an undecided judgement (or an operand
    /// failed to resolve) — an engine failure, never an oracle verdict.
    Unknown(String),
    /// The engine produced a verdict. Bindings are ALWAYS empty this block
    /// (the supported key carries no inference context, so there is nothing to
    /// bind); when inference support lands, engine bindings must be raised
    /// through the SAME normalized projection
    /// (`relation_probe::RELATION_BINDING_PROJECTION`) before comparison.
    Verdict(RelationVerdictValue),
}

/// Observe the engine's live answer for a spec: REJECT a broader-than-
/// supported key (inference context) WITHOUT touching the engine; otherwise
/// resolve both operands through the ONE shared resolver and call the SOLE
/// relation authority (`execute(SemanticQueryKey::Relate)` via the full-key
/// constructor `execute_relate_pair`) under its actual supported identity
/// (assignable, default policy, regular source, no inference context).
pub(crate) fn observe_engine(spec: &RelationQuerySpec) -> EngineObservation {
    use crate::project_semantic_dispatch::dispatch_txn::RelationStep;
    use crate::project_semantic_dispatch::ProjectSemanticDispatch;
    use crate::semantic_query::ProjectionMode;
    use crate::typeinfo::typeinfo_tests::support::{make_host_with_footprint, upsert_ts};

    if !spec.binder_layout.is_empty() {
        return EngineObservation::UnsupportedKey;
    }

    // The engine-observation fixture (adapter-internal, derived purely from the
    // spec's operand texts — NOT the probe file): both operands as plain type
    // aliases, each resolved through the ONE shared resolver.
    let fixture = format!(
        "type __OracleSource = {};\ntype __OracleTarget = {};\n",
        spec.source_text, spec.target_text
    );
    let canonical = format!("/fixtures/relation_engine/{}.ts", spec.row_function);
    let host = make_host_with_footprint();
    upsert_ts(&host, &canonical, &fixture);

    let resolve = |name: &str| -> Option<crate::semantic_query::SemanticNodeId> {
        let (outcome, _record) = host
            .resolve_named_symbol_with_audit(&canonical, name, Some(ProjectionMode::Expanded))
            .into_parts();
        outcome.ok().flatten()
    };
    let Some(source) = resolve("__OracleSource") else {
        return EngineObservation::Unknown("source operand did not resolve".to_string());
    };
    let Some(target) = resolve("__OracleTarget") else {
        return EngineObservation::Unknown("target operand did not resolve".to_string());
    };

    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    match dispatch.execute_relate_pair(source, target) {
        RelationStep::Assignable { bindings } => {
            if !bindings.is_empty() {
                return EngineObservation::Unknown(format!(
                    "execute(Relate) returned {} unexpected inference bindings on a binder-free key",
                    bindings.len()
                ));
            }
            EngineObservation::Verdict(RelationVerdictValue {
                verdict: RelationVerdict::Assignable,
                bindings: Vec::new(),
            })
        }
        RelationStep::NotAssignable => EngineObservation::Verdict(RelationVerdictValue {
            verdict: RelationVerdict::NotAssignable,
            bindings: Vec::new(),
        }),
        RelationStep::Unknown | RelationStep::BudgetExceeded(_) => EngineObservation::Unknown(
            "execute(Relate) returned an undecided judgement".to_string(),
        ),
        RelationStep::Assumed(_) => EngineObservation::Unknown(
            "execute(Relate) hit an unexpected open assumption at the root".to_string(),
        ),
    }
}

/// Validate the relation snapshot's stored env-pins / `snapshot_id` /
/// `row_ref` against the registry-derived expectation (the v4 analog of the v3
/// driver's `validate_env_pins`). Returns the derived id. `pub(crate)` for the
/// discriminating authentication guard (which proves these registry⇄file
/// rails alone ACCEPT a tampered-identity document).
pub(crate) fn validate_relation_env_pins(
    snapshot: &OracleSnapshot,
    spec: &RelationQuerySpec,
    identity: &identity::RelationVerdictIdentity,
    env: &PinnedEnv,
) -> Result<String, RelationDriverError> {
    let derived_id = identity::derive_relation_snapshot_id(identity, env);
    let checks: [(&str, &str, &str); 9] = [
        ("tsgo_version", &snapshot.tsgo_version, &env.tsgo_version),
        (
            "oracle_schema_version",
            &snapshot.oracle_schema_version.to_string(),
            &env.oracle_schema_version.to_string(),
        ),
        (
            "normalizer_version",
            &snapshot.normalizer_version.to_string(),
            &env.normalizer_version.to_string(),
        ),
        (
            "probe_synthesis_version",
            &snapshot.probe_synthesis_version.to_string(),
            &env.probe_synthesis_version.to_string(),
        ),
        (
            "compiler_options_hash",
            &snapshot.compiler_options_hash,
            &env.compiler_options_hash,
        ),
        ("env_corpus_id", &snapshot.env_corpus_id, &env.env_corpus_id),
        ("snapshot_id", &snapshot.snapshot_id, &derived_id),
        (
            "row_ref.row_file",
            &snapshot.row_ref.row_file,
            spec.row_file,
        ),
        (
            "row_ref.row_function",
            &snapshot.row_ref.row_function,
            spec.row_function,
        ),
    ];
    for (field, found, expected) in checks {
        if found != expected {
            return Err(RelationDriverError::EnvPinMismatch {
                field: field.to_string(),
                expected: expected.to_string(),
                found: found.to_string(),
            });
        }
    }
    if snapshot.row_ref.query_ordinal != spec.query_ordinal {
        return Err(RelationDriverError::EnvPinMismatch {
            field: "row_ref.query_ordinal".to_string(),
            expected: spec.query_ordinal.to_string(),
            found: snapshot.row_ref.query_ordinal.to_string(),
        });
    }
    Ok(derived_id)
}

/// Authenticate the STORED identity against the stored `snapshot_id`: redrive
/// the id from the snapshot's own stored `identity` + `row_ref` + stored env
/// pins (the same registry-free redrive `snapshot::redrive_snapshot_id` runs
/// for the TypeExpr lane) and require equality with the file's top-level id.
/// `validate_relation_env_pins` alone proves only registry⇄file agreement — a
/// tampered `identity.policy.*` (or any identity axis) whose top-level id and
/// filename were left intact passes THAT check; it fails here.
pub(crate) fn authenticate_stored_id(snapshot: &OracleSnapshot) -> Result<(), RelationDriverError> {
    let redriven = snapshot::redrive_snapshot_id(snapshot).map_err(RelationDriverError::Decode)?;
    if redriven != snapshot.snapshot_id {
        return Err(RelationDriverError::EnvPinMismatch {
            field: "snapshot_id(redrive-from-stored-identity)".to_string(),
            expected: snapshot.snapshot_id.clone(),
            found: redriven,
        });
    }
    Ok(())
}

/// Validate ONE relation registry spec against its checked-in snapshot + the
/// engine observation. Split out so its error path is testable without a
/// panic.
pub(crate) fn run_relation_spec(
    spec: &RelationQuerySpec,
    env: &PinnedEnv,
) -> Result<(), RelationDriverError> {
    // (1) Registry-derivable identity → snapshot_id → runtime fs load → strict
    //     decode → env pins → corpus (NO tsgo, mirroring the v3 driver).
    let identity = relation_probe::relation_identity_from_spec(spec)
        .map_err(|e| RelationDriverError::Spec(format!("{e:?}")))?;
    let snapshot_id = identity::derive_relation_snapshot_id(&identity, env);
    let path = driver::snapshot_abs_path(spec.oracle_family, &snapshot_id);
    let bytes = std::fs::read(&path)
        .map_err(|e| RelationDriverError::SnapshotIo(format!("{}: {e}", path.display())))?;
    let json: Value = serde_json::from_slice(&bytes).map_err(|e| {
        RelationDriverError::Decode(snapshot::SnapshotDecodeError::Envelope(e.to_string()))
    })?;
    let snapshot = snapshot::decode_strict(&json).map_err(RelationDriverError::Decode)?;
    validate_relation_env_pins(&snapshot, spec, &identity, env)?;
    // (1a) The stored identity must hash to the stored id (authentication —
    //      independent of the registry derivation above).
    authenticate_stored_id(&snapshot)?;
    driver::validate_env_corpus(&snapshot, &driver::corpus_root(&env.env_corpus_id))
        .map_err(RelationDriverError::Corpus)?;
    // The stored oracle value, materialized through the strict rails into the
    // SAME normalized boundary the engine observation uses.
    let oracle =
        snapshot::materialize_relation_value(&snapshot).map_err(RelationDriverError::Decode)?;

    // (2) The engine observation, compared under the row's parity posture.
    let observation = observe_engine(spec);
    let oracle_canonical = relation_probe::relation_value_canonical_form(&oracle);
    match spec.engine_pin {
        // Parity-enforced row: the engine MUST answer, and its answer must
        // equal the captured oracle value under the canonical form.
        None => match observation {
            EngineObservation::Verdict(engine) => {
                let engine_canonical = relation_probe::relation_value_canonical_form(&engine);
                if engine_canonical != oracle_canonical {
                    return Err(RelationDriverError::ParityMismatch {
                        row_function: spec.row_function.to_string(),
                        engine: engine_canonical,
                        oracle: oracle_canonical,
                    });
                }
                Ok(())
            }
            EngineObservation::UnsupportedKey => Err(RelationDriverError::EngineFailure {
                row_function: spec.row_function.to_string(),
                detail: "parity row's key was rejected as unsupported".to_string(),
            }),
            EngineObservation::Unknown(detail) => Err(RelationDriverError::EngineFailure {
                row_function: spec.row_function.to_string(),
                detail,
            }),
        },
        // Ledger row — parity DISABLED; the engine observation must match the
        // PIN exactly, so a future engine fix flips the row loudly.
        Some(EngineObservationPin::UnsupportedKey) => match observation {
            EngineObservation::UnsupportedKey => Ok(()),
            other => Err(RelationDriverError::LedgerFlip {
                row_function: spec.row_function.to_string(),
                detail: format!(
                    "the engine now answers an inference-context key ({other:?}) — remove the \
                     UnsupportedKey pin and enforce parity against {oracle_canonical}"
                ),
            }),
        },
        Some(EngineObservationPin::MismatchedVerdict(pinned)) => match observation {
            EngineObservation::Verdict(engine) => {
                let pinned_tag = match pinned {
                    RelationEngineVerdict::Assignable => RelationVerdict::Assignable.tag(),
                    RelationEngineVerdict::NotAssignable => RelationVerdict::NotAssignable.tag(),
                };
                if engine.verdict.tag() != pinned_tag {
                    return Err(RelationDriverError::LedgerFlip {
                        row_function: spec.row_function.to_string(),
                        detail: format!(
                            "the engine now answers `{}` (pinned: `{pinned_tag}`, oracle: \
                             `{oracle_canonical}`) — a fix landed or the engine drifted; \
                             re-prove the row and update the ledger",
                            engine.verdict.tag()
                        ),
                    });
                }
                if engine.verdict == oracle.verdict {
                    return Err(RelationDriverError::LedgerFlip {
                        row_function: spec.row_function.to_string(),
                        detail: format!(
                            "the engine now AGREES with the oracle (`{oracle_canonical}`) — the \
                             mismatch is fixed; remove the pin and enforce parity"
                        ),
                    });
                }
                Ok(())
            }
            other => Err(RelationDriverError::EngineFailure {
                row_function: spec.row_function.to_string(),
                detail: format!("ledger row (pinned `{pinned:?}`) produced no verdict: {other:?}"),
            }),
        },
    }
}

/// The relation-family sweep: validate every `RELATION_QUERY_SPECS` entry
/// against its checked-in snapshot + the engine observation. The capture-only
/// analog of the v3 driver's `run_row` — one runner over the pure-data
/// registry (the rows are never lifts, so no proc-macro body calls them).
/// Panics (the test-failure surface) on any divergence, with row attribution.
#[allow(dead_code)]
pub(crate) fn run_relation_rows() {
    let env = driver::pinned_env();
    for spec in RELATION_QUERY_SPECS {
        if let Err(err) = run_relation_spec(spec, &env) {
            panic!(
                "relation oracle row {}::{}#{}: {err:?}",
                spec.row_file, spec.row_function, spec.query_ordinal
            );
        }
    }
}
