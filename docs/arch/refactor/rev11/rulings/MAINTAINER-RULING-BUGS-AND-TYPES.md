---
ruling_id: "BUGS-AND-TYPES"
type: "maintainer-directive"
date: "2026-08-17"
date_source: "stated"
binds: ["program-wide (every remaining block, not only BF3)", "AT-2 (applied here as the prompting case)"]
source_file: "MAINTAINER-RULING-BUGS-AND-TYPES.md"
summary: "General standing rule, given in response to an AT-2 disposition question but binding project-wide: no error path for a type problem (Verter compiles/builds and returns; only a genuine compilation error produces an error); a test-discovered issue is a bug fixed in owning production code, never wrapped in a guard/tracker/refusal/allowlist; types are WAIVED from that fix-now rule for the program's duration (maintainer fixes types personally post-program); interim handling is every bug captured as an added #[ignore]d test with the fix deferred to a named owner."
supersedes: []
superseded_by: []
contradicts: []
notes: "Applies its own rule to the prompting AT-2 case in the same document, arriving at the same disposition MAINTAINER-ACT-AT2.md separately records as a direct act. This is the general standing rule referenced by name ('the standing bugs-and-types rule' / 'the maintainer's standing bug handling rule') throughout later documents in this corpus, e.g. MAINTAINER-RULING-NO-LIGHTNINGCSS.md's CSS-REFUSE-001 debt row."
---

# Maintainer standing ruling — bug handling and the type waiver (2026-08-17)

Maintainer: Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub: pikax), designated maintainer.
Given in response to an AT-2 disposition question, but stated as a GENERAL rule and recorded
as one. It binds every remaining block, not just BF3.

## Verbatim ruling

> I don't want the compile error if the type is, unless there's a legit compilation error,
> verter should compile/build and return, if our tests shows an issue we should fix it as a
> bug, we don't ship something we know that has bugs, but for types I waive that rule, I'll
> personally fix the types after the plan is done, for now all the bugs found should be added
> tests and make them ignored to be fixed in the future

## Normalized rules

1. **No error path for a type problem.** Verter compiles/builds and RETURNS. Only a genuine
   compilation error produces an error. A bad or wrong TYPE never becomes a compile error, a
   refusal, or any other production failure path. This extends the existing no-error-on-bad-output
   product ruling to the type surface.
2. **A test-discovered issue is a BUG.** It is fixed in the owning production code — never wrapped
   in a guard, tracker, refusal or allowlist consumed by production.
3. **Types are WAIVED from rule 2** for the duration of the program. The maintainer fixes Verter's
   types personally after the plan completes. No block opens type-correctness work.
4. **Interim handling during the program: every bug found is captured as an ADDED TEST, marked
   `#[ignore]`d, with the fix deferred.** The program's job is to find, characterize and dispatch
   to a named owner — not to fix in place. "We don't ship known bugs" binds at RELEASE, not at
   every intermediate landing.

## Consequences

- A block that finds a genuine defect records it, adds a precise discriminating `#[ignore]`d test,
  names the correction owner, and moves on. That is the COMPLETE obligation.
- A finding that is NOT a demonstrated, reproduced defect must NOT carry a required-RED target.
  A target that fails only because of some other unrelated ratified gap is a stub, not evidence.
- No block may add a production guard, typed refusal, withhold path, retraction, or runtime
  tracking artifact in response to any finding — type-related or otherwise.

## Application to AT-2 (the question that prompted this)

AT-2's ratified claim (a batch entry publishes a product beside a genuine typed refusal) is NOT a
demonstrated defect: all nine construction sites were enumerated, eight are atomic by hardcoded
literal, the typed refusal lands on an atomic site, the single non-atomic site has no demonstrated
reachable input, and the one residual was probed and not reproduced.

Under rule 4 its correct shape is: reclassify as a latent construction hazard with reachability
unproven, retain the DEFER to BA0, capture it as an `#[ignore]`d characterization test, and DROP
the requirement that a Svelte-refusal atomicity target be RED (that target would fail only because
a separate ratified row, RT-1, prevents Svelte classification at all — a stub by rule 4's own
terms). Bytes affected: the AT-2 row in `evidence/BF3/dispositions.md` and `charters/BA0.md`
lines 28 and 37.

The program orchestrator applied this reading rather than re-asking, on the ground that the ruling
is general and decides the case. If the maintainer intended something narrower for AT-2
specifically, this record is the thing to correct.
