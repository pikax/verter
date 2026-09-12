// Public optionality contract for `NativeComponentMetaResult['origin']`.
//
// The origin graph is audit-only: the field is ABSENT from the payload when the
// host is not configured for audit, so consumers must be able to handle
// `meta.origin === undefined` and must be able to build a result without it.
// Dropping the `?` would be a silent breaking change for every such consumer.
//
// Checked by `tsc --noEmit -p tsconfig.contract-tests.json` — vitest does not
// evaluate type-level assertions.

import type { NativeComponentMetaResult, NativeOriginGraph } from "../src/native-component-meta.js";

type OriginField = NativeComponentMetaResult["origin"];
type Expected = NativeOriginGraph | undefined;

// Bidirectional assignability proves structural equality.
const _check_origin_assignable_to_expected: Expected = null as unknown as OriginField;
const _check_expected_assignable_to_origin: OriginField = null as unknown as Expected;

// Constructing a NativeComponentMetaResult literal MUST be possible without
// providing `origin`. If `origin` becomes required, this assignment
// fails to compile.
declare const _meta_without_origin: Omit<NativeComponentMetaResult, "origin">;
const _accept_partial_assignment: NativeComponentMetaResult = {
  ..._meta_without_origin,
};

// Silence unused-variable warnings (file is type-only, no runtime code).
void _check_origin_assignable_to_expected;
void _check_expected_assignable_to_origin;
void _accept_partial_assignment;
