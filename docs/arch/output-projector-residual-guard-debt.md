# Output Projector Residual Guard — Accepted Resolver Edge Cases

**Status**: ACCEPTED defense-in-depth debt. The cross-sink guard
(`cross_sink_raw_authority_to_type_expr_boundary` and the `resolve_type_ref`
identity classifier it drives, in
`crates/verter_session/tests/cases/output_projector_residual_guards.rs`) is NOT
the production boundary; the sealed publication tokens
(`AdmittedPublishedMember`), the sealed `ResolvedSurfaceAccess` trait (`E0603`),
and the sink-local `pub(in <sink-module>)` mint constructors are the
COMPILER-ENFORCED primary. This guard is a defense-in-depth drift detector behind
that primary.

**Ruling source (codex-DEFER, binding)**: terminal architecture consult
`8a3i2-consult-8020-terminal` (2026-06-24; `gpt-5.5` / `xhigh`, neutral framing
dispatcher-verified, trailing `__DONE__`, ratified). The consult judged the guard
against an 80/20 acceptance bar and ruled **ACCEPT / LAND with the claim
narrowed** for its defense-in-depth role: the guard clears the bar for the
current production reference shapes; the residual gaps below are real but
edge/adversarial for this crate's references, not common sink-token reference
patterns, and the guard is not load-bearing. The consult REJECTED a directed
resolver fix ("would mostly tighten adversarial shapes, not common current
references") and a conservative-fire redesign ("not required by the bar … would
trade a working discriminating guard for more manual allowlist pressure"). The
prior resolve-by-proof tightening approach was empirically non-convergent across
four review passes; this debt row records the terminal decision to STOP
tightening.

## What the guard covers (the common in-tree paths — 80/20 coverage)

The `resolve_type_ref` classifier resolves the reference shapes that genuinely
occur in this crate:

- own-module definitions;
- rooted `crate` / `self` / `super` direct paths (a relative `super` rebased onto
  the referencing module, never escaping above the crate root);
- exact `pub` / `pub(crate)` re-exports (target module the candidate's real home
  exactly, keyed by the normalized absolute written path);
- ordinary file imports (a `use` import, incl. `as Alias` renames, whose target
  resolves by proof);
- the audited `registry_decl` private `use`-binding chain (the genuine
  `super::ResolvedTypeDeclaration` chain through a parent module's private
  `use`).

Each of the three sanctioned tokens (`AdmittedPublishedMember`,
`ResolvedVueSurface`, `SvelteResolvedSurface`) has exactly one struct definition
in the crate, so even an over-resolution by a residual shape below lands on the
single genuine def — there is no same-named decoy to forge, and the candidate set
is name-keyed.

## Accepted residuals (OUTSIDE the guard's proof claim)

These forged shapes are adversarial relative to the crate's current production
sink-token references; the classifier may resolve them without proof. They are
disclosed by ROOT-CAUSE CLASS — the two classes below are **complete by
construction**: any specific instance is subsumed by its class, so this list is
NOT extended per-instance.

**Class A — syntactic `use` collection.** All three `use`-collectors are
syntactic: none evaluates item-level `cfg` / `cfg_attr`, and the file-import
collector additionally ignores module nesting.

- `collect_use_index` (file imports) is FILE-WIDE: it walks the whole file and
  inspects neither `u.attrs` nor module nesting. So a `cfg` / `cfg_attr`-gated
  `use` item is indexed WITHOUT evaluating the active cfg, AND a `use` item inside
  an INLINE module (including a `#[cfg(test)] mod`) is treated as an ordinary file
  import for every def in the file.
- `collect_reexport_index` (the `pub`/`pub(crate)` re-export proof rail) and
  `collect_use_binding_index` (the intra-crate use-binding chain) likewise index
  `use` items WITHOUT evaluating item-level `cfg` / `cfg_attr`, so a cfg-gated
  re-export proof entry or use-binding-chain edge can be recorded for a binding
  the active build does not have.
- (The `mod_is_cfg_test` skip applies to the SINK-FN collector's
  `visit_item_mod`, NOT to any of these `use`-collectors — so "the scanner skips
  `#[cfg(test)]` modules" is true only for the sink-fn collection, not for `use`
  collection.)

**Class B — non-proof bare-name fallback.** The unqualified arms resolve by
uniqueness / first-match rather than proof whenever a unique single PROVEN target
is not found.

- the `candidates.len() == 1` global-uniqueness fallback (arm (d)) is reached for
  a no-import bare name, an AMBIGUOUS multi-target same-name `use`
  (`UseIndex::unique_path` is `None` for >1 target), AND a unique SINGLE-SEGMENT
  self-import (`use Foo;` — the recursion guard skips the import-claim arm because
  the import path IS the bare name).
- the use-binding CHAIN (`resolve_use_binding_chain`) returns the FIRST accessible
  target that resolves, not a single proven one (this over-resolves even with a
  single cfg-gated target, per Class A).
- a qualified UNROOTED unshadowed path raw-suffix matches a collected unique token
  (`candidate_matches`'s direct arm trusts the suffix for an unrooted first
  segment the file does not shadow).

Every Class-B resolution lands on a uniquely-named sanctioned token's single
genuine def (there is no same-named decoy to forge). The compiler-enforced
sealed-token boundary remains the production guarantee for all of these.

## Disposition

Do NOT run further incremental resolver-hardening passes for this guard, and do
NOT extend the residual disclosure per-instance — Classes A and B above are the
complete characterization. This classifier is revisited only if:

- it is promoted from residual guard to PRIMARY enforcement (then it must become
  sound-by-construction — e.g. a closed conservative-fire mechanism that
  over-flags rather than under-flags, with an audited allowlist of resolvable
  shapes); OR
- a real production sink reference starts using one of these residual shapes at a
  publication-sink boundary — in which case the correct fix is to rewrite that
  reference to a rooted/proven form, not to extend the classifier.
