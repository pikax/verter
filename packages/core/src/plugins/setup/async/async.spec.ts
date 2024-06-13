import AsyncPlugin from "./index.js";

describe("Slots plugin", () => {
  it("sanitise value", () => {
    expect(AsyncPlugin).toEqual({
      name: "Async",
      process: expect.any(Function),
    });
  });
  describe("process", () => {
    describe("undefined", () => {
      test("isSetup: false", () => {
        expect(
          // @ts-expect-error not the right type
          AsyncPlugin.process({}, { isSetup: false })
        ).toBeUndefined();
      });
      test("empty node", () => {
        // @ts-expect-error empty node
        expect(AsyncPlugin.process({}, { isSetup: true })).toBeUndefined();
      });
    });

    describe.skip("detect cases", () => {
      // const valid = ["await test()", "await promise", "if(await a) {}"];
    });
  });
});
