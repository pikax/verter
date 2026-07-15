/**
 * Discriminating regression tests for the TS port of the
 * `PublishedSurfacePolicy` registry. Mirrors the Rust
 * `published_surface_tests.rs` discriminating contract — each test
 * FAILS if its structural rule is dropped or relaxed.
 *
 * The TS port and the Rust source-of-truth must agree on every
 * structural decision. If a divergence is detected between the
 * two implementations, that's a registry drift bug — fix the
 * source-of-truth (Rust) first, then re-port to TS.
 */

import { describe, it, expect } from "vitest";

import {
  type AnalyzedSurface,
  type AnalyzedSurfaceItem,
  COMPAT_BLOCKED_SLOT_NAMES,
  VUE_INTRINSIC_ATTR_NAMES,
  compatSlotSurvives,
  eventNameToOnPropName,
  namesForPolicy,
  refinedPropSurvives,
} from "./published-surface.js";

function item(name: string): AnalyzedSurfaceItem {
  return { name, declared_in_macro_type_arg: false, global: false };
}

function itemDeclared(name: string): AnalyzedSurfaceItem {
  return { name, declared_in_macro_type_arg: true, global: false };
}

function itemGlobal(name: string): AnalyzedSurfaceItem {
  return { name, declared_in_macro_type_arg: false, global: true };
}

describe("PublishedSurfacePolicy — TS port", () => {
  describe("constants", () => {
    it("COMPAT_BLOCKED_SLOT_NAMES matches the Rust source's list verbatim", () => {
      // Sync gate against the Rust `verter_audit::COMPAT_BLOCKED_SLOT_NAMES`
      // constant. Drift = registry-bug. Update both sides at once.
      expect(COMPAT_BLOCKED_SLOT_NAMES).toEqual([
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
      ]);
    });

    it("VUE_INTRINSIC_ATTR_NAMES matches the Rust source's list verbatim", () => {
      expect(VUE_INTRINSIC_ATTR_NAMES).toEqual(["class", "style", "key", "ref"]);
    });
  });

  describe("eventNameToOnPropName", () => {
    it("matches the bench refiner's prior camelCase derivation", () => {
      // These cases mirror the Rust
      // `event_name_to_on_prop_name_matches_bench_refiner_camelcase`
      // discriminating test. If TS and Rust disagree on any
      // case, the shadow-event-prop classifier mis-fires.
      expect(eventNameToOnPropName("submit")).toBe("onSubmit");
      expect(eventNameToOnPropName("click")).toBe("onClick");
      expect(eventNameToOnPropName("error")).toBe("onError");
      expect(eventNameToOnPropName("state-change")).toBe("onStateChange");
      expect(eventNameToOnPropName("update:modelValue")).toBe("onUpdateModelValue");
      expect(eventNameToOnPropName("update:open")).toBe("onUpdateOpen");
      expect(eventNameToOnPropName("foo_bar")).toBe("onFooBar");
      expect(eventNameToOnPropName("FOO")).toBe("onFOO");
    });
  });

  describe("namesForPolicy", () => {
    it("Native returns every name unfiltered (R19b regression gate)", () => {
      const surface: AnalyzedSurface = {
        props: [item("title"), item("class"), item("onSubmit"), itemGlobal("autofocus")],
        events: [item("submit")],
        slots: [item("default"), item("key")],
        exposed: [item("focus")],
      };
      const r = namesForPolicy("Native", surface);
      expect(r.props).toEqual(["title", "class", "onSubmit", "autofocus"]);
      expect(r.events).toEqual(["submit"]);
      expect(r.slots).toEqual(["default", "key"]);
      expect(r.exposed).toEqual(["focus"]);
    });

    it("Compat strips COMPAT_BLOCKED_SLOT_NAMES from slots only", () => {
      const surface: AnalyzedSurface = {
        props: [item("key"), item("title")],
        events: [],
        slots: [item("default"), ...COMPAT_BLOCKED_SLOT_NAMES.map(item)],
        exposed: [],
      };
      const r = namesForPolicy("Compat", surface);
      expect(r.props).toContain("key");
      expect(r.slots).toEqual(["default"]);
    });

    it("Compat and Refined never block author-declared slot names", () => {
      // Mirrors Rust `compat_policy_never_blocks_author_declared_slot_names`.
      const surface: AnalyzedSurface = {
        props: [],
        events: [],
        slots: [
          item("default"),
          itemDeclared("anchor"), // author-declared → survives
          itemDeclared("el"), // author-declared → survives
          item("anchor2"), // non-blocked name → survives
          item("placeholder"), // NOT declared → blocked
        ],
        exposed: [],
      };
      const compat = namesForPolicy("Compat", surface);
      expect(compat.slots).toEqual(["default", "anchor", "el", "anchor2"]);
      const refined = namesForPolicy("Refined", surface);
      expect(refined.slots).toEqual(compat.slots);
    });

    it("Refined retains declared onSubmit even when submit emit is declared", () => {
      // R19b's key regression case. The Rust counterpart is
      // `refined_policy_strips_on_event_shadow_props_when_not_declared_in_macro_type_arg`.
      const surface: AnalyzedSurface = {
        props: [item("title"), itemDeclared("onSubmit")],
        events: [item("submit")],
        slots: [],
        exposed: [],
      };
      const r = namesForPolicy("Refined", surface);
      expect(r.props).toContain("onSubmit");
    });

    it("Refined strips undeclared onSubmit when submit emit is declared", () => {
      const surface: AnalyzedSurface = {
        props: [item("title"), item("onSubmit")],
        events: [item("submit")],
        slots: [],
        exposed: [],
      };
      const r = namesForPolicy("Refined", surface);
      expect(r.props).not.toContain("onSubmit");
      expect(r.props).toContain("title");
    });

    it("Refined retains declared Vue intrinsics", () => {
      const surface: AnalyzedSurface = {
        props: [item("class"), itemDeclared("style"), item("title")],
        events: [],
        slots: [],
        exposed: [],
      };
      const r = namesForPolicy("Refined", surface);
      expect(r.props).not.toContain("class");
      expect(r.props).toContain("style");
      expect(r.props).toContain("title");
    });

    it("Refined strips producer-flagged global props", () => {
      const surface: AnalyzedSurface = {
        props: [item("title"), itemGlobal("autofocus")],
        events: [],
        slots: [],
        exposed: [],
      };
      const r = namesForPolicy("Refined", surface);
      expect(r.props).toContain("title");
      expect(r.props).not.toContain("autofocus");
    });
  });

  describe("refinedPropSurvives — direct predicate", () => {
    it("returns false for global props", () => {
      expect(refinedPropSurvives(itemGlobal("autofocus"), [])).toBe(false);
    });
    it("returns false for shadow-event-prop name when not declared", () => {
      expect(refinedPropSurvives(item("onSubmit"), ["submit"])).toBe(false);
    });
    it("returns true for shadow-event-prop name when declared", () => {
      expect(refinedPropSurvives(itemDeclared("onSubmit"), ["submit"])).toBe(true);
    });
    it("returns false for intrinsic name when not declared", () => {
      expect(refinedPropSurvives(item("class"), [])).toBe(false);
    });
    it("returns true for intrinsic name when declared", () => {
      expect(refinedPropSurvives(itemDeclared("class"), [])).toBe(true);
    });
    it("returns true for ordinary props", () => {
      expect(refinedPropSurvives(item("title"), [])).toBe(true);
    });
  });

  describe("compatSlotSurvives", () => {
    it("returns false for blocked slot names", () => {
      for (const blocked of COMPAT_BLOCKED_SLOT_NAMES) {
        expect(compatSlotSurvives(blocked)).toBe(false);
      }
    });
    it("returns true for non-blocked slot names", () => {
      expect(compatSlotSurvives("default")).toBe(true);
      expect(compatSlotSurvives("body")).toBe(true);
    });
    it("never blocks an author-declared slot, whatever its name", () => {
      // Mirrors Rust `compat_policy_never_blocks_author_declared_slot_names`:
      // the block is a structural condition on `declared_in_macro_type_arg`,
      // not bare name-set membership (Popover.vue's declared `anchor` slot —
      // vue-component-meta publishes it too).
      for (const blocked of COMPAT_BLOCKED_SLOT_NAMES) {
        expect(compatSlotSurvives(blocked, true)).toBe(true);
      }
      // Undeclared stays blocked; the explicit-false form matches the default.
      expect(compatSlotSurvives("anchor", false)).toBe(false);
    });
  });
});
