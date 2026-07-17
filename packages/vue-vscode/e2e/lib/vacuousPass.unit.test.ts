/**
 * @ai-generated - Verifies that the live editor runner rejects tests which try
 * to report an inapplicable path as a pass.
 */
import { describe, expect, it } from "vitest";

import { assertNotVacuousPassLog } from "./vacuousPass";

describe("VS Code E2E vacuous-pass guard", () => {
  it.each(["N/A", "pass (N/A for this fixture)", "not applicable - passing"])(
    "rejects %s",
    (message) => {
      expect(() => assertNotVacuousPassLog(message)).toThrow(/vacuous.*refused/i);
    },
  );

  it("allows ordinary diagnostic output", () => {
    expect(() => assertNotVacuousPassLog("provider sync complete")).not.toThrow();
  });
});
