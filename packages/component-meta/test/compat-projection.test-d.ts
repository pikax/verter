// Public type contract of the compat declared-only projection helpers.
//
// Checked by `tsc --noEmit -p tsconfig.contract-tests.json`. Vitest does not
// evaluate type-level assertions, and the package build excludes spec files, so
// this file is the only thing that fails when a helper signature widens.
//
// Both subjects are asserted: the SOURCE barrel (the contract as authored) and
// the BUILT declaration published under the `./compat` export map entry (the
// contract as consumers receive it). The dist import is required — a missing
// build fails this check rather than skipping it. The two subjects are checked
// against separately-derived expected types on purpose: `src` and `dist` each
// declare their own `unique symbol` graph brand, so the two module graphs are
// deliberately not mutually assignable.

import type * as SourceCompat from "../src/compat/index.js";
import type * as PublishedCompat from "../dist/compat/index.js";
import type { NativeComponentMetaResult } from "../src/native-component-meta.js";

/**
 * Exact type identity, so a widening to `any` fails instead of quietly
 * satisfying assignability in both directions.
 */
type Equals<A, B> =
  (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2 ? true : false;

// ── Source contract ──────────────────────────────────────────────
// Both helpers project a native result down to a native result — never a Volar
// shape — and both null-pass.

const _source_result_helper: Equals<
  typeof SourceCompat.projectDeclaredOnlyNativeResult,
  (meta: NativeComponentMetaResult | null) => NativeComponentMetaResult | null
> = true;

const _source_payload_helper: Equals<
  typeof SourceCompat.projectDeclaredOnlyFromNativePayload,
  (payload: Buffer | null) => NativeComponentMetaResult | null
> = true;

// ── Published contract ───────────────────────────────────────────

type PublishedResult = NonNullable<
  ReturnType<typeof PublishedCompat.projectDeclaredOnlyNativeResult>
>;

const _published_result_helper: Equals<
  typeof PublishedCompat.projectDeclaredOnlyNativeResult,
  (meta: PublishedResult | null) => PublishedResult | null
> = true;

const _published_payload_helper: Equals<
  typeof PublishedCompat.projectDeclaredOnlyFromNativePayload,
  (payload: Buffer | null) => PublishedResult | null
> = true;

// The published helpers return the NATIVE result: `fallthroughSurface` and
// `acceptedProps` are native-only fields, so a helper that started returning
// the Volar `ComponentMeta` shape fails here.
const _published_result_is_native_shaped: PublishedResult extends {
  filePath: string;
  fallthroughSurface: unknown;
  acceptedProps: unknown;
}
  ? true
  : false = true;

void _source_result_helper;
void _source_payload_helper;
void _published_result_helper;
void _published_payload_helper;
void _published_result_is_native_shaped;
