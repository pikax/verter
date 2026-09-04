# TypeScript mapper rescope: the dual-plane owner replaces the closure package

Date: 2026-09-02

## Decision

The TypeScript mapper vertical's owner is the **ratified dual-plane
mapper/snapshot/oracle identity contract** at
[`contracts/typescript-mapper-dual-plane.md`](../contracts/typescript-mapper-dual-plane.md).
The previous owner — the closure package and the string mapper plane — is
displaced.

### Ratification

The rescope is **RATIFIED**. The decision was taken by the architect reviewing
the rescope on its merits, not delegated to a box somebody else ticks, and this
record is the ratification — there is no second artifact to wait for. It is
written here rather than in a review thread because a decision that exists only
in a conversation cannot be read back by the block that depends on it.

Ratification was decided against the rescope's own stated requirements, each
checked against the artifacts rather than against their description:

- Status is derived, not authored. The register schema declares no `status`
  property and sets `additionalProperties: false` on every table, so an
  author-set status is a parse error rather than a convention someone can
  break. The validator is the only producer of a status value.
- The lifecycle vocabulary has no self-granting terminal. `OPEN`, `REFUSED`,
  `PROVEN-BOUNDED`, and `PROVEN` are the derived values; `ADMISSIBLE` is
  absent by construction, and the successful pre-review state is
  `READY_FOR_REVIEW`.
- The remainder set is closed at three, each with a non-circular owner that is
  a strict DAG descendant, a receiving criterion its owner's charter actually
  declares, and a named resolution gate. A fourth remainder is not expressible.
- The finding universe reconciles exactly: thirty-two must-close entries and
  four remainder entries, held as closed sets the register cannot widen or
  quietly shrink.
- The controls are portable by construction. The adapter runner vocabulary is
  the closed pair `node` and `cargo` and a record supplies only an argument
  tail, so a shell, batch, or interpreter control is not expressible — the
  displaced package's tracked POSIX controls are rejected structurally rather
  than removed by inspection and re-added later.
- The instrument is not a name-keyed scanner. Every check reads declared
  structure — the register, its schema, the DAG, the charters, this decision,
  and the contract's own headings. Where it reads a crate tree it re-derives a
  count the record already declared, which is the opposite of searching the
  source for a spelling.
- The instrument is executed, not merely present. `--check` and the control
  suite run in the roadmap CI job, and that job's path filter is eligible on
  the crate trees the validator actually opens.

A rescope whose thesis is that an artifact must not supply its own verdict
would fail that thesis if its ratification were self-asserted with no stated
grounds. The grounds are therefore listed above, and each is a property of the
committed bytes that a reader can re-check without trusting this paragraph.

Nothing about this decision is derived by tooling. The register at
`closure/typescript-mapper/register.toml` points at this file, and the
validator resolves it as an existing authority artifact — that is a presence
check on a ratified decision, not a re-derivation of it.

What this record cannot do is evidence itself. Every ground above is a
property of committed bytes a reader can re-check, but no artifact inside the
tree can distinguish a ratification a maintainer took from a document
asserting that one was taken. That is why the rescope requirement is
discharged by the maintainer accepting this record and not by any check over
it, and why an unaccepted record is an open requirement rather than a
satisfied one.

## Why the closure package was rejected rather than repaired

The package certified itself. Its input carried an author-written status, so
the artifact that was supposed to be judged also supplied the verdict. Every
other defect follows from that one: prose stood in for commands, probes that
selected nothing read as passes, remainders were bound to a train instead of
to a criterion somebody has to satisfy, and a bounded result could be
re-recorded as proven with no transfer.

Repairing the text would have left the shape intact. The replacement inverts
it: the register is input only, and status is derived by a validator the
register cannot influence.

## What the replacement is

Three artifacts, none of which can certify itself.

- **The ratified contract** states the architecture: two planes with a one-way
  dependency direction, a closed five-class projection partition with two
  fail-closed classes, three non-aliasing observation identities, a certified
  binding that is a witness rather than a handle, and a lifecycle in which
  degraded outcomes never warm.
- **The input register** carries claims, atoms, proof records, negative
  controls, findings, remainders, and the concrete deletion and survivor rows.
  Its schema declares no status property and forbids additional properties on
  every table, so an author-set status is a parse error rather than a
  convention someone can break.
- **The validator** derives `OPEN`, `REFUSED`, `PROVEN-BOUNDED`, and `PROVEN`,
  generates the human view, and holds the closed claim, atom, finding,
  remainder, and deletion/survivor-row universes. It reads declared structure
  only — the register, its schema, the DAG, the charters, and the contract's own
  section text. It never searches the source tree for a spelling, so it is not a
  name-keyed scanner.

An obligation this rescope only *states* is not admissible as prose, and a bare
acceptance identifier is not a citation. Every charter in this program declares
the same four boilerplate slots, so `<any descendant>-AC<1..4>` resolves against
hundreds of unrelated blocks; identifier existence therefore proves nothing on
its own. A citation is admissible only when three independent predicates hold
together: the owner is this node or a strict descendant, the owner is inside
this node's train, and the ROLE the citation declares matches the role the
owner's charter attaches to that ordinal. A contract atom must additionally
quote the sentence the contract states, and the quotation must still be in the
bound section — a heading that survives while its body is replaced by a
placeholder does not keep an atom proven.

The remainder that most needed this is the implementation baseline. Its four
gates originally sat on the bounded-work slot, whose own charter text ends
"otherwise record a terse not-applicable rationale" — a remainder whose entire
content is "a pre-change comparison must exist" was therefore bound to a
criterion an implementer may close by writing that it does not apply. It is
rebound to the positive-contract slot, which requires exact identities and
provenance to be preserved across the change and so cannot be satisfied without
producing the baseline first.

## Acyclicity, and what the instrument may not prove about itself

Each claim declares the artifacts it is about. A proof record's verdict
producers are derived from its adapter and command tail, and a record whose own
run produces the verdict for one of a claim's subjects may not cover that
claim's atoms — the cycle refuses the claim outright. Nothing here is
volunteered by a proof record, so a record cannot exempt itself by leaving a
field out; the earlier design made the subject an optional declaration, which
meant an unannotated proof could never fail the rule at all.

The consequence is deliberate and load-bearing: the instrument's claim names the
validator and its schema as subjects, so the validator's own successful run is
structurally barred from proving that the validator is correct. What proves it
is the adversarial control suite, which is a different artifact and whose cases
pass only when the validator *refuses* a planted mutation. A permissive or
broken validator fails that suite rather than passing it, which is the inverse
of the self-certification this rescope displaced.

The suite does of course *import* and execute the validator — a validator's
controls cannot run anything else — and that is the half the producer derivation
cannot see. A record's verdict producers are the artifacts its transcript is a
statement ABOUT: the tool a `node <tool>` record invokes prints that transcript
itself, and the packages a cargo selector names are what its summary counts.
`node --test <file>` transcribes the harness's verdict on that file's
assertions, so the artifact reported on is the test, and a module it exercises is
an input to those assertions. True — and on its own it left the exemption
implicit, which is exactly the shape this instrument exists to refuse. A record
whose suite reaches its claim's subject through its own imports is running that
subject's behaviour, and whether that is acceptable is a judgement, not an
absence. An import is treated as execution here rather than proven to be one:
that over-reads in the safe direction, widening what counts as executed, where
demanding proof of a call would narrow it.

So the import edge is derived too, and refused by default. A node record's entry
file is read, its first-party imports are resolved *transitively* with a visited
set, and a record whose entry file reaches one of its claim's subjects through
that graph is a cycle unless an exemption is pinned beside the validator,
published in the generated view next to that record, and still true — a pinned
record that stops exercising its subject fails, because a carve-out nobody needs
is a carve-out nobody reviews. Exactly one is pinned.

The depth is the whole content of that derivation, not an implementation note.
Reading one level treats a re-export as a boundary: `suite -> shim -> subject`
reports the shim and not the subject, derives no cycle, and requires no
exemption, while the record's run executes the subject exactly as before. Every
other over-reading here fails closed — a specifier is read wherever it appears,
and an import is treated as execution — but stopping at one level NARROWS what
counts as executed, which is the unsound direction, so the walk continues
through every first-party module it reaches.

The reason on that pin is stated for the cases that carry it rather than as a
universal about the file. Most of the suite's cases plant a mutation and pass
only when the validator refuses it, so a permissive validator fails them rather
than passing — that is what makes the exemption sound. Some cases assert over
the suite's own declarations instead: that the registered failure classes equal
the declared set, that the pinned universes are not derived from the register.
Those are real checks and they are not adversarial mutations, so claiming every
case has that property would be a published claim slightly stronger than the
thing it describes — the defect this instrument exists to refuse, in miniature.
A run of the validator
certifying itself stays barred, because there the transcript IS the subject's own
output.

## What the evidence records do and do not establish

A record used to state its counters *beside* a free-text summary, so any
plausible sentence with self-consistent numbers next to it was accepted as
evidence. The counters are now read *out of* the transcript the record claims to
have observed. Each adapter declares the terminal-summary grammar its runner
emits — libtest, nextest, `node:test`, a tool's own `PASS key=value` line, or the
compile-contract runner's fixture line — and the validator re-derives selected,
executed, passed, failed, and skipped from the recorded text. The record's own
numbers must equal them exactly. A paraphrase, a transcribed *failing* run, a
summary belonging to a different runner, and a count that drifted after the run
each parse to nothing or to different numbers, and each refuses the record.

Fixtures resolve as real files, and a command's package selectors must resolve to
crates this workspace actually has, so a record cannot cite work that does not
exist. Which records are re-executed is a capability the adapter declares rather
than a runner name checked in one place: every record whose adapter declares
instrument re-execution is re-run by the control suite and compared against its
transcription, the suite asserts it ran *all* of them rather than at least two,
and a `node` runner may not declare anything else, because this instrument can
invoke it.

A `cargo`-runner record is not re-derived here, so it names the lane that does
re-run its work, and that declaration is resolved rather than believed: the named
job must exist in the workflow, must issue the named command, must be gated
on the named path filter, and its own universe must demonstrably reach the
record's packages. That last one is not implied by the first three — a job can
issue the named command over a narrower universe than the record's packages,
leaving them unrefreshed while every other check passes.

The declared command is therefore the job's **complete command line**, not a
prefix of it, and that is the load-bearing repair. A containment check accepts a
prefix, and the live Rust lane's narrowing arguments all come *after* the point a
comfortable prefix stops at: a declaration reading `cargo nextest run
--archive-file <archive>` matches a job that continues `--partition …` and
`-E <filter>`, so every flag deciding what the lane runs stays invisible to the
check while the archive alone resolves it. Rewriting that job's run line to
select one unrelated package left `--check` green. The validator now requires
the declaration to EQUAL the line the job issues, so the same rewrite fails
loudly, and the selection is derived from that verified line.

Three shapes resolve the lane's universe: the command selects the whole
workspace; it consumes a whole-workspace archive that a job in the same workflow
builds in ONE command and that this job names among its `needs`; or it is not a
cargo selector, carries no selection argument at all, and the register declares
the enumeration of the list that runner iterates. That third arm used to end at
"so it re-runs its own default universe" — a sentence about a list nothing had
read. The compile-contract lane runs `node scripts/compile-contracts.mjs`, whose
own `OWNERS` array decides which owners it covers; dropping `identity` from that
array stops the lane refreshing the identity compile-fail record while the job
still exists, still issues the same line, and is still gated on the same filter,
so every other check here passes. The register therefore declares
`node scripts/compile-contracts.mjs --list-owners`, the validator requires that
to be the lane's own command line plus exactly one listing flag — same program,
same arguments, one switch that turns the run into its own inventory, because
matching the program alone accepted any argument vector after it, and a list
printed under a selection the lane never issues is not the list the lane
iterates — executes it, and resolves the record's own `--features identity`
against what it printed. What that flag prints and what the bare run iterates
is the one binding this arm still holds by review of that script rather than by
derivation; asking the script is already better than scraping its source, and a
successor may pin both invocations to a single declared constant inside it. The archive arm needed
both halves — a job that merely mentions the archive somewhere and carries an
unrelated whole-workspace step used to resolve it, and a builder this lane does
not depend on is a different job's artifact. A record's own re-derived selected
count is deliberately not one of the three: re-deriving a count from a fixture
directory is a property of the record and of this validator, and answering a
question about the lane with it is answering about the register instead.

Every artifact the record cites — its fixtures, its control's subject, the crate
root of every package it selects, and the anchor of every atom it covers — must
be covered by that filter's own patterns, and a pattern shape this check cannot
read is reported there as it is in the instrument's own filter. Binding a Rust
record to the Rust lane is also what covers its *transitive* dependencies: a
Rust lane's filter is the crate tree, not a hand-listed subset, so a change to a
dependency of a selected package re-runs the work rather than slipping past a
narrower list.

What that binding does *not* establish is now derived and published rather than
argued in prose, in three parts. This instrument cannot invoke `cargo`, so for
those records it compares nothing against the transcription. A lane that runs
work drawn from a record's packages is still not a lane that runs that record's
selection, so the flags on that verified command line are read rather than
annotated: a lane that names or excludes packages, or narrows to one build
target, is compared against the record's own selection and refuses the binding
when it does not run the record's work. And what remains after that comparison
is narrowing WITHIN a universe — the live Rust lane consumes the whole-workspace
archive under a core filter expression and a four-way shard partition.

The third part is the one that used to be published as a name and is now
resolved. A filter expression spelled `-E "$VERTER_NEXTEST_FILTER"` tells a
reader nothing about what is excluded, and publishing the command that computes
it was only half a step: `--check` still stayed green while that predicate
narrowed to anything at all. So the assignment is resolved out of the lane's own
step — with or without an `export` keyword, and across a shell line continuation,
because neither changes where the lane's selection comes from and keying on the
keyword turned a cosmetic shell edit into a silently unresolved producer — it
must equal the producer the register declares, and the validator RUNS it. The
expression it prints is then decomposed. Packages the predicate excludes
outright are related to the record's own selection: exclude one this record
selects and the lane is a derived limit, not breadth. What is left is name-scoped
exclusion inside a package the lane still runs, and the view publishes those by
package while saying plainly that this check does not evaluate them — whether
those names reach this record's cases is a property of the test binaries rather
than of the expression. A variable no step assigns, a producer that is not the
declared one, an expression this reader cannot decompose into what it excludes,
or a command that will not run are all resolution FAILURES rather than silent
breadth.

Decomposition means every operator and every leaf, or the guarantee is the same
unread selection one level down. The operators are split in the expression
language's own precedence — `or`, then `and`, then `not` — because splitting
conjunctions first read `not package(a) or package(b)` as "neither a nor b" and
attributed an exclusion to a disjunct that never carried one. And a leaf whose
head was merely RECOGNISED used to be returned unrecorded, so a lane narrowed by
`test(^x)`, a lane selecting nothing with `none()`, and a lane moving its
package set with `deps()` each decomposed to "excluding no package, with no
test-name narrowing" — full breadth, published as a derived sentence, for a
lane that runs none of this record's work. Only `package()` names a whole
package and only `all()` is the universe; every other positive leaf is a
resolution failure, which is the same answer this reader already gives an
expression it cannot read at all. The producing script stays a cited artifact for the same reason as
before, so a change to it re-runs both the lane and the instrument that
published the sentence.

The evidence model this register implements binds a record to no repository SHA,
tree, or digest, so the remaining residual is a scope statement rather than a
defect — but a view that printed one undifferentiated "refreshed by" column
published the weaker guarantee as the stronger one, and a reader cannot audit a
distinction nobody states.

Executing two lane commands is a deliberate widening of what this instrument
does, and it is kept narrow in the same way everything else here is. Only the
allowlisted `node` runner is executable, so the vocabulary that already refuses
a shell or interpreter control refuses one here too; the command comes from the
register, which declares it, and the workflow must agree that this is the
command the job computes its selection from or the script that job runs. Nothing
is searched for — the command is handed to the check, exactly as the workflow's
job ids and filter names are.

The residual that stays after all of that is worth naming exactly, because it is
the one thing this shape cannot close. Nothing in CI ever compares a cargo
record's transcribed counts against a fresh run of that lane. A test added to one
of the three packages a targeted record selects makes its transcribed numbers
stale, the Rust lane stays green because the tests still pass, and this
instrument — which cannot invoke `cargo` — has nothing to compare. The
transcription is corrected when an author re-runs and re-transcribes it, and the
control suite proves that the numbers it *can* re-derive are current.

Closing it needs the refreshing lane to emit what it counted, per record, in a
form the instrument can read back — which is work in the Rust lane, not in this
authority tree, and belongs to the blocks that own the Rust surfaces this
register's targeted records select. Until then the scope is stated in the
generated view rather than implied away, which is the honest shape available to
an authority-only block: a reader is told exactly which records this instrument
re-derives and which it only binds to a lane.

The instrument's own lane is bound the same way, and it is triggered by the
artifacts *this validator opens* — which is not the same set as the artifacts a
record's tests exercise. Attributing a validator input only to the refreshing
lane was an attribution error with a real failure mode: `analyze` reads a
control's subject bytes, re-derives a directory-counted record's selection by
reading that directory, and resolves every cited fixture, evidence anchor, and
package manifest, so an ordinary Rust-only change can turn `--check` red on a
pull request where this lane is not eligible at all. The break would then merge
green and surface later on an unrelated roadmap change, attributed to an author
who touched none of it — a stale-evidence window inside the instrument built to
close stale evidence. Every artifact the validator opens is therefore cited to
its own lane as well, and the existing coverage check turns a missing trigger
path into a hard error rather than a note.

Where a binding does not resolve, the record carries a **derived limit**. A limit
is no longer a field an author may fill in or leave empty — the input schema has
no `limits` property at all, so declaring one is a schema error and omitting one
is not an option. It is computed from the workflow, it forces the covering claim
to bounded, and a bounded claim is inadmissible without an approved transfer. The
incentive that used to run backwards — disclosure fatal, silence free — is gone
because disclosure is not the author's move.

Two remaining edges were volunteered and are now derived. **Relevance**: `covers`
is a list an author writes, so on its own any green command could be credited
with any obligation. Each atom declares the artifact its evidence must exercise,
and each record's surface is derived from its own fixtures, path arguments, and
selected crate roots; appending an unrelated atom to a passing record is refused.
**Mutation locatability**: a negative control names the artifact it edited and
both halves of the edit, and the validator requires the replaced text to be
present exactly once and the introduced text to be absent — so `unique` and `new`
are verified against the tree under review, and a mutation that could never have
applied, or one still sitting in the tree, fails. A control whose observed
outcome parses as a transcript this validator would admit records no refusal and
is refused too.

That last one carries a cost worth naming instead of hiding. Asserting a
verbatim spelling of a named production file couples the instrument to bytes it
does not otherwise care about: a behaviour-preserving rename or reflow of the
mutated region breaks the control while changing nothing the register asserts.
Nothing weaker proves a reverted mutation was unique and new against the tree
under review, which is exactly the failure class the control exists to close, so
the coupling is accepted here — bounded to a control's own named subject, never
a search for a spelling across a tree. It is not a licence to multiply: a
successor adding controls over churning source should take the narrowest binding
that still proves uniqueness, such as anchoring to the test its mutation broke,
which the observed outcome already quotes.

Acyclicity is derived per RUNNER rather than per argument shape, because the two
runners name their work differently. A `node` command names a path; a `cargo`
command names a package. Deriving the verdict-producing artifacts from path
arguments alone made the rule vacuous for every `cargo` record — the adapter the
Rust successors' claims will use — so a selected crate root is a producer too,
and containment decides: a run of a tree is the verdict for a subject inside it.

Finally, a runner that announces its selection *before* doing the work does not
get that announcement counted as passes. The compile-fail runner prints its
fixture count and then runs; a run that failed afterwards still printed the
banner. Passes are counted from the per-case lines the runner emits as each case
clears, and the selected count is independently re-derived from the fixture
directory the runner enumerates.

## Findings close against an obligation, not a heading

A finding used to be closed by routing it to a claim. Routing is an assignment:
it cannot distinguish an obligation that discriminates the finding from one that
merely sits nearby. Every claim-routed finding therefore names the ATOM that
would fail if the finding were still live, and the validator requires that atom
to belong to that claim.

That is also why the downstream-narrative finding has an obligation of its own
rather than a receiving claim. Every charter in this train states its owner
twice, and the two statements are written by different hands. The generated
`owner=` header is one; the outcome sentence a reader meets first — naming the
owner being displaced and the one that ends up sole — is the other. Resolving
only the header would have left the claim "the ratified owner reached the
downstream narratives" asserting more than it checked, because a regenerated
header standing over untouched prose still hands the capability to the displaced
owner everywhere a reader actually looks. So the validator resolves both against
each charter's own bytes: the header must be the ratified owner, and a charter
that DELIVERS an outcome must narrate the ratified owner as final and this
register's displaced owner as current. That narrative half is resolved inside
the section that owns it, and only while there is exactly one pair there. Taking
the first matching sentence from the flattened document accepts a compliant
sentence written above a stale one — the reader meets both, the check saw one —
and a section written twice resolves silently to whichever came last, so both
shapes are refused rather than resolved by position. A historical identity
wrapper delivers no outcome and narrates no pair; that is the single exempt
shape, and the control suite exercises it rather than assuming it. A charter
still handing the capability to the displaced owner, in either register, fails
here instead of being closed by declaration.

## Mutation boundary

This block is authority and evidence bytes plus one production repair. The
charter's zero-LOC ceiling is a planning reference, and the drift against it is
exactly one line of shipped code with its coverage — the line without which the
contract this block ratifies would assert something false about the encoder it
describes. The reasoning for that single exception is in the section below;
everything else this block changes outside the authority tree is CI
configuration, repository-owned tooling, or test code.

Four repository paths outside the authority tree change IN THIS BLOCK'S OWN
COMMITS.

- `.github/workflows/ci.yml` gains trigger paths, which is how the lanes this
  register binds to become eligible on the artifacts it cites.
- `scripts/compile-contracts.mjs` gains a `--list-owners` flag that prints the
  owner list it already declares and exits; the lane's own invocation is
  untouched. It is there because the alternative was for the validator to
  scrape that list out of the script's source, which is the name-keyed
  scanning this block deletes — asking the script is the same discipline as
  asking the workflow for its job ids. This is repository-owned gate-lane
  tooling rather than incidental configuration, so the charter names it as a
  declared surface instead of leaving the reading to a reviewer.
- `crates/verter_identity/src/encoding.rs` gains the deduplication step that
  makes its set field a set, and the case that discriminates it from a sorted
  bag. This is the production repair, and it is one statement.
- `crates/verter_identity/src/identity.rs` changes in its composition doc
  comment and inside its `#[cfg(test)]` module: the doc now describes the
  encoder's actual behaviour, and the existing case asserts the repeat property
  separately from the order property. That is why the charter's `Test homes`
  line names `crates/verter_identity/src`: the evidence belongs in the crate
  that owns the type.

The enumeration is scoped to this block's commits deliberately, because that is
what it is evidence about, and it is the enumeration of the FINAL tree rather
than of any intermediate shape — a path that a squashed-away draft touched and
the landed diff does not is not a path this block changes, and listing one would
put a sentence here that a reader checking the diff cannot confirm. A candidate
branch also carries whatever else landed on it —
`packages/playground/scripts/capture-wasm-carrier-fixtures.mjs` is one such
file, changed by the block that migrated the WASM fixture capture and disclosed
by that block's own ledger row, not by this decision. Reading a branch-wide diff
against this list would attribute another block's change here; reading this list
as branch-wide would make it look incomplete. It is neither: it is the exact set
of out-of-authority files these commits touch, which is what makes the
production-surface claim auditable.

One evidence change is behavioural, and it lives in the crate that owns the
type. The profile-set obligation this block ratifies needs evidence that an
observed profile is part of the composed query identity, and the pre-existing
coverage proved only that profile ORDER is not — which a query identity that
dropped the profiles entirely would also satisfy. So the negative control for
that atom, the one that removes the profile field from
`QueryIdentity::compose`, was discriminated by nothing: the mutation passed.

The discriminating assertions extend the case that already existed, in
`crates/verter_identity/src/identity.rs`. An earlier shape of this change put
them in a new file under `crates/verter_session/tests/cases`, because that was
one of the two test homes the charter declared. That was charter geography
driving code placement, and it had three costs a reader pays later: coverage of
one type split across two crates, so an audit of `QueryIdentity` finds half of
it; an unrelated case in the largest integration binary in the workspace; and a
consumer package pulled into this block's evidence surface for no reason its own
contract needs. The lowest reusable owner rule points the other way, and so does
the instruction to extend one existing test before creating a new one. The
charter's `Test homes` line now declares `crates/verter_identity/src`, which is
the amendment the placement actually needed. The record that covers the case
selects one package, so an unrelated change to a consumer crate cannot move its
counts either.

## The encoder gap, and why it is repaired here rather than carried

An earlier draft of the contract composed `QueryIdentity` from "the canonical
sorted set of observed profile identities" while the shared leaf encoder,
`CanonicalEncoder::field_sorted_set`, sorted its elements and length-prefixed the
count without deduplicating them. `[p]` and `[p, p]` therefore composed different
identities: multiplicity was load-bearing exactly where the word "set" said it
must not be. A ratified contract asserting something false about the code it
describes is worse than a gap, because everything downstream reads it as settled.

Three shapes of a carry were tried before the repair, and they are recorded
because the sequence is what makes the repair the right answer rather than a
convenient one.

The first marked the atom as owed through a status column while leaving it
COVERED, so `CLM-IDENTITY` still derived `PROVEN`: a requirement known not to
hold reached the summary row under the status of one that holds, through a field
an author volunteered rather than a route a gate authorised.

The second over-corrected. It routed the atom through
`TCM0-R-IMPLEMENTATION-BASELINE`, which made `CLM-IDENTITY` derive
`PROVEN-BOUNDED`. That is inadmissible for a reason the first shape's repair
skipped past: the ruling this block implements admits three remainders by
identity AND by content — `TCM0-R-IMPLEMENTATION-BASELINE` is `C9`, the
pre-change comparison baseline — and closes the set with "no fourth residue ID
**or other bounded row** is admissible". Adding an unrelated second question to
an approved remainder is a fourth bounded row wearing an approved id, and
pointing `approved_by` at a charter that approves the remainder does not make it
an approval of the atom. A bounded `CLM-IDENTITY` is exactly the row the ruling
forbids.

The third stated both halves in the contract — the requirement, and what the
composition did today — and forwarded the requirement to `TCM3-AC2` as an
ordinary enforced obligation. That is defensible prose, and it is what this
record previously argued. It is nevertheless the weakest of the four available
answers. It leaves the ratified text describing a set the shipped code does not
implement. It leaves a successor obliged to change a crate that successor's own
charter does not name, which is a scope defect discovered while implementing.
And it routes AROUND the instrument's own anti-laundering machinery: that
mechanism exists to force an unmet obligation through an approved remainder, and
framing the obligation as a property of the contract TEXT meant no unmet
obligation was ever declared. A reader scanning `CLM-IDENTITY` saw seven covered
obligations and nothing owed, for a claim one of whose obligations the shipped
code did not meet.

The fourth answer is to make the sentence true, and that is what this block
does. Deduplication is one statement in `CanonicalEncoder::field_sorted_set`, in
a crate this charter declares as a production surface — the same crate the three
named identity boundaries are defined in. Fixing it at the leaf encoder rather
than at `QueryIdentity::compose` is deliberate: the method is named for a set,
every caller that reaches it is describing a set of observed things, and a repair
at the one call site would leave the shared method able to mint the same defect
at the next caller, under the name that made the mistake plausible. The only
other caller today — `verter_language`'s parse identity — collects its custom
elements through a `BTreeSet` before encoding, so nothing was live there; that is
the point rather than a counterexample, because it means the field's set
semantics were being supplied by each caller separately instead of by the method
that claims them. The composition now inherits them instead of restating them.

The evidence discriminates the property the encoding does not supply for free.
Sorting already made `[p1, p2]` and `[p2, p1]` agree, so an order-independence
case passes under either encoding; the repeat case is the discriminating half,
and it is asserted separately at both levels — over the encoder field and over
the composed identity. The negative control removes the deduplication step and
records what that costs: the order case still passes, and both repeat cases
fail.

So `A-profile-multiplicity-is-one-question` is now an obligation the shipped code
meets, anchored at the code rather than at the sentence, covered by a record that
executes it, and received at this block's own positive-contract criterion.
`CLM-IDENTITY` derives `PROVEN` because seven obligations hold, not because one
of them was framed as somebody else's.

Two consequences follow. TCM3's charter is not widened by this block: an earlier
routing needed some owner in a receiving sequence to declare a production surface
containing `crates/verter_identity/src`, and an earlier shape obtained that by
adding the tree to a LOCKED successor's `Production surfaces` line — a
mutation-boundary change to a successor, made to satisfy a check this block
introduced. With the obligation met here, no carry needs a receiver and TCM3's
charter is left as it was. And this charter, which declares
`crates/verter_identity/src` among its own production surfaces, is where the
repair belongs; the contract's own section 3 says the same thing from the other
direction, so the charter and the contract now agree rather than requiring a
reader to resolve them against each other.

## The owed-obligation route, and why it stays

Repairing the encoder leaves the owed-obligation machinery — the requirement that
an unmet obligation leave through an approved remainder to a receiver that may
change the surface — with no live carry in this register. That is a fair question
to ask of any mechanism, and the answer is not "it is there for later".

The input half changed. Declaring the disposition was optional, and an optional
input to a derivation is not an input at all: an author could disclose an unmet
composition in an atom's own statement, omit the field, keep the quotation
coverage, and derive `PROVEN` — which is precisely what the third shape above
did. Every atom now declares `shipped_obligation`, and its value is closed at
"the shipped code meets this" or the production path it does not. Silence is no
longer available, the mechanism has fifty-two live declarations rather than none,
and the only route to a met status is a positive claim about bytes a reviewer can
check. One control refuses the omission and one refuses a blank declaration, so
the requirement is discriminated rather than asserted.

What stays deliberately unwidened is the exit. An owed obligation must still
leave through one of the three approved remainders, and those three carry three
specific questions, so an unmet obligation fitting none of them cannot be routed
out of this block at all. That is the governing ruling's design rather than a gap
in the mechanism: section 8 of the contract says a newly discovered open question
requires an amendment and a new node, not a fourth remainder row. The
alternative — letting an owed obligation leave through a bare `received_by`
citation on a named surface — recreates exactly the volunteered fourth remainder
the first shape above was rejected for. So the mechanism is reachable for the
questions the ruling approved, closed for the ones it did not, and its input is
now mandatory rather than optional.

Two of this block’s four acceptance criteria need their disposition stated
rather than assumed, and only one of them is not applicable:

- **TCM0R-AC3 — incremental equivalence.** Applicable, and proved on the
  authority this block actually owns. Since the encoder amendment it owns one
  production cache-candidate key, not none: the canonical bytes of
  `QueryIdentity::compose`, including the observed-profile set algebra the
  shared encoder now supplies — sorted and deduplicated, inherited by every
  set-shaped composition rather than restated per caller. For a key, both
  halves of the criterion are key algebra, and the identity coverage
  discriminates them rather than asserting them. Incremental equals fresh is
  repeat-and-order collapse: the same question observed twice, or observed in
  a different order, composes the same key, so an incremental observation
  cannot land beside the slot a cold run would fill — asserted at both the
  encoder field and the composed identity, with the negative control that
  removes the deduplication step re-applied live. Degraded never warms is
  no-aliasing: a different observed-profile set or result contract composes a
  different key, and the basis never enters one, so an answer produced under a
  superseded snapshot cannot be found under a live question's slot; the
  compile-fail cases hold the type boundary, so a slot keyed by
  `InputBasisId` or `SemanticFlightKey` cannot even be written. What the
  contract *states* about warm and cold answers on the plane that will key
  slots on this identity is text received at `TCM3-AC3` and `TCM4-AC3`, which
  the register binds and the validator resolves. The block also owns one
  derived artifact with a publication lifecycle, the generated view, and both
  halves of the criterion are stated over it and both are executable.
  Incremental equals fresh: the committed `closure.md` must equal a fresh
  render byte for byte, and `--check` refuses a drifted one rather than
  reading it as current. Degraded never warms: publication is a separate and
  stricter decision than certification, and a run that produced errors
  publishes nothing, so a partial reading of a register the same run refused
  can never become the artifact freshness is then measured against.
  `publication()` is that gate, and one control exercises both halves in both
  directions. Recording this as not-applicable was wrong: the criterion asks
  what the changed scope publishes and keys, and the changed scope publishes
  something and keys a cache candidate.
- **TCM0R-AC4 — bounded work.** Not applicable. This block touches no hot path.
  The validator runs once per lane invocation over a few hundred declared rows,
  and there is no production parse, resolve, plan, emit, or retained candidate
  for it to duplicate. Adding counters or a soak here would be evidence invented
  to fill a slot, which this charter explicitly forbids.

## Two things the id sets do not pin, and one filter that has to stay

Pinning the claim, atom, row, and finding IDENTIFIERS beside the validator stops
the universe from shrinking under the register. It does not stop the universe
from being hollowed. Every id can stay, every count can stay, every coverage
list can stay, and the derived status can stay at PROVEN while what a claim
ASSERTS is quietly rewritten into something its existing evidence already
showed. Weakening a proposition to fit its proof is the same laundering as
promoting a bounded claim, in a shape no set check can see, and the reason the
identifier pin exists at all is that "the required universe is author-controlled"
was a defect in this instrument's own first draft.

So each proposition's digest is pinned beside its id, over whitespace-normalised
text: reflowing a paragraph is not a change, and rewriting what it says is. This
does not stop an author from correcting a statement — corrections are expected —
it stops one from doing it invisibly, because the repin is a line of the same
review as the rewrite.

Every proposition, not the three kinds that happen to carry an id of their own.
A deletion row's disposition is the sentence saying how a displaced route was
rejected; a remainder's statement is what says which question is carried; a
negative control's `mutation` and `observed` are the whole record of what that
control demonstrated; a receiving row's `gate` is what its owner must clear, and
the derivation constrains only its opening words; a record's `skip_basis` is why
its declared skips are expected rather than unexpected. Every one of them was
reachable by an edit that moved no id, no count, and no derived status, so every
one of them is pinned here too.

The control fields are the sharpest case, because they are the one place a
hollowing is invisible to the hollowed-statement control itself. Replacing an
`observed` transcript with "it broke something, exit 1" left every id, every
count and every derived status where it was, and the register kept publishing a
control whose discriminating power no longer existed anywhere in the tree. That
is the same laundering as weakening a claim, in the field that records the
evidence rather than the field that records the assertion.

Pinning what an atom SAYS still leaves where it POINTS author-controlled, and
the two fail differently. `evidence_anchor` is the sole input to the relevance
gate, so repointing it moves an atom onto whatever a green record happens to
touch and the gate then passes for a reason nobody chose — the coverage move
alone is correctly refused, which is what makes the anchor the load-bearing
half. `contract_section` and `contract_anchor` are the sole binding between an
atom and the ratified contract, so deleting a sentence and repointing the atom
at a surviving one leaves a pinned statement describing a contract that no
longer says it, with the statement pin silent because the statement's own bytes
never moved. Both are digested together, beside the statement pin and separately
from it, and `owed_surface` rides in the same digest because moving it moves the
ownership question the claim summary publishes.

The second thing is the tama trigger filter, which lists five crate trees and
the scripts the Rust and compile-contract lanes compute their own selections
from. That listing looks like breadth with no detection power — the roadmap job
becomes eligible on most Rust changes — and the case for deleting it has been
made. It must nevertheless stay: the validator cites every artifact it opens to
its own lane and hard-errors on any cited artifact no pattern covers, so
removing those patterns turns `--check` red on the next roadmap change. That is
a rejection with a reason, and it is recorded here rather than left to be
rediscovered, because the disposition was once summarised as a removal that had
not been made — and a written conclusion that does not match the tree is
precisely the failure this instrument exists to prevent.

What the listing has to cover is the point that was missed the first time. A
lane selector is an ENTRY: `scripts/provider-ci.mjs` prints the filter
expression, but the excluded package set and the name-scoped selectors this
check decomposes and publishes are declared in `scripts/provider-ci-internals.mjs`
and, below that, in `scripts/gate-internals.mjs`. Citing only the path-shaped
tokens of the executed command line cited the entry and stopped, so both modules
sat outside the tama filter while `--check`'s own derivation depended on them:
adding one selector to `PROVIDER_LIVE_SELECTORS`, or one exclusion to
`buildCanonicalSurface1FilterExpr`, turns `--check` red on a pull request the
roadmap job is not eligible for. The break merges green there and surfaces later
on an unrelated roadmap change by an author who touched none of it — verbatim
the failure the filter's own comment says it exists to prevent, and a proven
atom (`A-external-refresh-lane-bound`) asserting an invariant the derivation did
not hold.

The citation is therefore the selector's transitive first-party import graph,
walked with the same helper the acyclicity edge already uses and for the same
reason: a re-export must not be read as a boundary. It over-reads — an import is
treated as a contribution — and that direction is the safe one, because it can
only widen what a change has to re-run. The filter is widened to
`scripts/provider-ci*.mjs` and `scripts/gate-internals.mjs` so the coverage
check passes for the right reason rather than by omission.

## What a transcribed count is bound to, and what it is not

The instrument re-executes every record whose runner it can invoke and compares
its own derived counts against the transcription. It cannot invoke `cargo`, so
for those records it compares nothing — and review raised the consequence
directly: adding a passing test to a selected package moves `selected`,
`executed` and `passed` while the Rust lane stays green and `--check` stays
green with it.

That is real, and this candidate demonstrated it against itself. Moving the
observed-profile case out of `verter_session` and into the crate that owns the
type deleted one case from one of the three packages the targeted-domain record
selects. Its count did not move: the record shipped `9438 / 8889 / 549` and this
tree measures `9438 / 8889 / 549` with a case removed. A count that survives the
deletion of a case it was supposed to include was not derived from the tree it
shipped with. Every cargo-backed record here has now been re-run against this
tree and re-transcribed from the run's own terminal output, and every negative
control's mutation has been planted, run, and reverted, with the runner's own
refusal recorded rather than described.

Two dispositions follow, and both are recorded rather than left implicit.

**Adopted.** A count that can be moved by an UNRELATED change is strictly worse
than one that can only be moved by a related one, and two of the three cargo
records did not need the exposure they had. `P-identity-observation` selected
`verter_identity` and `verter_session` in order to reach a case that lived in
the consumer; with the case in its owning crate the record selects one package,
and nothing a consumer crate does can move its counts. `P-identity-semantics`
already selected one. What remains is `P-targeted-domain`, whose whole subject
is that three packages are green — breadth is the claim there, so narrowing it
would be answering a different question.

**Rejected, with the reason.** Making `--check` itself notice a drifted cargo
count needs one of two things this block may not have. Re-deriving the count
means invoking `cargo` from the roadmap lane, which would put a Rust toolchain
and a multi-thousand-test build behind a node-only, portable authority job —
and, by this register's own rule that a runner the instrument can invoke may not
opt out of re-execution, would drag the whole targeted-domain run into it.
Detecting drift without re-deriving means binding the record to a digest over
the sources its count is a function of, which the ruling this block implements
forbids in the sentence that defines an ordinary proof record: it "is not bound
to a repository SHA, tree, or digest."

Nor is "this instrument cannot invoke cargo" available as a disclosed limit. A
limit bounds its claim, and the same ruling closes the remainder set with "no
fourth residue ID **or other bounded row** is admissible", so recording it that
way would make every cargo-backed claim inadmissible and the register
unlandable. The ruling instead states the intended mechanism in the same breath:
a row the control suite cannot re-execute "is bound to the resolved CI lane that
re-runs the work instead". That binding is resolved here rather than asserted —
the job must exist, issue the exact command line, be gated on the named filter,
and reach the record's own packages — and what it does NOT establish is derived
and published in the record's own sentence rather than argued in prose.

That reasoning stands for a lane the roadmap job cannot invoke. It does NOT
stand for the four cargo records whose control the second lane already drives,
and the earlier version of this paragraph said it did — recording as a permanent
limit an absence that the control lane itself removes.

The control lane runs each of those records' OWN command, in the mirror, before
planting the mutation. That clean run's terminal summary is the record's own
counters, produced now rather than transcribed once, so the re-derivation needs
no new input and no cargo invocation from the roadmap lane: it is a comparison
between two things the lane already has. `reapply` makes it, and it is not
optional or sampled — every control that mutates an artifact goes through it. A
case added to or deleted from a record's selection now fails that record, which
is exactly the drift a self-consistent transcript hides. `P-identity-nonalias`,
`P-identity-semantics`, `P-identity-observation` and `P-identity-multiplicity`
are therefore re-derived, not merely transcribed.

`P-targeted-domain` is not, and that is the residual: it names no control, so no
lane re-runs its command, and its 9,438-case selection is exactly the whole-
workspace archive run this instrument may not invoke. For that ONE record a
count is current as of the run transcribed beside it, and what a later change
can be relied on to re-run is the WORK, not the transcription.

Closing that last one needs what the earlier paragraph described, and the
description is kept because it is still true of this case: what is missing is
not a check but an INPUT. The lane that already re-runs the work does not emit
what it counted in any form this validator can read back. Give that lane a
machine-readable counter artifact — the run's own
selected/executed/passed/failed/skipped, written by the runner that produced
them — and the existing derivation applies unchanged, with no cargo invocation
from the roadmap lane and no digest over sources. It is recorded here, in the
block that raised it, because the closed remainder set admits no fourth bounded
row to carry it and a residue minted for it would be inadmissible.

A second residual belongs beside it. A negative control's recorded outcome must
now be the runner's own refusal in the runner's own grammar — a libtest
`FAILED` line with a nonzero count, a nextest summary that failed or selected
nothing, a `node:test` block with a nonzero fail, an `ERROR:` line from a tool
that has no other refusal channel, or the compile-contract runner's banner with
fewer cleared cases than it announced. A sentence describing a failure no longer
parses, which is what a mutation that was never planted leaves behind. It does
not make the transcription unforgeable — nothing available to a node-only
instrument does — it moves the bar from "any sentence at all" to "a transcript
of the shape that runner emits, consistent with the mutation's own subject and
with the uniqueness and absence checks beside it".

## Dispositions

Every correctness finding raised against this block that touched its own scope
is dispositioned here rather than carried as an open deferral. Three subjects
were raised as candidates for deferral and all three are ADOPTED, because each
is inside the instrument this block ships and deferring work inside a block's own
deliverable is the defect the disposition rule exists to prevent.

- **ADOPT-NOW — the owed-obligation field's live use.** `shipped_obligation` had
  been applied mechanically as `met` across all fifty-two atoms, three of which
  are this register's own transferred remainders, so "the shipped code meets
  this" and "this is an open question for a successor" were asserted about one
  atom with nothing deriving the contradiction. The field is now a closed
  vocabulary — `met`, `authority-only`, `carried`, or the production path an
  obligation is unmet at — and each named value is refused against a
  contradiction the register already carries. Discriminated by the
  `owed-obligation-unowned` control.
- **ADOPT-NOW — cargo-record counter re-derivation.** Four of the five
  cargo-backed records are now re-derived by the control lane's own clean run of
  the record's command, not merely transcribed. See the section above for what
  remains and its exact shape.
- **ADOPT-NOW — the control wrapper's own gate.** A case registered as a control
  had only to SUPPLY a mutator, so an empty one satisfied the gate, and nothing
  required the case to reach a refusal at all. Both halves are now derived from
  what the case produced: the plant counts only when the fixture differs from the
  baseline, and the refusal counts only at the places that derive one.

No deferral is open at this block's close. A defect discovered later is an
amendment and a new node, not a debt row minted here after the fact.

## What stays open

Three remainders, and only three. The validator holds that closed set, so a
fourth cannot be introduced by editing the register.

- `TCM0-R-HANG-TOPOLOGY` — how the semantic plane behaves under concurrent
  flights.
- `TCM0-R-TOPOLOGY-SELECTION` — which projection and semantic topologies are
  selected.
- `TCM0-R-IMPLEMENTATION-BASELINE` — the pre-change comparison reference the
  successors measure against, which an authority-only change cannot produce.

Each binds ordered receiving rows naming a strict descendant inside this train,
a criterion that descendant's charter declares under the role the row needs, and
a gate sentence that opens by naming the owner the criterion resolved against. A
newly discovered open question requires an amendment and a new node, not a
fourth row.

The closed set is not the whole shape, so the ROUTING is pinned too. Which atom
leaves through which remainder, and which blocks receive that remainder in which
order, are held beside the validator and matched exactly. Without that, an atom
re-routed to a different admissible remainder lands under receiving rows
resolved for a different question, and a required owner dropped from a
remainder's sequence still leaves a non-empty, correctly numbered, correctly
roled list — both are substitutions every membership, descendant, train, and
role check accepts. The block that must produce the baseline cannot be dropped
from the sequence that receives the baseline remainder.

## Portability

The register's adapter runners are the closed pair `node` and `cargo`. A
shell, batch, or interpreter control is not expressible, which is how the
displaced package's tracked POSIX controls are rejected structurally rather
than removed by inspection and re-added later.
