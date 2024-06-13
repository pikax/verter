import ImportPlugin from "./index.js";

describe("Generic plugin", () => {
  it("walk should be defined", () => {
    expect(ImportPlugin.walk).toBeDefined();
  });
});
