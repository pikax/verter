---
ruling_id: "AT2-NAMED-ACT"
type: "maintainer-directive"
date: "2026-08-17"
date_source: "stated"
binds: ["BF3", "BA0", "AT-2 finding row"]
source_file: "MAINTAINER-ACT-AT2.md"
summary: "Explicit maintainer act (requested after a review seat correctly refused to infer authority from an unnamed general ruling): rejects AT-2's claim that a reachable batch entry publishes a product beside a genuine typed refusal; reclassifies AT-2 as a latent HostBacked construction hazard with reachability unproven; retains the DEFER to BA0; carries it as an #[ignore]d characterization test; drops the required-RED Svelte-refusal atomicity target. Authorizes exactly the bytes already in the tree (evidence/BF3/dispositions.md AT-2 row, charters/BA0.md lines 28 and 37); no production guard, typed refusal, withhold path, retraction, or removal ID."
supersedes: []
superseded_by: []
contradicts: []
notes: "Does NOT accept BF3, does NOT accept BA0, does NOT unlock B2/B3. Clarified by MAINTAINER-ACT-AT2-CLARIFICATION.md (same date) on two scope points the seat also declined to infer."
---

# Maintainer act — AT-2 amendment, named explicitly (2026-08-17)

Maintainer: Carlos Rodrigues <carlos@hypermob.co.uk> (GitHub: pikax), designated maintainer.

## Why this act exists

The AT-2 row was amended in the tree by inferring authority from the maintainer's GENERAL
bugs-and-types standing ruling of 2026-08-17. A review seat blocked that inference: the general
ruling never NAMES AT-2, and a general act does not authorize a change to a specific ratified
findings row. The seat was correct — acting on an unnamed authority is the same governance defect
that blocked this block once already. The maintainer was asked directly and issued the act below.

## The act

> Reject AT-2's claim that a reachable batch entry publishes a product beside a genuine typed
> refusal; reclassify AT-2 as a latent HostBacked construction hazard with reachability unproven;
> retain the DEFER to BA0; carry it as an `#[ignore]`d characterization test; and drop the
> required-RED Svelte-refusal atomicity target.

## Scope

- Authorizes exactly the bytes already in the tree: the AT-2 row in
  `evidence/BF3/dispositions.md` and `charters/BA0.md` lines 28 and 37.
- The rest of the ratified findings table is byte-unchanged and stays that way.
- Authorizes NO production guard, typed refusal, withhold path, retraction, or removal ID.
- Does NOT accept BF3, does NOT accept BA0, and does NOT unlock B2/B3.

## Evidence this act rests on

All nine `CompileBatchEntry` construction sites enumerated in `host_compile.rs`: eight are atomic by
hardcoded literal; the typed refusal (`RuntimeSurfaceRefused`) lands on an atomic arm publishing no
product; the single non-atomic site (the HostBacked `Ok(response)` path) has no demonstrated
reachable input. A later plant confirmed the hazard is real as a CONSTRUCTION property — injecting an
error-severity diagnostic into the success response downstream of the routing gate produces an entry
carrying a 480-byte product beside the error — while reachability through a real request remains
unproven and was probed without reproduction. The previously-cited gating test drove a different
failure class entirely (duplicate-canonical conflict), which publishes nothing.

## Effect

The seat's authority objection is discharged. Charter procedure item 6 is no longer blocked on AT-2.
One acceptance blocker remains and is unrelated: `architecture_review` is NOT_PROVEN because only two
review seats were commissioned in the closing round — an orchestrator scoping error. BF3 is
foundational class and requires all three mandates PASS, so a full architecture seat is still owed.
