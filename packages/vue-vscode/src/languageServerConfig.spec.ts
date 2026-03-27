import { describe, expect, it } from "vitest";
import { shouldRestartLanguageServerForConfigurationChange } from "./languageServerConfig";

function makeEvent(changed: string[]) {
  return {
    affectsConfiguration(section: string) {
      return changed.includes(section);
    },
  };
}

describe("shouldRestartLanguageServerForConfigurationChange", () => {
  it("restarts for other init-only experimental settings", () => {
    expect(
      shouldRestartLanguageServerForConfigurationChange(
        makeEvent(["verter.experimental.conditionalRootNarrowing"]),
      ),
    ).toBe(true);
    expect(
      shouldRestartLanguageServerForConfigurationChange(
        makeEvent(["verter.experimental.strictSlots"]),
      ),
    ).toBe(true);
  });

  it("does not restart for unrelated settings", () => {
    expect(
      shouldRestartLanguageServerForConfigurationChange(makeEvent(["verter.analysis.enabled"])),
    ).toBe(false);
  });
});
