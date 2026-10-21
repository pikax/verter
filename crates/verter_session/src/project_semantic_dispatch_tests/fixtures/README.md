# Vendored Redux Toolkit declarations

`rtk_slice.ts` is a trimmed excerpt of the type declarations of
[Redux Toolkit](https://github.com/reduxjs/redux-toolkit) 2.x
(`@reduxjs/toolkit`, the `createSlice` declarations and the action and
case-reducer types they read). Only the members a slice's action creators
read are kept; bodies are declarations only. The utility aliases at the top
of the file (`Exclude`, `Pick`, `Omit`, `Record`) restate TypeScript's own
library declarations so the fixture type-checks without a library.

Redux Toolkit is MIT licensed (see `LICENSE.md` in this directory). The
fixture is read by the `rtk_slice_*` tests in `../mapped_expand_tests.rs`
through `include_str!`; no Verter code path consumes it at runtime.
