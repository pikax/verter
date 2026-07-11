# Svelte CSS-scoping coverage summary

Derived review artifact. The typed Rust manifest (`crates/verter_svelte_conformance`) is the sole authority; regenerate this file and the fixture corpus with `cargo run -p verter_svelte_conformance -- write`.

- schema version: 1
- manifest hash: `fnv1a64-527c96f87a2bfda7`
- selected cases: 518 of 272160 candidate rows (0.19%)
- fixtures: `fixtures/<slug>.svelte`, one per case; each case compiles on both backends (`client`, `server`)

## Selected cases by partition

| partition | cases |
| --- | ---: |
| oracle-rejected:css-nesting-selector-invalid-placement | 1 |
| refused:legacy-slot-scope-unprovable | 1 |
| supported | 516 |

## Full candidate space by partition

| partition | kind | rows |
| --- | --- | ---: |
| supported | — | 31890 |
| refused | legacy-slot-scope-unprovable | 6458 |
| oracle-rejected | css-nesting-selector-invalid-placement | 4320 |
| invalid | boolean-quoting-on-valued-target | 60480 |
| invalid | boolean-quoting-carries-no-value | 25920 |
| invalid | spread-carries-no-quoting | 12960 |
| invalid | selector-escape-on-valueless-selector | 65340 |
| invalid | universal-selector-always-matches | 4800 |
| invalid | global-selector-never-prunes | 13368 |
| invalid | selector-cannot-read-target | 19200 |
| invalid | attr-selector-value-escape-never-matches | 2340 |
| invalid | maybe-needs-uncertain-source | 21988 |
| invalid | spread-outcome-always-uncertain | 2376 |
| invalid | svelte-element-tag-uncertain | 720 |

## Representation axes (selected cases: template spelling × selector spelling)

| template \ selector | `lit` | `eschex` | `eschar` | `escmix` |
| --- | ---: | ---: | ---: | ---: |
| `lit` | 81 | 9 | 9 | 9 |
| `named` | 49 | 6 | 6 | 6 |
| `dec` | 48 | 5 | 6 | 7 |
| `hex` | 48 | 7 | 5 | 6 |
| `mixent` | 48 | 6 | 6 | 6 |
| `dyn` | 63 | 10 | 10 | 10 |
| `spread` | 37 | 5 | 5 | 5 |

## Strengthened interaction groups

- group 0 (strength 5): template-value × target × quoting × element-region × match-outcome
- group 1 (strength 4): selector-kind × selector-value × structural-kind × match-outcome

## Covering-array proof

```
covering-array proof
candidates: 272160
selected-rows: 518
global: required=5194 covered=5194
group[0]: required=473 covered=473
group[1]: required=163 covered=163
focus-cells: 38
refusal-partitions: 2
```
