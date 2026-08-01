import { describe, expect, it } from "vitest";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import {
  DEFAULT_SERVER_PROFILE,
  E2E_SERVER_PROFILES,
  isE2eServerProfile,
  serverProfileKeys,
  serverProfileSettings,
} from "./serverProfiles";

const PACKAGE_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

describe("E2E server profiles", () => {
  it("ships the default profile with the native lane OFF", () => {
    expect(DEFAULT_SERVER_PROFILE).toBe("default");
    expect(serverProfileSettings("default")).toEqual({
      "verter.analysis.enabled": false,
      "verter.hover.nativeSemantics": false,
    });
  });

  it("agrees with the DEFAULTS the extension actually contributes", () => {
    // The point of the `default` profile is to be the shipped default. If the
    // product flips one of these to `true`, the profile must move with it —
    // otherwise the E2E suite goes on measuring a configuration no user has.
    const manifest = JSON.parse(
      fs.readFileSync(path.join(PACKAGE_ROOT, "package.json"), "utf8"),
    ) as {
      contributes: { configuration: { properties: Record<string, { default?: unknown }> } };
    };
    const contributed = manifest.contributes.configuration.properties;
    for (const key of serverProfileKeys()) {
      expect(contributed[key], `${key} must be a contributed setting`).toBeDefined();
      expect(contributed[key].default, `${key} default`).toBe(E2E_SERVER_PROFILES.default[key]);
    }
  });

  it("turns the whole native lane on together, never half of it", () => {
    // Native hover needs the hover flag AND the analysis snapshot: with only the
    // first, `FileAnalysisSnapshot.template` is `None` and every markup-side
    // native hover has nothing to resolve against, so a half-on profile would
    // fail in a way that looks like a product defect.
    expect(serverProfileSettings("verter-native-semantics")).toEqual({
      "verter.analysis.enabled": true,
      "verter.hover.nativeSemantics": true,
    });
  });

  it("makes every profile a total assignment over every key", () => {
    const keys = serverProfileKeys();
    expect(keys.length).toBeGreaterThan(0);
    for (const profile of Object.keys(E2E_SERVER_PROFILES)) {
      const settings = serverProfileSettings(profile as never);
      expect(Object.keys(settings).sort()).toEqual([...keys]);
    }
  });

  it("rejects a partial profile rather than leaking the previous value", () => {
    const partial = E2E_SERVER_PROFILES as Record<string, Record<string, boolean>>;
    const saved = partial["verter-native-semantics"];
    try {
      partial["verter-native-semantics"] = { "verter.hover.nativeSemantics": true };
      expect(() => serverProfileSettings("verter-native-semantics")).toThrow(
        /does not declare "verter\.analysis\.enabled"/,
      );
    } finally {
      partial["verter-native-semantics"] = saved;
    }
  });

  it("fails closed on an unknown profile name", () => {
    expect(isE2eServerProfile("default")).toBe(true);
    expect(isE2eServerProfile("verter-native-semantics")).toBe(true);
    expect(isE2eServerProfile("native")).toBe(false);
    expect(isE2eServerProfile(undefined)).toBe(false);
  });
});
