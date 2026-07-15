// F1 (Plan §3 Step 3 Test 3) — public-type optionality regression invariant.
//
// Asserts that `NativeComponentMetaResult['origin']` is `NativeOriginGraph | undefined`
// (not required). The audit-only contract D34 means the field is absent from
// the payload when the host is not configured for audit; consumers MUST be
// able to handle `meta.origin === undefined`. Pre-fix this was already
// optional in the type; post-fix it must STAY optional.
//
// This is a `.test-d.ts` file — vitest's default include glob matches
// `**/*.test.ts` only, so this file does not run as a test (no test bodies).
// Its job is to be type-checked by `tsc --noEmit` / `pnpm tsc` so any
// future change that drops the `?` on `origin` breaks the build.
//
// To force-run a type-check on this file in CI, add it to a tsconfig
// includes glob or run `pnpm tsc --noEmit packages/component-meta/test/*.test-d.ts`.

import type { NativeComponentMetaResult, NativeOriginGraph } from "../src/native-component-meta.js";

// Type-level assertion: `origin` must be optional. The `?` on the field
// declaration makes the property type `NativeOriginGraph | undefined`.
// If a future refactor drops the `?` and makes it required, the
// assignability check below fails to compile.

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
