# B3 — context packet

Base `7e8b025b8`. Candidate `1c8e01792`.

## Scope

One canonical typed compile request replacing the parallel option
authorities (`CompileTarget`, `CodegenOptions`, `VerterCompileOptions`,
`CompileProfile`, `FfiCompileProfile` decode, `NapiCompileProfile`)
across every currently-reachable production compile route: the internal
one-shot compiler entrypoint, the host's per-file/virtual session route,
NAPI `compile_many` and `compile_with_audit`, WASM, and the bundler/
unplugin Rust ingress. Full ratified scope: `docs/arch/refactor/rev11/
charters/B3.md` (unchanged by this candidate) plus the amendment and
scope ruling cited in the review record below.

## Review record

Three concurrent seats (conformance/codex, architecture/grok, adversarial/
Claude-subagent-with-plant-prove-RED-GREEN) ran against the candidate.
Round 1 came back BLOCKING on all three seats — the real gap was that the
session's per-file/virtual route (`virtual_file_pipeline.rs`, the most-used
production compile path) never actually constructed a `CompileRequest`,
still building `RuntimeCompileOptions` directly off the legacy
`CompileProfile` with no admission gate; R3/R5/R7 inherited the same gap
by construction. One fix round closed that plus several convergent
findings (unwired unknown-option refusal, under-specified debt rows,
non-discriminating tests, three fixes with no regression coverage,
program-vocabulary references in doc comments). No further 3-seat review
round was needed after that — the two remaining fix rounds were closing
real regressions this session's own pre-landing verification caught
directly (see landing-record.md), not new review findings.
