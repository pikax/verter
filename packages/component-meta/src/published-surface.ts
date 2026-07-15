/**
 * `@verter/component-meta/published-surface` — TS port of the
 * `verter_audit::PublishedSurfacePolicy` registry.
 *
 * One TS implementation shared by `@verter/component-meta/compat`
 * (which consumes the `Compat` projection's slot blocklist) and
 * `@verter/benchmark` (which consumes the `Refined` projection's
 * shadow-event-prop + intrinsic + global filters). The Rust
 * registry at `crates/verter_audit/src/published_surface.rs` is the
 * canonical source of truth; this port mirrors its structural
 * decisions exactly. The Rust integration test
 * `published_surface_constants_match_ts_port` (in
 * `crates/verter_audit/tests/`) parses this file and fails on
 * drift in either constant (`COMPAT_BLOCKED_SLOT_NAMES`,
 * `VUE_INTRINSIC_ATTR_NAMES`), and the companion
 * `event_name_to_on_prop_name_matches_ts_port_fixed_cases` test
 * pins the camelCase derivation against the same payload table
 * `published-surface.spec.ts` asserts on the TS side.
 *
 * No name-string heuristics; no thresholds; no ratios. Every
 * projection decision is driven by per-name structural facts on
 * `AnalyzedSurfaceItem` (`declared_in_macro_type_arg`, `global`)
 * and by the structural fingerprint of the declared emits.
 */

import type {
  AnalyzedSurface,
  AnalyzedSurfaceItem,
  PolicyNamesResult,
  PublishedSurfacePolicy,
} from "@verter/types/audit.generated";

export type { AnalyzedSurface, AnalyzedSurfaceItem, PolicyNamesResult, PublishedSurfacePolicy };

/**
 * VNode-only transport keys suppressed on the slots surface when they
 * reach it WITHOUT an authored declaration
 * (`declared_in_macro_type_arg === false`) — an author-declared slot is
 * never blocked, whatever its name. Mirror of Rust
 * `verter_audit::COMPAT_BLOCKED_SLOT_NAMES`.
 */
export const COMPAT_BLOCKED_SLOT_NAMES: readonly string[] = [
  "type",
  "props",
  "key",
  "ref",
  "scopeId",
  "children",
  "component",
  "dirs",
  "transition",
  "el",
  "placeholder",
  "anchor",
  "target",
  "targetStart",
  "targetAnchor",
  "suspense",
  "shapeFlag",
  "patchFlag",
  "appContext",
] as const;

/**
 * Vue intrinsic attribute names — always merged through
 * fallthrough on the runtime and never published on the
 * consumer-facing macro surface UNLESS the SFC author explicitly
 * re-declared the name in the macro's type argument. Mirror of
 * Rust `verter_audit::VUE_INTRINSIC_ATTR_NAMES`.
 */
export const VUE_INTRINSIC_ATTR_NAMES: readonly string[] = [
  "class",
  "style",
  "key",
  "ref",
] as const;

/**
 * Convert an emit name to its `on{Event}` (camelCase) prop name
 * equivalent — the structural shadow form the `Refined` policy
 * filters.
 *
 * Mirror of Rust `verter_audit::event_name_to_on_prop_name`.
 *
 * Algorithm: `"on_" + event_name`, then camelCase via:
 *   1. Strip leading non-alphanumerics.
 *   2. Collapse runs of non-alphanumeric followed by an
 *      alphanumeric, uppercasing the alphanumeric.
 *   3. Lowercase the first character of the result if uppercase.
 */
export function eventNameToOnPropName(eventName: string): string {
  return camelCase(`on_${eventName}`);
}

function camelCase(input: string): string {
  return input
    .replace(/^[^a-zA-Z0-9]+/, "")
    .replace(/[^a-zA-Z0-9]+([a-zA-Z0-9])/g, (_match, char: string) => char.toUpperCase())
    .replace(/^[A-Z]/, (char) => char.toLowerCase());
}

/**
 * Apply the projection policy and return the names that survive.
 *
 * Mirror of Rust `verter_audit::names_for_policy`. All projection
 * decisions are structural (driven by `AnalyzedSurfaceItem` facts).
 */
export function namesForPolicy(
  policy: PublishedSurfacePolicy,
  surface: AnalyzedSurface,
): PolicyNamesResult {
  switch (policy) {
    case "Native":
      return {
        props: surface.props.map((i) => i.name),
        events: surface.events.map((i) => i.name),
        slots: surface.slots.map((i) => i.name),
        exposed: surface.exposed.map((i) => i.name),
      };
    case "Compat": {
      const blocked = new Set<string>(COMPAT_BLOCKED_SLOT_NAMES);
      return {
        props: surface.props.map((i) => i.name),
        events: surface.events.map((i) => i.name),
        // Structural block: a VNode-transport NAME is suppressed only
        // when the author did NOT declare the slot on the component's
        // own macro surface (Popover.vue's declared `anchor` slot
        // always survives — `vue-component-meta` publishes it too).
        slots: surface.slots
          .filter((s) => s.declared_in_macro_type_arg || !blocked.has(s.name))
          .map((s) => s.name),
        exposed: surface.exposed.map((i) => i.name),
      };
    }
    case "Refined": {
      const shadowEventProps = new Set<string>(
        surface.events.map((e) => eventNameToOnPropName(e.name)),
      );
      const intrinsics = new Set<string>(VUE_INTRINSIC_ATTR_NAMES);
      const blocked = new Set<string>(COMPAT_BLOCKED_SLOT_NAMES);
      return {
        props: surface.props
          .filter((p) => {
            if (p.global) {
              return false;
            }
            if (shadowEventProps.has(p.name) && !p.declared_in_macro_type_arg) {
              return false;
            }
            if (intrinsics.has(p.name) && !p.declared_in_macro_type_arg) {
              return false;
            }
            return true;
          })
          .map((p) => p.name),
        events: surface.events.map((i) => i.name),
        // Same declared-slot exemption as `Compat`.
        slots: surface.slots
          .filter((s) => s.declared_in_macro_type_arg || !blocked.has(s.name))
          .map((s) => s.name),
        exposed: surface.exposed.map((i) => i.name),
      };
    }
  }
}

/**
 * Helper: predicate filter for the `Refined` policy's prop pass.
 *
 * Returns `true` iff the prop survives the `Refined` projection
 * given its per-name facts and the set of declared emits. Useful
 * for direct array.filter use cases where a full
 * `AnalyzedSurface` round-trip is not needed.
 *
 * `eventNames` is the set of declared `defineEmits` names (NOT
 * the `on{Event}` shadow form — this helper computes the shadow
 * form internally).
 */
export function refinedPropSurvives(
  prop: AnalyzedSurfaceItem,
  eventNames: readonly string[],
): boolean {
  if (prop.global) {
    return false;
  }
  const shadowSet = new Set<string>(eventNames.map(eventNameToOnPropName));
  if (shadowSet.has(prop.name) && !prop.declared_in_macro_type_arg) {
    return false;
  }
  if (VUE_INTRINSIC_ATTR_NAMES.includes(prop.name) && !prop.declared_in_macro_type_arg) {
    return false;
  }
  return true;
}

/**
 * Helper: predicate filter for the `Compat` policy's slot pass.
 *
 * Returns `true` iff the slot survives the `Compat` projection —
 * either the author declared the slot on the component's own macro
 * surface (`declaredInMacroTypeArg`, the structural exemption: an
 * author-declared slot is never blocked, whatever its name) or
 * `slotName` is not in `COMPAT_BLOCKED_SLOT_NAMES`. Callers without
 * the producer fact fall back to `false` (conservative: the name
 * block applies). Useful for direct array.filter use cases.
 */
export function compatSlotSurvives(slotName: string, declaredInMacroTypeArg = false): boolean {
  return declaredInMacroTypeArg || !COMPAT_BLOCKED_SLOT_NAMES.includes(slotName);
}
