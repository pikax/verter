# Vendored Redux Toolkit declarations

`rtk_slice.ts` is a trimmed excerpt of the type declarations of
[Redux Toolkit](https://github.com/reduxjs/redux-toolkit) 2.x
(`@reduxjs/toolkit`, the `createSlice` declarations and the action and
case-reducer types they read). Only the members a slice's action creators
read are kept; bodies are declarations only. The utility aliases at the top
of the file (`Exclude`, `Pick`, `Omit`, `Record`) restate TypeScript's own
library declarations. `ReturnType` and `Parameters` come from the benchmark
library (`lib.bench.d.ts`) that the `rtk_slice_*` tests load under `noLib`.

Redux Toolkit is MIT licensed (see `LICENSE.md` in this directory). The
fixture is read by the `rtk_slice_*` tests in `../mapped_expand_tests.rs`
through `include_str!`; no Verter code path consumes it at runtime.

# Reconstructed Pinia options store

`pinia_options_store.ts` is a reconstruction of a Pinia options store, not a
copy of an original benchmark scenario. Its declarations restate the type
declarations of [Pinia](https://github.com/vuejs/pinia) 3.0.4 (`defineStore`'s
options overload, `Store`, `_StoreWithState`, `_StoreWithGetters`,
`_GettersTree`, `DefineStoreOptions`, `StoreDefinition`) and Vue 3.5's
`UnwrapRef` (`@vue/reactivity`), trimmed to the members an options store reads;
the store itself (state, getters `double` and `first`, actions `increment` and
`add`, id `'counter'`) is authored for the tests. Pinia and Vue are MIT
licensed. The fixture is read by `../pinia_options_store_tests.rs` through
`include_str!`; the global `ThisType` marker is supplied per test.
