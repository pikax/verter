# ACTIVE-occupancy window inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).
The ACTIVE-occupancy window lets a test or the soak/tooling runner read the
most ACTIVE bytes the one process-local retention account held at once
while a workload ran, without audit or footprint capture. It measures the
account's real charge paths only: it never changes an admission decision,
a charge, a release or a cache answer. Production policy reads none of it,
so every OPTIONAL row is compiled only under `test`, `test-support` or the
default-off `soak-support` feature; the account's current occupancy
(`SemanticRetentionAccount::snapshot`, `VerterHost::retention_snapshot`)
remains REQUIRED production state and is unchanged.

### `verter_retention`

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `SemanticRetentionAccount::active_windows` (`ActiveWindows`: owner/window id counter, ACTIVE transition sequence, open-window count, per-owner and UNKNOWN-provenance ACTIVE bytes, open window states) | none (measurement-only; read by `mint_active_owner`, `open_active_window`, `active_transition_count` and the transition hook) | OPTIONAL | One per account; an owner's byte entry lives while it holds ACTIVE bytes, each window state from `open_active_window` until its window finishes or drops | `src/active_window.rs` | `cfg(any(test, feature = "test-support", feature = "soak-support"))` (ratified narrow exception to `semantic-observe`) |
| `RetentionCharge::provenance` (`Provenance.owner`), `ChargeCtx::provenance` | none (the transition hook attributes each ACTIVE transition to the charge's owner; an untagged or mixed charge is unknown) | OPTIONAL | Per charge, inherited by `split_off` and transfer remainders, merged by `absorb` and `holding` (different owners become unknown), released with the charge; `Provenance` is zero-sized without the gate | `src/lib.rs`, `src/owned.rs`, `src/granular.rs` | `cfg(any(test, feature = "test-support", feature = "soak-support"))` (ratified narrow exception to `semantic-observe`) |
| `note_active_locked` at ACTIVE reserve, ACTIVE release, `commit_retained`, ACTIVE `transfer_to_pinned` and `holding` (a retained slice joining an ACTIVE charge) | none (raises each open window's peak and counts its rise/fall bytes and transitions under the admission lock that decides the transition) | OPTIONAL | One call per ACTIVE transition; compiles to nothing without the gate | `src/lib.rs` | `cfg(any(test, feature = "test-support", feature = "soak-support"))` (ratified narrow exception to `semantic-observe`) |
| `ActiveOwner`, `ActiveOwnerId`, `reserve_as`, `ChargeCtx::owned_by`, `ActiveOccupancyWindow`, `ActiveWindowReport`, `ActiveWindowId`, `AttributedActivePeak`, `ActiveWitnessRejection` (`observe`, `finish`, `attributed`, `crossed`) | none (tooling and tests) | OPTIONAL | Guard-scoped: an unfinished window deregisters on drop and keeps its account alive while open | `src/active_window.rs` | `cfg(any(test, feature = "test-support", feature = "soak-support"))` (ratified narrow exception to `semantic-observe`) |
| `ACTIVE_OCCUPANCY_WINDOW_AVAILABLE` | Tooling reports whether its build carries the window instead of reading a fabricated zero | REQUIRED | Compile-time constant | `src/lib.rs` | always |
| `soak-support` feature, forwarded by `verter_session_query/soak-support` and `verter_session/soak-support` | none (the soak/tooling runner's opt-in to the window alone) | OPTIONAL | Default-off; shipped normal/build closures of the production roots are proven free of it by `observe_feature_closure_is_off_for_production_and_its_dev_unification`, its absence without the opt-in by the `active_occupancy_window_absent_without_support` compile contract, and a consumer opening it under only `soak-support` by the `active_occupancy_window_opt_in_through_soak_support` pass contract | `Cargo.toml` | feature |

### `verter_session`

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `VerterHost::mint_active_occupancy_owner`, `VerterHost::open_active_occupancy_window` | none (tooling; mint an owner and open its window on the account the host's `ProjectTypeStore` charges, the account `retention_snapshot` reads) | OPTIONAL | Per window | `src/host_lifecycle.rs` | `cfg(any(test, feature = "test-support", feature = "soak-support"))` (ratified narrow exception to `semantic-observe`) |

Attribution: a window measures one explicit owner. Owner provenance is bound to
the account and travels with the charge; identified foreign owners are
excluded and counted, while untagged or mixed-provenance ACTIVE transitions in
the interval reject the witness (`UnknownProvenance`), as do the owner's own
ACTIVE bytes at open or at close. No production ACTIVE producer reserves
through an owner; a workload with no owned ACTIVE activity reports zero (the
public-host control asserts that truthful zero, with no test-manufactured
reservation standing in for a workload). UNKNOWN ACTIVE bytes charged when a
window opens reject it (`UnknownActiveAtOpen`), and a charge that changes
provenance (holds adoption) settles its class and attribution in one
account-locked transaction.

Supported build: the soak/tooling runner enables
`verter_session/soak-support` (for example
`cargo build -p <runner> --features verter_session/soak-support`); tests
reach the window through `test-support`.

Owner proof: `crates/verter_retention/src/active_window_tests.rs` (active
rise/fall, retained/pinned totals and a pre-window peak never crossing,
class transfers, shared charges, overlapping windows, foreign work wholly
inside the window excluded, unknown/mixed-provenance rejection including
occupancy spanning a whole window, deterministic boundary straddles, concurrent
boundary transitions with forced overlap, teardown, unchanged admission).
Public-host control:
`crates/verter_session/tests/cases/g_session/active_occupancy_window.rs`.
