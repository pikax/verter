# Generic binder environment inventory

Classification follows [the observation policy](../../../docs/arch/semantic-observe.md).

## `verter_type_expr_oxc`

| item (field, store, hook, counter, trace) | production consumer | classification | lifetime | owner module | gate |
| --- | --- | --- | --- | --- | --- |
| `BinderEnv` binder stack (live type parameters in introduction order) | Type-parameter reference binding during lowering: which declaration a generic name denotes | REQUIRED | One normalization of one generic signature; each nested scope releases its binders when its body is rebuilt | `src/binder_env.rs` | always |
| `BinderEnv` name index (per spelling, ascending stack positions) | Same binding decision: outermost visible, first-introduced binder of a name | REQUIRED | Same as the stack; a spelling's entry is removed when its last live binder is released | `src/binder_env.rs` | always |
| Visibility floor and scope start depth carried by normalization frames | Forward/enclosing visibility of constraints and defaults versus signature bodies | REQUIRED | One normalization walk's explicit frame stack | `src/lib.rs` (`normalize_in_env`) | always |
| Copied per-scope prefix lists and the linked scope chain they formed | None; replaced by the indexed environment | REQUIRED, retired | No population or backing allocation | `src/lib.rs` | absent |
| `BinderWork` introduction, lookup and examined-position counters | none; tests bounding work growth only | OPTIONAL | Thread-local per test thread; read as before/after deltas | `src/binder_env.rs` | `cfg(test)` |

The counters have no reader outside this crate's unit tests, so they are
compiled only into its test build; no default or instrumented production build
carries them, and no per-operation enabled check is introduced.
