# Svelte CSS-scoping coverage summary

Derived review artifact. The typed Rust manifest (`crates/verter_svelte_conformance`) is the sole authority; regenerate this file and the fixture corpus with `cargo run -p verter_svelte_conformance -- write`.

- schema version: 3
- manifest hash: `fnv1a64-0a8b7b90c5b3bbf8`
- selected cases: 609 of 272160 candidate rows (0.22%)
- fixtures: `fixtures/<slug>.svelte`, one per case; each case compiles on both backends (`client`, `server`)

## Selected cases by partition

| partition | cases |
| --- | ---: |
| oracle-rejected:css-nesting-selector-invalid-placement | 1 |
| supported | 608 |

## Full candidate space by partition

| partition | kind | rows |
| --- | --- | ---: |
| supported | — | 38348 |
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
| `lit` | 99 | 9 | 9 | 9 |
| `named` | 57 | 7 | 7 | 7 |
| `dec` | 57 | 6 | 7 | 8 |
| `hex` | 57 | 7 | 8 | 6 |
| `mixent` | 59 | 6 | 6 | 7 |
| `dyn` | 77 | 11 | 10 | 10 |
| `spread` | 44 | 7 | 6 | 6 |

## Strengthened interaction groups

- group 0 (strength 5): template-value × target × quoting × element-region × match-outcome
- group 1 (strength 4): selector-kind × selector-value × structural-kind × match-outcome

## Covering-array proof

```
covering-array proof
candidates: 272160
selected-rows: 609
global: required=5628 covered=5628
group[0]: required=562 covered=562
group[1]: required=163 covered=163
focus-cells: 39
refusal-partitions: 1
```
