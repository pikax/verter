# Phase 0a — Tier 1 known mismatches (Verter resolver defects)

This file is the deferred-defect register produced by the Phase 0a
worker per §0p.A.4 case-2. It lists each Class A fixture whose
hand-derived TS-spec / Verter-rule expected value DIFFERS from
Verter's current `ComponentMetaAnalysis` output, classifies the
defect, and links to the rule citation that the resolver violates.

The .snap.json files for these fixtures capture **Verter's current
behaviour** (so the gate locks in non-drift on the regression),
NOT the rule-correct value. Each case is a known defect that future
phases must close. When a phase fixes one of these defects:

1. The phase brief MUST carry `EXPECTS_SNAPSHOT_REGEN: <reason>`
   (§0.6.4) — the .snap.json drift is intended.
2. The worker re-derives `expected.rs::<id>()` to the rule-correct
   value (per the `EXPECTED OUTPUT` column below), removes the
   `KNOWN DEFECT` annotation from `expected.rs` and the derivation
   note, runs `--ignored generate_class_a_snapshots_from_expected`,
   and removes the row from this file.

Each mismatch was verified by reading TS spec §X.Y or
`.claude/skills/...` directly — NOT by running Volar or
vue-component-meta (Tier 1 authorship rule, §0p.A.0).

---

## 1. `mapped_exclude` — Exclude<T,U> not evaluated

| Field          | Value                                                          |
|----------------|----------------------------------------------------------------|
| RULE CITATION  | TS spec §4.4 (`Exclude<T,U> = T extends U ? never : T`)         |
| EXPECTED       | `kind: "a" \| "c"` (distributive conditional, `'b'` filtered)  |
| ACTUAL         | `kind: /*unknown*/ semanticMiss`                               |
| ROOT CAUSE     | `Exclude<>` is not evaluated through Verter's macro path. The  |
|                | analyzer surfaces the unresolved utility as `semanticMiss`.    |
| OWNER (later)  | Type-resolver / mapped-type evaluator, likely Phase 5 (engine  |
|                | retirement) or a dedicated utility-evaluation phase.            |

### Rule-correct expected (machine-readable per §5.B.5.1 r15)

The JSON block below is the hand-authored rule-correct `SnapshotView`
for `mapped_exclude`, derived from TS spec §4.4 (distributive
conditional `Exclude<T,U> = T extends U ? never : T`). The Phase 5i
§5.B.5.1 rule-correctness gate test
(`deferred_fixture_mapped_exclude_byte_equal_to_rule_correct_expected`
in
`crates/verter_session/tests/correctness/deferred_fixtures_rule_correct.rs`)
asserts byte-equality between this block and Verter's
post-Phase-5i-fix output. Discrimination: pre-fix Verter produces
`/*unknown*/ semanticMiss`; post-fix Verter produces `"a" | "c"`.

```json
{
  "fixture_id": "mapped_exclude",
  "expected": {
    "component_name": "C",
    "props": [
      {
        "name": "kind",
        "type_signature": "\"a\" | \"c\"",
        "required": true,
        "has_default": false,
        "default_signature": null,
        "doc": null
      }
    ],
    "events": [],
    "slots": [],
    "models": [],
    "exposed": [],
    "fallthrough": null,
    "flags": {
      "async_setup": false,
      "has_inherit_attrs_false": false
    }
  }
}
```

## 2. `mapped_extract` — Extract<T,U> not evaluated

| Field          | Value                                                          |
|----------------|----------------------------------------------------------------|
| RULE CITATION  | TS spec §4.4 (`Extract<T,U> = T extends U ? T : never`)         |
| EXPECTED       | `kind: "a" \| "b"`                                             |
| ACTUAL         | `kind: /*unknown*/ semanticMiss`                               |
| ROOT CAUSE     | Same as `mapped_exclude` — the distributive-conditional        |
|                | utility is not evaluated.                                       |
| OWNER (later)  | Same as `mapped_exclude`.                                       |

### Rule-correct expected (machine-readable per §5.B.5.1 r15)

Hand-authored rule-correct `SnapshotView` for `mapped_extract`,
derived from TS spec §4.4 (distributive conditional
`Extract<T,U> = T extends U ? T : never`). The Phase 5i §5.B.5.1
rule-correctness gate test
(`deferred_fixture_mapped_extract_byte_equal_to_rule_correct_expected`)
asserts byte-equality. Discrimination: pre-fix Verter produces
`/*unknown*/ semanticMiss`; post-fix Verter produces `"a" | "b"`.

```json
{
  "fixture_id": "mapped_extract",
  "expected": {
    "component_name": "C",
    "props": [
      {
        "name": "kind",
        "type_signature": "\"a\" | \"b\"",
        "required": true,
        "has_default": false,
        "default_signature": null,
        "doc": null
      }
    ],
    "events": [],
    "slots": [],
    "models": [],
    "exposed": [],
    "fallthrough": null,
    "flags": {
      "async_setup": false,
      "has_inherit_attrs_false": false
    }
  }
}
```

## 3. `template_literal_as_key` — template-literal key iteration loses props

| Field          | Value                                                          |
|----------------|----------------------------------------------------------------|
| RULE CITATION  | TS spec §4.5 (template literal types in mapped key positions)   |
| EXPECTED       | `props = [prefixA: number, prefixB: number]`                   |
| ACTUAL         | `props = []` (empty)                                            |
| ROOT CAUSE     | Verter's mapped-type evaluator does not interpolate the         |
|                | template literal across the source union, dropping all keys.    |
| OWNER (later)  | Mapped-type evaluator + template-literal lowering. Touches      |
|                | `verter_semantic::analysis::type_expand` plus the mapped/typed-  |
|                | key handling in the macro resolver.                            |

### Rule-correct expected (machine-readable per §5.B.5.1 r15)

Hand-authored rule-correct `SnapshotView` for
`template_literal_as_key`, derived from TS spec §4.5 (template
literal types in mapped key positions). The Phase 5i §5.B.5.1
rule-correctness gate test
(`deferred_fixture_template_literal_as_key_byte_equal_to_rule_correct_expected`)
asserts byte-equality. Discrimination: pre-fix Verter produces
either `props = []` or props named after the source literals (`A`,
`B`) without applying the `as <template>` clause; post-fix Verter
produces `prefixA: number` and `prefixB: number`.

```json
{
  "fixture_id": "template_literal_as_key",
  "expected": {
    "component_name": "C",
    "props": [
      {
        "name": "prefixA",
        "type_signature": "number",
        "required": true,
        "has_default": false,
        "default_signature": null,
        "doc": null
      },
      {
        "name": "prefixB",
        "type_signature": "number",
        "required": true,
        "has_default": false,
        "default_signature": null,
        "doc": null
      }
    ],
    "events": [],
    "slots": [],
    "models": [],
    "exposed": [],
    "fallthrough": null,
    "flags": {
      "async_setup": false,
      "has_inherit_attrs_false": false
    }
  }
}
```

## 4. `generic_substitution_via_typeof` — typeof substitution skipped

| Field          | Value                                                          |
|----------------|----------------------------------------------------------------|
| RULE CITATION  | TS spec §3.6 (generic substitution); CLAUDE.md "generic         |
|                | substitutions are part of semantic meaning".                    |
| EXPECTED       | `id: string` (after substituting `T → typeof sample.id`,        |
|                | which widens to `string`).                                      |
| ACTUAL         | `id: T` (free type parameter; no substitution performed).       |
| ROOT CAUSE     | Verter's resolver does not instantiate `IdShape<T>` with the    |
|                | `typeof sample.id` argument; T remains abstract.                |
| OWNER (later)  | Type-argument substitution path in macro resolver. Likely       |
|                | overlaps with the generic-instantiation work flagged in         |
|                | Phase 5 / Phase 7 of the cutover plan.                          |

## 5. `userland_shadowing_pick` — TS-first / userland shadow not honoured

| Field          | Value                                                          |
|----------------|----------------------------------------------------------------|
| RULE CITATION  | Verter rule `./.claude/skills/type-resolution` ("TS-first       |
|                | resolution priority"); CLAUDE.md §"Macro Type Traversal Rule"   |
|                | (single shared cross-file type resolver).                       |
| EXPECTED       | `props = [alpha, beta, gamma]` — the userland `Pick<T,_K> = T`  |
|                | shadows lib's `Pick`, returning the entire `Cfg` interface.     |
| ACTUAL         | `props = [alpha, beta]` — Verter dispatches to lib's `Pick`,    |
|                | filtering by the second type argument.                         |
| ROOT CAUSE     | The macro resolver does not perform an outward lexical-scope    |
|                | walk before falling back to ambient lib declarations. Userland  |
|                | type aliases of common utility names are silently overridden.   |
| OWNER (later)  | Resolver scope-walk policy. Likely Phase 5 / engine-retirement  |
|                | scope.                                                          |

### Rule-correct expected (machine-readable per §5.B.5.1 r15)

The JSON block below is the hand-authored rule-correct `SnapshotView`
for `userland_shadowing_pick`, derived from the Verter rule cited in
the table above (TS-first resolution priority + user shadowing wins).
The Phase 5h §5.B.5.1 rule-correctness gate test
(`deferred_fixture_userland_shadowing_pick_byte_equal_to_rule_correct_expected`
in
`crates/verter_session/src/component_meta_audit_rule_correctness_tests.rs`)
asserts byte-equality between this block and Verter's
post-Phase-5h-fix output. Discrimination: pre-fix Verter produces
`["alpha"]`, post-fix Verter produces `["alpha", "beta", "gamma"]`.

```json
{
  "fixture_id": "userland_shadowing_pick",
  "expected": {
    "component_name": "C",
    "props": [
      {
        "name": "alpha",
        "type_signature": "string",
        "required": true,
        "has_default": false,
        "default_signature": null,
        "doc": null
      },
      {
        "name": "beta",
        "type_signature": "number",
        "required": true,
        "has_default": false,
        "default_signature": null,
        "doc": null
      },
      {
        "name": "gamma",
        "type_signature": "boolean",
        "required": true,
        "has_default": false,
        "default_signature": null,
        "doc": null
      }
    ],
    "events": [],
    "slots": [],
    "models": [],
    "exposed": [],
    "fallthrough": null,
    "flags": {
      "async_setup": false,
      "has_inherit_attrs_false": false
    }
  }
}
```

---

## Summary

5 known defects committed as Phase 0a regression baselines. None
are blockers per the Phase 0 brief: the .snap.json files lock in
**non-drift** of Verter's current (incorrect) behaviour, ensuring
later refactors do not silently change the output further. Each
defect has a derivation note citing the violated rule and a
cross-reference to this file.

The Class A fixture set still satisfies §0p.A.0's "no self-confirming
snapshot" rule because every defect carries:
- a derivation note with TS-spec / Verter-rule citation explaining
  what SHOULD be there;
- a `KNOWN DEFECT` annotation in `expected.rs` with the same rule
  reference;
- an entry in this file with classification + owner suggestion.

Future phases that close these defects must follow the regen
recipe documented at the top of this file.
