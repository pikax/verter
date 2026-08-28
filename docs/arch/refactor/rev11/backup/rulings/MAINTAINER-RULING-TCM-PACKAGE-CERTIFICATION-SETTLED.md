---
ruling_id: "TCM-PACKAGE-CERTIFICATION-SETTLED"
type: "maintainer-ruling"
date: "2026-08-23"
date_source: "stated"
binds: ["TCM0", "TCM1", "TCM2", "TCM3", "TCM4"]
source_file: "MAINTAINER-RULING-TCM-PACKAGE-CERTIFICATION-SETTLED.md"
summary: "Certifies typescript@7.1.0-dev.20260822.1 for PRODUCTION activation, upgrading package-lock-and-semantic-api.md §5's 'candidate-discovery only' hedge, and closes package identity/version selection for the TCM0-TCM4 train. Does not waive the reproduced stale-Program-after-Snapshot-dispose defect (§4c), which stays a required TCM3 design constraint, or the two open verification gaps (exact wire method-name spelling, owned by TCM2; the API.fromLSPConnection session-hang probe, owned by TCM3). Supersedes an external architecture-consult finding that read the §5 hedge as a production blocker; does not itself accept TCM0."
supersedes:
  - document: "an architecture consult (recorded as input to this integration, not an in-tree document this ruling edits)"
    claim: "typescript@7.1.0-dev.20260822.1 should not be certified for production use, reading package-lock-and-semantic-api.md §5's 'not for production activation' hedge as a live blocker."
superseded_by: []
contradicts: []
notes: "Closes the one open certification hedge TCM0's own package-lock-and-semantic-api.md §5 left; grants no authority to skip TCM0's own three independent reviews (conformance_review/architecture_review/adversarial_review in program-state.toml)."
---

# MAINTAINER RULING — TCM package certification is SETTLED

**Status:** RATIFIED by the maintainer, 2026-08-23.
**Scope:** the candidate-package half of TCM0 §1-2 (`evidence/TCM0/package-lock-and-semantic-api.md`),
and its downstream effect on TCM0's own acceptance and on TCM1-TCM4's material bounds.

## The ruling

`typescript@7.1.0-dev.20260822.1` **is the correct, certified candidate** for the TCM0-TCM4 train.
The maintainer states directly: "the probe passed previously so the mentioned version is correct."

This upgrades `package-lock-and-semantic-api.md` §5's own verdict — "Certified for candidate-discovery
purposes, not for production activation" — to **certified for production activation**, subject only to
the design constraints already recorded against this exact package (below). Package identity/version
selection is CLOSED for this train; TCM1-TCM4 do not need to re-run package discovery or re-litigate
whether a later `7.1.x` build should be preferred instead.

## What this does NOT reopen or waive

This ruling certifies the *package*. It does not waive any correctness requirement TCM0 already
recorded against it:

1. **The reproduced stale-`Program`-after-`Snapshot`-dispose defect** (`package-lock-and-semantic-api.md`
   §4c) **is real and stays recorded as evidence.** It is not disputed, not retested, and not deleted.
   It remains, as TCM0 already ruled, a **required TCM3 design constraint** — never retain a
   `Program`/`Checker` handle past its owning `Snapshot`'s `dispose()`, enforced structurally (a
   type-state guard) where the surrounding language allows it. See
   `tcm1-tcm4-charter-refinements.md`'s TCM3 section and TCM3's rewritten charter (Owned scope item
   naming this constraint, Forbidden section naming the anti-pattern it rules out).
2. **The two open verification gaps stay open**, gated to the blocks that must close them, not to TCM0:
   - exact wire method-name spelling for the content-mapper protocol — TCM2 must close it (live protocol
     trace or `typescript-go` source read) before claiming byte-exact protocol fidelity;
   - the `API.fromLSPConnection` (session-attach) topology candidate was not probed for the
     session-initialization-hang defect class — TCM3 must run that probe itself before selecting that
     topology candidate, not inherit TCM0's certification by association.
3. **No relay/carrier workaround is authorized by this ruling.** The charter's own rule stands: if a
   *future* candidate build fails a required correctness probe, the response is to select a later
   certified package or keep TCM4 blocked — never to add a hidden relay fallback. This ruling only
   settles that the *current* candidate, as already probed, clears the bar.

## Supersession, not deletion

An architecture consult (recorded as input to this integration, not as an in-tree document this ruling
edits) flagged the reproduced stale-handle probe result as a reason `typescript@7.1.0-dev.20260822.1`
should not be certified for production use — reading `package-lock-and-semantic-api.md` §5's own
"not for production activation" hedge as a live blocker.

**That finding is SUPERSEDED by this ruling.** It is recorded here as superseded, not deleted from
`package-lock-and-semantic-api.md` — the underlying defect observation (§4c) and the honest verdict
hedge (§5) stay exactly as TCM0 wrote them; only their downstream consequence changes, from "blocks
production certification" to "binds TCM3 as a design constraint," per this ruling's authority. See the
cross-reference added to `package-lock-and-semantic-api.md` §5.

## Effect on TCM0 acceptance

This ruling closes the one open certification question TCM0's own text left as a verdict hedge (§5).
It does **not** itself accept TCM0 — TCM0's three independent reviews
(`conformance_review`/`architecture_review`/`adversarial_review` in
`docs/arch/architecture-lock/ledger/program-state.toml`) have not run, and this ruling grants no
authority to skip them. What it does is remove the one substantive finding an adversarial or
architecture reviewer could otherwise have raised as a package-certification blocker.

## Effect on TCM1-TCM4

None of TCM1-TCM4's owned scope changes. This ruling is cited as material-bounds authority in each
rewritten charter's Material Bounds section, wherever a bound depends on the certified package's
measured behaviour (e.g. TCM0's locked performance baselines in `performance-baselines.md`, gathered
against this exact candidate).
