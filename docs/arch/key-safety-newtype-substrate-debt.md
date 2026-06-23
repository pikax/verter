# Key-Safety Newtype Substrate — structural debt (P1) + adjacent ShapeCacheDb perf debt

This file tracks two related, deferred items on the content-free cache-key
substrate. Both are RECORDED debt with explicit closure criteria — they are not
silent gaps.

**Ruling source (binding architecture design ruling)**:
`docs/arch/cache-key-guard-mechanism-rulings.md` — which classifies these guards
as recorded scanners (not structural enforcement), records the structural close
as debt, and pins the synthetic explicit-deepen prose as narrowed (the text
matcher is not broadened).

---

## 1. Key-safety newtype substrate (STRUCTURAL debt, P1)

### Status

DEFERRED. Three cache-key-hygiene guards are RECORDED SOURCE SCANNERS today
because the structural (compiler-enforced) mechanism does not exist yet:

- `no_unsanctioned_semantic_node_id_in_shape_or_materialize_key`
  (`crates/verter_session/tests/cases/g_cache/r6_r21_query_identity_keys.rs`) —
  forbids the content/version field markers and pins the `SemanticNodeId`
  allow-list on the shape/materialize derived-`Hash` keys.
- `no_carrier_verdict_db`
  (`crates/verter_session/tests/cases/g_misc2/no_carrier_verdict_db.rs`) —
  retired-symbol absence scan.
- `synthetic_carrier_explicit_deepen_routes_through_shape_cache_key`
  (`crates/verter_session/tests/cases/g_misc2/synthetic_carrier_explicit_deepen_routes_through_shape_cache_key.rs`)
  — bans the direct `SemanticNodeId(<ident>.value_node)` ordinal-key shape.

These are name-spelling source scans, not structural asserts, because Rust has
no negative/forbidden-field-type trait bound: the raw `Hash16` / `HashValue`
type aliases cannot distinguish `resolve_env_hash` from a `whole_hash` by type,
and `SemanticNodeId` is a public tuple struct whose `0: u64` (and
`SyntheticCarrierKey.value_node: u64`) can be written into a key by hand.

### Why a scanner today (the soundness/scope trap)

A blanket `SemanticNodeId` ban would be UNSOUND — it would catch legitimate
intra-graph operands (`SemanticQueryKey::Instantiate.args`, `ProjectMember.base`,
`ProjectPath.base`, `ResolveOverloadSet.callee`, `IndexKey::TypeNode`). So the
shape/materialize scanner is scoped to the shape/materialize key bodies only,
and the synthetic-deepen scanner matches exactly the direct single-ident
`SemanticNodeId(<ident>.value_node)` shape. Per the binding ruling, broadening
the synthetic-deepen text matcher into receiver-expression / chained-access /
binding-indirection data-flow would spend effort on a text scanner still
vulnerable to aliases / re-exports / macros / helpers / expression laundering —
increasing false confidence WITHOUT improving the real guarantee, because
structural confinement is the PRIMARY cache-safety mechanism. That is the wrong
direction; the right direction is the structural close below.

### Closure criteria

The TWO `SemanticNodeId`-keyed scanners
(`no_unsanctioned_semantic_node_id_in_shape_or_materialize_key` + the
synthetic-deepen `value_node` scanner
`synthetic_carrier_explicit_deepen_routes_through_shape_cache_key`) become
STRUCTURAL (compiler-enforced) and are DELETED once BOTH land:

1. **(a) Newtype the env/content hashes.** Replace the bare `Hash16` /
   `HashValue` aliases with distinct newtypes so `resolve_env_hash` is
   type-distinct from a `whole_hash` / `content_hash`. A content/version hash
   then cannot be written into a derived-`Hash` query-identity key without a
   type error — the `no_unsanctioned_semantic_node_id_in_shape_or_materialize_key`
   marker scan is no longer needed.
2. **(b) Seal `SemanticNodeId` + the `value_node` tuple fields.** Make the
   `SemanticNodeId` tuple field (and `SyntheticCarrierKey.value_node`) private
   with narrow constructors so a raw `SemanticNodeId(u64)` /
   `SemanticNodeId(<ident>.value_node)` construction is IMPOSSIBLE outside the
   owning layer. The synthetic-deepen scanner and the `SemanticNodeId`-position
   half of the shape/materialize scanner are then structurally moot.

When (a) + (b) land, structural enforcement replaces those TWO
`SemanticNodeId`-keyed scanners and they are DELETED. The ruling notes the
structural close — specifically the private `SemanticNodeId` tuple field — is the
ONLY design that dominates scanner-chasing for the `SemanticNodeId`-keyed
invariant. This makes those two scanners "last-resort-WITH-A-PATH", not
permanent.

The THIRD scanner, `no_carrier_verdict_db`, is NOT closed by this substrate. It
is a RETIRED-SYMBOL absence scan: newtyping the env/content hashes and sealing
`SemanticNodeId` / `value_node` does not prevent reintroducing a private
`CarrierVerdictDb` / `carrier_verdicts` / `carrier_verdict_db` symbol, so Rust's
type system cannot structurally close it. It REMAINS a recorded scanner (or would
need its OWN separate structural closure for retired-symbol absence — a distinct,
not-yet-designed mechanism), independent of the newtype substrate landing.

---

## 2. ShapeCacheDb single-entry → multi-candidate (CONDITIONAL perf debt)

### Status

DEFERRED — CONDITIONAL, not mandatory for the content-free cache-key cutover.

`ShapeCacheDb` is currently a single-entry-per-key cache. The family-memo slots
(`SemanticQueryKey::Instantiate.base` / `ResolveMacroPayload.owner`) use the
multi-candidate substrate (concurrent env/version variants coexist as candidates
in one slot, capped by `FAMILY_SLOT_CANDIDATE_CAP`). Migrating `ShapeCacheDb`
onto that same multi-candidate substrate is a possible future-perf item.

A binding ruling classified this "if it shows up hot, flag — not mandatory": the
single-entry `ShapeCacheDb` is correct (a candidate that fails validation is
recomputed); the multi-candidate migration is purely a contention optimization.

### Closure criterion

Revisit ONLY if `ShapeCacheDb` shows up as a hot single-entry contention point
in a profile (e.g. concurrent env variants thrashing one slot). Absent that
signal, the single-entry form stays. This row is cleared when either the
migration lands (after a hot-path profile justifies it) or the conditional is
explicitly retired.
