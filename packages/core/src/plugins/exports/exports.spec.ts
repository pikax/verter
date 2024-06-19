import ExportPlugin from "./index.js";

describe("Exports plugin", () => {
  it("walk should be defined", () => {
    expect(ExportPlugin.walk).toBeDefined();
  });
});
