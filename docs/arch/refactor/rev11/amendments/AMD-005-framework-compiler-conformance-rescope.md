# AMD-005 — Framework compiler conformance rescope

**Status:** PROPOSED — NOT RATIFIED. This candidate has no execution authority.
**Prepared against:** local `program/architecture-lock` commit
`b3249d13d07806a14a4307954dfcc459cf7301ac`, tree
`57e412549c24c903877b471000569c99591a49fc`.
**Amends on ratification:** [`../program.md`](../program.md),
[`../program-dag.toml`](../program-dag.toml), the live program ledger, the B2–B6 and
C3–C4 charters, and the capability and performance locks named below.

The published consolidated master and release artifacts remain immutable historical
inputs. This amendment becomes operative only after the exact candidate tree passes
three independent challenges and the maintainer performs the action in
[§15](#15-exact-ratification-action). B1 is already accepted; until ratification no
successor newly introduced or delayed here is dispatchable.

## 1. Maintainer direction and product boundary

Vue and Svelte compiler correctness are mandatory Revision 11 scope. Verter is an SFC
compiler. It owns SFC parsing; framework semantics; framework-owned models and product
plans; generated client and server JavaScript; established public API, TSC,
declaration, diagnostic, CSS, and mapping products; and correct generated-code
topology. It does not implement, fork, bundle, replace, or ship either framework
runtime.

`RuntimeClient` and `RuntimeServer` name JavaScript products intended to execute on
the official framework runtime. They never name a Verter-owned runtime. Official
compilers are test oracles only; official runtimes are hermetic test executors only.

B1 was accepted at commit `03b2fdbfc6d12452824768d9e389a5f6f3d680df`, tree
`7f8230066735db17650b5d594a95d597540b3729`, under
[`../charters/B1.md`](../charters/B1.md), unchanged. Its former implementation
worktree and branch have been removed. No framework semantics, compiler options,
conformance behavior, or fail-closed guard is added to or attributed to B1 by this
package.

## 2. Exact compatibility domains

The only Vue runtime-output oracle domain is `vuejs/core v3.6.0-rc.3`, immutable
commit `3adb225775c9b28223a56e07f7a2f874b6fbb138`, tree
`36da8dc8841a35d3e1163e4b9bb5752f95ca527a`. Vue VDOM and Vue Vapor are separate
capability families. Because the upstream domain is a release candidate, neither
family may be classified Stable.

The only Svelte runtime-output oracle domain is `sveltejs/svelte svelte@5.56.8`, tag
object `a49603bbb50f948fd0c2bf5c55582a8f89b4d91c`, immutable commit
`44a7813730579b94004e182e5a67aab27aa9d2a6`, tree
`63390158bfe8f997c474e35215a4fa627194c229`.

Exact package identities, integrities, and complete resolved harness closures are in
[`../evidence/framework-conformance/version-domain.md`](../evidence/framework-conformance/version-domain.md)
and its package locks. No dist-tag, range, branch, or automatic upgrade is allowed.
A later Vue RC, Vue stable, or Svelte release is a new compatibility-domain proposal
requiring ratification and regenerated conformance evidence.

## 3. Architecture

Each framework owns its own semantic architecture:

```text
SFC source
  -> framework parser
  -> framework semantic model
  -> requested product-specific plan
  -> structured emitter/edit plan
  -> atomic compiler artifact set
```

Narrow staged IRs within one framework are preferred when they provide explicit
ownership, skip, reuse, mapping, or lifetime value. The following are forbidden:

- a universal Vue/Svelte AST, template IR, runtime IR, fact bag, or options bag;
- a tagged Vue-or-Svelte semantic hierarchy;
- Vue as the implementation base for Svelte;
- reconstructing one product by reparsing another generated product; and
- production fallback to an official compiler or dependency on an official compiler
  or runtime.

The exact boundary and product vocabulary are normative in
[`../contracts/framework-compiler-boundary.md`](../contracts/framework-compiler-boundary.md).

## 4. Amended DAG

The machine-readable authority is [`../program-dag.toml`](../program-dag.toml). The
amended region has this acceptance shape:

```text
B1 -> BF1 -> BF2 -> BF3 -> {B2, B3}
B2 -> C1
{B2, B3} -> B4
B4 -> {BV1, BS1}
{BV1, BS1} -> B5 -> B6
{C1, B3, B5} -> C2 -> C3
{B6, C3} -> C4
```

All unaffected predecessor edges remain. In particular C1 retains A6 and B1, and
downstream D–L edges are unchanged. B2 and B3 may overlap only after an exact writable
ownership proof establishes disjoint code, fixtures, manifests, generated artifacts,
and shared lockfiles. BV1 and BS1 may overlap only after the same proof additionally
assigns disjoint heavy-machine leases. Absence of that proof means serialization.

B2 and B3 remain undispatchable until this amendment is ratified and BF1, BF2, and
BF3 are individually accepted. B5 waits for both framework trains. C4 waits for B6
because prepared and batch routes do not exist before B6.

## 5. New block allocation

The executable charters are:

- [`../charters/BF1.md`](../charters/BF1.md): lock domains, options, capability truth,
  acceptance IDs, cases, conformance contracts, performance cells, and program
  amendments; no broad compiler fix.
- [`../charters/BF2.md`](../charters/BF2.md): test-only official-core harness,
  generated-code validation/execution, goldens, manifests, coverage, and mutation
  tests; no production compiler behavior or runtime.
- [`../charters/BF3.md`](../charters/BF3.md): retract reachable success cells proven
  wrong before publication; no broad backend correction.
- [`../charters/BV1.md`](../charters/BV1.md): Vue semantic models, plans, VDOM,
  Vapor, SSR, diagnostics, maps, TypeScript-visible products, and the accepted Vue
  pack; no Vue runtime.
- [`../charters/BS1.md`](../charters/BS1.md): Svelte-native client/server semantics,
  topology, diagnostics, maps, TypeScript-visible products, and the accepted Svelte
  pack; no Svelte runtime and no residual Vue assumption.

Existing responsibilities are amended by [`../charters/B2.md`](../charters/B2.md),
[`B3.md`](../charters/B3.md), [`B4.md`](../charters/B4.md), [`B5.md`](../charters/B5.md),
[`B6.md`](../charters/B6.md), [`C3.md`](../charters/C3.md), and
[`C4.md`](../charters/C4.md). K2 still owns transport carriers and their conversion
into B3's request; it may not reinterpret framework semantics.

## 6. Canonical request and option policy

B3 owns one typed request containing framework, exact compatibility domain, requested
products, client/server profile, development/production, Vue VDOM/Vapor, Svelte
runes/legacy where applicable, normalized framework options, map requests, capability
lookup, early unsupported-combination rejection, and minimal prerequisite planning.
There is no second semantic authority in a universal options bag.

Every semantics-affecting official option is classified exactly once as `supported
canonical`, `derived`, `host-resolved`, `test-only`, `external`, `unsupported
fail-closed`, or `not applicable` in the Vue and Svelte inventories. Unknown
semantics-affecting options fail request construction. No public option is silently
ignored.

## 7. Capability and maturity policy

[`../evidence/framework-conformance/capability-matrix.tsv`](../evidence/framework-conformance/capability-matrix.tsv)
is the proposed exact capability lock. A row separately records route reachability,
present disposition, target maturity, compatibility domain, acceptance owner, and
unsupported or projection-required behavior. `Experimental`, `Preview`, `Supported`,
and `Stable` are product maturity terms; they do not replace conformance evidence.

An enabled supported cell has no unresolved blocked official cases and no semantic
known-divergence allowlist. `SSR x Vapor` is not assumed to be a Cartesian compiler
mode: RC.3 uses the SSR compiler for server output and Vapor-specific metadata where
officially applicable. Any request for a nonexistent combined backend fails closed.

## 8. Oracle, exclusion, and golden rules

The binding contracts are:

- [`../contracts/official-core-oracles.md`](../contracts/official-core-oracles.md);
- [`../contracts/language-tools-exclusion.md`](../contracts/language-tools-exclusion.md)
  and [`../contracts/third-party-exclusion.md`](../contracts/third-party-exclusion.md);
- [`../contracts/conformance-goldens.md`](../contracts/conformance-goldens.md); and
- [`../contracts/conformance-normalizer.md`](../contracts/conformance-normalizer.md).

`vuejs/language-tools`, `sveltejs/language-tools`, Vize, rsvelte, PrimeVue,
`pikax/vue-benchmarks`, `pikax/svelte-benchmarks`, and every other third-party app,
library, compiler, or fixture repository are forbidden as oracle, corpus, expected
output, baseline, or acceptance source. A difference from language-tools is not by
itself a defect.

Expected goldens are generated only from the exact official compiler pins. Candidate
Verter output cannot update its own expectations. Cosmetic normalization is limited
to whitespace/layout, harmless parentheses, quote spelling, and scope-aware alpha
renaming of private generated identifiers. It cannot erase helper family/source,
declarations, meaningful order, DOM/effect/block/event/component/slot/hydration/SSR
topology, prop-versus-attribute meaning, diagnostics, mappings, literals, or authored
and public names.

## 9. Conformance acceptance

For each applicable successful case, acceptance proves: requested products only;
atomic publication; fragment validity; assembled parse validity; real-package link
validity; normalized structure; helper/import/call topology; deterministic official
runtime execution; server behavior; hydration behavior; diagnostics; mappings;
TypeScript-visible behavior; route equivalence; zero unrequested work; and locked
performance gates.

The acceptance identifiers are:

| ID | proof |
|---|---|
| FC-DOMAIN-001 | immutable official source and package closure |
| FC-BOUNDARY-001 | compiler/runtime and framework ownership boundary |
| FC-OPTIONS-001 | complete, single-classification option inventories |
| FC-CAPABILITY-001 | reachable/default route and maturity truth |
| FC-HARNESS-001 | hermetic official invocation, validation, execution, and mutations |
| FC-MANIFEST-001 | every official case has one allowed disposition |
| FC-ATOMIC-001 | no partial artifact publication on success or refusal |
| FC-NORMALIZER-001 | cosmetic-only normalizer with negative/mutation discrimination |
| FC-VUE-001 | complete accepted Vue VDOM/Vapor/SSR pack |
| FC-SVELTE-001 | complete accepted Svelte client/server runes/legacy pack |
| FC-HYDRATION-001 | official/official, Verter/Verter, and meaningful cross-pair proof |
| FC-TS-001 | exact Revision 11 TypeScript-domain observable equivalence |
| FC-ROUTES-001 | direct, prepared, batch, staged, and later public route equivalence |
| FC-ZERO-WORK-001 | unrequested stages and products perform zero work |
| FC-PERF-001 | pre-candidate performance cells pass conjunctively |
| FC-GOV-001 | exact-tree challenges and maintainer ratification |

Official-case seed manifests are at
[`../evidence/framework-conformance/vue-official-cases.tsv`](../evidence/framework-conformance/vue-official-cases.tsv)
and [`svelte-official-cases.tsv`](../evidence/framework-conformance/svelte-official-cases.tsv).
They are declarations to be completed by BF2, not acceptance evidence. `blocked` rows
must be resolved before any containing supported cell succeeds.

## 10. Fragments, maps, server output, and hydration

B4 owns logical source units and identities, placement, fragment/assembly map
composition, and atomic artifact publication. It does not select `CodeTransform` or
any other current emitter in advance. Every current owner receives one evidence-backed
`Preserve`, `Converge`, `Replace`, `Delete`, or `Defer` disposition in
[`../evidence/framework-conformance/emitter-mapping-dispositions.tsv`](../evidence/framework-conformance/emitter-mapping-dispositions.tsv).

Fragment assembly follows
[`../contracts/fragment-assembly.md`](../contracts/fragment-assembly.md). Server and
hydration behavior follows [`../contracts/ssr-hydration.md`](../contracts/ssr-hydration.md).
The harness cannot patch generated output, inject helpers, mock nonexistent exports,
or replace official runtimes with simplified ones.

## 11. TypeScript-observable products

TSC, TSX, public API, and declaration conformance use the exact TypeScript domain
already governing the relevant Revision 11 operation, the TypeScript compiler/API's
observable behavior, ratified Verter contracts, and independently authored local
fixtures. The repository currently contains distinct exact TypeScript domains; BF1
must preserve their ownership rather than inventing one floating version. The full
contract is [`../contracts/typescript-product-conformance.md`](../contracts/typescript-product-conformance.md).

BV1 exposes closed typed demands for project-aware imported Vue macro information.
C3 supplies those demands and cannot replace Vue code generation.

## 12. BF3 safety retraction

BF3 probes every reachable cell currently reporting success. On a minimum parse,
link, or conformance failure it records request/route/profile/failure, detects the
request through existing typed information before publication, returns typed
non-success, publishes no partial product, adds a discriminating local regression,
names the later correction owner, and records guard removal as part of that owner's
acceptance. If a broken subset cannot be distinguished safely, BF3 retracts the
entire cell. Its bounded initial inspection set is
[`../evidence/framework-conformance/bf3-safety-retraction-scope.md`](../evidence/framework-conformance/bf3-safety-retraction-scope.md).

## 13. Performance lock

The candidate does not choose thresholds after observing candidate code. BF1 must
add and independently review the exact cells, corpora, runners, repetitions,
absolute/relative limits, memory limits, work counters, and heavy-machine lease
policy specified in
[`../evidence/framework-conformance/performance-impact.md`](../evidence/framework-conformance/performance-impact.md)
before BF2 begins. Existing required cells remain required and are not reweighted.

## 14. Program-state transition

The candidate DAG and both tracked state shapes contain 56 identical block IDs. New
rows are `LOCKED`, reviews and maintainer decisions are `PENDING`, and all identity
and evidence fields are empty. No existing accepted row is rewritten. The live B1
row remains the last integrated program fact and records its accepted commit/tree;
there is no separate B1 worktree. Detailed transition rules are in
[`../evidence/framework-conformance/program-state-transition.md`](../evidence/framework-conformance/program-state-transition.md).

On ratification the current amendment candidate lands on the already accepted B1
line, after which BF1 may be exposed as `READY`. No transition exposes B2 or B3
before BF3 acceptance.

## 15. Exact ratification action

After the package is committed, three independent agents must author the architecture,
conformance, and governance reports at the paths reserved in
[`../evidence/framework-conformance/reviews/README.md`](../evidence/framework-conformance/reviews/README.md).
Each report must bind the same full candidate commit, repository tree, amendment
digest, DAG digest, both oracle-lock and exact-closure digests, and all
generated-manifest digests.

The validator has two non-interchangeable phases. The immutable package candidate is
checked with `validate-package.mjs --pre-review`, which requires the three primary
report paths to be absent. After independent review, a ratification-bundle candidate
attaches those reports and is checked in `--post-review` mode with exact
`--reviewed-commit <full-sha>` and `--reviewed-tree <tree-oid>` arguments. That mode
requires every primary report to name that exact reviewed package object and a closed verdict;
it does not convert `BLOCKING` to `PASS` or let an attachment claim to have reviewed
the bundle bytes that contain itself. The ratification bundle may differ from its
reviewed package candidate only at the three primary report paths. Any other changed
byte requires a new reviewed package identity and fresh independent reports.

If and only if all three closed verdicts are `PASS`, the designated maintainer must
then make one explicit recorded decision with this exact semantic action:

> Ratify AMD-005 for reviewed package commit `<reviewed-full-sha>`, tree
> `<reviewed-tree-oid>`, attached without non-report changes in ratification-bundle
> commit `<bundle-full-sha>`, tree `<bundle-tree-oid>`, and the listed package digests;
> accept the exact Vue RC.3 and Svelte 5.56.8 domains, exclusions, amended DAG,
> charters, capability lock, and pre-candidate performance-lock process; authorize
> landing that byte-exact ratification bundle on `program/architecture-lock`, whose B1
> predecessor is accepted at `03b2fdbfc6d12452824768d9e389a5f6f3d680df`;
> authorize BF1 exposure to `READY` after ratification; and authorize no B2/B3 dispatch
> until BF1, BF2, and BF3 are accepted.

Any changed reviewed-package byte requires regenerated identities and fresh reports;
the only post-review exception is the declared attachment of those exact reports.
Silence, merge, or this proposal's commit is not ratification. The preparer cannot
ratify, review, or satisfy any independent mandate.

### 15.1 Recorded ratification

The architecture, conformance, and governance reports at
[`../evidence/framework-conformance/reviews/`](../evidence/framework-conformance/reviews/)
each closed `BLOCKING_FINDINGS` against reviewed candidate
`ce1d0e4688af1b5bd548b6b68286632cc0f7ede8`, tree
`1ff1f83d8e994b6f1169b0b209c9f557c23f4728`. A bounded fix round resolved every named
finding at commit `7442bb9060b7faa0720e528d3f96ee1df1abff95`, tree
`69502487b55f87eb7c0c009876865b64397da660`, independently confirmed by
`architecture-challenge-reattestation2.md` (PASS), `conformance-challenge-reattestation2.md`
(PASS), and `governance-challenge-reattestation2.md` plus
`governance-challenge-reattestation3.md` (the second governance finding required one
further narrow correction to an untracked, gitignored scratch note that was never part
of the reviewed package identity; PASS). All three closed verdicts are `PASS`.

> Ratify AMD-005 for reviewed package commit
> `7442bb9060b7faa0720e528d3f96ee1df1abff95`, tree
> `69502487b55f87eb7c0c009876865b64397da660`, attached without non-report changes in
> ratification-bundle commit `aa757eecc1f7748d2eec076ab0665da76cb2904a`, tree
> `0c078357bac74724208df75c25da4fa74ab95013`, and the listed package digests; accept
> the exact Vue RC.3 and Svelte 5.56.8 domains, exclusions, amended DAG, charters,
> capability lock, and pre-candidate performance-lock process; authorize landing that
> byte-exact ratification bundle on `program/architecture-lock`, whose B1 predecessor
> is accepted at `03b2fdbfc6d12452824768d9e389a5f6f3d680df`; authorize BF1 exposure to
> `READY` after ratification; and authorize no B2/B3 dispatch until BF1, BF2, and BF3
> are accepted.

The ratification bundle `aa757eecc1f7748d2eec076ab0665da76cb2904a` landed on
`program/architecture-lock` by fast-forward from `b3249d13d07806a14a4307954dfcc459cf7301ac`.
This record was added in a separate follow-up commit and is not itself part of the
ratified bundle's byte-exact tree.

## 16. Supersession and non-goals

On ratification this amendment supersedes only the conflicting B2–B6/C3–C4 scope and
edges in the original split program and immutable historical copies. It retains all
unaffected Revision 11 constraints and AMD-001 through AMD-004. It neither accepts a
compiler algorithm nor adds a production dependency, runtime, broad fix, public
product, or compatibility claim.
