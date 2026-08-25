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
         probe7-mapper-wire-capture probe8-lsp-session-attach; do
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
Probe 1 is the exception: it is pure measurement (timings and counts have no pass/fail bound that would not
be flaky on a loaded host), so it reports numbers and always exits 0. Read probe 1 as an observation, never
as a passed check.

## What each probe covers

| Probe | Charter item 2 clause | What it establishes |
|---|---|---|
| `probe1-init-timing.mjs` | session initialisation | MEASUREMENT ONLY — construction, cold first snapshot, warm unchanged snapshot; no hang in the in-process spawn path. Asserts nothing. |
| `probe2-stale-snapshot.mjs` | snapshot update | Control, asserted: `updateSnapshot()` does not poll the filesystem. A new snapshot without `fileChanges` serves pre-edit content **by design**. Fails if the server ever observes an edit it was not told about. |
| `probe3-stale-sourcefile-confirm.mjs` | snapshot disposal | The post-dispose asymmetry, asserted in both directions: `Program.getSourceFile` returns the **identical object** with no round-trip, while all four sibling `Program` methods throw `snapshot N not found`. Fails if either half stops holding — including if the defect is fixed. |
| `probe4-filechanges-correct.mjs` | snapshot update | Asserted: passing `fileChanges.changed` makes the next snapshot observe **exactly** the appended byte count, not merely "some change". |
| `probe5-bulk-semantic-api.mjs` | project/source-file lookup, `Program` and `Checker` operations, bulk symbol/type/reference queries, completions, diagnostics, cancellation, failure behaviour | The bulk surface. 50+ checks, each asserting a discriminating property rather than merely reporting a value. |
| `probe8-lsp-session-attach.mjs` | charter item 2 (LSP API-session behaviour) | Spawns `tsc --lsp`, obtains an API pipe via `custom/initializeAPISession`, and attaches a second client. Establishes that attach works, that it is ASYNC-CLIENT-ONLY, and that no session-attach hang occurs. Closes the §4a delegation. |
| `probe7-mapper-wire-capture.mjs` | charter item 1 (exact mapper request/response shapes) | LIVE capture of the content-mapper JSON-RPC protocol: runs the pinned native `tsc --runExternalCode` against a real `contentMappers` config with a stub mapper, and asserts the four-step lifecycle by NAME and ORDER plus every params shape. Closes the wire-spelling gap §3 had delegated. |
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

The probe-7 plant matters for a specific reason: that probe's whole value is the claim that the wire
lifecycle is exactly those four method names. A check that merely printed whatever it captured would
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
