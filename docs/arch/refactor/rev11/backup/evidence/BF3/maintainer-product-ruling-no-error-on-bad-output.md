# Maintainer product ruling — no production error on bad output

**Date:** 2026-08-16  
**Designated maintainer:** Carlos Rodrigues / pikax

## Verbatim ruling

> verter is allowed to output invalid results without failing, the goal of this
> plan is to find the issues offline and fix them, verter will always try to
> send the correct output, but bugs happen and theress no way to detect a bad
> output unless we test and fix issues, we do not return error on bad ouputt
> cases, we fix those whenever possivle

## Normalized product rule

1. A supported request always **tries** to emit the correct products.
2. A compiler/emitter/product bug that yields wrong bytes is **not** a
   production error. Verter does **not** detect-and-refuse “bad output.”
   There is no runtime oracle for that class.
3. The program finds those bugs **offline** (tests, harness, inventory) and
   **fixes the owner** whenever possible.
4. Typed production refusal remains legal only for a **real capability
   boundary** decided from the typed request *before* compilation (example:
   Svelte `ServerGenerate`). Never for fixture identity, a known defect, an
   oracle mismatch, or a miscompile pattern.

## Ratification effect

This ruling ratifies the AMD-009 §1 and §2 no-retraction direction: supported
requests are not converted into production errors because an offline oracle shows
wrong output, and the superseded production retraction mechanism has no continuing
authority.

This ratification does **not** accept BF3 or any immediate correction block and
does **not** unlock B2 or B3. BF3 remains an audit charter, and the live program
ledger is unchanged by this evidence record.
