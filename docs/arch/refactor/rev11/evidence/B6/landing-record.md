# B6 — landing record

Base `c37da0ed9`. Accepted candidate `3ae319a23`, tree `8547c26b8`.

This block has no `context-packet.md`; see [Context packet](#context-packet) for
why, and for the ratified instrument that exempts the field.

This record is the acceptance record, not an implementation record. B6's
implementation reached the working branch before its certification was written
down, so what is recorded here is the identity that was certified, the verdicts
that bind to it, and the two exemptions the acceptance rests on. What the block
built is described in its charter and in the commits named below.

## Accepted identity

| | |
|---|---|
| `base_sha` | `c37da0ed9a2883b2e86f4c10d070f88ecb7e2644` |
| `candidate_sha` = `accepted_sha` | `3ae319a2367a35cda0ade86de7e72dec62d8fa16` |
| `candidate_tree` = `accepted_tree` | `8547c26b8faf0781e24e379ce3653906c506e1cf` |

The accepted identity equals the reviewed candidate identity exactly, in both
fields, so no landing-equivalence artifact is required and
`landing_equivalence_digest` stays empty.

The implementation reached the working branch as `1dc0339dc` ("close the
prepared and batch compile routes over one shared closure", the squash of the
earlier candidate `90a91d076`) and then `3ae319a23` ("group batch items by map
probe instead of a linear scan"), which is the accepted identity. `3ae319a23`
is an ancestor of `program/architecture-lock`.

Checked at the time of writing, not taken on report:
`crates/verter_compiler/src/standalone.rs` is blob `022c9a379` at both
`3ae319a23` and the branch tip, so the accepted identity's production surface
is what is live. `base_sha` is an ancestor of the accepted sha. The tree oid
above is `3ae319a23`'s own tree.

## Review mandates

All three PASS, each bound to `3ae319a2367a35cda0ade86de7e72dec62d8fa16` — one
candidate, three mandates, which is what the ledger's stale-SHA rule requires
and what took the longest to establish here.

The verdict history is real work and is preserved rather than re-derived. The
terminal round at `6f05d6fc4` returned architecture PASS and conformance PASS
but adversarial FAIL on one P1 (`B6F-1`: a rustdoc separated from
`compile_client` by an inserted guard, so a five-line SSR check was documented
as running the full pipeline). `B6F-1` was fixed and conformance re-ran,
tier-1 over the doc delta, PASS at `ad1247b75`. A fresh adversarial leg then
ran 18 apply-verified plants against `1f57df816` and returned PASS with three
P3 findings, two since closed. The adversarial mandate was subsequently
re-bound to the accepted candidate; the delta it had to cover is test- and
docs-only, which is what allows the re-bind rather than a fresh lane.

The re-bind was validated through `scripts/orchestration/check-results.mjs`
over the three verdict artifacts — exit 0, all sound, three results, zero
blockers — and the round-1 conformance FAIL was verified preserved alongside
the round-2 PASS rather than overwritten in place, since a FAIL replaced by a
PASS in the same file is the shape evidence tampering takes. The verdict
artifacts themselves are session-local and are not in the tree.

## Context packet

No immutable dispatch packet was produced when B6 was dispatched, and none
survives anywhere in history. Writing one now, after implementation, would be
a fabricated input artifact backdated to look like a record of something that
did not happen that way, so `context_packet_digest` stays empty.

The exemption is not asserted by resemblance to the three ids already
grandfathered. It comes by
[`AMD-014-b6-context-packet-legacy-gap.md`](../../amendments/AMD-014-b6-context-packet-legacy-gap.md),
ratified 2026-08-24, which is the explicit written instrument
`MAINTAINER-RULING-2026-08-22-CODE-OVER-LEDGER.md` §1 requires before a fourth
id may join its enumerated set. The amendment is registered in
`authority-registry.toml` and bound to its exact bytes. It exempts one field
for one id; every other identity, digest and review field on this row is
enforced unchanged.

## Performance

The four-arm route-overhead measurement was taken and is recorded at
`1c9018d96`
([`B6_COMPILER_ROUTE_OVERHEAD-measurement.md`](../../../../architecture-lock/ledger/B6/B6_COMPILER_ROUTE_OVERHEAD-measurement.md)):
median wall of 1.757 ms direct, 1.116 ms prepared-first, 4.457 ms
prepared-repeat, 1.814 ms batch over 30 measured cold process invocations
within a 46-invocation session; nothing refused, every invocation exit 0, and
a single output digest across all 184 arm-runs. The measured binary's SHA-256
is recorded there, built from `3ae319a23`.

That record states its own limits and they are left standing, not repaired
here: it ran `compiler_route_overhead.rs` (7 fixtures) while the
`B6_COMPILER_ROUTE_OVERHEAD` cell's `corpus_fingerprint` pins
`route_overhead_baseline.rs` (8 sources), it emits none of the cell's metric
names, and the `pre-measure-registration.md` §5 conjunct `load1 < 2.00` was
not met on this host (2.41–2.54 observed). The cell's `corpus_fingerprint` was
re-pinned, not re-measured, per the `B6F-2` ruling.

A protocol-conformant re-measurement is **waived by maintainer instruction for
this block's exit against `B6_COMPILER_ROUTE_OVERHEAD`**. The waiver's scope is
its content and must not be compressed to "measurement waived": it covers B6's
exit only. The cell remains in force for every other consumer, and the separate
finding that the cell's pinned harness refuses three of the four arms it locks
(`Q2-GATE`, `ARCHITECT-RULING-2026-08-24-SIX-WAY-B6-CM1.md`) stays a live
program-level defect owned by BF1, independent of this acceptance.

## Descoped

The final canonical gate on the certified tree was descoped by the same
maintainer instruction. The proportionate residue was honoured: the change that
edits `scripts/validate-program-state.mjs` had that script's own tests run
(75 pass, 0 fail), which is a targeted check within remit, not a reinstatement
of the full gate.

## Checked at landing

- Validator, live mode with `--authority`: exactly one violation, and it is
  CM1's fixed-landing-order replay conflict — the standing pre-existing
  baseline. The cumulative-tree oid inside that message moves run to run; the
  block, the two named files and the count are what is compared.
- `node --test scripts/validate-program-state.test.mjs`: 75 pass, 0 fail.
- `node scripts/effective-state.mjs`: 0 findings, no contradictions.
- `shasum -a 256 docs/arch/refactor/rev11/charters/B6.md` equals both the
  `B6-CHARTER` row in `authority-registry.toml` and the `charter_digest`
  written on the row.
- `AMD-014`'s file digest equals its registered `sha256`.
- Exactly one `[[authorization]]` row exists for B6; no second one was created.

## Successors

C2, C4, F1 and H1 list B6 as a predecessor and are unblocked by this record.
