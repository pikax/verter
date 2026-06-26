# Cache-key guard mechanisms — binding architecture rulings (recorded scanners, scanner dispositions, fail-closed registry walk)

This file is the permanent, in-repo, machine-independent home for the binding
architecture rulings that govern the content-free cache-key substrate's guard
mechanisms. It records WHY a small set of cache-key-hygiene guards are RECORDED
SOURCE SCANNERS rather than compiler/structural enforcement, the disposition of
two specific scanners (carrier-verdict retired-symbol absence; synthetic
explicit-deepen ordinal-key shape), and the fail-closed registry-completeness
walk verdict that lands alongside them.

These are durable design decisions, not transient notes. Each scanner that
relies on a ruling here cites this file by its committed path in its
`mechanism_ruling` record.

---

## 1. Recorded scanners vs structural enforcement

Three cache-key-hygiene guards are RECORDED SOURCE SCANNERS — bounded text
scans over a precisely-scoped surface — because no compiler/structural mechanism
expresses their invariant today:

- `no_unsanctioned_semantic_node_id_in_shape_or_materialize_key`
  (`crates/verter_session/tests/cases/g_cache/r6_r21_query_identity_keys.rs`).
- `no_carrier_verdict_db`
  (`crates/verter_session/tests/cases/g_misc2/no_carrier_verdict_db.rs`).
- `synthetic_carrier_explicit_deepen_routes_through_shape_cache_key`
  (`crates/verter_session/tests/cases/g_misc2/synthetic_carrier_explicit_deepen_routes_through_shape_cache_key.rs`).

### Ruling

Each of the three is classified as a RECORDED SCANNER, NOT structural
enforcement, for the scoped cache-key work. None may be described in its record
as a structural/compiler-enforced assert.

### Rationale

- **Key-field typing has no negative trait bound.** "Forbid these field TYPES in
  a derived-`Hash` key while allowing exactly two `SemanticNodeId` positions" has
  no compiler mechanism: Rust has no negative/forbidden-field-type bound, and the
  raw `Hash16`/`HashValue` aliases cannot distinguish `resolve_env_hash` from
  `whole_hash` by type. A bounded source scan over the key struct/enum bodies is
  the justified expression until a key-safety newtype substrate exists.
- **Retired-symbol absence is name-spelling-scoped.** Proving "this identifier is
  absent from the entire production source and must never be reintroduced" is not
  expressible structurally: a compile-fail import would catch only a PUBLIC
  reintroduction, not a private type/module/field/accessor. Absence of a retired
  spelling across `crates/*/src/**` is expressible only as a source name scan.
- **The synthetic ordinal-key shape is not structurally impossible yet.**
  `SemanticNodeId` is a public tuple struct and the synthetic carrier's
  `value_node` is a public `u64`, so a direct `SemanticNodeId(<ident>.value_node)`
  ordinal-key construction is reachable. A bounded direct-shape scan is a
  permitted residual supplement on top of the PRIMARY structural confinement
  (sealed `NonSyntheticTypeExpr`, module-private `ShapeSubject` / `ShapeCacheKey`
  construction, sealed `MemberShapeNodeSubject`).

### The structural close is recorded debt, not done here

The design that DOMINATES the TWO `SemanticNodeId`-keyed scanners
(`no_unsanctioned_semantic_node_id_in_shape_or_materialize_key` + the
synthetic-deepen ordinal-key scanner) — making raw `SemanticNodeId(u64)`
construction impossible outside the owning layer (private tuple field + narrow
constructors) and giving the env/version key dimensions distinct newtypes so a
forbidden field type is a type error — is a SEPARATE, repo-wide structural-design
item. It is recorded as the key-safety newtype substrate debt
(`docs/arch/key-safety-newtype-substrate-debt.md`), not smuggled into this
scoped work. Until it lands, those two guards remain recorded scanners.

That structural close does NOT dominate `no_carrier_verdict_db`: its invariant is
retired-symbol absence, and newtyping hashes / sealing `SemanticNodeId` does not
prevent reintroducing a private `CarrierVerdictDb` / `carrier_verdicts` symbol.
The retired-symbol scanner STAYS a recorded scanner (or would need its own
separate structural closure for retired-symbol absence) regardless of the newtype
substrate.

---

## 2. Carrier-verdict retired-symbol scanner — preserved, not retired

The carrier-verdict retired-symbol absence scanner (`no_carrier_verdict_db`)
remains an active guard. It is the justified mechanism for proving the retired
carrier-verdict substrate's symbols stay absent from production source (see §1).

### Ruling

- PRESERVE the scanner and its self-test. Retiring it is out of scope and would
  drop a real absence guarantee.
- The carrier-verdict retired-symbol scanner is the SOLE occupant of
  `no_carrier_verdict_db.rs` (the retired-symbol scan plus its self-test).
- It carries a guard-local scanner record naming its invariant, justification,
  and this ruling.

---

## 3. Synthetic explicit-deepen scanner — bounded direct-shape supplement, prose narrowed

The synthetic explicit-deepen scanner bans the direct
`SemanticNodeId(<ident>.value_node)` ordinal-key shape and lives in its own file
(`synthetic_carrier_explicit_deepen_routes_through_shape_cache_key.rs`), separate
from the carrier-verdict retired-symbol scanner.

### Ruling — disposition

- PRESERVE the scanner as a bounded residual supplement to the structural
  primary. It is not redundant and is not deleted.
- It carries a full guard-local scanner record.

### Ruling — claim scope (narrowed, matcher unchanged)

The scanner enforces EXACTLY the direct, whitespace/newline-tolerant
`SemanticNodeId(<single_ident>.value_node)` shape — and nothing broader. The
record states the scope as that exact shape, not "any/every construction."

Explicitly OUTSIDE the scanner's claim (covered by the structural primary, not
by this text scan):

- a receiver EXPRESSION — `(expr).value_node`;
- CHAINED field access — `self.carrier.value_node`;
- METHOD/INDEX receivers — `carrier().value_node`, `foo[0].value_node`;
- BINDING INDIRECTION — `let vn = carrier.value_node; SemanticNodeId(vn)`.

The matcher is NOT broadened to chase these. Broadening a text scanner into
receiver-expression / chained-access / data-flow territory would add false
confidence without improving the real guarantee: a text scan stays vulnerable to
aliases, re-exports, macros, bindings, helpers, and expression laundering, while
structural confinement is the primary cache-safety mechanism. The honest-narrow
prose is non-broadening maintenance and does not advance the scanner's hardening
bound.

---

## 4. Registry-completeness walk — fail-closed

The registry-completeness meta-guard
(`crates/verter_session/tests/cases/g_misc0/critical_rules_have_guards.rs`)
walks the crate tree to confirm every critical rule has a registered guard. A
fail-OPEN directory walk could make the system believe guard coverage exists
when files or subtrees were never scanned, weakening the guard registry itself.

### Ruling

The walk fails CLOSED:

- per-`DirEntry` iteration unwraps explicitly and PANICS on an errored entry —
  no silent skip of an unreadable entry;
- directory classification routes through a uniquely-named fail-closed helper:
  a successful metadata read decides `is_dir`; a legitimate `NotFound` (a crate
  may genuinely lack `src/` or `tests/`) is the only tolerated skip; any other
  metadata error PANICS with the offending path;
- recursive `read_dir` failures PANIC except the legitimate `NotFound` skip;
- a uniquely-named self-test pins the fail-closed walk discipline (a unique name
  so two same-named self-tests in different files cannot leave the contract
  unprotected), contrasting a not-a-directory panic against a tolerated
  `NotFound` no-panic;
- a corresponding registry entry records the guard under that unique self-test
  name.

Rarity of repository-metadata errors does not make silent vacuity acceptable for
a production architecture guard.
