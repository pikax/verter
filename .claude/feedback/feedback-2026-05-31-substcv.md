# Prepared-substitution slow-lane elimination — feedback log

## 2026-05-31

- [debt] `crates/verter_session/src/meta_resolve/dispatch_helpers.rs:670-712` — doc comments reference the deleted helpers `build_default_type_param_substitutions` / `apply_type_param_substitutions` by name in prose. These are not RETIRED_SYMBOLS-gated (the gate strips comments) but become stale references after deletion. Should update the comment to point at the dispatch equivalent.
