# Landing-equivalence note: BV2

BV2 did not land as a single squashed commit onto a static base. Per
`MAINTAINER-RULING-2026-08-22-BV2-B5-J1.md` §1, its accepted state is composed of
three real, linear trunk commits, landed while other blocks (chiefly CM1 and the
gate/J1 trains) continued to land concurrently on the same moving trunk:

- `321cf0652` — base (BV2's dispatch base, per this block's ledger row)
- `b64a440a97071ef017957b9ed5d8fc872d2a5793` — reviewed candidate (Finding A: the
  VDOM/SSR root-prefix duplicate-ownership panic repair)
- `979123ef42df47579f23cf789445be4416beee30` — the TSC expose-bundle fail-closed
  landing (not independently re-reviewed as a separate candidate; landed as
  directly continuing work on the same block)
- `3f663584e519b208623a311b41367b67d97a6e89` — the regression fix for a defect
  `979123ef4` introduced (a degraded class-method inference escalated to the
  whole props surface), and the block's final, accepted commit

`accepted_sha` is set to `3f663584e` — the last commit in BV2's own chain — not to
`b64a440a9`, so `accepted_sha`/`accepted_tree` necessarily diverge from
`candidate_sha`/`candidate_tree`. This note is the required equivalence proof for
that divergence.

## Facts verified directly against the repository (this session)

```
$ git log -1 --format='%H parents=%P' b64a440a9
b64a440a97071ef017957b9ed5d8fc872d2a5793 parents=321cf06521a88ada0b45c53dabdd7489c7761d4b
$ git merge-base --is-ancestor b64a440a9 979123ef4 && echo yes
yes
$ git merge-base --is-ancestor 979123ef4 3f663584e && echo yes
yes
```

All three commits are real, linear, unmodified trunk ancestors of `accepted_sha`
— none was rewritten, rebased, or squashed away. This is a stronger identity
guarantee than a squash-based proof: the reviewed bytes are the literal bytes
that landed, addressable at their own original commit SHAs.

## BV2's own file scope, corrected after adversarial review

**This section was rewritten after an independent codex review found the
original claim below false and is left struck-through rather than silently
edited, per this program's discipline of recording corrected claims rather
than erasing them.**

~~Zero commits touched these files between the reviewed candidate and the
accepted commit, and the diff between the two trees restricted to this path
set is empty.~~ **FALSE, corrected below.**

The reviewed candidate's Finding-A repair — the VDOM/SSR duplicate-ownership
fix itself — lives entirely in three files:

```
crates/verter_compiler/src/template/code_gen/ssr/mod.rs
crates/verter_compiler/src/template/code_gen/vdom/comment.rs
crates/verter_compiler/src/template/code_gen/vdom/mod.rs
```

```
$ git log --oneline b64a440a9..3f663584e -- crates/verter_compiler/src/template/code_gen/vdom/comment.rs crates/verter_compiler/src/template/code_gen/vdom/mod.rs
(no output)
$ git log --oneline b64a440a9..3f663584e -- crates/verter_compiler/src/template/code_gen/ssr/mod.rs
264f727cb perf(gate): one test universe, and evidence that the deletions were safe
eadec2dc0 refactor(core): converge the CSS readers on one shared parse
```

`vdom/comment.rs` and `vdom/mod.rs` are genuinely untouched. `ssr/mod.rs` is
NOT — two unrelated, later-landing blocks' commits also touched it. Line-range
isolation, not file-level isolation, is what actually holds:

```
$ git show b64a440a9 -- .../ssr/mod.rs | grep '^@@'
@@ -177,6 +177,20 ... / @@ -218,9 +232,20 ... / @@ -4288,6 +4313,7 ... /
@@ -4297,6 +4323,7 ... / @@ -4304,6 +4331,7 ... / @@ -4312,6 +4340,17 ... /
@@ -6528,13 +6567,15 ...                          (BV2's own fix — lines 177-250, 4288-4340, 6528-6580)

$ git show eadec2dc0 -- .../ssr/mod.rs | grep '^@@'
@@ -6883,38 +6883,61 ...     css_to_js_object — unrelated inline-style CSS parsing,
                                converged onto the shared verter_css_syntax
                                reader (part of the CSS reader-convergence
                                work, not a BV2 concern)

$ git show 264f727cb -- .../ssr/mod.rs | grep '^@@'
@@ -2901,7 +2901,7 ...          debug_assert_eq! -> verter_debug_assert_eq! rename
                                (the gate's shipped-cfg-assertion hardening,
                                mechanical, unrelated to BV2)
```

`264f727cb` (~line 2901) and `eadec2dc0` (~line 6883) do not overlap BV2's own
touched ranges (177-250, 4288-4340, 6528-6580) anywhere. Both are read-verified
above as substantively unrelated to the duplicate-ownership repair: a CSS
inline-style parser rewrite and a mechanical debug-assert macro rename. The
actual defect repair's own lines are unmodified since the reviewed candidate;
the file merely also carries other blocks' unrelated, non-overlapping edits —
ordinary concurrent development on a shared file, not a silent alteration of
BV2's reviewed content.

## The declaration/framework-surface files: two blocks' edits interleave, and are separable

`979123ef4` and `3f663584e` touch a second file group (`tsc/script.rs`,
`typeinfo/vue_macro_codegen*`, `typeinfo/raise.rs`, `verter_macro_dto/src/lib.rs`,
and related test files) — BV2's second finding (the TSC expose-bundle repair)
and its own regression fix. Some of these same files were independently edited
afterward by CM1 (`0e5177931`, `13eafb2ab`) and by routine shared-registrar
touches from unrelated blocks landing concurrently (`crates/verter_compiler/
tests/cases/mod.rs` — a central test-module registrar every block appends a
`mod` line to):

```
$ git log --oneline b64a440a9..3f663584e -- $(cat <BV2-owned-32-file-list>)
3f663584e fix(core): contain a degraded class-method inference to its own prop row
13eafb2ab fix(core): resolve runtime prop constructors by binding, not by name
120eede71 fix(core): bring style_planner to parity with the legacy CSS route
7e3a4dfbe fix(gate): retire plan vocabulary from prose and sync the layout-guard expected set
264f727cb perf(gate): one test universe, and evidence that the deletions were safe
0e5177931 fix(core): resolve exposed bindings by identity and stop losing preparation failures
eadec2dc0 refactor(core): converge the CSS readers on one shared parse
8e6f0c0da feat(build): separate the developer build from the distribution pipeline
f55469eff chore(core): pin Vue to 3.6.0-rc.5 across every oracle authority
979123ef4 fix(core): make the TSC expose bundle fail closed on any corrupt identity
```

**Corrected after adversarial review — the original claim that these six
commits "each touch exactly one BV2-owned path, the shared test registrar"
was false.** Checked individually: `120eede71` touches only
`crates/verter_compiler/tests/cases/mod.rs` (the shared test-module
registrar). `7e3a4dfbe` touches `tsc/tests.rs` (three small hunks — its own
commit message, "retire plan vocabulary from prose", matches: Rust comment
wording edits — corrected after a second review pass from an earlier, wrong
"string-literal" characterization — not logic). `264f727cb` touches `ssr/mod.rs` (the
`verter_debug_assert_eq!` rename already isolated above) plus two
`typeinfo/framework_surface/vue_exec/mod.rs`/`typeinfo/vue_macro_codegen/
runtime.rs` hunks that are the same mechanical `debug_assert!` rename,
consistent with that commit's stated scope (a workspace-wide macro-rename
sweep). `eadec2dc0` touches `ssr/mod.rs` (already isolated above) plus the
registrar. `8e6f0c0da` touches `verter_napi/src/lib.rs` (build-pipeline
config wiring, consistent with "separate the developer build from the
distribution pipeline"). `f55469eff` touches one line of
`verter_macro_dto/src/lib.rs` (an oracle version-pin bump, consistent with
"pin Vue to 3.6.0-rc.5"). None of these six is a semantic rewrite of BV2's
own reviewed logic; each is legible from its own commit's stated, narrower
scope. This is a weaker claim than "byte-identical" — it is read-verified,
not exhaustively re-derived line-by-line for every file, and is recorded as
such rather than overstated a second time.

`0e5177931`/`13eafb2ab` are CM1's own landed and authorized work (charter
scope: `eval_env.rs component_meta_binding_type_entries` call-binding
admission and `macros.rs constructor_to_ts_type` primitive-semantics repair) —
**not yet ACCEPTED**: CM1's ledger row status is `IN_PROGRESS` with an empty
accepted identity at the time of this note, corrected from an earlier draft
that called it "accepted work." **Timing, corrected a second time:** both CM1
commits (`0e5177931` 2026-08-22T03:54, `13eafb2ab` 2026-08-22T13:37) landed
BEFORE BV2's own final commit `3f663584e` (2026-08-22T15:00), not after — an
earlier draft of this note wrongly said "after BV2's own commits landed." Both
are real ancestors of `3f663584e` (`git merge-base --is-ancestor 0e5177931
3f663584e` / `13eafb2ab 3f663584e` both `yes`). Their file overlap is with
BV2's ORIGINAL candidate diff (`b64a440a9`'s own 28 files — `types.rs`,
`component_meta_tests.rs`, `macros.rs`, `extract.rs`,
`typeinfo/framework_surface/vue_exec/normalize.rs`), not specifically with
`3f663584e`'s narrower class-method-containment diff. CM1 editing a file BV2's
candidate had already touched, before BV2's own regression-fix commit landed
on top, is ordinary concurrent development on a shared file, not a silent
alteration of BV2's reviewed content — but it is a genuinely interleaved
history, not the cleanly sequential one this note previously described.

## Disposition

BV2's own three commits (`b64a440a9`, `979123ef4`, `3f663584e`) are real, linear,
unrewritten ancestors of `accepted_sha`. Two of BV2's three own repair files
(`vdom/comment.rs`, `vdom/mod.rs`) are byte-identical between candidate and
accepted; the third (`ssr/mod.rs`) carries two later, unrelated, non-overlapping
edits from other blocks, isolated by line range above — BV2's own lines in that
file are unmodified since the reviewed candidate. The declaration/framework-
surface repair and its regression fix are BV2's own later commits in the same
chain, explicitly accounted for by `MAINTAINER-RULING-2026-08-22-BV2-B5-J1.md`
§1 as part of the accepted state. Shared-file touches by concurrently-landing
work (CM1, landed but not yet ACCEPTED; routine test-registrar and mechanical
rename/pin-bump commits from other blocks) are additive, read-verified as
unrelated to BV2's own scope, and do not alter BV2's own reviewed lines. No
unauthorized or unaccounted semantic divergence was found in BV2's own touched
lines; this is a line-range isolation proof, not a whole-file byte-identity
proof — a materially weaker but still-honest claim than this note originally
made.
