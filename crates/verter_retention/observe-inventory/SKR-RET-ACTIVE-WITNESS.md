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
| `SemanticRetentionAccount::active_windows` (`ActiveWindows`: window epoch, ACTIVE transition sequence, open-window count as the hooks' lock-free miss path, open window states) | none (measurement-only; read by `open_active_window`, `active_transition_count` and the transition hook) | OPTIONAL | One per account; each window state lives from `open_active_window` until its window finishes or drops | `src/active_window.rs` | `cfg(any(test, feature = "test-support", feature = "soak-support"))` (ratified narrow exception to `semantic-observe`) |
| `RetentionCharge::minted_epoch` (`ChargeEpoch.minted`) | none (the transition hook treats a charge minted before a window opened as pre-window work) | OPTIONAL | Per charge, inherited by `split_off` and transfer remainders, released with the charge; `ChargeEpoch` is zero-sized without the gate | `src/lib.rs` | `cfg(any(test, feature = "test-support", feature = "soak-support"))` (ratified narrow exception to `semantic-observe`) |
| `note_active_locked` at ACTIVE reserve, ACTIVE release, `commit_retained`, ACTIVE `transfer_to_pinned` and `holding` (a retained slice joining an ACTIVE charge) | none (raises each open window's peak and counts its rise/fall bytes and transitions under the admission lock that decides the transition) | OPTIONAL | One call per ACTIVE transition; compiles to nothing without the gate | `src/lib.rs` | `cfg(any(test, feature = "test-support", feature = "soak-support"))` (ratified narrow exception to `semantic-observe`) |
| `ActiveOccupancyWindow`, `ActiveWindowReport`, `ActiveWindowId`, `AttributedActivePeak`, `ActiveWitnessRejection` (`observe`, `finish`, `attributed`, `crossed`) | none (tooling and tests) | OPTIONAL | Guard-scoped: an unfinished window deregisters on drop and keeps its account alive while open | `src/active_window.rs` | `cfg(any(test, feature = "test-support", feature = "soak-support"))` (ratified narrow exception to `semantic-observe`) |
| `ACTIVE_OCCUPANCY_WINDOW_AVAILABLE` | Tooling reports whether its build carries the window instead of reading a fabricated zero | REQUIRED | Compile-time constant | `src/lib.rs` | always |
| `soak-support` feature, forwarded by `verter_session_query/soak-support` and `verter_session/soak-support` | none (the soak/tooling runner's opt-in to the window alone) | OPTIONAL | Default-off; shipped normal/build closures of the production roots are proven free of it by `observe_feature_closure_is_off_for_production_and_its_dev_unification`, its absence without the opt-in by the `active_occupancy_window_absent_without_support` compile contract, and a consumer opening it under only `soak-support` by the `active_occupancy_window_opt_in_through_soak_support` pass contract | `Cargo.toml` | feature |

### `verter_session`

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `VerterHost::open_active_occupancy_window` | none (tooling; opens the window on the account the host's `ProjectTypeStore` charges, the account `retention_snapshot` reads) | OPTIONAL | Per window | `src/host_lifecycle.rs` | `cfg(any(test, feature = "test-support", feature = "soak-support"))` (ratified narrow exception to `semantic-observe`) |

Attribution boundary: a charge carries no owner identity, so a window
attributes every ACTIVE transition on the account inside its interval to the
measured workload. `attributed()` accepts only a workload running alone on
the process account and rejects the witness when foreign work is detectable:
ACTIVE bytes charged at open, a transition of a charge minted before the
window opened, or ACTIVE bytes still charged at close.

Supported build: the soak/tooling runner enables
`verter_session/soak-support` (for example
`cargo build -p <runner> --features verter_session/soak-support`); tests
reach the window through `test-support`.

Owner proof: `crates/verter_retention/src/active_window_tests.rs` (active
rise/fall, retained/pinned totals and a pre-window peak never crossing,
class transfers, shared charges, overlapping windows, foreign-work
rejection, concurrent boundary transitions, teardown, unchanged admission).
Public-host control:
`crates/verter_session/tests/cases/g_session/active_occupancy_window.rs`.
