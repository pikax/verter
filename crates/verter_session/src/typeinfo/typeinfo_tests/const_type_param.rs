//! @ai-generated - `const` type-parameter contracts (TS7).
//!
//! TS7 behaviour for `<const T extends …>`: literal and tuple values passed at
//! call sites are preserved as readonly literals at the type level, without an
//! explicit `as const` at the use site. Both rows are `#[oracle_row]` lifts —
//! seated in the `ORACLE_QUERY_SPECS` registry (which owns the workspace-file
//! set and its vendored source bytes) and compared against a checked-in tsgo
//! snapshot through the shared driver.

use super::oracle;
use verter_session_oracle_macro::oracle_row;

// TS7 contract: `makeRoute([{ path: "/home" }, { path: "/about" }])` with
// `<const T extends readonly { path: string }[]>` infers T as the
// readonly tuple of two readonly object literals:
//   readonly [{ readonly path: "/home" }, { readonly path: "/about" }]
#[oracle_row]
#[test]
fn const_type_param_route_call_preserves_readonly_tuple_with_literal_paths() {}

// TS7 contract: `makeStrings(["a", "b", "c"])` with `<const T extends
// readonly string[]>` infers T as `readonly ["a", "b", "c"]` — readonly
// tuple of the literal strings, even without an explicit `as const` on
// the call-site array.
#[oracle_row]
#[test]
fn const_type_param_string_call_preserves_readonly_literal_string_tuple() {}
