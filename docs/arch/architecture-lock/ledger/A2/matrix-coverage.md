# A2 — crossed capture-write matrix: coverage statement

Mechanism: `crates/verter_session/src/u6_flow_expect_tests.rs` → `matrix` module.
One shared program generator (`CAPTURE_WRITE` + `iife_write_statement` +
`iife_position_program`) composes every position-cell program, so a position cannot
drift from its siblings. Cells are measured through the PUBLIC audited boundary
(`get_flow_return_type_with_audit`, twice) and pin: recursive rendering, typed
degradation, second-call cache-replay state, or a typed no-value refusal. Every cell
records the pinned tsgo checker answer (measured, raw outputs in
`command-proofs/tsgo-probes/`).

Key §7 assertion — `same_capture_write_cell_is_position_independent`: the same
capture-write cell must measure identically at every covered expression position,
asserted from LIVE measurement. Because the current tree IS position-dependent, the
cell is pinned `PositionDependent` (a recorded expected-versus-actual gap): the test
asserts live within-group uniformity AND live between-group divergence, so a fix at
one position, a regression at one position, or the full fix landing without a re-pin
each fails loudly. Once re-pinned `Uniform`, the assertion becomes the unconditional
position-independence fence.

## Cells covered (axes: binding × write-timing × depth × position × guard × container)

### Position axis (1 cell × 8 positions), checker `"b"` at EVERY position
`let_iife_write_positions` — binding=Let, timing=InsideInvokedIife, depth=1,
guard=None, container=None. MEASURED (parent tree): position-DEPENDENT —
- statement, sequence_operand, call_argument → typed NoValue refusal
- declarator_init, if_test, template, short_circuit, object_literal → stale
  pre-write `"a"`, degradation None, WARM (the wrong-and-warm G4/G5 class)
Pinned per-position; owner `U6.LOOP_CLOSURE`.

### Write-timing axis (binding=Let, depth=1, guard=None, container=None)
| cell | checker | measured (pinned) | gap |
|---|---|---|---|
| `let_write_before_creation` | `() => "b"` | `() => "b"` clean warm | none |
| `let_write_after_creation` | `() => "a" \| "b"` | `() => "a"` clean warm | G6, wrong-and-warm |
| `let_sibling_closure_write` | `() => "a" \| "b"` | `() => "a"` clean warm | G7, wrong-and-warm |
| `let_deeper_closure_write` (depth=2) | `() => "a" \| "b"` | `() => "a"` clean warm | G7 depth-2, wrong-and-warm |
| (timing=InsideReturnedClosure ≙ corpus row X85; timing=Never ≙ corpus row X87) | | | |

### Binding axis (timing=AfterCreation / Never)
| cell | checker | measured (pinned) | gap |
|---|---|---|---|
| `var_write_after_creation` | `() => "a" \| "b"` | `() => Union("a" \| "b")` clean warm | none |
| `param_write_after_creation` | `() => "a" \| "b"` | `() => Union("a" \| "b")` clean warm | none |
| `const_capture_never_written` | `() => "a"` | `() => "a"` clean warm | none |

### Guard axis
| cell | checker | measured (pinned) | gap |
|---|---|---|---|
| `typeof_guard_before_creation` (Param, container=If) | `() => string` | `Union(() => Union(string \| number) \| () => "z")` clean warm | G9: pre-creation typeof guard not applied to the capture; union not subtype-collapsed; wrong-and-warm |

### Completion-container axis (invoked-IIFE write, binding=Let)
| cell | checker | measured (pinned) | gap |
|---|---|---|---|
| `iife_write_in_try_finally` | `"b"` | NoValue refusal | honest cold refusal |
| `iife_write_in_labeled_block` | `"b"` | NoValue refusal | honest cold refusal |
| `iife_write_in_if_branch` | `"a" \| "b"` | NoValue refusal | honest cold refusal |
| `iife_write_in_switch_case` | `"a" \| "b"` | NoValue refusal | honest cold refusal |

## Cells NOT covered (named gaps — left to the closure owner)

§7 of `docs/arch/u6-flow-return-gaps-and-target.md` prescribes the FULL crossed matrix
("closure work is accepted only against a crossed matrix, not a case list") as the
acceptance bar for the closure work itself; this block seeds the matrix mechanism and
the cells above, and leaves the remaining cross product to that owner. (An earlier
revision of this file attributed the phrase "full cross product is later closure work"
to §7 as a quotation; §7 contains no such sentence — the deferral is this block's
scoping decision, corrected here.)

- binding kinds: destructured, destructured-with-default, catch-clause, loop bindings
  (corpus rows X86/X78/X79 cover adjacent shapes on the corpus lanes)
- expression positions: return-expression, default-parameter initializer, class field
  initializer (the `ExprPosition` vocabulary is deliberately total-ready; class-field
  `any` publication is a separately-recorded ledger entry)
- write timings crossed with non-None guards (guard × timing product)
- containers: catch, finally-block writes, loop bodies (X81/X82 cover adjacent loop
  shapes); container × position product beyond the four covered containers
- closure depth ≥ 3

The uncovered axes remain expressible without touching the drivers: a new cell is one
`FixedCell`/`PositionCell` literal plus, for a genuinely new position, one
`iife_write_statement` arm.
