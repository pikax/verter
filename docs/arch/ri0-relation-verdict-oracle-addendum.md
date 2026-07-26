# RI-0 — Relation-Verdict Oracle Value Family (addendum to U0 + U2.RELATION_INFER)

> Status: **LOCKED for the RI-0 capture block.** Narrowly scoped addendum: it adds
> a SECOND closed value family (`relation_verdict`, schema v4) to the U0 oracle
> harness. It does NOT amend the restored `docs/arch/u0-oracle-harness-design.md`
> or `docs/arch/u2-relation-infer-design.md` contracts; where those documents are
> silent this file is the authority for the new family ONLY.
>
> **Capture-only.** This block captures tsgo's relation verdicts as checked-in
> snapshots. It makes NO relation-engine semantic change, lands NO RI-3 cutover,
> claims NO parity on rows the engine gets wrong, and does NOT label the family
> M=0. The honest state is *captured records with a known-mismatch ledger*.

## 1. Empirical wire proof (pinned tsgo `7.0.0-dev.20260526.1`)

Driving the pinned tsgo's `textDocument/hover` (empty client capabilities — the
pinned Q3 driver shape, bare plaintext hover, no markdown fence) over

```ts
type __oracle_probe__N = [Source] extends [TargetWithInfer]
  ? readonly [true, readonly [readonly [0, "A", A], readonly [1, "B", B]]]
  : readonly [false, readonly []];
```

returns the REDUCED tuple wire as the hover alias RHS. Confirmed against the
live binary (oracle corpus tsconfig: `strict`, `exactOptionalPropertyTypes`,
`target es2020`, `moduleResolution bundler`, no lib files):

| Probe (source → target) | Hover RHS |
| --- | --- |
| `never` → `string` | `readonly [true, readonly []]` (ONE true — no distribution collapse) |
| `any` → `string` | `readonly [true, readonly []]` (ONE true — the outer tuples suppress `any`'s both-branch union) |
| `unknown` → `string` | `readonly [false, readonly []]` |
| `{ value: number }` → `{ value: infer V }` | `readonly [true, readonly [readonly [0, "V", number]]]` |
| `[1, 2, 3]` → `[infer H, ...unknown[]]` | `readonly [true, readonly [readonly [0, "H", 1]]]` (literal `1`) |
| `[1, 2, 3]` → `[unknown, ...infer R]` | `readonly [true, readonly [readonly [0, "R", [2, 3]]]]` |
| `() => "hello"` → `(...args: any[]) => infer R` | `readonly [true, readonly [readonly [0, "R", "hello"]]]` |
| `(x: string, y?: number) => void` → `(...args: infer A) => any` | `readonly [true, readonly [readonly [0, "A", [x: string, y?: number \| undefined]]]]` |
| `{ a?: string }` → `{ a: string }` | `readonly [false, readonly []]` (exactOptionalPropertyTypes) |
| `[string \| number]` → `[string]` | `readonly [false, readonly []]` (whole union, no distribution) |
| `{ readonly a: string }` → `{ a: string }` | `readonly [true, readonly []]` |
| `{ a: string }` → `{ readonly a: string }` | `readonly [true, readonly []]` |

Hover is ONLY the transport. The persisted value is produced by a STRICT
tuple-wire decoder that accepts exactly this grammar; anything else is a loud
generation error.

## 2. The v4 `relation_verdict` identity (closed shape)

`ORACLE_SCHEMA_VERSION` 3 → 4. `relation_verdict` joins `structured_type_expr`
in the closed `OracleValueKind` set; each kind has a DISTINCT, closed,
`deny_unknown_fields` identity DTO — cross-kind fields and unknown fields are
rejected at strict decode. The v3 TypeExpr identity is UNCHANGED (no relation
fields bolted on).

The `relation_verdict` identity axes (all registry-derivable, tsgo-free; NO
graph-local node IDs anywhere):

- `source_operand` / `target_operand` — canonical operand ASTs: the spec's
  source / target type texts lowered through the harness's strict OXC lowerer
  and normalized under the ONE fixed relation-binding projection (`Expanded`).
  `infer X` positions in the TARGET are encoded as reserved binder refs
  (`__oracle_binder__X`) inside the canonical AST JSON — a closed encoding used
  only inside this identity axis, never in `oracle_value`.
- `binder_layout` — ordered `[{ ordinal, name }]`, target-pattern binder
  PREORDER (NOT name sort); ordinals exactly `0..n-1`; duplicate names rejected;
  names must set-match the reserved binder refs in `target_operand`.
- `relation` — closed tag; only `"assignable"` is admissible this block.
- `policy` — canonical closed record `{ overload_selection, excess_property_check,
  variance }`; only the default record is admissible this block.
- `freshness` — closed tag; only `"regular"` this block.
- `inference_mode` — closed tag `"none" | "target_pattern"`, validated consistent
  with `binder_layout`.
- `host_project` + `workspace_files` — the same host/config + workspace identity
  axes as v3.

The v4 `snapshot_id` derivation hashes this field set under a DISTINCT domain
tag (`verter.oracle.snapshot_id.relation.v1`) with the same length-prefixed
BLAKE3 recipe; the v3-family derivation (tag `…snapshot_id.v2`) is untouched.

## 3. Schema / envelope consequences

- The envelope is kind-keyed for the migration-fidelity mirror:
  `migration_fingerprint` + `migration_fingerprint_version` are REQUIRED for
  `structured_type_expr` (unchanged v3 rule) and FORBIDDEN (cross-kind fields)
  on `relation_verdict` — relation rows are capture-only NEW rows, never lifts,
  so they carry no retained-lift provenance (no sentinel values).
- `raw_capture.probe_scaffold` stays `null` for relation captures; the v3
  scaffold-consistency rail keys off the v3 identity only. The v4 analog: the
  stored `raw_capture.probe_header` must equal the versioned tuple-wire
  synthesis re-derived from the identity (a pure function), and the recorded
  hover must re-decode through the strict tuple-wire decoder to the stored
  `oracle_value`.
- `oracle_value` for `relation_verdict`:
  `{ "verdict": "assignable" | "not_assignable",
     "bindings": [{ "ordinal": 0, "name": "A", "bound": <normalized TypeExpr JSON> }] }`
  — bindings ordered by target-pattern binder preorder; ordinal AND name must
  match `binder_layout`; `bound` is canonical normalized TypeExpr JSON under the
  one fixed relation-binding projection; a bound that cannot be projected
  losslessly is rejected at generation; a false verdict carries no bindings.

## 4. Corpus: 26 relation identities from the 28 projection contracts

The `relation_semantics.rs` suite (28 projection contracts) REMAINS a separate,
untouched suite. Two contract PAIRS collapse into identical RAW relation
identities (the distribution scaffold is projection-level, not relation
identity): `never`→`string` (direct + via-generic) and `string|number`→`string`
(distributive + tuple-wrapped). 28 − 2 = 26 registry specs. NO strict-axis
multiplication (no ON/OFF pairs).

## 5. Observation boundary + known-mismatch ledger (no RI-3)

`ObservedRelationVerdict` is the ONE normalized boundary matching the oracle
DTO. The test adapter calls `relate_nodes` ONLY for its actually-supported
identity (assignable, default policy, regular source, no inference context) and
REJECTS broader keys. Pre-cutover `execute(SemanticQueryKey::Relate)` stays an
explicit `Miss` (guarded). Engine bindings are raised through the same
normalized projection before comparison. `Unknown` / `Miss` / `BudgetExceeded`
are engine failures, NOT oracle verdicts. Parity enforcement is DISABLED for
the mismatch-ledger rows (the engine's live answer is pinned as data, so a
future engine fix flips the row loudly); the mismatch ledger lives in the
registry as first-class data.

## 6. Landed implementation notes (this block)

- **Envelope**: `source_admission_digest` is kind-keyed like the migration
  mirror — REQUIRED for `structured_type_expr`, FORBIDDEN on
  `relation_verdict`. A capture-only relation row has NO source-admission walk
  (its workspace file is the synthesized probe, which would REJECT under
  admission), so recording one would be fabricated data.
- **Generation-time engine resolution**: the generator resolves the EXACT
  pinned `@typescript/native-preview` `7.0.0-dev.20260526.1` binary
  (`VERTER_TSGO_BIN`, then the project-local `node_modules` flat / pnpm-store
  layouts) and requires `--version` to report the pin exactly. It does NOT
  ride the product toolchain resolver — the resolver's stable-only support
  window (`>=7.0.2, <7.1.0`) governs the PRODUCT's engine provisioning, not
  this dev-only harness, whose every snapshot records the pinned nightly in
  `tsgo_version` (validated by the consumption driver).
- **Engine-observation fixture**: the adapter synthesizes
  `type __OracleSource = <source_text>; type __OracleTarget = <target_text>;`
  per spec (adapter-internal, not an identity axis), resolves both aliases
  through the ONE shared resolver, and calls `relate_nodes`. A binder-carrying
  spec is rejected as `UnsupportedKey` BEFORE touching the engine (a target
  pattern with `infer` is an inference context the engine's key does not
  support this block). The ledger seats exactly 9 rows: 6 `UnsupportedKey`
  (the infer rows) + 3 `MismatchedVerdict` (`relation_optional_to_required`:
  engine `assignable`; `relation_readonly_to_mutable`: engine `not_assignable`;
  `relation_fixed_to_first_rest`: engine `not_assignable`).
