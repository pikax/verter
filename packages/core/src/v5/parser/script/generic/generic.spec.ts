import { parseGeneric } from "./index.js";

describe("parseGeneric", () => {
  it("return null if no generic", () => {
    expect(parseGeneric("")).toBeNull();
  });
  it("should parse generic", () => {
    expect(parseGeneric("T")).toMatchInlineSnapshot(`
      {
        "declaration": "__VERTER__TS__T = any",
        "names": [
          "T",
        ],
        "position": {
          "end": 1,
          "start": 0,
        },
        "sanitisedNames": [
          "__VERTER__TS__T",
        ],
        "source": "T",
      }
    `);
  });
});
