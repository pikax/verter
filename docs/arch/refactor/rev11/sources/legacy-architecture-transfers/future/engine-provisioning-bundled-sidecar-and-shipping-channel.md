# Engine provisioning — Tier 4 (bundled sidecar) has no shipping channel

**Status:** BLOCKED — the premise of "add a packaging step" does not hold. Needs a product
decision about what ships `verter-lsp`, and whether a guarded invariant is relaxed.

**Audit verdict (2026-07-22): BLOCKED.**

**Exact owner decision needed:** identify the artifact that owns `verter-lsp` distribution and explicitly approve or reject shipping a bundled tsgo sidecar by relaxing the guarded “tsgo is never packaged” invariant.

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

1. **`verter-lsp` now has an npm channel.** The `verter-lsp` launcher package ships the
   server per-platform via `@verter/lsp-<platform>` (7 targets, glibc/musl split),
   published from the same `build-lsp` matrix the VSIX consumes. The VSIX also bundles
   the server binary — `stage-bin.mjs` allowlists `verter-lsp` in `bin/` alongside the
   relay shim. What remains missing is a channel for a bundled **tsgo** engine, not a
   channel for the server itself.
2. **The VSIX explicitly forbids shipping tsgo.** `stage-bin.mjs` enforces a strict `bin/`
   whitelist — the staged relay shim plus `EXTRA_ALLOWED_BIN_ENTRIES`
   (`verter-lsp`, `verter-lsp.exe`) — and **fails the build** if anything tsgo-shaped
   appears, including a renamed source whose bytes are tsgo's. "tsgo is NEVER packaged" is
   an enforced invariant with tests.

So "add the packaging step" cannot be executed as briefed for tsgo. Doing it would require
quietly weakening a guarded rule, which was correctly refused. The server binary itself is
not the obstacle — it already ships through both channels above.

## The decision required

**Q1 (answered) — does the VSIX ship the `verter-lsp` binary?** Yes: the whitelist
explicitly permits it and the release copies the per-platform binary into
`packages/vue-vscode/bin/` before packaging, so `findLspBinary`'s bundled branch is live.
Tier 4's obstacle is the engine, not the server.

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

Into a per-platform package build in CI — either the `@verter/tsc-<platform>` family or the
`@verter/lsp-<platform>` family, both of which already stage a released binary into a
platform package. The `lsp` family is the closer fit: it is the artifact whose consumers
need an engine. The VSIX path additionally requires relaxing the whitelist in Q2; nothing
about it is blocked on the server binary any more.

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
