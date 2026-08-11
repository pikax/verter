# A2 — oracle / profile stamps

## Oracle

`ORACLE_STAMP` (tracked, `crates/verter_session/src/u6_flow_expect_tests.rs`):

    tsgo 7.0.0-dev.20260526.1 --noEmit --strict --ignoreConfig --pretty false (checker only)

- Binary invoked for this block's measurements:
  `node_modules/.pnpm/node_modules/.bin/tsgo` (pnpm-installed pinned dev build),
  `tsgo --version` → `Version 7.0.0-dev.20260526.1` — byte-equal to the corpus's
  pinned `TSGO_VERSION` (cross-asserted by the tracked test
  `stamps_match_the_pinned_oracle_and_profile`).
- Probe form: the corpus's own two-step `probe_program` (declare-then-assign-to-null,
  plus the `IsAny` detector). Probe fixtures + raw checker output:
  `command-proofs/tsgo-probes/` (25 probes: the five repaired rows re-verified, and
  every matrix cell program).
- tsgo is GENERATION-ONLY in the codebase: no test invokes it; every `checker` value
  is a RECORDED measurement, refreshable via the documented dump procedure.

## Semantic profile

`PROFILE_STAMP` (tracked, same file):

    VerterHost standalone { analysis_level: Full, audit_enabled: true,
    footprint_capture: false, scheduler cpu_threads: 1 };
    demand = ReturnProjectionDemand::whole_return();
    rail = body-derived FlowReturn via VerterHost::get_flow_return_type_with_audit

- Host construction is `make_audit_host()` in the same module — the stamp names the
  exact HostConfig fields that differ from default.
- Every expect/boundary/matrix measurement demands `whole_return()` through the ONE
  public audited entry (`get_flow_return_type_with_audit`), never a second resolver.
- Both stamps ride EVERY failure message the expect lane, boundary lane, and matrix
  emit, so a failing row's report names the oracle it was compared against and the
  profile in force without re-derivation.

## Where each row's evidence records them

- Corpus rows: the `checker` column (oracle value) + `verdict` + the stamps in the
  failure renderer; the five repaired rows re-verified against the live pinned binary
  (raw: `command-proofs/tsgo-probes/tsgo-output.txt` — X85/X87/X88/N25/N26 blocks).
- Matrix cells: per-cell `checker` field (oracle value, measured — same raw file) +
  the stamps in `check_cell_outcome`'s report.
