# Conditional-chain narrowing guard inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## verter_compiler

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `ChainMember::terms`: one chain's member conditions, resolved and wrapped once, shared by every member; a member reads its predecessors as a prefix | IDE narrowing guards (`v-if` IIFE block guards, callback ternary/block guards) | REQUIRED | One parent's children walk; dropped with the last scope referencing it | `src/ide/condition.rs` | always |
| `GuardScope` / `GuardFrame::parent`: persistent list of the chain members enclosing a template position, linked instead of copied | Nested `v-if` IIFE guards and callback guards for every element in the scope | REQUIRED | One IDE template emission | `src/ide/condition.rs` | always |
| `GuardFrame::guard`: the scope's bounded guard text (at most `MAX_GUARD_TERMS` terms), rendered once and shared | Every element and callback inside the scope | REQUIRED | Same as the owning frame | `src/ide/condition.rs` | always |
| `ResolvedBranch`: a chain member's mapped condition plan plus its narrowing terms, from one resolution | Emitted `if`/`else if`/ternary test and the guards repeating it | REQUIRED | One parent's children walk | `src/ide/template/mod.rs` | always |
| `GUARD_WORK` thread-local counter, `take_guard_work` reader and `record_guard_work` hook: condition terms visited while rendering guards | none; tests and measurement builds only | OPTIONAL | Per thread; the reader resets it | `src/ide/condition.rs` | `cfg(any(test, feature = "semantic-observe"))` |

Default builds compile `record_guard_work` to an empty inline function; there is
no per-operation enabled check.
