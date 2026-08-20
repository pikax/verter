# Five accepted blocks carry no ratification record

**Status:** RESOLVED 2026-08-20 — the maintainer chose disposition 2,
retroactive ratification, and approved all five. The authorizing document is
[`MAINTAINER-RULING-PRE-ENFORCEMENT-ACCEPTANCES.md`](../../refactor/rev11/rulings/MAINTAINER-RULING-PRE-ENFORCEMENT-ACCEPTANCES.md);
each block now carries an `[[authorization]]` record citing it, dated to that
ruling rather than to the original transition. Live-mode validation passes.

The ratification covers exactly these five blocks and grants no standing
exemption — every transition after enforcement landed is bound by the registry.

The record below is kept as the account of what was found and why.

## What the machine reports

Live-mode ledger validation now fails:

```
$ node scripts/validate-program-state.mjs \
    --dag docs/arch/refactor/rev11/program-dag.toml \
    --state docs/arch/architecture-lock/ledger/program-state.toml \
    --mode live

VIOLATION: state block BF1 is ACCEPTED — past LOCKED — but authority registry
  has no [[authorization]] record for it
VIOLATION: state block BF2 is ACCEPTED — ...
VIOLATION: state block B2  is ACCEPTED — ...
VIOLATION: state block B3  is ACCEPTED — ...
VIOLATION: state block B4  is ACCEPTED — ...
FAIL: 5 violation(s) (mode live)
```

**BF1, BF2, B2, B3, B4** left `LOCKED` without a digest-bound, ratified
authorization record.

## Why this surfaced now, and why it is not new breakage

The authorization registry and its enforcement landed in `2efa644a7` +
`a7b07d31b`. Before that, nothing checked whether a block had authority to leave
`LOCKED`; the rule existed in prose only, and prose does not fail a build. The
five transitions happened under the unenforced regime. Enforcement did not break
them — it made an existing gap visible.

This is the gap the workstream was commissioned to close: *"the main remaining
risk is the orchestration machinery incorrectly certifying an invalid
transition."* The machinery was certifying five.

## What was checked before writing this

The implementer searched the ruling corpus and the amendment records for a
document that genuinely authorizes each of the five, and found none. No rows
were invented to clear the failure. A fabricated authorization is precisely the
failure mode the registry exists to prevent, and it would be undetectable
afterwards — which is why the honest failing state is the correct one to leave.

## Consequence while this is open

**Live-mode validation fails for every future transition**, because these five
violations are always present. Recording any new block transition therefore
cannot be validated cleanly until this is dispositioned. `--no-authority`
suppresses the check but defeats its purpose and must not become the routine
invocation.

CI is unaffected: `.github/workflows/ci.yml` runs the validator's *test suite*,
not the validator against this ledger. Nothing is red that was green.

## The four ways out

1. **Genuine authority exists and was missed.** Point at the document for each
   block; the record is written from it — real id, kind, path, recomputed
   `sha256`, and the ratifier and date taken from the document itself.
2. **Retroactive ratification.** The maintainer ratifies these five transitions
   now, and each record cites that ratification with its own date, stating on
   its face that it is retroactive.
3. **A scoped, recorded exemption.** The five are grandfathered as
   pre-enforcement transitions via an explicit registry construct, bounded to
   exactly these five and dated, so the exemption cannot silently widen. This
   keeps enforcement live for everything after it.
4. **The transitions were invalid.** The blocks return to an earlier status and
   re-enter through the enforced path.

Option 3 is the smallest change that restores a working gate without asserting
anything untrue about the past. Option 2 asserts more and needs the maintainer
to actually mean it. This document takes no position beyond that observation;
the choice is the maintainer's.

## Do not

- Do not write authorization rows for these five without a real document behind
  each one.
- Do not weaken the check, and do not make `--no-authority` the default
  invocation.
- Do not resolve this by editing the ledger's status fields to hide it.
