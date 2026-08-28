# TCM0 semantic-API probes

Executable probes against the candidate package `typescript@7.1.0-dev.20260822.1`, backing the claims in
`../package-lock-and-semantic-api.md`. Every probe performs real RPCs against the candidate's native
binary; nothing here is read out of a `.d.ts`.

## Provenance of these files

`package-lock-and-semantic-api.md`'s Reproduction block named four probe scripts
(`probe1-init-timing.mjs`, `probe2-stale-snapshot.mjs`, `probe3-stale-sourcefile-confirm.mjs`,
`probe4-filechanges-correct.mjs`) that were **never committed** — the citation named files that did not
exist anywhere in the repository. Probes 1-4 here are re-created from the behaviours that document
records and re-executed against the same package; `transcript.md` is the output of that re-run, not a
transcript of the original uncommitted run. Probes 5 and 6 are new and cover the bulk-method surface
charter item 2 requires.

## Running them

The probes refuse to run against any package other than the pinned candidate version, so they cannot be
silently satisfied by whatever `typescript` happens to be installed in this repository.

Or regenerate the committed transcript in one step: `./regenerate-transcript.sh /tmp/tcm0-probe`.

```bash
# 1. Install the candidate into a scratch directory OUTSIDE this repository.
mkdir -p /tmp/tcm0-probe && cd /tmp/tcm0-probe
echo '{"name":"tcm0-probe","private":true,"version":"0.0.0"}' > package.json
npm install typescript@7.1.0-dev.20260822.1 --no-save
# The matching @typescript/typescript-<platform>-<arch> native binary arrives via optionalDependencies.

# 2. Run each probe, pointing it at that directory.
cd <repo>/docs/arch/refactor/rev11/evidence/TCM0/probes
for p in probe1-init-timing probe2-stale-snapshot probe3-stale-sourcefile-confirm \
         probe4-filechanges-correct probe5-bulk-semantic-api probe6-out-of-range-completion-panic \
         probe7-mapper-wire-capture probe8-lsp-session-attach \
         probe9-transform-response-contract probe10-external-source-unit; do
  node "$p.mjs" --ts /tmp/tcm0-probe
done
```

`TS_CANDIDATE_DIR=/tmp/tcm0-probe` works in place of `--ts`. Each probe builds its own throwaway fixture
project in an OS temp directory and removes it afterwards, so no fixture is committed into the
documentation tree and no repository state is touched.

**The version guard is real, not decorative.** `harness.mjs` resolves the package from the directory you
name and refuses to proceed if it is not the pin — verified by pointing a probe at a scratch install of
`typescript@7.0.2`, which exits 1 with `refusing to run: resolved typescript@7.0.2 … expected
7.1.0-dev.20260822.1`. Running with no `--ts` and no `TS_CANDIDATE_DIR` likewise exits 1 rather than
silently resolving whatever `typescript` the repository happens to have installed.

**Exit status.** Probes 2-8 exit 0 only when every one of their assertions held, and non-zero otherwise.
Probe 1 is nearly the exception, and the difference matters: its TIMINGS assert nothing (no count or
duration has a pass/fail bound that would not be flaky on a loaded host), but the probe is not
assertion-free — it carries exactly one, at `probe1-init-timing.mjs:85`, that every iteration completed,
which is the charter's actual liveness question and fails the probe if the cold path ever hangs. So probe 1
exits non-zero on that one condition and zero otherwise. Read every NUMBER probe 1 reports as an
observation and never as a passed check; read its exit status as the single liveness assertion it is.

## What each probe covers

| Probe | Charter item 2 clause | What it establishes |
|---|---|---|
| `probe1-init-timing.mjs` | session initialisation | MEASUREMENT ONLY for its numbers — construction, cold first snapshot, warm unchanged snapshot. Its one assertion (`:85`) is that every iteration completed, i.e. no hang in the in-process spawn path; no timing is asserted. |
| `probe2-stale-snapshot.mjs` | snapshot update | Control, asserted: `updateSnapshot()` does not poll the filesystem. A new snapshot without `fileChanges` serves pre-edit content **by design**. Fails if the server ever observes an edit it was not told about. |
| `probe3-stale-sourcefile-confirm.mjs` | snapshot disposal | The post-dispose asymmetry, asserted in both directions: `Program.getSourceFile` returns the **identical object** with no round-trip, while the four probed siblings — `getSemanticDiagnostics`, `getSourceFileNames`, `emitToString`, and `getSyntacticDiagnostics` — throw `snapshot N not found`. Fails if either half stops holding — including if the defect is fixed. |
| `probe4-filechanges-correct.mjs` | snapshot update | Asserted: passing `fileChanges.changed` makes the next snapshot observe **exactly** the appended byte count, not merely "some change". |
| `probe5-bulk-semantic-api.mjs` | project/source-file lookup, `Program` and `Checker` operations, bulk symbol/type/reference queries, completions, diagnostics, cancellation, failure behaviour | The bulk surface. 50+ checks, each asserting a discriminating property rather than merely reporting a value. |
| `probe8-lsp-session-attach.mjs` | charter item 2 (LSP API-session behaviour) | Spawns `tsc --lsp`, obtains an API pipe via `custom/initializeAPISession`, and attaches a second client. Establishes that attach works, that it is ASYNC-CLIENT-ONLY, and that no hang was observed on the exercised attach path. TCM3's duty to run the session-attach probe before selecting that topology is ratified by `docs/arch/refactor/rev11/rulings/MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md` §2. |
| `probe7-mapper-wire-capture.mjs` | charter item 1 (mapper request capture) | LIVE capture of one content-mapper JSON-RPC compile: runs the pinned native `tsc --runExternalCode` against a real `contentMappers` config with a stub mapper, and asserts the captured four-step lifecycle by NAME and ORDER plus the params shapes exercised in that compile. Probe 7 establishes that request sample; Probe 9 derives the response body, including the `diagnosticDirectives` entry layouts, with the four remaining limits recorded in `../closure-register.md` row `S1.d`. |
| `probe9-transform-response-contract.mjs` | charter item 1 (exact mapper request/response shapes) | The successful `transform` RESPONSE contract, derived by driving the pinned compiler and reading the decoder's typed errors: the flat `{extension, text, mappings?, supplemental?, diagnostics?, diagnosticDirectives?}` object, the 5-or-6-number mapping tuple and its bounds, the three `SpanMapKind` values, and the extension domain over 14 tested values. Records the vacuous-pass trap that kept the gap open: `content` is an IGNORED field, and `{extension, content}` "succeeds" only because an empty program type-checks. |
| `probe10-external-source-unit.mjs` | charter item 6 (`<template src>` under the steering's model 2) | Transform-input, project and configuration identity for a file referenced from inside a carrier. Each assertion pairs with an injectable rival hypothesis (`--inject input|project|config|mapper`), all four observed driving it red. |
| `probe6-out-of-range-completion-panic.mjs` | failure behaviour | Asserted in both halves: an out-of-range completion position produces a Go `slice bounds out of range` panic with a stack trace on the client, **and** the session is still serving afterwards. Fails if the panic stops reproducing or if it stops being contained. |

## Discrimination

Every `check()` body in probe 5 contains at least one `assert()` — verified mechanically, currently 49 of
49, plus 4 `checkThrows()`. For example the references checks assert that the declaration symbol finds
**zero** references in an importing file *and* that the file-local alias symbol finds several, so the pair
fails if a project-wide references primitive ever appears. Observations that have no pass/fail bound
(timings, raw samples) are emitted through `record()`, which never claims a pass.

**This was not true when the probes first landed, and the correction is worth recording.** In the first
version, five of the six probes contained no assertions at all and exited 0 whatever the package did, and
thirteen of probe 5's `check()` bodies only reported a value. Two of those thirteen guarded constraints
this evidence declares BINDING on TCM2/TCM3 — the auto-import completion rejection (§6.2(c)) and the
beyond-EOF position degrade (§6.2(e)) — and both would have reported PASS had the behaviour reversed. A
constraint declared binding on a downstream block, guarded by a check that cannot fail, is worse than an
unguarded constraint, because the next reader sees a probe and stops looking.

Both are now proven to discriminate by planting the reversal and observing red:

| Guard | Plant | Result |
|---|---|---|
| §6.2(c) auto-import rejection | point the check at a member-access position, where completion succeeds | `FAIL … returned 3 entries instead of refusing — evidence 6.2(c) no longer holds`, exit 1 |
| §6.2(e) beyond-EOF degrade | point the check at a file outside the project, where the callee fails closed | `FAIL … it FAILED CLOSED … so evidence 6.2(e) no longer holds`, exit 1 |
| probe 7's wire method list | assert a lifecycle of `initialize,openProject,mapFile,closeProject` | `FAIL … captured [initialize,openProject,transform,closeProject]`, exit 1 |

Each plant was verified present, unique and new in the source before the run, and the unplanted copy was
re-run as a control and stayed green.

The probe-7 plant matters for a specific reason: that probe establishes that the captured compile's
inbound lifecycle uses those four method names in that order. A check that merely printed whatever it captured would
"establish" any protocol at all. Asserting a wrong name goes red, so the assertion is load-bearing rather
than descriptive.

Probe 3 is worth singling out: it asserts a **defect**. If a future package fixes the post-dispose
asymmetry, probe 3 goes red — which is the correct signal, because the design constraint it justifies would
no longer be necessary. It is a characterisation test, not a regression guard.

## Staleness against a future package

The version guard pins every probe, and `transcript.md` records a run against
`typescript@7.1.0-dev.20260822.1` specifically. A later candidate package could behave differently, which
raises the question of whether this transcript goes stale.

**It does not, and the gap is acceptable.** `docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-2026-08-24-TCM0-DECISIONS.md`
Q7: the transcript is **immutable evidence for its exact pinned package**, so a future package cannot make
it stale — it says what that build did, and that stays true. **TCM4 owns future-package verification at the
certified-engine gate: its mapper-conformance and semantic-capability probes must pass before
activation.** Re-running these probes against a newer package is therefore TCM4's act at that gate, not a
maintenance obligation hanging over this directory.

## Tree derivations (not package probes)

Two scripts here read the REPOSITORY rather than the pinned candidate package, so they need no `--ts`
and no network:

| Script | Backs | What it derives |
|---|---|---|
| `closure-validator.mjs` | acceptance admission | Derives every sentence in the charter's Scope, acyclic-invariant, and Acceptance sections, requires verbatim row claims to tile them, and refuses acceptance while any mandatory register row is open. |
| `capability-provider-hop-walk.mjs` | the ownership ledger's capability verdicts | Walks each steering-named capability's request path from its LSP entry point, reporting the shortest path to a provider hop (a call to a method derived from the trait body, or a read of the `type_provider` handle). Edges resolve by CALL SHAPE, not name alone. Writes `../capability-provider-hop-walk.md`. |
| `typeprovider-call-site-derivation.mjs` | the ownership ledger's caller column | Reads the method list out of the `TypeProvider` trait body and lexes every `.rs` file under `crates/`, classifying every occurrence of every method: trait declaration, implementation, production call, same-name forwarding call, trait default-body call, test call, bare reference, or a mention in a comment or string. Writes `../typeprovider-call-sites.md`. |

`typeprovider-call-site-derivation.mjs --check` re-derives from the live tree and exits 1 on any drift —
a new call site, a deleted one, or a new trait method. Its own header states what a textual derivation
cannot see (identifier collisions, generic-parameter receivers, a method reached under a renamed alias,
macro-pasted identifiers) and why its counts are an upper bound on true provider call sites rather than a
lower one.

It is proven to discriminate by planting, each verified present, unique and absent-before-planting, then
reverted:

| Plant | Expected | Result |
|---|---|---|
| a new `fn` in the `TypeProvider` trait body | the method count comes from the trait, not from a list | `44` → `45` |
| a call in a non-test `impl` (`tsgo/composite.rs`) | classified `call-production` | classified `call-production` |
| a call in a `#[cfg(test)] mod`-gated file (`real_provider_tests/completion.rs`) | classified `call-test`, NOT production | classified `call-test` |

`--check` exited 1 under the plants and 0 after reverting them. The third plant is the load-bearing one:
several directories of test code live under `src/` with no `#[cfg(test)]` in their own files, gated only
by a `#[cfg(test)] mod NAME;` declaration in a parent, and a derivation that read in-file attributes
alone would have promoted hundreds of test calls into the production column.

`capability-provider-hop-walk.mjs --check` behaves the same way. Its header records the four false rails
that earlier revisions reported before the resolution rules were tightened — `HandlerGuard::new`,
`Box::new`, `tokio::spawn` and `crate::type_provider::specifier_rewrite` each handed every capability a
hop through code it never calls — because a walk that answers HOP for everything answers nothing, and the
next reader is owed the reason each rule exists.

Proven to discriminate: a two-edge provider call planted under the `directives` entry point
(`features/hover_directive_names.rs`), verified present, unique and absent-before-planting, flipped that
capability from `NO-HOP` to `HOP`, named both edges and printed the planted line; `--check` exited 1 under
the plant and 0 after reverting it.
