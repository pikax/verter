# Engine provisioning — Tier 4 (bundled sidecar) has no shipping channel

**Status:** BLOCKED — the premise of "add a packaging step" does not hold. Needs a product
decision about what ships `verter-lsp`, and whether a guarded invariant is relaxed.

## Symptom

Tier 4, Verter's offline floor, is fully specified and never staged. On any machine that
cannot satisfy tiers 0–3, Verter has no engine.

## Mechanism

The contract exists and is complete:

- Sidecar location `<exe-dir>/tsgo/lib/tsc[.exe]` plus a `verter-tsgo-bundle.json`
  integrity manifest (`crates/verter_tsgo_api/src/toolchain/bundle.rs`).
- `BUNDLED_TSGO_VERSION = 7.0.2` (`crates/verter_tsgo_api/src/toolchain/policy.rs:25`).
- Validation implemented, including the distinction that a bundled binary which EXISTS but
  fails validation is a product-integrity failure, not a "no provider" outcome.

What is missing is not a packaging step but a **channel**:

1. **No distribution currently ships `verter-lsp` at all.** The only binary-shipping npm
   package is `verter-tsc`, via `@verter/tsc-<platform>`.
2. **The VSIX explicitly forbids shipping tsgo.** `stage-bin.mjs` prunes the extension's
   `bin/` directory to *exactly* `[verter-relay-shim]` and **fails the build** if anything
   tsgo-shaped appears. "tsgo is NEVER packaged" is an enforced invariant with tests.
3. Consequently `findLspBinary`'s bundled branch (`<extensionPath>/bin/verter-lsp`) is
   **dead code** — staging deletes the file it looks for.

So "add the packaging step" cannot be executed as briefed. Doing it would require quietly
weakening a guarded rule, which was correctly refused.

## The decision required

Two coupled questions:

**Q1 — Does the VSIX ship the `verter-lsp` binary?** Today it does not, and the bundled
lookup branch is dead. If the extension is expected to work standalone, this must be
answered before tier 4 means anything in an editor context.

**Q2 — Is the "tsgo is NEVER packaged" invariant intentional and permanent?** It is
currently enforced by a build failure and covered by tests, which means it was a deliberate
decision by someone. Tier 4 is *definitionally* the act of packaging a tsgo engine. The two
cannot both stand. Either:

- the invariant is intentional ⇒ **tier 4 should be struck from the provisioning policy**,
  not left as a permanently unbuildable tier, and the policy becomes three usable tiers
  plus an optional download tier; or
- the invariant predates the provisioning policy ⇒ it must be relaxed **deliberately**,
  with the reasoning recorded and its guard updated, before any sidecar staging lands.

## Where the sidecar would go, if approved

Into the `@verter/tsc-<platform>` platform-package build in CI — that is the only existing
channel that ships a binary. The VSIX path additionally depends on Q1.

## Blast radius

- Leaving tier 4 unimplemented: machines that satisfy no other tier get no engine, reported
  honestly (see the tier-3 document — tier 3 is the other candidate offline answer, itself
  blocked).
- Implementing it: increases artifact size by an engine per platform, and relaxes a rule
  that currently has teeth. The integrity-manifest validation already exists to make a
  corrupt shipped engine loud rather than silent, so the failure mode is covered.

## Related

- `engine-provisioning-download-tier.md` — tier 3, blocked on a dependency decision. If
  tier 3 is rejected, tier 4 becomes the sole offline floor and this decision becomes
  correspondingly more urgent; if tier 4 is struck, tier 3 becomes the sole one.
  **Rejecting both leaves Verter with no offline engine story at all** — that is the
  combination to avoid deciding by accident.
