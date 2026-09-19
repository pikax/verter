# BWH2 evidence index

Selected case IDs: `BWH2-AC1`, `BWH2-AC2`, `BWH2-AC3`, `BWH2-AC-OWNER`,
`BWH2-AC-BASIS`, `BWH2-AC-RESOURCE`, `BWH2-AC-EXPOSURE`.

Commands (run on the candidate; do not treat this file as an execution
transcript):

- `cargo nextest run -p verter_session --lib -E 'test(platform_services) + test(input_handoff) + test(cooperative_scheduler)'`
  — boundary discriminators: browser-closure guard, typed NeedInputs,
  complete-negative Absent, identical-basis equivalence, incremental
  retry vs fresh commit, cooperative cancel/yield points (before the
  first stage, between driven stages, and at admission), cancelled
  drive exposing no half-committed snapshot, and a driver-installed
  drive that parks on the driver instead of inline-pumping stages on
  the caller's thread.
- `cargo nextest run -p verter_session --test main -E 'test(cooperative_drive_seam) + test(verter_session_public_surface)'`
  — the host load-seam discriminators at the exact `ensure_loaded`
  production route (cancelled drive refuses admission; constructor-time
  yield hook installed through `HostConfig::cooperative_yield`;
  withdrawn drive answers not-loaded without running the integrate
  step; native `ensure_loaded` parks on the driver instead of
  inline-executing scheduler stages; Source-without-Analysis never
  answers the loaded fast path — the resumed load goes through the
  submit/drive seam to Analysis + integrate) plus the Guard 5
  public-surface snapshot covering the three new pub modules.
- `cargo nextest run -p verter_wasm --lib` — includes the
  committed input-snapshot core statuses (file / absent / needInputs,
  identical basis identity, incoherent wave refusal).
- `cargo check -p verter_session --target wasm32-unknown-unknown` and
  `cargo check -p verter_wasm --target wasm32-unknown-unknown` — the
  browser closure compiles with the boundary and the inline scheduler
  verification in place.
- `node --test scripts/browser-host-gate.test.mjs` — gate selftest plus
  the browser client handoff half: wave validation, async acquisition
  shape, typed NeedInputsError only for needInputs (absent stays a
  complete negative).
- `pnpm --filter @verter/wasm run test:types` — the typed Host facade
  for commitInputSnapshot / observeInputSnapshot.
- `node scripts/browser-host-gate.mjs [--skip-build]` — the
  browser-host-domain gate remains the runtime harness for the real
  worker closure; the worker-side activation of the committed-snapshot
  handoff is assigned to BWH3/BWH5.

Grounding notes: acquisition is asynchronous and lives outside every
semantic callback; commit and observe are synchronous and carry no
acquisition capability, so a resolver cannot fetch. The cooperative
adapter owns no runtime and no cache — it wraps the existing
submit_request/wait_or_drive contracts and preserves their outcomes
whenever it is not cancelled.
