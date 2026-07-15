/**
 * Declared-only projection helpers.
 *
 * These helpers project a fully-resolved native component-meta result down
 * to the declared-only surface that callers want from compat output.
 * There are two helpers because there are two distinct session types:
 *
 * - `projectDeclaredOnlyNativeResult` — for callers that already have a
 *   decoded `NativeComponentMetaResult` (the `ProjectSession.getComponentMeta`
 *   path inside `@verter/component-meta`).
 * - `projectDeclaredOnlyFromNativePayload` — for callers holding a raw NAPI
 *   `Buffer` payload (the public `@verter/native` `ComponentMetaSession`
 *   path). Decodes the buffer then delegates to the decoded helper.
 *
 * Both helpers return `NativeComponentMetaResult | null`. They never produce
 * a Volar shape — Volar mapping stays at the caller via
 * `nativeComponentMetaToComponentMeta + mapComponentMeta` exactly as before.
 *
 * Declared-only semantics:
 * - `props`, `events`, `slots`, `models`, `exposed`, and the rest of the
 *   per-symbol surface are preserved as-is.
 * - `acceptedProps` / `acceptedEvents` are filtered to keep only entries
 *   whose `provenance.kind === "declared"` (inherited entries are dropped).
 * - `fallthroughSurface` is reset to `{ kind: "none", reason: ... }` so
 *   downstream consumers see a no-fallthrough surface.
 */

import { decodeComponentMetaPayload } from "../type-graph.js";
import type {
  NativeComponentMetaResult,
  NativeAcceptedPropMeta,
  NativeAcceptedEventMeta,
  NativeFallthroughSurface,
  NativeNoFallthroughReason,
} from "../native-component-meta.js";

function deriveDeclaredOnlyFallthroughReason(
  surface: NativeFallthroughSurface,
): NativeNoFallthroughReason {
  if (surface.kind === "none") {
    return surface.reason;
  }
  return "noTemplate";
}

/**
 * Decoded helper: projects a fully resolved native component-meta result
 * down to its declared-only surface. Returns `null` for a `null` input.
 *
 * Internal callers (`@verter/component-meta` `ProjectSession`) use this
 * helper directly because their `getComponentMeta(...)` already decodes
 * the protobuf payload.
 */
export function projectDeclaredOnlyNativeResult(
  meta: NativeComponentMetaResult | null,
): NativeComponentMetaResult | null {
  if (meta == null) {
    return null;
  }

  const acceptedProps: NativeAcceptedPropMeta[] = meta.acceptedProps.filter(
    (entry) => entry.provenance.kind === "declared",
  );
  const acceptedEvents: NativeAcceptedEventMeta[] = meta.acceptedEvents.filter(
    (entry) => entry.provenance.kind === "declared",
  );
  const fallthroughSurface: NativeFallthroughSurface = {
    kind: "none",
    reason: deriveDeclaredOnlyFallthroughReason(meta.fallthroughSurface),
  };

  return {
    ...meta,
    acceptedProps,
    acceptedEvents,
    fallthroughSurface,
  };
}

/**
 * Buffer helper: decodes a raw NAPI `ComponentMetaSession.getComponentMeta`
 * payload then delegates to {@link projectDeclaredOnlyNativeResult}.
 * Returns `null` for a `null` payload.
 *
 * External raw-NAPI consumers (callers holding a `@verter/native`
 * `ComponentMetaSession` directly) use this helper to project the
 * canonical native payload to the declared-only surface they want from
 * compat output. See `docs/migration/` for the public migration guide.
 */
export function projectDeclaredOnlyFromNativePayload(
  payload: Buffer | null,
): NativeComponentMetaResult | null {
  if (payload == null) {
    return null;
  }
  return projectDeclaredOnlyNativeResult(decodeComponentMetaPayload(payload));
}
