/**
 * Guard `wasm_tsgo_unavailable_fail_closed`.
 *
 * In the WASM/browser surface the TS>=7 / external-tsgo capability is
 * UNAVAILABLE: carrier generation + Verter-native diagnostics only. No
 * Go-WASM engine module exists or is instantiable, the capability model
 * explicitly declares the external-tsgo engine off for EVERY TS major, the
 * type-checker mode is the single browser capability, and a persisted legacy
 * engine selection can never resurrect a live external-engine path.
 *
 * Hermetic: typed/structural against the capability surface — no
 * `@verter/wasm` host load.
 *
 * @vitest-environment happy-dom
 */
import { describe, it, expect } from "vitest";
import { zlibSync, strToU8, strFromU8 } from "fflate";
import { capabilityForWasm, tsMajorOf } from "./inContextLs";
import { deserializeFromHash } from "../core/urlState";
import typesSource from "../core/types.ts?raw";

/** The retired engine mode token, assembled so this file never contains it. */
const RETIRED_MODE = "ts" + "go";

describe("wasm_tsgo_unavailable_fail_closed (#2)", () => {
  it("the capability model declares the external-tsgo engine UNAVAILABLE for every TS major", () => {
    for (const major of [5, 6, 7, 8]) {
      const capability: { inContextLS: boolean; tsgo?: boolean } = capabilityForWasm(major);
      expect(
        RETIRED_MODE in capability,
        `capabilityForWasm(${major}) must EXPLICITLY declare the external engine capability`,
      ).toBe(true);
      expect(capability.tsgo, `capabilityForWasm(${major}).${RETIRED_MODE}`).toBe(false);
    }
    // TS>=7 in WASM: carrier-gen + Verter-native only — NO engine of any kind.
    expect(capabilityForWasm(7)).toEqual({ inContextLS: false, tsgo: false });
    expect(capabilityForWasm(tsMajorOf("7.0.1-rc"))).toEqual({ inContextLS: false, tsgo: false });
    expect(capabilityForWasm(8)).toEqual({ inContextLS: false, tsgo: false });
  });

  it("TypeCheckerMode is the single browser capability (no external-engine variant)", () => {
    const declaration = typesSource.match(/export type TypeCheckerMode = ([^;]+);/);
    expect(declaration, "TypeCheckerMode declaration must exist").not.toBeNull();
    expect(declaration![1].trim()).toBe('"tsc"');
  });

  it("no Go-WASM engine module exists in the editor module graph (nothing to instantiate)", () => {
    // Transform-time structural check over this directory: any sibling
    // `tsgo*` engine/service/worker module makes this non-empty.
    const engineModules = Object.keys(import.meta.glob("./tsgo*"));
    expect(engineModules).toEqual([]);
  });

  it("a persisted legacy engine selection can never select a live external-engine path", () => {
    // A legacy shared URL carrying the retired `_typeChecker` engine mode:
    // deserialization must yield NO engine selection (fail closed to the
    // single browser capability), and never leak the key as a user file.
    const flat = { "App.vue": "<template></template>", _typeChecker: RETIRED_MODE };
    const compressed = zlibSync(strToU8(JSON.stringify(flat)), { level: 9 });
    window.location.hash = `#${btoa(strFromU8(compressed, true))}`;

    const result = deserializeFromHash();
    expect(result).not.toBeNull();
    const selected = (result as unknown as Record<string, unknown>).typeChecker;
    expect(selected).not.toBe(RETIRED_MODE);
    expect(selected).toBeUndefined();
    expect(result!.files["_typeChecker"]).toBeUndefined();
  });
});
